//! Context window tracking and compaction helpers.

use serde_json::Value;

use crate::error::{Error, Result};
use crate::model::SystemLanguageModel;
use crate::options::GenerationOptions;
use crate::session::Session;

/// Conservative fallback context window size for Apple's on-device models.
///
/// Since the 26.4 SDK, the authoritative value is
/// `SystemLanguageModel::context_size()`; pre-27 runtimes report the
/// back-deployed default of 4096 tokens, while macOS/iOS 27 hardware can
/// report larger windows. Prefer [`ContextLimit::for_model`], which queries
/// the model and only falls back to this constant when the build SDK cannot
/// report a size.
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

    /// Creates a configuration from the model-reported context size.
    ///
    /// Falls back to [`DEFAULT_CONTEXT_TOKENS`] only when the build SDK
    /// cannot report a size ([`crate::Error::UnsupportedPlatform`]). Other
    /// failures — such as Private Cloud Compute network or quota errors —
    /// are propagated rather than silently mapped to a fake limit.
    ///
    /// # Errors
    ///
    /// Returns any non-platform error from `context_size()`, or an internal
    /// error if the reported size does not fit in `usize`.
    pub fn for_model<M: crate::model::LanguageModel + ?Sized>(model: &M) -> Result<Self> {
        let max_tokens = match model.context_size() {
            Ok(tokens) => usize::try_from(tokens).map_err(|_| {
                crate::error::Error::InternalError(format!(
                    "Model context size {tokens} does not fit in usize"
                ))
            })?,
            Err(crate::error::Error::UnsupportedPlatform(_)) => DEFAULT_CONTEXT_TOKENS,
            Err(err) => return Err(err),
        };
        Ok(Self {
            max_tokens,
            reserved_response_tokens: 512,
            chars_per_token: 4,
        })
    }

    /// Creates a default configuration for on-device models.
    ///
    /// This uses the conservative [`DEFAULT_CONTEXT_TOKENS`] fallback;
    /// prefer [`ContextLimit::for_model`] when a model handle is available.
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
    /// Estimated utilization of the available budget (0.0 - 1.0+).
    ///
    /// Measured against `available_tokens` (the window minus the reserved
    /// response budget). `over_limit` uses the exact integer counts and is
    /// false at the boundary where utilization is 1.0.
    pub utilization: f32,
    /// Whether the estimate exceeds the available budget.
    pub over_limit: bool,
}

/// Result of compacting a session into a new summarized session.
pub struct CompactedSession {
    /// Newly created session seeded with compacted summary instructions.
    pub session: Session,
    /// Compacted summary generated from the prior transcript.
    pub summary: String,
}

