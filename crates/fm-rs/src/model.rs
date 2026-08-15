//! `SystemLanguageModel`, `TokenUsage`, and `ModelAvailability` types.

use std::ffi::{CStr, CString};
use std::ptr::{self, NonNull};
use std::sync::Arc;
#[cfg(feature = "private-cloud-compute")]
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::error::{Error, Result, ToolCallError};
use crate::ffi::{self, AvailabilityCode, SwiftPtr};
use crate::tool::{Tool, tools_to_json};

const TOKEN_USAGE_UNAVAILABLE_SENTINEL: i64 = -2;
const TOKEN_ESTIMATE_CHARS_PER_TOKEN: usize = 4;

/// Represents the availability status of a `FoundationModel`.
#[non_exhaustive]
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
    /// Private Cloud Compute isn't ready to serve requests.
    PrivateCloudComputeSystemNotReady,
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
            ModelAvailability::PrivateCloudComputeSystemNotReady => {
                Some(Error::PrivateCloudComputeSystemNotReady)
            }
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
            AvailabilityCode::PrivateCloudComputeSystemNotReady => {
                ModelAvailability::PrivateCloudComputeSystemNotReady
            }
            AvailabilityCode::Unknown => ModelAvailability::Unknown,
        }
    }
}

mod private {
    pub trait Sealed {}
}

/// A Foundation Models implementation that can create a [`crate::Session`].
///
/// This sealed abstraction supports both the on-device model and model types
/// introduced by newer Apple SDKs without allowing invalid external FFI
/// implementations.
pub trait LanguageModel: private::Sealed + Send + Sync {
    /// Returns the opaque Swift model box used by the FFI layer.
    #[doc(hidden)]
    fn raw_model_ptr(&self) -> *mut std::ffi::c_void;

    /// Checks whether the model is currently available.
    fn is_available(&self) -> bool {
        unsafe { ffi::fm_model_is_available(self.raw_model_ptr()) }
    }

    /// Returns the current reason-specific model availability.
    fn availability(&self) -> ModelAvailability {
        let code = unsafe { ffi::fm_model_availability(self.raw_model_ptr()) };
        AvailabilityCode::from(code).into()
    }

    /// Returns a reason-specific error if the model is unavailable.
    fn ensure_available(&self) -> Result<()> {
        match self.availability().into_error() {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }

    /// Returns the model context window size in tokens.
    ///
    /// For the on-device model this is reported synchronously (26.4+ build
    /// SDK required); for Private Cloud Compute it performs a network
    /// request and can fail with [`Error::NetworkFailure`] or
    /// [`Error::ServiceUnavailable`].
    fn context_size(&self) -> Result<u64> {
        let mut error: SwiftPtr = ptr::null_mut();
        let size = unsafe { ffi::fm_model_context_size(self.raw_model_ptr(), &raw mut error) };

        if !error.is_null() {
            return Err(error_from_swift(error));
        }

        u64::try_from(size).map_err(|_| {
            Error::InternalError(format!("Context size returned invalid value {size}"))
        })
    }
}

struct ModelHandle {
    ptr: NonNull<std::ffi::c_void>,
}

impl ModelHandle {
    fn from_ffi(ptr: SwiftPtr, error: SwiftPtr, model_name: &str) -> Result<Self> {
        if !error.is_null() {
            return Err(error_from_swift(error));
        }

        NonNull::new(ptr).map(|ptr| Self { ptr }).ok_or_else(|| {
            Error::InternalError(format!(
                "{model_name} creation returned null without an error"
            ))
        })
    }

    fn as_ptr(&self) -> SwiftPtr {
        self.ptr.as_ptr()
    }
}

impl Drop for ModelHandle {
    fn drop(&mut self) {
        unsafe {
            ffi::fm_model_free(self.ptr.as_ptr());
        }
    }
}

// SAFETY: ModelHandle owns an immutable Swift language-model box. Foundation
// Models declares its concrete model implementations Sendable, and all mutable
// session state is stored in separate LanguageModelSession instances.
unsafe impl Send for ModelHandle {}
unsafe impl Sync for ModelHandle {}

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
    handle: ModelHandle,
}

