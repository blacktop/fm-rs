//! Example: Streaming responses from `FoundationModels`
//!
//! This example demonstrates how to stream responses chunk-by-chunk
//! as they are generated, which provides better user experience for
//! longer responses.
//!
//! To run this example:
//!   `cargo run --example streaming`

use fm_rs::{GenerationOptions, Session, SystemLanguageModel};
use std::io::{self, Write};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("FoundationModels Streaming Example");
    println!("===================================\n");

    // Create the default system language model
    let model = SystemLanguageModel::new()?;

    if !model.is_available() {
        println!("FoundationModels is not available on this device.");
        return Ok(());
    }

    // Create a session
    let session = Session::with_instructions(
        &model,
        "You are a creative storyteller. Tell engaging, vivid stories.",
    )?;

    let options = GenerationOptions::builder()
        .temperature(0.8)
        .max_response_tokens(300)
        .build();

    // Streaming example - print each chunk as it arrives
    println!("Prompt: Tell me a short story about a robot learning to paint.\n");
    println!("Response (streaming):");
    println!("----------------------");

    session.stream_response(
        "Tell me a short story about a robot learning to paint.",
        &options,
        |chunk| {
            // Print each chunk immediately without a newline
            print!("{chunk}");
            // Flush to ensure immediate display
            let _ = io::stdout().flush();
        },
    )?;

    println!("\n----------------------");
    println!("\nStreaming complete!");

    Ok(())
}
