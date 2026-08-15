//! Manual FFI declarations for the Swift layer.
//!
//! These declarations match the `@_cdecl` functions in `src/swift/ffi.swift`.

use std::ffi::{c_char, c_int, c_void};

/// Opaque pointer to a Swift object (type-erased via Unmanaged<AnyObject>).
pub type SwiftPtr = *mut c_void;

/// Callback type for streaming chunks.
/// Parameters: `user_data`, chunk (C string)
pub type ChunkCallback = extern "C" fn(*mut c_void, *const c_char);

/// Callback type for completion.
/// Parameters: `user_data`
pub type DoneCallback = extern "C" fn(*mut c_void);

/// Callback type for errors.
/// Parameters: `user_data`, `error_code`, `error_message` (C string)
pub type ErrorCallback = extern "C" fn(*mut c_void, c_int, *const c_char);

/// Callback type for tool invocation.
/// Parameters: `user_data`, `tool_name`, `arguments_json`
/// Returns: result JSON string (caller must free)
pub type ToolCallback = extern "C" fn(*mut c_void, *const c_char, *const c_char) -> *mut c_char;

unsafe extern "C" {
    // ========================================================================
    // Model functions
    // ========================================================================

    /// Creates the default `SystemLanguageModel`.
    /// Returns null if not available; sets errorOut on failure.
    pub fn fm_model_default(error_out: *mut SwiftPtr) -> SwiftPtr;

    /// Creates a Foundation Models 27 `PrivateCloudComputeLanguageModel`.
    /// Returns null and sets `error_out` on pre-27 SDKs or runtimes.
    #[cfg(feature = "private-cloud-compute")]
    pub fn fm_model_private_cloud_compute(error_out: *mut SwiftPtr) -> SwiftPtr;

    /// Checks if the model is available.
    pub fn fm_model_is_available(model: SwiftPtr) -> bool;

    /// Returns the availability status as an integer:
    /// 0 = Available, 1 = `DeviceNotEligible`, 2 = `AppleIntelligenceNotEnabled`,
    /// 3 = `ModelNotReady`, 4+ = Unknown
    pub fn fm_model_availability(model: SwiftPtr) -> c_int;

    /// Returns token usage for a prompt using 26.4+ APIs when supported by the build SDK.
    /// Returns `-1` on error and sets `error_out`.
    /// Returns `-2` when token usage APIs are unavailable so Rust can use fallback estimation.
    pub fn fm_model_token_usage_for(
        model: SwiftPtr,
        prompt: *const c_char,
        error_out: *mut SwiftPtr,
    ) -> i64;

    /// Returns token usage for instructions + tools using 26.4+ APIs when supported by the build SDK.
    /// Returns `-1` on error and sets `error_out`.
    /// Returns `-2` when token usage APIs are unavailable so Rust can use fallback estimation.
    pub fn fm_model_token_usage_for_tools(
        model: SwiftPtr,
        instructions: *const c_char,
        tools_json: *const c_char,
        error_out: *mut SwiftPtr,
    ) -> i64;

    /// Returns Private Cloud Compute quota usage as a JSON string.
    /// Returns null and sets `error_out` for models without quota reporting.
    #[cfg(feature = "private-cloud-compute")]
    pub fn fm_model_pcc_quota_usage(model: SwiftPtr, error_out: *mut SwiftPtr) -> *mut c_char;

    /// Returns the model context size in tokens.
    /// Returns -1 and sets `error_out` for models without context-size reporting.
    pub fn fm_model_context_size(model: SwiftPtr, error_out: *mut SwiftPtr) -> i64;

    /// Frees a language model box.
    pub fn fm_model_free(model: SwiftPtr);

    // ========================================================================
    // Session functions
    // ========================================================================

    /// Creates a new session with optional instructions and tools.
    /// instructions may be null for no instructions.
    /// `tools_json` may be null for no tools.
    /// `builtin_tools_json` may be null for no built-in system tools.
    /// `user_data` and `tool_callback` are used for tool invocation.
    pub fn fm_session_create(
        model: SwiftPtr,
        instructions: *const c_char,
        tools_json: *const c_char,
        builtin_tools_json: *const c_char,
        user_data: *mut c_void,
        tool_callback: ToolCallback,
        error_out: *mut SwiftPtr,
    ) -> SwiftPtr;

    /// Creates a session from a transcript JSON string.
    pub fn fm_session_from_transcript(
        model: SwiftPtr,
        transcript_json: *const c_char,
        error_out: *mut SwiftPtr,
    ) -> SwiftPtr;

    /// Frees a session.
    pub fn fm_session_free(session: SwiftPtr);

    /// Sends a prompt and blocks until response is ready.
    /// `options_json` may be null for default options.
    /// Returns the response text (caller must free with `fm_string_free`).
    pub fn fm_session_respond(
        session: SwiftPtr,
        prompt: *const c_char,
        options_json: *const c_char,
        error_out: *mut SwiftPtr,
    ) -> *mut c_char;

    /// Sends a prompt and blocks until response is ready, with a timeout in milliseconds.
    /// `options_json` may be null for default options.
    /// Returns the response text (caller must free with `fm_string_free`).
    pub fn fm_session_respond_with_timeout(
        session: SwiftPtr,
        prompt: *const c_char,
        options_json: *const c_char,
        timeout_ms: u64,
        error_out: *mut SwiftPtr,
    ) -> *mut c_char;

    /// Sends a prompt using a Foundation Models 27 extended-reasoning level.
    /// `reasoning_level`: 1 = light, 2 = moderate, 3 = deep.
    pub fn fm_session_respond_with_reasoning(
        session: SwiftPtr,
        prompt: *const c_char,
        options_json: *const c_char,
        reasoning_level: c_int,
        error_out: *mut SwiftPtr,
    ) -> *mut c_char;

    /// Sends a prompt using extended reasoning, with a timeout in milliseconds.
    /// `reasoning_level`: 1 = light, 2 = moderate, 3 = deep.
    pub fn fm_session_respond_with_reasoning_timeout(
        session: SwiftPtr,
        prompt: *const c_char,
        options_json: *const c_char,
        reasoning_level: c_int,
        timeout_ms: u64,
        error_out: *mut SwiftPtr,
    ) -> *mut c_char;

    /// Starts streaming a response.
    /// Calls `on_chunk` for each text chunk, `on_done` when complete, `on_error` on failure.
    /// Tool calls are handled internally via the session's `GenericToolBridge`.
    pub fn fm_session_stream(
        session: SwiftPtr,
        prompt: *const c_char,
        options_json: *const c_char,
        user_data: *mut c_void,
        on_chunk: ChunkCallback,
        on_done: DoneCallback,
        on_error: ErrorCallback,
    );

    /// Returns cumulative session token usage as JSON (Foundation Models 27).
    /// Returns null and sets `error_out` on pre-27 SDKs or runtimes.
    pub fn fm_session_usage(session: SwiftPtr, error_out: *mut SwiftPtr) -> *mut c_char;

    /// Returns the usage recorded for the most recent completed response as
    /// JSON, or null when none has been recorded.
    pub fn fm_session_last_response_usage(session: SwiftPtr) -> *mut c_char;

    /// Replaces the session transcript (Foundation Models 27).
    /// Returns false and sets `error_out` on failure.
    pub fn fm_session_set_transcript(
        session: SwiftPtr,
        transcript_json: *const c_char,
        error_out: *mut SwiftPtr,
    ) -> bool;

    /// Sets the transcript error handling policy (Foundation Models 27).
    /// `policy`: 0 = clear, 1 = revert transcript, 2 = preserve transcript.
    pub fn fm_session_set_transcript_error_policy(
        session: SwiftPtr,
        policy: c_int,
        error_out: *mut SwiftPtr,
    ) -> bool;

    /// Sends a prompt with image attachments (Foundation Models 27).
    /// `attachments_json` describes each attachment; data attachments reference
    /// entries in the parallel `buffers`/`buffer_lens` arrays by index.
    pub fn fm_session_respond_with_attachments(
        session: SwiftPtr,
        prompt: *const c_char,
        options_json: *const c_char,
        attachments_json: *const c_char,
        buffers: *const *const u8,
        buffer_lens: *const usize,
        buffer_count: c_int,
        error_out: *mut SwiftPtr,
    ) -> *mut c_char;

    /// Cancels an ongoing stream operation.
    pub fn fm_session_cancel(session: SwiftPtr);

    /// Checks if the session is currently responding.
    pub fn fm_session_is_responding(session: SwiftPtr) -> bool;

    /// Gets the session transcript as JSON.
    /// Returns null on error. Caller must free with `fm_string_free`.
    pub fn fm_session_get_transcript(session: SwiftPtr, error_out: *mut SwiftPtr) -> *mut c_char;

    /// Prewarms the model with an optional prompt prefix.
    /// `prompt_prefix` may be null.
    pub fn fm_session_prewarm(session: SwiftPtr, prompt_prefix: *const c_char);

    // ========================================================================
    // Structured (JSON) response functions
    // ========================================================================

    /// Sends a prompt and returns a JSON response matching the provided schema.
    /// The schema is used to instruct the model to output valid JSON.
    /// Returns null on error. Caller must free with `fm_string_free`.
    pub fn fm_session_respond_json(
        session: SwiftPtr,
        prompt: *const c_char,
        schema_json: *const c_char,
        options_json: *const c_char,
        error_out: *mut SwiftPtr,
    ) -> *mut c_char;

    /// Sends a prompt and returns a JSON response matching the provided schema,
    /// waiting at most `timeout_ms` milliseconds.
    /// Returns null on error. Caller must free with `fm_string_free`.
    pub fn fm_session_respond_json_with_timeout(
        session: SwiftPtr,
        prompt: *const c_char,
        schema_json: *const c_char,
        options_json: *const c_char,
        timeout_ms: u64,
        error_out: *mut SwiftPtr,
    ) -> *mut c_char;

    /// Streams a JSON response matching the provided schema.
    /// Calls `on_chunk` for each text chunk, `on_done` when complete, `on_error` on failure.
    pub fn fm_session_stream_json(
        session: SwiftPtr,
        prompt: *const c_char,
        schema_json: *const c_char,
        options_json: *const c_char,
        user_data: *mut c_void,
        on_chunk: ChunkCallback,
        on_done: DoneCallback,
        on_error: ErrorCallback,
    );

    // ========================================================================
    // Error functions
    // ========================================================================

    /// Gets the error code from an error object.
    pub fn fm_error_code(error: SwiftPtr) -> c_int;

    /// Gets the error message from an error object.
    /// Returns a copy the caller must free with `fm_string_free`.
    pub fn fm_error_message(error: SwiftPtr) -> *mut c_char;

    /// Gets the tool name from a tool error (may be null).
    /// Returns a copy the caller must free with `fm_string_free`.
    pub fn fm_error_tool_name(error: SwiftPtr) -> *mut c_char;

    /// Gets the tool arguments JSON from a tool error (may be null).
    /// Returns a copy the caller must free with `fm_string_free`.
    pub fn fm_error_tool_arguments(error: SwiftPtr) -> *mut c_char;

    /// Frees an error object.
    pub fn fm_error_free(error: SwiftPtr);

    // ========================================================================
    // String functions
    // ========================================================================

    /// Frees a string allocated by the Swift layer.
    pub fn fm_string_free(s: *mut c_char);

}

