//! Session management for `FoundationModels`.
//!
//! A session maintains conversation context between requests.

use std::collections::HashMap;
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::ptr::{self, NonNull};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::context::{ContextLimit, ContextUsage, context_usage_from_transcript};
use crate::error::{Error, Result};
use crate::ffi::{self, SwiftPtr};
use crate::model::{SystemLanguageModel, error_from_swift};
use crate::options::GenerationOptions;
use crate::tool::{Tool, ToolResult, tools_to_json};

/// Type alias for the tool map used in sessions.
type ToolMapInner = HashMap<String, Arc<dyn Tool>>;

/// Callback data shared between the session and tool callbacks.
///
/// This struct ensures safe cleanup by tracking active callbacks and
/// preventing new callbacks from starting when the session is being dropped.
struct ToolCallbackData {
    tools: Mutex<ToolMapInner>,
    /// Set to true when the session is being dropped.
    dropping: AtomicBool,
    /// Number of callbacks currently in progress.
    active_callbacks: AtomicUsize,
}

/// RAII guard to track active callbacks.
struct CallbackGuard<'a>(&'a AtomicUsize);

impl Drop for CallbackGuard<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Response returned by the model.
#[derive(Debug, Clone)]
pub struct Response {
    content: String,
}

impl Response {
    /// Creates a new response with the given content.
    pub(crate) fn new(content: String) -> Self {
        Self { content }
    }

    /// Gets the text content of the response.
    pub fn content(&self) -> &str {
        &self.content
    }

    /// Converts the response into its text content.
    pub fn into_content(self) -> String {
        self.content
    }
}

impl AsRef<str> for Response {
    fn as_ref(&self) -> &str {
        &self.content
    }
}

impl std::fmt::Display for Response {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.content)
    }
}

/// A session that interacts with a language model.
///
/// A session maintains state between requests, allowing for multi-turn conversations.
/// You can reuse the same session for multiple prompts or create a new one each time.
///
/// # Example
///
/// ```rust,no_run
/// use fm_rs::{Session, SystemLanguageModel, GenerationOptions};
///
/// let model = SystemLanguageModel::new()?;
/// let session = Session::new(&model)?;
///
/// let response = session.respond("Hello!", &GenerationOptions::default())?;
/// println!("{}", response.content());
/// # Ok::<(), fm_rs::Error>(())
/// ```
pub struct Session {
    ptr: NonNull<c_void>,
    /// Arc to the callback data, shared with the FFI callback.
    /// Using Arc ensures the data stays alive while callbacks are in flight.
    tool_callback_data: Option<Arc<ToolCallbackData>>,
}

impl Session {
    /// Creates a new session with the given model.
    pub fn new(model: &SystemLanguageModel) -> Result<Self> {
        Self::create_internal(model, None, &[])
    }

    /// Creates a new session with instructions.
    ///
    /// Instructions define the model's behavior and role.
    pub fn with_instructions(model: &SystemLanguageModel, instructions: &str) -> Result<Self> {
        Self::create_internal(model, Some(instructions), &[])
    }

    /// Creates a new session with tools.
    ///
    /// Tools allow the model to call external functions during generation.
    pub fn with_tools(model: &SystemLanguageModel, tools: &[Arc<dyn Tool>]) -> Result<Self> {
        Self::create_internal(model, None, tools)
    }

    /// Creates a new session with both instructions and tools.
    pub fn with_instructions_and_tools(
        model: &SystemLanguageModel,
        instructions: &str,
        tools: &[Arc<dyn Tool>],
    ) -> Result<Self> {
        Self::create_internal(model, Some(instructions), tools)
    }

