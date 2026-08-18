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
| [Architecture](architecture.md) | Technology stack, backend/frontend architecture, project structure, crate dependencies, build & deployment |
| [Data Model](data-model.md) | All entities (Tree, Person, Family, Event…), enums with GEDCOM tag mapping, ERD |
| [API Contract](api.md) | REST (`/api/v1`) and GraphQL (`/graphql`) endpoints, pagination, types, GEDCOM compatibility |
| [Roadmap](roadmap.md) | EPICs A–G, sprint breakdown, milestones, completion status |
| [Read Projections](read-projections.md) | Denormalized read models in the database: person projections, pedigree assembly, search, refresh |
| [Geneanet Media Import](geneanet-media-import.md) | Technical: recovering the person↔photo links a Geneanet export drops — the media API, the GeneWeb join key, size matching |
| [Geneanet Upload API](geneanet-upload-api.md) | Reference: the upload app's `api.geneanet.org` surface, Cloudflare/HTTP-client matrix, originals vs renditions, login findings (reverse-engineered 2026-08-16) |

## Cross-cutting

| Document | Description |
|----------|-------------|
| [i18n](i18n.md) | Internationalization: translation keys, date/number formatting, locale handling |
| [Error Handling](error-handling.md) | API error codes, toasts, inline validation, loading states, empty states, offline behavior |
| [Design Tokens](ui-design-tokens.md) | CSS variables, color palette, typography, spacing, shadows, responsive breakpoints |

## UI Specifications

### Shared

| Document | Description |
|----------|-------------|
| [Topbar](ui-topbar.md) | Shared topbar: logo, navigation/breadcrumb, search fields, user actions |
| [Shared Components](ui-shared-components.md) | ConfirmDialog, PersonPicker, DateInput, PlaceInput, MediaUploader, EventIcon, EmptyState |

### Pages

| Document | Description | Key cross-references |
|----------|-------------|----------------------|
| [Homepage](ui-home.md) | Tree dashboard, tree cards, search/sort, create/delete modals | -> [Settings](ui-settings.md) · [Tree View](ui-genealogy-tree.md) |
| [Genealogy Tree](ui-genealogy-tree.md) | Pedigree canvas, person cards, connectors, navigation, events sidebar | -> [Person Edit Modal](ui-person-edit-modal.md) · [Person Profile](ui-person-profile.md) · [Search Results](ui-search-results.md) |
| [Person Profile](ui-person-profile.md) | Full person detail view: identity, timeline, family connections, media, notes | -> [Tree View](ui-genealogy-tree.md) · [Person Edit Modal](ui-person-edit-modal.md) |
| [Search Results](ui-search-results.md) | Filterable person search results page | -> [Tree View](ui-genealogy-tree.md) · [Person Profile](ui-person-profile.md) |
| [Dictionary](ui-dictionary.md) | Read-only index of family names, sources, places, occupations with usage counts | -> [Tree View](ui-genealogy-tree.md) · [Person Profile](ui-person-profile.md) · [Search Results](ui-search-results.md) |
| [Settings](ui-settings.md) | Tree & roots, privacy, date display, entry options, tools, export | -> [Homepage](ui-home.md) · [Data Model](data-model.md) |
| [App Settings](ui-app-settings.md) | Application-level preferences: appearance (theme), language | -> [Homepage](ui-home.md) · [Design Tokens](ui-design-tokens.md) |

### Modals & Flows

| Document | Description | Key cross-references |
|----------|-------------|----------------------|
| [Person Edit Modal](ui-person-edit-modal.md) | Create & edit person (all context variants), couple/union edit, media, deletion | -> [Tree View](ui-genealogy-tree.md) · [Settings](ui-settings.md) |
| [Person Merge](ui-merge.md) | 3-step wizard: select duplicate, compare side-by-side, confirm merge | -> [Tree View](ui-genealogy-tree.md) · [Settings](ui-settings.md) (duplicates tool) |
| [Import](ui-import.md) | The import modal: the file tab (`.ged`/`.gw`) and the shell both tabs share | -> [Geneanet Import](ui-geneanet-import.md) · [Homepage](ui-home.md) · [Settings](ui-settings.md) (export) |
| [Geneanet Import](ui-geneanet-import.md) | The modal's Geneanet tab: five steps importing a tree *with its media* — `.gw`, data archives, in-app login, preview, import | -> [Import](ui-import.md) · [Geneanet Media Import](geneanet-media-import.md) |

---

## Document Map

```
index.md  <- you are here
|
|- Foundation
|  |- general.md           Vision, users, features, MVP scope, page layout
|  |- architecture.md      Tech stack, crate layout, deployment
|  |- data-model.md        Entities, enums, GEDCOM mapping, ERD
|  |- api.md               REST + GraphQL + GEDCOM compat
|  |- roadmap.md           EPICs & sprints (with status)
|  |- read-projections.md  Denormalized read models (no cache tier)
|  |- geneanet-media-import.md  Geneanet media API, join key, size matching
|  '- geneanet-upload-api.md   Upload-app API reference, Cloudflare matrix
|
|- Cross-cutting
|  |- i18n.md              Internationalization
|  |- error-handling.md    Errors, loading, empty states
|  '- ui-design-tokens.md  Colors, typography, spacing
|
'- UI Specifications
	|- Shared
	|  |- ui-topbar.md              Topbar component
	|  '- ui-shared-components.md   Reusable components
	|
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
		|- ui-import.md            Import modal + file tab
		'- ui-geneanet-import.md   Geneanet import (tree + media)
```
