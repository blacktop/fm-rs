//! `SystemLanguageModel` and `ModelAvailability` types.

use std::ffi::CStr;
use std::ptr::{self, NonNull};

use crate::error::{Error, Result};
use crate::ffi::{self, AvailabilityCode, SwiftPtr};

/// Represents the availability status of a `FoundationModel`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelAvailability {
    /// Model is available and ready to use.
    Available,
    /// Device is not eligible for Apple Intelligence.
    DeviceNotEligible,
    /// Apple Intelligence is not enabled in system settings.
    AppleIntelligenceNotEnabled,
    /// Model is not ready (downloading or other system reasons).
    ModelNotReady,
    /// Unavailability for an unknown reason.
    Unknown,
}

impl ModelAvailability {
    /// Returns an error describing why the model is unavailable.
    pub fn into_error(self) -> Option<Error> {
        match self {
            ModelAvailability::Available => None,
            ModelAvailability::DeviceNotEligible => Some(Error::DeviceNotEligible),
            ModelAvailability::AppleIntelligenceNotEnabled => {
                Some(Error::AppleIntelligenceNotEnabled)
            }
            ModelAvailability::ModelNotReady => Some(Error::ModelNotReady),
            ModelAvailability::Unknown => Some(Error::ModelNotAvailable),
        }
    }
}

impl From<AvailabilityCode> for ModelAvailability {
    fn from(code: AvailabilityCode) -> Self {
        match code {
            AvailabilityCode::Available => ModelAvailability::Available,
            AvailabilityCode::DeviceNotEligible => ModelAvailability::DeviceNotEligible,
            AvailabilityCode::AppleIntelligenceNotEnabled => {
                ModelAvailability::AppleIntelligenceNotEnabled
            }
            AvailabilityCode::ModelNotReady => ModelAvailability::ModelNotReady,
            AvailabilityCode::Unknown => ModelAvailability::Unknown,
        }
    }
}

/// The system language model provided by Apple Intelligence.
///
/// This is the main entry point for using on-device AI capabilities.
/// Use [`SystemLanguageModel::new()`] to get the default model.
///
/// # Example
///
/// ```rust,no_run
/// use fm_rs::SystemLanguageModel;
///
/// let model = SystemLanguageModel::new()?;
/// if model.is_available() {
///     println!("Model is ready to use!");
/// }
/// # Ok::<(), fm_rs::Error>(())
/// ```
pub struct SystemLanguageModel {
    ptr: NonNull<std::ffi::c_void>,
}

impl SystemLanguageModel {
    /// Creates the default system language model.
    ///
    /// # Errors
    ///
    /// Returns an error if the model cannot be created or if `FoundationModels`
    /// is not available on the device.
    pub fn new() -> Result<Self> {
        let mut error: SwiftPtr = ptr::null_mut();

        let ptr = unsafe { ffi::fm_model_default(&raw mut error) };

        if !error.is_null() {
            return Err(error_from_swift(error));
        }

        NonNull::new(ptr).map(|ptr| Self { ptr }).ok_or_else(|| {
            Error::InternalError(
                "SystemLanguageModel creation returned null without error. \
                 This may indicate FoundationModels.framework is unavailable."
                    .to_string(),
            )
        })
    }

    /// Returns a raw pointer to the underlying Swift object.
    ///
    /// This is used internally for FFI calls.
    pub(crate) fn as_ptr(&self) -> SwiftPtr {
        self.ptr.as_ptr()
    }

    /// Checks if the model is available for use.
    ///
    /// Returns `true` if the model is available and ready to generate responses.
    pub fn is_available(&self) -> bool {
        unsafe { ffi::fm_model_is_available(self.ptr.as_ptr()) }
    }

    /// Gets the current availability status of the model.
    ///
    /// This provides more detailed information about why the model might not be available.
    pub fn availability(&self) -> ModelAvailability {
        let code = unsafe { ffi::fm_model_availability(self.ptr.as_ptr()) };
        AvailabilityCode::from(code).into()
    }

    /// Returns a reason-specific error if the model is unavailable.
    pub fn ensure_available(&self) -> Result<()> {
        match self.availability().into_error() {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }
}

impl std::fmt::Debug for SystemLanguageModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SystemLanguageModel")
            .field("availability", &self.availability())
            .finish()
    }
}

impl Drop for SystemLanguageModel {
    fn drop(&mut self) {
        unsafe {
            ffi::fm_model_free(self.ptr.as_ptr());
        }
    }
}

// SAFETY: SystemLanguageModel is a wrapper around a Swift object that is
// internally thread-safe (uses DispatchQueue for async operations).
unsafe impl Send for SystemLanguageModel {}
unsafe impl Sync for SystemLanguageModel {}

/// Converts a Swift error pointer to a Rust Error.
pub(crate) fn error_from_swift(error: SwiftPtr) -> Error {
    use crate::error::ToolCallError;

    if error.is_null() {
        return Error::InternalError(
            "FFI error object was null; unable to retrieve error details".to_string(),
        );
    }

    let code = unsafe { ffi::fm_error_code(error) };
    let msg_ptr = unsafe { ffi::fm_error_message(error) };

    let message = if msg_ptr.is_null() {
        "Error message unavailable (null pointer from Swift)".to_string()
    } else {
        unsafe { CStr::from_ptr(msg_ptr).to_string_lossy().into_owned() }
    };

    // Extract tool context if this is a tool error
    let tool_name = unsafe {
        let ptr = ffi::fm_error_tool_name(error);
        if ptr.is_null() {
            None
        } else {
            Some(CStr::from_ptr(ptr).to_string_lossy().into_owned())
        }
    };

    let tool_arguments = unsafe {
        let ptr = ffi::fm_error_tool_arguments(error);
        if ptr.is_null() {
            None
        } else {
            let json_str = CStr::from_ptr(ptr).to_string_lossy().into_owned();
            serde_json::from_str(&json_str).ok()
        }
    };

    unsafe {
        ffi::fm_error_free(error);
    }

    match ffi::ErrorCode::from(code) {
        ffi::ErrorCode::ModelNotAvailable => Error::ModelNotAvailable,
        ffi::ErrorCode::GenerationFailed => Error::GenerationError(message),
        ffi::ErrorCode::Cancelled => Error::GenerationError("Operation cancelled".to_string()),
        ffi::ErrorCode::Timeout => Error::Timeout(message),
        ffi::ErrorCode::ToolError => {
            // Construct ToolCallError with context if available
            Error::ToolCall(ToolCallError {
                tool_name: tool_name.unwrap_or_else(|| "unknown".to_string()),
                arguments: tool_arguments.unwrap_or(serde_json::Value::Null),
                inner_error: message,
            })
        }
        ffi::ErrorCode::InvalidInput => Error::InvalidInput(message),
        ffi::ErrorCode::Unknown => Error::InternalError(message),
    }
}
