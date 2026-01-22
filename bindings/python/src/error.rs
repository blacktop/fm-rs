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
        fm_rs::Error::GenerationError(msg) => GenerationError::new_err(msg),
        fm_rs::Error::Timeout(msg) => PyTimeoutError::new_err(msg),
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
    Ok(())
}
