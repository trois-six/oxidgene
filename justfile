# OxidGene - Justfile
# Build orchestration for the OxidGene genealogy platform.
#
# Every check a GitHub workflow runs is a recipe of this file, called as
# `just <recipe>`, so that CI and a developer's machine run the same command
# (docs/development.md §2.9; the `workflows` guard fails on a workflow step
# that calls cargo, npm, uv or a script of scripts/ itself). The checking
# recipes pass `--locked`: a lockfile behind its manifests fails here as it
# does in CI. What differs in CI is set by its environment, never by another
# command: `CI`, which GitHub sets, makes Playwright install the system
# libraries of its browsers, and `CARGO_PROFILE_DEV_DEBUG=0` drops the debug
# information.

# Default recipe: show available commands
default:
    @just --list

# `just setup hooks` also installs the pre-push hook (`just hooks`).
# Install the project development tools and Rust targets
setup *options:
    #!/usr/bin/env bash
    set -euo pipefail
    command -v mise >/dev/null 2>&1 || { echo "mise is required: https://mise.jdx.dev/getting-started.html" >&2; exit 1; }
    command -v rustup >/dev/null 2>&1 || { echo "rustup is required: https://rustup.rs" >&2; exit 1; }
    mise install
    rustup target add wasm32-unknown-unknown
    rustup target add x86_64-pc-windows-msvc
    case " {{ options }} " in
        *" hooks "*) "{{ just_executable() }}" hooks ;;
        *) echo "Optional: \`just setup hooks\` installs a pre-push hook that runs the CI gate before every push (docs/development.md §2.9)." ;;
    esac

# Opt-in. The hook runs scripts/pre-push.sh of the checkout being pushed:
# `just check` when the push changes code, `just guards` when it changes
# only documentation, Dockerfiles, Compose files or workflows. It is the
# clone's, shared by its worktrees; a hook that is not this one is left
# untouched. `git push --no-verify` skips it, `just hooks remove` removes it.
# Install the pre-push hook that runs the CI gate before every push
hooks action="install":
    #!/usr/bin/env bash
    set -euo pipefail
    hook="$(git rev-parse --git-path hooks/pre-push)"
    marker="# OxidGene pre-push hook"
    if [ -e "$hook" ] && ! grep -qF "$marker" "$hook"; then
        echo "$hook exists and is not OxidGene's: left untouched" >&2
        exit 1
    fi
    case "{{ action }}" in
        install)
            mkdir -p "$(dirname "$hook")"
            {
                echo '#!/bin/sh'
                echo "$marker, installed by \`just hooks\`: runs the checkout's scripts/pre-push.sh."
                echo 'script="$(git rev-parse --show-toplevel)/scripts/pre-push.sh"'
                echo 'if [ -x "$script" ]; then exec "$script" "$@"; fi'
            } > "$hook"
            chmod +x "$hook"
            echo "installed $hook"
            ;;
        remove)
            rm -f "$hook"
            echo "removed $hook"
            ;;
        *)
            echo "usage: just hooks [install|remove]" >&2
            exit 2
            ;;
    esac

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

# The browsers `just e2e` installs for Playwright: a run of another project
# names its own (`E2E_BROWSERS=webkit just e2e --project=webkit`).
e2e_browsers := env("E2E_BROWSERS", "chromium")

# Python is `python` on Windows, where the release smoke-tests the desktop.
python := if os_family() == "windows" { "python" } else { "python3" }

# Performance and browser tests have their own recipes: `just scaling`,
# `just e2e`, `just ui-js`. All Rust recipes need cargo-nextest (`just setup`).
# Run the unit and functional tests
test: test-unit test-functional

# Covers the `#[cfg(test)]` modules of every library and binary, then the
# documentation examples, which nextest does not run.
# Run the unit tests
test-unit:
    cargo nextest run --locked --workspace --lib --bins --no-fail-fast
    cargo test --locked --workspace --doc

# The integration test targets under `crates/*/tests` and `apps/*/tests`:
# REST, GraphQL, repository, import and MCP scenarios.
# Run the functional tests
test-functional:
    cargo nextest run --locked --workspace --test '*' --no-fail-fast

# The tests of `oxidgene-guards` alone (docs/development.md §2.8): they read
# the sources, the specifications, the justfile and the workflows, and build
# in seconds. `just test` runs them with the rest; CI gives them a job of
# their own, which a documentation-only change runs too. With
# OXIDGENE_SPEC_BASE naming a git revision, an edited specification must
# carry a new `generated.at`.
# Run the repository guards
guards:
    cargo nextest run --locked -p oxidgene-guards --no-fail-fast

