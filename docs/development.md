---
type: "Development Specification"
title: "Development Environment and Workflows"
description: "Local development, secure coding practices, verification workflows, and just command reference for OxidGene."
tags: [oxidgene, specification, development, rust, security, just]
generated: { by: claude-code/claude-opus-5-5, at: 2026-10-05T08:00:00Z }
---

# Development Environment and Workflows

> Part of the [OxidGene Specifications](index.md).
> See also: [Architecture](architecture.md) · [Cross-cutting Rules](cross-cutting.md)

---

## 1. Prerequisites

- [Rust](https://rustup.rs/) through rustup, which installs the release
   `rust-toolchain.toml` pins on first use (§2.8).
- [just](https://github.com/casey/just) task runner.
- [mise](https://mise.jdx.dev/) tool version manager.
- PostgreSQL 16+ or Docker Compose for the web backend. The Compose stack also
   provides RustFS for S3-compatible media storage and Redis for future user
   sessions.
- The `wasm32-unknown-unknown` Rust target for the browser application.
- [Dioxus CLI](https://dioxuslabs.com/learn/0.7/getting_started/) 0.7.10.
- `cargo-nextest` for the workspace test recipes.
- `cargo-watch` for backend hot reload; optional unless using `just dev-web-watch`.
- `cargo-xwin` for cross-compiling the desktop application to Windows X64.
- `cargo-deny`, `cargo-machete` and `cargo-audit` for `just deps`;
   `cargo-udeps`, `scc` and `actionlint` for the scheduled reports and the
   workflow checks (§2.8).
- [uv](https://docs.astral.sh/uv/) for the cyclomatic complexity check.
- [Node.js](https://nodejs.org/) 24 for the browser JavaScript tests and the
   end-to-end suite, which installs Playwright and its Chromium on first run.

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
| `just test` | Run the unit and functional tests (§2.7). |
| `just test-unit` | Run the unit tests and the documentation examples (§2.7). |
| `just test-functional` | Run the functional tests (§2.7). |
| `just test-verbose` | Run the workspace tests while preserving test output. |
| `just ui-js` | Run the browser JavaScript unit tests on Node.js (§2.7). |
| `just e2e [args]` | Build the web bundle and server, then run the Playwright end-to-end suite (§2.7). |
| `just screenshots` | Build the web bundle and server, take the README and [Features](features.md) screenshots of the fictitious screenshot tree, and encode them with the README carousel into `assets/screenshots/` (§2.7). |
| `just fmt` | Format all Rust source files. |
| `just fmt-check` | Check Rust formatting without changing files. |
| `just clippy` | Run Clippy for all workspace targets and deny warnings, then for `oxidgene-archives` with its `native` transport, which no workspace member enables yet. |
| `just wasm` | Run Clippy on the browser application for the `wasm32-unknown-unknown` target and deny warnings, the check that the shared UI still compiles to WebAssembly; run it after changing `oxidgene-ui` or its dependencies. The browser binary declares its dependencies for `wasm32` only, so the native `just clippy` does not see its code. Not part of `just check`; the CI Clippy matrix runs it. |
| `just deps` | Check the dependency graph: no unused dependency (`cargo machete`), nothing `deny.toml` refuses (`cargo deny check`: advisories, licences, bans, sources), no known vulnerability (`cargo audit`), and no more duplicated crates than `scripts/budgets.json` allows (§2.8). |
| `just sql-plans` | Read the query plan of every statement the API runs over a populated tree and fail on a full scan of a large table (§2.8). |
| `just duplication [--report]` | Fail when the duplicated share of the Rust code grows past its budget; `--report` lists the duplicate blocks (§2.8). |
| `just cyclomatic` | Fail on any function above a cyclomatic complexity of 15 (§2.1). |
| `just check` | Run formatting verification, Clippy, the cyclomatic complexity check, and tests. |
| `just scaling` | Time the tree-wide computations on two tree sizes in release mode (§2.1). |
| `just real-import [args]` | Import your own exports end to end through the desktop's backend, in release mode (§3, *Real-data import check*). |
| `just import-memory` | Import a generated, fictitious Geneanet tree in release mode and fail when its peak or retained memory exceeds its budget (§2.8). |
| `just bench` | Run the opt-in benchmarks of the profile service and the person search, in release mode. |
| `just test-postgres` | Migrate the disposable PostgreSQL database named by `OXIDGENE_TEST_DATABASE_URL` up and down. |
| `just test-s3` | Round-trip media through the Compose stack's RustFS service (§3). |
| `just session-check` | Stream the private Geneanet session archive named by `OXIDGENE_GENEANET_SESSION` through REST (§5.7). |
| `just geneanet-harness [args]` | Run the Geneanet content-matching harnesses on your own archives, in release mode (§3, *Geneanet content matching*). |
| `just theme-preview` | Write an HTML preview of every theme's pedigree card to `OXIDGENE_PREVIEW_DIR`. |
| `just graphql-schema` | Rewrite `docs/schema.graphql`, the committed SDL of the GraphQL schema, after an intended schema change ([API Contract](api.md#schema)). |
| `just openapi` | Build `oxidgene-api`, whose build script regenerates from the REST router the OpenAPI document served at `/api/v1/openapi.json` ([API Contract](api.md)). |
| `just clean` | Remove Cargo build artifacts. |
| `just doc` | Generate and open workspace API documentation. |

Run `just check` before committing code changes.

Every function stays within a cognitive complexity of 15, the conventional
range. The workspace `Cargo.toml` enables Clippy's `cognitive_complexity`
lint for every member (`[lints] workspace = true`) and `clippy.toml` sets the
threshold, so `just clippy` and the CI Clippy job, which deny warnings, fail
on a function above it. Bring such a function back under the threshold by
extracting named steps or sharing code with its look-alikes; an `allow` is not
a fix. A `tracing` macro counts for several points on its own, so a function
that logs on several branches is better served by one log call.

Every function also stays within a cyclomatic complexity of 15, counted by
[lizard](https://github.com/terryyin/lizard) through `scripts/cyclomatic.py`.
`just cyclomatic`, part of `just check`, and the CI Cyclomatic complexity job
fail on a function above it. Where cognitive complexity weighs nesting,
cyclomatic complexity counts paths: every `if`, `for`, `while`, `match`, `?`,
`&&` and `||` adds one. The wrapper pins lizard and corrects its Rust
reader, which counts the `||` opening a closure without parameters as a
logical operator — every `use_signal(|| false)` of a component would add a
path — and a `where` clause as a branch. It keeps `||` only between two
operands and drops `where`, then passes its arguments to lizard unchanged.
Closure bodies count towards the function that defines them, so a component
brings its handlers under the threshold by moving them into named functions
or methods of a small state struct.

Algorithmic complexity is tested at three levels.

- **SQL statements.** `crates/oxidgene-api/tests/query_scaling_test.rs`, in
  the normal suite, runs a survey of REST requests — the tree-wide reads,
  a person's pages, the lists, the dictionaries, and representative
  mutations — against a generated tree and one four times larger, both with
  a note, a citation, a media link and a portrait crop on every person,
  counting the statements SeaORM issues. A request that runs more on the
  larger one queries per person, family or event, an N+1 to batch. The same
  survey runs over GraphQL, the connections with their nested fields among
  it, which must cost the same for a page of 40 records and one of 100: the
  nested fields are read in batches, one query per relation for the whole
  page (`graphql/loaders.rs`). The batch reads (pedigrees at three depths,
  portrait images, image data, gallery bundles, relation labels) are asked
  for 4 and for 64 ids and must cost the same.
  Add a new tree-wide or per-person route to the survey.
- **Counted work.** Where an in-memory computation risks quadratic work on
  a common case, a unit test counts that work deterministically: the
  potential duplicates compare each record only with the homonyms born
  within the gap of it, and a test counts the pairs.
- **Timing.** `just scaling` times the statistics, anomalies, duplicates and
  ancestry computations, the GEDCOM import and export, the projection
  rebuild and the dictionaries on a tree and one eight times larger, in
  release mode, and fails when one grows more than three times as fast as a
  reference linear pass over the same projections — the reference absorbs
  the cache effects a larger tree has on linear work too. The nightly
  Scaling job runs it (§2.8), as timing on a shared runner is too noisy to
  gate a merge; locally it is opt-in, as timing wants an optimised build and
  a quiet machine.

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
| `just build-desktop-win64-release` | Cross-compile an optimized Windows X64 desktop release with the WebView2 loader linked into the executable. |

Set `OTEL_EXPORTER_OTLP_ENDPOINT` when running the desktop binary to export native
desktop logs, spans, and metrics over OTLP/gRPC. The export covers the embedded
API and worker as well as native UI `tracing` events; it is disabled when the
variable is absent. In that mode, log events still reach the console but span
callsites are disabled rather than creating spans that are later discarded.

`OXIDGENE_LOG_LEVEL` configures desktop logs. `--log-level FILTER` overrides
the environment for one invocation, and `--debug` selects
`info,oxidgene_ui=debug,oxidgene_api=debug,oxidgene_db=debug` only when neither
explicit setting is present. `OXIDGENE_LOG_FORMAT` (`text` by default, or
`json`) selects the console format, and `--log-format FORMAT` overrides it for
one invocation.

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

`just build-desktop-win64-release` uses the MSVC target through `cargo-xwin`.
This target statically links Microsoft's WebView2 loader, so
`WebView2Loader.dll` does not need to be distributed beside the executable.
The WebView2 Evergreen Runtime itself remains a system prerequisite; it is
normally already installed on supported Windows 10 and Windows 11 systems.
The release executable uses the Windows GUI subsystem, so opening it from
Explorer does not create a console window. When started from an existing
terminal, it attaches to that parent console and writes its normal logs there.
Debug builds retain the console subsystem.

### 2.5 Observability configuration by process

Each native process reads only its own environment. It can therefore choose a
different filter and collector, or disable OTLP independently:

| Process | Log configuration | OTLP configuration |
|---|---|---|
| Server | `OXIDGENE_LOG_LEVEL`, `OXIDGENE_LOG_FORMAT` | `OTEL_EXPORTER_OTLP_ENDPOINT` |
| Worker | `OXIDGENE_LOG_LEVEL`, `OXIDGENE_LOG_FORMAT` | `OTEL_EXPORTER_OTLP_ENDPOINT` |
| Desktop | `OXIDGENE_LOG_LEVEL` or `--log-level`, `OXIDGENE_LOG_FORMAT` or `--log-format` | `OTEL_EXPORTER_OTLP_ENDPOINT` |
| Browser WASM | Build-time `OXIDGENE_LOG_LEVEL` threshold | Runtime `frontend.otlpEndpoint`, falling back to build-time `OTEL_EXPORTER_OTLP_ENDPOINT`, over OTLP/HTTP |

Native log filters use `tracing_subscriber::EnvFilter` syntax. A simple level
such as `warn` applies globally; a directive list such as
`info,oxidgene_api=debug,sea_orm=warn` sets per-target levels. Invalid filters
fail process initialization. `RUST_LOG` is not read, so there is no hidden
second source of filter configuration.

Native console logs are human-readable `text` by default, coloured only when
written to a terminal. `OXIDGENE_LOG_FORMAT=json` writes one flat JSON object
per event (`timestamp`, `level`, `target`, `message`, and the event's own
fields) for log collectors; the server and worker images set it. An invalid
format stops the process at startup without echoing the value. With OTLP export
enabled every event is also exported as an OTLP log record, so a pipeline that
collects both the JSON console lines and the OTLP logs ships each event twice:
collect one or the other.

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

### 2.6 Reference Data

| Command | Purpose |
|---------|---------|
| `just places` | Regenerate the place dictionary, `assets/places/places.csv.br`, from the latest edition of each open-data source. Commit the result. |
| `just places --cached` | Regenerate it from the downloads of the previous run. |

The sources, the file format and the output location are specified in
[Place Dictionary](place-dictionary.md). The generator needs network access to
INSEE, data.gouv.fr, geo.api.gouv.fr, the ONS Open Geography Portal, Destatis,
ISTAT, INE, the BFS register, GUS TERYT, the Census Bureau, the DGT CAOP, CBS
and the Wikidata query service.

### 2.7 Test Categories

| Category | What | Command | CI job | Gates `CI` |
|----------|------|---------|--------|------------|
| Unit | The `#[cfg(test)]` modules of every library and binary, then the documentation examples, which nextest does not run | `just test-unit` | Unit tests | Yes |
| Functional | The integration test targets under `crates/*/tests` and `apps/*/tests`: REST and GraphQL scenarios against an in-memory SQLite database, repositories, migrations, GEDCOM, MCP, and the SQL statement counts of `query_scaling_test.rs` | `just test-functional` | Functional tests | Yes |
| Browser JavaScript | The dependency-free Node.js tests of the browser glue under `crates/oxidgene-ui/tests/*.test.mjs` | `just ui-js` | UI JavaScript tests | Yes |
| Performance | The `#[ignore]`d timing tests of `algorithm_scaling_test.rs`, in release mode (§2.1) | `just scaling` | Nightly: Scaling | No |
| Memory | The `#[ignore]`d `import_memory_test.rs`: a Geneanet import of a generated tree, in release mode, against its memory budgets (§2.8) | `just import-memory` | Nightly: Import memory | No |
| End-to-end | The Playwright suite of `e2e/`, driving the web application in Chromium, its request budgets and trace continuity included | `just e2e` | Nightly: E2E | No |

`just test`, and through it `just check`, runs the unit and functional tests.
The selection is by Cargo target (`--lib --bins`, `--doc`, `--test '*'`)
rather than by a nextest filter, so each category builds only its own test
binaries. Other `#[ignore]`d tests need private data, PostgreSQL or RustFS, or
only time something; each one's ignore reason names the `just` recipe that
runs it (`just bench`, `just test-postgres`, `just test-s3`,
`just session-check`, `just geneanet-harness`, `just theme-preview`,
`just real-import`, `just scaling`, `just import-memory`), and a guard fails
on a reason that names none.

The opt-in tests and the golden checks read these variables:

| Variable | Read by |
|----------|---------|
| `OXIDGENE_TEST_DATABASE_URL` | `oxidgene-db`'s `migration_test` with the `postgres` feature: an empty disposable PostgreSQL database to migrate up and down |
| `OXIDGENE_REAL_*` | `just real-import` (§3, *Real-data import check*), which also sets `OXIDGENE_REAL_WORKDIR` to its staging directory under `target/` |
| `OXIDGENE_GENEANET_ARCHIVES`, `OXIDGENE_RENDITION_WIDTH` | `oxidgene-geneanet`'s `phash_separation` (§3, *Geneanet content matching*); the width of the simulated rendition defaults to 600 pixels |
| `OXIDGENE_GW`, `OXIDGENE_REFERENCES` | `oxidgene-geneanet`'s `unkeyed_references`: a `.gw` export and a dump of Geneanet's media references, to measure which unkeyed references the name alone would join |
| `OXIDGENE_PHASH_JPEG` | The `compare_jpeg_phash_decode_strategies` test of `oxidgene-geneanet`'s `phash` module: a JPEG, or a ZIP holding one, to time the decode strategies on |
| `OXIDGENE_PREVIEW_DIR` | The `theme_preview` test of `oxidgene-ui`, which writes an HTML preview of every theme there (default: the current directory) |
| `OXIDGENE_BLESS` | The pedigree layout golden tests of `oxidgene-ui`: set, they print fresh golden blocks instead of comparing |
| `OXIDGENE_BLESS_E2E_FIXTURE` | `rest_test.rs`: set to `1`, it rewrites the end-to-end fixture (below) |

CI runs the unit, functional and browser JavaScript categories as jobs of
their own on every change outside the documentation; the performance and
end-to-end ones run every night (§2.8). The `CI` job, the one status check
branch protection requires, gathers the jobs of tiers 1 and 2.

**End-to-end suite.** `e2e/` holds a Node.js project whose only dependency is
a pinned `@playwright/test`. `just e2e` builds the debug web bundle for the
API on `http://127.0.0.1:18080` and the server, installs the dependencies and
Chromium when missing, and runs the suite; extra arguments go to Playwright
(`just e2e --headed`, `just e2e tests/home.spec.ts`). The ports stay clear of
`just dev-web`'s 8080 and 8081, so both can run at once; `E2E_API_PORT` and
`E2E_WEB_PORT` move them. Playwright starts two servers and refuses to reuse
one already listening on those ports:

- `e2e/scripts/backend.mjs` runs `oxidgene-server` on a SQLite database, a
   media root and a working directory (`OXIDGENE_WORK_DIR`) inside a fresh
   temporary directory, removed when it stops, with
   the web origin as its CORS origin. The suite never sees a developer's data.
- `e2e/scripts/static-server.mjs` serves `target/dx/oxidgene-web/debug/web/public`,
   answering client-side routes with `index.html`, plus the web image's
   `docker/runtime-config.js`, with the security headers of
   `docker/security-headers.conf`: the suite runs under the deployed
   Content-Security-Policy, which it extends only with what the debug bundle
   adds (the hash of `dx`'s inline development-toast script and its web
   font).

Each test that needs data seeds its own tree through the REST API, as an
import job the backend's embedded worker runs, from
`e2e/fixtures/family-blocks.ged`: thirty fictitious persons in three
unrelated families, block 0's root as the SOSA root. The file is the
functional tests' `family_blocks_gedcom(3)`; `rest_test.rs` fails when the
two drift apart and rewrites the file when run with
`OXIDGENE_BLESS_E2E_FIXTURE=1`. Every test also fails on a console error, an
uncaught exception, or a translation key left untranslated on the page, and
the browser is cut off from the network (the debug bundle's
development-toast font is the only outside request). Tests select elements by role and accessible name,
falling back to a title or a class where the markup offers no name.

A test that documents a known defect is marked `test.fail()` with a comment
naming it: it passes while the defect stands and fails once it is fixed,
which is the signal to remove the mark.

**Screenshots.** `just screenshots` takes the images of the README and of
[Features](features.md) with the suite's servers and browser, through its own
Playwright configuration (`e2e/playwright.screenshots.config.ts`), so
`just e2e` never runs it. `e2e/screenshots/family.ts` builds the fictitious
Landrevel family from a seeded generator — some 400 persons over eight
generations, invented names and dates on real place names so the map can
locate them, notes, sources, repositories, witnesses, a few deliberate
anomalies and a pair of duplicates — and imports it as a GEDZIP with the
public-domain portraits and document scans of `e2e/fixtures/media/`, whose
sources and licences `e2e/fixtures/media/CREDITS.md` lists. A handful of
edits through REST then give the home page its recently modified persons and
the root a history. `e2e/screenshots/screenshots.spec.ts` opens each page at
1440 × 900 pixels, in English, in the light or dark theme it names, with the
browser clock and every timestamp the API returns pinned to fixed instants,
and saves the captures under `target/screenshots/raw/`.
`scripts/screenshots.py` then writes each one to `assets/screenshots/` as a
lossless WebP, about half the size of the same PNG, and assembles the README
carousel, `readme-carousel.webp`: an animated WebP of ten frames, three
seconds each, 1200 pixels wide, lossy. A capture that differs from the
committed image only by Chromium's occasional rounding — at most 200 pixels,
two levels apart — keeps the committed file, so a run over unchanged pages
leaves the repository untouched. Re-run it after a visible change to a page
it shows, and look at every image before committing it: the fixture is
fictitious, and a screenshot must never show anyone's real data.

The E2E workflow (`.github/workflows/e2e.yml`) runs from the nightly
workflow, before a release, and on demand. It installs the toolchain of
`rust-toolchain.toml` with its WebAssembly target and the Dioxus CLI itself,
uploads the request report of every run, and the Playwright report and
traces when a test fails.

### 2.8 Guards

Guards are checks that fail when the codebase drifts from a rule of
AGENTS.md — the layering, the REST/GraphQL symmetry, the specification
format, privacy — rather than when one feature breaks. Each one documents in
its source what drift it prevents and how to fix a failure. They run in
three tiers, so that a pull request waits only for the fast ones:

- **Tier 1** — every pull request, blocking, fast. Rust tests that read the
  sources, most of them part of `just check`, plus quick tools.
- **Tier 2** — every pull request touching code, blocking, each its own
  parallel CI job: the functional guards that need a database, the SQL
  plans, the dependency audit, the duplication budget.
- **Tier 3** — every night (`.github/workflows/nightly.yml`), on demand,
  and as gates of a release; never on a pull request: the browser
  suite, budgets, timing, reports and the next toolchains.

`.github/workflows/release.yml` runs on a `v*` tag, and on a push to a
`release/**` branch to rehearse one: it calls the CI workflow with every job
forced on and the nightly workflow, and publishes nothing unless every job
of both passed (the next-toolchain jobs report without failing).

| Check | Tier | Command | CI job | What it guards |
|---|---|---|---|---|
| Layering | 1 | `just test` (`oxidgene-guards`) | Repository guards | `service/` and `profile/` import no surface; REST handlers and GraphQL mutations call no repository write and open no transaction |
| Migration policy | 1 | `just test` (`oxidgene-guards`) | Repository guards | The initial migration is the only one until a release (`RELEASED` lifts it) |
| Lint discipline | 1 | `just test` (`oxidgene-guards`), `just clippy` | Repository guards, Clippy | Every crate takes the workspace lints; no `#[allow]` (Clippy's `allow_attributes`), no crate-wide allow or cap; every `#[ignore]` names an existing `just` recipe |
| Specification format | 1 | `just test` (`oxidgene-guards`) | Repository guards | `docs/` is a conformant OKF bundle; every link, `#anchor` and `§N` resolves; on a pull request, an edited specification carries a new `generated.at` |
| Dead CSS | 1 | `just test` (`oxidgene-guards`) | Repository guards | Every class of the `*_STYLES` sheets is produced by the markup; every `var(--x)` is defined |
| Raw HTML | 1 | `just test` (`oxidgene-guards`) | Repository guards | Every `dangerous_inner_html` is declared with its sanitizer |
| UI tracing coverage | 1 | `just test` (`oxidgene-guards`) | Repository guards | Every route opens its page's load trace; no plain `use_resource` |
| i18n keys | 1 | `just test-unit` | Unit tests | The eight tables carry the same keys and placeholders; every literal key exists; every key is used |
| Projection shape | 1 | `just test` | Functional tests | `PersonProfile`'s JSON shape changes only with `PROJECTION_SCHEMA_VERSION` |
| REST/GraphQL parity | 1 | `just test` (`guards_test`) | Functional tests | Every route is mapped to its GraphQL twin in a declared table, every root field to a route |
| api.md and schema | 1 | `just test` (`guards_test`), `just graphql-schema` | Functional tests | docs/api.md's tables list exactly the router's routes; `docs/schema.graphql` is the schema's SDL |
| Clippy matrix | 1 | `just clippy`, `just wasm`, Clippy of `-p oxidgene-desktop --no-default-features`, of `-p oxidgene-api` and of `-p oxidgene-archives --features native` | Clippy (5 variants) | Every build variant compiles without a warning: native, WebAssembly, desktop without telemetry, API without GraphQL, archive portals with the native transport |
| Cyclomatic complexity | 1 | `just cyclomatic` | Cyclomatic complexity | No function above 15 paths |
| Unused dependencies | 1 | `cargo machete` (in `just deps`) | Unused dependencies | No dependency declared and unused |
| Stack depth | 2 | `just test` (`guards_test`) | Functional tests | Every REST route, GraphQL root field and the background jobs they queue run on a 1 MiB stack in a debug build, half a tokio worker's |
| Cross-tree access | 2 | `just test` (`guards_test`) | Functional tests | No REST route or GraphQL field reaches a record of another tree |
| Pagination | 2 | `just test` (`guards_test`) | Functional tests | Every collection clamps `first` to 1–100 and pages by cursor; every whole list is declared |
| Purge completeness | 2 | `just test` (`guards_test`) | Functional tests | A purged tree leaves no row in any table and no file in the media store |
| History without duplicates | 2 | `just test` (`guards_test`) | Functional tests | An import stores no version, no stored version copies the live record, deletion markers carry no snapshot |
| Statement counts | 2 | `just test` (`query_scaling_test`) | Functional tests | No REST or GraphQL request issues statements per record; a batch of 64 ids costs what one of 4 does |
| Privacy of logs | 2 | `just test` (`guards_test`) | Functional tests | No span or event field carries a name, place, note or SQL value |
| SQL plans | 2 | `just sql-plans` | SQL plans | No statement scans a large table without an index |
| Dependencies | 2 | `just deps` | Dependencies | No advisory, licence, wildcard or source `deny.toml` refuses; no known vulnerability; duplicated crates within `scripts/budgets.json` |
| Duplication | 2 | `just duplication` | Code duplication | The duplicated share of the Rust code stays within its budget |
| Request budgets | 3 | `just e2e tests/request-budgets.spec.ts` | Nightly: E2E | Every page's API requests, waterfall depth, duplicates and bytes within `e2e/budgets/requests.json` |
| Trace continuity | 3 | `just e2e tests/trace-continuity.spec.ts` | Nightly: E2E | Every page display's API requests share one trace of their own |
| Binary budgets | 3 | `python3 scripts/budgets.py binaries` after the release builds | Nightly: Binary budgets | The size of the desktop, server, worker and web bundle, and the crates each links, within `scripts/budgets.json` |
| Scaling | 3 | `just scaling` | Nightly: Scaling | No tree-wide computation grows faster than linear |
| Import memory | 3 | `just import-memory` | Nightly: Import memory | A Geneanet import of a generated tree peaks, and keeps once done, within the budgets of `import_memory_test.rs` |
| udeps | 3 | `cargo +nightly udeps --workspace --all-targets` | Nightly: Unused dependencies (udeps) | No dependency the compiler finds unused |
| Reports | 3 | `cargo update --dry-run --verbose`, `scc` | Nightly: Reports | The pending and held-back updates, the size of the code (artifacts, never failing) |
| Next toolchains | 3 | `cargo +beta clippy …`, `cargo +nightly clippy …`, `cargo +beta check --future-incompat-report` | Nightly: Next toolchain | Lints and warnings of the next Rust releases, seen before they break the pinned one (reported, never failing) |

**Updating a budget.** A budget moves only with an intended change, in the
commit that makes it, whose message says why:

- request budgets: `E2E_BUDGET_UPDATE=1 just e2e tests/request-budgets.spec.ts`
  rewrites `e2e/budgets/requests.json`. Requests, depth and duplicates
  compare exactly, bytes within a quarter; the time to content, measured
  under 50 ms of added latency, lands in `e2e/test-results/request-report.json`
  and the nightly artifact, a trend to read, never a gate;
- binary budgets: after the release builds, `python3 scripts/budgets.py
  binaries --update` (5 % on sizes and three crates of margin);
- duplicated crates: `python3 scripts/budgets.py dependencies --update`,
  and a direct dependency that cannot be aligned is declared with its
  reason in `scripts/budgets.json`;
- duplication: lower `BUDGET` in `scripts/duplication.py` when a
  refactoring brings the rate down;
- import memory: `PEAK_BUDGET_MIB` and `RETAINED_BUDGET_MIB` in
  `crates/oxidgene-api/tests/import_memory_test.rs`, set about a quarter
  above what `just import-memory` measures, with that measurement in their
  comments;
- GraphQL schema and projection shape: `just graphql-schema`, and
  `OXIDGENE_BLESS=1` on `projection_shape_test` with the version bump.

**The pinned toolchain.** `rust-toolchain.toml` pins the Rust release every
build uses, locally and in CI; the CI image installs it, and rebuilds when
the file changes. Renovate proposes each new release. Take it once the
nightly Next toolchain jobs are green on beta — they show the new lints and
warnings weeks before — fix what they report, then merge the bump.

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
just test-s3
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
  just geneanet-harness phash_real_session
```

### Real-data import check

`crates/oxidgene-api/tests/real_import_test.rs` imports your own exports the
way the desktop does — the router with local file access and the in-process
background worker — over a file-backed SQLite database and a media root on
disk, then checks the trees they produce. Every test is `#[ignore]`d and skips
itself when the variables naming its files are unset; `just real-import` runs
them in release mode, one at a time, and passes extra arguments to nextest
(`just real-import gedcom` runs the GEDCOM test alone).

| Variable | File |
|----------|------|
| `OXIDGENE_REAL_GW` and `OXIDGENE_REAL_SESSION` | A GeneWeb `.gw` export of a Geneanet tree, and a saved session of the same account |
| `OXIDGENE_REAL_ARCHIVES` | That account's media archives, comma-separated (optional) |
| `OXIDGENE_REAL_FIDELITY` | `originals` or `renditions`; defaults to `originals` when archives are given |
| `OXIDGENE_REAL_GED` | GEDCOM files, comma-separated |
| `OXIDGENE_REAL_GDZ` | A GEDZIP archive |

The Geneanet test walks the wizard's REST calls — inspect, archive index,
session decode, preview, plan, import — and follows the job as the wizard
polls it; the GEDCOM and GEDZIP tests upload through the import jobs. Each
imported tree is then checked: the receipt against the stored rows, no media
row pointing at nothing or missing its file, every person projected
(`is_materialized`) and searchable, no record version written by the import,
and the tree-wide reads (statistics, anomalies, duplicates, dictionaries,
search, the SOSA root's pedigree when one is set) answering. The Geneanet tree
and the GEDZIP one are also exported as GEDZIP and re-imported into a second
tree, and every count that changed on the way fails the run.

The recipe stages everything, `TMPDIR` included, under `target/real-import`,
on disk rather than in a RAM-backed `/tmp`, and deletes it on exit. The tests
print aggregates only — timings, the peak resident memory of each phase and
what the process still holds after it, counts and error codes — never a row,
a name or an error message, which can quote the file.

**Import memory check.** `just import-memory` measures the same Geneanet
import on a tree it generates itself, so it runs anywhere and every night:
ten thousand fictitious people, 160 photographs (a group photo every eighth,
an identification box on every other one) matched in the archive by size,
and two scanned documents of 48 and 8 pages matched by the content of their
renditions. It stages the fixture and the job's files under
`target/import-memory`, runs the job with the worker's own loop step, and
reports each import phase's time and peak resident memory, by the tracing
span it runs under. It fails when the job's peak, or what the process keeps
once the job has ended, exceeds its budget (§2.8, *Updating a budget*).

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
[OxidGene Helm chart](../charts/oxidgene/README.md).

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
   at the actual deployment and authorization boundaries. GraphiQL loads its
   page from a public CDN, so it is off unless `OXIDGENE_GRAPHIQL` enables it:
   `just dev-web` and the Compose stack do, the server's default and the Helm
   chart (`backend.graphiql`) do not.

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

- Stage every working file through `oxidgene_api::workdir::WorkDir`
   (`job_scratch`, `staged_file`, `staged_directory`, `anonymous_file`),
   never `std::env::temp_dir()` or a bare `tempfile::tempfile()`: the system
   temporary directory is often a RAM-backed `tmpfs`, and an import stages
   files as large as its archives ([Architecture §8.3](architecture.md#83-local-files)).
   Those helpers create private, collision-resistant `tempfile` entries; do
   not construct predictable names and then open them separately. Tests may
   use `tempfile::tempdir()` for small fixtures; anything large goes under
   `target/`, as `just real-import` does.
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

For an explicit local session-load check, set `OXIDGENE_GENEANET_SESSION` to
an absolute archive path and run `just session-check`. The check streams the archive through REST, verifies staged files,
and removes them without printing genealogy content. It is not part of normal CI.

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

For changes to the web application's pages or the API they call, also run
`just e2e`.

For browser download transport changes, also run the dependency-free Node.js
tests:

```bash
just ui-js
```

These exercise stream delivery, picker cancellation, failed writes and requests,
and the local-Blob fallback. Native transfer tests run in the normal Rust suite
and verify that data reaches disk before the response ends, and that failures
preserve existing destination files. Real-browser save-dialog interaction still
requires a manual check on supported desktop and mobile browsers.

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

Before anything is built or published, the release gates run tiers 1 to 3
of the guards (§2.8). The release is created only after every platform build
and publication job has succeeded. Desktop artifacts are currently unsigned portable executables, not
platform installers.
