//! Tests for the Generable derive macro.
//!
//! These tests verify that the derive macro produces correct JSON schemas
//! for various Rust types and attribute combinations.

#![cfg(feature = "derive")]
#![allow(dead_code, non_snake_case)]

use fm_rs::Generable;
use serde_json::json;
use std::collections::HashMap;

// ============================================================================
// Basic Struct Tests
// ============================================================================

#[test]
fn test_simple_struct_schema() {
    #[derive(Generable)]
    struct Person {
        name: String,
        age: u32,
    }

    let schema = Person::schema();

    assert_eq!(schema["type"], "object");
    assert!(schema["properties"]["name"].is_object());
    assert_eq!(schema["properties"]["name"]["type"], "string");
    assert!(schema["properties"]["age"].is_object());
    assert_eq!(schema["properties"]["age"]["type"], "integer");

    let required = schema["required"]
        .as_array()
        .expect("required should be array");
    assert!(required.contains(&json!("name")));
    assert!(required.contains(&json!("age")));
}

#[test]
fn test_optional_field_not_required() {
    #[derive(Generable)]
    struct Config {
        host: String,
        port: Option<u16>,
    }

    let schema = Config::schema();

    let required = schema["required"]
        .as_array()
        .expect("required should be array");
    assert!(required.contains(&json!("host")));
    assert!(!required.contains(&json!("port")));

    // Optional field should still appear in properties
    assert!(schema["properties"]["port"].is_object());
    assert_eq!(schema["properties"]["port"]["type"], "integer");
}

#[test]
fn test_default_field_not_required() {
    #[derive(Generable)]
    struct Settings {
        name: String,
        #[serde(default)]
        enabled: bool,
    }

    let schema = Settings::schema();

    let required = schema["required"]
        .as_array()
        .expect("required should be array");
    assert!(required.contains(&json!("name")));
    assert!(!required.contains(&json!("enabled")));
}

#[test]
fn test_all_optional_no_required_field() {
    #[derive(Generable)]
    struct AllOptional {
        a: Option<String>,
        b: Option<i32>,
    }

    let schema = AllOptional::schema();

    // required should not exist or be empty
    assert!(
        schema.get("required").is_none() || schema["required"].as_array().is_none_or(Vec::is_empty)
    );
}

// ============================================================================
// Rename Tests
// ============================================================================

#[test]
fn test_rename_all_camel_case() {
    #[derive(Generable)]
    #[generable(rename_all = "camelCase")]
    struct CamelConfig {
        max_retries: u32,
        api_url: String,
        is_enabled: bool,
    }

    let schema = CamelConfig::schema();
    let props = schema["properties"]
        .as_object()
        .expect("properties should be object");

    assert!(props.contains_key("maxRetries"), "should have maxRetries");
    assert!(props.contains_key("apiUrl"), "should have apiUrl");
    assert!(props.contains_key("isEnabled"), "should have isEnabled");

    // Original names should not exist
    assert!(!props.contains_key("max_retries"));
    assert!(!props.contains_key("api_url"));
    assert!(!props.contains_key("is_enabled"));
}

#[test]
fn test_rename_all_snake_case() {
    #[derive(Generable)]
    #[generable(rename_all = "snake_case")]
    struct SnakeConfig {
        MaxRetries: u32,
        ApiUrl: String,
    }

    let schema = SnakeConfig::schema();
    let props = schema["properties"]
        .as_object()
        .expect("properties should be object");

    assert!(props.contains_key("max_retries"), "should have max_retries");
    assert!(props.contains_key("api_url"), "should have api_url");
}

#[test]
fn test_rename_all_pascal_case() {
    #[derive(Generable)]
    #[generable(rename_all = "PascalCase")]
    struct PascalConfig {
        max_retries: u32,
        api_url: String,
    }

    let schema = PascalConfig::schema();
    let props = schema["properties"]
        .as_object()
        .expect("properties should be object");

    assert!(props.contains_key("MaxRetries"), "should have MaxRetries");
    assert!(props.contains_key("ApiUrl"), "should have ApiUrl");
}

