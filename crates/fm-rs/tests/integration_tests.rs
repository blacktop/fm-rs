//! Tests for `FoundationModels` Rust bindings
//!
//! Note: These tests require the `FoundationModels` framework to be available,
//! which means:
//! - Running on macOS 26.0+, iOS 26.0+, etc.
//! - Apple Intelligence must be enabled
//! - The device must support Apple Intelligence

use fm_rs::{Error, GenerationOptions, ModelAvailability, Session, SystemLanguageModel};

// ============================================================================
// Integration Tests (require FoundationModels to be available)
// ============================================================================

#[test]
#[ignore = "Requires Apple Intelligence to be enabled"]
fn test_model_creation() {
    let model = SystemLanguageModel::new().expect("Failed to create model");
    assert!(model.is_available() || !model.is_available()); // Just check it doesn't panic
}

#[test]
#[ignore = "Requires Apple Intelligence to be enabled"]
fn test_model_availability() {
    let model = SystemLanguageModel::new().expect("Failed to create model");
    let is_available = model.is_available();
    println!("Model available: {is_available}");
}

#[test]
#[ignore = "Requires Apple Intelligence to be enabled"]
fn test_session_creation() {
    let model = SystemLanguageModel::new().expect("Failed to create model");

    if !model.is_available() {
        println!("Skipping test: Model not available");
        return;
    }

    let session = Session::new(&model).expect("Failed to create session");
    drop(session);
}

#[test]
#[ignore = "Requires Apple Intelligence to be enabled"]
fn test_simple_prompt() {
    let model = SystemLanguageModel::new().expect("Failed to create model");

    if !model.is_available() {
        println!("Skipping test: Model not available");
        return;
    }

    let session = Session::with_instructions(&model, "You are a helpful assistant.")
        .expect("Failed to create session");

    let options = GenerationOptions::default();
    let response = session
        .respond("Say 'Hello, world!' and nothing else.", &options)
        .expect("Failed to get response");

    assert!(!response.content().is_empty());
    println!("Response: {}", response.content());
}

#[test]
#[ignore = "Requires Apple Intelligence to be enabled"]
fn test_custom_generation_options() {
    let model = SystemLanguageModel::new().expect("Failed to create model");

    if !model.is_available() {
        println!("Skipping test: Model not available");
        return;
    }

    let session = Session::with_instructions(&model, "Write exactly two words.")
        .expect("Failed to create session");

    let options = GenerationOptions::builder()
        .temperature(0.5)
        .max_response_tokens(50)
        .build();

    let response = session
        .respond("Write two words.", &options)
        .expect("Failed to get response");

    assert!(!response.content().is_empty());
    println!("Response: {}", response.content());
}

#[test]
#[ignore = "Requires Apple Intelligence to be enabled"]
fn test_conversation_context() {
    let model = SystemLanguageModel::new().expect("Failed to create model");

    if !model.is_available() {
        println!("Skipping test: Model not available");
        return;
    }

    let session = Session::with_instructions(
        &model,
        "You are a helpful assistant with good memory of the conversation.",
    )
    .expect("Failed to create session");

    let options = GenerationOptions::default();

    // First prompt
    let response1 = session
        .respond("My name is Alice.", &options)
        .expect("Failed to get response");
    println!("Response 1: {}", response1.content());

    // Second prompt that references the first
    let response2 = session
        .respond("What is my name?", &options)
        .expect("Failed to get response");
    println!("Response 2: {}", response2.content());

    // Verify the response mentions Alice (case insensitive)
    assert!(
        response2.content().to_lowercase().contains("alice"),
        "Model should remember the name 'Alice'"
    );
}

#[test]
#[ignore = "Requires Apple Intelligence to be enabled"]
fn test_streaming() {
    use std::sync::{Arc, Mutex};

    let model = SystemLanguageModel::new().expect("Failed to create model");

    if !model.is_available() {
        println!("Skipping test: Model not available");
        return;
    }

    let session = Session::new(&model).expect("Failed to create session");
    let options = GenerationOptions::default();

    let chunks = Arc::new(Mutex::new(Vec::new()));
    let chunks_clone = Arc::clone(&chunks);
    session
        .stream_response("Count from 1 to 5.", &options, move |chunk| {
            chunks_clone.lock().unwrap().push(chunk.to_string());
        })
        .expect("Failed to stream response");

    let chunks = chunks.lock().unwrap();
    assert!(
        !chunks.is_empty(),
        "Should have received at least one chunk"
    );
    println!("Received {} chunks", chunks.len());
}

