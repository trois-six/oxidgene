# OxidGene — Project Context for Claude

## What this is

A multiplatform genealogy application built entirely in Rust:
- **Dioxus 0.7** frontend (WASM web + desktop via WebView)
- **Axum 0.8** backend exposing both REST (`/api/v1`) and GraphQL (`/graphql`)
- **SeaORM** with PostgreSQL (web) or SQLite (desktop, embedded)
- **GEDCOM** import/export via `ged_io 0.12`

Specifications are available in the `docs/specifications/*.md`.

## Workspace layout

```
crates/
  oxidgene-core/    # Domain types, enums — no internal deps
  oxidgene-db/      # SeaORM entities + migrations
  oxidgene-gedcom/  # GEDCOM ↔ domain conversion (wraps ged_io)
  oxidgene-api/     # Axum REST handlers + async-graphql resolvers
  oxidgene-ui/      # Dioxus components, shared web/desktop
apps/
  oxidgene-server/  # Web server binary
  oxidgene-desktop/ # Desktop binary (embeds Axum + SQLite + WebView)
  oxidgene-cli/     # CLI (import/export, migrations)
```

## Key design decisions

| Topic | Decision |
|---|---|
| Primary keys | UUID v7 (time-ordered) |
| Pagination | Cursor-based (Relay-style) |
| Deletion | Soft delete (`deleted_at`) |
| Ancestor queries | `PersonAncestry` closure table |
| Auth | Deferred to EPIC E (not in MVP) |

## Core domain model

- **Tree** → contains Persons, Families, Events, Sources, Media, Notes
- **Person** → has PersonNames (one `is_primary`), FamilySpouse links, FamilyChild links, Events
- **Family** → links spouses (FamilySpouse) and children (FamilyChild); has Events
- **PersonAncestry** — closure table: `(ancestor_id, descendant_id, depth)` for O(1) traversal

## Frontend (oxidgene-ui)

Dioxus 0.7, components in `src/components/`, pages in `src/pages/`.

CSS is embedded as a `const &str` in `src/components/layout.rs` (`LAYOUT_STYLES`).

Key components:
- `layout.rs` — app shell + all CSS
- `pedigree_chart.rs` — vertical bidirectional pan/zoom tree (ancestors above root, descendants below)
- `tree_detail.rs` — page orchestrator: data fetching, context menu, modals, GEDCOM I/O
- `person_node.rs` — reusable person card
- `person_form.rs` / `union_form.rs` — tabbed edit modals

## REST API base path: `/api/v1`

Main resource paths:
- `/trees`, `/trees/{tid}/persons`, `/trees/{tid}/families`
- `/trees/{tid}/persons/{pid}/names`
- `/trees/{tid}/families/{fid}/spouses`, `.../children`
- `/trees/{tid}/events`, `/trees/{tid}/places`, `/trees/{tid}/sources`
- `/trees/{tid}/gedcom/import`, `/trees/{tid}/gedcom/export`

## Current MVP sprint (EPIC C/D)

Sprints A–B complete. Currently in D-series (UX, tree visualization). The pedigree chart is a vertical bidirectional tree (ancestors up, descendants down) with pan/zoom and a floating depth-control panel.

## What's NOT in MVP

Authentication, access control, collaborative editing, tree matching, async upload pipeline.