#[test]
fn test_rename_all_kebab_case() {
    #[derive(Generable)]
    #[generable(rename_all = "kebab-case")]
    struct KebabConfig {
        max_retries: u32,
        api_url: String,
    }

    let schema = KebabConfig::schema();
    let props = schema["properties"]
        .as_object()
        .expect("properties should be object");

    assert!(props.contains_key("max-retries"), "should have max-retries");
    assert!(props.contains_key("api-url"), "should have api-url");
}

#[test]
fn test_rename_all_screaming_snake_case() {
    #[derive(Generable)]
    #[generable(rename_all = "SCREAMING_SNAKE_CASE")]
    struct ScreamingConfig {
        max_retries: u32,
        api_url: String,
    }

    let schema = ScreamingConfig::schema();
    let props = schema["properties"]
        .as_object()
        .expect("properties should be object");

    assert!(props.contains_key("MAX_RETRIES"), "should have MAX_RETRIES");
    assert!(props.contains_key("API_URL"), "should have API_URL");
}

#[test]
fn test_field_rename_overrides_rename_all() {
    #[derive(Generable)]
    #[generable(rename_all = "camelCase")]
    struct MixedConfig {
        normal_field: String,
        #[generable(rename = "custom_name")]
        custom_field: u32,
    }

    let schema = MixedConfig::schema();
    let props = schema["properties"]
        .as_object()
        .expect("properties should be object");

    assert!(props.contains_key("normalField"), "should have normalField");
    assert!(
        props.contains_key("custom_name"),
        "should have custom_name (explicit rename)"
    );
    assert!(!props.contains_key("customField"));
}

#[test]
fn test_serde_rename_all_interop() {
    #[derive(Generable)]
    #[serde(rename_all = "camelCase")]
    struct SerdeConfig {
        max_retries: u32,
        api_url: String,
    }

    let schema = SerdeConfig::schema();
    let props = schema["properties"]
        .as_object()
        .expect("properties should be object");

    assert!(
        props.contains_key("maxRetries"),
        "should honor serde rename_all"
    );
    assert!(props.contains_key("apiUrl"));
}

#[test]
fn test_serde_rename_field_interop() {
    #[derive(Generable)]
    struct SerdeRename {
        #[serde(rename = "userName")]
        user_name: String,
    }

    let schema = SerdeRename::schema();
    let props = schema["properties"]
        .as_object()
        .expect("properties should be object");

    assert!(props.contains_key("userName"), "should honor serde rename");
    assert!(!props.contains_key("user_name"));
}

// ============================================================================
// Enum Tests
// ============================================================================

#[test]
fn test_enum_string_schema() {
    #[derive(Generable)]
    enum Status {
        Pending,
        InProgress,
        Completed,
        Failed,
    }

    let schema = Status::schema();

    assert_eq!(schema["type"], "string");

    let variants = schema["enum"].as_array().expect("enum should be array");
    assert_eq!(variants.len(), 4);
    assert!(variants.contains(&json!("Pending")));
    assert!(variants.contains(&json!("InProgress")));
    assert!(variants.contains(&json!("Completed")));
    assert!(variants.contains(&json!("Failed")));
}

#[test]
fn test_enum_rename_all() {
    #[derive(Generable)]
    #[generable(rename_all = "snake_case")]
    enum SnakeStatus {
        InProgress,
        NotStarted,
        AlmostDone,
    }

    let schema = SnakeStatus::schema();
    let variants = schema["enum"].as_array().expect("enum should be array");

    assert!(variants.contains(&json!("in_progress")));
    assert!(variants.contains(&json!("not_started")));
    assert!(variants.contains(&json!("almost_done")));
}

