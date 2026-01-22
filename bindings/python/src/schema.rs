//! JSON Schema builder for structured generation.
//!
//! Provides a fluent API for building JSON Schemas in Python.

use pyo3::prelude::*;
use pyo3::types::PyDict;
use serde_json::{Map, Value};

use crate::tool::json_to_py;

/// A fluent JSON Schema builder for structured generation.
///
/// Example:
///     schema = (`Schema.object()`
///         .property("name", `Schema.string()`, required=True)
///         .property("age", Schema.integer().minimum(0)))
///     result = `session.respond_structured("Generate` a person", `schema.to_dict()`)
#[pyclass(module = "fm")]
#[derive(Debug, Clone)]
pub struct Schema {
    inner: Map<String, Value>,
}

#[pymethods]
impl Schema {
    /// Creates a new empty schema.
    #[new]
    fn new() -> Self {
        Self { inner: Map::new() }
    }

    /// Creates a string schema.
    #[staticmethod]
    fn string() -> Self {
        let mut inner = Map::new();
        inner.insert("type".to_string(), Value::String("string".to_string()));
        Self { inner }
    }

    /// Creates an integer schema.
    #[staticmethod]
    fn integer() -> Self {
        let mut inner = Map::new();
        inner.insert("type".to_string(), Value::String("integer".to_string()));
        Self { inner }
    }

    /// Creates a number schema.
    #[staticmethod]
    fn number() -> Self {
        let mut inner = Map::new();
        inner.insert("type".to_string(), Value::String("number".to_string()));
        Self { inner }
    }

    /// Creates a boolean schema.
    #[staticmethod]
    fn boolean() -> Self {
        let mut inner = Map::new();
        inner.insert("type".to_string(), Value::String("boolean".to_string()));
        Self { inner }
    }

    /// Creates a null schema.
    #[staticmethod]
    fn null() -> Self {
        let mut inner = Map::new();
        inner.insert("type".to_string(), Value::String("null".to_string()));
        Self { inner }
    }

    /// Creates an object schema.
    #[staticmethod]
    fn object() -> Self {
        let mut inner = Map::new();
        inner.insert("type".to_string(), Value::String("object".to_string()));
        inner.insert("properties".to_string(), Value::Object(Map::new()));
        Self { inner }
    }

    /// Creates an array schema.
    ///
    /// Args:
    ///     items: Optional schema for array items.
    #[staticmethod]
    #[pyo3(signature = (items=None))]
    fn array(items: Option<&Schema>) -> Self {
        let mut inner = Map::new();
        inner.insert("type".to_string(), Value::String("array".to_string()));
        if let Some(item_schema) = items {
            inner.insert(
                "items".to_string(),
                Value::Object(item_schema.inner.clone()),
            );
        }
        Self { inner }
    }

    /// Adds a property to an object schema.
    ///
    /// Args:
    ///     name: Property name.
    ///     schema: Property schema.
    ///     required: Whether the property is required (default: False).
    ///
    /// Returns:
    ///     Schema: A new schema with the property added.
    #[pyo3(signature = (name, schema, *, required=false))]
    fn property(&self, name: &str, schema: &Schema, required: bool) -> Self {
        let mut result = self.clone();

        // Get or create properties object
        let properties = result
            .inner
            .entry("properties")
            .or_insert_with(|| Value::Object(Map::new()));

        if let Value::Object(props) = properties {
            props.insert(name.to_string(), Value::Object(schema.inner.clone()));
        }

        // Handle required
        if required {
            let required_arr = result
                .inner
                .entry("required")
                .or_insert_with(|| Value::Array(Vec::new()));

            if let Value::Array(arr) = required_arr {
                let name_val = Value::String(name.to_string());
                if !arr.contains(&name_val) {
                    arr.push(name_val);
                }
            }
        }

        result
    }

    /// Sets a description for this schema.
    ///
    /// Args:
    ///     description: The description text.
    ///
    /// Returns:
    ///     Schema: A new schema with the description.
    fn description(&self, description: &str) -> Self {
        let mut result = self.clone();
        result.inner.insert(
            "description".to_string(),
            Value::String(description.to_string()),
        );
        result
    }

    /// Sets the minimum value for a number/integer schema.
    ///
    /// Args:
    ///     value: The minimum value.
    ///
    /// Returns:
    ///     Schema: A new schema with the minimum.
    fn minimum(&self, value: f64) -> PyResult<Self> {
        let number = serde_json::Number::from_f64(value).ok_or_else(|| {
            pyo3::exceptions::PyValueError::new_err("minimum must be a finite number")
        })?;
        let mut result = self.clone();
        result
            .inner
            .insert("minimum".to_string(), Value::Number(number));
        Ok(result)
    }

