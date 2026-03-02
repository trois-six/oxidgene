# Technical Architecture

> Part of the [OxidGene Specifications](README.md).
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
| ORM | SeaORM | 1.1+ | Async, supports PostgreSQL + SQLite |
| Web database | PostgreSQL | 16+ | Production web deployment |
| Desktop database | SQLite | 3.35+ | Embedded in desktop binary |
| GEDCOM | ged_io | 0.12+ | Read/write, GEDCOM 5.5.1 + 7.0, streaming |
| Build orchestration | just | latest | Unified justfile for all tasks |

---

## 2. Data Model Approach

- **Family-centric** model (classic GEDCOM style): Persons exist independently; Families link spouses and children.
- Not person-centric (GEDCOM-X style) — deferred to post-MVP consideration.
- Closure table (`PersonAncestry`) for optimized ancestor/descendant traversal.

For full entity definitions, see [Data Model](data-model.md).

---

## 3. Key Design Decisions

| Decision | Choice | Rationale |
|---|---|---|
| Primary keys | UUID v7 | Time-ordered, no collision across web/desktop, no sequential ID leakage |
| Pagination | Cursor-based (Relay-style) | Handles concurrent modifications, natural fit for GraphQL connections |
| Deletion | Soft delete (`deleted_at`) | Undo capability, audit trail, filtered out by default |
| Desktop architecture | Single binary | Embeds Axum on localhost + SQLite + Dioxus WebView |
| Authentication | Deferred to EPIC E | No auth in MVP; single-user desktop, open web for now |

---

## 4. Backend Architecture

- Rust core crate (`oxidgene-core`) with domain types, shared across all binaries.
- SeaORM entities crate (`oxidgene-db`) with migrations.
- API crate (`oxidgene-api`) with Axum handlers (REST) and async-graphql resolvers.
- GEDCOM crate (`oxidgene-gedcom`) wrapping `ged_io` with domain conversion logic.
- Separate binary crates for web server, desktop app, and CLI tool.

API endpoints are documented in [API Contract](api.md).

---

## 5. Frontend Architecture

- Dioxus components crate (`oxidgene-ui`).
- Shared between web and desktop targets.
- Communicates with the backend via REST/GraphQL.
- On desktop: points to `http://127.0.0.1:<port>` served by the embedded Axum server.

UI specifications:
- [Homepage](ui-home.md) — tree dashboard
- [Genealogy Tree](ui-genealogy-tree.md) — pedigree canvas
- [Person Edit Modal](ui-person-edit-modal.md) — edit forms
- [Settings](ui-settings.md) — tree configuration

---

## 6. Asynchronous Processing — Post-MVP (EPIC F)

- Message queue container (Redis/RabbitMQ/NATS).
- `document-queue` orchestration service.
- Rust workers (scalable).
- Temporary + persistent object storage.

---

## 7. Build & Testing

- Unified `justfile` for build, test, lint, format, migration, and deployment tasks.
- Full test suite: unit tests, integration tests, and end-to-end tests.
- CI/CD pipelines (GitHub Actions).
- Code coverage reporting.

---

## 8. Deployment

### 8.1 Web Deployment

- Docker Compose for local development.
- Kubernetes deployment for production (dev & prod).
- GitOps with FluxCD.
- Liveness/readiness probes on the Axum server.

### 8.2 Desktop Distribution

- Single binary per platform (Windows, Linux, macOS).
- Built via `cargo build --release` with appropriate target.
- No external runtime dependencies (SQLite embedded, WebView from system).
- Offline place databases (SQLite files per country) stored in the app data directory; downloaded on demand from [Settings](ui-settings.md) §10. See [PlaceInput](ui-shared-components.md) §5.1.

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
│   ├── oxidgene-gedcom/    # GEDCOM import/export (wraps ged_io)
│   └── oxidgene-ui/        # Dioxus components (shared web/desktop)
├── apps/
│   ├── oxidgene-server/    # Web backend binary
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
    ↑
oxidgene-api (depends on: oxidgene-core, oxidgene-db, oxidgene-gedcom)
    ↑
oxidgene-server (depends on: oxidgene-api, oxidgene-db)
oxidgene-desktop (depends on: oxidgene-api, oxidgene-db, oxidgene-ui)
oxidgene-cli (depends on: oxidgene-db, oxidgene-gedcom)

oxidgene-ui (depends on: oxidgene-core)
```
