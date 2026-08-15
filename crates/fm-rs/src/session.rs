//! Session management for `FoundationModels`.
//!
//! A session maintains conversation context between requests.

use std::collections::{HashMap, HashSet};
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::ptr::{self, NonNull};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::context::{ContextLimit, ContextUsage, context_usage_from_transcript};
use crate::error::{Error, Result};
use crate::ffi::{self, SwiftPtr};
use crate::model::{LanguageModel, error_from_parts, error_from_swift};
use crate::options::{GenerationOptions, ReasoningLevel};
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

/// Release the [`ToolCallbackData`] strong reference handed to Swift.
///
/// `Session::create_internal` gives Swift ownership of one `Arc` clone via
/// `Arc::into_raw`, so something has to give it back. `ToolDispatcher.deinit`
/// calls this when the dispatcher is deallocated.
///
/// # Safety
///
/// `user_data` must be null, or a pointer from `Arc::into_raw` on an
/// `Arc<ToolCallbackData>` that has not already been reclaimed. Swift calls
/// this exactly once per dispatcher, from `deinit`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fm_rust_tool_data_free(user_data: *mut c_void) {
    if user_data.is_null() {
        return;
    }
    // Drops one strong reference. `Session::tool_callback_data` holds the
    // other, so whichever side goes last frees the allocation.
    drop(unsafe { Arc::from_raw(user_data as *const ToolCallbackData) });
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
    usage: Option<SessionUsage>,
}

impl Response {
    /// Creates a response, optionally carrying per-response token usage.
    pub(crate) fn with_usage(content: String, usage: Option<SessionUsage>) -> Self {
        Self { content, usage }
    }

    /// Returns exact token usage for this response (Foundation Models 27).
    ///
    /// `None` on pre-27 build SDKs or runtimes, or when the framework did
    /// not report usage for this response.
    pub fn usage(&self) -> Option<SessionUsage> {
        self.usage
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

/// Apple's built-in system tools for sessions (Foundation Models 27).
///
/// These tools run inside the framework; unlike [`Tool`] implementations,
/// they never call back into Rust. All use Apple's default configuration.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SystemTool {
    /// Vision text recognition over image attachments.
    Ocr,
    /// Vision barcode and QR-code reading over image attachments.
    BarcodeReader,
    /// Core Spotlight semantic search over the device index.
    SpotlightSearch,
}

impl SystemTool {
    fn ffi_name(self) -> &'static str {
        match self {
            SystemTool::Ocr => "ocr",
            SystemTool::BarcodeReader => "barcodeReader",
            SystemTool::SpotlightSearch => "spotlightSearch",
        }
    }
}

/// Builder combining instructions, custom tools, and built-in system tools.
///
/// ```rust,no_run
/// use fm_rs::{Session, SystemLanguageModel, SystemTool};
///
/// let model = SystemLanguageModel::new()?;
/// let session = Session::builder(&model)
///     .instructions("Describe attached images and read any text in them.")
///     .system_tool(SystemTool::Ocr)
///     .build()?;
/// # Ok::<(), fm_rs::Error>(())
/// ```
pub struct SessionBuilder<'m, M: LanguageModel + ?Sized> {
    model: &'m M,
    instructions: Option<String>,
    tools: Vec<Arc<dyn Tool>>,
    system_tools: Vec<SystemTool>,
}

impl<M: LanguageModel + ?Sized> SessionBuilder<'_, M> {
    /// Sets the session instructions.
    #[must_use]
    pub fn instructions(mut self, instructions: impl Into<String>) -> Self {
        self.instructions = Some(instructions.into());
        self
    }

    /// Adds a Rust-implemented tool.
    #[must_use]
    pub fn tool(mut self, tool: Arc<dyn Tool>) -> Self {
        self.tools.push(tool);
        self
    }

    /// Adds a built-in system tool (Foundation Models 27).
    ///
    /// Session creation fails with [`crate::Error::UnsupportedPlatform`] on
    /// pre-27 build SDKs or runtimes.
    #[must_use]
    pub fn system_tool(mut self, tool: SystemTool) -> Self {
        self.system_tools.push(tool);
        self
    }

    /// Builds the session.
    pub fn build(self) -> Result<Session> {
        Session::create_internal(
            self.model,
            self.instructions.as_deref(),
            &self.tools,
            &self.system_tools,
        )
    }
}

