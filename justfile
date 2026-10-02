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
    rustup target add x86_64-pc-windows-msvc

# Build all workspace crates
build:
    cargo build

# Build in release mode
build-release:
    cargo build --release

# Ports of the e2e suite's backend and web bundle, away from the 8080/8081 of
# `just dev-web` so both can run at once.
e2e_api_port := env("E2E_API_PORT", "18080")
e2e_web_port := env("E2E_WEB_PORT", "18081")

# Performance and browser tests have their own recipes: `just scaling`,
# `just e2e`, `just ui-js`. All Rust recipes need cargo-nextest (`just setup`).
# Run the unit and functional tests (what `just check` runs)
test: test-unit test-functional

# Covers the `#[cfg(test)]` modules of every library and binary, then the
# documentation examples, which nextest does not run.
# Run the unit tests
test-unit:
    cargo nextest run --workspace --lib --bins
    cargo test --workspace --doc

# The integration test targets under `crates/*/tests` and `apps/*/tests`:
# REST, GraphQL, repository, import and MCP scenarios.
# Run the functional tests
test-functional:
    cargo nextest run --workspace --test '*'

# Run the browser JavaScript unit tests (Node.js, no dependencies)
ui-js:
    node --test crates/oxidgene-ui/tests/*.test.mjs

# Builds the web bundle for the e2e API port and the server, then runs the
# Playwright suite in `e2e/` against both on a throwaway database (see
# docs/development.md). Arguments go to Playwright: `just e2e --headed`,
# `just e2e tests/home.spec.ts`.
# Run the browser end-to-end tests
e2e *args:
    OXIDGENE_API_URL="http://127.0.0.1:{{ e2e_api_port }}" scripts/dx.sh build --package oxidgene-web --platform web
    cargo build --locked --package oxidgene-server
    cd e2e && npm ci --no-audit --no-fund && npx playwright install chromium
    cd e2e && E2E_API_PORT="{{ e2e_api_port }}" E2E_WEB_PORT="{{ e2e_web_port }}" npx playwright test {{ args }}

# Run tests with output
test-verbose:
    cargo nextest run --workspace --no-capture

# Run clippy linter
clippy:
    cargo clippy --workspace --all-targets -- -D warnings

# Check that the shared UI still compiles to WASM.
#
# `clippy --workspace` only ever builds the host target, so a dependency that
# exists on desktop but not on the web — tokio, say — breaks the web build
# without any native check noticing. `oxidgene-ui` is platform-independent by
# contract (see AGENTS.md); run this before touching its dependencies.
# Deliberately not part of `check` — the web target is not a current priority.
wasm:
    cargo clippy -p oxidgene-web --target wasm32-unknown-unknown --all-targets -- -D warnings

# Format code
fmt:
    cargo fmt --all

# Check formatting without modifying files
fmt-check:
    cargo fmt --all -- --check

# Fail on any function whose cyclomatic complexity exceeds 15: lizard, run
# through scripts/cyclomatic.py, which stops it from counting Rust closures
# and `where` clauses as branches (see docs/development.md).
cyclomatic:
    uv run --quiet scripts/cyclomatic.py -l rust -C 15 -w crates apps -x "*/target/*"

# Run all checks (fmt + clippy + cyclomatic + test)
check: fmt-check clippy cyclomatic test

# Time the tree-wide computations on a tree and on one eight times larger, in
# release mode, and fail on any that grows markedly faster than linear (see
# docs/development.md). Kept out of `check`: timing wants an optimised build
# and a quiet machine. --no-capture runs the tests one at a time and shows
# each growth.
scaling:
    cargo nextest run --release -p oxidgene-api --test algorithm_scaling_test --run-ignored only --no-capture

# Import your own exports end to end through the desktop's backend, in release
# mode, and check the trees they produce (see docs/development.md). Opt-in: the
# OXIDGENE_REAL_* variables name the files, and a test whose files are unset
# skips itself. Everything is staged under target/real-import — on disk, not
# in a RAM-backed /tmp — and deleted afterwards. Arguments go to nextest.
real-import *args:
    #!/usr/bin/env bash
    set -euo pipefail
    work="$PWD/target/real-import"
    trap 'rm -rf "$work"' EXIT
    mkdir -p "$work/tmp"
    TMPDIR="$work/tmp" OXIDGENE_REAL_WORKDIR="$work" \
      cargo nextest run --release -p oxidgene-api --test real_import_test \
        --run-ignored only --no-capture {{ args }}

# Every `#[ignore]`d test names the recipe below that runs it; the
# `lint_discipline` guard fails on a reason naming no existing recipe.

# Run the opt-in benchmarks of the profile service and the person search, in
# release mode, one at a time. They print timings and assert nothing a shared
# machine could fail by being slow.
bench:
    cargo nextest run --release -p oxidgene-api -p oxidgene-db --test profile_service_test --test person_search_test --run-ignored only --no-capture

# Migrate an empty, disposable PostgreSQL database up and down; the
# OXIDGENE_TEST_DATABASE_URL variable names it (see docs/development.md).
test-postgres:
    cargo nextest run -p oxidgene-db --features postgres --test migration_test --run-ignored only

# Round-trip media through the RustFS service of docker/docker-compose.yml,
# which must be running (`docker compose -f docker/docker-compose.yml up -d`).
test-s3:
    cargo nextest run -p oxidgene-api --features s3 --lib --run-ignored only -E 'test(s3_round_trip_deduplication_and_tree_deletion)'

# Stream a private Geneanet session archive, named by
# OXIDGENE_GENEANET_SESSION, through REST without logging its contents.
session-check:
    cargo nextest run -p oxidgene-api --features graphql --test geneanet_session_test --run-ignored only

# Run the Geneanet content-matching harnesses on your own archives, in
# release mode (see docs/development.md, *Geneanet content matching*). Each
# skips itself when the variables naming its files are unset. Arguments go to
# nextest: `just geneanet-harness phash_cost`.
geneanet-harness *args:
    cargo nextest run --release -p oxidgene-geneanet --run-ignored only --no-capture {{ args }}

# Write an HTML preview of every theme's pedigree card to OXIDGENE_PREVIEW_DIR
# (default: the current directory).
theme-preview:
    cargo nextest run -p oxidgene-ui --lib --run-ignored only --no-capture -E 'test(theme_preview)'

# Regenerate the place dictionary (France, United Kingdom, Germany, Italy,
# Spain, Switzerland, Poland, United States, Portugal, Belgium, Luxembourg,
# Netherlands) from the latest
# open data into assets/places/places.csv.br, which is committed and embedded
# into the binaries (see docs/place-dictionary.md). Run by hand when a source
# publishes a new edition. --cached reuses the previous run's downloads and
# --csv FILE also writes the uncompressed CSV.
places *args:
    cargo run --package oxidgene-place-dictionary -- {{args}}

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
    OXIDGENE_LOG_LEVEL="{{ log_level }}" OTEL_EXPORTER_OTLP_ENDPOINT="http://127.0.0.1:4317" cargo run --package oxidgene-desktop

# Run the desktop app with direct OTLP/gRPC export to OpenObserve
desktop-openobserve log_level="info":
    @test -n "${OPENOBSERVE_BASIC_TOKEN:-}" || { echo "OPENOBSERVE_BASIC_TOKEN is required" >&2; exit 1; }
    OXIDGENE_LOG_LEVEL="{{ log_level }}" OTEL_EXPORTER_OTLP_ENDPOINT="${OPENOBSERVE_OTLP_ENDPOINT:-http://127.0.0.1:5081}" OTEL_EXPORTER_OTLP_HEADERS="authorization=Basic%20${OPENOBSERVE_BASIC_TOKEN},organization=${OPENOBSERVE_ORGANIZATION:-default},stream-name=${OPENOBSERVE_STREAM:-oxidgene}" cargo run --package oxidgene-desktop

# Build an optimized desktop release with runtime-optional OTLP telemetry
build-desktop-release:
    cargo build --release --package oxidgene-desktop

# Build an optimized desktop release for Windows X64 with runtime-optional OTLP telemetry
build-desktop-win64-release:
    cargo xwin build --target x86_64-pc-windows-msvc --release --package oxidgene-desktop

# Generate documentation
doc:
    cargo doc --workspace --no-deps --open
