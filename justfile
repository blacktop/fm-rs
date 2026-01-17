# fm-rs justfile
# Run `just` to see available recipes

set shell := ["bash", "-cu"]

# Default recipe - show help
default:
    @just --list

# Build all crates
build:
    cargo build --all-features

# Build in release mode
build-release:
    cargo build --release --all-features

# Run all tests (unit tests only, no Apple Intelligence required)
test:
    cargo test --all-features

# Run integration tests (requires Apple Intelligence device)
test-integration:
    cargo test --all-features -- --ignored

# Run all tests including integration
test-all:
    cargo test --all-features -- --include-ignored

# Run clippy lints
clippy:
    cargo clippy --all --benches --tests --examples --all-features -- -D warnings

# Format code
fmt:
    cargo fmt --all

# Check formatting without modifying
fmt-check:
    cargo fmt --all -- --check

# Run all checks (format, clippy, test)
check: fmt clippy test

# Build documentation
doc:
    cargo doc --no-deps --all-features

# Build and open documentation
doc-open:
    cargo doc --no-deps --all-features --open

# Clean build artifacts
clean:
    cargo clean

# Run the basic example
example-basic:
    cargo run --example basic

# Run the tools example
example-tools:
    cargo run --example tools --features derive

# Run the streaming example
example-streaming:
    cargo run --example streaming

# Run the context example
example-context:
    cargo run --example context

# Publish fm-rs-derive to crates.io (run first)
publish-derive:
    cargo publish -p fm-rs-derive

# Publish fm-rs to crates.io (run after publish-derive)
publish-main:
    cargo publish -p fm-rs --all-features

# Publish all crates to crates.io (derive first, then main)
publish: publish-derive
    @echo "Waiting 30s for crates.io to index fm-rs-derive..."
    @sleep 30
    just publish-main
    @echo "Published successfully!"

# Dry-run publish fm-rs-derive
publish-derive-dry:
    cargo publish -p fm-rs-derive --dry-run --allow-dirty

# Dry-run publish fm-rs (only works after fm-rs-derive is on crates.io)
publish-main-dry:
    cargo publish -p fm-rs --all-features --dry-run --allow-dirty

# Create a new version tag and push (usage: just tag 0.1.0)
tag version:
    @echo "Creating tag v{{ version }}..."
    git tag -a "v{{ version }}" -m "Release v{{ version }}"
    git push origin "v{{ version }}"
    @echo "Tag v{{ version }} pushed. GitHub Actions will handle the release."

# Show current version from Cargo.toml
version:
    @grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/'

# Bump patch version, commit, tag, and push (requires cargo-release: cargo install cargo-release)
bump: bump-patch

# Bump patch version (0.1.0 -> 0.1.1)
bump-patch:
    cargo release patch --execute --no-publish

# Bump minor version (0.1.0 -> 0.2.0)
bump-minor:
    cargo release minor --execute --no-publish

# Bump major version (0.1.0 -> 1.0.0)
bump-major:
    cargo release major --execute --no-publish

# Preview what bump would do (dry-run)
bump-dry level="patch":
    cargo release {{ level }}

# Update dependencies
update:
    cargo update

# Run cargo-deny checks (requires cargo-deny installed)
deny:
    cargo deny check

# Generate code coverage (requires cargo-llvm-cov installed)
coverage:
    cargo llvm-cov --all-features --html
    @echo "Coverage report: target/llvm-cov/html/index.html"
