# OxidGene — Context for Claude

## Project

Multiplatform genealogy app, 100% Rust. Dioxus frontend (WASM + desktop), Axum backend (REST `/api/v1` + GraphQL `/graphql`), SeaORM (PostgreSQL web / SQLite desktop), GEDCOM via ged_io.

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
| `caching.md` | Server-side cache: PersonCache, PedigreeCache, SearchIndex, invalidation |
| `ui-home.md` | Homepage / tree dashboard |
| `ui-genealogy-tree.md` | Pedigree canvas, cards, connectors, sidebar |
| `ui-person-edit-modal.md` | Person edit, couple edit, media, deletion |
| `ui-settings.md` | Tree settings, tools, export |

## Workspace

```
crates/
  oxidgene-core/    # Domain types, enums, errors — no internal deps
  oxidgene-db/      # SeaORM entities, migrations, repos
  oxidgene-gedcom/  # GEDCOM ↔ domain (wraps ged_io)
  oxidgene-cache/   # Server-side cache (Redis / in-memory + disk)
  oxidgene-api/     # Axum REST + async-graphql + service layer
  oxidgene-ui/      # Dioxus components + pages
apps/
  oxidgene-server/  # Web server binary
  oxidgene-desktop/ # Desktop binary (embeds Axum + SQLite + WebView)
  oxidgene-cli/     # CLI (import/export, migrations)
```

Dependency flow: `core` ← `db` ← `cache` ← `api` ← `server`/`desktop`/`cli`; `core` ← `gedcom` ← `api`; `core` ← `ui`.

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

- `rest/` — one handler file per resource (tree, person, family, event, place, source, citation, media, media_link, note, gedcom, family_member)
- `graphql/` — query.rs, mutation.rs, types.rs, inputs.rs
- `service/` — business logic (gedcom service)
- `rest/dto.rs` — request/response DTOs
- `rest/state.rs` — AppState (DB connection)
- `router.rs` — Axum router wiring
- `service/cache_service.rs` — cache orchestration, invalidation, builders

## Build commands

```bash
just build          # Build all
just test           # Run tests
just check          # fmt + clippy + test
just fmt            # Format
just clippy         # Lint
just server         # Run web server (dev)
just desktop        # Run desktop app (dev)
just cli <ARGS>     # Run CLI
```

## Assets

Logo: `docs/assets/OxidGene.{png,svg}` — used in navbar and README.

## Current sprint

EPICs A–E complete. EPIC E (Server-Side Caching) finished — all 5 sprints done: cache foundation, person cache + API integration, pedigree cache with LRU, search index + GEDCOM integration, Redis backend + desktop disk persistence. Only "Performance testing with 100K-person trees" remains deferred. Next up: EPIC F (Security & Deployment). See `docs/specifications/roadmap.md` for details. Update this file each time a new feature is developped.
