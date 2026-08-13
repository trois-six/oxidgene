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
- [x] Implement ancestor/descendant endpoints (closure table; replaced by a recursive CTE in Aug 2026).
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

### GeneWeb `.gw` import ✅ (Aug 2026)

> Rationale: GeneWeb is the genealogy software behind a large share of French online
> genealogies, and its `.gw` export carries more than the GEDCOM its own `gwb2ged` produces.
> The [`geneweb`](https://github.com/trois-six/rust-geneweb) crate converts `.gw` into the same
> `ged_io` model this EPIC already consumes, so importing the format costs a reader, not a
> second domain mapping.

- [x] Add the `geneweb` crate as a workspace dependency.
- [x] Split `import_gedcom` into a parse step and a reusable
  `import_gedcom_data(&GedcomData, tree_id)`, so both formats share one domain mapping.
- [x] Add `oxidgene_gedcom::geneweb::import_geneweb(bytes, origin_file, tree_id)` — lenient
  reading, `.gw` parse errors surfaced as import warnings rather than aborting the file.
- [x] Carry raw bytes end to end: `POST /trees/{id}/geneweb/import` takes
  `application/octet-stream` + `?filename=`, GraphQL `importGeneweb` takes base64. `.gw` is
  ISO-8859-1 unless a file opts into UTF-8 mid-stream, so decoding it upstream of the reader
  would mangle accented names.
- [x] Extract `persist_import_result` in the service layer so every import format shares one
  FK-ordered, transactional persistence path; rename the summary type to be format-neutral
  (`ImportGedcomResponse` → `ImportResponse`, `GqlImportGedcomResult` → `GqlImportResult`).
- [x] UI: one Import button, file dialog accepting `.ged` and `.gw`, dispatch by extension.
- [x] Tests: REST (incl. an ISO-8859-1 fidelity regression test), GraphQL, and unit coverage.
  Verified against a real 10K-person base (10,254 persons / 2,507 families / 23,201 events).

**Not covered** (the `geneweb` crate reads only): writing `.gw`, the binary `.gwb` database,
and GeneWeb-only concepts with no GEDCOM counterpart (per-person access rights, wizard notes,
wiki pages) — they survive conversion as `_GW…` tags, which the GEDCOM importer does not model.

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

**Post-delivery (Aug 2026):**
- [x] Note bodies render as sanitized HTML (`ammonia` allowlist, applied on write in `oxidgene_db::html`) instead of escaped text, because the imported formats put markup in them.
- [x] Line breaks in note bodies canonicalized to `\n` on write and restored to `<br>` at display, so the same note reads identically whether it arrived as GEDCOM `CONT` lines, as GeneWeb `<br/>` + newline, or typed into the note textarea.

---

## EPIC D — UX, Languages, Performance ✅

- [x] Theme system (CSS-based, switchable at runtime). → see [Settings spec](ui-settings.md)
- [x] Implement at least 2 themes (default + one genealogy-platform-inspired theme). → see [Design Tokens](ui-design-tokens.md) §10
- [x] Internationalization (i18n) with runtime language switching. → `crates/oxidgene-ui/src/i18n/`
- [x] At least 2 languages (English + French). → `i18n/en.rs`, `i18n/fr.rs`
- [x] Client-side caching of API responses. → `ApiClient` in-memory cache, 30s TTL, invalidated on mutations.
- [x] Lazy loading of tree branches in the visualization. → Parallel JoinSet fetches for names & family members.
- [x] Performance optimization pass (bundle size, render performance). → Parallel fetches; cache avoids redundant round-trips.

**Post-delivery (Aug 2026):**
- [x] First-run defaults follow the OS: language picked from the ordered `navigator.languages` list (first translated entry wins), theme from `prefers-color-scheme`. English and light theme when detection fails. → `i18n::Language::from_preferences`, `layout::use_init_theme`

---

## EPIC E — Read Projections & Search ✅

> See [Read Projections specification](read-projections.md) for the full architecture.

### Sprint E.1 — Cache Foundation ✅

- [x] Create `oxidgene-cache` crate with `CacheStore` trait. *(removed in E.7 — see [Read Projections](read-projections.md))*
- [x] Implement cache type structs (`CachedPerson`, `CachedPedigree`, `CachedSearchIndex`, sub-types).
- [x] Implement `MemoryCacheStore` (DashMap-based, no persistence yet).
- [x] Implement `CacheBuilder` — build `CachedPerson` from DB data.
- [x] Implement `CacheService` with `rebuild_person`, `rebuild_tree_full`.
- [x] Unit tests for cache builder and service.

### Sprint E.2 — Person Cache & API Integration ✅

- [x] Add `CacheService` and `CacheStore` to `AppState`.
- [x] Implement `GET /cache/persons/{id}` and `GET /cache/persons?ids=...` REST endpoints. → see [API Contract](api.md) (Cache)
- [x] Implement `cachedPerson` and `cachedPersons` GraphQL queries. *(renamed `personProfile` / `personProfiles` in E.9)*
- [x] Hook all mutation handlers to trigger synchronous cache invalidation.
- [x] Update `person_detail.rs` to use cached endpoint.
- [x] Update `person_form.rs` and `union_form.rs` to use cached endpoint.

### Sprint E.3 — Pedigree Cache ✅

- [x] Implement pedigree cache builder from PersonAncestry + PersonCache (both since removed).
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
- [x] Implement `POST /cache/rebuild` REST endpoint and `rebuildTreeCache` GraphQL mutation. *(renamed in E.9)*

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
- [x] Update the caching spec to document the SQLite-native path vs. Redis path.
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

**Post-delivery (E.7 improvements):**
- [x] Sources smart drill-down: intelligent letter/prefix navigation (> 250 results → drill-down; <= 250 → display all), with server-side compression that auto-skips forced single-choice levels (see [ui-dictionary.md §8.10](ui-dictionary.md)) — a branch is only ever shown to the user when there is a genuine choice.
- [x] Multi-profession `OCCU` handling: import splits a Geneanet-style multi-profession `OCCU` value (`"Presales, Trainer"`) on `,` (each part trimmed) into one case-normalized `Occupation` event per profession; export gained an opt-in `merge_occupations` option to collapse them back into a single comma-separated `OCCU` tag for importers (Geneanet) that only support one profession field (see [API Contract](api.md) §3, [ui-settings.md](ui-settings.md) §18).
- [x] Multi-alias `SURN` handling: import previously trusted `SURN` over `NAME` for the primary surname, silently dropping the real primary name when a Geneanet-style multi-alias `SURN` value (`"LE NADEN,NADAM"`) was present. Fixed to prefer `NAME`'s surname as primary and split `SURN`'s extra parts (on `,`) into `AlsoKnownAs` `PersonName` rows; export gained a matching opt-in `merge_names` option to collapse non-primary names back into the primary `NAME`'s comma-separated `SURN` tag for importers (Geneanet) that only read the first `NAME` structure (see [API Contract](api.md) §3, [ui-settings.md](ui-settings.md) §18).
- [x] Reference content tooltips: new `oxidgene-api::reference` module + `GET /reference/{lang}/occupations|given-names?term=...` resolve free-text OCCU/GIVN values to short fiches (occupation sheets, given-name meanings), one gzip-compressed JSON file per language per data type, decompressed once into an in-memory table. Person profile page shows a hover tooltip over the occupation and the given name. Seeded with 5 occupations + 5 given names (fr/en); growing the data set is a follow-up content task (see [API Contract](api.md) §1 Reference Content).

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

### Sprint E.9 — Denormalization Replaces Caching ✅ (Jul 2026)

> Rationale: the read models the cache held are a function of one person plus their immediate
> relatives, cheap to rebuild, and never acceptably stale — that is denormalization, not caching.
> E.6 proved the point by moving search into the database; E.9 finishes the job.
> See [Read Projections](read-projections.md) for the full architecture.

- [x] Add the `person_denorm` table (JSON payload, FK cascade on person/tree), `PersonDenormRepo`
  and migration `m20260728_000001_person_denorm`.
- [x] Move the projection types (`CachedPerson`, `CachedPedigree`, `SearchEntry`, …) from
  `oxidgene-cache::types` to `oxidgene_core::projection`, so `oxidgene-ui` no longer drags
  `oxidgene-db` / `tokio` / `dashmap` into the WASM build.
- [x] Replace `CacheService` with `ProfileService` (`oxidgene-api/src/profile/`), carrying the
  builder and the affected-set algorithm over unchanged.
- [x] Assemble pedigrees per request from ancestry traversal ⋈ `person_denorm` — pedigree cache,
  LRU budget and pedigree invalidation all removed.
- [x] **Delete the `oxidgene-cache` crate** (~4,100 lines, three storage backends) and the
  `redis`, `dashmap`, `rmp-serde`, `bincode` dependencies.
- [x] Remove the desktop disk-cache lifecycle (cache dir, staleness check, load-on-start,
  persist-on-shutdown handshake).
- [x] Rename the REST routes off `/cache/*` → `/profiles*` and `/pedigree/*`, **and the matching
  GraphQL fields and types**, so the two surfaces stay symmetric: `cachedPerson` → `personProfile`,
  `rebuildTreeCache` → `rebuildTreeProfiles`, `invalidateTreeCache` → `dropTreeProfiles`,
  `GqlCached*` → `GqlPersonProfile`/`GqlProfile*`/`GqlPedigree`, field `cachedAt` → `builtAt`.
  `expandPedigree` gained an optional `otherDepth`.
- [x] Port the integration tests to `crates/oxidgene-api/tests/profile_service_test.rs`, adding
  coverage for the guarantees a cache could not offer: projections survive a service restart,
  a relative's projection is never left stale after a rename, and a rolled-back mutation leaves
  no projection behind.
- [x] Make the refresh **atomic with the mutation**: widen all 119 repo methods from
  `&DatabaseConnection` to `&impl ConnectionTrait` (SeaORM implements it for `DatabaseTransaction`
  too), and open/commit one transaction across the write and the projection refresh in the 35
  REST + GraphQL mutation handlers. Whole-tree rebuilds stay outside — idempotent bulk work.

**Known follow-ups:**
- [ ] Refresh projections embedding a `Place` name when that place is renamed (pre-existing gap).

---

### Sprint E.10 — Surname Particle Fix & Bulk Repair ✅ (Aug 2026)

> Rationale: `split_surname_particle`'s flat particle list treated a bare leading article ("Le",
> "La") the same as a preposition ("de", "van"), so a whole class of names — Breton/Norman "Le …"
> surnames chief among them — got a spurious particle on every import. Fixing detection does not
> repair a tree already imported wrong, so the dictionary also gained a bulk edit.

