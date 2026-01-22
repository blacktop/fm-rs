//! Build script for fm-python
//!
//! Sets up rpath for Swift runtime libraries required by fm-rs.

use std::env;
use std::process::Command;

fn main() {
    // Only run on Apple platforms
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos")
        && env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("ios")
    {
        return;
    }

    // Set rpath for Swift standard libraries
    if let Some(swift_lib_path) = get_swift_lib_path() {
        println!("cargo:rustc-link-arg=-Wl,-rpath,{swift_lib_path}");
    }

    // Also add rpath for system Swift libraries (needed for Swift Concurrency)
    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
}

fn get_swift_lib_path() -> Option<String> {
    // Try to find Swift lib path from xcrun
    let output = Command::new("xcrun")
        .args(["--show-sdk-path"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let sdk_path = String::from_utf8(output.stdout).ok()?.trim().to_string();
    Some(format!("{sdk_path}/usr/lib/swift"))
}
