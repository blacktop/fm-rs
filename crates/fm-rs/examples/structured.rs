//! Example: Structured JSON output from `FoundationModels`
//!
//! This example demonstrates how to get structured JSON responses
//! that conform to a schema, which can then be deserialized into
//! Rust types.
//!
//! To run this example:
//!   `cargo run --example structured`

use fm_rs::{GenerationOptions, Session, SystemLanguageModel};
use serde::Deserialize;
use serde_json::json;

/// A person with basic information.
#[derive(Debug, Deserialize)]
struct Person {
    name: String,
    age: u32,
    occupation: String,
    hobbies: Vec<String>,
}

/// A movie recommendation.
#[derive(Debug, Deserialize)]
struct MovieRecommendation {
    title: String,
    year: u32,
    genre: String,
    reason: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("FoundationModels Structured Output Example");
    println!("==========================================\n");

    // Create the default system language model
    let model = SystemLanguageModel::new()?;

    if !model.is_available() {
        println!("FoundationModels is not available on this device.");
        return Ok(());
    }

    // Create a session
    let session = Session::new(&model)?;

    let options = GenerationOptions::builder()
        .temperature(0.7)
        .max_response_tokens(500)
        .build();

    // Example 1: Generate a fictional person
    println!("--- Example 1: Generate a Person ---");

    let person_schema = json!({
        "type": "object",
        "properties": {
            "name": { "type": "string", "description": "Full name of the person" },
            "age": { "type": "integer", "minimum": 1, "maximum": 120 },
            "occupation": { "type": "string", "description": "Job or profession" },
            "hobbies": {
                "type": "array",
                "items": { "type": "string" },
                "minItems": 1,
                "maxItems": 5
            }
        },
        "required": ["name", "age", "occupation", "hobbies"]
    });

    let person: Person = session.respond_structured(
        "Generate a fictional person who works in technology",
        &person_schema,
        &options,
    )?;

    println!("Generated Person:");
    println!("  Name: {}", person.name);
    println!("  Age: {}", person.age);
    println!("  Occupation: {}", person.occupation);
    println!("  Hobbies: {:?}", person.hobbies);

    // Example 2: Get movie recommendations
    println!("\n--- Example 2: Movie Recommendation ---");

    let movie_schema = json!({
        "type": "object",
        "properties": {
            "title": { "type": "string", "description": "Movie title" },
            "year": { "type": "integer", "minimum": 1900, "maximum": 2025 },
            "genre": { "type": "string" },
            "reason": { "type": "string", "description": "Why this movie is recommended" }
        },
        "required": ["title", "year", "genre", "reason"]
    });

    let movie: MovieRecommendation = session.respond_structured(
        "Recommend a classic science fiction movie",
        &movie_schema,
        &options,
    )?;

    println!("Movie Recommendation:");
    println!("  Title: {} ({})", movie.title, movie.year);
    println!("  Genre: {}", movie.genre);
    println!("  Reason: {}", movie.reason);

    // Example 3: Get raw JSON (useful for dynamic schemas)
    println!("\n--- Example 3: Raw JSON Response ---");

    let list_schema = json!({
        "type": "object",
        "properties": {
            "items": {
                "type": "array",
                "items": { "type": "string" },
                "minItems": 3,
                "maxItems": 5
            }
        },
        "required": ["items"]
    });

    let json_str = session.respond_json(
        "List some popular programming languages for web development",
        &list_schema,
        &options,
    )?;

    println!("Raw JSON response:");
    println!("{json_str}");

    // Parse it manually if needed
    let parsed: serde_json::Value = serde_json::from_str(&json_str)?;
    if let Some(items) = parsed["items"].as_array() {
        println!("\nParsed items:");
        for (i, item) in items.iter().enumerate() {
            println!("  {}. {}", i + 1, item.as_str().unwrap_or(""));
        }
    }

    println!("\n==========================================");
    println!("Structured output example completed!");

    Ok(())
}