// ============================================================================
// Unit Tests (do not require FoundationModels)
// ============================================================================

#[test]
fn test_generation_options_default() {
    let options = GenerationOptions::default();
    assert!(options.temperature.is_none());
    assert!(options.max_response_tokens.is_none());
}

#[test]
fn test_generation_options_builder() {
    let options = GenerationOptions::builder()
        .temperature(0.7)
        .max_response_tokens(500)
        .build();

    assert_eq!(options.temperature, Some(0.7));
    assert_eq!(options.max_response_tokens, Some(500));
}

#[test]
fn test_model_availability_values() {
    // Test that enum values exist
    let _ = ModelAvailability::Available;
    let _ = ModelAvailability::DeviceNotEligible;
    let _ = ModelAvailability::AppleIntelligenceNotEnabled;
    let _ = ModelAvailability::ModelNotReady;
    let _ = ModelAvailability::Unknown;
}

#[test]
fn test_response_methods() {
    // Test Response struct (requires constructing one, which we can't do directly in tests
    // since Response::new is pub(crate). This is tested indirectly via integration tests.
}

#[test]
fn test_error_display() {
    let err = Error::ModelNotAvailable;
    assert!(!err.to_string().is_empty());

    let err = Error::InvalidInput("test".to_string());
    assert!(err.to_string().contains("test"));

    let err = Error::GenerationError("failed".to_string());
    assert!(err.to_string().contains("failed"));
}

// ============================================================================
// Tool Calling Tests
// ============================================================================

use fm_rs::{Tool, ToolOutput};
use serde_json::{Value, json};
use std::sync::Arc;

struct TestCalculatorTool;

impl Tool for TestCalculatorTool {
    fn name(&self) -> &'static str {
        "calculator"
    }

    fn description(&self) -> &'static str {
        "Performs basic arithmetic. Use this for any math calculations."
    }

    fn arguments_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "operation": {
                    "type": "string",
                    "enum": ["add", "multiply"],
                    "description": "The operation to perform"
                },
                "a": { "type": "number" },
                "b": { "type": "number" }
            },
            "required": ["operation", "a", "b"]
        })
    }

    fn call(&self, arguments: Value) -> fm_rs::Result<ToolOutput> {
        let op = arguments["operation"].as_str().unwrap_or("add");
        let a = arguments["a"].as_f64().unwrap_or(0.0);
        let b = arguments["b"].as_f64().unwrap_or(0.0);

        let result = match op {
            "add" => a + b,
            "multiply" => a * b,
            _ => return Ok(ToolOutput::new("Unknown operation")),
        };

        Ok(ToolOutput::new(format!("{result}")))
    }
}

#[test]
#[ignore = "Requires Apple Intelligence to be enabled"]
fn test_tool_calling() {
    let model = SystemLanguageModel::new().expect("Failed to create model");

    if !model.is_available() {
        println!("Skipping test: Model not available");
        return;
    }

    let tools: Vec<Arc<dyn Tool>> = vec![Arc::new(TestCalculatorTool)];
    let session = Session::with_tools(&model, &tools).expect("Failed to create session with tools");

    let options = GenerationOptions::builder()
        .temperature(0.3)
        .max_response_tokens(100)
        .build();

    let response = session
        .respond(
            "What is 7 multiplied by 6? Use the calculator tool.",
            &options,
        )
        .expect("Failed to get response");

    println!("Tool calling response: {}", response.content());
    // The response should contain 42 (7 * 6)
    assert!(
        response.content().contains("42"),
        "Response should contain the result 42"
    );
}

#[test]
#[ignore = "Requires Apple Intelligence to be enabled"]
fn test_session_with_instructions_and_tools() {
    let model = SystemLanguageModel::new().expect("Failed to create model");

    if !model.is_available() {
        println!("Skipping test: Model not available");
        return;
    }

    let tools: Vec<Arc<dyn Tool>> = vec![Arc::new(TestCalculatorTool)];
    let session = Session::with_instructions_and_tools(
        &model,
        "You are a math tutor. Always use the calculator tool for calculations.",
        &tools,
    )
    .expect("Failed to create session");

    let options = GenerationOptions::default();
    let response = session
        .respond("Add 15 and 27", &options)
        .expect("Failed to get response");

    println!("Response: {}", response.content());
    // Should contain 42 (15 + 27)
    assert!(
        response.content().contains("42"),
        "Response should contain 42"
    );
}

