# OxidGene - Justfile
# Build orchestration for the OxidGene genealogy platform.

# Default recipe: show available commands
default:
    @just --list

# Build all workspace crates
build:
    cargo build

# Build in release mode
build-release:
    cargo build --release

# Run all tests
test:
    cargo test --workspace

# Run tests with output
test-verbose:
    cargo test --workspace -- --nocapture

# Run clippy linter
clippy:
    cargo clippy --workspace --all-targets -- -D warnings

# Format code
fmt:
    cargo fmt --all

# Check formatting without modifying files
fmt-check:
    cargo fmt --all -- --check

# Run all checks (fmt + clippy + test)
check: fmt-check clippy test

# Clean build artifacts
clean:
    cargo clean

# Run the web server (dev mode)
server:
    cargo run --package oxidgene-server

# Run the desktop app (dev mode)
desktop:
    cargo run --package oxidgene-desktop

# Run the CLI tool
cli *ARGS:
    cargo run --package oxidgene-cli -- {{ARGS}}

# Generate documentation
doc:
    cargo doc --workspace --no-deps --open
