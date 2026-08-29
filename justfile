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
    OXIDGENE_API_URL="${OXIDGENE_API_URL:-http://127.0.0.1:8080}" dx serve --package oxidgene-web --platform web --port 8081

# Build the production browser bundle
web-build:
    dx build --package oxidgene-web --platform web --release

# Run the backend and browser frontend together (frontend hot reload)
dev-web:
    bash scripts/dev-web.sh

# Run the backend and browser frontend with hot reload for both
dev-web-watch:
    bash scripts/dev-web.sh --watch-backend

# Run the desktop app (dev mode)
desktop:
    cargo run --package oxidgene-desktop

# Build the desktop app in release mode
build-desktop-release:
    cargo build --release --package oxidgene-desktop


# Generate documentation
doc:
    cargo doc --workspace --no-deps --open