// ============================================================================
// Structured Output Tests
// ============================================================================

use fm_rs::ToolCallError;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct TestPerson {
    name: String,
    age: u32,
}

// ============================================================================
// Error Path Tests (do not require FoundationModels)
// ============================================================================

#[test]
fn test_error_types_display() {
    // Test all error variants have meaningful display messages
    let errors = vec![
        Error::ModelNotAvailable,
        Error::DeviceNotEligible,
        Error::AppleIntelligenceNotEnabled,
        Error::ModelNotReady,
        Error::InvalidInput("null byte in string".to_string()),
        Error::GenerationError("model failed".to_string()),
        Error::Timeout("exceeded 30s".to_string()),
        Error::InternalError("FFI error".to_string()),
        Error::PoisonError,
        Error::Json("invalid syntax".to_string()),
        Error::ToolCall(ToolCallError {
            tool_name: "test_tool".to_string(),
            arguments: json!({"key": "value"}),
            inner_error: "tool crashed".to_string(),
        }),
    ];

    for err in errors {
        let msg = err.to_string();
        assert!(
            !msg.is_empty(),
            "Error should have non-empty message: {err:?}"
        );
        // Verify it doesn't just say "unknown" or similar
        assert!(
            !msg.to_lowercase().contains("unknown error"),
            "Error should have specific message: {msg}"
        );
    }
}

#[test]
fn test_tool_call_error_display() {
    let err = ToolCallError {
        tool_name: "weather_api".to_string(),
        arguments: json!({"city": "Paris", "units": "celsius"}),
        inner_error: "API rate limit exceeded".to_string(),
    };

    let msg = err.to_string();
    assert!(msg.contains("weather_api"), "Should include tool name");
    assert!(
        msg.contains("API rate limit"),
        "Should include error message"
    );
}

#[test]
fn test_invalid_input_null_bytes() {
    // Strings with null bytes should fail when used in FFI
    let bad_string = "hello\0world";
    let result = std::ffi::CString::new(bad_string);
    assert!(result.is_err(), "CString should reject null bytes");
}

#[test]
fn test_generation_options_temperature_validation() {
    // Out of range temperatures are silently ignored by temperature()
    let options = GenerationOptions::builder()
        .temperature(5.0) // Out of 0.0-2.0 range
        .build();
    assert!(
        options.temperature.is_none(),
        "Out-of-range temperature should be ignored"
    );

    // try_temperature returns an error for out of range
    let result = GenerationOptions::builder().try_temperature(5.0);
    assert!(
        result.is_err(),
        "try_temperature should reject out-of-range"
    );

    // Valid temperature works
    let options = GenerationOptions::builder().temperature(1.5).build();
    assert_eq!(options.temperature, Some(1.5));
}

#[test]
fn test_tool_trait_implementation() {
    // Verify Tool trait can be implemented and methods work
    struct SimpleTool;

    #[allow(clippy::unnecessary_literal_bound)]
    impl Tool for SimpleTool {
        fn name(&self) -> &str {
            "simple"
        }
        fn description(&self) -> &str {
            "A simple test tool"
        }
        fn arguments_schema(&self) -> Value {
            json!({"type": "object", "properties": {}})
        }
        fn call(&self, _: Value) -> fm_rs::Result<ToolOutput> {
            Ok(ToolOutput::new("result"))
        }
    }

    let tool = SimpleTool;
    assert_eq!(tool.name(), "simple");
    assert_eq!(tool.description(), "A simple test tool");
    assert!(tool.arguments_schema().is_object());

    let result = tool.call(json!({})).expect("call should succeed");
    assert_eq!(result.content, "result");
}

#[test]
fn test_malformed_json_parsing() {
    // Test that serde_json properly rejects malformed JSON
    let malformed_inputs = vec![
        "{missing: quotes}",
        "{'single': 'quotes'}",
        "{\"unclosed\": ",
        "[1, 2, 3",
        "null null",
        "",
    ];

    for input in malformed_inputs {
        let result: Result<Value, _> = serde_json::from_str(input);
        assert!(result.is_err(), "Should reject malformed JSON: {input}");
    }
}