#[test]
fn test_enum_skip_variant() {
    #[derive(Generable)]
    enum FilteredStatus {
        Active,
        #[generable(skip)]
        Internal,
        Inactive,
    }

    let schema = FilteredStatus::schema();
    let variants = schema["enum"].as_array().expect("enum should be array");

    assert_eq!(variants.len(), 2);
    assert!(variants.contains(&json!("Active")));
    assert!(variants.contains(&json!("Inactive")));
    assert!(!variants.contains(&json!("Internal")));
}

#[test]
fn test_enum_serde_skip_interop() {
    #[derive(Generable)]
    enum SerdeSkip {
        Public,
        #[serde(skip)]
        Private,
        AlsoPublic,
    }

    let schema = SerdeSkip::schema();
    let variants = schema["enum"].as_array().expect("enum should be array");

    assert_eq!(variants.len(), 2);
    assert!(!variants.contains(&json!("Private")));
}

// ============================================================================
// Skip Field Tests
// ============================================================================

#[test]
fn test_skip_field() {
    #[derive(Generable)]
    struct WithSkipped {
        visible: String,
        #[generable(skip)]
        hidden: String,
    }

    let schema = WithSkipped::schema();
    let props = schema["properties"]
        .as_object()
        .expect("properties should be object");

    assert!(props.contains_key("visible"));
    assert!(!props.contains_key("hidden"));
}

#[test]
fn test_serde_skip_serializing_interop() {
    #[derive(Generable)]
    struct SerdeSkipField {
        included: String,
        #[serde(skip_serializing)]
        excluded: String,
    }

    let schema = SerdeSkipField::schema();
    let props = schema["properties"]
        .as_object()
        .expect("properties should be object");

    assert!(props.contains_key("included"));
    assert!(!props.contains_key("excluded"));
}

// ============================================================================
// Collection Type Tests
// ============================================================================

#[test]
fn test_vec_schema() {
    #[derive(Generable)]
    struct WithVec {
        items: Vec<String>,
        numbers: Vec<i32>,
    }

    let schema = WithVec::schema();

    assert_eq!(schema["properties"]["items"]["type"], "array");
    assert_eq!(schema["properties"]["items"]["items"]["type"], "string");

    assert_eq!(schema["properties"]["numbers"]["type"], "array");
    assert_eq!(schema["properties"]["numbers"]["items"]["type"], "integer");
}

#[test]
fn test_hashmap_schema() {
    #[derive(Generable)]
    struct WithMap {
        metadata: HashMap<String, String>,
        counts: HashMap<String, i64>,
    }

    let schema = WithMap::schema();

    assert_eq!(schema["properties"]["metadata"]["type"], "object");
    assert_eq!(
        schema["properties"]["metadata"]["additionalProperties"]["type"],
        "string"
    );

    assert_eq!(schema["properties"]["counts"]["type"], "object");
    assert_eq!(
        schema["properties"]["counts"]["additionalProperties"]["type"],
        "integer"
    );
}

#[test]
fn test_optional_vec() {
    #[derive(Generable)]
    struct OptionalVec {
        required_items: Vec<String>,
        optional_items: Option<Vec<String>>,
    }

    let schema = OptionalVec::schema();

    let required = schema["required"]
        .as_array()
        .expect("required should be array");
    assert!(required.contains(&json!("required_items")));
    assert!(!required.contains(&json!("optional_items")));

    // Both should still have array type
    assert_eq!(schema["properties"]["optional_items"]["type"], "array");
}

// ============================================================================
// Nested Type Tests
// ============================================================================

#[test]
fn test_nested_generable() {
    #[derive(Generable)]
    struct Inner {
        value: String,
    }

    #[derive(Generable)]
    struct Outer {
        name: String,
        inner: Inner,
    }

    let schema = Outer::schema();

    assert_eq!(schema["type"], "object");
    assert!(schema["properties"]["inner"].is_object());
    // The inner schema should be inlined
    assert_eq!(schema["properties"]["inner"]["type"], "object");
    assert!(schema["properties"]["inner"]["properties"]["value"].is_object());
}