    /// Creates a session from a transcript JSON string.
    ///
    /// This allows restoring a previous conversation.
    /// Note: Restored sessions do not have tools - use `with_tools` for new sessions.
    pub fn from_transcript(model: &SystemLanguageModel, transcript_json: &str) -> Result<Self> {
        let transcript_c = CString::new(transcript_json)?;
        let mut error: SwiftPtr = ptr::null_mut();

        let ptr = unsafe {
            ffi::fm_session_from_transcript(model.as_ptr(), transcript_c.as_ptr(), &raw mut error)
        };

        if !error.is_null() {
            return Err(error_from_swift(error));
        }

        NonNull::new(ptr)
            .map(|ptr| Self {
                ptr,
                tool_callback_data: None,
            })
            .ok_or_else(|| {
                Error::InternalError(
                    "Session creation from transcript returned null without error. \
                     The transcript JSON may be malformed or incompatible."
                        .to_string(),
                )
            })
    }

    /// Internal helper to create a session.
    fn create_internal(
        model: &SystemLanguageModel,
        instructions: Option<&str>,
        tools: &[Arc<dyn Tool>],
    ) -> Result<Self> {
        let instructions_c = instructions.map(CString::new).transpose()?;
        let instructions_ptr = instructions_c.as_ref().map_or(ptr::null(), |s| s.as_ptr());

        // Build tool map and serialize for FFI
        let mut tool_map = HashMap::new();
        let tools_json = if tools.is_empty() {
            None
        } else {
            let tool_refs: Vec<&dyn Tool> = tools.iter().map(std::convert::AsRef::as_ref).collect();
            for tool in tools {
                tool_map.insert(tool.name().to_string(), Arc::clone(tool));
            }
            let json_str = tools_to_json(&tool_refs)?;
            Some(CString::new(json_str)?)
        };
        let tools_ptr = tools_json.as_ref().map_or(ptr::null(), |s| s.as_ptr());

        // Create callback data with synchronization primitives
        let callback_data = if tools.is_empty() {
            None
        } else {
            Some(Arc::new(ToolCallbackData {
                tools: Mutex::new(tool_map),
                dropping: AtomicBool::new(false),
                active_callbacks: AtomicUsize::new(0),
            }))
        };

        // Get user_data pointer for FFI (we leak an Arc clone that Swift holds)
        let user_data = callback_data.as_ref().map_or(ptr::null_mut(), |arc| {
            Arc::into_raw(Arc::clone(arc)) as *mut c_void
        });

        let mut error: SwiftPtr = ptr::null_mut();

        let ptr = unsafe {
            ffi::fm_session_create(
                model.as_ptr(),
                instructions_ptr,
                tools_ptr,
                user_data,
                session_tool_callback,
                &raw mut error,
            )
        };

        if !error.is_null() {
            // Clean up leaked Arc if we allocated it
            if !user_data.is_null() {
                let _ = unsafe { Arc::from_raw(user_data as *const ToolCallbackData) };
            }
            return Err(error_from_swift(error));
        }

        NonNull::new(ptr)
            .map(|ptr| Self {
                ptr,
                tool_callback_data: callback_data,
            })
            .ok_or_else(|| {
                // Clean up leaked Arc if we allocated it
                if !user_data.is_null() {
                    let _ = unsafe { Arc::from_raw(user_data as *const ToolCallbackData) };
                }
                Error::InternalError(
                    "Session creation returned null without error. \
                     Check model availability and instructions validity."
                        .to_string(),
                )
            })
    }

    /// Sends a prompt and waits for the complete response.
    ///
    /// This method blocks until the model finishes generating.
    pub fn respond(&self, prompt: &str, options: &GenerationOptions) -> Result<Response> {
        let prompt_c = CString::new(prompt)?;
        let options_json = options.to_json();
        let options_c = CString::new(options_json)?;

        let mut error: SwiftPtr = ptr::null_mut();

        let response_ptr = unsafe {
            ffi::fm_session_respond(
                self.ptr.as_ptr(),
                prompt_c.as_ptr(),
                options_c.as_ptr(),
                &raw mut error,
            )
        };

        if !error.is_null() {
            return Err(error_from_swift(error));
        }

        if response_ptr.is_null() {
            return Err(Error::GenerationError("Received null response".to_string()));
        }

        let content = unsafe {
            let cstr = CStr::from_ptr(response_ptr);
            let s = cstr
                .to_str()
                .map_err(|e| Error::GenerationError(format!("Invalid UTF-8 in response: {e}")))?
                .to_owned();
            ffi::fm_string_free(response_ptr);
            s
        };

        Ok(Response::new(content))
    }

