//! Tool calling support for `FoundationModels`.
//!
//! Tools allow the model to call external functions during generation.
//! Implement the [`Tool`] trait to define custom tools.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;

use crate::error::Result;

/// A tool that can be invoked by the model.
///
/// # Example
///
/// ```rust
/// use fm_rs::{Tool, ToolOutput};
/// use serde_json::{json, Value};
///
/// struct WeatherTool;
///
/// impl Tool for WeatherTool {
///     fn name(&self) -> &str {
///         "checkWeather"
///     }
///
///     fn description(&self) -> &str {
///         "Check current weather conditions"
///     }
///
///     fn arguments_schema(&self) -> Value {
///         json!({
///             "type": "object",
///             "properties": {
///                 "location": {
///                     "type": "string",
///                     "description": "The city and country"
///                 }
///             },
///             "required": ["location"]
///         })
///     }
///
///     fn call(&self, arguments: Value) -> fm_rs::Result<ToolOutput> {
///         let location = arguments["location"].as_str().unwrap_or("Unknown");
///         Ok(ToolOutput::new(format!("Weather in {location}: Sunny, 72°F")))
///     }
/// }
/// ```
pub trait Tool: Send + Sync {
    /// Returns the name of the tool.
    fn name(&self) -> &str;

    /// Returns a description of what the tool does.
    fn description(&self) -> &str;

    /// Returns the JSON schema for the tool's arguments.
    fn arguments_schema(&self) -> Value;

    /// Invokes the tool with the given arguments.
    ///
    /// # Errors
    ///
    /// Returns an error if the tool invocation fails.
    fn call(&self, arguments: Value) -> Result<ToolOutput>;
}

/// Output returned by a tool invocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolOutput {
    /// The content returned by the tool.
    pub content: String,
}

impl ToolOutput {
    /// Creates a new tool output with the given content.
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
        }
    }

    /// Creates a tool output from a JSON-serializable value.
    pub fn from_json<T: Serialize>(value: &T) -> Result<Self> {
        let content = serde_json::to_string(value)?;
        Ok(Self { content })
    }
}

/// Internal representation of a tool for serialization to Swift.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ToolDefinition {
    pub name: String,
    pub description: String,
    #[serde(rename = "argumentsSchema")]
    pub arguments_schema: Value,
}

/// Serializes a list of tools to JSON for FFI.
///
/// # Errors
///
/// Returns an error if JSON serialization fails.
pub(crate) fn tools_to_json(tools: &[&dyn Tool]) -> crate::error::Result<String> {
    let mut seen = HashSet::new();
    let mut definitions = Vec::with_capacity(tools.len());

    for tool in tools {
        let name = tool.name().trim();
        if name.is_empty() {
            return Err(crate::error::Error::InvalidInput(
                "Tool name cannot be empty".to_string(),
            ));
        }
        if !seen.insert(name.to_string()) {
            return Err(crate::error::Error::InvalidInput(format!(
                "Duplicate tool name: {name}"
            )));
        }

        let schema = tool.arguments_schema();
        let schema_obj = schema.as_object().ok_or_else(|| {
            crate::error::Error::InvalidInput(format!(
                "Tool '{name}' arguments schema must be a JSON object"
            ))
        })?;
        if let Some(Value::String(ty)) = schema_obj.get("type") {
            if ty != "object" {
                return Err(crate::error::Error::InvalidInput(format!(
                    "Tool '{name}' arguments schema must have type \"object\""
                )));
            }
        }

        definitions.push(ToolDefinition {
            name: name.to_string(),
            description: tool.description().to_string(),
            arguments_schema: schema,
        });
    }

    serde_json::to_string(&definitions)
        .map_err(|e| crate::error::Error::InvalidInput(format!("Failed to serialize tools: {e}")))
}

