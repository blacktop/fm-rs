//! Python wrapper for Session.

use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

use pyo3::prelude::*;
use pyo3::types::PyDict;

use crate::context::ContextLimit;
use crate::error::to_py_err;
use crate::model::SystemLanguageModel;
use crate::options::GenerationOptions;
use crate::response::{Response, SessionUsage};
use crate::tool::{dict_to_json, json_to_py, tools_from_python};

/// An image attachment for multimodal prompting (macOS/iOS 27+).
///
/// Example:
///     attachment = `Attachment.file("receipt.png"`, label="receipt")
///     response = `session.respond_with_attachments("What` does it say?", [attachment])
#[pyclass(module = "fm", from_py_object)]
#[derive(Debug, Clone)]
pub struct Attachment {
    source: AttachmentSource,
    label: Option<String>,
}

#[derive(Debug, Clone)]
enum AttachmentSource {
    File(String),
    ImageBytes(Vec<u8>),
}

#[pymethods]
impl Attachment {
    /// Creates an image attachment from a file path.
    #[staticmethod]
    #[pyo3(signature = (path, *, label=None))]
    fn file(path: String, label: Option<String>) -> Self {
        Self {
            source: AttachmentSource::File(path),
            label,
        }
    }

    /// Creates an image attachment from encoded image bytes (PNG, JPEG, HEIC, ...).
    #[staticmethod]
    #[pyo3(signature = (data, *, label=None))]
    fn image_bytes(data: Vec<u8>, label: Option<String>) -> Self {
        Self {
            source: AttachmentSource::ImageBytes(data),
            label,
        }
    }

    fn __repr__(&self) -> String {
        match &self.source {
            AttachmentSource::File(path) => format!("Attachment.file({path:?})"),
            AttachmentSource::ImageBytes(bytes) => {
                format!("Attachment.image_bytes(<{} bytes>)", bytes.len())
            }
        }
    }
}

/// Converts a Python system tool name to the fm-rs enum.
fn system_tool_from_name(name: &str) -> PyResult<fm_rs::SystemTool> {
    match name {
        "ocr" => Ok(fm_rs::SystemTool::Ocr),
        "barcode_reader" => Ok(fm_rs::SystemTool::BarcodeReader),
        "spotlight_search" => Ok(fm_rs::SystemTool::SpotlightSearch),
        other => Err(pyo3::exceptions::PyValueError::new_err(format!(
            "Unknown system tool '{other}'; expected 'ocr', 'barcode_reader', or 'spotlight_search'"
        ))),
    }
}

/// Resolves generation options, using defaults if not provided.
fn resolve_options(options: Option<&GenerationOptions>) -> fm_rs::GenerationOptions {
    options.map_or_else(fm_rs::GenerationOptions::default, |o| o.inner().clone())
}

/// Runs a Python callback in a separate thread, receiving chunks via channel.
///
/// Returns a join handle that resolves to the callback result.
fn spawn_callback_thread(
    rx: mpsc::Receiver<String>,
    on_chunk: Py<PyAny>,
) -> std::thread::JoinHandle<PyResult<()>> {
    std::thread::spawn(move || {
        for chunk in rx {
            Python::attach(|py| on_chunk.call1(py, (chunk.as_str(),)))?;
        }
        Ok(())
    })
}

/// Waits for callback thread and combines results with stream result.
fn finalize_stream(
    callback_handle: std::thread::JoinHandle<PyResult<()>>,
    stream_result: Result<(), fm_rs::Error>,
) -> PyResult<()> {
    let callback_result = callback_handle
        .join()
        .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("Callback thread panicked"))?;

    stream_result.map_err(to_py_err)?;
    callback_result
}

/// A session that interacts with a language model.
///
/// A session maintains state between requests, allowing for multi-turn conversations.
///
/// Note: Session is not thread-safe and must be used from a single thread.
/// If you need to share a session across threads, use Python's threading locks.
///
/// Example:
///     model = `SystemLanguageModel()`
///     session = Session(model, instructions="You are helpful.")
///     response = session.respond("Hello!")
///     print(response.content)
#[pyclass(module = "fm", unsendable)]
pub struct Session {
    inner: fm_rs::Session,
    // Keep tools alive for the session lifetime
    #[allow(dead_code)]
    tools: Vec<Arc<dyn fm_rs::Tool>>,
}