# Run the browser JavaScript unit tests (Node.js, no dependencies)
ui-js:
    node --test crates/oxidgene-ui/tests/*.test.mjs apps/oxidgene-desktop/tests/*.test.mjs

# Builds the web bundle for the e2e API port and the server, then runs the
# Playwright suite in `e2e/` against both on a throwaway database (see
# docs/development.md). Arguments go to Playwright: `just e2e --headed`,
# `just e2e tests/home.spec.ts`, `just e2e --project=chromium`. On a CI
# runner, which lacks them, Playwright installs the browser's system
# libraries too.
# Run the browser end-to-end tests
e2e *args:
    OXIDGENE_API_URL="http://127.0.0.1:{{ e2e_api_port }}" scripts/dx.sh build --package oxidgene-web --platform web
    cargo build --locked --package oxidgene-server
    cd e2e && npm ci --no-audit --no-fund && npx playwright install ${CI:+--with-deps} {{ e2e_browsers }}
    cd e2e && E2E_API_PORT="{{ e2e_api_port }}" E2E_WEB_PORT="{{ e2e_web_port }}" npx playwright test {{ args }}

# The suite against images already built, as a release runs it
# (docs/development.md §6): OXIDGENE_SERVER_IMAGE and OXIDGENE_WEB_IMAGE name
# them, docker/docker-compose.e2e.yml starts them behind one origin on a
# throwaway PostgreSQL, and Playwright drives that origin (`E2E_EXTERNAL`).
# The containers' logs land in e2e/test-results/containers.log and the
# stack is removed, whatever the outcome. Arguments go to Playwright.
# Run the end-to-end tests against the server and web images
e2e-images *args:
    #!/usr/bin/env bash
    set -euo pipefail
    compose=(docker compose -f docker/docker-compose.e2e.yml)
    export E2E_WEB_PORT="{{ e2e_web_port }}"
    finish() {
        mkdir -p e2e/test-results
        "${compose[@]}" logs --no-color --timestamps > e2e/test-results/containers.log 2>&1 || true
        "${compose[@]}" down --volumes || true
    }
    trap finish EXIT
    "${compose[@]}" up -d --wait --wait-timeout 300
    curl --fail --silent --show-error --retry 30 --retry-delay 2 --retry-all-errors \
      --output /dev/null "http://127.0.0.1:${E2E_WEB_PORT}/"
    curl --fail --silent --show-error --output /dev/null "http://127.0.0.1:${E2E_WEB_PORT}/healthz"
    (cd e2e && npm ci --no-audit --no-fund && npx playwright install ${CI:+--with-deps} chromium)
    # One origin: the ingress serves the bundle and the API.
    (cd e2e && E2E_EXTERNAL=1 E2E_API_PORT="$E2E_WEB_PORT" npx playwright test {{ args }})

# The sign-in journeys of e2e/auth/ (docs/authentication.md §12): builds and
# starts the development Compose stack, Rauthy and its fictitious users
# included, then drives it through the same-origin web container on
# http://localhost:8081. Opt-in, like every suite needing outside services:
# never part of `check`. The stack keeps running afterwards (`docker compose
# -f docker/docker-compose.yml down` stops it). Every test skips itself while
# the server answers `/auth/session` without authentication. Arguments go to
# Playwright.
# Run the end-to-end sign-in tests against the Compose stack
e2e-auth *args:
    docker compose -f docker/docker-compose.yml up -d --build --wait --remove-orphans
    cd e2e && npm ci --no-audit --no-fund && npx playwright install chromium
    cd e2e && npx playwright test --config playwright.auth.config.ts {{ args }}

# Takes every page of the fictitious Landrevel family (e2e/screenshots/) on
# the e2e suite's throwaway servers, then encodes the captures into
# assets/screenshots/ with the README carousel (scripts/screenshots.py). The
# data, clock, viewport, language and themes are fixed, so a run reproduces
# the committed images (see docs/development.md).
# Regenerate the README and feature screenshots
screenshots:
    OXIDGENE_API_URL="http://127.0.0.1:{{ e2e_api_port }}" scripts/dx.sh build --package oxidgene-web --platform web
    cargo build --locked --package oxidgene-server
    cd e2e && npm ci --no-audit --no-fund && npx playwright install chromium
    rm -rf target/screenshots/raw
    cd e2e && E2E_API_PORT="{{ e2e_api_port }}" E2E_WEB_PORT="{{ e2e_web_port }}" npx playwright test --config playwright.screenshots.config.ts
    uv run --quiet scripts/screenshots.py

# Run tests with output
test-verbose:
    cargo nextest run --workspace --no-capture

# Each build variant has its recipe, which the CI Clippy matrix runs as a
# job of its own; `just wasm` is the fifth.
# Run Clippy on every native build variant
clippy: clippy-workspace clippy-archives clippy-api clippy-desktop-minimal

# Run Clippy on every native target with every default feature
clippy-workspace:
    cargo clippy --locked --workspace --all-targets -- -D warnings

# Run Clippy on the archive crate's live checks, a feature no workspace member enables
clippy-archives:
    cargo clippy --locked -p oxidgene-archives --features native,live --all-targets -- -D warnings

# The workspace run unifies the server's `graphql` and `auth` features into
# the API, so code only those features use looks used there and dead here.
# Run Clippy on the API alone, as the desktop links it
clippy-api:
    cargo clippy --locked -p oxidgene-api --all-targets -- -D warnings

# Run Clippy on the desktop without telemetry, the build whose `cfg`s differ most
clippy-desktop-minimal:
    cargo clippy --locked -p oxidgene-desktop --no-default-features --all-targets -- -D warnings

# Check that the shared UI still compiles to WASM.
#
# `clippy --workspace` only ever builds the host target, so a dependency that
# exists on desktop but not on the web — tokio, say — breaks the web build
# without any native check noticing; and the browser binary declares its
# dependencies for wasm32 alone, so natively it compiles to an empty shell.
# `oxidgene-ui` is platform-independent by contract (see AGENTS.md); run this
# after touching the UI or its dependencies. `just check` and the CI Clippy
# matrix run it.
wasm:
    cargo clippy --locked -p oxidgene-web --target wasm32-unknown-unknown --all-targets -- -D warnings

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

# Build the rustdoc the documentation site publishes, warnings denied, so a
# broken or private intra-doc link fails here rather than in the Pages job
# (scripts/site.py documents the same crates).
doc-check:
    RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps -p oxidgene-core -p oxidgene-db -p oxidgene-gedcom -p oxidgene-geneanet -p oxidgene-archives -p oxidgene-observability -p oxidgene-api -p oxidgene-ui

# The gate: every check the `CI` status of a pull request or of a push to
# `main` gathers (.github/workflows/ci.yml, a job per recipe), in one
# command. The `workflows` guard fails when the two differ. The quick checks
# come first; `sql-plans` follows `test`, whose build it reuses; `deps` reads
# the advisory databases, so it needs the network.
# Run every check of the CI gate
check: fmt-check cyclomatic ui-js duplication deps clippy wasm doc-check test sql-plans

# Time the tree-wide computations on a tree and on one eight times larger, in
# release mode, and fail on any that grows markedly faster than linear (see
# docs/development.md). Kept out of `check`: timing wants an optimised build
# and a quiet machine. --no-capture runs the tests one at a time and shows
# each growth.
scaling:
    cargo nextest run --locked --release -p oxidgene-api --test integration --run-ignored only --no-capture -E 'test(/^algorithm_scaling_test::/)'

# Time a pedigree view's portraits cut from generated camera-sized photographs
# (cold) and read back from their stored crops (warm), in release mode, and
# report the crops' sizes (see docs/architecture.md §4.2). The fixture is
# staged under target/, on disk rather than in a RAM-backed /tmp.
crop-bench:
    cargo nextest run --release -p oxidgene-api --test integration --run-ignored only --no-capture -E 'test(a_pedigree_view_reads_stored_crops_instead_of_cutting_them)'

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
      cargo nextest run --release -p oxidgene-api --test integration \
        --run-ignored only --no-capture -E 'test(/^real_import_test::/)' {{ args }}

# Measure the memory of a Geneanet import of a generated, fictitious tree, in
# release mode, and fail over its budget (see docs/development.md). The
# fixture and the job's files are staged under target/, on disk rather than
# in a RAM-backed /tmp.
import-memory:
    #!/usr/bin/env bash
    set -euo pipefail
    work="$PWD/target/import-memory"
    trap 'rm -rf "$work"' EXIT
    mkdir -p "$work"
    TMPDIR="$work" cargo nextest run --locked --release -p oxidgene-api --test integration \
      --run-ignored only --no-capture -E 'test(/^import_memory_test::/)'

# Fail when the share of duplicated Rust code grows past its budget
# (scripts/duplication.py); `just duplication --report` lists the blocks.
duplication *args:
    uv run --quiet scripts/duplication.py {{ args }}

# Check the dependency graph: no unused dependency (cargo-machete), no
# advisory, licence, ban or source deny.toml refuses (cargo-deny), no known
# vulnerability (cargo-audit), and no more duplicated crates than the budget
# of scripts/budgets.json allows.
deps:
    cargo machete
    cargo deny check
    cargo audit
    python3 scripts/budgets.py dependencies

# Build what ships — the desktop, the server, the worker and the web bundle —
# in release mode, and compare each one's size and the crates it links with
# scripts/budgets.json (docs/development.md §2.8). `just budgets --update`
# records a growth that was intended. Python comes from uv, as for the
# other scripts: the CI image has no other.
# Check the size of the shipped binaries against their budgets
budgets *args:
    cargo build --locked --release -p oxidgene-desktop -p oxidgene-server -p oxidgene-worker
    scripts/dx.sh build --package oxidgene-web --platform web --release
    uv run --quiet --no-project scripts/budgets.py binaries {{ args }}

# Dependencies declared and never used, which only a nightly compiler can
# tell: needs `rustup toolchain install nightly --profile minimal` and
# cargo-udeps (`just setup`).
# Look for unused dependencies with cargo-udeps
udeps:
    cargo +nightly udeps --locked --workspace --all-targets

# Run Clippy on a Rust release ahead of the pinned one (`beta`, `nightly`),
# to see its new lints coming; the toolchain must be installed with clippy.
next-toolchain toolchain:
    cargo +{{ toolchain }} clippy --locked --workspace --all-targets -- -D warnings

# Report what the next Rust release will refuse in the dependencies
future-incompat toolchain="beta":
    cargo +{{ toolchain }} check --locked --workspace --all-targets --future-incompat-report

# Report the updates waiting in the lockfile and the major releases our requirements hold back
report-updates:
    cargo update --dry-run --verbose

# Count the code with scc; arguments go to it (`--by-file --format csv`)
report-size *args:
    scc --exclude-dir target,node_modules {{ args }} crates apps e2e scripts

# The test is selected in the workspace's test build, the one `just
# test-functional` leaves, whose unified features include GraphQL and
# authentication, so that it compiles nothing more.
# Read the query plan of every statement the API runs over a populated tree
# and fail on a full scan of a large table (docs/development.md, guards).
sql-plans:
    cargo nextest run --locked --workspace --test integration --run-ignored only -E 'test(no_statement_scans_a_large_table)'

# The live end-to-end checks of the archive portals (docs/archives.md §9.1):
# every catalogued archive with an adapter and live checks, or those named
# (`just archives-live fr-ad44`). They contact the real portals, sequentially
# and with the identifying User-Agent, archives sharing a portal address
# paused apart, so they run on demand and weekly in
# .github/workflows/archives.yml, never in `check`. Reports land in
# target/archives-live/, in a directory per archive for several.
# Check the archive portals against their adapters
archives-live *archives:
    scripts/archives-live.sh {{ archives }}

# Every `#[ignore]`d test names the recipe below that runs it; the
# `lint_discipline` guard fails on a reason naming no existing recipe.

# Run the opt-in benchmarks of the profile service, the person search, the
# person visibility and the access scopes, in release mode, one at a time. They print timings and assert nothing a shared
# machine could fail by being slow.
bench:
    cargo nextest run --release -p oxidgene-api -p oxidgene-db --features oxidgene-api/auth,oxidgene-api/graphql --test integration --run-ignored only --no-capture -E 'test(/^(profile_service_test|person_search_test|visibility_test|sharing_test|redaction_cache_test)::/)'

# Migrate an empty, disposable PostgreSQL database up and down, run the
# repository and API functional tests on it, each test in a schema of its
# own, then follow jobs across processes on it (cancellation, progress
# notifications, and a job queued elsewhere waking an idle worker) and a
# notification written elsewhere waking a long poll, one at a time, since
# they share the database; the
# OXIDGENE_TEST_DATABASE_URL variable names it (see docs/development.md).
test-postgres:
    cargo nextest run --locked -p oxidgene-db --features postgres --test integration --run-ignored only -E 'test(/^migration_test::/)'
    OXIDGENE_TEST_POSTGRES_URL="${OXIDGENE_TEST_DATABASE_URL:?set OXIDGENE_TEST_DATABASE_URL}" cargo nextest run --locked -p oxidgene-db -p oxidgene-api --features oxidgene-api/postgres,oxidgene-api/graphql,oxidgene-api/auth --test integration --no-fail-fast
    cargo nextest run --locked -p oxidgene-api --features postgres,auth --lib --run-ignored only --test-threads 1 -E 'test(/on_postgres_/)'

# Round-trip media and crops through the RustFS service of docker/docker-compose.yml,
# then run the stored crops' lifecycle on it. The recipe starts the service
# and creates its bucket, and leaves it running
# (`docker compose -f docker/docker-compose.yml stop rustfs` stops it).
test-s3:
    docker compose -f docker/docker-compose.yml run --rm rustfs-init
    cargo nextest run --locked -p oxidgene-api --features s3 --lib --run-ignored only -E 'test(s3_round_trip_deduplication_and_tree_deletion)'
    cargo nextest run --locked -p oxidgene-api --features s3 --test integration --run-ignored only -E 'test(crops_live_and_die_on_the_s3_store_as_on_disk)'

# Keep, index, expire and end sessions in the Redis server named by
# OXIDGENE_TEST_REDIS_URL, such as the Compose stack's
# (`docker compose -f docker/docker-compose.yml up -d redis`, then
# `OXIDGENE_TEST_REDIS_URL=redis://127.0.0.1:6379`).
test-redis:
    cargo nextest run --locked -p oxidgene-api --features auth --lib --run-ignored only -E 'test(/on_redis_/)'

# `just test` builds the whole workspace, which unifies the server's
# `graphql`, `auth`, `postgres` and `s3` features into the API: the API the
# desktop links — `mcp` and the telemetry context, nothing else — is linted
# by `just clippy-api` and tested only here.
# Run the API's tests in the desktop's feature set
test-desktop-api:
    cargo nextest run --locked -p oxidgene-api --features mcp,telemetry-context --no-fail-fast

# libFuzzer on synthetic inputs, for SECONDS; needs the nightly toolchain and
# cargo-fuzz (`just setup`). The weekly workflow runs it: a crash is fixed
# with a minimal anonymized regression test (docs/development.md §5.7).
# Fuzz the Geneanet session decoder for a time budget
fuzz seconds="60":
    cargo +nightly fuzz run --fuzz-dir crates/oxidgene-geneanet/fuzz session_decode -- -max_total_time={{ seconds }}

# PLACEHOLDER, awaiting the opt-in tests on generated trees of 100,000 and
# 1,000,000 persons: their command replaces this body, and the weekly
# workflow's Large trees job runs it as it is.
# Run the large-tree scale tests (not written yet)
scale-large:
    @echo "just scale-large: the 100,000- and 1,000,000-person scale tests are not wired yet; nothing ran"

# Refuse a bulk target past 10,000 persons end to end on both surfaces. It
# creates 10,001 persons, so it runs only on request.
tag-bounds:
    cargo nextest run -p oxidgene-api --features graphql --test integration --run-ignored only -E 'test(a_target_past_ten_thousand_persons_is_refused)'

# Stream a private Geneanet session archive, named by
# OXIDGENE_GENEANET_SESSION, through REST without logging its contents.
session-check:
    cargo nextest run -p oxidgene-api --features graphql --test integration --run-ignored only -E 'test(/^geneanet_session_test::/)'

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

# Vendor pdfjs-dist VERSION, the PDF viewer's library, into
# assets/public/pdfjs/, after checking the archive against INTEGRITY: the
# `sha512-…` the npm registry lists for it (`npm view pdfjs-dist@VERSION
# dist.integrity`). Run by hand for each release taken, security releases
# first (see docs/development.md); commit the result.
# Update the vendored pdf.js
pdfjs version integrity:
    scripts/pdfjs.sh "{{ version }}" "{{ integrity }}"

# Rewrite docs/schema.graphql, the committed SDL of the GraphQL schema, after
# an intended schema change (a guard fails while the two differ).
graphql-schema:
    OXIDGENE_BLESS=1 cargo nextest run -p oxidgene-api --features graphql,auth --test integration -E 'test(the_schema_matches_its_snapshot)'

# Regenerate the OpenAPI specification from the REST router
openapi:
    cargo build --package oxidgene-api

# Clean build artifacts
clean:
    cargo clean

# Run the web server (dev mode)
server:
    cargo run --package oxidgene-server

# The Compose stack run from the checkout (docker/docker-compose.dev.yml):
# the server, the worker and the browser frontend rebuild themselves when a
# source changes, on http://localhost:8081. The first start builds the
# development image and every crate.
# Run the Compose stack with backend and frontend rebuilt on every change
dev-stack:
    docker compose -f docker/docker-compose.yml -f docker/docker-compose.dev.yml up -d --build --wait --remove-orphans

# Stop the development Compose stack, keeping its data and build volumes
dev-stack-down:
    docker compose -f docker/docker-compose.yml -f docker/docker-compose.dev.yml down

# Check that the Compose stack's configuration is valid
compose-check:
    docker compose -f docker/docker-compose.yml config --quiet

# Start the PostgreSQL development database
dev-db-up:
    docker compose -f docker/docker-compose.yml up -d --wait postgres

# Stop the PostgreSQL development database without deleting its data
dev-db-down:
    docker compose -f docker/docker-compose.yml stop postgres

# Lints the chart with each values file of charts/oxidgene/ci and renders
# every authentication mode, asserting on what it produces and refuses and
# on the Rauthy setup it ships against the development one (scripts/chart-tests.py,
# docs/authentication.md §12). Needs Helm; fetches the chart's dependencies.
# Lint and test the Helm chart
chart-test:
    helm dependency build charts/oxidgene
    uv run --quiet scripts/chart-tests.py

# The Compose stack's Rauthy alone — with its mail relay and the step
# completing its web client — on http://localhost:8180, for
# `OXIDGENE_AUTH_MODE=oidc just dev-web` (docs/authentication.md §8.5)
# Start the development identity provider
dev-idp-up:
    docker compose -f docker/docker-compose.yml run --rm rauthy-init

# Stop and remove the development identity provider: the next start bootstraps it afresh
dev-idp-down:
    docker compose -f docker/docker-compose.yml rm --stop --force rauthy-init rauthy mailpit

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
    cargo build --locked --release --package oxidgene-desktop

# Build an optimized desktop release for Windows X64 with runtime-optional OTLP telemetry
build-desktop-win64-release:
    cargo xwin build --target x86_64-pc-windows-msvc --release --package oxidgene-desktop

# Build the universal macOS application and its disk image (on macOS), as the release does
build-desktop-macos-app dmg="target/oxidgene-desktop-macos-universal.dmg":
    rustup target add aarch64-apple-darwin x86_64-apple-darwin
    MACOSX_DEPLOYMENT_TARGET=11.0 cargo build --locked --release --package oxidgene-desktop --target aarch64-apple-darwin --target x86_64-apple-darwin
    scripts/macos-app.sh "{{ dmg }}" target/aarch64-apple-darwin/release/oxidgene-desktop target/x86_64-apple-darwin/release/oxidgene-desktop

# Each download as packaged — the Linux archive, the macOS disk image, the
# Windows archive — without opening a window: its version, and an MCP
# session over stdio answering `list_trees` (docs/development.md §6).
# Smoke-test packaged desktop downloads
desktop-smoke *downloads:
    {{ python }} scripts/desktop-smoke.py {{ downloads }}

# The release workflow's first step, and a check to run before pushing a
# tag: TAG is `v` followed by the version of the workspace's packages and
# of the chart's `appVersion`. Prints that version.
# Check a release tag against the workspace and chart versions
release-version tag:
    #!/usr/bin/env bash
    set -euo pipefail
    tag="{{ tag }}"
    version="${tag#v}"
    cargo_version="$(cargo metadata --locked --no-deps --format-version 1 | jq -r '.packages[] | select(.name == "oxidgene-desktop") | .version')"
    chart_version="$(sed -n 's/^appVersion: "\([^"]*\)"$/\1/p' charts/oxidgene/Chart.yaml)"
    if [[ "$tag" == "$version" || -z "$version" ]]; then
        echo "release tags must start with v" >&2
        exit 1
    fi
    if [[ "$version" != "$cargo_version" ]]; then
        echo "tag version $version does not match Cargo version $cargo_version" >&2
        exit 1
    fi
    if [[ "$version" != "$chart_version" ]]; then
        echo "tag version $version does not match chart appVersion $chart_version" >&2
        exit 1
    fi
    echo "$version"

# Generate documentation
doc:
    cargo doc --workspace --no-deps --open

# Build the documentation site into site/public: the user guide, the
# specifications, the REST and GraphQL references and the rustdoc
# (scripts/site.py, docs/development.md §7)
site:
    uv run --quiet scripts/site.py build

# Preview the documentation site with live reload
site-serve:
    uv run --quiet scripts/site.py serve
