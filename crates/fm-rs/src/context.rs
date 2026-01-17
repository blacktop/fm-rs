//! Context window tracking and compaction helpers.

use serde_json::Value;

use crate::error::Result;
use crate::model::SystemLanguageModel;
use crate::options::GenerationOptions;
use crate::session::Session;

/// Default context window size for Apple's on-device Foundation Models.
///
/// This value is based on observed behavior during WWDC 2025 sessions and early
/// developer testing. Apple has not officially documented the context window size.
/// The actual limit may vary by device, model version, or available memory.
///
/// For production use, monitor [`ContextUsage::utilization`] and implement
/// compaction strategies when approaching the limit.
pub const DEFAULT_CONTEXT_TOKENS: usize = 4096;

/// Configuration for estimating context usage.
#[derive(Debug, Clone, Copy)]
pub struct ContextLimit {
    /// Maximum tokens available in the session context window.
    pub max_tokens: usize,
    /// Tokens reserved for the model's next response.
    pub reserved_response_tokens: usize,
    /// Estimated characters per token (English ~3-4, CJK ~1).
    pub chars_per_token: usize,
}

impl ContextLimit {
    /// Creates a new context limit with a max token budget.
    pub fn new(max_tokens: usize) -> Self {
        Self {
            max_tokens,
            reserved_response_tokens: 0,
            chars_per_token: 4,
        }
    }

    /// Creates a default configuration for on-device models.
    pub fn default_on_device() -> Self {
        Self {
            max_tokens: DEFAULT_CONTEXT_TOKENS,
            reserved_response_tokens: 512,
            chars_per_token: 4,
        }
    }

    /// Sets the reserved response tokens.
    pub fn with_reserved_response_tokens(mut self, tokens: usize) -> Self {
        self.reserved_response_tokens = tokens;
        self
    }

    /// Sets the character-per-token estimate.
    pub fn with_chars_per_token(mut self, chars: usize) -> Self {
        if chars > 0 {
            self.chars_per_token = chars;
        }
        self
    }
}

/// Estimated context usage for a session.
#[derive(Debug, Clone, Copy)]
pub struct ContextUsage {
    /// Estimated number of tokens consumed by the transcript.
    pub estimated_tokens: usize,
    /// Maximum tokens configured for the session.
    pub max_tokens: usize,
    /// Tokens reserved for the next response.
    pub reserved_response_tokens: usize,
    /// Estimated tokens available for prompts before hitting the limit.
    pub available_tokens: usize,
    /// Estimated utilization ratio (0.0 - 1.0+).
    pub utilization: f32,
    /// Whether the estimate exceeds the available budget.
    pub over_limit: bool,
}

/// Configuration for transcript compaction.
#[derive(Debug, Clone)]
pub struct CompactionConfig {
    /// Estimated tokens per chunk sent to the summarizer.
    pub chunk_tokens: usize,
    /// Instructions for the summarizer session.
    pub instructions: String,
    /// Options used for summary generation.
    pub summary_options: GenerationOptions,
    /// Estimated characters per token.
    pub chars_per_token: usize,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self {
            chunk_tokens: 800,
            instructions: "Summarize the conversation for future context. Preserve user intent, key facts, decisions, and open questions. Keep the summary concise."
                .to_string(),
            summary_options: GenerationOptions::builder()
                .temperature(0.2)
                .max_response_tokens(256)
                .build(),
            chars_per_token: 4,
        }
    }
}

/// Estimates token usage for the session transcript JSON.
pub fn context_usage_from_transcript(
    transcript_json: &str,
    limit: &ContextLimit,
) -> Result<ContextUsage> {
    let transcript_text = transcript_to_text(transcript_json)?;
    let estimated_tokens = estimate_tokens(&transcript_text, limit.chars_per_token);
    let available_tokens = limit
        .max_tokens
        .saturating_sub(limit.reserved_response_tokens);
    let utilization = if limit.max_tokens == 0 {
        0.0
    } else {
        estimated_tokens as f32 / limit.max_tokens as f32
    };
    let over_limit = estimated_tokens > available_tokens;

    Ok(ContextUsage {
        estimated_tokens,
        max_tokens: limit.max_tokens,
        reserved_response_tokens: limit.reserved_response_tokens,
        available_tokens,
        utilization,
        over_limit,
    })
}

