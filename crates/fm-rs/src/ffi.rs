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

    /// Checks if the model is available.
    pub fn fm_model_is_available(model: SwiftPtr) -> bool;

    /// Returns the availability status as an integer:
    /// 0 = Available, 1 = `DeviceNotEligible`, 2 = `AppleIntelligenceNotEnabled`,
    /// 3 = `ModelNotReady`, 4+ = Unknown
    pub fn fm_model_availability(model: SwiftPtr) -> c_int;

    /// Frees a `SystemLanguageModel`.
    pub fn fm_model_free(model: SwiftPtr);

    // ========================================================================
    // Session functions
    // ========================================================================

    /// Creates a new session with optional instructions and tools.
    /// instructions may be null for no instructions.
    /// `tools_json` may be null for no tools.
    /// `user_data` and `tool_callback` are used for tool invocation.
    pub fn fm_session_create(
        model: SwiftPtr,
        instructions: *const c_char,
        tools_json: *const c_char,
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
    /// The returned string is valid until `fm_error_free` is called.
    pub fn fm_error_message(error: SwiftPtr) -> *const c_char;

    /// Gets the tool name from a tool error (may be null).
    /// The returned string is valid until `fm_error_free` is called.
    pub fn fm_error_tool_name(error: SwiftPtr) -> *const c_char;

    /// Gets the tool arguments JSON from a tool error (may be null).
    /// The returned string is valid until `fm_error_free` is called.
    pub fn fm_error_tool_arguments(error: SwiftPtr) -> *const c_char;

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
}

impl From<c_int> for AvailabilityCode {
    fn from(value: c_int) -> Self {
        match value {
            0 => AvailabilityCode::Available,
            1 => AvailabilityCode::DeviceNotEligible,
            2 => AvailabilityCode::AppleIntelligenceNotEnabled,
            3 => AvailabilityCode::ModelNotReady,
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
            _ => ErrorCode::Unknown,
        }
    }
}