    /// Sends a prompt and waits for the complete response, with a timeout.
    ///
    /// If `timeout` is zero, this behaves like [`respond`](Self::respond).
    pub fn respond_with_timeout(
        &self,
        prompt: &str,
        options: &GenerationOptions,
        timeout: Duration,
    ) -> Result<Response> {
        if timeout.is_zero() {
            return self.respond(prompt, options);
        }

        let timeout_ms = u64::try_from(timeout.as_millis()).map_err(|_| {
            Error::InvalidInput("Timeout is too large to represent in milliseconds".to_string())
        })?;

        let prompt_c = CString::new(prompt)?;
        let options_json = options.to_json();
        let options_c = CString::new(options_json)?;

        let mut error: SwiftPtr = ptr::null_mut();

        let response_ptr = unsafe {
            ffi::fm_session_respond_with_timeout(
                self.ptr.as_ptr(),
                prompt_c.as_ptr(),
                options_c.as_ptr(),
                timeout_ms,
                &raw mut error,
            )
        };

        if !error.is_null() {
            return Err(error_from_swift(error));
        }

        if response_ptr.is_null() {
            return Err(Error::GenerationError("Received null response".to_string()));
        }

        let content = unsafe {
            let cstr = CStr::from_ptr(response_ptr);
            let s = cstr
                .to_str()
                .map_err(|e| Error::GenerationError(format!("Invalid UTF-8 in response: {e}")))?
                .to_owned();
            ffi::fm_string_free(response_ptr);
            s
        };

        Ok(Response::new(content))
    }

    /// Sends a prompt and streams the response.
    ///
    /// The `on_chunk` callback is called for each text chunk as it arrives.
    /// This method blocks until streaming is complete.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use fm_rs::{Session, SystemLanguageModel, GenerationOptions};
    ///
    /// let model = SystemLanguageModel::new()?;
    /// let session = Session::new(&model)?;
    ///
    /// session.stream_response("Tell me a story", &GenerationOptions::default(), |chunk| {
    ///     print!("{}", chunk);
    /// })?;
    /// # Ok::<(), fm_rs::Error>(())
    /// ```
    pub fn stream_response<F>(
        &self,
        prompt: &str,
        options: &GenerationOptions,
        on_chunk: F,
    ) -> Result<()>
    where
        F: FnMut(&str) + Send + 'static,
    {
        let prompt_c = CString::new(prompt)?;
        let options_json = options.to_json();
        let options_c = CString::new(options_json)?;

        // Create callback state
        let state = Box::new(StreamState {
            on_chunk: Mutex::new(Box::new(on_chunk)),
            error: Mutex::new(None),
        });
        let state_ptr = Box::into_raw(state).cast::<c_void>();

        unsafe {
            ffi::fm_session_stream(
                self.ptr.as_ptr(),
                prompt_c.as_ptr(),
                options_c.as_ptr(),
                state_ptr,
                stream_chunk_callback,
                stream_done_callback,
                stream_error_callback,
            );
        }

        // Reclaim the state and check for errors
        let state = unsafe { Box::from_raw(state_ptr.cast::<StreamState>()) };
        let error = state.error.lock().map_err(|_| Error::PoisonError)?;
        if let Some(err) = error.as_ref() {
            return Err(Error::GenerationError(err.clone()));
        }

        Ok(())
    }

    /// Cancels an ongoing stream operation.
    pub fn cancel(&self) {
        unsafe {
            ffi::fm_session_cancel(self.ptr.as_ptr());
        }
    }

    /// Checks if the session is currently generating a response.
    pub fn is_responding(&self) -> bool {
        unsafe { ffi::fm_session_is_responding(self.ptr.as_ptr()) }
    }