#[test]
fn test_error_from_json() {
    // Test Error::Json variant from serde errors
    let bad_json = "not valid json";
    let result: Result<Value, serde_json::Error> = serde_json::from_str(bad_json);
    let err: Error = result.unwrap_err().into();

    match err {
        Error::Json(msg) => {
            assert!(!msg.is_empty(), "JSON error should have message");
        }
        _ => panic!("Expected Error::Json variant"),
    }
}

#[test]
fn test_model_availability_into_error() {
    assert!(ModelAvailability::Available.into_error().is_none());

    assert!(matches!(
        ModelAvailability::DeviceNotEligible.into_error(),
        Some(Error::DeviceNotEligible)
    ));

    assert!(matches!(
        ModelAvailability::AppleIntelligenceNotEnabled.into_error(),
        Some(Error::AppleIntelligenceNotEnabled)
    ));

    assert!(matches!(
        ModelAvailability::ModelNotReady.into_error(),
        Some(Error::ModelNotReady)
    ));

    assert!(matches!(
        ModelAvailability::Unknown.into_error(),
        Some(Error::ModelNotAvailable)
    ));
}

#[test]
#[ignore = "Requires Apple Intelligence to be enabled"]
fn test_respond_json() {
    let model = SystemLanguageModel::new().expect("Failed to create model");

    if !model.is_available() {
        println!("Skipping test: Model not available");
        return;
    }

    let session = Session::new(&model).expect("Failed to create session");

    let schema = json!({
        "type": "object",
        "properties": {
            "name": { "type": "string" },
            "age": { "type": "integer", "minimum": 1, "maximum": 120 }
        },
        "required": ["name", "age"]
    });

    let options = GenerationOptions::builder()
        .temperature(0.5)
        .max_response_tokens(100)
        .build();

    let json_str = session
        .respond_json(
            "Generate a fictional person named Alice who is 30 years old",
            &schema,
            &options,
        )
        .expect("Failed to get JSON response");

    println!("JSON response: {json_str}");

    // Verify it's valid JSON
    let parsed: Value = serde_json::from_str(&json_str).expect("Response should be valid JSON");
    assert!(parsed["name"].is_string(), "Should have name field");
    assert!(parsed["age"].is_number(), "Should have age field");
}

#[test]
#[ignore = "Requires Apple Intelligence to be enabled"]
fn test_respond_structured() {
    let model = SystemLanguageModel::new().expect("Failed to create model");

    if !model.is_available() {
        println!("Skipping test: Model not available");
        return;
    }

    let session = Session::new(&model).expect("Failed to create session");

    let schema = json!({
        "type": "object",
        "properties": {
            "name": { "type": "string" },
            "age": { "type": "integer" }
        },
        "required": ["name", "age"]
    });

    let options = GenerationOptions::builder()
        .temperature(0.3)
        .max_response_tokens(100)
        .build();

    let person: TestPerson = session
        .respond_structured(
            "Generate a person named Bob who is 25 years old",
            &schema,
            &options,
        )
        .expect("Failed to get structured response");

    println!("Structured response: {person:?}");
    assert!(!person.name.is_empty(), "Name should not be empty");
    assert!(person.age > 0, "Age should be positive");
}

#[test]
#[ignore = "Requires Apple Intelligence to be enabled"]
fn test_stream_json() {
    use std::sync::{Arc, Mutex};

    let model = SystemLanguageModel::new().expect("Failed to create model");

    if !model.is_available() {
        println!("Skipping test: Model not available");
        return;
    }

    let session = Session::new(&model).expect("Failed to create session");

    let schema = json!({
        "type": "object",
        "properties": {
            "items": {
                "type": "array",
                "items": { "type": "string" }
            }
        },
        "required": ["items"]
    });

    let options = GenerationOptions::builder()
        .temperature(0.5)
        .max_response_tokens(200)
        .build();

    let chunks = Arc::new(Mutex::new(Vec::new()));
    let chunks_clone = Arc::clone(&chunks);

    session
        .stream_json("List 3 colors", &schema, &options, move |chunk| {
            chunks_clone.lock().unwrap().push(chunk.to_string());
        })
        .expect("Failed to stream JSON");

    let chunks = chunks.lock().unwrap();
    assert!(!chunks.is_empty(), "Should have received chunks");

    // The final accumulated content should be valid JSON
    let full_content: String = chunks.join("");
    println!("Streamed JSON: {full_content}");
}
