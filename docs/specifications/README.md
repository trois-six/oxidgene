# OxidGene — Specifications Index

![OxidGene](../assets/OxidGene.png)

This directory contains all functional, technical, and visual specifications for the OxidGene genealogy application.

---

## Foundation

| Document | Description |
|----------|-------------|
| [General](general.md) | Project vision, objectives, target users, core features, security, performance, MVP scope |
| [Architecture](architecture.md) | Technology stack, backend/frontend architecture, project structure, crate dependencies, build & deployment |
| [Data Model](data-model.md) | All entities (Tree, Person, Family, Event…), enums with GEDCOM tag mapping, ERD |
| [API Contract](api.md) | REST (`/api/v1`) and GraphQL (`/graphql`) endpoints, pagination, types, GEDCOM compatibility |
| [Roadmap](roadmap.md) | EPICs A–F, sprint breakdown, milestones, completion status |

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
| [Homepage](ui-home.md) | Tree dashboard, tree cards, search/sort, create/delete modals | → [Settings](ui-settings.md) · [Tree View](ui-genealogy-tree.md) |
| [Genealogy Tree](ui-genealogy-tree.md) | Pedigree canvas, person cards, connectors, navigation, events sidebar | → [Person Edit Modal](ui-person-edit-modal.md) · [Person Profile](ui-person-profile.md) · [Search Results](ui-search-results.md) |
| [Person Profile](ui-person-profile.md) | Full person detail view: identity, timeline, family connections, media, notes | → [Tree View](ui-genealogy-tree.md) · [Person Edit Modal](ui-person-edit-modal.md) |
| [Search Results](ui-search-results.md) | Filterable person search results page | → [Tree View](ui-genealogy-tree.md) · [Person Profile](ui-person-profile.md) |
| [Settings](ui-settings.md) | Tree & roots, privacy, date display, entry options, tools, export | → [Homepage](ui-home.md) · [Data Model](data-model.md) |

### Modals & Flows

| Document | Description | Key cross-references |
|----------|-------------|----------------------|
| [Person Edit Modal](ui-person-edit-modal.md) | Create & edit person (all context variants), couple/union edit, media, deletion | → [Tree View](ui-genealogy-tree.md) · [Settings](ui-settings.md) |
| [Person Merge](ui-merge.md) | 3-step wizard: select duplicate, compare side-by-side, confirm merge | → [Tree View](ui-genealogy-tree.md) · [Settings](ui-settings.md) (duplicates tool) |
| [GEDCOM Import](ui-gedcom-import.md) | Upload, preview, and import a GEDCOM file into a tree | → [Tree View](ui-genealogy-tree.md) · [Homepage](ui-home.md) · [Settings](ui-settings.md) (export) |

---

## Document Map

```
README.md  ← you are here
│
├── Foundation
│   ├── general.md           Vision, users, features, MVP scope
│   ├── architecture.md      Tech stack, crate layout, deployment
│   ├── data-model.md        Entities, enums, GEDCOM mapping, ERD
│   ├── api.md               REST + GraphQL + GEDCOM compat
│   └── roadmap.md           EPICs & sprints (with status)
│
├── Cross-cutting
│   ├── i18n.md              Internationalization
│   ├── error-handling.md    Errors, loading, empty states
│   └── ui-design-tokens.md  Colors, typography, spacing
│
└── UI Specifications
    ├── Shared
    │   ├── ui-topbar.md              Topbar component
    │   └── ui-shared-components.md   Reusable components
    │
    ├── Pages
    │   ├── ui-home.md              Homepage / tree dashboard
    │   ├── ui-genealogy-tree.md    Tree view / pedigree canvas
    │   ├── ui-person-profile.md    Person detail view
    │   ├── ui-search-results.md    Search results page
    │   └── ui-settings.md          Tree settings & tools
    │
    └── Modals & Flows
        ├── ui-person-edit-modal.md Person create/edit & couple edit modals
        ├── ui-merge.md             Person merge wizard
        └── ui-gedcom-import.md     GEDCOM import wizard
```
