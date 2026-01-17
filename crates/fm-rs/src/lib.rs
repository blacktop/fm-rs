//! Rust bindings for Apple's FoundationModels.framework
//!
//! This crate provides safe, idiomatic Rust bindings to Apple's `FoundationModels` framework,
//! which enables on-device AI capabilities powered by Apple Intelligence.
//!
//! # Platform Requirements
//!
//! - **Minimum OS**: macOS 26.0+, iOS 26.0+, iPadOS 26.0+, visionOS 26.0+, tvOS 26.0+, watchOS 26.0+
//! - **Apple Intelligence**: Must be enabled on the device
//! - **Device**: Must support Apple Intelligence
//!
//! # Quick Start
//!
//! ```rust,no_run
//! use fm_rs::{SystemLanguageModel, Session, GenerationOptions};
//!
//! // Create the default system language model
//! let model = SystemLanguageModel::new()?;
//!
//! // Check availability
//! if !model.is_available() {
//!     println!("FoundationModels is not available on this device");
//!     return Ok(());
//! }
//!
//! // Create a session with instructions
//! let session = Session::with_instructions(&model, "You are a helpful assistant.")?;
//!
//! // Send a prompt
//! let options = GenerationOptions::builder()
//!     .temperature(0.7)
//!     .build();
//! let response = session.respond("What is the capital of France?", &options)?;
//!
//! println!("Response: {}", response.content());
//! # Ok::<(), fm_rs::Error>(())
//! ```
//!
//! # Streaming Responses
//!
//! ```rust,no_run
//! use fm_rs::{SystemLanguageModel, Session, GenerationOptions};
//!
//! let model = SystemLanguageModel::new()?;
//! let session = Session::new(&model)?;
//!
//! session.stream_response("Tell me a story", &GenerationOptions::default(), |chunk| {
//!     print!("{}", chunk);
//! })?;
//! # Ok::<(), fm_rs::Error>(())
//! ```
//!
//! # Tool Calling
//!
//! ```rust,no_run
//! use fm_rs::{SystemLanguageModel, Session, Tool, ToolOutput, GenerationOptions};
//! use serde_json::{json, Value};
//! use std::sync::Arc;
//!
//! struct WeatherTool;
//!
//! impl Tool for WeatherTool {
//!     fn name(&self) -> &str { "get_weather" }
//!     fn description(&self) -> &str { "Gets the current weather for a location" }
//!     fn arguments_schema(&self) -> Value {
//!         json!({
//!             "type": "object",
//!             "properties": {
//!                 "location": { "type": "string" }
//!             },
//!             "required": ["location"]
//!         })
//!     }
//!     fn call(&self, args: Value) -> fm_rs::Result<ToolOutput> {
//!         let location = args["location"].as_str().unwrap_or("Unknown");
//!         Ok(ToolOutput::new(format!("Sunny, 72°F in {location}")))
//!     }
//! }
//!
//! let model = SystemLanguageModel::new()?;
//! let tools: Vec<Arc<dyn Tool>> = vec![Arc::new(WeatherTool)];
//! let session = Session::with_tools(&model, &tools)?;
//!
//! let response = session.respond("What's the weather in Paris?", &GenerationOptions::default())?;
//! # Ok::<(), fm_rs::Error>(())
//! ```
//!
//! # Resource Management
//!
//! ## Session Lifecycle
//!
//! - **Ownership**: [`Session`] owns its underlying Swift `LanguageModelSession`. When
//!   a `Session` is dropped, the Swift session is freed.
//! - **Thread Safety**: Both [`SystemLanguageModel`] and [`Session`] are `Send + Sync`.
//!   They can be shared across threads, though concurrent calls to the same session
//!   may block waiting for the model.
//! - **Drop Behavior**: Dropping a `Session` with active tool callbacks will wait
//!   (up to 1 second) for in-flight callbacks to complete before freeing resources.
//!
//! ## Memory Considerations
//!
//! - Sessions maintain conversation history in memory. Long conversations can consume
//!   significant memory. Use [`Session::transcript_json()`] to persist and
//!   [`Session::from_transcript()`] to restore conversations.
//! - Monitor context usage with [`context_usage_from_transcript()`] and implement
//!   compaction strategies using [`compact_transcript()`] when approaching limits.
//! - The default context window is approximately [`DEFAULT_CONTEXT_TOKENS`] tokens,
//!   though this may vary by device and model version.
//!
//! ## Blocking Operations
//!
//! - [`Session::respond()`] blocks until generation completes. For long generations,
//!   consider using [`Session::respond_with_timeout()`] or [`Session::stream_response()`].
//! - Tool callbacks are invoked synchronously during generation. Long-running tools
//!   will block the generation pipeline.
//!
//! ## No Persistence Across Restarts
//!
//! Sessions do not persist across process restarts. To resume a conversation:
//! 1. Save the transcript with [`Session::transcript_json()`] before shutdown
//! 2. Restore with [`Session::from_transcript()`] on next launch
//!
//! ## Error Recovery
//!
//! - If a session enters an error state, create a new session rather than retrying.
//! - Tool errors are reported via [`Error::ToolCall`] with context about which tool
//!   failed and the arguments that were passed.

#![warn(missing_docs)]
#![warn(clippy::all)]

mod context;
mod error;
mod ffi;
mod model;
mod options;
mod session;
mod tool;

/// Trait for types that can provide a JSON Schema for structured generation.
pub trait Generable {
    /// Returns a JSON Schema describing the type.
    fn schema() -> serde_json::Value;
}

/// Re-export the derive macro when the `derive` feature is enabled.
#[cfg(feature = "derive")]
pub use fm_rs_derive::Generable;

/// Re-export `serde_json` so derive macro output doesn't require a direct dependency.
///
/// Named `__serde_json` to avoid namespace conflicts with serde's internal derive paths.
#[doc(hidden)]
pub use serde_json as __serde_json;

// Re-export public API
pub use crate::context::{
    CompactionConfig, ContextLimit, ContextUsage, DEFAULT_CONTEXT_TOKENS, compact_transcript,
    context_usage_from_transcript, estimate_tokens, transcript_to_text,
};
pub use crate::error::{Error, Result, ToolCallError};
pub use crate::model::{ModelAvailability, SystemLanguageModel};
pub use crate::options::{GenerationOptions, GenerationOptionsBuilder, Sampling};
pub use crate::session::{Response, Session};
pub use crate::tool::{Tool, ToolOutput};

// FFI exports for Swift to call back into Rust

/// Frees a string allocated by Rust (via `CString::into_raw`).
///
/// This function must be called by Swift to properly deallocate strings
/// returned from Rust callbacks (e.g., tool callback results).
///
/// # Safety
///
/// The pointer must have been allocated by Rust using `CString::into_raw()`.
/// Passing a pointer from any other source (e.g., Swift's strdup) is undefined behavior.
///
/// This function is called from Swift via FFI. The clippy `not_unsafe_ptr_arg_deref`
/// lint is allowed because the unsafety contract is on the FFI caller side (Swift).
#[unsafe(no_mangle)]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn fm_rust_string_free(s: *mut std::ffi::c_char) {
    if !s.is_null() {
        // SAFETY: The pointer was created by CString::into_raw() in Rust,
        // as documented in the function contract.
        unsafe {
            drop(std::ffi::CString::from_raw(s));
        }
    }
}
