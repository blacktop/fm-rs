//! Build script for fm-rs
//!
//! Compiles the Swift FFI layer into a static library and links it with Rust.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-env-changed=DEVELOPER_DIR");
    println!("cargo:rerun-if-env-changed=SWIFTC");

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

    let module_name = "fm_ffi";
    let out_dir = PathBuf::from(env::var("OUT_DIR")?);
    compile_swift(&target, &out_dir, module_name)?;

    // Tell cargo where the library is
    println!("cargo:rustc-link-lib=static={module_name}");
    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=framework=Foundation");
    println!("cargo:rustc-link-lib=framework=FoundationModels");
    // Image decoding for multimodal attachments (session_27_api.swift).
    println!("cargo:rustc-link-lib=framework=CoreGraphics");
    println!("cargo:rustc-link-lib=framework=ImageIO");
    // Built-in system tools via cross-import overlays (session_27_api.swift).
    println!("cargo:rustc-link-lib=framework=Vision");
    println!("cargo:rustc-link-lib=framework=CoreSpotlight");

    // Link Swift standard libraries
    let swift_target = get_swift_target(&target)?;
    if let Some(sdk_name) = get_sdk_name(&swift_target)
        && let Some(swift_lib_path) = get_swift_lib_path(sdk_name)
    {
        println!("cargo:rustc-link-search=native={swift_lib_path}");
    }

    Ok(())
}

/// Checks if the current platform is Apple
fn is_apple_platform() -> bool {
    env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "macos" || os == "ios")
}

fn compile_swift(
    target: &str,
    out_dir: &std::path::Path,
    module_name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let swift_target = get_swift_target(target)?;
    let sdk_name = get_sdk_name(&swift_target);
    let sdk_path = sdk_name.and_then(get_sdk_path);
    if let Some(path) = sdk_path.as_deref() {
        emit_sdk_change_tracking(std::path::Path::new(path));
        write_swift_concurrency_link_stub(std::path::Path::new(path), out_dir)?;
    }
    let sdk_version = sdk_name.and_then(get_sdk_version);
    let private_cloud_compute = env::var_os("CARGO_FEATURE_PRIVATE_CLOUD_COMPUTE").is_some();
    let sources = select_swift_sources(sdk_version.as_deref(), private_cloud_compute);
    let swift_output = out_dir.join(format!("lib{module_name}.a"));

    let mut args = vec![
        "-emit-library".to_string(),
        "-static".to_string(),
        "-module-name".to_string(),
        module_name.to_string(),
        "-swift-version".to_string(),
        "6".to_string(),
        "-o".to_string(),
        path_string(&swift_output)?,
        "-target".to_string(),
        swift_target,
    ];
    // Match the Cargo profile: without this the FFI layer (JSON codecs,
    // transcript encoding) ships at -Onone even in release builds.
    if let Some(flag) = swift_optimization_flag(env::var("OPT_LEVEL").ok().as_deref()) {
        args.push(flag.to_string());
    }
    if let Some(sdk) = sdk_path {
        args.extend(["-sdk".to_string(), sdk]);
    }
    for source in sources {
        args.push(path_string(&source)?);
    }

    // SECURITY: SWIFTC follows the same trusted build-time override model as
    // Cargo's CC, CXX, and RUSTC configuration.
    let swiftc = env::var("SWIFTC").unwrap_or_else(|_| "swiftc".to_string());
    println!("Compiling Swift code with: swiftc {}", args.join(" "));
    let status = Command::new(&swiftc).args(&args).status()?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("Swift compilation failed with status: {status}").into())
    }
}

fn swift_optimization_flag(opt_level: Option<&str>) -> Option<&'static str> {
    match opt_level {
        Some("s" | "z") => Some("-Osize"),
        Some("0") | None => None,
        Some(_) => Some("-O"),
    }
}

fn select_swift_sources(sdk_version: Option<&str>, private_cloud_compute: bool) -> Vec<PathBuf> {
    let version = sdk_version.and_then(|raw_version| {
        let version = parse_sdk_version(raw_version);
        if version.is_none() {
            println!(
                "cargo:warning=Failed to parse SDK version '{raw_version}'. Falling back to compatibility stubs."
            );
        }
        version
    });
    let supports_27 = version.is_some_and(sdk_supports_foundation_models_27);
    let token_usage = match version {
        Some(version) if sdk_supports_token_usage_api(version) => "src/swift/token_usage_api.swift",
        _ => "src/swift/token_usage_fallback.swift",
    };
    let private_cloud_compute_source = if private_cloud_compute && supports_27 {
        "src/swift/private_cloud_compute_api.swift"
    } else {
        "src/swift/private_cloud_compute_fallback.swift"
    };
    let reasoning_source = if supports_27 {
        "src/swift/reasoning_api.swift"
    } else {
        "src/swift/reasoning_fallback.swift"
    };
    let generation_options_source = if supports_27 {
        "src/swift/generation_options_api.swift"
    } else {
        "src/swift/generation_options_legacy.swift"
    };
    let session_27_source = if supports_27 {
        "src/swift/session_27_api.swift"
    } else {
        "src/swift/session_27_fallback.swift"
    };
    let sources = [
        "src/swift/ffi.swift",
        token_usage,
        private_cloud_compute_source,
        reasoning_source,
        generation_options_source,
        session_27_source,
    ]
    .map(PathBuf::from)
    .to_vec();

    for source in all_swift_sources() {
        println!("cargo:rerun-if-changed={}", source.display());
    }
    sources
}

