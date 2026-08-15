//! Repro: is a tool-enabled Session's `ToolCallbackData` ever reclaimed?
//!
//! `Session::new` hands Swift a strong `Arc<ToolCallbackData>` via
//! `Arc::into_raw`. If nothing gives it back, that reference — and every
//! `Arc<dyn Tool>` in its map — lives for the life of the process.
//!
//! RSS is useless for measuring this: a Swift session object dwarfs the leaked
//! Arc. So count `Drop`s of the tool itself, which is only reachable through
//! that map. N sessions created and dropped should produce N tool drops.
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use fm_rs::{Result, Session, SystemLanguageModel, Tool, ToolOutput};

static DROPPED: AtomicUsize = AtomicUsize::new(0);

struct Counted;

impl Drop for Counted {
    fn drop(&mut self) {
        DROPPED.fetch_add(1, Ordering::SeqCst);
    }
}

impl Tool for Counted {
    fn name(&self) -> &str { "counted" }
    fn description(&self) -> &str { "does nothing; counts its own drop" }
    fn arguments_schema(&self) -> serde_json::Value { serde_json::json!({"type":"object"}) }
    fn call(&self, _args: serde_json::Value) -> Result<ToolOutput> { Ok(ToolOutput::new("{}")) }
}

fn main() {
    let model = match SystemLanguageModel::new() {
        Ok(m) if m.is_available() => m,
        _ => { println!("SKIP: Foundation Models unavailable"); return; }
    };
    let n: usize = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(50);

    for _ in 0..n {
        let session = Session::builder(&model).tool(Arc::new(Counted)).build();
        drop(session);
    }

    let dropped = DROPPED.load(Ordering::SeqCst);
    println!("sessions created+dropped: {n}");
    println!("tools dropped:            {dropped}");
    println!(
        "verdict: {}",
        if dropped == n { "OK — every tool reclaimed" } else { "LEAK — tools never reclaimed" }
    );
    std::process::exit(if dropped == n { 0 } else { 1 });
}