#[test]
fn test_vec_of_generable() {
    #[derive(Generable)]
    struct Item {
        id: u32,
        name: String,
    }

    #[derive(Generable)]
    struct Container {
        items: Vec<Item>,
    }

    let schema = Container::schema();

    assert_eq!(schema["properties"]["items"]["type"], "array");
    assert_eq!(schema["properties"]["items"]["items"]["type"], "object");
    assert!(schema["properties"]["items"]["items"]["properties"]["id"].is_object());
}

// ============================================================================
// Field Attribute Tests
// ============================================================================

#[test]
fn test_description_attribute() {
    #[derive(Generable)]
    struct Described {
        #[generable(description = "The user's full name")]
        name: String,
        #[generable(description = "Age in years")]
        age: u32,
    }

    let schema = Described::schema();

    assert_eq!(
        schema["properties"]["name"]["description"],
        "The user's full name"
    );
    assert_eq!(schema["properties"]["age"]["description"], "Age in years");
}

#[test]
fn test_doc_comment_as_description() {
    #[derive(Generable)]
    struct DocCommented {
        /// The user's email address
        email: String,
    }

    let schema = DocCommented::schema();

    assert_eq!(
        schema["properties"]["email"]["description"],
        "The user's email address"
    );
}

#[test]
fn test_minimum_maximum_attributes() {
    #[derive(Generable)]
    struct Bounded {
        #[generable(minimum = 0, maximum = 100)]
        percentage: u32,
        #[generable(minimum = 1)]
        count: i32,
    }

    let schema = Bounded::schema();

    assert_eq!(schema["properties"]["percentage"]["minimum"], 0);
    assert_eq!(schema["properties"]["percentage"]["maximum"], 100);
    assert_eq!(schema["properties"]["count"]["minimum"], 1);
    assert!(schema["properties"]["count"].get("maximum").is_none());
}

#[test]
fn test_string_length_attributes() {
    #[derive(Generable)]
    struct StringConstraints {
        #[generable(min_length = 1, max_length = 100)]
        username: String,
        #[generable(min_length = 8)]
        password: String,
    }

    let schema = StringConstraints::schema();

    assert_eq!(schema["properties"]["username"]["minLength"], 1);
    assert_eq!(schema["properties"]["username"]["maxLength"], 100);
    assert_eq!(schema["properties"]["password"]["minLength"], 8);
}

#[test]
fn test_pattern_attribute() {
    #[derive(Generable)]
    struct WithPattern {
        #[generable(pattern = r"^[a-z]+$")]
        lowercase_only: String,
    }

    let schema = WithPattern::schema();

    assert_eq!(
        schema["properties"]["lowercase_only"]["pattern"],
        "^[a-z]+$"
    );
}

#[test]
fn test_array_item_constraints() {
    #[derive(Generable)]
    struct ArrayConstraints {
        #[generable(min_items = 1, max_items = 10)]
        tags: Vec<String>,
    }

    let schema = ArrayConstraints::schema();

    assert_eq!(schema["properties"]["tags"]["minItems"], 1);
    assert_eq!(schema["properties"]["tags"]["maxItems"], 10);
}

#[test]
fn test_example_attribute() {
    #[derive(Generable)]
    struct WithExample {
        #[generable(example = "john@example.com")]
        email: String,
        #[generable(example = 42)]
        count: i32,
    }

    let schema = WithExample::schema();

    assert_eq!(schema["properties"]["email"]["example"], "john@example.com");
    assert_eq!(schema["properties"]["count"]["example"], 42);
}

#[test]
fn test_nullable_attribute() {
    #[derive(Generable)]
    struct WithNullable {
        #[generable(nullable = true)]
        maybe_name: String,
    }

    let schema = WithNullable::schema();

    assert_eq!(schema["properties"]["maybe_name"]["nullable"], true);
}