    /// Gets the session transcript as a JSON string.
    ///
    /// This can be used to persist and restore conversations.
    pub fn transcript_json(&self) -> Result<String> {
        let mut error: SwiftPtr = ptr::null_mut();
        let ptr = unsafe { ffi::fm_session_get_transcript(self.ptr.as_ptr(), &raw mut error) };

        if !error.is_null() {
            return Err(error_from_swift(error));
        }

        if ptr.is_null() {
            return Err(Error::InternalError(
                "Transcript retrieval returned null without error. \
                 The session may be in an invalid state."
                    .to_string(),
            ));
        }

        let json = unsafe {
            let cstr = CStr::from_ptr(ptr);
            let s = cstr
                .to_str()
                .map_err(|e| Error::InternalError(format!("Invalid UTF-8 in transcript: {e}")))?
                .to_owned();
            ffi::fm_string_free(ptr);
            s
        };

        Ok(json)
    }

    /// Estimates current context usage based on the session transcript.
    pub fn context_usage(&self, limit: &ContextLimit) -> Result<ContextUsage> {
        let transcript_json = self.transcript_json()?;
        context_usage_from_transcript(&transcript_json, limit)
    }

    /// Returns an error if the estimated context usage exceeds the configured limit.
    pub fn ensure_context_within(&self, limit: &ContextLimit) -> Result<()> {
        let usage = self.context_usage(limit)?;
        if usage.over_limit {
            return Err(Error::InvalidInput(format!(
                "Estimated context usage {} exceeds configured limit {} (reserved: {})",
                usage.estimated_tokens, usage.max_tokens, usage.reserved_response_tokens
            )));
        }
        Ok(())
    }

    /// Prewarms the model with an optional prompt prefix.
    ///
    /// This can reduce latency for the first response.
    pub fn prewarm(&self, prompt_prefix: Option<&str>) -> Result<()> {
        let prefix_c = prompt_prefix.map(CString::new).transpose()?;
        let prefix_ptr = prefix_c.as_ref().map_or(ptr::null(), |s| s.as_ptr());

        unsafe {
            ffi::fm_session_prewarm(self.ptr.as_ptr(), prefix_ptr);
        }

        Ok(())
    }