/// Token usage returned by `SystemLanguageModel` 26.4+ APIs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenUsage {
    /// Number of tokens, either measured by the framework tokenizer or
    /// estimated locally (see [`estimated`](Self::estimated)).
    pub token_count: usize,
    /// Whether `token_count` is a local chars-per-token estimate rather than
    /// a framework tokenizer measurement.
    ///
    /// `true` when the build SDK or runtime lacks the token-usage APIs and
    /// the count was derived heuristically (~4 characters per token).
    pub estimated: bool,
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

        ModelHandle::from_ffi(ptr, error, "SystemLanguageModel").map(|handle| Self { handle })
    }

    /// Returns a raw pointer to the underlying Swift object.
    ///
    /// This is used internally for FFI calls.
    pub(crate) fn as_ptr(&self) -> SwiftPtr {
        self.handle.as_ptr()
    }

    /// Checks if the model is available for use.
    ///
    /// Returns `true` if the model is available and ready to generate responses.
    pub fn is_available(&self) -> bool {
        LanguageModel::is_available(self)
    }

    /// Gets the current availability status of the model.
    ///
    /// This provides more detailed information about why the model might not be available.
    pub fn availability(&self) -> ModelAvailability {
        LanguageModel::availability(self)
    }

    /// Returns a reason-specific error if the model is unavailable.
    pub fn ensure_available(&self) -> Result<()> {
        LanguageModel::ensure_available(self)
    }

    /// Returns the on-device context window size in tokens.
    ///
    /// Requires building with the macOS/iOS 26.4 SDK or later; older build
    /// SDKs return [`Error::UnsupportedPlatform`]. On pre-27 runtimes the
    /// system reports the back-deployed default of 4096 tokens.
    pub fn context_size(&self) -> Result<u64> {
        LanguageModel::context_size(self)
    }

    /// Returns token usage for a prompt.
    ///
    /// Uses platform token-usage APIs when available in both the build SDK and runtime.
    /// Otherwise returns a heuristic estimate.
    pub fn token_usage_for(&self, prompt: &str) -> Result<TokenUsage> {
        let prompt_c = CString::new(prompt)?;
        let mut error: SwiftPtr = ptr::null_mut();

        let token_count = unsafe {
            ffi::fm_model_token_usage_for(self.as_ptr(), prompt_c.as_ptr(), &raw mut error)
        };

        if !error.is_null() {
            return Err(error_from_swift(error));
        }

        if token_count == TOKEN_USAGE_UNAVAILABLE_SENTINEL {
            return Ok(TokenUsage {
                token_count: crate::context::estimate_tokens(
                    prompt,
                    TOKEN_ESTIMATE_CHARS_PER_TOKEN,
                ),
                estimated: true,
            });
        }

        token_usage_from_raw(token_count)
    }

    /// Returns token usage for session instructions and tool definitions.
    ///
    /// Tool definitions are serialized from the Rust [`Tool`] trait objects.
    /// Uses platform token-usage APIs when available in both the build SDK and runtime.
    /// Otherwise returns a heuristic estimate.
    pub fn token_usage_for_tools(
        &self,
        instructions: &str,
        tools: &[Arc<dyn Tool>],
    ) -> Result<TokenUsage> {
        let instructions_c = CString::new(instructions)?;
        let tools_json = if tools.is_empty() {
            None
        } else {
            let tool_refs: Vec<&dyn Tool> = tools.iter().map(std::convert::AsRef::as_ref).collect();
            Some(CString::new(tools_to_json(&tool_refs)?)?)
        };
        let tools_ptr = tools_json.as_ref().map_or(ptr::null(), |s| s.as_ptr());

        let mut error: SwiftPtr = ptr::null_mut();
        let token_count = unsafe {
            ffi::fm_model_token_usage_for_tools(
                self.as_ptr(),
                instructions_c.as_ptr(),
                tools_ptr,
                &raw mut error,
            )
        };

        if !error.is_null() {
            return Err(error_from_swift(error));
        }

        if token_count == TOKEN_USAGE_UNAVAILABLE_SENTINEL {
            let fallback =
                crate::context::estimate_tokens(instructions, TOKEN_ESTIMATE_CHARS_PER_TOKEN)
                    + tools_json.as_ref().map_or(0, |json| {
                        crate::context::estimate_tokens(
                            &json.to_string_lossy(),
                            TOKEN_ESTIMATE_CHARS_PER_TOKEN,
                        )
                    });
            return Ok(TokenUsage {
                token_count: fallback,
                estimated: true,
            });
        }

        token_usage_from_raw(token_count)
    }
}