impl Session {
    /// Creates a session builder for combining instructions, tools, and
    /// built-in system tools.
    pub fn builder<M: LanguageModel + ?Sized>(model: &M) -> SessionBuilder<'_, M> {
        SessionBuilder {
            model,
            instructions: None,
            tools: Vec::new(),
            system_tools: Vec::new(),
        }
    }

    /// Creates a new session with the given model.
    pub fn new<M: LanguageModel + ?Sized>(model: &M) -> Result<Self> {
        Self::create_internal(model, None, &[], &[])
    }

    /// Creates a new session with instructions.
    ///
    /// Instructions define the model's behavior and role.
    pub fn with_instructions<M: LanguageModel + ?Sized>(
        model: &M,
        instructions: &str,
    ) -> Result<Self> {
        Self::create_internal(model, Some(instructions), &[], &[])
    }

    /// Creates a new session with tools.
    ///
    /// Tools allow the model to call external functions during generation.
    pub fn with_tools<M: LanguageModel + ?Sized>(
        model: &M,
        tools: &[Arc<dyn Tool>],
    ) -> Result<Self> {
        Self::create_internal(model, None, tools, &[])
    }

    /// Creates a new session with both instructions and tools.
    pub fn with_instructions_and_tools<M: LanguageModel + ?Sized>(
        model: &M,
        instructions: &str,
        tools: &[Arc<dyn Tool>],
    ) -> Result<Self> {
        Self::create_internal(model, Some(instructions), tools, &[])
    }

    /// Creates a session from a transcript JSON string.
    ///
    /// This allows restoring a previous conversation.
    /// Note: Restored sessions do not have tools - use `with_tools` for new sessions.
    pub fn from_transcript<M: LanguageModel + ?Sized>(
        model: &M,
        transcript_json: &str,
    ) -> Result<Self> {
        let transcript_c = CString::new(transcript_json)?;
        let mut error: SwiftPtr = ptr::null_mut();

        let ptr = unsafe {
            ffi::fm_session_from_transcript(
                model.raw_model_ptr(),
                transcript_c.as_ptr(),
                &raw mut error,
            )
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
    fn create_internal<M: LanguageModel + ?Sized>(
        model: &M,
        instructions: Option<&str>,
        tools: &[Arc<dyn Tool>],
        system_tools: &[SystemTool],
    ) -> Result<Self> {
        let instructions_c = instructions.map(CString::new).transpose()?;
        let instructions_ptr = instructions_c.as_ref().map_or(ptr::null(), |s| s.as_ptr());

        // Serialize built-in system tool names for FFI
        let system_tools_json = system_tools_json(system_tools)?;
        let system_tools_ptr = system_tools_json
            .as_ref()
            .map_or(ptr::null(), |s| s.as_ptr());

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
                model.raw_model_ptr(),
                instructions_ptr,
                tools_ptr,
                system_tools_ptr,
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

        Ok(Response::with_usage(
            content,
            self.fetch_last_response_usage(),
        ))
    }

    /// Sends a prompt with Foundation Models 27 extended reasoning.
    ///
    /// Extended reasoning is a Foundation Models 27 `ContextOptions` request.
    /// The framework decides whether the session's model honors the level;
    /// models without the capability report a typed error. On an older build
    /// SDK or runtime, this returns [`Error::UnsupportedPlatform`].
    pub fn respond_with_reasoning(
        &self,
        prompt: &str,
        options: &GenerationOptions,
        reasoning_level: ReasoningLevel,
    ) -> Result<Response> {
        let prompt_c = CString::new(prompt)?;
        let options_c = CString::new(options.to_json())?;
        let mut error: SwiftPtr = ptr::null_mut();

        let response_ptr = unsafe {
            ffi::fm_session_respond_with_reasoning(
                self.ptr.as_ptr(),
                prompt_c.as_ptr(),
                options_c.as_ptr(),
                reasoning_level.as_ffi_code(),
                &raw mut error,
            )
        };

        if !error.is_null() {
            return Err(error_from_swift(error));
        }

        if response_ptr.is_null() {
            return Err(Error::GenerationError(
                "Received null reasoning response".to_string(),
            ));
        }

        let content = unsafe {
            let cstr = CStr::from_ptr(response_ptr);
            let content = cstr
                .to_str()
                .map_err(|error| {
                    Error::GenerationError(format!("Invalid UTF-8 in reasoning response: {error}"))
                })?
                .to_owned();
            ffi::fm_string_free(response_ptr);
            content
        };

        Ok(Response::with_usage(
            content,
            self.fetch_last_response_usage(),
        ))
    }

    /// Sends a prompt with extended reasoning and waits up to `timeout`.
    ///
    /// If `timeout` is zero, this behaves like
    /// [`respond_with_reasoning`](Self::respond_with_reasoning).
    /// Positive sub-millisecond timeouts are rounded up to one millisecond.
    pub fn respond_with_reasoning_timeout(
        &self,
        prompt: &str,
        options: &GenerationOptions,
        reasoning_level: ReasoningLevel,
        timeout: Duration,
    ) -> Result<Response> {
        if timeout.is_zero() {
            return self.respond_with_reasoning(prompt, options, reasoning_level);
        }

        let timeout_ms = timeout_millis(timeout)?;
        let prompt_c = CString::new(prompt)?;
        let options_c = CString::new(options.to_json())?;
        let mut error: SwiftPtr = ptr::null_mut();

        let response_ptr = unsafe {
            ffi::fm_session_respond_with_reasoning_timeout(
                self.ptr.as_ptr(),
                prompt_c.as_ptr(),
                options_c.as_ptr(),
                reasoning_level.as_ffi_code(),
                timeout_ms,
                &raw mut error,
            )
        };

        if !error.is_null() {
            return Err(error_from_swift(error));
        }

        if response_ptr.is_null() {
            return Err(Error::GenerationError(
                "Received null reasoning response".to_string(),
            ));
        }

        let content = unsafe {
            let cstr = CStr::from_ptr(response_ptr);
            let content = cstr
                .to_str()
                .map_err(|error| {
                    Error::GenerationError(format!("Invalid UTF-8 in reasoning response: {error}"))
                })?
                .to_owned();
            ffi::fm_string_free(response_ptr);
            content
        };

        Ok(Response::with_usage(
            content,
            self.fetch_last_response_usage(),
        ))
    }

    /// Sends a prompt and waits for the complete response, with a timeout.
    ///
    /// If `timeout` is zero, this behaves like [`respond`](Self::respond).
    /// Positive sub-millisecond timeouts are rounded up to one millisecond.
    pub fn respond_with_timeout(
        &self,
        prompt: &str,
        options: &GenerationOptions,
        timeout: Duration,
    ) -> Result<Response> {
        if timeout.is_zero() {
            return self.respond(prompt, options);
        }

        let timeout_ms = timeout_millis(timeout)?;

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

        Ok(Response::with_usage(
            content,
            self.fetch_last_response_usage(),
        ))
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
        if let Some((code, message)) = error.as_ref() {
            return Err(error_from_parts(*code, message.clone()));
        }

        Ok(())
    }

    /// Sends a prompt with image attachments and blocks until the response
    /// is ready (Foundation Models 27 multimodal prompting).
    ///
    /// The on-device model accepts images for description, extraction, and
    /// classification; the framework handles scaling and color conversion.
    /// With no attachments this behaves like [`respond`](Self::respond).
    /// Requires a 27 build SDK and runtime; otherwise returns
    /// [`Error::UnsupportedPlatform`]. Models without the image capability
    /// report [`Error::UnsupportedCapability`].
    pub fn respond_with_attachments(
        &self,
        prompt: &str,
        attachments: &[Attachment<'_>],
        options: &GenerationOptions,
    ) -> Result<Response> {
        if attachments.is_empty() {
            return self.respond(prompt, options);
        }

        let (specs_json, buffers, buffer_lens) = attachment_specs(attachments)?;
        let buffer_count = c_int::try_from(buffers.len())
            .map_err(|_| Error::InvalidInput("Too many attachments".to_string()))?;

        let prompt_c = CString::new(prompt)?;
        let options_c = CString::new(options.to_json())?;
        let specs_c = CString::new(specs_json)?;
        let mut error: SwiftPtr = ptr::null_mut();

        let response_ptr = unsafe {
            ffi::fm_session_respond_with_attachments(
                self.ptr.as_ptr(),
                prompt_c.as_ptr(),
                options_c.as_ptr(),
                specs_c.as_ptr(),
                buffers.as_ptr(),
                buffer_lens.as_ptr(),
                buffer_count,
                &raw mut error,
            )
        };

        if !error.is_null() {
            return Err(error_from_swift(error));
        }

        if response_ptr.is_null() {
            return Err(Error::GenerationError(
                "Received null multimodal response".to_string(),
            ));
        }

        let content = unsafe {
            let cstr = CStr::from_ptr(response_ptr);
            let content = cstr
                .to_str()
                .map_err(|error| {
                    Error::GenerationError(format!("Invalid UTF-8 in multimodal response: {error}"))
                })?
                .to_owned();
            ffi::fm_string_free(response_ptr);
            content
        };

        Ok(Response::with_usage(
            content,
            self.fetch_last_response_usage(),
        ))
    }

    /// Returns exact cumulative token usage for this session.
    ///
    /// Foundation Models 27 reports authoritative input, cached-input,
    /// output, and reasoning token counts across all requests in this
    /// session. To measure a single request, snapshot before and after and
    /// use [`SessionUsage::delta_since`]. Per-response usage is bridged into
    /// [`Response::usage`] on blocking response paths; per-response usage for
    /// streaming remains unsupported. On older build SDKs or runtimes this
    /// returns [`Error::UnsupportedPlatform`]; use [`crate::estimate_tokens`]
    /// there instead.
    pub fn usage(&self) -> Result<SessionUsage> {
        let mut error: SwiftPtr = ptr::null_mut();
        let json_ptr = unsafe { ffi::fm_session_usage(self.ptr.as_ptr(), &raw mut error) };

        if !error.is_null() {
            return Err(error_from_swift(error));
        }

        if json_ptr.is_null() {
            return Err(Error::InternalError(
                "Session usage returned null without an error".to_string(),
            ));
        }

        let json = unsafe {
            let json = CStr::from_ptr(json_ptr).to_string_lossy().into_owned();
            ffi::fm_string_free(json_ptr);
            json
        };

        session_usage_from_json(&json)
    }

    /// Replaces this session's transcript (Foundation Models 27).
    ///
    /// The JSON must be a Foundation Models `Transcript`, such as one
    /// returned by [`transcript_json`](Self::transcript_json) and edited. Fails while
    /// the session is responding. On older build SDKs or runtimes this
    /// returns [`Error::UnsupportedPlatform`]; there, create a new session
    /// with [`Session::from_transcript`] instead.
    pub fn set_transcript(&self, transcript_json: &str) -> Result<()> {
        let transcript_c = CString::new(transcript_json)?;
        let mut error: SwiftPtr = ptr::null_mut();

        let replaced = unsafe {
            ffi::fm_session_set_transcript(self.ptr.as_ptr(), transcript_c.as_ptr(), &raw mut error)
        };

        if !error.is_null() {
            return Err(error_from_swift(error));
        }

        if replaced {
            Ok(())
        } else {
            Err(Error::InternalError(
                "Transcript replacement failed without an error".to_string(),
            ))
        }
    }

    /// Sets or clears how the transcript is repaired after a failed request
    /// (Foundation Models 27).
    ///
    /// `None` restores the framework default. On older build SDKs or
    /// runtimes this returns [`Error::UnsupportedPlatform`].
    pub fn set_transcript_error_handling_policy(
        &self,
        policy: Option<TranscriptErrorHandlingPolicy>,
    ) -> Result<()> {
        let code = match policy {
            None => 0,
            Some(TranscriptErrorHandlingPolicy::RevertTranscript) => 1,
            Some(TranscriptErrorHandlingPolicy::PreserveTranscript) => 2,
        };
        let mut error: SwiftPtr = ptr::null_mut();

        let applied = unsafe {
            ffi::fm_session_set_transcript_error_policy(self.ptr.as_ptr(), code, &raw mut error)
        };

        if !error.is_null() {
            return Err(error_from_swift(error));
        }

        if applied {
            Ok(())
        } else {
            Err(Error::InternalError(
                "Transcript policy update failed without an error".to_string(),
            ))
        }
    }

    /// Fetches the usage recorded for the most recent completed response.
    ///
    /// Returns `None` on pre-27 build SDKs or runtimes, or when no blocking
    /// response has completed yet. Unreadable payloads also map to `None`;
    /// [`Response::usage`] documents this as "not reported".
    fn fetch_last_response_usage(&self) -> Option<SessionUsage> {
        let json_ptr = unsafe { ffi::fm_session_last_response_usage(self.ptr.as_ptr()) };
        if json_ptr.is_null() {
            return None;
        }

        let json = unsafe {
            let json = CStr::from_ptr(json_ptr).to_string_lossy().into_owned();
            ffi::fm_string_free(json_ptr);
            json
        };

        session_usage_from_json(&json).ok()
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

    /// Sends a prompt and returns a structured JSON response with a timeout.
    ///
    /// The response content contains the extracted JSON string. On Foundation Models 27,
    /// [`Response::usage`] also exposes the framework's per-response token usage when
    /// available. If `timeout` is zero, this uses the same path as
    /// [`respond_json`](Self::respond_json). Positive sub-millisecond timeouts are rounded up
    /// to one millisecond, the finest resolution supported by the Swift bridge.
    ///
    /// The timeout bounds only the caller's wait. On expiry, the bridge requests cooperative
    /// task cancellation and returns [`Error::Timeout`]; it cannot force Foundation Models
    /// framework work to stop immediately.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use std::time::Duration;
    ///
    /// use fm_rs::{GenerationOptions, Session, SystemLanguageModel};
    /// use serde_json::json;
    ///
    /// let model = SystemLanguageModel::new()?;
    /// let session = Session::new(&model)?;
    /// let schema = json!({
    ///     "type": "object",
    ///     "properties": { "answer": { "type": "string" } },
    ///     "required": ["answer"]
    /// });
    ///
    /// let response = session.respond_json_with_timeout(
    ///     "Answer briefly",
    ///     &schema,
    ///     &GenerationOptions::default(),
    ///     Duration::from_secs(30),
    /// )?;
    /// let json: serde_json::Value = serde_json::from_str(response.content())?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn respond_json_with_timeout(
        &self,
        prompt: &str,
        schema: &serde_json::Value,
        options: &GenerationOptions,
        timeout: Duration,
    ) -> Result<Response> {
        let Some(timeout_ms) = json_timeout_millis(timeout)? else {
            let content = self.respond_json(prompt, schema, options)?;
            return Ok(Response::with_usage(
                content,
                self.fetch_last_response_usage(),
            ));
        };

        let prompt_c = CString::new(prompt)?;
        let schema_c = CString::new(serde_json::to_string(schema)?)?;
        let options_c = CString::new(options.to_json())?;
        let mut error: SwiftPtr = ptr::null_mut();

        let response_ptr = unsafe {
            ffi::fm_session_respond_json_with_timeout(
                self.ptr.as_ptr(),
                prompt_c.as_ptr(),
                schema_c.as_ptr(),
                options_c.as_ptr(),
                timeout_ms,
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
            let content = cstr
                .to_str()
                .map_err(|error| {
                    Error::GenerationError(format!("Invalid UTF-8 in JSON response: {error}"))
                })?
                .to_owned();
            ffi::fm_string_free(response_ptr);
            content
        };

        Ok(Response::with_usage(
            content,
            self.fetch_last_response_usage(),
        ))
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
        if let Some((code, message)) = error.as_ref() {
            return Err(error_from_parts(*code, message.clone()));
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

/// An image input for Foundation Models 27 multimodal prompting.
///
/// Create one from a file path or from encoded image bytes (PNG, JPEG,
/// HEIC, ...), then pass it to [`Session::respond_with_attachments`]. Label
/// attachments so prompts can reference specific images by name.
#[derive(Debug, Clone)]
pub struct Attachment<'a> {
    source: AttachmentSource<'a>,
    label: Option<String>,
}

#[derive(Debug, Clone)]
enum AttachmentSource<'a> {
    File(std::path::PathBuf),
    ImageBytes(&'a [u8]),
}

fn system_tools_json(system_tools: &[SystemTool]) -> Result<Option<CString>> {
    if system_tools.is_empty() {
        return Ok(None);
    }

    let mut seen = HashSet::with_capacity(system_tools.len());
    let mut names = Vec::with_capacity(system_tools.len());
    for tool in system_tools {
        if !seen.insert(*tool) {
            return Err(Error::InvalidInput(format!(
                "Duplicate built-in system tool: {}",
                tool.ffi_name()
            )));
        }
        names.push(tool.ffi_name());
    }

    Ok(Some(CString::new(serde_json::to_string(&names)?)?))
}

impl<'a> Attachment<'a> {
    /// Creates an image attachment from a file on disk.
    pub fn file(path: impl Into<std::path::PathBuf>) -> Attachment<'static> {
        Attachment {
            source: AttachmentSource::File(path.into()),
            label: None,
        }
    }

    /// Creates an image attachment from encoded image bytes.
    pub fn image_bytes(bytes: &'a [u8]) -> Attachment<'a> {
        Attachment {
            source: AttachmentSource::ImageBytes(bytes),
            label: None,
        }
    }

    /// Labels the attachment so prompts can reference it by name.
    #[must_use]
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct AttachmentSpec<'a> {
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    buffer_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    label: Option<&'a str>,
}

type AttachmentBuffers = (String, Vec<*const u8>, Vec<usize>);

/// Serializes attachments to a JSON spec plus parallel byte-buffer arrays.
fn attachment_specs(attachments: &[Attachment<'_>]) -> Result<AttachmentBuffers> {
    let mut specs = Vec::with_capacity(attachments.len());
    let mut buffers = Vec::new();
    let mut buffer_lens = Vec::new();

    for attachment in attachments {
        let label = attachment.label.as_deref();
        match &attachment.source {
            AttachmentSource::File(path) => {
                let path = path.to_str().ok_or_else(|| {
                    Error::InvalidInput(format!(
                        "Attachment path is not valid UTF-8: {}",
                        path.display()
                    ))
                })?;
                specs.push(AttachmentSpec {
                    kind: "file",
                    path: Some(path),
                    buffer_index: None,
                    label,
                });
            }
            AttachmentSource::ImageBytes(bytes) => {
                specs.push(AttachmentSpec {
                    kind: "data",
                    path: None,
                    buffer_index: Some(buffers.len()),
                    label,
                });
                buffers.push(bytes.as_ptr());
                buffer_lens.push(bytes.len());
            }
        }
    }

    let json = serde_json::to_string(&specs)?;
    Ok((json, buffers, buffer_lens))
}

/// Exact cumulative token usage reported by a Foundation Models 27 session.
///
/// Returned by [`Session::usage`]. Unlike [`crate::estimate_tokens`], these
/// counts are authoritative and include prompt caching and
/// extended-reasoning accounting.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionUsage {
    /// Total input tokens across all requests in this session.
    pub input_tokens: u64,
    /// Input tokens that were served from the prompt cache.
    pub cached_input_tokens: u64,
    /// Total output tokens across all responses in this session.
    pub output_tokens: u64,
    /// Output tokens spent on extended reasoning.
    pub reasoning_tokens: u64,
}

impl SessionUsage {
    /// Returns the usage consumed since an earlier snapshot of this session.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] if any counter went backwards, which
    /// indicates the snapshots came from different sessions or were taken
    /// out of order.
    pub fn delta_since(&self, earlier: &SessionUsage) -> Result<SessionUsage> {
        let sub = |now: u64, before: u64, field: &str| {
            now.checked_sub(before).ok_or_else(|| {
                Error::InvalidInput(format!(
                    "Usage {field} went backwards ({now} < {before}); snapshots must come \
                     from the same session in order"
                ))
            })
        };

        Ok(SessionUsage {
            input_tokens: sub(self.input_tokens, earlier.input_tokens, "input_tokens")?,
            cached_input_tokens: sub(
                self.cached_input_tokens,
                earlier.cached_input_tokens,
                "cached_input_tokens",
            )?,
            output_tokens: sub(self.output_tokens, earlier.output_tokens, "output_tokens")?,
            reasoning_tokens: sub(
                self.reasoning_tokens,
                earlier.reasoning_tokens,
                "reasoning_tokens",
            )?,
        })
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
#[expect(
    clippy::struct_field_names,
    reason = "the _tokens suffix maps to the FFI JSON keys and mirrors Apple's usage terminology"
)]
struct SessionUsageDto {
    input_tokens: i64,
    cached_input_tokens: i64,
    output_tokens: i64,
    reasoning_tokens: i64,
}

fn session_usage_from_json(json: &str) -> Result<SessionUsage> {
    let dto: SessionUsageDto = serde_json::from_str(json)
        .map_err(|error| Error::InternalError(format!("Invalid session usage JSON: {error}")))?;

    let count = |value: i64, field: &str| {
        u64::try_from(value).map_err(|_| {
            Error::InternalError(format!("Session usage {field} was negative: {value}"))
        })
    };

    Ok(SessionUsage {
        input_tokens: count(dto.input_tokens, "inputTokens")?,
        cached_input_tokens: count(dto.cached_input_tokens, "cachedInputTokens")?,
        output_tokens: count(dto.output_tokens, "outputTokens")?,
        reasoning_tokens: count(dto.reasoning_tokens, "reasoningTokens")?,
    })
}

/// How a session repairs its transcript after a failed request
/// (Foundation Models 27).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptErrorHandlingPolicy {
    /// Remove the failed request from the transcript.
    RevertTranscript,
    /// Keep the failed request in the transcript.
    PreserveTranscript,
}

/// Internal state for streaming callbacks.
struct StreamState {
    on_chunk: Mutex<Box<ChunkCallbackFn>>,
    error: Mutex<Option<(c_int, String)>>,
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
extern "C" fn stream_error_callback(user_data: *mut c_void, code: c_int, message: *const c_char) {
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
        *error = Some((code, msg));
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

fn timeout_millis(timeout: Duration) -> Result<u64> {
    let milliseconds = u64::try_from(timeout.as_millis()).map_err(|_| {
        Error::InvalidInput("Timeout is too large to represent in milliseconds".to_string())
    })?;

    if timeout.is_zero() {
        Ok(0)
    } else {
        Ok(milliseconds.max(1))
    }
}

fn json_timeout_millis(timeout: Duration) -> Result<Option<u64>> {
    if timeout.is_zero() {
        Ok(None)
    } else {
        timeout_millis(timeout).map(Some)
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
            '}' if stack.pop() != Some('}') => return None,
            ']' if stack.pop() != Some(']') => return None,
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
    use std::time::Duration;

    use crate::error::Error;
    use crate::session::{
        Attachment, Response, SessionUsage, SystemTool, attachment_specs, json_timeout_millis,
        session_usage_from_json, system_tools_json, timeout_millis,
    };

    #[test]
    fn test_response() {
        let response = Response::with_usage("Hello, world!".to_string(), None);
        assert_eq!(response.content(), "Hello, world!");
        assert_eq!(response.as_ref(), "Hello, world!");
        assert_eq!(format!("{response}"), "Hello, world!");
        assert_eq!(response.into_content(), "Hello, world!");
    }

    #[test]
    fn timeout_millis_rejects_values_larger_than_u64() {
        let timeout = Duration::from_secs(u64::MAX);
        assert!(matches!(
            timeout_millis(timeout),
            Err(Error::InvalidInput(_))
        ));
    }

    #[test]
    fn timeout_millis_preserves_zero_and_rounds_positive_sub_millisecond_values_up() {
        assert!(matches!(timeout_millis(Duration::ZERO), Ok(0)));

        for timeout in [
            Duration::from_nanos(1),
            Duration::from_micros(500),
            Duration::from_micros(999),
        ] {
            assert!(matches!(timeout_millis(timeout), Ok(1)));
        }

        assert!(matches!(timeout_millis(Duration::from_millis(1)), Ok(1)));
    }

    const _: fn(
        &crate::Session,
        &str,
        &serde_json::Value,
        &crate::GenerationOptions,
        Duration,
    ) -> crate::Result<Response> = crate::Session::respond_json_with_timeout;

    #[test]
    fn respond_json_with_timeout_zero_selects_existing_json_path() {
        assert!(matches!(json_timeout_millis(Duration::ZERO), Ok(None)));
    }

    #[test]
    fn respond_json_with_timeout_rounds_positive_sub_millisecond_values_up() {
        for timeout in [
            Duration::from_nanos(1),
            Duration::from_micros(500),
            Duration::from_micros(999),
        ] {
            assert!(matches!(json_timeout_millis(timeout), Ok(Some(1))));
        }

        assert!(matches!(
            json_timeout_millis(Duration::from_millis(1)),
            Ok(Some(1))
        ));
    }

    #[test]
    fn respond_json_with_timeout_reuses_timeout_overflow_error() {
        let timeout = Duration::from_secs(u64::MAX);
        assert!(matches!(
            json_timeout_millis(timeout),
            Err(Error::InvalidInput(_))
        ));
    }

    #[test]
    fn session_usage_should_parse_all_counters() {
        let usage = session_usage_from_json(
            r#"{"inputTokens":120,"cachedInputTokens":48,"outputTokens":64,"reasoningTokens":16}"#,
        )
        .expect("session usage JSON should parse");

        assert_eq!(usage.input_tokens, 120);
        assert_eq!(usage.cached_input_tokens, 48);
        assert_eq!(usage.output_tokens, 64);
        assert_eq!(usage.reasoning_tokens, 16);
    }

    #[test]
    fn session_usage_should_reject_negative_counters() {
        let err = session_usage_from_json(
            r#"{"inputTokens":-1,"cachedInputTokens":0,"outputTokens":0,"reasoningTokens":0}"#,
        )
        .expect_err("negative token counts should fail");
        assert!(matches!(err, Error::InternalError(_)));
    }

    #[test]
    fn session_usage_should_reject_invalid_json() {
        let err = session_usage_from_json("not json").expect_err("invalid JSON should fail");
        assert!(matches!(err, Error::InternalError(_)));
    }

    #[test]
    fn response_should_expose_optional_usage() {
        let usage = SessionUsage {
            input_tokens: 10,
            cached_input_tokens: 1,
            output_tokens: 5,
            reasoning_tokens: 0,
        };
        let response = Response::with_usage("ok".to_string(), Some(usage));
        assert_eq!(response.usage(), Some(usage));

        let response = Response::with_usage("ok".to_string(), None);
        assert_eq!(response.usage(), None);
    }

    #[test]
    fn session_usage_delta_should_subtract_counters() {
        let earlier = SessionUsage {
            input_tokens: 100,
            cached_input_tokens: 40,
            output_tokens: 50,
            reasoning_tokens: 10,
        };
        let later = SessionUsage {
            input_tokens: 180,
            cached_input_tokens: 90,
            output_tokens: 75,
            reasoning_tokens: 30,
        };

        let delta = later
            .delta_since(&earlier)
            .expect("in-order snapshots should subtract");
        assert_eq!(delta.input_tokens, 80);
        assert_eq!(delta.cached_input_tokens, 50);
        assert_eq!(delta.output_tokens, 25);
        assert_eq!(delta.reasoning_tokens, 20);
    }

    #[test]
    fn session_usage_delta_should_reject_backwards_counters() {
        let earlier = SessionUsage {
            input_tokens: 100,
            cached_input_tokens: 0,
            output_tokens: 0,
            reasoning_tokens: 0,
        };
        let later = SessionUsage {
            input_tokens: 50,
            cached_input_tokens: 0,
            output_tokens: 0,
            reasoning_tokens: 0,
        };

        let err = later
            .delta_since(&earlier)
            .expect_err("backwards counters should fail");
        assert!(matches!(err, Error::InvalidInput(_)));
    }

    #[test]
    fn attachment_specs_should_index_buffers_and_keep_labels() {
        let bytes = [0_u8, 1, 2, 3];
        let attachments = [
            Attachment::file("/tmp/photo.png").with_label("photo"),
            Attachment::image_bytes(&bytes),
        ];

        let (json, buffers, buffer_lens) =
            attachment_specs(&attachments).expect("attachment specs should serialize");

        assert_eq!(
            json,
            r#"[{"kind":"file","path":"/tmp/photo.png","label":"photo"},{"kind":"data","bufferIndex":0}]"#
        );
        assert_eq!(buffers, vec![bytes.as_ptr()]);
        assert_eq!(buffer_lens, vec![bytes.len()]);
    }

    #[test]
    fn attachment_specs_should_reject_non_utf8_paths() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let path = std::path::PathBuf::from(OsString::from_vec(vec![0x66, 0x6f, 0x80]));
        let attachments = [Attachment::file(path)];
        let err = attachment_specs(&attachments).expect_err("non-UTF-8 path should fail");
        assert!(matches!(err, Error::InvalidInput(_)));
    }

    #[test]
    fn system_tools_should_reject_duplicates() {
        let err = system_tools_json(&[SystemTool::Ocr, SystemTool::Ocr])
            .expect_err("duplicate built-in tools should fail");
        assert!(matches!(err, Error::InvalidInput(_)));
    }
}
