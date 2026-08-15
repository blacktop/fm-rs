//! Error types for `FoundationModels` operations.

use std::ffi::NulError;
use std::fmt;
use std::sync::PoisonError as StdPoisonError;

/// Result type for `FoundationModels` operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Error types for `FoundationModels` operations.
#[non_exhaustive]
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

    /// Private Cloud Compute isn't ready to serve requests.
    PrivateCloudComputeSystemNotReady,

    /// Invalid input provided (e.g., string contains null bytes).
    InvalidInput(String),

    /// Error during generation.
    GenerationError(String),

    /// The operation was cancelled (e.g. via [`crate::CancellationHandle`]).
    Cancelled(String),

    /// Operation timed out.
    Timeout(String),

    /// The requested API requires a newer Apple platform or SDK.
    UnsupportedPlatform(String),

    /// A Private Cloud Compute request failed due to a network error.
    NetworkFailure(String),

    /// The user's Private Cloud Compute quota is exhausted until it resets.
    QuotaLimitReached(String),

    /// Private Cloud Compute is temporarily unable to serve requests.
    ServiceUnavailable(String),

    /// The request exceeded the model's context window.
    ContextSizeExceeded(String),

    /// The request was rate limited; retry later.
    RateLimited(String),

    /// The request or response was blocked by a safety guardrail.
    GuardrailViolation(String),

    /// The model declined to answer the request.
    Refusal(String),

    /// The request uses a capability this model does not support.
    UnsupportedCapability(String),

    /// The transcript contains content this model cannot process.
    UnsupportedTranscriptContent(String),

    /// The generation guide or schema is not supported by this model.
    UnsupportedGenerationGuide(String),

    /// The request language or locale is not supported by this model.
    UnsupportedLanguageOrLocale(String),

    /// The on-device model assets are unavailable (e.g. not yet downloaded).
    AssetsUnavailable(String),

    /// The session is already responding to another request.
    ConcurrentRequests(String),

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
            Error::PrivateCloudComputeSystemNotReady => {
                write!(f, "Private Cloud Compute is not ready to serve requests")
            }
            Error::InvalidInput(msg) => write!(f, "Invalid input: {msg}"),
            Error::GenerationError(msg) => write!(f, "Generation error: {msg}"),
            Error::Cancelled(msg) => write!(f, "Operation cancelled: {msg}"),
            Error::Timeout(msg) => write!(f, "Operation timed out: {msg}"),
            Error::UnsupportedPlatform(msg) => write!(f, "Unsupported platform: {msg}"),
            Error::NetworkFailure(msg) => write!(f, "Network failure: {msg}"),
            Error::QuotaLimitReached(msg) => write!(f, "Quota limit reached: {msg}"),
            Error::ServiceUnavailable(msg) => write!(f, "Service unavailable: {msg}"),
            Error::ContextSizeExceeded(msg) => write!(f, "Context size exceeded: {msg}"),
            Error::RateLimited(msg) => write!(f, "Rate limited: {msg}"),
            Error::GuardrailViolation(msg) => write!(f, "Guardrail violation: {msg}"),
            Error::Refusal(msg) => write!(f, "Model refused the request: {msg}"),
            Error::UnsupportedCapability(msg) => write!(f, "Unsupported capability: {msg}"),
            Error::UnsupportedTranscriptContent(msg) => {
                write!(f, "Unsupported transcript content: {msg}")
            }
            Error::UnsupportedGenerationGuide(msg) => {
                write!(f, "Unsupported generation guide: {msg}")
            }
            Error::UnsupportedLanguageOrLocale(msg) => {
                write!(f, "Unsupported language or locale: {msg}")
            }
            Error::AssetsUnavailable(msg) => write!(f, "Model assets unavailable: {msg}"),
            Error::ConcurrentRequests(msg) => {
                write!(f, "Session is already responding: {msg}")
            }
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

#[cfg(test)]
mod tests {
    use crate::error::Error;

    #[test]
    fn unsupported_platform_preserves_requirement() {
        let error = Error::UnsupportedPlatform("requires macOS 27.0".to_string());
        assert_eq!(
            error.to_string(),
            "Unsupported platform: requires macOS 27.0"
        );
    }

    #[test]
    fn pcc_errors_preserve_details() {
        let error = Error::NetworkFailure("connection lost".to_string());
        assert_eq!(error.to_string(), "Network failure: connection lost");

        let error = Error::QuotaLimitReached("resets 2026-07-21".to_string());
        assert_eq!(error.to_string(), "Quota limit reached: resets 2026-07-21");

        let error = Error::ServiceUnavailable("try again later".to_string());
        assert_eq!(error.to_string(), "Service unavailable: try again later");
    }
}
