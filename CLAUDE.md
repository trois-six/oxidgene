# OxidGene — Context for Claude

## Project

Multiplatform genealogy app, 100% Rust. Dioxus frontend (WASM + desktop), Axum backend (REST `/api/v1` + GraphQL `/graphql`), SeaORM (PostgreSQL web / SQLite desktop), GEDCOM via ged_io, GeneWeb `.gw` import via geneweb.

## Specs

All specifications live in `docs/specifications/` — always read the relevant spec before implementing a feature:

| Spec | What it covers |
|------|----------------|
| `README.md` | Index with cross-links to all specs |
| `general.md` | Vision, users, features, MVP scope |
| `architecture.md` | Tech stack, crate layout, build, deployment |
| `data-model.md` | All 14 entities, enums, ERD |
| `api.md` | Full REST + GraphQL contract |
| `roadmap.md` | EPICs A–G, sprint breakdown |
| `read-projections.md` | Denormalized read models in the DB: `person_denorm`, pedigree assembly, search, refresh |
| `ui-home.md` | Homepage / tree dashboard |
| `ui-genealogy-tree.md` | Pedigree canvas, cards, connectors, sidebar |
| `ui-person-edit-modal.md` | Person edit, couple edit, media, deletion |
| `ui-settings.md` | Tree settings, tools, export |

## Workspace

```
crates/
  oxidgene-core/    # Domain types, enums, errors, read projections — no internal deps
  oxidgene-db/      # SeaORM entities, migrations, repos
  oxidgene-gedcom/  # GEDCOM ↔ domain (wraps ged_io) + GeneWeb .gw → domain (wraps geneweb)
  oxidgene-geneanet/# Geneanet person↔photo recovery: join, key folding, archive indexing
  oxidgene-api/     # Axum REST + async-graphql + service + profile layer
  oxidgene-ui/      # Dioxus components + pages
apps/
  oxidgene-server/  # Web server binary
  oxidgene-desktop/ # Desktop binary (embeds Axum + SQLite + WebView)
```

Dependency flow: `core` ← `db` ← `api` ← `server`/`desktop`; `core` ← `gedcom` ← `api`; `geneanet` ← `api`/`desktop`; `core` ← `ui`.

**`oxidgene-geneanet` speaks no HTTP.** Every Geneanet request goes through the desktop app's login window — Cloudflare refuses direct clients outright — so the crate holds only the join, the key folding, the archive indexing, the perceptual hash and the scripts the window runs.

**`oxidgene-ui` must stay platform-free** — it compiles to wasm, so it depends on neither `dioxus-desktop` nor `oxidgene-geneanet`. Where it needs a desktop-only capability it declares a trait and the desktop binary injects an implementation as context (see `ui/src/geneanet.rs` ↔ `apps/oxidgene-desktop/src/geneanet.rs`).

**No cache tier.** Denormalized read models are materialized in the DB (`person_denorm`, `person_search_fts`) and refreshed on every mutation; pedigrees are assembled per request from `person_ancestry` ⋈ `person_denorm`. See `docs/specifications/read-projections.md`.

## Key design rules

- **UUIDs v7** for all PKs (time-ordered)
- **Cursor-based pagination** (Relay-style) on all list endpoints
- **Soft delete** (`deleted_at`) — filter out by default
- **`PersonAncestry` closure table** for O(1) ancestor/descendant traversal
- **No auth in MVP** (EPIC F, deferred)
- **Family-centric model**: Persons exist independently; Families link spouses + children

## Frontend (oxidgene-ui)

Dioxus. Components in `src/components/`, pages in `src/pages/`.

**CSS**: all styles embedded as `const &str` in `components/layout.rs` (`LAYOUT_STYLES`). Uses CSS vars (see `ui-home.md` §12 for design tokens). Dark theme by default. Fonts: Cinzel (headings) + Lato (body) via Google Fonts `@import`.

**Key files**:
- `layout.rs` — app shell, navbar, all shared CSS
- `import_modal.rs` — import modal: file tab (.ged/.gw) + the 5-step Geneanet flow
- `media_gallery.rs` / `media_input.rs` / `image_cropper.rs` / `vignette_linker.rs` — media grid, upload cell, drag-to-crop, crop↔event linking
- `pedigree_chart.rs` — vertical bidirectional pan/zoom tree canvas
- `tree_detail.rs` — page orchestrator: data fetching, topbar, modals, GEDCOM I/O
- `person_detail.rs` — full person profile page
- `person_form.rs` — person edit modal (civil status, birth, death, events, media)
- `union_form.rs` — couple edit modal (union events, children, both persons)
- `person_node.rs` — reusable person card component
- `home.rs` — tree dashboard with cards, create/delete
- `api.rs` — HTTP client (`ApiClient`) for all backend calls