/// Configuration for transcript compaction.
#[derive(Debug, Clone)]
pub struct CompactionConfig {
    /// Estimated tokens per chunk sent to the summarizer.
    pub chunk_tokens: usize,
    /// Maximum tokens allowed for the rolling summary.
    ///
    /// As chunks are processed, the running summary can grow unbounded.
    /// This limit ensures the summary is truncated to avoid exceeding
    /// the model's context window during multi-chunk compaction.
    pub max_summary_tokens: usize,
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
            max_summary_tokens: 400,
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
    let utilization = if available_tokens > 0 {
        estimated_tokens as f32 / available_tokens as f32
    } else if estimated_tokens > 0 {
        1.0
    } else {
        0.0
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

    for (chunk_index, chunk) in chunks.into_iter().enumerate() {
        let session = Session::with_instructions(model, &config.instructions)?;
        let prompt = build_summary_prompt(
            &summary,
            &chunk,
            config.max_summary_tokens,
            config.chars_per_token,
        );
        let response = session.respond(&prompt, &config.summary_options)?;
        let updated = response.into_content();
        if updated.trim().is_empty() {
            return Err(Error::GenerationError(format!(
                "Summarizer returned an empty response for transcript chunk {}",
                chunk_index + 1
            )));
        }
        summary = updated;
    }

    Ok(summary)
}

/// Compacts a session transcript and creates a new summarized session when needed.
///
/// This helper implements a common context-window rollover pattern:
/// 1. Estimate usage from the current session transcript.
/// 2. If still within budget, return `Ok(None)`.
/// 3. If over budget, summarize the transcript and return a fresh `Session`.
///
/// `base_instructions` are prepended to the generated summary in the compacted session.
pub fn compact_session_if_needed(
    model: &SystemLanguageModel,
    session: &Session,
    limit: &ContextLimit,
    config: &CompactionConfig,
    base_instructions: Option<&str>,
) -> Result<Option<CompactedSession>> {
    let usage = session.context_usage(limit)?;
    if !usage.over_limit {
        return Ok(None);
    }

    let transcript_json = session.transcript_json()?;
    let summary = compact_transcript(model, &transcript_json, config)?;
    let compacted = session_from_summary(model, base_instructions, &summary)?;

    // Report an ineffective compaction rather than letting a caller's
    // compact-until-under-limit loop spin forever.
    let compacted_usage = compacted.context_usage(limit)?;
    if compacted_usage.over_limit {
        return Err(crate::error::Error::InvalidInput(format!(
            "Compacted session still exceeds the context budget \
             ({} estimated tokens > {} available); reduce the base \
             instructions, max_summary_tokens, or summary response limit",
            compacted_usage.estimated_tokens, compacted_usage.available_tokens
        )));
    }

    Ok(Some(CompactedSession {
        session: compacted,
        summary,
    }))
}

/// Creates a new session from optional base instructions and a conversation summary.
pub fn session_from_summary(
    model: &SystemLanguageModel,
    base_instructions: Option<&str>,
    summary: &str,
) -> Result<Session> {
    match compacted_instructions(base_instructions, summary) {
        Some(instructions) => Session::with_instructions(model, &instructions),
        None => Session::new(model),
    }
}

/// Builds instructions text for a compacted session.
pub fn compacted_instructions(base_instructions: Option<&str>, summary: &str) -> Option<String> {
    let base = base_instructions.map_or("", str::trim);
    let summary = summary.trim();

    match (base.is_empty(), summary.is_empty()) {
        (true, true) => None,
        (false, true) => Some(base.to_string()),
        (true, false) => Some(format!("Conversation summary:\n{summary}")),
        (false, false) => Some(format!("{base}\n\nConversation summary:\n{summary}")),
    }
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

fn build_summary_prompt(
    current_summary: &str,
    chunk: &str,
    max_summary_tokens: usize,
    chars_per_token: usize,
) -> String {
    if current_summary.trim().is_empty() {
        format!(
            "Summarize the following conversation transcript:\n\n{chunk}\n\nReturn a concise summary."
        )
    } else {
        // Truncate summary if it exceeds the token limit to prevent unbounded growth
        let summary_tokens = estimate_tokens(current_summary, chars_per_token);
        let truncated_summary = if summary_tokens > max_summary_tokens {
            // Keep the end of the summary to preserve recent context
            let max_chars = max_summary_tokens.saturating_mul(chars_per_token.max(1));
            let char_count = current_summary.chars().count();
            if char_count > max_chars {
                let skip = char_count - max_chars;
                format!(
                    "..{}",
                    current_summary.chars().skip(skip).collect::<String>()
                )
            } else {
                current_summary.to_string()
            }
        } else {
            current_summary.to_string()
        };

        format!(
            "Update the summary with new conversation content.\n\nCurrent summary:\n{truncated_summary}\n\nNew transcript chunk:\n{chunk}\n\nReturn the updated concise summary."
        )
    }
}

fn chunk_text(text: &str, chunk_tokens: usize, chars_per_token: usize) -> Vec<String> {
    let max_chars = chunk_tokens.max(1).saturating_mul(chars_per_token.max(1));
    let mut chunks = Vec::new();
    let mut current = String::new();

    for line in text.lines() {
        // A single line can exceed the chunk budget (transcript_to_text emits
        // one line per message); split it so no chunk overflows the
        // summarizer's context window.
        for piece in split_line(line, max_chars) {
            let piece_len = piece.chars().count() + 1;
            if !current.is_empty() && current.chars().count() + piece_len > max_chars {
                chunks.push(current.trim_end().to_string());
                current.clear();
            }
            current.push_str(piece);
            current.push('\n');
        }
    }

    if !current.trim().is_empty() {
        chunks.push(current.trim_end().to_string());
    }

    if chunks.is_empty() {
        chunks.push(text.to_string());
    }

    chunks
}

/// Splits a line into pieces of at most `max_chars` characters, in one pass.
fn split_line(line: &str, max_chars: usize) -> Vec<&str> {
    let max_chars = max_chars.max(1);
    let mut pieces = Vec::new();
    let mut piece_start = 0;
    let mut piece_chars = 0;

    for (byte_index, _) in line.char_indices() {
        if piece_chars == max_chars {
            pieces.push(&line[piece_start..byte_index]);
            piece_start = byte_index;
            piece_chars = 0;
        }
        piece_chars += 1;
    }

    pieces.push(&line[piece_start..]);
    pieces
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
    use crate::context::{
        ContextLimit, chunk_text, compacted_instructions, context_usage_from_transcript,
        estimate_tokens,
    };

    #[test]
    fn test_estimate_tokens() {
        let text = "abcd";
        assert_eq!(estimate_tokens(text, 4), 1);
        assert_eq!(estimate_tokens(text, 3), 2);
    }

    #[test]
    fn context_usage_at_available_token_boundary_is_not_over_limit() {
        let usage = context_usage_from_transcript(r#"[{"content":"abcd"}]"#, &ContextLimit::new(1))
            .expect("context usage should be estimated");

        assert!((usage.utilization - 1.0).abs() < f32::EPSILON);
        assert!(!usage.over_limit);
    }

    #[test]
    fn test_chunk_text() {
        let text = "Line one\nLine two\nLine three";
        let chunks = chunk_text(text, 2, 4);
        assert!(!chunks.is_empty());
    }

    #[test]
    fn test_chunk_text_splits_single_line_exceeding_budget() {
        let long_line = "x".repeat(100);
        let chunks = chunk_text(&long_line, 2, 4);
        assert!(chunks.len() > 1);
        assert!(chunks.iter().all(|chunk| chunk.chars().count() <= 8));
        let total: usize = chunks.iter().map(|chunk| chunk.chars().count()).sum();
        assert_eq!(total, 100);
    }

    #[test]
    fn test_compacted_instructions() {
        assert_eq!(compacted_instructions(None, ""), None);
        assert_eq!(
            compacted_instructions(Some("You are helpful."), ""),
            Some("You are helpful.".to_string())
        );
        assert_eq!(
            compacted_instructions(None, "Summary body"),
            Some("Conversation summary:\nSummary body".to_string())
        );
        assert_eq!(
            compacted_instructions(Some("You are helpful."), "Summary body"),
            Some("You are helpful.\n\nConversation summary:\nSummary body".to_string())
        );
    }
}
