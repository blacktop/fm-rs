//! Python bindings for fm-rs (Apple FoundationModels.framework).
//!
//! This module provides Python bindings for fm-rs, enabling on-device AI
//! via Apple Intelligence from Python.
//!
//! # Platform Requirements
//!
//! - macOS 26.0+, iOS 26.0+, iPadOS 26.0+, visionOS 26.0+, tvOS 26.0+, watchOS 26.0+
//! - Apple Intelligence must be enabled on the device
//! - Device must support Apple Intelligence
//!
//! # Example
//!
//! ```python
//! import fm
//!
//! model = fm.SystemLanguageModel()
//! session = fm.Session(model, instructions="You are helpful.")
//! response = session.respond("Hello!")
//! print(response.content)
//! ```

mod context;
mod error;
mod model;
mod options;
mod response;
mod schema;
mod session;
mod tool;

use pyo3::prelude::*;

/// Python module for Apple FoundationModels.framework bindings.
#[pymodule]
fn fm(m: &Bound<'_, PyModule>) -> PyResult<()> {
    // Register exceptions
    error::register(m)?;

    // Register classes
    m.add_class::<options::Sampling>()?;
    m.add_class::<options::GenerationOptions>()?;
    m.add_class::<response::Response>()?;
    m.add_class::<model::ModelAvailability>()?;
    m.add_class::<model::SystemLanguageModel>()?;
    m.add_class::<session::Session>()?;
    m.add_class::<tool::ToolOutput>()?;
    m.add_class::<context::ContextLimit>()?;
    m.add_class::<context::ContextUsage>()?;
    m.add_class::<schema::Schema>()?;

    // Register functions
    m.add_function(wrap_pyfunction!(context::estimate_tokens, m)?)?;
    m.add_function(wrap_pyfunction!(context::context_usage_from_transcript, m)?)?;
    m.add_function(wrap_pyfunction!(context::transcript_to_text, m)?)?;
    m.add_function(wrap_pyfunction!(context::compact_transcript, m)?)?;

    // Register constants
    m.add("DEFAULT_CONTEXT_TOKENS", context::DEFAULT_CONTEXT_TOKENS)?;

    // Module version
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;

    Ok(())
}