impl private::Sealed for SystemLanguageModel {}

impl LanguageModel for SystemLanguageModel {
    fn raw_model_ptr(&self) -> *mut std::ffi::c_void {
        self.as_ptr()
    }
}

impl std::fmt::Debug for SystemLanguageModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SystemLanguageModel")
            .field("availability", &self.availability())
            .finish()
    }
}

/// Apple Foundation Models running on Private Cloud Compute.
///
/// This model requires a 27.0-or-newer Apple platform, network access, an
/// Apple Intelligence-capable device, and Apple's managed
/// `com.apple.developer.private-cloud-compute` entitlement authorized by the
/// app's provisioning profile. Construction on an older SDK or runtime returns
/// [`Error::UnsupportedPlatform`].
#[cfg(feature = "private-cloud-compute")]
pub struct PrivateCloudComputeLanguageModel {
    handle: ModelHandle,
}

#[cfg(feature = "private-cloud-compute")]
impl PrivateCloudComputeLanguageModel {
    /// Creates the Private Cloud Compute language model.
    pub fn new() -> Result<Self> {
        let mut error: SwiftPtr = ptr::null_mut();
        let ptr = unsafe { ffi::fm_model_private_cloud_compute(&raw mut error) };

        ModelHandle::from_ffi(ptr, error, "PrivateCloudComputeLanguageModel")
            .map(|handle| Self { handle })
    }

    /// Checks whether PCC is currently available.
    pub fn is_available(&self) -> bool {
        LanguageModel::is_available(self)
    }

    /// Returns the current reason-specific PCC availability.
    pub fn availability(&self) -> ModelAvailability {
        LanguageModel::availability(self)
    }

    /// Returns a reason-specific error if PCC is unavailable.
    pub fn ensure_available(&self) -> Result<()> {
        LanguageModel::ensure_available(self)
    }

    /// Returns the user's current Private Cloud Compute quota usage.
    ///
    /// Apple applies a daily, per-user iCloud quota; check this before
    /// sending requests to decide whether to fall back to the on-device
    /// [`crate::SystemLanguageModel`].
    pub fn quota_usage(&self) -> Result<QuotaUsage> {
        let mut error: SwiftPtr = ptr::null_mut();
        let json_ptr = unsafe { ffi::fm_model_pcc_quota_usage(self.as_ptr(), &raw mut error) };

        if !error.is_null() {
            return Err(error_from_swift(error));
        }

        if json_ptr.is_null() {
            return Err(Error::InternalError(
                "Quota usage returned null without an error".to_string(),
            ));
        }

        let json = unsafe {
            let json = CStr::from_ptr(json_ptr).to_string_lossy().into_owned();
            ffi::fm_string_free(json_ptr);
            json
        };

        quota_usage_from_json(&json)
    }

    /// Returns the Private Cloud Compute context window size in tokens.
    ///
    /// This performs a network request and can fail with
    /// [`Error::NetworkFailure`] or [`Error::ServiceUnavailable`].
    pub fn context_size(&self) -> Result<u64> {
        LanguageModel::context_size(self)
    }

    pub(crate) fn as_ptr(&self) -> SwiftPtr {
        self.handle.as_ptr()
    }
}

