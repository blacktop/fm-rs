//! Error types for `FoundationModels` operations.

use std::ffi::NulError;
use std::fmt;
use std::sync::PoisonError as StdPoisonError;

/// Result type for `FoundationModels` operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Error types for `FoundationModels` operations.
#[derive(Debug)]
pub enum Error {
    /// Model is not available on this device.
    ModelNotAvailable,

    /// Device is not eligible for Apple Intelligence.
    DeviceNotEligible,

    /// Apple Intelligence is not enabled in system settings.
    AppleIntelligenceNotEnabled,

    /// Model is not ready (downloading or other system reasons).
    ModelNotReady,

    /// Invalid input provided (e.g., string contains null bytes).
    InvalidInput(String),

    /// Error during generation.
    GenerationError(String),

    /// Operation timed out.
    Timeout(String),

    /// Error during tool invocation.
    ToolCall(ToolCallError),

    /// Internal error in the FFI layer.
    InternalError(String),

    /// A lock was poisoned.
    PoisonError,

    /// JSON serialization/deserialization error.
    Json(String),
}

/// Error that occurred during tool invocation.
#[derive(Debug, Clone)]
pub struct ToolCallError {
    /// Name of the tool that failed.
    pub tool_name: String,
    /// Arguments passed to the tool.
    pub arguments: serde_json::Value,
    /// Description of the error.
    pub inner_error: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::ModelNotAvailable => {
                write!(f, "FoundationModels is not available on this device")
            }
            Error::DeviceNotEligible => write!(f, "Device is not eligible for Apple Intelligence"),
            Error::AppleIntelligenceNotEnabled => {
                write!(f, "Apple Intelligence is not enabled in system settings")
            }
            Error::ModelNotReady => {
                write!(
                    f,
                    "Model is not ready (downloading or other system reasons)"
                )
            }
            Error::InvalidInput(msg) => write!(f, "Invalid input: {msg}"),
            Error::GenerationError(msg) => write!(f, "Generation error: {msg}"),
            Error::Timeout(msg) => write!(f, "Operation timed out: {msg}"),
            Error::ToolCall(err) => {
                write!(f, "Tool '{}' failed: {}", err.tool_name, err.inner_error)
            }
            Error::InternalError(msg) => write!(f, "Internal error: {msg}"),
            Error::PoisonError => write!(f, "A lock was poisoned"),
            Error::Json(msg) => write!(f, "JSON error: {msg}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        None
    }
}

impl From<NulError> for Error {
    fn from(_: NulError) -> Self {
        Error::InvalidInput("String contains null byte".to_string())
    }
}

impl<T> From<StdPoisonError<T>> for Error {
    fn from(_: StdPoisonError<T>) -> Self {
        Error::PoisonError
    }
}

impl From<serde_json::Error> for Error {
    fn from(err: serde_json::Error) -> Self {
        Error::Json(err.to_string())
    }
}

impl fmt::Display for ToolCallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Tool '{}' failed with arguments {}: {}",
            self.tool_name, self.arguments, self.inner_error
        )
    }
}

impl std::error::Error for ToolCallError {}
