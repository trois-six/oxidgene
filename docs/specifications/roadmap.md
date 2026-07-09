---
type: "Roadmap Specification"
title: "Roadmap — EPICs, Sprints & Milestones"
description: "Delivery roadmap with EPICs, sprint milestones, and completion status for OxidGene."
tags: [oxidgene, specification, roadmap, planning]
timestamp: 2026-06-17T00:00:00Z
---


# Roadmap — EPICs, Sprints & Milestones

> Part of the [OxidGene Specifications](index.md).
> See also: [Architecture](architecture.md) · [General](general.md) (MVP scope)

---

## EPIC A — Technical Foundation ✅

### Sprint A.1 — Project Scaffolding ✅

- [x] Initialize Cargo workspace with all crate stubs.
- [x] Configure workspace-level dependencies in root `Cargo.toml`.
- [x] Create `oxidgene-core` crate with all domain types and enums.
- [x] Set up `justfile` with basic commands (build, test, fmt, clippy).
- [x] Create `.gitignore`, `README.md`.
- [x] Set up GitHub Actions CI pipeline (build + test + clippy + fmt).

### Sprint A.2 — Database Schema & Migrations ✅

- [x] Define SeaORM entities for all 13 tables in `oxidgene-db`. → see [Data Model](data-model.md)
- [x] Write database migrations (create tables, indexes, foreign keys).
- [x] Implement migration runner (up/down).
- [x] Test migrations against both PostgreSQL and SQLite.

### Sprint A.3 — Repository Layer ✅

- [x] Implement repository traits in `oxidgene-db` for CRUD operations.
- [x] Implement soft-delete filtering.
- [x] Implement cursor-based pagination helpers. → see [API Contract](api.md) (pagination)
- [x] Unit tests for all repositories.

### Sprint A.4 — REST API Skeleton ✅

- [x] Set up Axum router in `oxidgene-api`. → see [API Contract](api.md) (REST)
- [x] Implement REST handlers for Trees (full CRUD).
- [x] Implement REST handlers for Persons (full CRUD + names).
- [x] Implement REST handlers for Families (full CRUD + spouses + children).
- [x] JSON serialization, error handling, validation.
- [x] Integration tests against a test database.

### Sprint A.5 — REST API (continued) ✅

- [x] Implement REST handlers for Events, Places, Sources, Citations.
- [x] Implement REST handlers for Media (upload/download), MediaLinks, Notes.
- [x] Implement ancestor/descendant endpoints using closure table.
- [x] Complete integration test coverage.

### Sprint A.6 — GraphQL API ✅

- [x] Set up async-graphql schema in `oxidgene-api`. → see [API Contract](api.md) (GraphQL)
- [x] Implement all queries with connection types (cursor pagination).
- [x] Implement all mutations.
- [x] GraphQL playground / introspection endpoint.
- [x] Integration tests for GraphQL.

### Sprint A.7 — Web Server Binary ✅

- [x] Create `oxidgene-server` binary. → see [Architecture](architecture.md) (deployment)
- [x] Configuration loading (environment variables, config file).
- [x] Database connection pool setup (PostgreSQL).
- [x] Health check endpoint (`/healthz`).
- [x] Graceful shutdown.
- [x] Docker build for server + Docker Compose for local dev (server + PostgreSQL).

### Sprint A.8 — Desktop Binary Skeleton ✅

- [x] Create `oxidgene-desktop` binary. → see [Architecture](architecture.md) (desktop)
- [x] Embed Axum server on localhost with SQLite.
- [x] Open Dioxus WebView pointing to the local server.
- [x] Verify all API endpoints work with SQLite backend.

---

## EPIC B — GEDCOM Engine ✅

- [x] Implement `oxidgene-gedcom` crate wrapping `ged_io`.
- [x] GEDCOM → domain model import (persons, families, events, sources, media, places, notes).
- [x] Domain model → GEDCOM export.
- [x] Round-trip tests (import → export → import, verify equivalence).
- [x] Error and warning collection during import.
- [x] Streaming import for large files.
- [x] Wire up import/export REST and GraphQL endpoints. → see [API Contract](api.md) (GEDCOM)