#[cfg(feature = "private-cloud-compute")]
impl private::Sealed for PrivateCloudComputeLanguageModel {}

#[cfg(feature = "private-cloud-compute")]
impl LanguageModel for PrivateCloudComputeLanguageModel {
    fn raw_model_ptr(&self) -> *mut std::ffi::c_void {
        self.as_ptr()
    }
}

#[cfg(feature = "private-cloud-compute")]
impl std::fmt::Debug for PrivateCloudComputeLanguageModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PrivateCloudComputeLanguageModel")
            .field("availability", &self.availability())
            .finish()
    }
}

/// The user's daily Private Cloud Compute quota usage.
///
/// Returned by [`PrivateCloudComputeLanguageModel::quota_usage`]. Apple does
/// not publish numeric limits; applications react to the reported status.
#[cfg(feature = "private-cloud-compute")]
#[derive(Debug, Clone, PartialEq)]
pub struct QuotaUsage {
    /// The user's position relative to their quota limit.
    pub status: QuotaStatus,
    /// When the quota next resets, if the system reports it.
    pub reset_date: Option<SystemTime>,
    /// Whether the system can show the user a quota-increase suggestion
    /// (for example, upgrading to iCloud+).
    pub has_limit_increase_suggestion: bool,
}

#[cfg(feature = "private-cloud-compute")]
impl QuotaUsage {
    /// Returns true when the quota is exhausted until the reset date.
    pub fn is_limit_reached(&self) -> bool {
        self.status == QuotaStatus::LimitReached
    }
}

/// Position of the user's Private Cloud Compute usage relative to the quota.
#[cfg(feature = "private-cloud-compute")]
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotaStatus {
    /// Requests are being served.
    BelowLimit {
        /// Whether usage is close enough to the limit that the application
        /// should prepare an on-device fallback.
        is_approaching_limit: bool,
    },
    /// The quota is exhausted; requests fail until the quota resets.
    LimitReached,
    /// A quota state this crate version does not recognize.
    Unknown,
}

#[cfg(feature = "private-cloud-compute")]
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct QuotaUsageDto {
    status: String,
    #[serde(default)]
    is_approaching_limit: Option<bool>,
    #[serde(default)]
    reset_date: Option<f64>,
    #[serde(default)]
    has_limit_increase_suggestion: bool,
}

#[cfg(feature = "private-cloud-compute")]
fn quota_usage_from_json(json: &str) -> Result<QuotaUsage> {
    let dto: QuotaUsageDto = serde_json::from_str(json)
        .map_err(|error| Error::InternalError(format!("Invalid quota usage JSON: {error}")))?;

    let status = match dto.status.as_str() {
        "belowLimit" => QuotaStatus::BelowLimit {
            is_approaching_limit: dto.is_approaching_limit.unwrap_or(false),
        },
        "limitReached" => QuotaStatus::LimitReached,
        _ => QuotaStatus::Unknown,
    };

    Ok(QuotaUsage {
        status,
        reset_date: dto.reset_date.and_then(unix_seconds_to_system_time),
        has_limit_increase_suggestion: dto.has_limit_increase_suggestion,
    })
}

#[cfg(feature = "private-cloud-compute")]
fn unix_seconds_to_system_time(seconds: f64) -> Option<SystemTime> {
    if seconds >= 0.0 {
        UNIX_EPOCH.checked_add(Duration::try_from_secs_f64(seconds).ok()?)
    } else {
        UNIX_EPOCH.checked_sub(Duration::try_from_secs_f64(-seconds).ok()?)
    }
}

fn token_usage_from_raw(token_count: i64) -> Result<TokenUsage> {
    if token_count < 0 {
        return Err(Error::InternalError(
            "Token usage API returned a negative token count".to_string(),
        ));
    }

    let token_count = usize::try_from(token_count)
        .map_err(|_| Error::InternalError("Token usage value does not fit in usize".to_string()))?;

    Ok(TokenUsage {
        token_count,
        estimated: false,
    })
}