fn all_swift_sources() -> Vec<PathBuf> {
    [
        "src/swift/ffi.swift",
        "src/swift/token_usage_api.swift",
        "src/swift/token_usage_fallback.swift",
        "src/swift/private_cloud_compute_api.swift",
        "src/swift/private_cloud_compute_fallback.swift",
        "src/swift/reasoning_api.swift",
        "src/swift/reasoning_fallback.swift",
        "src/swift/generation_options_api.swift",
        "src/swift/generation_options_legacy.swift",
        "src/swift/session_27_api.swift",
        "src/swift/session_27_fallback.swift",
    ]
    .map(PathBuf::from)
    .to_vec()
}

fn path_string(path: &std::path::Path) -> Result<String, Box<dyn std::error::Error>> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("Invalid UTF-8 path: {}", path.display()).into())
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

/// Gets the path to Swift runtime libraries for the given SDK.
fn get_swift_lib_path(sdk_name: &str) -> Option<String> {
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
    // Libraries are at: /path/to/toolchain/usr/lib/swift/<sdk>
    let toolchain_path = std::path::Path::new(&swift_path)
        .parent()? // usr/bin
        .parent()?; // usr

    let lib_path = toolchain_path.join("lib/swift").join(sdk_name);
    if lib_path.exists() {
        return Some(lib_path.to_string_lossy().into_owned());
    }

    None
}

/// Gets the selected SDK path.
fn get_sdk_path(sdk_name: &str) -> Option<String> {
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

/// Gets the selected SDK version with one `xcrun` invocation per build.
fn get_sdk_version(sdk_name: &str) -> Option<String> {
    let output = match Command::new("xcrun")
        .args(["--show-sdk-version", "--sdk", sdk_name])
        .output()
    {
        Ok(output) if output.status.success() => output,
        _ => {
            println!(
                "cargo:warning=Failed to query the '{sdk_name}' SDK version. Falling back to compatibility stubs."
            );
            return None;
        }
    };

    let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if version.is_empty() {
        println!(
            "cargo:warning=The '{sdk_name}' SDK version was empty. Falling back to compatibility stubs."
        );
        None
    } else {
        Some(version)
    }
}

/// Tracks the files that change when Xcode or the selected SDK is upgraded.
fn emit_sdk_change_tracking(sdk_path: &std::path::Path) {
    println!(
        "cargo:rerun-if-changed={}",
        sdk_path.join("SDKSettings.json").display()
    );

    if let Some(contents_path) = sdk_path
        .ancestors()
        .find(|path| path.file_name().is_some_and(|name| name == "Contents"))
    {
        println!(
            "cargo:rerun-if-changed={}",
            contents_path.join("version.plist").display()
        );
    }
}

/// Removes the concurrency back-deployment install-name override from the SDK
/// stub used by downstream Rust links.
///
/// Rust's macOS target defaults to an older deployment version, so the SDK
/// rewrites this one Swift runtime dependency to `@rpath`. The Foundation
/// Models framework requires macOS 26, where the runtime is in the dyld cache.
/// Using the SDK's current absolute install name avoids requiring link flags
/// from every final binary crate.
fn write_swift_concurrency_link_stub(
    sdk_path: &std::path::Path,
    out_dir: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    const PREVIOUS_INSTALL_NAME: &str = "$ld$previous$@rpath/libswift_Concurrency.dylib";
    const DISABLED_INSTALL_NAME: &str =
        "__fm_rs_disabled_previous_install_name$@rpath/libswift_Concurrency.dylib";

    let source = sdk_path.join("usr/lib/swift/libswift_Concurrency.tbd");
    println!("cargo:rerun-if-changed={}", source.display());
    let contents = fs::read_to_string(&source)?;
    let contents = contents.replace(PREVIOUS_INSTALL_NAME, DISABLED_INSTALL_NAME);
    fs::write(out_dir.join("libswift_Concurrency.tbd"), contents)?;
    Ok(())
}

fn get_sdk_name(swift_target: &str) -> Option<&'static str> {
    if swift_target.contains("macosx") {
        Some("macosx")
    } else if swift_target.contains("ios") && swift_target.contains("simulator") {
        Some("iphonesimulator")
    } else if swift_target.contains("ios") {
        Some("iphoneos")
    } else if swift_target.contains("xros") {
        Some("xros")
    } else if swift_target.contains("tvos") {
        Some("appletvos")
    } else if swift_target.contains("watchos") {
        Some("watchos")
    } else {
        None
    }
}

fn sdk_supports_token_usage_api(version: (u32, u32)) -> bool {
    sdk_version_is_at_least(version, 26, 4)
}

fn sdk_supports_foundation_models_27(version: (u32, u32)) -> bool {
    sdk_version_is_at_least(version, 27, 0)
}

fn parse_sdk_version(version: &str) -> Option<(u32, u32)> {
    let mut parts = version.split('.');
    let major = parts.next()?.parse::<u32>().ok()?;
    let minor = parts
        .next()
        .map_or(Some(0), |part| part.parse::<u32>().ok())?;
    Some((major, minor))
}

fn sdk_version_is_at_least(
    (major, minor): (u32, u32),
    required_major: u32,
    required_minor: u32,
) -> bool {
    major > required_major || (major == required_major && minor >= required_minor)
}
