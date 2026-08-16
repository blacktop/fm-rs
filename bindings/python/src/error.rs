//! Python exception hierarchy mapping fm-rs errors.

use pyo3::exceptions::{PyException, PyRuntimeError, PyTimeoutError, PyValueError};
use pyo3::prelude::*;
use pyo3::{PyErr, create_exception};

// Base exception for all fm errors
create_exception!(
    fm,
    FmError,
    PyException,
    "Base exception for FoundationModels errors."
);

// Specific exception types
create_exception!(
    fm,
    ModelNotAvailableError,
    FmError,
    "Model is not available on this device."
);

create_exception!(
    fm,
    DeviceNotEligibleError,
    FmError,
    "Device is not eligible for Apple Intelligence."
);

create_exception!(
    fm,
    AppleIntelligenceNotEnabledError,
    FmError,
    "Apple Intelligence is not enabled in system settings."
);

create_exception!(
    fm,
    ModelNotReadyError,
    FmError,
    "Model is not ready (downloading or other system reasons)."
);

create_exception!(
    fm,
    GenerationError,
    FmError,
    "Error during model generation."
);

create_exception!(
    fm,
    UnsupportedPlatformError,
    FmError,
    "The requested API requires a newer Apple platform or SDK."
);

create_exception!(
    fm,
    NetworkFailureError,
    FmError,
    "A Private Cloud Compute request failed due to a network error."
);

create_exception!(
    fm,
    QuotaLimitReachedError,
    FmError,
    "The Private Cloud Compute quota is exhausted until it resets."
);

create_exception!(
    fm,
    ServiceUnavailableError,
    FmError,
    "Private Cloud Compute is temporarily unable to serve requests."
);

create_exception!(
    fm,
    ContextSizeExceededError,
    FmError,
    "The request exceeded the model's context window."
);

create_exception!(
    fm,
    RateLimitedError,
    FmError,
    "The request was rate limited; retry later."
);

create_exception!(
    fm,
    GuardrailViolationError,
    FmError,
    "The request or response was blocked by a safety guardrail."
);

create_exception!(
    fm,
    RefusalError,
    FmError,
    "The model declined to answer the request."
);

create_exception!(
    fm,
    UnsupportedCapabilityError,
    FmError,
    "The request uses a capability this model does not support."
);

create_exception!(
    fm,
    UnsupportedTranscriptContentError,
    FmError,
    "The transcript contains content this model cannot process."
);

create_exception!(
    fm,
    UnsupportedGenerationGuideError,
    FmError,
    "The generation guide or schema is not supported by this model."
);

create_exception!(
    fm,
    UnsupportedLanguageOrLocaleError,
    FmError,
    "The request language or locale is not supported by this model."
);

create_exception!(
    fm,
    AssetsUnavailableError,
    FmError,
    "The on-device model assets are unavailable (e.g. not yet downloaded)."
);

create_exception!(
    fm,
    ConcurrentRequestsError,
    FmError,
    "The session is already responding to another request."
);

create_exception!(fm, ToolCallError, FmError, "Error during tool invocation.");

create_exception!(
    fm,
    JsonError,
    FmError,
    "JSON serialization/deserialization error."
);