**Dioxus 0.7 gotchas**:
- `use_signal` returns Copy types — closures capture by copy
- SVG in rsx!: use quoted attrs for camelCase — `"viewBox"`, `"strokeWidth"`, `"fillOpacity"`
- `EventHandler<T>` for component callbacks (e.g. `on_confirm: EventHandler<()>`)

## Backend (oxidgene-api)

- `rest/` — one handler file per resource (tree, person, family, event, place, source, citation, media, media_link, vignette, note, gedcom, geneweb, family_member)
- `graphql/` — query.rs, mutation.rs, types.rs, inputs.rs
- `service/` — business logic (gedcom + geneweb import services; persistence shared via `gedcom::persist_import_result`)
- `profile/` — read projections: `service.rs` (ProfileService), `builder.rs`, `invalidation.rs`
- `rest/dto.rs` — request/response DTOs
- `rest/state.rs` — AppState (DB connection + `Arc<ProfileService>`)
- `router.rs` — Axum router wiring

## Build commands

```bash
just build          # Build all
just test           # Run tests
just check          # fmt + clippy + test
just fmt            # Format
just clippy         # Lint
just server         # Run web server (dev)
just desktop        # Run desktop app (dev)
```

## Assets

Logo: `docs/assets/OxidGene.{png,svg}` — used in navbar and README.

## Current sprint

EPICs A–D complete; EPIC E complete through Sprint E.9 (E.8 dictionary descent view still planned). **EPIC F — Media Management** in progress: F.1–F.3 and the Geneanet import wizard (F.3b) shipped, F.4 (performance & polish) next.

Media storage lives in `crates/oxidgene-api/src/media/` — `store.rs` (the `MediaStore` trait and the content-addressed `FsStore`), `thumbnail.rs`, `pages.rs`. Files default to the platform user-data directory (`~/.local/share/oxidgene/media` on Linux); `OXIDGENE_MEDIA_ROOT` overrides. The UI side is `components/media_gallery.rs` (grid, tiles, inline edit panel), `media_input.rs` (upload cell), `image_cropper.rs` (drag-to-crop) and `vignette_linker.rs`, used by `person_form`, `union_form` and — read-only — `person_detail`.

A media is one of three things and every view tells them apart: **stored** (bytes in our store, thumbnail, croppable), **remote** (`file_path` is an http(s) URL we record and never fetch), **unheld** (a GEDCOM record naming a file nobody uploaded). A **multi-page document** is a `media` with `is_document` whose pages are `media` rows carrying `parent_media_id` + `page_index` — a page is a media, so upload/storage/thumbnails/cropping need no second path; listings filter `parent_media_id IS NULL`.

**Import** (`docs/specifications/ui-import.md`, `ui-geneanet-import.md`): the tree card's `⋮` → Import opens `components/import_modal.rs` — a file tab (.ged/.gw) and a five-step Geneanet tab. Pipeline in `crates/oxidgene-geneanet`; server side in `api/src/service/geneanet.rs` + `rest/geneanet.rs`; login window in `apps/oxidgene-desktop/src/geneanet.rs`, reached through the `GeneanetCollector` trait so `oxidgene-ui` never sees `wry`. Steps 2 (archives), 3 (login) and the media half of 5 are **desktop-only**.

Two rules that are load-bearing there: a **multi-page deposit imports whole** as a `media.is_document` with its pages ordered by Geneanet's page number (links attach to pages, and users link the cover); and media are **recognised, not re-downloaded** — exact byte length where Geneanet states one, a 256-bit perceptual hash against the data archives where it does not. Step 3's output **saves to a file and loads back**, which is how to test without making several hundred `HEAD` requests against a real account.

Open EPIC F items: the S3 backend for the server deployment, PostgreSQL verification, PDF page rendering (needs a C rasteriser, deliberately declined), showing a vignette as an event's illustration on the timeline, and — for the Geneanet wizard — the instructional screenshots (§3) plus per-photo progress and cancellation in step 5. The wizard has **not been run against a live Geneanet account**.

See [`docs/specifications/general.md` §8b](docs/specifications/general.md) for the EPIC status table and recently shipped work, and [`docs/specifications/roadmap.md`](docs/specifications/roadmap.md) for full sprint details. Update both files each time a new feature is developed.