// ============================================================================
// Container Attribute Tests
// ============================================================================

#[test]
fn test_container_description() {
    /// A person with name and age
    #[derive(Generable)]
    struct DocumentedPerson {
        name: String,
        age: u32,
    }

    let schema = DocumentedPerson::schema();

    assert_eq!(schema["description"], "A person with name and age");
}

#[test]
fn test_container_explicit_description() {
    #[derive(Generable)]
    #[generable(description = "Configuration for the application")]
    struct AppConfig {
        debug: bool,
    }

    let schema = AppConfig::schema();

    assert_eq!(schema["description"], "Configuration for the application");
}

// ============================================================================
// Primitive Type Tests
// ============================================================================

#[test]
fn test_all_integer_types() {
    #[derive(Generable)]
    struct Integers {
        a: u8,
        b: u16,
        c: u32,
        d: u64,
        e: i8,
        f: i16,
        g: i32,
        h: i64,
    }

    let schema = Integers::schema();

    for field in ["a", "b", "c", "d", "e", "f", "g", "h"] {
        assert_eq!(
            schema["properties"][field]["type"], "integer",
            "field {field} should be integer"
        );
    }
}

#[test]
fn test_float_types() {
    #[derive(Generable)]
    struct Floats {
        a: f32,
        b: f64,
    }

    let schema = Floats::schema();

    assert_eq!(schema["properties"]["a"]["type"], "number");
    assert_eq!(schema["properties"]["b"]["type"], "number");
}

#[test]
fn test_bool_type() {
    #[derive(Generable)]
    struct Booleans {
        flag: bool,
    }

    let schema = Booleans::schema();

    assert_eq!(schema["properties"]["flag"]["type"], "boolean");
}

// ============================================================================
// Newtype Struct Tests
// ============================================================================

#[test]
fn test_newtype_struct() {
    #[derive(Generable)]
    struct UserId(u64);

    let schema = UserId::schema();

    // Newtype should unwrap to inner type
    assert_eq!(schema["type"], "integer");
}

#[test]
fn test_newtype_string() {
    #[derive(Generable)]
    struct Email(String);

    let schema = Email::schema();

    assert_eq!(schema["type"], "string");
}

// ============================================================================
// Edge Case Tests
// ============================================================================

#[test]
fn test_acronym_handling_camel_case() {
    #[derive(Generable)]
    #[generable(rename_all = "camelCase")]
    struct AcronymTest {
        xml_parser: String,
        html_element: String,
        io_error: String,
    }

    let schema = AcronymTest::schema();
    let props = schema["properties"]
        .as_object()
        .expect("properties should be object");

    assert!(props.contains_key("xmlParser"));
    assert!(props.contains_key("htmlElement"));
    assert!(props.contains_key("ioError"));
}

#[test]
fn test_single_letter_words() {
    #[derive(Generable)]
    #[generable(rename_all = "camelCase")]
    struct SingleLetter {
        a_field: String,
        x_value: i32,
    }

    let schema = SingleLetter::schema();
    let props = schema["properties"]
        .as_object()
        .expect("properties should be object");

    assert!(props.contains_key("aField"));
    assert!(props.contains_key("xValue"));
}

#[test]
fn test_empty_struct() {
    #[derive(Generable)]
    struct Empty {}

    let schema = Empty::schema();

    assert_eq!(schema["type"], "object");
    assert!(schema["properties"].as_object().unwrap().is_empty());
}

#[test]
fn test_deeply_nested() {
    #[derive(Generable)]
    struct Level3 {
        value: String,
    }

    #[derive(Generable)]
    struct Level2 {
        level3: Level3,
    }

    #[derive(Generable)]
    struct Level1 {
        level2: Level2,
    }

    let schema = Level1::schema();

    assert_eq!(
        schema["properties"]["level2"]["properties"]["level3"]["properties"]["value"]["type"],
        "string"
    );
}
