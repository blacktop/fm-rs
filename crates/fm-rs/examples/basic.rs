//! Example: Using `FoundationModels` in Rust
//!
//! This example demonstrates how to use the fm-rs library to interact with
//! Apple's FoundationModels.framework for on-device AI capabilities.
//!
//! To run this example:
//!   `cargo run --example basic`

use fm_rs::{GenerationOptions, Session, SystemLanguageModel};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("FoundationModels Rust Example");
    println!("==============================\n");

    // Create the default system language model
    println!("Creating default system language model...");
    let model = SystemLanguageModel::new()?;

    // Check availability
    println!("Checking availability...");
    if !model.is_available() {
        println!("FoundationModels is not available on this device.");
        println!("This could be because:");
        println!("  - Your device doesn't support Apple Intelligence");
        println!("  - Apple Intelligence is not enabled in settings");
        println!("  - The model hasn't finished downloading");
        return Ok(());
    }

    println!("Model is available!");
    println!("Availability status: {:?}\n", model.availability());

    // Create a session with instructions
    println!("Creating a session with instructions...");
    let session = Session::with_instructions(
        &model,
        "You are a helpful assistant that provides concise, accurate answers.",
    )?;

    // Create generation options with a moderate temperature for balanced responses
    println!("Creating generation options...");
    let options = GenerationOptions::builder()
        .temperature(0.7)
        .max_response_tokens(500)
        .build();

    // Example 1: Simple question
    println!("\n--- Example 1: Simple Question ---");
    let prompt = "What is the capital of France? Answer in one sentence.";
    println!("Prompt: {prompt}");

    let response = session.respond(prompt, &options)?;
    println!("Response: {}\n", response.content());

    // Example 2: Creative writing
    println!("--- Example 2: Creative Writing ---");
    let prompt = "Write a haiku about programming.";
    println!("Prompt: {prompt}");

    let response = session.respond(prompt, &options)?;
    println!("Response: {}", response.content());

    // Example 3: Ask about the weather (model doesn't have real-time data)
    println!("\n--- Example 3: Ask about weather ---");
    let prompt = "What should I wear today for a walk in the park?";
    println!("Prompt: {prompt}");

    let response = session.respond(prompt, &options)?;
    println!("Response: {}", response.content());

    println!("\n==============================");
    println!("Example completed successfully!");

    Ok(())
}