- [x] Split `PARTICLES` into `HEAD_PARTICLES` (prepositions, may open a particle run) and
  `TAIL_PARTICLES` (bare articles, count only immediately after a head particle) in
  `oxidgene-core::types::surname`, with the same two-tier rule for elided forms (`d'`/`l'`).
  A leading article ("Le …", "La …") no longer splits; "de la Cruz" / "van der Berg" /
  "de l'Étang" still do.
- [x] `DictionaryRepo::set_family_name_particle`: re-cuts every `person_name` row matching a given
  surname (particle included) at a new particle, empty meaning "no particle". Rejects a particle
  not at the head of the surname (would inject a word the tree never had) and skips rows already
  cut that way (repeat calls are a no-op).
- [x] `PATCH /trees/{id}/dictionary/family-names/particle` (REST) and `setFamilyNameParticle`
  (GraphQL), both transactional and triggering a full projection rebuild when anything changed —
  a surname reaches every projection embedding a display name, so the affected set is unbounded.
- [x] Dictionary Family Names tab: pencil icon per row opens a modal showing the person count and
  a live preview (particle / root / filing letter) before applying.
- [x] Root-first display when filing by root: `d'Aubigné` under A now reads `Aubigné (d')` instead
  of leaving the particle stranded at the row's front.

