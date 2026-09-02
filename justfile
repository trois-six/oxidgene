# OxidGene - Justfile
# Build orchestration for the OxidGene genealogy platform.

# Default recipe: show available commands
default:
    @just --list

# Install the project development tools and Rust targets
setup:
    @command -v mise >/dev/null 2>&1 || { echo "mise is required: https://mise.jdx.dev/getting-started.html" >&2; exit 1; }
    @command -v rustup >/dev/null 2>&1 || { echo "rustup is required: https://rustup.rs" >&2; exit 1; }
    mise install
    rustup target add wasm32-unknown-unknown

# Build all workspace crates
build:
    cargo build

# Build in release mode
build-release:
    cargo build --release

# Run all tests (requires cargo-nextest: cargo install cargo-nextest --locked)
test:
    cargo nextest run --workspace

# Run tests with output
test-verbose:
    cargo nextest run --workspace --no-capture

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

# Regenerate the OpenAPI specification from the REST router
openapi:
    cargo build --package oxidgene-api

# Clean build artifacts
clean:
    cargo clean

# Run the web server (dev mode)
server:
    cargo run --package oxidgene-server

# Start the PostgreSQL development database
dev-db-up:
    docker compose -f docker/docker-compose.yml up -d --wait postgres

# Stop the PostgreSQL development database without deleting its data
dev-db-down:
    docker compose -f docker/docker-compose.yml stop postgres

# Check the browser frontend for its actual WebAssembly target
web-check:
    cargo check --package oxidgene-web --target wasm32-unknown-unknown

# Run the browser frontend against the development backend on port 8080
web:
    OXIDGENE_API_URL="${OXIDGENE_API_URL:-http://127.0.0.1:8080}" scripts/dx.sh serve --package oxidgene-web --platform web --port 8081

# Build the production browser bundle
web-build:
    scripts/dx.sh build --package oxidgene-web --platform web --release

# Run the backend and browser frontend together (frontend hot reload)
dev-web:
    bash scripts/dev-web.sh

# Run the backend and browser frontend with hot reload for both
dev-web-watch:
    bash scripts/dev-web.sh --watch-backend

# Run the desktop app (dev mode)
desktop:
    cargo run --package oxidgene-desktop

# Start the local collector and run the desktop app with telemetry enabled
# examples: `just desktop-telemetry debug` or `just desktop-telemetry 'info,oxidgene_api=debug,sea_orm=warn'`
desktop-telemetry log_level="info":
    docker compose -f docker/docker-compose.yml up -d --wait otel-collector
    OXIDGENE_LOG_LEVEL="{{log_level}}" OTEL_EXPORTER_OTLP_ENDPOINT="http://127.0.0.1:4317" cargo run --package oxidgene-desktop

# Build an optimized desktop release with runtime-optional OTLP telemetry
build-desktop-release:
    cargo build --release --package oxidgene-desktop

# Generate documentation
doc:
    cargo doc --workspace --no-deps --open
