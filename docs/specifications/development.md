---
type: "Development Specification"
title: "Development Environment and Workflows"
description: "Local development prerequisites and just command reference for OxidGene."
tags: [oxidgene, specification, development, rust, just]
timestamp: 2026-08-26T00:00:00Z
---

# Development Environment and Workflows

> Part of the [OxidGene Specifications](index.md).
> See also: [Architecture](architecture.md) · [Cross-cutting Rules](cross-cutting.md)

---

## 1. Prerequisites

- [Rust](https://rustup.rs/) stable toolchain.
- [just](https://github.com/casey/just) task runner.
- PostgreSQL 16+ or Docker Compose for the web backend. The Compose stack also
   provides RustFS for S3-compatible media storage and Redis for future user
   sessions.
- The `wasm32-unknown-unknown` Rust target for the browser application.
- [Dioxus CLI](https://dioxuslabs.com/learn/0.7/getting_started/) 0.7.10.
- `cargo-nextest` for the workspace test recipes.
- `cargo-watch` for backend hot reload; optional unless using `just dev-web-watch`.

```bash
rustup target add wasm32-unknown-unknown
cargo install cargo-nextest --locked
cargo install dioxus-cli --version 0.7.10 --locked
cargo install cargo-watch --locked
```

The complete installation and deployment paths are in the
[Quickstart](quickstart.md).

## 2. Command Reference

Run `just` without arguments to list available recipes. All commands run from
the repository root.

### 2.1 Build and Quality

| Command | Purpose |
|---------|---------|
| `just build` | Build all workspace crates in debug mode. |
| `just build-release` | Build all workspace crates in release mode. |
| `just test` | Run the workspace test suite with `cargo-nextest`. |
| `just test-verbose` | Run tests while preserving test output. |
| `just fmt` | Format all Rust source files. |
| `just fmt-check` | Check Rust formatting without changing files. |
| `just clippy` | Run Clippy for all workspace targets and deny warnings. |
| `just check` | Run formatting verification, Clippy, and tests. |
| `just clean` | Remove Cargo build artifacts. |
| `just doc` | Generate and open workspace API documentation. |

Run `just check` before committing code changes.

### 2.2 Backend and Database

| Command | Purpose |
|---------|---------|
| `just server` | Run the Axum development server on `http://127.0.0.1:8080`. |
| `just dev-db-up` | Start the PostgreSQL development container and wait until it is ready. |
| `just dev-db-down` | Stop the PostgreSQL development container without deleting its data. |

### 2.3 Browser Application

| Command | Purpose |
|---------|---------|
| `just web-check` | Check the browser application for the `wasm32-unknown-unknown` target. |
| `just web` | Run the browser application on `http://127.0.0.1:8081` against the local API by default. |
| `just web-build` | Build the production browser bundle. |
| `just dev-web` | Run the API and browser application together; the browser application hot reloads. |
| `just dev-web-watch` | Run the API and browser application with hot reload for both processes. |

`just web` uses `OXIDGENE_API_URL` when set; otherwise it connects to
`http://127.0.0.1:8080`. Repository commands invoke Dioxus through
`scripts/dx.sh`. The Dioxus rustc wrapper serializes its build environment under
`target/dx/.captured-args`, so the launcher removes credential-shaped environment
variables before starting `dx`. Direct `dx serve` and `dx build` invocations are
not supported because they can persist shell credentials in those local build
artifacts.

### 2.4 Desktop Application

| Command | Purpose |
|---------|---------|
| `just desktop` | Run the desktop application in development mode. |
| `just build-desktop-release` | Build the desktop application in release mode. |

## 3. Local Web Workflow

1. Start the database with `just dev-db-up`.
2. Run `just dev-web` for frontend hot reload, or `just dev-web-watch` to also
   restart the backend when its Rust sources change.
3. Open `http://127.0.0.1:8081` in a browser.
4. Stop the database with `just dev-db-down` when it is no longer needed.

For the complete containerized web stack, run:

```bash
docker compose -f docker/docker-compose.yml up -d --wait --remove-orphans
```

Compose builds and exposes the browser frontend on `http://127.0.0.1:8081` and
explicitly selects the S3 media backend. The server and frontend containers
have no persistent volumes: PostgreSQL and RustFS own durable state, while
import upload spooling uses disposable container storage. Redis is running and
persistent in the development stack but remains unused until authentication
and session management are implemented.

RustFS exposes its S3 API on `http://127.0.0.1:9000` and its console on
`http://127.0.0.1:9001`. The one-shot `rustfs-init` service creates the
`oxidgene-media` bucket idempotently. Run the ignored storage round-trip test
while the stack is healthy with:

```bash
cargo test --package oxidgene-api --features s3 \
   s3_round_trip_deduplication_and_tree_deletion -- --ignored
```

Kubernetes deployment and both supported S3 modes are documented in the
[OxidGene Helm chart](../../charts/oxidgene/README.md).

## 4. Responsive Visual Validation

Changes to shared layout, responsive CSS, modals, cards, or dense controls
require browser validation at a representative desktop width and at a narrow
mobile viewport such as `390x844`. Also test the exact breakpoint boundaries
affected by the change, including both sides of a threshold.

For each viewport:

- inspect the bounding rectangles of the page's principal children, not only
   `body.scrollWidth`; a child can overflow a clipped container without changing
   the document width;
- verify that cards, grid tracks, toolbars, modals, and fixed-format controls
   remain within their containing block and do not overlap;
- verify that long names, places, labels, and translated strings wrap without
   hiding adjacent content;
- verify that every action remains visible or otherwise directly reachable,
   and that icon-only actions keep their accessible name and tooltip;
- exercise loading, validation, progress, expanded, and collapsed states to
   detect layout movement that is absent in the initial screenshot; and
- recheck the desktop layout after mobile changes so compact overrides do not
   leak into wider viewports.

Use anonymized content in screenshots and measurements. A responsive change is
not complete when only the outer page fits; its significant descendants must
fit and remain usable as well.

## 5. Release Automation

Pushing a tag that matches `v<workspace-version>` starts
`.github/workflows/release.yml`. The workflow rejects a tag whose version does
not match both the Cargo workspace version and the Helm chart `appVersion`.

A successful run publishes:

- `oxidgene-server` and `oxidgene-web` multi-architecture images to GitHub
   Container Registry with immutable version tags and a moving `latest` tag;
- the OxidGene chart as an OCI artifact under `ghcr.io/trois-six/charts`;
- native desktop archives for Linux, macOS, and Windows; and
- a GitHub Release containing the desktop archives, packaged chart, generated
   release notes, and `SHA256SUMS`.

The release is created only after every platform build and publication job has
succeeded. Desktop artifacts are currently unsigned portable executables, not
platform installers.