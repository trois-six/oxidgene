---
type: "Development Specification"
title: "Development Environment and Workflows"
description: "Local development, secure coding practices, verification workflows, and just command reference for OxidGene."
tags: [oxidgene, specification, development, rust, security, just]
timestamp: 2026-08-26T00:00:00Z
---

# Development Environment and Workflows

> Part of the [OxidGene Specifications](index.md).
> See also: [Architecture](architecture.md) · [Cross-cutting Rules](cross-cutting.md)

---

## 1. Prerequisites

- [Rust](https://rustup.rs/) stable toolchain.
- [just](https://github.com/casey/just) task runner.
- [mise](https://mise.jdx.dev/) tool version manager.
- PostgreSQL 16+ or Docker Compose for the web backend. The Compose stack also
   provides RustFS for S3-compatible media storage and Redis for future user
   sessions.
- The `wasm32-unknown-unknown` Rust target for the browser application.
- [Dioxus CLI](https://dioxuslabs.com/learn/0.7/getting_started/) 0.7.10.
- `cargo-nextest` for the workspace test recipes.
- `cargo-watch` for backend hot reload; optional unless using `just dev-web-watch`.

```bash
just setup
```

This installs the versions declared in `mise.toml` and adds the
`wasm32-unknown-unknown` target to the active Rust toolchain. Rust, rustup,
just, Docker, and Docker Compose remain host prerequisites.

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

`OXIDGENE_LOG_LEVEL` sets the browser log threshold at compile time. Supported
values are `trace`, `debug`, `info`, `warn`, and `error`; the default is
`info`. Because the deployed WASM bundle is static, changing a frontend pod's
environment does not change an already-built bundle.

`OTEL_EXPORTER_OTLP_ENDPOINT` configures browser tracing at compile time for
local and Compose builds. The published web image also reads
`globalThis.OXIDGENE_OTLP_ENDPOINT` from `/runtime-config.js` before starting
WASM; Helm writes this value from `frontend.otlpEndpoint`, so the same image can
target a different collector per installation. A non-empty value enables
OTLP/HTTP protobuf export to `/v1/traces` and W3C Trace Context injection on API
requests. The URL must be public to the browser, and the collector must allow
the frontend origin on its OTLP/HTTP receiver. An absent or empty value keeps
client spans and trace headers disabled.

### 2.4 Desktop Application

| Command | Purpose |
|---------|---------|
| `just desktop` | Run the desktop application in development mode. |
| `just desktop-telemetry [log_level]` | Start the local collector and run the desktop with OTLP enabled; the optional filter defaults to `info`. |
| `just desktop-openobserve [log_level]` | Run the desktop with direct OTLP/gRPC export to a local OpenObserve instance. |
| `just build-desktop-release` | Build an optimized desktop release retaining runtime-optional OTLP telemetry support. |

Set `OTEL_EXPORTER_OTLP_ENDPOINT` when running the desktop binary to export native
desktop logs, spans, and metrics over OTLP/gRPC. The export covers the embedded
API and worker as well as native UI `tracing` events; it is disabled when the
variable is absent. In that mode, log events still reach the console but span
callsites are disabled rather than creating spans that are later discarded.

`OXIDGENE_LOG_LEVEL` configures desktop logs. `--log-level FILTER` overrides
the environment for one invocation, and `--debug` selects
`info,oxidgene_ui=debug,oxidgene_api=debug,oxidgene_db=debug` only when neither
explicit setting is present.

For the common local collection workflow, use `just desktop-telemetry`. It
starts the Compose collector, waits for it, and points the desktop process to
`http://127.0.0.1:4317`. Pass an optional filter when needed, for example
`just desktop-telemetry debug` or
`just desktop-telemetry 'info,oxidgene_api=debug,sea_orm=warn'`.

For direct local OpenObserve export, set `OPENOBSERVE_BASIC_TOKEN` to the
Base64-encoded credentials and run `just desktop-openobserve`. The recipe uses
`http://127.0.0.1:5081`, organization `default`, and stream `oxidgene` unless
`OPENOBSERVE_OTLP_ENDPOINT`, `OPENOBSERVE_ORGANIZATION`, or
`OPENOBSERVE_STREAM` overrides them. Credentials must remain outside tracked
files. An OpenObserve error stating that a stream is being deleted means the
selected stream still has a deletion tombstone; choose another
`OPENOBSERVE_STREAM` or wait for that deletion to complete.

Production desktop releases built with `just build-desktop-release` retain the
OpenTelemetry dependency, HTTP trace layer, and tracing callsites. Export
remains disabled at runtime until a non-empty `OTEL_EXPORTER_OTLP_ENDPOINT` is
set, at which point logs, spans, and metrics are sent to that collector.

### 2.5 Observability configuration by process

Each native process reads only its own environment. It can therefore choose a
different filter and collector, or disable OTLP independently:

| Process | Log configuration | OTLP configuration |
|---|---|---|
| Server | `OXIDGENE_LOG_LEVEL` | `OTEL_EXPORTER_OTLP_ENDPOINT` |
| Worker | `OXIDGENE_LOG_LEVEL` | `OTEL_EXPORTER_OTLP_ENDPOINT` |
| Desktop | `OXIDGENE_LOG_LEVEL` or `--log-level` | `OTEL_EXPORTER_OTLP_ENDPOINT` |
| Browser WASM | Build-time `OXIDGENE_LOG_LEVEL` threshold | Runtime `frontend.otlpEndpoint`, falling back to build-time `OTEL_EXPORTER_OTLP_ENDPOINT`, over OTLP/HTTP |

Native log filters use `tracing_subscriber::EnvFilter` syntax. A simple level
such as `warn` applies globally; a directive list such as
`info,oxidgene_api=debug,sea_orm=warn` sets per-target levels. Invalid filters
fail process initialization. `RUST_LOG` is not read, so there is no hidden
second source of filter configuration.

Compose exposes the same separation through host-side substitution variables:
`OXIDGENE_SERVER_LOG_LEVEL`, `OXIDGENE_WORKER_LOG_LEVEL`,
`OXIDGENE_WEB_LOG_LEVEL`, `OXIDGENE_SERVER_OTLP_ENDPOINT`, and
`OXIDGENE_WORKER_OTLP_ENDPOINT`, and `OXIDGENE_WEB_OTLP_ENDPOINT`. Web settings
are build arguments because the resulting WASM bundle is static. Leaving a
Compose endpoint variable unset uses the bundled collector; setting it to an
empty string disables OTLP for that process or bundle.

An absent or empty `OTEL_EXPORTER_OTLP_ENDPOINT` disables OpenTelemetry export
and span callsites for that native process while retaining console log events.
For example, the server and worker may target separate collectors:

```bash
OXIDGENE_LOG_LEVEL=info \
OTEL_EXPORTER_OTLP_ENDPOINT=http://127.0.0.1:4317 \
   cargo run --package oxidgene-server

OXIDGENE_LOG_LEVEL=warn \
OTEL_EXPORTER_OTLP_ENDPOINT=http://127.0.0.1:5317 \
   cargo run --package oxidgene-worker
```

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

### Geneanet content matching

Three opt-in harnesses measure the perceptual matcher against real data. All
are `#[ignore]`d and self-skipping, because a data archive and a saved session
are hundreds of megabytes of someone's family photographs and are never
committed. Point them at your own and run them in `--release`: the decoders and
the resampler are an order of magnitude slower otherwise, and in different
proportions, so a development build misattributes the cost.

`phash_separation` answers whether the matcher is correct, on renditions it
generates itself. `phash_cost` answers where a hash spends its time, phase by
phase. `phash_real_session` replays a saved session and its archives through
the whole pipeline — exact size claims, target shapes, index build, per-page
lookup — and prints how many pages resolved plus a digest of the pairing.

That pairing is the point: judge a candidate change to the hashing by whether
it survives, never by whether it feels faster. Record a reference with
`OXIDGENE_GENEANET_PAIRING_OUT`, then run the variant against it with
`OXIDGENE_GENEANET_PAIRING_REF`. The comparison separates the two ways a
variant can differ, which are not equivalent: a page the reference resolved and
the variant declined costs one download, while a page both resolved to
different entries means one of them attached the wrong picture. The first is a
number to weigh, the second fails the run.

One account is also one account — a tree with no multi-page deposit exercises
nothing here, and a green run means "this change did not regress that account",
not a general guarantee.

```bash
OXIDGENE_GENEANET_SESSION=/path/geneanet-session.zip \
OXIDGENE_GENEANET_ARCHIVES=/path/a.zip:/path/b.zip \
OXIDGENE_GENEANET_PAIRING_OUT=/tmp/pairing-reference.tsv \
  cargo test --release --package oxidgene-geneanet \
    --test phash_real_session -- --ignored --nocapture
```

The Compose stack includes an OpenTelemetry Collector. It receives OTLP on
loopback ports `4317` (gRPC) and `4318` (HTTP), exposes its health endpoint on
`13133`, and writes log, trace, and metric summaries to its logs:

```bash
docker compose -f docker/docker-compose.yml logs -f otel-collector
```

Replace the `debug` exporters in `docker/otel-collector.yaml` with the desired
telemetry backend exporters for persistent storage. Native host processes can
export to it with
`OTEL_EXPORTER_OTLP_ENDPOINT=http://127.0.0.1:4317`; export remains disabled
when the variable is absent.

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

## 5. Secure Development Practices

These rules apply whenever code accepts network input, imported files, archive
entries, media, local paths, WebView messages, environment variables, or
storage keys. Security controls preserve legitimate genealogy workflows: a
scanner finding alone does not justify removing a supported developer surface
or imposing an arbitrary limit that rejects valid large exports.

### 5.1 Trust boundaries and secure defaults

- Treat request bodies, GraphQL documents, uploaded files, archive metadata,
   decoded media, external URLs, WebView IPC, environment variables, and
   persisted object keys as untrusted at their first application boundary.
- Put enforcement in the component that performs the sensitive operation, not
   only in the UI or a caller. Comments such as `desktop-only` are not access
   controls.
- Model privileged local behavior as an explicit runtime capability. The
   local-file capability defaults to disabled in shared API constructors and
   the standalone server; only the embedded desktop backend enables it.
- Check the capability before opening, indexing, decoding, deleting, or
   returning any local path. REST handlers and GraphQL resolvers enforce and
   test the same rule.
- Keep public constructors and default configurations on the least-privileged
   path. A more capable constructor remains crate-internal unless external
   callers have a documented need for it.
- OpenAPI and GraphiQL are intentional developer surfaces. Do not remove them
   as a substitute for authentication or network isolation; control exposure
   at the actual deployment and authorization boundaries.

### 5.2 Bounded input and resource use

- Enforce request and file limits while reading or streaming, before an
   untrusted payload is fully buffered. On rejection, remove partial spool
   files and other operation-owned state.
- For archive entries, validate both the declared uncompressed size and the
   bytes actually produced. Read at most `limit + 1` when detecting overflow;
   never trust a ZIP central-directory size for allocation or acceptance.
- Prefer structural and per-entry limits plus sequential processing over a
   cumulative archive cap. A GEDZIP may legitimately contain a large media
   collection, so process one medium at a time instead of retaining every
   decompressed entry in memory.
- A compressed-size bound does not by itself prevent decompression bombs.
   Bound expanded output independently and reject declared/decoded size
   mismatches where the format provides both values.
- Before allocating an image buffer, inspect `ImageDecoder::total_bytes()` and
   compare it with the documented decoded-byte budget. Also configure
   `image::Limits`; codec allocation limits are defense in depth, not a
   substitute for the explicit decoded-size check.
- Detect media formats from their bytes rather than trusting a MIME type or
   filename extension. Keep parsing and decoding errors sanitized at API
   boundaries.
- Validate storage keys with a strict grammar before joining paths or issuing
   object-store operations. Reject traversal segments and cross-namespace
   identifiers rather than trying to normalize them.
- Bound recursive or user-shaped computation as well as bytes. GraphQL schemas
   retain explicit depth, complexity, and recursion limits, with regression
   tests that assert rejection behavior.

Endpoint-specific budgets and exceptions remain authoritative in the
[API Contract](api.md). New limits must be justified by actual memory, storage,
or protocol constraints and must account for existing large genealogy exports.

### 5.3 Temporary files and ownership

- Use `tempfile::NamedTempFile`, `TempPath`, or `TempDir` for private,
   collision-resistant creation. Do not construct predictable names under
   `/tmp` and then open them separately.
- Transfer ownership explicitly when a temporary input moves from a request or
   WebView session into a durable background job. A worker must not depend on a
   login window or request-owned temporary directory remaining alive.
- Delete only files and directories created and registered by the current
   process. Never accept an arbitrary client path as cleanup authority.
- Prefer RAII cleanup and retain explicit cleanup on success, failure,
   cancellation, and startup recovery where durable staging is involved.
- Keep temporary paths, original filenames, and archive contents out of client
   errors, logs, fixtures, and committed artifacts.

### 5.4 Desktop WebView and external content

- Validate application-controlled download and IPC URLs at the action boundary:
   require HTTPS and an approved Geneanet host before native code fetches or
   writes anything.
- Do not confuse an action allowlist with a global WebView network filter. The
   authenticated page may load required scripts, styles, redirects, and other
   subresources from its provider's related domains, such as `geneacdn.net`.
- Never move cookies, tokens, passwords, page HTML, or session archives through
   logs or ordinary application telemetry. Keep authenticated network requests
   inside the WebView session when direct clients are intentionally rejected.
- Treat every filesystem path received from JavaScript as untrusted even when
   the top-level page URL was validated; native handlers still apply capability,
   ownership, and path checks.

### 5.5 Build secrets and generated artifacts

- Invoke Dioxus through `scripts/dx.sh`, including in local recipes and image
   builds. Dioxus serializes rustc arguments and environment data under
   `target/dx/.captured-args`; the wrapper removes credential-shaped variables
   before those files are generated.
- Do not pass credentials through compile-time frontend environment variables.
   A WASM bundle and its build metadata are client-visible artifacts.
- Keep fuzz corpora, crash artifacts, and fuzz build targets ignored. Commit the
   harness and its reproducible manifest, not generated inputs or binaries.
- Never use real genealogy exports, media, sessions, or files under `samples/`
   as a fuzz corpus, fixture, screenshot source, or committed reproduction.

### 5.6 API symmetry and regression coverage

- Security behavior is part of the REST/GraphQL symmetry requirement. Apply
   equivalent capability checks, size rules, error codes, and tests to both
   surfaces in the same change.
- Test both sides of a security boundary: secure defaults must reject the
   operation, while the explicitly capable desktop path must retain its
   legitimate workflow.
- Bug fixes include a focused test that would fail without the fix. Resource
   tests exercise declared-size lies, expanded-output overflow, decoder memory
   budgets, traversal attempts, and cleanup after failures as applicable.
- Keep public errors generic and stable. Tests may assert the machine-readable
   code and absence of internal paths, request IDs for expected validation
   failures, SQL, archive details, or source chains.

### 5.7 Static analysis, dependency audit, and fuzzing

Install or make the optional security tools available before running their
checks. Semgrep, Trivy, `cargo-audit`, and `cargo-fuzz` are declared in
`mise.toml`; fuzzing requires a nightly Rust toolchain.

```bash
just setup
rustup toolchain install nightly
```

Run a repository scan without generated build trees or fuzz outputs:

```bash
semgrep scan --config p/rust --config p/security-audit \
   --jobs 1 --max-target-bytes 2000000 \
   --exclude target --exclude '*/fuzz/target' \
   --exclude '*/fuzz/corpus' --exclude '*/fuzz/artifacts' .

trivy fs --scanners vuln,misconfig,secret \
   --skip-dirs target --skip-dirs crates/oxidgene-geneanet/fuzz/target .

cargo audit
```

Run the session decoder fuzz target with synthetic libFuzzer inputs and an
explicit time budget:

```bash
cargo +nightly fuzz run \
   --fuzz-dir crates/oxidgene-geneanet/fuzz \
   session_decode -- -max_total_time=30
```

Fuzzing is opt-in and never part of `just check`. A crash is not fixed until a
minimal anonymized regression test covers it; generated corpora remain local.

### 5.8 Triage findings before changing code

- Confirm that a dependency advisory is in an active target's normal or build
   graph with `cargo tree -i <crate>@<version> -e normal,build`. A lockfile-only
   package is tracked but is not linked into the current binaries.
- Trace active transitive advisories to their owning framework. Do not force an
   incompatible isolated upgrade; document the residual risk and update the
   framework when a compatible path exists.
- Distinguish a dangerous source/sink flow from a syntactic match. A static
   analysis result is accepted only after checking how the value reaches a
   filesystem, process, network, authorization, parser, or allocation boundary.
- Render templated deployment files before accepting a template-scanner result.
   For Helm, use `helm lint charts/oxidgene` and inspect `helm template`; values
   injected through `toYaml` may be invisible to a raw-template scanner.
- Do not hard-code a Helm namespace merely to satisfy a scanner; namespace is
   an installation choice. Treat the configured project registry as a trust
   decision, and prefer immutable tags or digests where release policy requires
   stronger provenance.
- Kubernetes startup, readiness, and liveness probes provide workload health
   checks. A Dockerfile `HEALTHCHECK` is separately useful for direct
   `docker run`, but duplicating probes or inventing a worker HTTP endpoint is
   not a security fix.

Record confirmed defects, contextual findings, false positives, residual
dependency risks, and commands actually executed separately. Never weaken a
scanner globally to hide one understood exception.

### 5.9 Validation sequence

During implementation, run the cheapest focused test that can falsify the
current fix immediately after the edit. Then run the affected crate tests and
finish code changes with:

```bash
just check
```

For deployment changes, additionally lint and render the chart. For parser,
archive, image, or session changes, run the relevant focused regression tests
and fuzz target where practical. Do not run concurrent Cargo builds in the same
workspace target directory; they add contention and make failures harder to
attribute.

## 6. Release Automation

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