/// Converts an fm-rs Error to a Python exception.
pub fn to_py_err(err: fm_rs::Error) -> PyErr {
    match err {
        fm_rs::Error::ModelNotAvailable => ModelNotAvailableError::new_err(err.to_string()),
        fm_rs::Error::DeviceNotEligible => DeviceNotEligibleError::new_err(err.to_string()),
        fm_rs::Error::AppleIntelligenceNotEnabled => {
            AppleIntelligenceNotEnabledError::new_err(err.to_string())
        }
        fm_rs::Error::ModelNotReady => ModelNotReadyError::new_err(err.to_string()),
        fm_rs::Error::InvalidInput(msg) => PyValueError::new_err(msg),
        // Preserve the documented Python contract: cancellation is a
        // generation failure and remains catchable as GenerationError.
        fm_rs::Error::GenerationError(msg) | fm_rs::Error::Cancelled(msg) => {
            GenerationError::new_err(msg)
        }
        fm_rs::Error::Timeout(msg) => PyTimeoutError::new_err(msg),
        fm_rs::Error::UnsupportedPlatform(msg) => UnsupportedPlatformError::new_err(msg),
        fm_rs::Error::NetworkFailure(msg) => NetworkFailureError::new_err(msg),
        fm_rs::Error::QuotaLimitReached(msg) => QuotaLimitReachedError::new_err(msg),
        fm_rs::Error::ServiceUnavailable(msg) => ServiceUnavailableError::new_err(msg),
        fm_rs::Error::ContextSizeExceeded(msg) => ContextSizeExceededError::new_err(msg),
        fm_rs::Error::RateLimited(msg) => RateLimitedError::new_err(msg),
        fm_rs::Error::GuardrailViolation(msg) => GuardrailViolationError::new_err(msg),
        fm_rs::Error::Refusal(msg) => RefusalError::new_err(msg),
        fm_rs::Error::UnsupportedCapability(msg) => UnsupportedCapabilityError::new_err(msg),
        fm_rs::Error::UnsupportedTranscriptContent(msg) => {
            UnsupportedTranscriptContentError::new_err(msg)
        }
        fm_rs::Error::UnsupportedGenerationGuide(msg) => {
            UnsupportedGenerationGuideError::new_err(msg)
        }
        fm_rs::Error::UnsupportedLanguageOrLocale(msg) => {
            UnsupportedLanguageOrLocaleError::new_err(msg)
        }
        fm_rs::Error::AssetsUnavailable(msg) => AssetsUnavailableError::new_err(msg),
        fm_rs::Error::ConcurrentRequests(msg) => ConcurrentRequestsError::new_err(msg),
        fm_rs::Error::ToolCall(tool_err) => {
            // Include tool context in the error message
            let msg = format!(
                "Tool '{}' failed with arguments {}: {}",
                tool_err.tool_name, tool_err.arguments, tool_err.inner_error
            );
            ToolCallError::new_err(msg)
        }
        fm_rs::Error::InternalError(msg) => PyRuntimeError::new_err(msg),
        fm_rs::Error::PoisonError => PyRuntimeError::new_err("A lock was poisoned"),
        fm_rs::Error::Json(msg) => JsonError::new_err(msg),
        _ => FmError::new_err(err.to_string()),
    }
}

/// Registers the exception types in the module.
pub fn register(parent: &Bound<'_, PyModule>) -> PyResult<()> {
    parent.add("FmError", parent.py().get_type::<FmError>())?;
    parent.add(
        "ModelNotAvailableError",
        parent.py().get_type::<ModelNotAvailableError>(),
    )?;
    parent.add(
        "DeviceNotEligibleError",
        parent.py().get_type::<DeviceNotEligibleError>(),
    )?;
    parent.add(
        "AppleIntelligenceNotEnabledError",
        parent.py().get_type::<AppleIntelligenceNotEnabledError>(),
    )?;
    parent.add(
        "ModelNotReadyError",
        parent.py().get_type::<ModelNotReadyError>(),
    )?;
    parent.add("GenerationError", parent.py().get_type::<GenerationError>())?;
    parent.add("ToolCallError", parent.py().get_type::<ToolCallError>())?;
    parent.add("JsonError", parent.py().get_type::<JsonError>())?;
    parent.add(
        "UnsupportedPlatformError",
        parent.py().get_type::<UnsupportedPlatformError>(),
    )?;
    parent.add(
        "NetworkFailureError",
        parent.py().get_type::<NetworkFailureError>(),
    )?;
    parent.add(
        "QuotaLimitReachedError",
        parent.py().get_type::<QuotaLimitReachedError>(),
    )?;
    parent.add(
        "ServiceUnavailableError",
        parent.py().get_type::<ServiceUnavailableError>(),
    )?;
    parent.add(
        "ContextSizeExceededError",
        parent.py().get_type::<ContextSizeExceededError>(),
    )?;
    parent.add(
        "RateLimitedError",
        parent.py().get_type::<RateLimitedError>(),
    )?;
    parent.add(
        "GuardrailViolationError",
        parent.py().get_type::<GuardrailViolationError>(),
    )?;
    parent.add("RefusalError", parent.py().get_type::<RefusalError>())?;
    parent.add(
        "UnsupportedCapabilityError",
        parent.py().get_type::<UnsupportedCapabilityError>(),
    )?;
    parent.add(
        "UnsupportedTranscriptContentError",
        parent.py().get_type::<UnsupportedTranscriptContentError>(),
    )?;
    parent.add(
        "UnsupportedGenerationGuideError",
        parent.py().get_type::<UnsupportedGenerationGuideError>(),
    )?;
    parent.add(
        "UnsupportedLanguageOrLocaleError",
        parent.py().get_type::<UnsupportedLanguageOrLocaleError>(),
    )?;
    parent.add(
        "AssetsUnavailableError",
        parent.py().get_type::<AssetsUnavailableError>(),
    )?;
    parent.add(
        "ConcurrentRequestsError",
        parent.py().get_type::<ConcurrentRequestsError>(),
    )?;
    Ok(())
}