    /// Sets the maximum value for a number/integer schema.
    ///
    /// Args:
    ///     value: The maximum value.
    ///
    /// Returns:
    ///     Schema: A new schema with the maximum.
    fn maximum(&self, value: f64) -> PyResult<Self> {
        let number = serde_json::Number::from_f64(value).ok_or_else(|| {
            pyo3::exceptions::PyValueError::new_err("maximum must be a finite number")
        })?;
        let mut result = self.clone();
        result
            .inner
            .insert("maximum".to_string(), Value::Number(number));
        Ok(result)
    }

    /// Sets the minimum length for a string schema.
    ///
    /// Args:
    ///     value: The minimum length.
    ///
    /// Returns:
    ///     Schema: A new schema with the minimum length.
    fn min_length(&self, value: u64) -> Self {
        let mut result = self.clone();
        result
            .inner
            .insert("minLength".to_string(), Value::Number(value.into()));
        result
    }

    /// Sets the maximum length for a string schema.
    ///
    /// Args:
    ///     value: The maximum length.
    ///
    /// Returns:
    ///     Schema: A new schema with the maximum length.
    fn max_length(&self, value: u64) -> Self {
        let mut result = self.clone();
        result
            .inner
            .insert("maxLength".to_string(), Value::Number(value.into()));
        result
    }

    /// Sets a regex pattern for a string schema.
    ///
    /// Args:
    ///     pattern: The regex pattern.
    ///
    /// Returns:
    ///     Schema: A new schema with the pattern.
    fn pattern(&self, pattern: &str) -> Self {
        let mut result = self.clone();
        result
            .inner
            .insert("pattern".to_string(), Value::String(pattern.to_string()));
        result
    }

    /// Sets allowed enum values.
    ///
    /// Args:
    ///     values: List of allowed values.
    ///
    /// Returns:
    ///     Schema: A new schema with the enum values.
    #[pyo3(name = "enum_")]
    fn enum_values(&self, values: Vec<String>) -> Self {
        let mut result = self.clone();
        let vals: Vec<Value> = values.into_iter().map(Value::String).collect();
        result.inner.insert("enum".to_string(), Value::Array(vals));
        result
    }

    /// Sets the default value.
    ///
    /// Args:
    ///     value: The default value (string).
    ///
    /// Returns:
    ///     Schema: A new schema with the default.
    fn default(&self, value: &str) -> Self {
        let mut result = self.clone();
        result
            .inner
            .insert("default".to_string(), Value::String(value.to_string()));
        result
    }

    /// Sets the minimum number of items for an array schema.
    ///
    /// Args:
    ///     value: The minimum items.
    ///
    /// Returns:
    ///     Schema: A new schema with the minimum items.
    fn min_items(&self, value: u64) -> Self {
        let mut result = self.clone();
        result
            .inner
            .insert("minItems".to_string(), Value::Number(value.into()));
        result
    }

    /// Sets the maximum number of items for an array schema.
    ///
    /// Args:
    ///     value: The maximum items.
    ///
    /// Returns:
    ///     Schema: A new schema with the maximum items.
    fn max_items(&self, value: u64) -> Self {
        let mut result = self.clone();
        result
            .inner
            .insert("maxItems".to_string(), Value::Number(value.into()));
        result
    }

    /// Sets whether array items must be unique.
    ///
    /// Args:
    ///     value: Whether items must be unique.
    ///
    /// Returns:
    ///     Schema: A new schema with unique items setting.
    fn unique_items(&self, value: bool) -> Self {
        let mut result = self.clone();
        result
            .inner
            .insert("uniqueItems".to_string(), Value::Bool(value));
        result
    }

    /// Disallows additional properties in an object schema.
    ///
    /// Returns:
    ///     Schema: A new schema with additional properties disabled.
    fn no_additional_properties(&self) -> Self {
        let mut result = self.clone();
        result
            .inner
            .insert("additionalProperties".to_string(), Value::Bool(false));
        result
    }

    /// Converts the schema to a Python dict.
    ///
    /// Returns:
    ///     dict: The JSON Schema as a Python dict.
    fn to_dict<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let value = Value::Object(self.inner.clone());
        let obj = json_to_py(py, &value)?;
        obj.bind(py)
            .cast::<PyDict>()
            .cloned()
            .map_err(|e| pyo3::exceptions::PyTypeError::new_err(format!("Expected dict: {e}")))
    }

    /// Converts the schema to a JSON string.
    ///
    /// Returns:
    ///     str: The JSON Schema as a string.
    fn to_json(&self) -> PyResult<String> {
        serde_json::to_string(&self.inner).map_err(|e| {
            pyo3::exceptions::PyValueError::new_err(format!("Failed to serialize schema: {e}"))
        })
    }

    fn __repr__(&self) -> PyResult<String> {
        let json = serde_json::to_string_pretty(&self.inner).map_err(|e| {
            pyo3::exceptions::PyValueError::new_err(format!("Failed to serialize schema: {e}"))
        })?;
        Ok(format!("Schema({json})"))
    }
}
