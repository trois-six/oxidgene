---
okf_version: "0.1"
---

# OxidGene — Specifications Index

![OxidGene](../assets/OxidGene.png)

This directory contains all functional, technical, and visual specifications for the OxidGene genealogy application.

---

## Foundation

| Document | Description |
|----------|-------------|
| [General](general.md) | Project vision, objectives, target users, core features, security, performance, MVP scope, consistent page layout |
| [Quickstart](quickstart.md) | Run OxidGene from a desktop download, a desktop source build, Docker Compose, or Kubernetes with Helm |
| [Architecture](architecture.md) | Technology stack, backend/frontend architecture, project structure, crate dependencies, build & deployment |
| [Development](development.md) | Development environment, prerequisites, local workflows, and `just` command reference |
| [Data Model](data-model.md) | Entities, enums, ERD, durable person projections, pedigree assembly, and search models |
| [API Contract](api.md) | REST (`/api/v1`) and GraphQL (`/graphql`) endpoints, pagination, types, GEDCOM compatibility |
| [Roadmap](roadmap.md) | Delivery status, active priorities, and future milestones |
| [Geneanet Media Import](geneanet-media-import.md) | Technical: recovering the person↔photo links a Geneanet export drops — the media API, the GeneWeb join key, size matching |
| [Geneanet Upload API](geneanet-upload-api.md) | Reference: the upload app's `api.geneanet.org` surface, Cloudflare/HTTP-client matrix, originals vs renditions, login findings (reverse-engineered 2026-08-16) |

## Cross-cutting

| Document | Description |
|----------|-------------|
| [Cross-cutting Rules](cross-cutting.md) | Technical language, i18n, errors, logging, privacy, loading, empty states, and verification |
| [Common UI](ui-common.md) | Shared layout, topbar, design tokens, components, accessibility, and responsive behavior |

## UI Specifications

Each routed page has exactly one specification. Shared behavior lives only in
[Common UI](ui-common.md); a modal or workflow has one canonical specification
and no versioned or per-tab companion file.

### Pages

| Document | Description | Key cross-references |
|----------|-------------|----------------------|
| [Homepage](ui-home.md) | Tree dashboard, tree cards, search/sort, create/delete modals | -> [Settings](ui-settings.md) · [Tree View](ui-genealogy-tree.md) |
| [Genealogy Tree](ui-genealogy-tree.md) | Pedigree canvas, person cards, connectors, navigation, events sidebar | -> [Person Edit Modal](ui-person-edit-modal.md) · [Person Profile](ui-person-profile.md) · [Search Results](ui-search-results.md) |
| [Person Profile](ui-person-profile.md) | Full person detail view: identity, timeline, family connections, media, notes | -> [Tree View](ui-genealogy-tree.md) · [Person Edit Modal](ui-person-edit-modal.md) |
| [Search Results](ui-search-results.md) | Filterable person search results page | -> [Tree View](ui-genealogy-tree.md) · [Person Profile](ui-person-profile.md) |
| [Dictionary](ui-dictionary.md) | Read-only index of family names, sources, places, occupations with usage counts | -> [Tree View](ui-genealogy-tree.md) · [Person Profile](ui-person-profile.md) · [Search Results](ui-search-results.md) |
| [Settings](ui-settings.md) | Tree & roots, privacy, date display, entry options, tools, export | -> [Homepage](ui-home.md) · [Data Model](data-model.md) |
| [App Settings](ui-app-settings.md) | Application-level preferences: appearance (theme), language | -> [Homepage](ui-home.md) · [Common UI](ui-common.md) |

### Modals & Flows

| Document | Description | Key cross-references |
|----------|-------------|----------------------|
| [Person Edit Modal](ui-person-edit-modal.md) | Create & edit person (all context variants), couple/union edit, media, deletion | -> [Tree View](ui-genealogy-tree.md) · [Settings](ui-settings.md) |
| [Person Merge](ui-merge.md) | 3-step wizard: select duplicate, compare side-by-side, confirm merge | -> [Tree View](ui-genealogy-tree.md) · [Settings](ui-settings.md) (duplicates tool) |
| [Import](ui-import.md) | One modal for `.ged`, `.gdz`, `.gw`, and the five-step Geneanet media flow | -> [Homepage](ui-home.md) · [Settings](ui-settings.md) · [Geneanet Media Import](geneanet-media-import.md) |

---

## Document Map

```
index.md  <- you are here
|
|- Foundation
|  |- general.md           Vision, users, features, MVP scope
|  |- quickstart.md        Desktop, Compose, and Kubernetes startup paths
|  |- architecture.md      Tech stack, crate layout, deployment
|  |- development.md       Development environment and just workflows
|  |- data-model.md        Entities, projections, pedigree, search
|  |- api.md               REST and GraphQL contract
|  |- roadmap.md           Status and remaining milestones
|  |- geneanet-media-import.md  Geneanet media join and import
|  '- geneanet-upload-api.md    Geneanet upload API reference
|
|- Cross-cutting
|  |- cross-cutting.md     Language, i18n, errors, logs, privacy
|  '- ui-common.md         Layout, topbar, tokens, shared components
|
'- UI Specifications
	|- Pages
	|  |- ui-home.md              Homepage / tree dashboard
	|  |- ui-genealogy-tree.md    Tree view / pedigree canvas
	|  |- ui-person-profile.md    Person detail view
	|  |- ui-search-results.md    Search results page
	|  |- ui-dictionary.md        Family names / sources / places / occupations index
	|  |- ui-settings.md          Tree settings & tools
	|  '- ui-app-settings.md      App-level settings (theme, language)
	|
	'- Modals & Flows
		|- ui-person-edit-modal.md Person create/edit & couple edit modals
		|- ui-merge.md             Person merge wizard
		'- ui-import.md            File and Geneanet import modal
```