// Note: fm_rust_string_free is exported by Rust (see src/lib.rs) and called by Swift
// after receiving tool callback results from Rust. Rust never calls it directly.

/// Availability status codes from Swift.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AvailabilityCode {
    Available = 0,
    DeviceNotEligible = 1,
    AppleIntelligenceNotEnabled = 2,
    ModelNotReady = 3,
    Unknown = 4,
    PrivateCloudComputeSystemNotReady = 5,
}

impl From<c_int> for AvailabilityCode {
    fn from(value: c_int) -> Self {
        match value {
            0 => AvailabilityCode::Available,
            1 => AvailabilityCode::DeviceNotEligible,
            2 => AvailabilityCode::AppleIntelligenceNotEnabled,
            3 => AvailabilityCode::ModelNotReady,
            5 => AvailabilityCode::PrivateCloudComputeSystemNotReady,
            _ => AvailabilityCode::Unknown,
        }
    }
}

/// Error codes from Swift.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    Unknown = 0,
    ModelNotAvailable = 1,
    GenerationFailed = 2,
    Cancelled = 3,
    ToolError = 4,
    InvalidInput = 5,
    Timeout = 6,
    UnsupportedPlatform = 7,
    NetworkFailure = 8,
    QuotaLimitReached = 9,
    ServiceUnavailable = 10,
    ContextSizeExceeded = 11,
    RateLimited = 12,
    GuardrailViolation = 13,
    Refusal = 14,
    UnsupportedCapability = 15,
    UnsupportedTranscriptContent = 16,
    UnsupportedGenerationGuide = 17,
    UnsupportedLanguageOrLocale = 18,
    AssetsUnavailable = 19,
    ConcurrentRequests = 20,
}

impl From<c_int> for ErrorCode {
    fn from(value: c_int) -> Self {
        match value {
            1 => ErrorCode::ModelNotAvailable,
            2 => ErrorCode::GenerationFailed,
            3 => ErrorCode::Cancelled,
            4 => ErrorCode::ToolError,
            5 => ErrorCode::InvalidInput,
            6 => ErrorCode::Timeout,
            7 => ErrorCode::UnsupportedPlatform,
            8 => ErrorCode::NetworkFailure,
            9 => ErrorCode::QuotaLimitReached,
            10 => ErrorCode::ServiceUnavailable,
            11 => ErrorCode::ContextSizeExceeded,
            12 => ErrorCode::RateLimited,
            13 => ErrorCode::GuardrailViolation,
            14 => ErrorCode::Refusal,
            15 => ErrorCode::UnsupportedCapability,
            16 => ErrorCode::UnsupportedTranscriptContent,
            17 => ErrorCode::UnsupportedGenerationGuide,
            18 => ErrorCode::UnsupportedLanguageOrLocale,
            19 => ErrorCode::AssetsUnavailable,
            20 => ErrorCode::ConcurrentRequests,
            _ => ErrorCode::Unknown,
        }
    }
}
