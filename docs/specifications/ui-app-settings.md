---
type: "UI Specification"
title: "Visual & Functional Specifications — App Settings"
description: "UI behavior and interaction specification for Visual & Functional Specifications — App Settings."
tags: [oxidgene, specification, ui, ux]
timestamp: 2026-06-17T00:00:00Z
---


# Visual & Functional Specifications — App Settings

> Part of the [OxidGene Specifications](index.md).
> See also: [Homepage](ui-home.md) · [Common UI](ui-common.md)

---

## 1. Overview

The app settings page (`/settings`) is a dedicated full-page interface for configuring **application-level** preferences that are not tied to any specific tree. It is accessed via the gear icon in the [Homepage](ui-home.md) page header.

This page is distinct from [Tree Settings](ui-settings.md), which configure per-tree options.

---

## 2. Layout

Uses the standard `sub-page` layout pattern (see [General](general.md) section 8).

```
+----------------------------------------------------------------------+
| NAVBAR                                                                |
+----------------------------------------------------------------------+
| Home / Settings                                                      |  <- td-topbar
+----------------------------------------------------------------------+
|                                                                       |
|   +------------------+---------------------------------------------+ |
|   |                  |                                              | |
|   | LEFT NAVIGATION  |   CONTENT AREA                              | |
|   | (200px)          |                                              | |
|   |                  |   Section eyebrow                            | |
|   | Preferences      |   Section title                              | |
|   | - Appearance     |   Content (cards, toggles, options)          | |
|   | - Language       |                                              | |
|   | - Pedigree       |                                              | |
|   |                  |                                              | |
|   +------------------+---------------------------------------------+ |
|                                                                       |
+----------------------------------------------------------------------+
```

The outer content uses the standard centered `max-width: 1200px` page layout.
Inside it, the settings content is capped at `860px` and uses the same compact
navigation, typography, spacing, and selection treatment as Tree Settings.
Left navigation + content area use a flex row layout (`.settings-layout`).

---

## 3. Topbar

Uses the shared `td-topbar` + `td-bc` breadcrumb component:

```
Home / Settings
```

- "Home" (`.td-bc-link`) links to the homepage (`/`)
- `/` separator (`.td-bc-sep`)
- "Settings" (`.td-bc-current`) — not clickable

---

## 4. Left Navigation

Fixed width: 200px. One group labeled "Preferences" (uppercase, orange).

Items:
| Item | Section |
|---|---|
| Appearance | Theme toggle |
| Language | Language selection |
| Pedigree | Initial ancestor and descendant depths |
| Names | Surname-particle sorting |
| API | REST OpenAPI access; GraphQL access in the web build |

Active item: primary text, bold weight, and the neutral selection background
(`var(--sel-bg)`), matching Tree Settings.

---

## 5. Section: Appearance

### Header

- Eyebrow: "Appearance" (uppercase, orange)
- Title: "Appearance" (Cinzel font)
- Subtitle: "Customise the look and feel of the application."

### Theme Toggle

Displayed in a card (`.app-settings-card`):

```
+-----------------------------------------------------------+
|  Theme                                                     |
|  Light theme is active / Dark theme is active             |
|                                                            |
|  [ (sun) Light ][ (moon) Dark ]                           |
+-----------------------------------------------------------+
```

- Label: "Theme" (bold) + current state hint (muted)
- Toggle group: two buttons side by side in a bordered container
- Each button: icon (sun/moon SVG) + label text
- Active button: `var(--orange)` background, white text
- Inactive button: transparent background, muted text

Clicking a button immediately applies the theme (no save step). The preference is persisted in `localStorage('oxidgene-theme')`.

---

## 6. Section: Language

### Header

- Eyebrow: "Language" (uppercase, orange)
- Title: "Language" (Cinzel font)
- Subtitle: "Choose your preferred language."

### Language Options

Displayed in a card:

```
+-----------------------------------------------------------+
|  [flag] English                                    [check] |
|  [flag] Francais                                          |
+-----------------------------------------------------------+
```

- Each language is a full-width button with: flag emoji, language name, optional checkmark for active
- Active language: orange border, subtle orange tint background
- Clicking a language immediately switches the UI language (no save step)
- The preference is persisted in `localStorage('oxidgene-lang')`

---

## 7. Section: Pedigree

The Pedigree section controls how the pedigree is drawn and how deep it opens.

### Theme

A stacked option listing every pedigree theme as a card: a swatch, its name and
a one-line description. The active theme carries an orange border and a tinted
background, and `aria-pressed`.

The swatch is drawn from the theme's own metrics, frame and link style — two
cards and the connector between them, without names or portraits — so it cannot
drift from what the theme actually draws. Each swatch paints its own ground,
so a theme with its own canvas is compared against that rather than against the
settings panel.

Choosing a theme applies immediately, with no save step, to every pedigree on
the device. It is persisted in `localStorage('oxidgene-pedigree-theme')` as the
theme's own name, so inserting a theme never repaints an existing choice; an
unknown name falls back to the default. See
[Themes](ui-genealogy-tree.md#9-themes) for what each one changes.

### Depth

The initial depth used when opening a tree that does not yet have a saved
pedigree view:

- **Ancestor generations**: 0–10, default 4.
- **Descendant generations**: 0–10, default 3.

Each value uses a bounded minus/value/plus stepper and is persisted immediately
in `localStorage('oxidgene-pedigree-defaults')`. The same shared controls appear
under **Global preferences > Pedigree** in [Tree Settings](ui-settings.md), and
both surfaces update the same application-level preference. An existing saved
view keeps its per-tree depths and therefore takes precedence over these
defaults.

---

## 8. Section: API

The API section displays absolute endpoint URLs derived from the same API base
URL as the frontend client:

- `GET /api/v1/openapi.json` opens the generated OpenAPI 3.1 document.
- In the web build, `GET /graphql` opens GraphiQL in a new browser tab and
	`POST /graphql` accepts GraphQL queries and mutations.

The desktop build displays only the REST/OpenAPI entry and continues to compile
the API crate without its optional `graphql` feature.

## 9. Responsive

- Outer content max-width: 1200px, responsive padding; settings content max-width: 860px
- At or below **768px**: the left navigation stacks above the content area as one
	compact, non-wrapping row. It scrolls horizontally when necessary instead of
	growing into a tall menu, so settings content remains in the first viewport.
- Theme, language, and pedigree controls remain full-width cards
- The pedigree theme swatches reflow to a single column when the content
	area can no longer fit two 150px cards side by side

---

## 10. Future Sections

Additional sections may be added in future EPICs:

| Section | Description |
|---|---|
| Account | User profile, email, password (EPIC G) |
| Notifications | Notification preferences (EPIC G) |
| Data & Privacy | Data export, account deletion (EPIC G) |