#[pymethods]
impl Session {
    /// Creates a new session with the given model.
    ///
    /// Args:
    ///     model: The `SystemLanguageModel` to use.
    ///     instructions: Optional instructions that define the model's behavior and role.
    ///     tools: Optional list of tool objects. Each tool must have name, description,
    ///            `arguments_schema` attributes and a call(args) method.
    ///     `system_tools`: Optional list of built-in Apple tool names
    ///         ("ocr", "`barcode_reader`", "`spotlight_search`"); macOS/iOS 27+.
    ///
    /// Raises:
    ///     `FmError`: If session creation fails.
    ///     `UnsupportedPlatformError`: If `system_tools` are requested on a
    ///         pre-27 build SDK or runtime.
    #[new]
    #[pyo3(signature = (model, *, instructions=None, tools=None, system_tools=None))]
    fn new(
        py: Python<'_>,
        model: &SystemLanguageModel,
        instructions: Option<&str>,
        tools: Option<Vec<Py<PyAny>>>,
        system_tools: Option<Vec<String>>,
    ) -> PyResult<Self> {
        let rust_tools = match tools {
            Some(py_tools) if !py_tools.is_empty() => tools_from_python(py, py_tools)?,
            _ => Vec::new(),
        };

        let mut builder = fm_rs::Session::builder(model.inner().as_ref());
        if let Some(instructions) = instructions {
            builder = builder.instructions(instructions);
        }
        for tool in &rust_tools {
            builder = builder.tool(Arc::clone(tool));
        }
        for name in system_tools.into_iter().flatten() {
            builder = builder.system_tool(system_tool_from_name(&name)?);
        }
        let inner = builder.build().map_err(to_py_err)?;

        Ok(Self {
            inner,
            tools: rust_tools,
        })
    }

    /// Creates a session from a transcript JSON string.
    ///
    /// This allows restoring a previous conversation.
    /// Note: Restored sessions do not have tools - use `new()` with tools for new sessions.
    ///
    /// Args:
    ///     model: The `SystemLanguageModel` to use.
    ///     `transcript_json`: JSON string of the conversation transcript.
    ///
    /// Returns:
    ///     Session: A session restored from the transcript.
    #[staticmethod]
    fn from_transcript(model: &SystemLanguageModel, transcript_json: &str) -> PyResult<Self> {
        let inner = fm_rs::Session::from_transcript(model.inner().as_ref(), transcript_json)
            .map_err(to_py_err)?;
        Ok(Self {
            inner,
            tools: Vec::new(),
        })
    }

    /// Sends a prompt and waits for the complete response.
    ///
    /// Args:
    ///     prompt: The text prompt to send.
    ///     options: Optional generation options.
    ///
    /// Returns:
    ///     Response: The model's response.
    ///
    /// Raises:
    ///     `GenerationError`: If generation fails.
    #[pyo3(signature = (prompt, options=None))]
    fn respond(
        &self,
        py: Python<'_>,
        prompt: &str,
        options: Option<&GenerationOptions>,
    ) -> PyResult<Response> {
        let opts = resolve_options(options);
        let inner_addr = std::ptr::from_ref(&self.inner) as usize;
        let response = py
            .detach(|| {
                let inner = unsafe { &*(inner_addr as *const fm_rs::Session) };
                inner.respond(prompt, &opts)
            })
            .map_err(to_py_err)?;
        Ok(Response::from_inner(response))
    }

