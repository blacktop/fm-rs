<p align="center">
  <a href="https://github.com/blacktop/fm-rs"><img alt="Logo" src="./docs/logo.svg" height="400"/></a>
  <h1 align="center">fm-rs</h1>
  <h4><p align="center">Rust bindings for Apple’s <code>FoundationModels.framework</code></p></h4>
  <p align="center">
    <a href="https://github.com/blacktop/fm-rs/actions" alt="Actions">
          <img src="https://github.com/blacktop/fm-rs/actions/workflows/ci.yml/badge.svg" /></a>
    <a href="https://crates.io/crates/fm-rs" alt="Downloads">
          <img src="https://img.shields.io/crates/d/fm-rs" /></a>
    <a href="https://docs.rs/fm-rs" alt="Docs">
          <img src="https://img.shields.io/docsrs/fm-rs" /></a>
    <a href="http://doge.mit-license.org" alt="LICENSE">
          <img src="https://img.shields.io/:license-mit-blue.svg" /></a>
</p>
<br>

## Requirements

- macOS 26+ or iOS/iPadOS 26+
- Apple Intelligence enabled
- Apple Silicon device
- Rust 1.85+ (edition 2024)

## Installation

```toml
[dependencies]
fm-rs = "0.1"
```

Enable the derive macro if you want compile-time schema generation:

```toml
[dependencies]
fm-rs = { version = "0.1", features = ["derive"] }
```

## Quick Start

```rust
use fm_rs::{GenerationOptions, Session, SystemLanguageModel};

fn main() -> Result<(), fm_rs::Error> {
    let model = SystemLanguageModel::new()?;
    model.ensure_available()?;

    let session = Session::with_instructions(&model, "You are a helpful assistant.")?;
    let options = GenerationOptions::builder()
        .temperature(0.7)
        .max_response_tokens(500)
        .build();

    let response = session.respond("What is the capital of France?", &options)?;
    println!("{}", response.content());

    Ok(())
}
```

## Key Features

- Blocking and streaming responses
- Tool calling with JSON argument schemas
- Structured JSON output (explicit schemas or derive macro)
- Transcript persistence and restoration
- Context usage estimates and compaction helpers
- Prewarming and timeout-aware respond APIs

## Runtime Notes (macOS)

- FFI calls are synchronous. Use `spawn_blocking` in async runtimes.
- If you see `libswift_Concurrency.dylib` load errors, add Swift rpaths in your binary crate’s `build.rs`.

```rust
use std::process::Command;

fn main() {
    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");

    if let Ok(output) = Command::new("xcrun")
        .args(["--toolchain", "default", "--find", "swift"])
        .output()
    {
        let swift_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if let Some(toolchain) = std::path::Path::new(&swift_path).parent().and_then(|p| p.parent()) {
            let lib_path = toolchain.join("lib/swift/macosx");
            if lib_path.exists() {
                println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib_path.display());
            }
        }
    }
}
```

## Prompting Guidance

For best results on-device:

- Keep prompts focused and explicit about length and style.
- Prefer instructions for stable behavior across prompts.
- Use small examples when you need consistent formatting.
- Break complex tasks into smaller steps.
- Use structured output when you need reliable parsing.

See Apple’s guidance for more detail:
- https://developer.apple.com/documentation/foundationmodels/prompting-an-on-device-foundation-model
- https://developer.apple.com/videos/play/wwdc2024/10150/
- https://developer.apple.com/videos/play/wwdc2024/10163/

## Examples

```bash
cargo run --example basic
cargo run --example streaming
cargo run --example tools
cargo run --example structured
cargo run --example context_compaction
```

## Documentation

See API details and advanced usage in the crate docs (docs.rs).

## License

MIT License - see [LICENSE](LICENSE) for details.
