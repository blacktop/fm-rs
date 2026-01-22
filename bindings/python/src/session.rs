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
use crate::response::Response;
use crate::tool::{dict_to_json, json_to_py, tools_from_python};

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
    ///
    /// Raises:
    ///     `FmError`: If session creation fails.
    #[new]
    #[pyo3(signature = (model, *, instructions=None, tools=None))]
    fn new(
        py: Python<'_>,
        model: &SystemLanguageModel,
        instructions: Option<&str>,
        tools: Option<Vec<Py<PyAny>>>,
    ) -> PyResult<Self> {
        let rust_tools = match tools {
            Some(py_tools) if !py_tools.is_empty() => tools_from_python(py, py_tools)?,
            _ => Vec::new(),
        };

        let has_tools = !rust_tools.is_empty();
        let inner = match (instructions, has_tools) {
            (Some(inst), true) => {
                fm_rs::Session::with_instructions_and_tools(model.inner(), inst, &rust_tools)
            }
            (Some(inst), false) => fm_rs::Session::with_instructions(model.inner(), inst),
            (None, true) => fm_rs::Session::with_tools(model.inner(), &rust_tools),
            (None, false) => fm_rs::Session::new(model.inner()),
        }
        .map_err(to_py_err)?;

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
        let inner =
            fm_rs::Session::from_transcript(model.inner(), transcript_json).map_err(to_py_err)?;
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

    /// Sends a prompt and streams the response.
    ///
    /// The `on_chunk` callback is called for each text chunk as it arrives.
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
    ///     `GenerationError`: If streaming fails.
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
    /// The `on_chunk` callback receives partial JSON as it's generated.
    /// Note that partial chunks may not be valid JSON until streaming completes.
    ///
    /// Args:
    ///     prompt: The text prompt to send.
    ///     schema: A JSON Schema dict describing the expected output format.
    ///     `on_chunk`: A callable that receives each JSON chunk.
    ///     options: Optional generation options.
    ///
    /// Raises:
    ///     `GenerationError`: If streaming fails.
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