/// Compacts a transcript into a summary using the on-device model.
pub fn compact_transcript(
    model: &SystemLanguageModel,
    transcript_json: &str,
    config: &CompactionConfig,
) -> Result<String> {
    let transcript_text = transcript_to_text(transcript_json)?;
    if transcript_text.trim().is_empty() {
        return Ok(String::new());
    }

    let chunks = chunk_text(
        &transcript_text,
        config.chunk_tokens,
        config.chars_per_token,
    );

    let mut summary = String::new();

    for chunk in chunks {
        let session = Session::with_instructions(model, &config.instructions)?;
        let prompt = build_summary_prompt(&summary, &chunk);
        let response = session.respond(&prompt, &config.summary_options)?;
        summary = response.into_content();
    }

    Ok(summary)
}

/// Extracts readable text from transcript JSON.
pub fn transcript_to_text(transcript_json: &str) -> Result<String> {
    let value: Value = serde_json::from_str(transcript_json)?;
    let mut lines = Vec::new();
    collect_transcript_lines(&value, &mut lines);

    if lines.is_empty() {
        Ok(transcript_json.to_string())
    } else {
        Ok(lines.join("\n"))
    }
}

/// Estimates tokens based on a characters-per-token heuristic.
pub fn estimate_tokens(text: &str, chars_per_token: usize) -> usize {
    let denom = chars_per_token.max(1);
    let chars = text.chars().count();
    chars.div_ceil(denom)
}

fn build_summary_prompt(current_summary: &str, chunk: &str) -> String {
    if current_summary.trim().is_empty() {
        format!(
            "Summarize the following conversation transcript:\n\n{chunk}\n\nReturn a concise summary."
        )
    } else {
        format!(
            "Update the summary with new conversation content.\n\nCurrent summary:\n{current_summary}\n\nNew transcript chunk:\n{chunk}\n\nReturn the updated concise summary."
        )
    }
}

fn chunk_text(text: &str, chunk_tokens: usize, chars_per_token: usize) -> Vec<String> {
    let max_chars = chunk_tokens.max(1).saturating_mul(chars_per_token.max(1));
    let mut chunks = Vec::new();
    let mut current = String::new();

    for line in text.lines() {
        let line_len = line.chars().count() + 1;
        if !current.is_empty() && current.chars().count() + line_len > max_chars {
            chunks.push(current.trim_end().to_string());
            current.clear();
        }
        current.push_str(line);
        current.push('\n');
    }

    if !current.trim().is_empty() {
        chunks.push(current.trim_end().to_string());
    }

    if chunks.is_empty() {
        chunks.push(text.to_string());
    }

    chunks
}

fn collect_transcript_lines(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Array(items) => {
            for item in items {
                collect_transcript_lines(item, out);
            }
        }
        Value::Object(map) => {
            // Track which keys we've already processed to avoid double-counting
            let mut processed_content = false;

            // If this is a message with role+content, add as "{role}: {content}"
            if let Some(role) = map.get("role").and_then(Value::as_str) {
                let content = map
                    .get("content")
                    .and_then(Value::as_str)
                    .or_else(|| map.get("text").and_then(Value::as_str));
                if let Some(content) = content {
                    out.push(format!("{role}: {content}"));
                    processed_content = true;
                }
            }

            // Add standalone text fields, skipping content/text if already included above
            for key in ["content", "text", "prompt", "response", "instructions"] {
                if processed_content && matches!(key, "content" | "text") {
                    continue;
                }
                if let Some(text) = map.get(key).and_then(Value::as_str) {
                    out.push(text.to_string());
                }
            }

            // Recurse into other fields
            for (key, value) in map {
                if matches!(
                    key.as_str(),
                    "role" | "content" | "text" | "prompt" | "response" | "instructions"
                ) {
                    continue;
                }
                collect_transcript_lines(value, out);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_tokens() {
        let text = "abcd";
        assert_eq!(estimate_tokens(text, 4), 1);
        assert_eq!(estimate_tokens(text, 3), 2);
    }

    #[test]
    fn test_chunk_text() {
        let text = "Line one\nLine two\nLine three";
        let chunks = chunk_text(text, 2, 4);
        assert!(!chunks.is_empty());
    }
}