---

## EPIC C — Tree Editing (Frontend) ✅

- [x] Set up `oxidgene-ui` crate with Dioxus. → see [Architecture](architecture.md) (frontend)
- [x] Implement frontend routing (tree list, tree detail, person detail).
- [x] Visual tree component (ancestor/descendant chart). → see [Tree View spec](ui-genealogy-tree.md)
- [x] Person detail sheet (names, events, sources, media, notes).
- [x] Inline editing of persons, families, events. → see [Person Edit Modal spec](ui-person-edit-modal.md)
- [x] Family creation and member linking UI.
- [x] GEDCOM import/export UI (file upload, download).
- [x] Frontend integration with REST/GraphQL API.

---

## EPIC D — UX, Languages, Performance ✅

- [x] Theme system (CSS-based, switchable at runtime). → see [Settings spec](ui-settings.md)
- [x] Implement at least 2 themes (default + one genealogy-platform-inspired theme). → see [Design Tokens](ui-design-tokens.md) §10
- [x] Internationalization (i18n) with runtime language switching. → `crates/oxidgene-ui/src/i18n/`
- [x] At least 2 languages (English + French). → `i18n/en.rs`, `i18n/fr.rs`
- [x] Client-side caching of API responses. → `ApiClient` in-memory cache, 30s TTL, invalidated on mutations.
- [x] Lazy loading of tree branches in the visualization. → Parallel JoinSet fetches for names & family members.
- [x] Performance optimization pass (bundle size, render performance). → Parallel fetches; cache avoids redundant round-trips.

---

## EPIC E — Server-Side Caching ✅

> See [Caching specification](caching.md) for the full architecture.

### Sprint E.1 — Cache Foundation ✅

- [x] Create `oxidgene-cache` crate with `CacheStore` trait. → see [Caching](caching.md)
- [x] Implement cache type structs (`CachedPerson`, `CachedPedigree`, `CachedSearchIndex`, sub-types).
- [x] Implement `MemoryCacheStore` (DashMap-based, no persistence yet).
- [x] Implement `CacheBuilder` — build `CachedPerson` from DB data.
- [x] Implement `CacheService` with `rebuild_person`, `rebuild_tree_full`.
- [x] Unit tests for cache builder and service.

### Sprint E.2 — Person Cache & API Integration ✅

- [x] Add `CacheService` and `CacheStore` to `AppState`.
- [x] Implement `GET /cache/persons/{id}` and `GET /cache/persons?ids=...` REST endpoints. → see [API Contract](api.md) (Cache)
- [x] Implement `cachedPerson` and `cachedPersons` GraphQL queries.
- [x] Hook all mutation handlers to trigger synchronous cache invalidation.
- [x] Update `person_detail.rs` to use cached endpoint.
- [x] Update `person_form.rs` and `union_form.rs` to use cached endpoint.

### Sprint E.3 — Pedigree Cache ✅

- [x] Implement pedigree cache builder from PersonAncestry + PersonCache.
- [x] Implement `GET /cache/pedigree/{root_id}` and `PATCH .../expand` REST endpoints.
- [x] Implement `pedigree` query and `expandPedigree` mutation in GraphQL.
- [x] Implement LRU memory budget for pedigree caches (configurable per deployment).
- [x] Update `pedigree_chart.rs` to consume pedigree cache instead of snapshot.
- [x] Update `tree_detail.rs` page orchestration.

### Sprint E.4 — Search Index & GEDCOM Integration ✅

- [x] Implement `CachedSearchIndex` builder with accent-folding and normalization.
- [x] Implement `GET /cache/search?q=...` REST endpoint and `searchPersons` GraphQL query.
- [x] Hook GEDCOM import to trigger eager background cache build.
- [x] Update search components to use server-side search.
- [x] Remove `TreeSnapshot` endpoint and client-side `ResponseCache`.
- [x] Implement `POST /cache/rebuild` REST endpoint and `rebuildTreeCache` GraphQL mutation.