/// Result of invoking a tool, serialized for FFI.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct ToolResult {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ToolResult {
    pub fn success(output: ToolOutput) -> Self {
        Self {
            success: true,
            content: Some(output.content),
            error: None,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            success: false,
            content: None,
            error: Some(message.into()),
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| {
            r#"{"success":false,"error":"Failed to serialize result"}"#.to_string()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct TestTool;

    impl Tool for TestTool {
        fn name(&self) -> &'static str {
            "test_tool"
        }

        fn description(&self) -> &'static str {
            "A test tool"
        }

        fn arguments_schema(&self) -> Value {
            json!({
                "type": "object",
                "properties": {
                    "input": {"type": "string"}
                }
            })
        }

        fn call(&self, arguments: Value) -> Result<ToolOutput> {
            let input = arguments["input"].as_str().unwrap_or("default");
            Ok(ToolOutput::new(format!("Processed: {input}")))
        }
    }

    #[test]
    fn test_tool_definition() {
        let tool = TestTool;
        let def = ToolDefinition {
            name: tool.name().to_string(),
            description: tool.description().to_string(),
            arguments_schema: tool.arguments_schema(),
        };

        assert_eq!(def.name, "test_tool");
        assert_eq!(def.description, "A test tool");
    }

    #[test]
    fn test_tools_to_json() {
        let tool = TestTool;
        let tools: Vec<&dyn Tool> = vec![&tool];
        let json = tools_to_json(&tools).expect("serialization should succeed");

        assert!(json.contains("test_tool"));
        assert!(json.contains("A test tool"));
    }

    #[test]
    fn test_tools_to_json_duplicate_names() {
        struct ToolA;
        struct ToolB;

        impl Tool for ToolA {
            fn name(&self) -> &'static str {
                "duplicate"
            }

            fn description(&self) -> &'static str {
                "Tool A"
            }

            fn arguments_schema(&self) -> Value {
                json!({"type": "object"})
            }

            fn call(&self, _arguments: Value) -> Result<ToolOutput> {
                Ok(ToolOutput::new("ok"))
            }
        }

        impl Tool for ToolB {
            fn name(&self) -> &'static str {
                "duplicate"
            }

            fn description(&self) -> &'static str {
                "Tool B"
            }

            fn arguments_schema(&self) -> Value {
                json!({"type": "object"})
            }

            fn call(&self, _arguments: Value) -> Result<ToolOutput> {
                Ok(ToolOutput::new("ok"))
            }
        }

        let tools: Vec<&dyn Tool> = vec![&ToolA, &ToolB];
        let err = tools_to_json(&tools).expect_err("expected duplicate error");
        assert!(err.to_string().contains("Duplicate tool name"));
    }

    #[test]
    fn test_tools_to_json_requires_object_schema() {
        struct BadTool;

        impl Tool for BadTool {
            fn name(&self) -> &'static str {
                "bad"
            }

            fn description(&self) -> &'static str {
                "Bad schema"
            }

            fn arguments_schema(&self) -> Value {
                json!("not-an-object")
            }

            fn call(&self, _arguments: Value) -> Result<ToolOutput> {
                Ok(ToolOutput::new("ok"))
            }
        }

        let tools: Vec<&dyn Tool> = vec![&BadTool];
        let err = tools_to_json(&tools).expect_err("expected schema error");
        assert!(
            err.to_string()
                .contains("arguments schema must be a JSON object")
        );
    }

    #[test]
    fn test_tools_to_json_requires_object_type() {
        struct WrongTypeTool;

        impl Tool for WrongTypeTool {
            fn name(&self) -> &'static str {
                "wrong_type"
            }

            fn description(&self) -> &'static str {
                "Wrong type"
            }

            fn arguments_schema(&self) -> Value {
                json!({"type": "string"})
            }

            fn call(&self, _arguments: Value) -> Result<ToolOutput> {
                Ok(ToolOutput::new("ok"))
            }
        }

        let tools: Vec<&dyn Tool> = vec![&WrongTypeTool];
        let err = tools_to_json(&tools).expect_err("expected type error");
        assert!(err.to_string().contains("must have type \"object\""));
    }

    #[test]
    fn test_tool_output() {
        let output = ToolOutput::new("Hello, World!");
        assert_eq!(output.content, "Hello, World!");
    }

    #[test]
    fn test_tool_result_json() {
        let result = ToolResult::success(ToolOutput::new("OK"));
        let json = result.to_json();
        assert!(json.contains("\"success\":true"));
        assert!(json.contains("\"content\":\"OK\""));
    }
}