    /// Sends a prompt and returns a structured JSON response.
    ///
    /// The schema is a JSON Schema that describes the expected output format.
    /// The model is instructed to produce JSON that matches the schema.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use fm_rs::{Session, SystemLanguageModel, GenerationOptions};
    /// use serde::Deserialize;
    /// use serde_json::json;
    ///
    /// #[derive(Deserialize)]
    /// struct Person {
    ///     name: String,
    ///     age: u32,
    /// }
    ///
    /// let model = SystemLanguageModel::new()?;
    /// let session = Session::new(&model)?;
    ///
    /// let schema = json!({
    ///     "type": "object",
    ///     "properties": {
    ///         "name": { "type": "string" },
    ///         "age": { "type": "integer" }
    ///     },
    ///     "required": ["name", "age"]
    /// });
    ///
    /// let json_str = session.respond_json(
    ///     "Generate a fictional person",
    ///     &schema,
    ///     &GenerationOptions::default()
    /// )?;
    ///
    /// let person: Person = serde_json::from_str(&json_str)?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn respond_json(
        &self,
        prompt: &str,
        schema: &serde_json::Value,
        options: &GenerationOptions,
    ) -> Result<String> {
        let prompt_c = CString::new(prompt)?;
        let schema_json = serde_json::to_string(schema)?;
        let schema_c = CString::new(schema_json)?;
        let options_json = options.to_json();
        let options_c = CString::new(options_json)?;

        let mut error: SwiftPtr = ptr::null_mut();

        let response_ptr = unsafe {
            ffi::fm_session_respond_json(
                self.ptr.as_ptr(),
                prompt_c.as_ptr(),
                schema_c.as_ptr(),
                options_c.as_ptr(),
                &raw mut error,
            )
        };

        if !error.is_null() {
            return Err(error_from_swift(error));
        }

        if response_ptr.is_null() {
            return Err(Error::GenerationError(
                "Received null response from JSON generation".to_string(),
            ));
        }

        let content = unsafe {
            let cstr = CStr::from_ptr(response_ptr);
            let s = cstr
                .to_str()
                .map_err(|e| {
                    Error::GenerationError(format!("Invalid UTF-8 in JSON response: {e}"))
                })?
                .to_owned();
            ffi::fm_string_free(response_ptr);
            s
        };

        Ok(content)
    }

    /// Sends a prompt and returns a deserialized structured response.
    ///
    /// This is a convenience method that calls `respond_json` and deserializes
    /// the result into the specified type.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use fm_rs::{Session, SystemLanguageModel, GenerationOptions};
    /// use serde::Deserialize;
    /// use serde_json::json;
    ///
    /// #[derive(Deserialize)]
    /// struct Person {
    ///     name: String,
    ///     age: u32,
    /// }
    ///
    /// let model = SystemLanguageModel::new()?;
    /// let session = Session::new(&model)?;
    ///
    /// let schema = json!({
    ///     "type": "object",
    ///     "properties": {
    ///         "name": { "type": "string" },
    ///         "age": { "type": "integer" }
    ///     },
    ///     "required": ["name", "age"]
    /// });
    ///
    /// let person: Person = session.respond_structured(
    ///     "Generate a fictional person",
    ///     &schema,
    ///     &GenerationOptions::default()
    /// )?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn respond_structured<T: serde::de::DeserializeOwned>(
        &self,
        prompt: &str,
        schema: &serde_json::Value,
        options: &GenerationOptions,
    ) -> Result<T> {
        let json_str = self.respond_json(prompt, schema, options)?;
        serde_json::from_str(&json_str)
            .map_err(|e| Error::InvalidInput(format!("Failed to deserialize response: {e}")))
    }

    /// Sends a prompt and returns a deserialized structured response using a derived schema.
    ///
    /// This uses the [`crate::Generable`] implementation to obtain the JSON schema.
    pub fn respond_structured_gen<T>(&self, prompt: &str, options: &GenerationOptions) -> Result<T>
    where
        T: crate::Generable + serde::de::DeserializeOwned,
    {
        self.respond_structured(prompt, &T::schema(), options)
    }

    /// Streams a structured JSON response.
    ///
    /// The `on_chunk` callback receives partial JSON as it's generated.
    /// Note that partial chunks may not be valid JSON until streaming completes.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use fm_rs::{Session, SystemLanguageModel, GenerationOptions};
    /// use serde_json::json;
    ///
    /// let model = SystemLanguageModel::new()?;
    /// let session = Session::new(&model)?;
    ///
    /// let schema = json!({
    ///     "type": "object",
    ///     "properties": {
    ///         "items": { "type": "array", "items": { "type": "string" } }
    ///     }
    /// });
    ///
    /// session.stream_json(
    ///     "List 5 programming languages",
    ///     &schema,
    ///     &GenerationOptions::default(),
    ///     |chunk| {
    ///         print!("{chunk}");
    ///     }
    /// )?;
    /// # Ok::<(), fm_rs::Error>(())
    /// ```
    pub fn stream_json<F>(
        &self,
        prompt: &str,
        schema: &serde_json::Value,
        options: &GenerationOptions,
        on_chunk: F,
    ) -> Result<()>
    where
        F: FnMut(&str) + Send + 'static,
    {
        let prompt_c = CString::new(prompt)?;
        let schema_json = serde_json::to_string(schema)?;
        let schema_c = CString::new(schema_json)?;
        let options_json = options.to_json();
        let options_c = CString::new(options_json)?;

        // Create callback state
        let state = Box::new(StreamState {
            on_chunk: Mutex::new(Box::new(on_chunk)),
            error: Mutex::new(None),
        });
        let state_ptr = Box::into_raw(state).cast::<c_void>();

        unsafe {
            ffi::fm_session_stream_json(
                self.ptr.as_ptr(),
                prompt_c.as_ptr(),
                schema_c.as_ptr(),
                options_c.as_ptr(),
                state_ptr,
                stream_chunk_callback,
                stream_done_callback,
                stream_error_callback,
            );
        }

        // Reclaim the state and check for errors
        let state = unsafe { Box::from_raw(state_ptr.cast::<StreamState>()) };
        let error = state.error.lock().map_err(|_| Error::PoisonError)?;
        if let Some(err) = error.as_ref() {
            return Err(Error::GenerationError(err.clone()));
        }

        Ok(())
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // Signal that we're dropping - new callbacks will return early
        if let Some(ref callback_data) = self.tool_callback_data {
            callback_data.dropping.store(true, Ordering::SeqCst);

            // Wait for any in-flight callbacks to complete (with timeout)
            let mut attempts = 0;
            while callback_data.active_callbacks.load(Ordering::SeqCst) > 0 && attempts < 100 {
                std::thread::sleep(std::time::Duration::from_millis(10));
                attempts += 1;
            }
        }

        // Now safe to free the Swift session
        unsafe {
            ffi::fm_session_free(self.ptr.as_ptr());
        }

        // The Arc in tool_callback_data will be dropped automatically.
        // Swift also holds an Arc clone (via Arc::into_raw), which will be
        // reclaimed when Swift's ToolDispatcher is deallocated.
    }
}