---

### Sprint E.11 — Date Entry & Display ✅ (Aug 2026)

> Rationale: `DateQualifier::FromAge` shipped as a `<select>` entry with no behaviour behind it,
> and the widget wrote Gregorian month abbreviations whatever calendar was selected — so a
> Republican date left as `2 FEB 14` under a `@#DFRENCH R@` escape, which no reader can take back.
> Dates were also displayed as their bare stored value, dropping the qualifier that gives them
> their meaning.

- [x] "From an age" is a real entry mode: it swaps the day/month/year triplet for an age and the
  year it was observed in, and `DateParts::resolved` collapses the pair into the `About
  <year − age>` it stands for. Never persisted as `FromAge` — no schema, GEDCOM or GeneWeb form
  can record "aged 14 in 2026" — so a stored row reads back as the `About` date it always meant.
- [x] One `format_date` / `format_event_date` renders every date the reader sees (person profile
  vitals, unions and event list, both edit modals' event rows, the pedigree events panel), so a
  date reads « vers 2012 » rather than "2012" and reads the same way everywhere. The editor's
  literal preview goes through it too, so preview and page cannot disagree.
- [x] Per-calendar month vocabularies (`VEND…COMP`, `TSH…ELL`, thirteenth month included) for both
  the canonical stored value and the localized display, replacing a hardcoded Gregorian table.
  Fixes both directions: a Republican date is now written `2 BRUM 14`, and an imported one keeps
  its month instead of dropping it.
- [x] BCE years, stored the GEDCOM way (`15 MAR 44 BCE`, not `-44`) so exports stay readable, and
  parsed back from `BCE` / `BC` / `B.C.` / a leading minus. Year range 9999 BCE – 2999, excluding
  year 0.
- [x] Input protection: a keystroke guard turns away non-digits (a leading minus is allowed in
  year fields), paste and IME are caught by digit-stripping, and validation rejects dates that
  never existed — 30 February, a thirteenth Gregorian month, a backwards `Between` range, an age
  past 130. Leap rules follow the calendar, so 29 Feb 1900 is valid Julian and invalid Gregorian.
  Out-of-range entries are kept and explained inline rather than silently blanked.

- [x] `date_sort` is derived by the API, not sent by the client. Normalising a Julian, Hebrew or
  Republican date onto the Gregorian calendar needs `ged_io`, which a WASM frontend cannot reach,
  so the frontend had been reading the month number as if it were Gregorian: a Republican
  `2 BRUM 14` sorted in year 14, and a thirteenth month produced no key at all.
  `oxidgene_gedcom::date::sort_key` exposes the conversion the import path already used, and
  `service::event_date` wraps it for both write surfaces — including a patch, where whichever of
  calendar/value the request leaves alone is read back off the stored event, the two being
  meaningless apart. The field is gone from both request shapes.
- [x] French Republican dates corrected by one day. `ged_io` converts from the *start* of the
  Republican day, and that calendar is anchored to Paris, so the instant falls 9m21s inside the
  previous Gregorian day and every Republican date came back a day early — its epoch,
  1 Vendémiaire An I, is 22 September 1792 and it answered the 21st. The shift is *measured*
  against that epoch rather than hardcoded, so it reads zero and stops applying if the conversion
  is ever fixed upstream, where a literal "+1 day" would start overshooting instead. Julian and
  Hebrew were checked against known dates and are correct; they are left alone.

---

## EPIC F — Media Management (New, Sprints F.1–F.4)

Comprehensive media workflow: upload, storage, thumbnails, multi-page documents, image cropping (vignettes), event linking.

**First consumer:** the [Geneanet import](ui-geneanet-import.md) is blocked on this sprint — it arrives with hundreds of real files, multi-page PDFs and photos shared between several people, which makes it a better shakedown of the storage design than manual upload. Its steps 1–4 can be built before F.1; only the write step depends on it.

### Sprint F.1 — Media Storage & Serving 🔄

- [x] Media storage architecture — a `MediaStore` trait with one `FsStore` implementation, content-addressed as `{tree_id}/{aa}/{bb}/{sha256}.{ext}` under `OXIDGENE_MEDIA_ROOT` (default: the platform user-data directory, `~/.local/share/oxidgene/media` on Linux). Keys are scoped per tree so a purge is one directory removal, with no reference counting and no chance of pulling a file out from under another tree. Uploading the same scan twice writes one file and two rows — what a census page documenting eight siblings needs.
- [ ] **S3 backend for the server deployment.** The trait seam is in place and is the only thing a second implementation has to satisfy; the implementation itself, its credential/region/bucket configuration and its error mapping are not written. The web server runs on `FsStore` today, which needs a persistent volume.
- [x] `POST /trees/{id}/media/upload` (multipart; type decided by magic bytes, not by the declared MIME or the extension; 64 MiB ceiling; the body limit is lifted on that one route)
- [x] `GET .../media/{id}/file` and `.../thumbnail` (binary, `Content-Type`, RFC 6266 `Content-Disposition` that survives an accented name, SHA-256 as a strong `ETag`, `304` on `If-None-Match`)
- [x] Thumbnail generation on upload (longest edge 400 px, EXIF orientation applied, alpha preserved as PNG, decode-bomb limit; PDFs get none — see below)
- [x] Multi-page document parsing — page counts for PDF (via `lopdf`) and TIFF (IFD-chain walk, classic and BigTIFF)
- [x] Database schema — `media` gains `storage_key`, `sha256`, `thumbnail_key`, `width`, `height`, `page_count`; new `vignette` table with REST + GraphQL CRUD and a crop-on-read image endpoint
- [x] Tested on SQLite (69 unit + 23 media integration + 28 GraphQL tests)
- [ ] **PostgreSQL.** The migration test runs against a real server when `OXIDGENE_TEST_DATABASE_URL` is set, but no server was available in the sprint and there is no container harness in the repo, so the PostgreSQL path is still unexercised — as it has been since E.9.

**Deliberately out of scope, and why**

- **PDF thumbnails.** Rasterising a page needs pdfium or mupdf, a C dependency on a project that ships a desktop binary for three platforms. `thumbnail_key` is null for PDFs and the thumbnail endpoint answers `404`, so the UI branches on a status code rather than on a format list.
- **Audio and video.** Serving them usefully means `Range` requests and streaming, which belongs with EPIC H's chunked uploads.
- **GEDZIP bundling.** `export_gedzip` still writes the GEDCOM alone; wiring the store into it now that bytes exist is a small follow-up.

**Two paths, on purpose.** `media.file_path` stays the GEDCOM `OBJE.FILE` value — the producer's own path, preserved verbatim so an export round-trips. `media.storage_key` is where OxidGene's copy lives, and is null for every GEDCOM-imported record until someone uploads the file; `POST .../media/upload` with a `media_id` is how that gap gets filled.

### Sprint F.2 — Media UI & Image Cropper 🔄

- [x] **MediaInput** (`components/media_input.rs`) — the upload cell that ends every gallery. Click for the platform file dialog, or drop files onto it; both land in the same loop. Uploads run **one at a time**: a user sending a folder of scans over a connection they do not control gets "3 of 12", which reads as progress, instead of twelve stalled requests finishing in an unpredictable order. One rejected file does not abandon the batch, and its message names the file — a folder containing a `.DS_Store` still delivers the other eleven.
- [x] **ImageCropper** (`components/image_cropper.rs`) — drag a rectangle on a scan, save it as a vignette. The whole component is about keeping **two coordinate systems apart**: the user drags in whatever pixels the image occupies on screen, a vignette is stored in the source image's own pixels, and the ratio comes from `media.width`/`media.height` (recorded at upload precisely so the frontend never decodes an image to find out how big it is) against the element's measured client rect. Crops already on the page are drawn while you draw the next one — without them the same entry gets cropped twice. Saving clears the draft but keeps the cropper open, since a register page is four crops in a row.
- [x] **MediaGallery** (`components/media_gallery.rs`) — thumbnail grid, ★ profile badge, hover controls, inline edit panel. One request per gallery, not one per tile: the `media-links?entity_type=…` endpoint returns the media alongside its link, because a tile cannot be drawn without the MIME type and whether a thumbnail exists. A missing `thumbnail_key` is the server saying it could not rasterise the file, so the tile draws a labelled icon rather than the broken image an `<img>` onto a 404 gives you. The edit panel opens **inline under the grid**, not as a second modal — the gallery already lives in one, and stacking leaves the user with two Cancel buttons.
- [x] **VignetteLinker** (`components/vignette_linker.rs`) — the crops on a media, each with the event it documents. Deliberately not part of the cropper: attribution is decided after the fact, looking at several crops at once, and forcing the choice while drawing turns "crop the page" into four interrupted tasks.
- [x] Integration with the **Person** and **Union** edit modals, and — read-only — with the **person profile page**. The profile shows the same grid with its controls withheld, so a reader who then clicks Edit finds the gallery they were just looking at; the section hides itself when the person has no files rather than leaving an empty frame on every profile in the tree.
- [x] Backend gaps F.1 left, filled here because the UI cannot have the features otherwise: `GET /media-links?entity_type=&entity_id=` (one entity's gallery, media included) and `PUT /media-links/{id}/profile` (the ★, which clears the person's others in the same statement so the tree never shows two). Both mirrored in GraphQL as `entityMedia` and `setProfileMediaLink`. Setting the flag rebuilds the person's projection, since the portrait is embedded in `person_denorm`.
- [ ] **Multi-page carousel.** A document's page count is known and shown on the tile, but there is no page-by-page viewer: `GET .../file` serves the whole PDF or TIFF, and rendering page 7 of one needs the rasteriser F.1 declined to take on. Cropping a document is refused for the same reason.
- [ ] **Date and place on a media.** The columns exist (`date_value`, `date_sort`, `place_id`) and [`ui-person-edit-modal.md` §10](ui-person-edit-modal.md) asks for them; the edit panel currently offers title and description only.
- [ ] Verified by compiling and by the API integration tests (32 in `media_test.rs`), **not by running the desktop app** — the gallery, the drag-to-crop interaction and the drop target have not been exercised by hand.

### Sprint F.3 — Event Linking & Desktop Support

- [ ] Event evidence linking (show media supporting event)
- [ ] Vignette assignment (use cropped image as event illustration)
- [ ] Desktop file picker (native dialog)

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
