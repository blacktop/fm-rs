//! Example: Context window tracking and compaction
//!
//! This example demonstrates how to estimate context usage from a session
//! transcript and compact it into a summary when you exceed a limit.
//!
//! To run this example:
//!   `cargo run --example context_compaction`

use fm_rs::{
    CompactionConfig, ContextLimit, GenerationOptions, Result, Session, SystemLanguageModel,
    compact_transcript,
};

fn main() -> Result<()> {
    println!("=== FoundationModels - Context Compaction Example ===\n");

    let model = SystemLanguageModel::new()?;
    model.ensure_available()?;

    let session = Session::with_instructions(
        &model,
        "You are a helpful assistant. Keep responses concise.",
    )?;

    let options = GenerationOptions::builder()
        .temperature(0.5)
        .max_response_tokens(200)
        .build();

    let _ = session.respond("Give me a quick overview of Tokyo.", &options)?;
    let _ = session.respond("What are two must-see attractions?", &options)?;
    let _ = session.respond("What's a good time of year to visit?", &options)?;

    // Use a small limit to force compaction in this example.
    let limit = ContextLimit::new(256).with_reserved_response_tokens(64);
    let usage = session.context_usage(&limit)?;

    println!(
        "Estimated context usage: {} / {} tokens (reserved: {})",
        usage.estimated_tokens, usage.max_tokens, usage.reserved_response_tokens
    );

    if usage.over_limit {
        println!("\nContext limit exceeded — compacting transcript...\n");
        let transcript_json = session.transcript_json()?;
        let summary = compact_transcript(&model, &transcript_json, &CompactionConfig::default())?;
        println!("Summary:\n{summary}\n");

        let instructions = format!("You are a helpful assistant. Conversation summary:\n{summary}");
        let compacted = Session::with_instructions(&model, &instructions)?;
        let response = compacted.respond("Any food recommendations?", &options)?;
        println!("Assistant: {}", response.content());
    } else {
        println!("Context is within the configured limit.");
    }

    Ok(())
}