// SAFETY: Session is a wrapper around a Swift object that uses
// DispatchQueue for thread safety internally.
unsafe impl Send for Session {}

// Note: Session is NOT Sync because streaming callbacks use internal mutable state.
// If you need to share a session across threads, wrap it in Arc<Mutex<Session>>.

/// Type alias for the chunk callback function.
type ChunkCallbackFn = dyn FnMut(&str) + Send;

/// Internal state for streaming callbacks.
struct StreamState {
    on_chunk: Mutex<Box<ChunkCallbackFn>>,
    error: Mutex<Option<String>>,
}

/// Callback invoked when a chunk arrives during streaming.
extern "C" fn stream_chunk_callback(user_data: *mut c_void, chunk: *const c_char) {
    if user_data.is_null() || chunk.is_null() {
        return;
    }

    let state = unsafe { &*(user_data as *const StreamState) };
    let chunk_str = unsafe { CStr::from_ptr(chunk).to_string_lossy() };

    if let Ok(mut on_chunk) = state.on_chunk.lock() {
        on_chunk(&chunk_str);
    }
}

/// Callback invoked when streaming is done.
extern "C" fn stream_done_callback(_user_data: *mut c_void) {
    // Nothing to do - state cleanup happens in stream_response
}

/// Callback invoked on error during streaming.
extern "C" fn stream_error_callback(user_data: *mut c_void, _code: c_int, message: *const c_char) {
    if user_data.is_null() {
        return;
    }

    let state = unsafe { &*(user_data as *const StreamState) };
    let msg = if message.is_null() {
        "Streaming error occurred (no message provided by Swift)".to_string()
    } else {
        unsafe { CStr::from_ptr(message).to_string_lossy().into_owned() }
    };

    if let Ok(mut error) = state.error.lock() {
        *error = Some(msg);
    }
}