/// Copies and frees a Swift-allocated (`strdup`) error string, or returns
/// `None` for a null pointer.
///
/// # Safety
///
/// `ptr` must be null or a `strdup` allocation owned by the caller.
unsafe fn take_error_string(ptr: *mut std::ffi::c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let value = unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned();
    unsafe { ffi::fm_string_free(ptr) };
    Some(value)
}

/// Converts a Swift error pointer to a Rust Error.
pub(crate) fn error_from_swift(error: SwiftPtr) -> Error {
    if error.is_null() {
        return Error::InternalError(
            "FFI error object was null; unable to retrieve error details".to_string(),
        );
    }

    let code = unsafe { ffi::fm_error_code(error) };

    let message = unsafe { take_error_string(ffi::fm_error_message(error)) }
        .unwrap_or_else(|| "Error message unavailable (null pointer from Swift)".to_string());

    // Extract tool context if this is a tool error
    let tool_name = unsafe { take_error_string(ffi::fm_error_tool_name(error)) };

    let tool_arguments = unsafe { take_error_string(ffi::fm_error_tool_arguments(error)) }
        .and_then(|json_str| serde_json::from_str(&json_str).ok());

    unsafe {
        ffi::fm_error_free(error);
    }

    if let ffi::ErrorCode::ToolError = ffi::ErrorCode::from(code) {
        // Construct ToolCallError with context if available
        return Error::ToolCall(ToolCallError {
            tool_name: tool_name.unwrap_or_else(|| "unknown".to_string()),
            arguments: tool_arguments.unwrap_or(serde_json::Value::Null),
            inner_error: message,
        });
    }

    error_from_parts(code, message)
}

