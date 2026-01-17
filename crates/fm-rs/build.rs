//! Build script for fm-rs
//!
//! Compiles the Swift FFI layer into a static library and links it with Rust.

use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Skip build on docs.rs
    if env::var("DOCS_RS").is_ok() {
        println!("cargo:warning=Skipping Swift compilation on docs.rs");
        return Ok(());
    }

    // Only build on Apple Intelligence platforms
    if !is_apple_platform() {
        println!(
            "cargo:warning=fm-rs only supports Apple Intelligence platforms (macOS, iOS/iPadOS). Skipping build."
        );
        return Ok(());
    }

    let target = env::var("TARGET")?;
    if !is_supported_target(&target) {
        return Err(format!(
            "fm-rs requires Apple Intelligence hardware; unsupported target '{target}'. Use Apple Silicon (aarch64) on macOS/iOS/iPadOS."
        )
        .into());
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR")?);
    let swift_src = PathBuf::from("src/swift/ffi.swift");
    let module_name = "fm_ffi";
    let lib_name = format!("lib{module_name}.a");

    println!("cargo:rerun-if-changed={}", swift_src.display());

    // Determine Swift compiler
    // SECURITY: SWIFTC is trusted build-time configuration. Command::new() does not
    // use shell expansion, so this is safe from command injection. However, an attacker
    // with env var control could point to a malicious binary - this is inherent to any
    // configurable compiler path and matches cargo's own CC/CXX/RUSTC behavior.
    let swiftc = env::var("SWIFTC").unwrap_or_else(|_| "swiftc".to_string());

    // Compile Swift to static library
    let swift_output = out_dir.join(&lib_name);
    let swift_target = get_swift_target(&target)?;

    // Get SDK path from xcrun
    let sdk_path = get_sdk_path(&swift_target);

    let swift_output_str = swift_output.to_str().ok_or("Invalid output path")?;
    let swift_src_str = swift_src.to_str().ok_or("Invalid Swift source path")?;

    let mut swift_args: Vec<String> = vec![
        "-emit-library".to_string(),
        "-static".to_string(),
        "-module-name".to_string(),
        module_name.to_string(),
        "-o".to_string(),
        swift_output_str.to_string(),
        "-target".to_string(),
        swift_target.clone(),
    ];

    // Add SDK path if available
    if let Some(ref sdk) = sdk_path {
        swift_args.push("-sdk".to_string());
        swift_args.push(sdk.clone());
    }

    swift_args.push(swift_src_str.to_string());

    println!("Compiling Swift code with: swiftc {}", swift_args.join(" "));

    let status = Command::new(&swiftc).args(&swift_args).status()?;
    if !status.success() {
        return Err(format!("Swift compilation failed with status: {status}").into());
    }

    // Tell cargo where the library is
    println!("cargo:rustc-link-lib=static={module_name}");
    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=framework=Foundation");
    println!("cargo:rustc-link-lib=framework=FoundationModels");

    // Link Swift standard libraries
    if let Some(swift_lib_path) = get_swift_lib_path() {
        println!("cargo:rustc-link-search=native={swift_lib_path}");
        // Set rpath for dynamic Swift libraries
        println!("cargo:rustc-link-arg=-Wl,-rpath,{swift_lib_path}");
    }

    // Also add rpath for system Swift libraries (needed for Swift Concurrency on macOS 26+)
    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");

    Ok(())
}

/// Checks if the current platform is Apple
fn is_apple_platform() -> bool {
    env::var("CARGO_CFG_TARGET_OS")
        .map(|os| os == "macos" || os == "ios")
        .unwrap_or(false)
}

fn is_supported_target(target: &str) -> bool {
    matches!(
        target,
        "aarch64-apple-darwin" | "aarch64-apple-ios" | "aarch64-apple-ios-sim"
    )
}

/// Gets the appropriate Swift target triple for the current Rust target
fn get_swift_target(target: &str) -> Result<String, Box<dyn std::error::Error>> {
    // Map Rust target to Swift target with minimum OS version (macOS 26.0+)
    let swift_target = match target {
        "aarch64-apple-darwin" => "arm64-apple-macosx26.0",
        "aarch64-apple-ios" => "arm64-apple-ios26.0",
        "aarch64-apple-ios-sim" => "arm64-apple-ios26.0-simulator",
        _ => {
            return Err(
                format!("Unsupported Apple target '{target}' for FoundationModels.").into(),
            );
        }
    };

    Ok(swift_target.to_string())
}

/// Gets the path to Swift runtime libraries.
fn get_swift_lib_path() -> Option<String> {
    // Try to get the path from xcrun
    let output = Command::new("xcrun")
        .args(["--toolchain", "default", "--find", "swift"])
        .output()
        .ok()?;

    let swift_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if swift_path.is_empty() {
        return None;
    }

    // Swift binary is at: /path/to/toolchain/usr/bin/swift
    // Libraries are at: /path/to/toolchain/usr/lib/swift/macosx
    let toolchain_path = std::path::Path::new(&swift_path)
        .parent()? // usr/bin
        .parent()?; // usr

    let lib_path = toolchain_path.join("lib/swift/macosx");
    if lib_path.exists() {
        return Some(lib_path.to_string_lossy().into_owned());
    }

    None
}

/// Gets the SDK path for the given Swift target
fn get_sdk_path(swift_target: &str) -> Option<String> {
    let sdk_name = if swift_target.contains("macosx") {
        "macosx"
    } else if swift_target.contains("ios") && swift_target.contains("simulator") {
        "iphonesimulator"
    } else if swift_target.contains("ios") {
        "iphoneos"
    } else if swift_target.contains("xros") {
        "xros"
    } else if swift_target.contains("tvos") {
        "appletvos"
    } else if swift_target.contains("watchos") {
        "watchos"
    } else {
        return None;
    };

    let output = Command::new("xcrun")
        .args(["--show-sdk-path", "--sdk", sdk_name])
        .output()
        .ok()?;

    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() || path.contains("cannot be found") {
        None
    } else {
        Some(path)
    }
}
