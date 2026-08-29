---
type: "Architecture Specification"
title: "Technical Architecture"
description: "Technical architecture, crate boundaries, stack choices, and deployment model for OxidGene."
tags: [oxidgene, specification, architecture, rust]
timestamp: 2026-06-17T00:00:00Z
---


# Technical Architecture

> Part of the [OxidGene Specifications](index.md).
> See also: [Data Model](data-model.md) · [API Contract](api.md) · [Roadmap](roadmap.md)

---

## 1. Technology Stack

| Layer | Technology | Version | Notes |
|---|---|---|---|
| Language | Rust | stable | Single language across the entire stack |
| Frontend | Dioxus | 0.7+ | Web (WASM) + Desktop from single codebase |
| Desktop shell | Wry (WebView) | via Dioxus | System WebView, small binary size |
| Backend framework | Axum | 0.8+ | Tokio-based, tower middleware |
| GraphQL | async-graphql | 7.2+ | With async-graphql-axum integration |
| ORM | SeaORM | 2.0+ | Async, supports PostgreSQL + SQLite |
| Web database | PostgreSQL | 16+ | Production web deployment |
| Desktop database | SQLite | 3.35+ | Embedded in desktop binary |
| GEDCOM | ged_io | 0.16+ | Read/write, GEDCOM 5.5.1 + 7.0, streaming |
| GeneWeb `.gw` | [geneweb](https://github.com/trois-six/rust-geneweb) | 0.1+ | Read only, incl. `gwplus`; converts to the same `ged_io` model, so one domain mapping serves both formats |
| Read projections | Same database | — | `person_denorm` and `person_search_fts`; no cache tier. See [Data Model §4](data-model.md) |
| Web object storage | S3-compatible | — | Durable media bytes; RustFS in development Compose |
| Session storage | Redis | 8+ | Infrastructure provisioned for EPIC G; not used before authentication exists |
| Build orchestration | just | latest | Unified justfile for all tasks |

---

## 2. Data Model Approach

- **Family-centric** model (classic GEDCOM style): Persons exist independently; Families link spouses and children.
- Not person-centric (GEDCOM-X style) — deferred to post-MVP consideration.
- Recursive CTE over the family links (`AncestryRepo`) for ancestor/descendant traversal — no closure table.

For full entity definitions, see [Data Model](data-model.md).

---

## 3. Key Design Decisions

| Decision | Choice | Rationale |
|---|---|---|
| Primary keys | UUID v7 | Time-ordered, no collision across web/desktop, no sequential ID leakage |
| Pagination | Cursor-based (Relay-style) | Handles concurrent modifications, natural fit for GraphQL connections |
| Deletion | Soft delete (`deleted_at`) | Undo capability, audit trail, filtered out by default |
| Desktop architecture | Single binary | Embeds Axum on localhost + SQLite + Dioxus WebView |
| Authentication | Deferred to EPIC G | No auth in MVP; backend access remains local/private and is never directly public |

---

## 4. Backend Architecture

- Rust core crate (`oxidgene-core`) with domain types, shared across all binaries.
- SeaORM entities crate (`oxidgene-db`) with migrations.
- API crate (`oxidgene-api`) with Axum handlers (REST) and async-graphql resolvers.
- GEDCOM crate (`oxidgene-gedcom`) wrapping `ged_io` with domain conversion logic, and `geneweb` for reading GeneWeb `.gw` files — the `.gw` reader emits an `ged_io` model, so both formats share one conversion into the domain.
- Denormalized read projections materialized in the database and maintained by
    `oxidgene-api::profile`. See [Data Model §4](data-model.md).
- Application crates provide the web server, browser frontend, and desktop
    application. There is no CLI.

API endpoints are documented in [API Contract](api.md).

---

## 5. Frontend Architecture

- Dioxus components crate (`oxidgene-ui`).
- Shared between web and desktop targets.
- Communicates with the backend via REST/GraphQL.
- In the browser, `oxidgene-web` compiles the shared UI to WebAssembly and
    injects the separately deployed Axum API URL.
- On desktop: points to `http://127.0.0.1:<port>` served by the embedded Axum server.
- The embedded and standalone servers bind to loopback by default. Before
    authentication and authorization ship, no deployment exposes Axum directly
    to an untrusted network; a container that listens on its private network is
    reachable only through a trusted same-origin gateway.
- The UI fetches backend media through `ApiClient` and gives the rendering
    engine only local `data:`/`blob:` resources. API URLs never become links,
    image sources, redirects, or new-window destinations.
- Geneanet import separates control and data planes: REST/GraphQL carry import
    metadata and local paths, while the login WebView and embedded backend
    exchange media bytes through their shared filesystem. No Geneanet media byte
    payload is uploaded through either API surface.

UI specifications:
- [Common UI](ui-common.md) — shared layout, tokens, and components
- [Homepage](ui-home.md) — tree dashboard
- [Genealogy Tree](ui-genealogy-tree.md) — pedigree canvas
- [Person Edit Modal](ui-person-edit-modal.md) — edit forms
- [Settings](ui-settings.md) — tree configuration

---

## 6. Asynchronous Processing — Post-MVP (EPIC H)

- Message queue technology remains undecided; Redis provisioned for sessions is
    not implicitly the processing queue.
- `document-queue` orchestration service.
- Rust workers (scalable).
- Resumable uploads and restart-safe import jobs.

---

## 7. Build & Testing

- Unified `justfile` for build, test, lint, format, migration, and deployment tasks.
- `just dev-web` runs the Axum backend and `dx serve` browser frontend together;
    the frontend hot-reloads. `just dev-web-watch` also restarts the backend via
    `cargo-watch`. PostgreSQL can be started separately with `just dev-db-up`.
- Unit and integration tests across the workspace. End-to-end UI coverage is a
    remaining quality goal where the roadmap names it.
- CI/CD pipelines (GitHub Actions).

---

## 8. Deployment

### 8.1 Web Deployment

- The host development workflow runs the backend on port 8080 and the Dioxus
    frontend on port 8081, with PostgreSQL optionally provided by Docker Compose.
- The standalone server selects durable media storage with
    `OXIDGENE_MEDIA_BACKEND`: `filesystem` is the local default and `s3` is used
    by the development Compose stack and stateless web deployments. PostgreSQL
    stores application data and S3 stores media bytes, so the web pod requires no
    persistent volume.
- Large GEDCOM, GEDZIP, and GeneWeb uploads use the pod's system temporary
    directory only while a job is running. Those files are deleted after the job
    and swept at startup; they are disposable scratch data, not durable state.
    Media extracted from GEDZIP is persisted through the selected `MediaStore`.
- Development Compose provisions the static frontend, Axum, PostgreSQL,
    RustFS, and Redis and publishes their ports on host loopback only. Redis is
    reserved for EPIC G user sessions and is not a read-projection cache.
    Direct public backend deployment is blocked until EPIC G authentication and
    per-tree authorization are enforced across every transport and media read.
- The Helm chart keeps the OxidGene frontend and backend PVC-free. For the
    database, it either consumes an existing PostgreSQL URL, creates a
    CloudNativePG `Cluster`, or uses ephemeral SQLite in the backend pod; the
    official CloudNativePG operator subchart is optional so an administrator can
    retain a separate cluster-wide operator lifecycle. Redis can remain disabled,
    use an external service, or be provisioned as a persistent Redis 8 instance
    through an optional OpsTree Redis Operator subchart; its URL is reserved for
    EPIC G sessions. For media, the chart either uses an ephemeral backend-local
    filesystem, consumes an existing S3-compatible bucket, or creates a RustFS
    `Tenant`, bucket, policy, and application user through RustFS Operator 0.0.6+.
    The local SQLite and filesystem modes force a single backend replica and lose
    their data when its pod is recreated. The RustFS operator is installed
    separately because its CRDs and cluster-wide RBAC have an independent
    lifecycle. Only operator-managed infrastructure owns persistent volumes.
- The release images, development Compose stack, and Kubernetes deliverables
    are tracked in [Roadmap §5](roadmap.md).

### 8.2 Desktop Distribution

- Single binary per platform (Windows, Linux, macOS).
- Built via `cargo build --release` with appropriate target.
- No external runtime dependencies (SQLite embedded, WebView from system).
- Offline place databases, when installed, live in the application data
    directory and are managed from [Settings](ui-settings.md). See
    [Common UI §4.4](ui-common.md).
- Release artifacts and their platform verification are tracked in
    [Roadmap §5](roadmap.md).

---

## 9. Project Structure

### 9.1 Cargo Workspace Layout

```
oxidgene/
├── Cargo.toml              # Workspace root
├── justfile                # Build orchestration
├── README.md               # Global README
├── docs/
│   ├── specifications/     # This directory
│   └── assets/             # Logos in other assets
├── crates/
│   ├── oxidgene-core/      # Domain types, enums, error types
│   ├── oxidgene-db/        # SeaORM entities + migrations
│   ├── oxidgene-api/       # Axum handlers + GraphQL resolvers
│   ├── oxidgene-gedcom/    # GEDCOM import/export + GeneWeb .gw import
│   ├── oxidgene-geneanet/  # Geneanet person↔photo recovery (join, key, archives)
│   └── oxidgene-ui/        # Dioxus components (shared web/desktop)
├── apps/
│   ├── oxidgene-server/    # Web backend binary
│   ├── oxidgene-web/       # Browser frontend (Dioxus/WASM)
│   ├── oxidgene-desktop/   # Desktop binary (Axum + SQLite + Dioxus WebView)
└── docker/                 # Docker files
```

### 9.2 Crate Dependency Graph

```
oxidgene-core (no internal deps)
    ↑
oxidgene-db (depends on: oxidgene-core)
    ↑
oxidgene-gedcom (depends on: oxidgene-core)
oxidgene-geneanet (no internal deps)
    ↑
oxidgene-api (depends on: oxidgene-core, oxidgene-db, oxidgene-gedcom, oxidgene-geneanet)
    ↑
oxidgene-server (depends on: oxidgene-api, oxidgene-db)
oxidgene-web (depends on: oxidgene-ui)
oxidgene-desktop (depends on: oxidgene-api, oxidgene-db, oxidgene-ui, oxidgene-geneanet)

oxidgene-ui (depends on: oxidgene-core)
```

**`oxidgene-ui` stays platform-free.** It is compiled for wasm as well as for
the desktop, so it depends on neither `dioxus-desktop` nor `oxidgene-geneanet`.
Where it needs something only the desktop can do — the
[Geneanet login window](ui-import.md) — it declares a trait
(`oxidgene_ui::geneanet::GeneanetCollector`) that `oxidgene-desktop` implements
and injects as context. The web build simply finds none and renders the
explanation instead of the control.

The workspace keeps libraries under `crates/` and application entry points
under `apps/`. A former CLI was removed after its workflows moved into the
desktop application. The initial migration holds the baseline schema; every
subsequent schema change adds a migration and existing migrations are not
squashed.