    /// Sends a prompt and waits for the complete response, with a timeout.
    ///
    /// Args:
    ///     prompt: The text prompt to send.
    ///     `timeout_secs`: Timeout in seconds.
    ///     options: Optional generation options.
    ///
    /// Returns:
    ///     Response: The model's response.
    ///
    /// Raises:
    ///     `TimeoutError`: If the operation times out.
    ///     `GenerationError`: If generation fails.
    #[pyo3(signature = (prompt, timeout_secs, options=None))]
    fn respond_with_timeout(
        &self,
        py: Python<'_>,
        prompt: &str,
        timeout_secs: f64,
        options: Option<&GenerationOptions>,
    ) -> PyResult<Response> {
        let max_secs = Duration::MAX.as_secs_f64();
        if !timeout_secs.is_finite() || timeout_secs < 0.0 || timeout_secs > max_secs {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "timeout_secs must be finite, non-negative, and within the supported range",
            ));
        }
        let opts = resolve_options(options);
        let timeout = Duration::from_secs_f64(timeout_secs);
        let inner_addr = std::ptr::from_ref(&self.inner) as usize;
        let response = py
            .detach(|| {
                let inner = unsafe { &*(inner_addr as *const fm_rs::Session) };
                inner.respond_with_timeout(prompt, &opts, timeout)
            })
            .map_err(to_py_err)?;
        Ok(Response::from_inner(response))
    }

    /// Sends a prompt with image attachments (macOS/iOS 27+).
    ///
    /// Args:
    ///     prompt: The text prompt to send.
    ///     attachments: List of Attachment objects (file paths or image bytes).
    ///     options: Optional generation options.
    ///
    /// Returns:
    ///     Response: The model's response.
    ///
    /// Raises:
    ///     `UnsupportedPlatformError`: On pre-27 build SDKs or runtimes.
    ///     `UnsupportedCapabilityError`: If the model cannot process images.
    #[pyo3(signature = (prompt, attachments, options=None))]
    fn respond_with_attachments(
        &self,
        py: Python<'_>,
        prompt: &str,
        attachments: Vec<Attachment>,
        options: Option<&GenerationOptions>,
    ) -> PyResult<Response> {
        let opts = resolve_options(options);
        let inner_addr = std::ptr::from_ref(&self.inner) as usize;
        let response = py
            .detach(move || {
                let inner = unsafe { &*(inner_addr as *const fm_rs::Session) };
                let converted: Vec<fm_rs::Attachment<'_>> = attachments
                    .iter()
                    .map(|attachment| {
                        let built = match &attachment.source {
                            AttachmentSource::File(path) => fm_rs::Attachment::file(path.as_str()),
                            AttachmentSource::ImageBytes(bytes) => {
                                fm_rs::Attachment::image_bytes(bytes)
                            }
                        };
                        match &attachment.label {
                            Some(label) => built.with_label(label.clone()),
                            None => built,
                        }
                    })
                    .collect();
                inner.respond_with_attachments(prompt, &converted, &opts)
            })
            .map_err(to_py_err)?;
        Ok(Response::from_inner(response))
    }

    /// Returns exact cumulative token usage for this session (macOS/iOS 27+).
    ///
    /// Returns:
    ///     `SessionUsage`: Input, cached-input, output, and reasoning token counts.
    ///
    /// Raises:
    ///     `UnsupportedPlatformError`: On pre-27 build SDKs or runtimes.
    fn usage(&self) -> PyResult<SessionUsage> {
        self.inner
            .usage()
            .map(SessionUsage::from_inner)
            .map_err(to_py_err)
    }

    /// Replaces this session's transcript (macOS/iOS 27+).
    ///
    /// Args:
    ///     `transcript_json`: A Foundation Models transcript JSON string,
    ///         such as one returned by `transcript_json()` and edited.
    ///
    /// Raises:
    ///     `UnsupportedPlatformError`: On pre-27 build SDKs or runtimes.
    ///     `ValueError`: If the transcript is malformed or the session is responding.
    fn set_transcript(&self, transcript_json: &str) -> PyResult<()> {
        self.inner
            .set_transcript(transcript_json)
            .map_err(to_py_err)
    }

    /// Sets or clears the transcript error handling policy (macOS/iOS 27+).
    ///
    /// Args:
    ///     policy: "revert", "preserve", or None to restore the framework default.
    ///
    /// Raises:
    ///     `UnsupportedPlatformError`: On pre-27 build SDKs or runtimes.
    #[pyo3(signature = (policy))]
    fn set_transcript_error_handling_policy(&self, policy: Option<&str>) -> PyResult<()> {
        let policy = match policy {
            None => None,
            Some("revert") => Some(fm_rs::TranscriptErrorHandlingPolicy::RevertTranscript),
            Some("preserve") => Some(fm_rs::TranscriptErrorHandlingPolicy::PreserveTranscript),
            Some(other) => {
                return Err(pyo3::exceptions::PyValueError::new_err(format!(
                    "policy must be 'revert', 'preserve', or None, got '{other}'"
                )));
            }
        };
        self.inner
            .set_transcript_error_handling_policy(policy)
            .map_err(to_py_err)
    }

    /// Sends a prompt and streams the response.
    ///
    /// The `on_chunk` callback receives incremental text deltas as they arrive;
    /// concatenating the callback values yields the complete response.
    /// This method blocks until streaming is complete.
    ///
    /// Note: The callback may be invoked from a non-main thread.
    /// Avoid heavy work or UI operations in the callback.
    ///
    /// Args:
    ///     prompt: The text prompt to send.
    ///     `on_chunk`: A callable that receives each text chunk.
    ///     options: Optional generation options.
    ///
    /// Raises:
    ///     `GenerationError`: If streaming fails or is cancelled.
    #[pyo3(signature = (prompt, on_chunk, options=None))]
    #[allow(clippy::needless_pass_by_value)]
    fn stream_response(
        &self,
        py: Python<'_>,
        prompt: &str,
        on_chunk: Py<PyAny>,
        options: Option<&GenerationOptions>,
    ) -> PyResult<()> {
        let opts = resolve_options(options);
        let (tx, rx) = mpsc::channel::<String>();
        let callback_handle = spawn_callback_thread(rx, on_chunk);

        // Call Swift FFI on the main thread with GIL released.
        // Swift's Task.detached requires proper runtime context.
        let inner_addr = std::ptr::from_ref(&self.inner) as usize;
        let stream_result = py.detach(|| {
            let inner = unsafe { &*(inner_addr as *const fm_rs::Session) };
            inner.stream_response(prompt, &opts, move |chunk: &str| {
                let _ = tx.send(chunk.to_string());
            })
        });

        finalize_stream(callback_handle, stream_result)
    }

    /// Sends a prompt and returns a structured JSON response.
    ///
    /// Args:
    ///     prompt: The text prompt to send.
    ///     schema: A JSON Schema dict describing the expected output format.
    ///     options: Optional generation options.
    ///
    /// Returns:
    ///     dict: The parsed JSON response as a Python dict.
    ///
    /// Raises:
    ///     `GenerationError`: If generation fails.
    ///     `JsonError`: If JSON parsing fails.
    #[pyo3(signature = (prompt, schema, options=None))]
    fn respond_structured<'py>(
        &self,
        py: Python<'py>,
        prompt: &str,
        schema: &Bound<'py, PyDict>,
        options: Option<&GenerationOptions>,
    ) -> PyResult<Py<PyAny>> {
        let opts = resolve_options(options);
        let schema_json = dict_to_json(py, schema)?;

        let inner_addr = std::ptr::from_ref(&self.inner) as usize;
        let json_str = py
            .detach(|| {
                let inner = unsafe { &*(inner_addr as *const fm_rs::Session) };
                inner.respond_json(prompt, &schema_json, &opts)
            })
            .map_err(to_py_err)?;

        let value: serde_json::Value = serde_json::from_str(&json_str).map_err(|e| {
            pyo3::exceptions::PyValueError::new_err(format!("Invalid JSON response: {e}"))
        })?;
        json_to_py(py, &value)
    }

    /// Sends a prompt and returns a raw JSON string response.
    ///
    /// Args:
    ///     prompt: The text prompt to send.
    ///     schema: A JSON Schema dict describing the expected output format.
    ///     options: Optional generation options.
    ///
    /// Returns:
    ///     str: The raw JSON string response.
    ///
    /// Raises:
    ///     `GenerationError`: If generation fails.
    #[pyo3(signature = (prompt, schema, options=None))]
    fn respond_json(
        &self,
        py: Python<'_>,
        prompt: &str,
        schema: &Bound<'_, PyDict>,
        options: Option<&GenerationOptions>,
    ) -> PyResult<String> {
        let opts = resolve_options(options);
        let schema_json = dict_to_json(py, schema)?;

        let inner_addr = std::ptr::from_ref(&self.inner) as usize;
        py.detach(|| {
            let inner = unsafe { &*(inner_addr as *const fm_rs::Session) };
            inner.respond_json(prompt, &schema_json, &opts)
        })
        .map_err(to_py_err)
    }

    /// Streams a structured JSON response.
    ///
    /// The `on_chunk` callback receives the cumulative partial JSON generated
    /// so far, not an incremental delta.
    /// Note that partial chunks may not be valid JSON until streaming completes.
    ///
    /// Args:
    ///     prompt: The text prompt to send.
    ///     schema: A JSON Schema dict describing the expected output format.
    ///     `on_chunk`: A callable that receives each JSON chunk.
    ///     options: Optional generation options.
    ///
    /// Raises:
    ///     `GenerationError`: If streaming fails or is cancelled.
    #[pyo3(signature = (prompt, schema, on_chunk, options=None))]
    #[allow(clippy::needless_pass_by_value)]
    fn stream_json(
        &self,
        py: Python<'_>,
        prompt: &str,
        schema: &Bound<'_, PyDict>,
        on_chunk: Py<PyAny>,
        options: Option<&GenerationOptions>,
    ) -> PyResult<()> {
        let opts = resolve_options(options);
        let schema_json = dict_to_json(py, schema)?;
        let (tx, rx) = mpsc::channel::<String>();
        let callback_handle = spawn_callback_thread(rx, on_chunk);

        // Call Swift FFI on the main thread with GIL released.
        // Swift's Task.detached requires proper runtime context.
        let inner_addr = std::ptr::from_ref(&self.inner) as usize;
        let stream_result = py.detach(|| {
            let inner = unsafe { &*(inner_addr as *const fm_rs::Session) };
            inner.stream_json(prompt, &schema_json, &opts, move |chunk: &str| {
                let _ = tx.send(chunk.to_string());
            })
        });

        finalize_stream(callback_handle, stream_result)
    }

    /// Cancels an ongoing stream operation.
    fn cancel(&self) {
        self.inner.cancel();
    }

    /// Returns True if the session is currently generating a response.
    #[getter]
    fn is_responding(&self) -> bool {
        self.inner.is_responding()
    }

    /// Gets the session transcript as a JSON string.
    ///
    /// This can be used to persist and restore conversations.
    ///
    /// Returns:
    ///     str: The transcript as a JSON string.
    #[getter]
    fn transcript_json(&self) -> PyResult<String> {
        self.inner.transcript_json().map_err(to_py_err)
    }

    /// Estimates current context usage based on the session transcript.
    ///
    /// Args:
    ///     limit: The context limit configuration.
    ///
    /// Returns:
    ///     `ContextUsage`: The estimated context usage.
    fn context_usage(&self, limit: &ContextLimit) -> PyResult<crate::context::ContextUsage> {
        let rust_limit = limit.to_inner();
        let usage = self.inner.context_usage(&rust_limit).map_err(to_py_err)?;
        Ok(crate::context::ContextUsage::from_inner(usage))
    }

    /// Prewarms the model with an optional prompt prefix.
    ///
    /// This can reduce latency for the first response.
    ///
    /// Args:
    ///     `prompt_prefix`: Optional text to prewarm with.
    #[pyo3(signature = (prompt_prefix=None))]
    fn prewarm(&self, prompt_prefix: Option<&str>) -> PyResult<()> {
        self.inner.prewarm(prompt_prefix).map_err(to_py_err)
    }

    fn __repr__(&self) -> String {
        let responding = if self.inner.is_responding() {
            "responding"
        } else {
            "idle"
        };
        let tools = self.tools.len();
        format!("Session(status={responding}, tools={tools})")
    }
}
