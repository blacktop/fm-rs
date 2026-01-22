//! Context window tracking wrappers.

use pyo3::prelude::*;

use crate::error::to_py_err;
use crate::model::SystemLanguageModel;

/// Default context window size for Apple's on-device Foundation Models.
pub const DEFAULT_CONTEXT_TOKENS: usize = fm_rs::DEFAULT_CONTEXT_TOKENS;

/// Configuration for estimating context usage.
#[pyclass(module = "fm")]
#[derive(Debug, Clone)]
pub struct ContextLimit {
    max_tokens: usize,
    reserved_response_tokens: usize,
    chars_per_token: usize,
}

#[pymethods]
impl ContextLimit {
    /// Creates a new context limit with a max token budget.
    ///
    /// Args:
    ///     `max_tokens`: Maximum tokens available in the session context window.
    ///     `reserved_response_tokens`: Tokens reserved for the model's next response (default: 0).
    ///     `chars_per_token`: Estimated characters per token (default: 4, English ~3-4, CJK ~1).
    #[new]
    #[pyo3(signature = (max_tokens, *, reserved_response_tokens=0, chars_per_token=4))]
    fn new(max_tokens: usize, reserved_response_tokens: usize, chars_per_token: usize) -> Self {
        Self {
            max_tokens,
            reserved_response_tokens,
            chars_per_token: chars_per_token.max(1),
        }
    }

    /// Creates a default configuration for on-device models.
    ///
    /// Returns:
    ///     `ContextLimit`: Default limit with 4096 tokens, 512 reserved.
    #[staticmethod]
    fn default_on_device() -> Self {
        let inner = fm_rs::ContextLimit::default_on_device();
        Self {
            max_tokens: inner.max_tokens,
            reserved_response_tokens: inner.reserved_response_tokens,
            chars_per_token: inner.chars_per_token,
        }
    }

    /// Maximum tokens available in the session context window.
    #[getter]
    fn max_tokens(&self) -> usize {
        self.max_tokens
    }

    /// Tokens reserved for the model's next response.
    #[getter]
    fn reserved_response_tokens(&self) -> usize {
        self.reserved_response_tokens
    }

    /// Estimated characters per token.
    #[getter]
    fn chars_per_token(&self) -> usize {
        self.chars_per_token
    }

    fn __repr__(&self) -> String {
        format!(
            "ContextLimit(max_tokens={}, reserved_response_tokens={}, chars_per_token={})",
            self.max_tokens, self.reserved_response_tokens, self.chars_per_token
        )
    }
}

impl ContextLimit {
    /// Converts to the inner fm-rs `ContextLimit`.
    pub fn to_inner(&self) -> fm_rs::ContextLimit {
        fm_rs::ContextLimit {
            max_tokens: self.max_tokens,
            reserved_response_tokens: self.reserved_response_tokens,
            chars_per_token: self.chars_per_token,
        }
    }
}

/// Estimated context usage for a session.
#[pyclass(module = "fm")]
#[derive(Debug, Clone)]
pub struct ContextUsage {
    /// Estimated number of tokens consumed by the transcript.
    #[pyo3(get)]
    pub estimated_tokens: usize,
    /// Maximum tokens configured for the session.
    #[pyo3(get)]
    pub max_tokens: usize,
    /// Tokens reserved for the next response.
    #[pyo3(get)]
    pub reserved_response_tokens: usize,
    /// Estimated tokens available for prompts before hitting the limit.
    #[pyo3(get)]
    pub available_tokens: usize,
    /// Estimated utilization ratio (0.0 - 1.0+).
    #[pyo3(get)]
    pub utilization: f32,
    /// Whether the estimate exceeds the available budget.
    #[pyo3(get)]
    pub over_limit: bool,
}

#[pymethods]
impl ContextUsage {
    fn __repr__(&self) -> String {
        format!(
            "ContextUsage(estimated_tokens={}, max_tokens={}, utilization={:.2}, over_limit={})",
            self.estimated_tokens, self.max_tokens, self.utilization, self.over_limit
        )
    }
}

impl ContextUsage {
    /// Creates from an fm-rs `ContextUsage`.
    pub fn from_inner(inner: fm_rs::ContextUsage) -> Self {
        Self {
            estimated_tokens: inner.estimated_tokens,
            max_tokens: inner.max_tokens,
            reserved_response_tokens: inner.reserved_response_tokens,
            available_tokens: inner.available_tokens,
            utilization: inner.utilization,
            over_limit: inner.over_limit,
        }
    }
}

/// Estimates tokens based on a characters-per-token heuristic.
///
/// Args:
///     text: The text to estimate tokens for.
///     `chars_per_token`: Estimated characters per token (default: 4).
///
/// Returns:
///     int: Estimated number of tokens.
#[pyfunction]
#[pyo3(signature = (text, chars_per_token=4))]
pub fn estimate_tokens(text: &str, chars_per_token: usize) -> usize {
    fm_rs::estimate_tokens(text, chars_per_token.max(1))
}

/// Estimates token usage for a session transcript JSON.
///
/// Args:
///     `transcript_json`: The transcript JSON string.
///     limit: The context limit configuration.
///
/// Returns:
///     `ContextUsage`: The estimated context usage.
#[pyfunction]
pub fn context_usage_from_transcript(
    transcript_json: &str,
    limit: &ContextLimit,
) -> PyResult<ContextUsage> {
    let rust_limit = limit.to_inner();
    let usage =
        fm_rs::context_usage_from_transcript(transcript_json, &rust_limit).map_err(to_py_err)?;
    Ok(ContextUsage::from_inner(usage))
}

/// Extracts readable text from transcript JSON.
///
/// Args:
///     `transcript_json`: The transcript JSON string.
///
/// Returns:
///     str: The extracted text.
#[pyfunction]
pub fn transcript_to_text(transcript_json: &str) -> PyResult<String> {
    fm_rs::transcript_to_text(transcript_json).map_err(to_py_err)
}

/// Compacts a transcript into a summary using the on-device model.
///
/// Args:
///     model: The `SystemLanguageModel` to use.
///     `transcript_json`: The transcript JSON string to compact.
///     `chunk_tokens`: Estimated tokens per chunk (default: 800).
///     `max_summary_tokens`: Maximum tokens for the summary (default: 400).
///
/// Returns:
///     str: The compacted summary.
#[pyfunction]
#[pyo3(signature = (model, transcript_json, *, chunk_tokens=800, max_summary_tokens=400))]
pub fn compact_transcript(
    model: &SystemLanguageModel,
    transcript_json: &str,
    chunk_tokens: usize,
    max_summary_tokens: usize,
) -> PyResult<String> {
    let config = fm_rs::CompactionConfig {
        chunk_tokens,
        max_summary_tokens,
        ..fm_rs::CompactionConfig::default()
    };
    fm_rs::compact_transcript(model.inner(), transcript_json, &config).map_err(to_py_err)
}