### Sprint E.5 — Redis Backend & Desktop Persistence ✅

- [x] Implement `RedisCacheStore` (MessagePack serialization, `MGET` batch reads).
- [x] Add Redis container to Docker Compose for web deployment.
- [x] Implement disk persistence for `MemoryCacheStore` (bincode, serialize on exit, load on startup).
- [x] Auto-detect Redis (web) vs. memory (desktop) via configuration.
- [x] Cache staleness detection and recovery for desktop.

### Sprint E.6 — Desktop Cache Simplification (SQLite-native) ✅

> Rationale: the in-memory PersonCache and SearchIndex are redundant on desktop where SQLite is local.
> PedigreeCache stays (layout is parameter-dependent: root × depth × structure).

- [x] Replace `CachedSearchIndex` with a SQLite **FTS5 virtual table** (`person_search_fts`).
  - Add FTS5 migration (name tokens, birth year, death year; plain indexed table on PostgreSQL).
  - Populate on GEDCOM import and person/name mutations (bounded upserts via `PersonSearchRepo`).
  - Remove `GET /cache/search`. Handled by the normal search path: `GET /persons/search?q=...`.
- [x] Evaluate and remove `PersonCache` from `MemoryCacheStore` — removed; persons are built on
  demand with targeted SQLite queries (`caches_persons()` store flag; Redis keeps PersonCache).
  Disk persistence reduced to pedigrees only (cache schema v2).
- [x] Update `caching.md` to document the SQLite-native path vs. Redis path.
- [x] Performance regression test: search and person-load times verified ≤ current with FTS5
  (`service_e6_test.rs`: person load < 100 ms asserted; measured ~1 ms at 2K persons).
- [x] Performance benchmarks on large GEDCOM-scale trees (`bench_large_tree_20k`, run with
  `cargo test -p oxidgene-cache -- --ignored`): 20K persons → person load ~9 ms, search ~10 ms,
  full rebuild ~0.7 s (release).

---

### Sprint E.7 — Refinement

> Rationale: improve the UX to the definitive form
> We must ensure that the UXP is good.

- [ ] Medias management
- [ ] Remove the hack (https://github.com/trois-six/oxidgene/commit/6364b23338b67743805b884e690e8e62e2010e53) for UTF-8 strings once ged_io will be patched (https://github.com/ge3224/ged_io/pull/68)
- [ ] Create a CLI
- [ ] Research by SOSA ID
- [ ] Create a page to manage locations, sources, occupations
- [ ] Create a page with statistics about the tree
- [ ] Create a page to render the tree differently for printing
- [ ] Reconsolidate DB migrations into initial migration

---

## EPIC F — Security & Deployment

- [ ] Authentication system (JWT or session-based).
- [ ] User registration and login.
- [ ] Per-tree access control (guest, read-only, editor). → see [General](general.md) (user roles)
- [ ] Contemporary individual masking for guests. → see [Settings spec](ui-settings.md) (privacy section)
- [ ] Audit logging.
- [ ] Kubernetes manifests (deployment, service, ingress).
- [ ] FluxCD GitOps configuration.
- [ ] Liveness/readiness probes.
- [ ] Production PostgreSQL configuration (connection pooling, backups).
- [ ] TLS termination + HTTP/2 for the web server (ingress or reverse proxy). `axum::serve` currently serves plain HTTP/1.1, so browsers cap concurrent requests per origin (~6) instead of multiplexing over one connection — pages like person detail that fire many parallel API calls would benefit from h2.

---

## EPIC G — Asynchronous Pipeline (Post-MVP)

- [ ] Platform-specific build and smoke test (Linux, macOS, Windows).
- [ ] Performance testing with 100K-person trees.
- [ ] Message queue integration (Redis/RabbitMQ/NATS).
- [ ] `document-queue` orchestration service.
- [ ] Chunked media uploads.
- [ ] Async GEDCOM processing for large files.
- [ ] Rust worker pool for background tasks.
- [ ] Notification system (processing status).
- [ ] Temporary and persistent object storage.

