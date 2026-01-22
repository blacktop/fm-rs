//! Python wrapper for Response.

use pyo3::prelude::*;

/// Response returned by the model.
#[pyclass(module = "fm")]
#[derive(Debug, Clone)]
pub struct Response {
    content: String,
}

#[pymethods]
impl Response {
    /// Gets the text content of the response.
    #[getter]
    fn content(&self) -> &str {
        &self.content
    }

    fn __repr__(&self) -> String {
        let preview: String = self.content.chars().take(50).collect();
        let preview = if self.content.chars().count() > 50 {
            format!("{preview}...")
        } else {
            preview
        };
        format!("Response(content={preview:?})")
    }

    fn __str__(&self) -> &str {
        &self.content
    }

    fn __len__(&self) -> usize {
        self.content.chars().count()
    }
}

impl Response {
    /// Creates a new Response from an fm-rs Response.
    pub fn from_inner(inner: fm_rs::Response) -> Self {
        Self {
            content: inner.into_content(),
        }
    }

    /// Creates a new Response from a string.
    pub fn from_string(content: String) -> Self {
        Self { content }
    }
}
