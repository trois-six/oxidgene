---
type: "Roadmap Specification"
title: "Roadmap — EPICs, Sprints & Milestones"
description: "Delivery roadmap with EPICs, sprint milestones, and completion status for OxidGene."
tags: [oxidgene, specification, roadmap, planning]
timestamp: 2026-07-19T00:00:00Z
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
- [x] Performance regression test: search and person-load times verified <= current with FTS5
  (`service_e6_test.rs`: person load < 100 ms asserted; measured ~1 ms at 2K persons).
- [x] Performance benchmarks on large GEDCOM-scale trees (`bench_large_tree_20k`, run with
  `cargo test -p oxidgene-cache -- --ignored`): 20K persons → person load ~9 ms, search ~10 ms,
  full rebuild ~0.7 s (release).

---

### Sprint E.7 — Refinement & Search Completion (✅ Jul 2026)

> Rationale: improve the UX to the definitive form. All items now completed.

**Completed:**
- [x] Reconsolidate DB migrations into initial migration — all schema in `m20250101_000001_initial.rs`; future changes add separate files (no squashing).
- [x] Search results grid view: one mini-pedigree card per result (self + parents + grandparents), 20 results/page.
- [x] Dictionary page (V1): read-only index of family names, sources, places, occupations with usage counts.
- [x] SOSA number search: numeric-only family-name queries resolve via `GET /persons/sosa/{number}`.
- [x] GEDCOM round-trip fidelity: EventType extended with 12 individual-attribute variants, ADOP as individual event, EventWitness join table, UTF-8 export.

**Deferred to Sprint F.1 (Media Management):**
- Media management (binary upload/download, thumbnails, multi-page docs, vignettes)

**Post-livraison (E.7 improvements):**
- [x] Sources smart drill-down: intelligent letter/prefix navigation (> 250 results → drill-down; <= 250 → display all), with server-side compression that auto-skips forced single-choice levels (see [ui-dictionary.md §8.10](ui-dictionary.md)) — a branch is only ever shown to the user when there is a genuine choice.
- [x] Multi-profession `OCCU` handling: import splits a Geneanet-style multi-profession `OCCU` value (`"Presales, Trainer"`) on `,` `;` `/` `|` into one case-normalized `Occupation` event per profession; export gained an opt-in `merge_occupations` option to collapse them back into a single comma-separated `OCCU` tag for importers (Geneanet) that only support one profession field (see [API Contract](api.md) §3, [ui-settings.md](ui-settings.md) §18).

**Future (lower priority):**
- Create a CLI tool for import/export
- Settings completion: manage locations, sources, occupations
- Statistics page (Post-MVP)
- Print layout (Post-MVP)

---

### Sprint E.8 — Dictionary V2: Genealogical Descent View (Planned)

Rationale: enhance the flat dictionary index with nested descent trees showing surname relationships.

- [ ] Database layer: group surname carriers into disjoint descent trees
- [ ] API: `GET /dictionary/family-names/{value}/tree` endpoint
- [ ] UI: recursive descent-tree component with generation indentation and SOSA badges when clicking on a last name in the dictionnary
- [ ] Resolve design questions: non-surname-carrying children handling, toggle vs. replacement

---

## EPIC F — Media Management (New, Sprints F.1–F.4)

Comprehensive media workflow: upload, storage, thumbnails, multi-page documents, image cropping (vignettes), event linking.

### Sprint F.1 — Media Storage & Serving

- [ ] Media storage architecture (filesystem vs. S3 decision + implementation)
- [ ] `POST /media/upload` endpoint (multipart form, file validation, size limits)
- [ ] `GET /media/{id}` download endpoint (binary, caching headers)
- [ ] Thumbnail generation on upload
- [ ] Multi-page document parsing (PDF, TIFF)
- [ ] Database schema: Media, MediaLink, Vignette entities
- [ ] Test against both PostgreSQL and SQLite

### Sprint F.2 — Media UI & Image Cropper

- [ ] MediaInput component (file picker, preview)
- [ ] ImageCropper component (interactive crop, save vignette)
- [ ] MediaGallery component (thumbnail grid, multi-page carousel)
- [ ] VignetteLinker (bind cropped region to event)
- [ ] Integration with Person Edit Modal (V2 from Sprint 1)

### Sprint F.3 — Event Linking & Desktop Support

- [ ] Event evidence linking (show media supporting event)
- [ ] Vignette assignment (use cropped image as event illustration)
- [ ] Desktop file picker (native dialog)
- [ ] SQLite blob vs. filesystem decision for desktop

### Sprint F.4 — Performance & Polish

- [ ] Thumbnail caching
- [ ] Performance testing (large media libraries)
- [ ] Error handling (format validation, upload limits)
- [ ] Full test coverage

---

## EPIC G — Security & Deployment (formerly EPIC F)

- [ ] Authentication system (JWT or session-based).
- [ ] User registration and login.
- [ ] Per-tree access control (guest, read-only, editor).
- [ ] Contemporary individual masking for guests.
- [ ] Audit logging.
- [ ] Kubernetes manifests (deployment, service, ingress).
- [ ] FluxCD GitOps configuration.
- [ ] Liveness/readiness probes.
- [ ] Production PostgreSQL configuration.
- [ ] TLS termination + HTTP/2 for the web server.

---

## EPIC H — Asynchronous Pipeline (Post-MVP, formerly EPIC G)

- [ ] Platform-specific build and smoke test (Linux, macOS, Windows).
- [ ] Performance testing with 100K-person trees.
- [ ] Message queue integration (Redis/RabbitMQ/NATS).
- [ ] `document-queue` orchestration service.
- [ ] Chunked media uploads.
- [ ] Async GEDCOM processing for large files.
- [ ] Rust worker pool for background tasks.
- [ ] Notification system (processing status).
- [ ] Object storage (temporary and persistent).
