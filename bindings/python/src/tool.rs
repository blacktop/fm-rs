//! Python tool protocol adapter.
//!
//! Tools allow the model to call external functions during generation.
//! Python tools are duck-typed objects with name, description, `arguments_schema`, and `call()`.

use std::sync::Arc;

use pyo3::prelude::*;
use pyo3::types::PyDict;
use serde_json::Value;

/// Output returned by a tool invocation.
#[pyclass(module = "fm")]
#[derive(Debug, Clone)]
pub struct ToolOutput {
    /// The content returned by the tool.
    #[pyo3(get)]
    pub content: String,
}

#[pymethods]
impl ToolOutput {
    /// Creates a new tool output with the given content.
    #[new]
    fn new(content: String) -> Self {
        Self { content }
    }

    fn __repr__(&self) -> String {
        let preview: String = self.content.chars().take(50).collect();
        let preview = if self.content.chars().count() > 50 {
            format!("{preview}...")
        } else {
            preview
        };
        format!("ToolOutput(content={preview:?})")
    }
}

impl From<ToolOutput> for fm_rs::ToolOutput {
    fn from(output: ToolOutput) -> Self {
        fm_rs::ToolOutput::new(output.content)
    }
}

/// Adapter that wraps a Python tool object to implement fm-rs Tool trait.
///
/// The Python object must have:
/// - `name`: str - The name of the tool
/// - `description`: str - A description of what the tool does
/// - `arguments_schema`: dict - JSON Schema for the tool's arguments
/// - `call(args: dict) -> str | ToolOutput` - Invokes the tool
pub struct PyToolAdapter {
    tool: Py<PyAny>,
    name: String,
    description: String,
    arguments_schema: Value,
}

impl PyToolAdapter {
    /// Creates a new `PyToolAdapter` from a Python tool object.
    ///
    /// # Errors
    ///
    /// Returns an error if the Python object doesn't have the required attributes.
    pub fn new(py: Python<'_>, tool: Py<PyAny>) -> PyResult<Self> {
        let obj = tool.bind(py);

        // Extract name
        let name: String = obj.getattr("name")?.extract()?;

        // Extract description
        let description: String = obj.getattr("description")?.extract()?;

        // Extract arguments_schema and convert to serde_json::Value
        let schema_py = obj.getattr("arguments_schema")?;
        let arguments_schema = py_to_json(py, &schema_py)?;

        Ok(Self {
            tool,
            name,
            description,
            arguments_schema,
        })
    }
}

impl fm_rs::Tool for PyToolAdapter {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn arguments_schema(&self) -> Value {
        self.arguments_schema.clone()
    }

    fn call(&self, arguments: Value) -> fm_rs::Result<fm_rs::ToolOutput> {
        // Acquire GIL and call the Python tool
        Python::attach(|py| {
            let args_py = json_to_py(py, &arguments)
                .map_err(|e| fm_rs::Error::InternalError(format!("Failed to convert args: {e}")))?;

            let result = self
                .tool
                .bind(py)
                .call_method1("call", (args_py,))
                .map_err(|e| fm_rs::Error::InternalError(format!("Tool call failed: {e}")))?;

            // Handle return value - can be str or ToolOutput
            if let Ok(output) = result.extract::<ToolOutput>() {
                Ok(fm_rs::ToolOutput::new(output.content))
            } else if let Ok(content) = result.extract::<String>() {
                Ok(fm_rs::ToolOutput::new(content))
            } else {
                // Try to convert to string
                let content = result.str().map_err(|e| {
                    fm_rs::Error::InternalError(format!(
                        "Tool returned invalid type (expected str or ToolOutput): {e}"
                    ))
                })?;
                Ok(fm_rs::ToolOutput::new(content.to_string()))
            }
        })
    }
}

// SAFETY: PyToolAdapter uses Py<PyAny> which is Send+Sync.
// All Python interactions acquire the GIL properly.
unsafe impl Send for PyToolAdapter {}
unsafe impl Sync for PyToolAdapter {}

/// Converts Python tools to fm-rs Tool trait objects.
pub fn tools_from_python(
    py: Python<'_>,
    tools: Vec<Py<PyAny>>,
) -> PyResult<Vec<Arc<dyn fm_rs::Tool>>> {
    tools
        .into_iter()
        .map(|tool| {
            let adapter = PyToolAdapter::new(py, tool)?;
            Ok(Arc::new(adapter) as Arc<dyn fm_rs::Tool>)
        })
        .collect()
}

/// Converts a Python object to `serde_json::Value`.
fn py_to_json(py: Python<'_>, obj: &Bound<'_, PyAny>) -> PyResult<Value> {
    // Use Python's json module for reliable conversion
    let json_mod = py.import("json")?;
    let json_str: String = json_mod.call_method1("dumps", (obj,))?.extract()?;
    serde_json::from_str(&json_str)
        .map_err(|e| pyo3::exceptions::PyValueError::new_err(format!("Invalid JSON: {e}")))
}

/// Converts `serde_json::Value` to a Python object.
pub fn json_to_py(py: Python<'_>, value: &Value) -> PyResult<Py<PyAny>> {
    let json_str = serde_json::to_string(value).map_err(|e| {
        pyo3::exceptions::PyValueError::new_err(format!("Failed to serialize JSON: {e}"))
    })?;
    let json_mod = py.import("json")?;
    let result = json_mod.call_method1("loads", (json_str,))?;
    Ok(result.unbind())
}

/// Converts a Python dict to `serde_json::Value`.
pub fn dict_to_json(py: Python<'_>, dict: &Bound<'_, PyDict>) -> PyResult<Value> {
    py_to_json(py, dict.as_any())
}