/// Maps an FFI error code and message onto a typed [`Error`].
///
/// Shared by the blocking respond paths (via [`error_from_swift`]) and the
/// streaming callbacks, which receive only a code and a message. Streaming
/// tool errors keep the [`Error::ToolCall`] category; the tool name is only
/// available embedded in the message, so the structured field is "unknown".
pub(crate) fn error_from_parts(code: std::ffi::c_int, message: String) -> Error {
    match ffi::ErrorCode::from(code) {
        ffi::ErrorCode::ModelNotAvailable => Error::ModelNotAvailable,
        ffi::ErrorCode::GenerationFailed => Error::GenerationError(message),
        ffi::ErrorCode::Cancelled => Error::Cancelled(message),
        ffi::ErrorCode::ToolError => Error::ToolCall(ToolCallError {
            tool_name: "unknown".to_string(),
            arguments: serde_json::Value::Null,
            inner_error: message,
        }),
        ffi::ErrorCode::Timeout => Error::Timeout(message),
        ffi::ErrorCode::UnsupportedPlatform => Error::UnsupportedPlatform(message),
        ffi::ErrorCode::NetworkFailure => Error::NetworkFailure(message),
        ffi::ErrorCode::QuotaLimitReached => Error::QuotaLimitReached(message),
        ffi::ErrorCode::ServiceUnavailable => Error::ServiceUnavailable(message),
        ffi::ErrorCode::ContextSizeExceeded => Error::ContextSizeExceeded(message),
        ffi::ErrorCode::RateLimited => Error::RateLimited(message),
        ffi::ErrorCode::GuardrailViolation => Error::GuardrailViolation(message),
        ffi::ErrorCode::Refusal => Error::Refusal(message),
        ffi::ErrorCode::UnsupportedCapability => Error::UnsupportedCapability(message),
        ffi::ErrorCode::UnsupportedTranscriptContent => {
            Error::UnsupportedTranscriptContent(message)
        }
        ffi::ErrorCode::UnsupportedGenerationGuide => Error::UnsupportedGenerationGuide(message),
        ffi::ErrorCode::UnsupportedLanguageOrLocale => Error::UnsupportedLanguageOrLocale(message),
        ffi::ErrorCode::AssetsUnavailable => Error::AssetsUnavailable(message),
        ffi::ErrorCode::ConcurrentRequests => Error::ConcurrentRequests(message),
        ffi::ErrorCode::InvalidInput => Error::InvalidInput(message),
        ffi::ErrorCode::Unknown => Error::InternalError(message),
    }
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "private-cloud-compute")]
    use std::time::{Duration, UNIX_EPOCH};

    use crate::error::Error;
    use crate::ffi::{AvailabilityCode, ErrorCode};
    use crate::model::{ModelAvailability, error_from_parts, token_usage_from_raw};
    #[cfg(feature = "private-cloud-compute")]
    use crate::model::{QuotaStatus, quota_usage_from_json, unix_seconds_to_system_time};

    #[cfg(feature = "private-cloud-compute")]
    #[test]
    fn quota_usage_should_parse_below_limit() {
        let usage = quota_usage_from_json(
            r#"{"status":"belowLimit","isApproachingLimit":true,"resetDate":1786000000,"hasLimitIncreaseSuggestion":true}"#,
        )
        .expect("below-limit quota JSON should parse");

        assert_eq!(
            usage.status,
            QuotaStatus::BelowLimit {
                is_approaching_limit: true
            }
        );
        assert!(!usage.is_limit_reached());
        assert_eq!(
            usage.reset_date,
            Some(UNIX_EPOCH + Duration::from_secs(1_786_000_000))
        );
        assert!(usage.has_limit_increase_suggestion);
    }

    #[cfg(feature = "private-cloud-compute")]
    #[test]
    fn quota_usage_should_parse_limit_reached_without_optional_fields() {
        let usage = quota_usage_from_json(r#"{"status":"limitReached"}"#)
            .expect("limit-reached quota JSON should parse");

        assert_eq!(usage.status, QuotaStatus::LimitReached);
        assert!(usage.is_limit_reached());
        assert_eq!(usage.reset_date, None);
        assert!(!usage.has_limit_increase_suggestion);
    }

    #[cfg(feature = "private-cloud-compute")]
    #[test]
    fn quota_usage_should_map_future_statuses_to_unknown() {
        let usage = quota_usage_from_json(r#"{"status":"throttled"}"#)
            .expect("unrecognized quota status should parse");
        assert_eq!(usage.status, QuotaStatus::Unknown);
    }

    #[cfg(feature = "private-cloud-compute")]
    #[test]
    fn quota_usage_should_reject_invalid_json() {
        let err = quota_usage_from_json("not json").expect_err("invalid JSON should fail");
        assert!(matches!(err, Error::InternalError(_)));
    }

    #[cfg(feature = "private-cloud-compute")]
    #[test]
    fn unix_seconds_should_reject_non_finite_values() {
        assert_eq!(unix_seconds_to_system_time(f64::NAN), None);
        assert_eq!(unix_seconds_to_system_time(f64::INFINITY), None);
        assert!(unix_seconds_to_system_time(-1.0).is_some());
    }

    #[test]
    fn token_usage_should_convert_positive_values() {
        let usage = token_usage_from_raw(42).expect("positive token count should convert");
        assert_eq!(usage.token_count, 42);
        assert!(!usage.estimated);
    }

    #[test]
    fn token_usage_should_reject_negative_values() {
        let err = token_usage_from_raw(-1).expect_err("negative token count should fail");
        assert!(err.to_string().contains("negative token count"));
    }

    #[test]
    fn context_size_exceeded_ffi_code_preserves_original_detail() {
        let detail = "Provided a request larger than the model context window.";
        let error = error_from_parts(ErrorCode::ContextSizeExceeded as i32, detail.to_string());

        match error {
            Error::ContextSizeExceeded(message) => assert_eq!(message, detail),
            other => panic!("expected ContextSizeExceeded, got {other:?}"),
        }
    }

    #[test]
    fn pcc_system_not_ready_should_remain_distinct() {
        assert_eq!(
            ModelAvailability::from(AvailabilityCode::PrivateCloudComputeSystemNotReady),
            ModelAvailability::PrivateCloudComputeSystemNotReady
        );
    }
}
