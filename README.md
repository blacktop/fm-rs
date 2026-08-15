<p align="center">
  <a href="https://github.com/blacktop/fm-rs"><img alt="Logo" src="https://raw.githubusercontent.com/blacktop/fm-rs/refs/heads/main/docs/logo.svg" height="400"/></a>
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

Private Cloud Compute additionally requires macOS/iOS/iPadOS 27+, a network
connection, and Apple's managed
[`com.apple.developer.private-cloud-compute`](https://developer.apple.com/documentation/bundleresources/entitlements/com.apple.developer.private-cloud-compute)
entitlement. The crate still targets 26.0; 27-only Swift APIs are selected by
the build SDK and guarded at runtime.

## Installation

```toml
[dependencies]
fm-rs = "0.2"
```

Enable the derive macro if you want compile-time schema generation:

```toml
[dependencies]
fm-rs = { version = "0.2", features = ["derive"] }
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
- Tool calling with JSON argument schemas and tool-calling modes
- Structured JSON output (explicit schemas or derive macro)
- Transcript persistence, restoration, replacement, and error policy
- Context usage estimates and compaction/rollover helpers
- Model-reported context sizes and exact session token usage (27+)
- Multimodal image attachments and built-in OCR/barcode/Spotlight tools (27+)
- Typed Foundation Models error taxonomy on blocking and streaming paths
- Prewarming and timeout-aware respond APIs
- Private Cloud Compute sessions on macOS/iOS/iPadOS 27+

## Private Cloud Compute (27+)

Private Cloud Compute support is intentionally excluded from the default
build. Per [Apple's eligibility requirements](https://developer.apple.com/private-cloud-compute/),
PCC access requires:

- Enrollment in the App Store Small Business Program
- Fewer than two million first-time downloads across your apps
- The managed entitlement assigned to your account and enabled for the App ID

Enable the `private-cloud-compute` feature only for applications whose
App ID has Apple's managed entitlement:

```toml
[dependencies]
fm-rs = { version = "0.2", features = ["private-cloud-compute"] }
```

```rust
use fm_rs::{
    GenerationOptions, PrivateCloudComputeLanguageModel, ReasoningLevel, Session,
};

fn main() -> Result<(), fm_rs::Error> {
    let model = PrivateCloudComputeLanguageModel::new()?;
    model.ensure_available()?;

    // PCC has a daily, per-user quota; check it to decide when to fall
    // back to the on-device SystemLanguageModel.
    if model.quota_usage()?.is_limit_reached() {
        return Err(fm_rs::Error::QuotaLimitReached("daily quota".into()));
    }

    let session = Session::new(&model)?;
    let response = session.respond_with_reasoning(
        "Compare these approaches and explain the tradeoffs.",
        &GenerationOptions::default(),
        // Start with Moderate; use Deep only when the added latency and
        // context consumption are justified.
        ReasoningLevel::Moderate,
    )?;

    println!("{}", response.content());
    Ok(())
}
```

On an older SDK or runtime, construction returns
`Error::UnsupportedPlatform`. Availability can also report
`PrivateCloudComputeSystemNotReady` when PCC cannot currently serve requests.
PCC request failures surface as typed errors — `Error::NetworkFailure`,
`Error::QuotaLimitReached`, and `Error::ServiceUnavailable` — so applications
can retry or fall back to the on-device model. `quota_usage()` reports the
current quota status and reset date, and `context_size()` reports the PCC
context window.

Running the example needs the managed
`com.apple.developer.private-cloud-compute` entitlement, which Apple assigns
per developer account and App ID; without it, signing fails with a
provisioning-profile error.

PCC is a managed capability: Apple must assign the entitlement to your App ID,
and its provisioning profile must authorize the final executable's signature.
An ad-hoc signature is rejected by macOS because this is a restricted
entitlement. A standalone executable cannot embed the required profile, so the
example runner wraps the executable in a minimal app bundle before signing it.
The repository includes the required Boolean entitlement in
`crates/fm-rs/examples/private_cloud_compute.entitlements`.
To build, automatically provision, sign, and run the repository example:

```fish
brew install xcodegen
just example-pcc
```

The runner passes the existing `DEVELOPMENT_TEAM` environment variable to an
Xcode-managed macOS app target. With an Apple account configured in Xcode,
`xcodebuild -allowProvisioningUpdates` creates or selects the profile and embeds
it in the signed app automatically. On the first build, Xcode may also register
the Mac with the developer account so it can be included in the development
profile. Set `PCC_BUNDLE_IDENTIFIER` only if the PCC capability is enabled for
an App ID other than the default
`com.blacktop.fm-rs.pcc-example`.

The profile's App ID must have the PCC managed capability enabled. A library
`build.rs` cannot provision or sign the final consuming application, so
applications should perform this step in their own app build or distribution
pipeline.

## macOS 27 Capabilities

These APIs work with the on-device model — no PCC entitlement needed. They
require building with the macOS/iOS 27 SDK. On older SDKs or runtimes, session
APIs return `Error::UnsupportedPlatform`; `ToolCallingMode` is ignored and the
framework keeps its default allowed behavior.

On the observed 8,192-token on-device context window, enable one built-in
system tool at a time. Combined tool schemas can exhaust the context window
before the prompt is processed.

For reliable image-tool invocation (OCR, barcode), assign attachments a label
with `.with_label(...)` and reference that exact label in the prompt. Without
an explicit label, the model may request an image label that does not exist and
the tool call fails. The example below uses `receipt` in both places.

```rust
use fm_rs::{
    Attachment, ContextLimit, GenerationOptions, Session, SystemLanguageModel, SystemTool,
    ToolCallingMode,
};

fn main() -> Result<(), fm_rs::Error> {
    let model = SystemLanguageModel::new()?;
    model.ensure_available()?;

    // Model-reported context window (falls back to 4096 on older SDKs).
    let limit = ContextLimit::for_model(&model)?;
    println!("context window: {} tokens", limit.max_tokens);

    // Built-in system tools run inside the framework.
    let session = Session::builder(&model)
        .instructions("Describe images and read any text in them.")
        .system_tool(SystemTool::Ocr)
        .build()?;

    // Multimodal prompting with image attachments.
    let options = GenerationOptions::builder()
        .tool_calling_mode(ToolCallingMode::Allowed)
        .build();
    let response = session.respond_with_attachments(
        "What does this receipt say?",
        &[Attachment::file("receipt.png").with_label("receipt")],
        &options,
    )?;
    println!("{}", response.content());

    // Exact token usage (input, cached, output, reasoning).
    let usage = session.usage()?;
    println!("input tokens: {}", usage.input_tokens);

    Ok(())
}
```

Failures surface as typed errors on both blocking and streaming paths:
`ContextSizeExceeded`, `RateLimited`, `GuardrailViolation`, `Refusal`,
`UnsupportedCapability`, `UnsupportedTranscriptContent`,
`UnsupportedGenerationGuide`, `UnsupportedLanguageOrLocale`,
`AssetsUnavailable`, `ConcurrentRequests`, and `Timeout`. Sessions also
support transcript replacement (`Session::set_transcript`) and
`TranscriptErrorHandlingPolicy`.

### Coverage

**Legend:** ✅ Implemented · 🟡 Partial · ⏳ Deferred · ➖ Automatic · ❌ Out of scope

| Foundation Models 27 API | Status |
|--------------------------|--------|
| Private Cloud Compute model, quota, availability | ✅ Implemented (`private-cloud-compute` feature) |
| Extended reasoning via `ContextOptions` | ✅ Implemented (`respond_with_reasoning`, `_timeout`) |
| Typed error taxonomy (`LanguageModelError`, PCC, session, model errors) | ✅ Implemented, blocking + streaming |
| 26-era `GenerationError` bridging (macOS/iOS 26 runtimes) | ✅ Implemented — same typed errors on all runtimes |
| `SystemLanguageModel.contextSize` / PCC `contextSize` | ✅ Implemented (`context_size`, `ContextLimit::for_model`) |
| Exact session token usage | ✅ Implemented (`Session::usage`, `SessionUsage::delta_since`) |
| Per-`Response` usage property | 🟡 Partial — implemented for blocking respond paths, including `respond_json_with_timeout` |
| Python bindings parity | 🟡 Partial — typed errors, context size, usage, tool modes, attachments, system tools, and transcript controls are implemented; PCC remains Rust-only |
| Tool-calling modes (allowed/required/disallowed) | ✅ Implemented (`ToolCallingMode`) |
| Multimodal image attachments | 🟡 Partial — blocking responds implemented; streaming/structured variants deferred |
| Built-in tools: Vision OCR, barcode, Spotlight search | ✅ Implemented with Apple's default configurations |
| Built-in tool configuration (sources, guides, delegates) | ⏳ Deferred — Swift delegate/closure hooks do not cross the C FFI |
| Transcript replacement and error policy | ✅ Implemented |
| Dynamic Profiles | ⏳ Deferred — Swift result-builder DSL maps poorly onto a C FFI |
| Custom `LanguageModel` providers / executors, including Core AI and MLX adapters | ⏳ Deferred — requires Rust-side executor callbacks; architectural |
| Foundation Models framework utilities | ⏳ Deferred with Dynamic Profiles; this is a separate Swift package |
| Foundation Models Instruments updates | ➖ Automatic for profiled consumers; no crate API required |
| Apple `fm` CLI, Python SDK, and Evaluations framework | ❌ Out of scope — separate Apple tooling, not reimplemented by this Rust binding |
| New on-device base model | ➖ Automatic via `SystemLanguageModel` |

## Runtime Notes (macOS)

- FFI calls are synchronous. Use `spawn_blocking` in async runtimes.
- macOS 27 built-in-tool schema overflows are classified as `ContextSizeExceeded`
  on blocking and streaming paths, including the runtime's private bridged error.
- Cargo consumers do not need to add Swift runtime rpaths to their binary
  targets; fm-rs links the system Swift Concurrency runtime directly.

## Prompting Guidance

For best results on-device:

- Keep prompts focused and explicit about length and style.
- Prefer instructions for stable behavior across prompts.
- Use small examples when you need consistent formatting.
- Break complex tasks into smaller steps.
- Use structured output when you need reliable parsing.

See Apple’s guidance for more detail:
- https://developer.apple.com/documentation/foundationmodels/prompting-an-on-device-foundation-model
- https://developer.apple.com/documentation/technotes/tn3193-managing-the-on-device-foundation-model-s-context-window
- https://developer.apple.com/videos/play/wwdc2024/10150/
- https://developer.apple.com/videos/play/wwdc2024/10163/

## FoundationModels Updates

This crate tracks Apple FoundationModels updates from:
- https://developer.apple.com/documentation/updates/foundationmodels

Current coverage includes context-window management helpers aligned with TN3193:
- `Session::context_usage(...)`
- `compact_transcript(...)`
- `compact_session_if_needed(...)`
- `session_from_summary(...)`

## Examples

```bash
cargo run --example basic
cargo run --example streaming
cargo run --example tools
cargo run --example structured
cargo run --example context_compaction
just example-pcc
```

## Documentation

See API details and advanced usage in the crate docs (docs.rs).

## License

MIT License - see [LICENSE](LICENSE) for details.