/// Callback invoked when a tool needs to be called during session operations.
/// This is used by Swift's `FFITool` to call back into Rust.
extern "C" fn session_tool_callback(
    user_data: *mut c_void,
    tool_name: *const c_char,
    arguments_json: *const c_char,
) -> *mut c_char {
    if user_data.is_null() || tool_name.is_null() {
        let result = ToolResult::error("Invalid callback parameters");
        return string_to_c(result.to_json());
    }

    // user_data is a raw pointer to Arc<ToolCallbackData> (from Arc::into_raw)
    // SAFETY: Swift holds a reference to this Arc, keeping it alive.
    // We must NOT consume the Arc here - just borrow it.
    let callback_data = unsafe { &*(user_data as *const ToolCallbackData) };

    // Check if session is being dropped - if so, return early
    if callback_data.dropping.load(Ordering::SeqCst) {
        let result = ToolResult::error("Session is being dropped");
        return string_to_c(result.to_json());
    }

    // Track that we're in a callback (guard ensures cleanup on all exit paths)
    callback_data
        .active_callbacks
        .fetch_add(1, Ordering::SeqCst);
    let _guard = CallbackGuard(&callback_data.active_callbacks);

    let name = unsafe { CStr::from_ptr(tool_name).to_string_lossy().into_owned() };
    let args_str = if arguments_json.is_null() {
        "{}".to_string()
    } else {
        unsafe {
            CStr::from_ptr(arguments_json)
                .to_string_lossy()
                .into_owned()
        }
    };

    // Parse arguments (with a best-effort auto-close for truncated JSON)
    let arguments: serde_json::Value = match parse_tool_arguments(&args_str) {
        Ok(v) => v,
        Err(message) => {
            let result = ToolResult::error(message);
            return string_to_c(result.to_json());
        }
    };

    // Find and call the tool
    let Ok(tools) = callback_data.tools.lock() else {
        let result = ToolResult::error("Failed to acquire tool lock");
        return string_to_c(result.to_json());
    };

    let Some(tool) = tools.get(&name).map(Arc::clone) else {
        let result = ToolResult::error(format!("Unknown tool: {name}"));
        return string_to_c(result.to_json());
    };

    // Release the lock before calling the tool (it might take a while)
    drop(tools);

    // Invoke the tool
    let result = match tool.call(arguments) {
        Ok(output) => ToolResult::success(output),
        Err(e) => ToolResult::error(e.to_string()),
    };

    string_to_c(result.to_json())
}

/// Helper to convert a Rust string to a C string that can be freed by Swift.
fn string_to_c(s: String) -> *mut c_char {
    match CString::new(s) {
        Ok(cs) => cs.into_raw(),
        Err(_) => ptr::null_mut(),
    }
}

fn parse_tool_arguments(input: &str) -> std::result::Result<serde_json::Value, String> {
    match serde_json::from_str(input) {
        Ok(value) => Ok(value),
        Err(err) => {
            if let Some(fixed) = autoclose_json(input) {
                match serde_json::from_str(&fixed) {
                    Ok(value) => {
                        // Log when auto-close fixes truncated JSON (debug builds only)
                        #[cfg(debug_assertions)]
                        eprintln!(
                            "[fm-rs] autoclose_json repaired truncated tool arguments: {input:?} -> {fixed:?}"
                        );
                        Ok(value)
                    }
                    Err(fixed_err) => Err(format!(
                        "Failed to parse arguments: {err}; attempted fix: {fixed_err}"
                    )),
                }
            } else {
                Err(format!("Failed to parse arguments: {err}"))
            }
        }
    }
}

/// Maximum input size for `autoclose_json` to prevent resource exhaustion (1 MB).
const AUTOCLOSE_JSON_MAX_SIZE: usize = 1024 * 1024;

fn autoclose_json(input: &str) -> Option<String> {
    // Limit input size to prevent resource exhaustion attacks
    if input.len() > AUTOCLOSE_JSON_MAX_SIZE {
        return None;
    }

    let mut stack: Vec<char> = Vec::new();
    let mut in_string = false;
    let mut escape = false;

    for ch in input.chars() {
        if in_string {
            if escape {
                escape = false;
                continue;
            }
            if ch == '\\' {
                escape = true;
                continue;
            }
            if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '{' => stack.push('}'),
            '[' => stack.push(']'),
            '}' => {
                if stack.pop() != Some('}') {
                    return None;
                }
            }
            ']' => {
                if stack.pop() != Some(']') {
                    return None;
                }
            }
            _ => {}
        }
    }

    if in_string || stack.is_empty() {
        return None;
    }

    let mut out = input.to_string();
    while let Some(close) = stack.pop() {
        out.push(close);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_response() {
        let response = Response::new("Hello, world!".to_string());
        assert_eq!(response.content(), "Hello, world!");
        assert_eq!(response.as_ref(), "Hello, world!");
        assert_eq!(format!("{response}"), "Hello, world!");
        assert_eq!(response.into_content(), "Hello, world!");
    }
}
