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
| Appearance | Theme picker |
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

### Theme picker

A stacked option in a card (`.app-settings-card`), listing every available
theme as a tile:

```
+-----------------------------------------------------------+
|  Theme                                                     |
|  The colours the whole application is drawn in.            |
|                                                            |
|  [ swatch ] [ swatch ] [ swatch ] [ swatch ] [ swatch ]    |
|    Light      Dark      ...          ...         ...       |
|                                                            |
|  [ swatch ]                                                |
|    Sepia                                                   |
|    CUSTOM                                                  |
|                                                            |
|  Themes are JSON files. Drop one into this folder and      |
|  open this page again to see it here.                      |
|  `/home/<user>/.local/share/oxidgene/themes`               |
+-----------------------------------------------------------+
```

- Built-in themes come first, in the order they ship; themes read from the
  user's folder follow, sorted by file name.
- The grid is the shared theme picker, also used by the pedigree theme choice
  (§7). Tracks are laid out with `auto-fill` rather than `auto-fit`, so a tile
  is the same size whether the picker holds two entries or twelve.
- Each tile shows a miniature painted in that theme's own colours — page
  background, navigation bar, a card with two text rules and an accent. The
  miniature uses the theme's values directly rather than `var(--token)`, so
  every tile previews its own palette instead of the active one.
- "Light" and "Dark" are translated; every other name — shipped or not —
  shows the `name` from its file verbatim. A user theme also carries a
  "Custom" tag.
- The active tile carries an orange border and the selection background.

Choosing a theme applies immediately, with no save step, and is persisted in
`localStorage('oxidgene-theme')` as the theme's id.

There is no automatic light/dark selection. The application starts on `light`
and stays there until a theme is chosen: with a theme list anyone can extend,
"follow the system" has no well-defined member, and a palette that changed on
its own under a window left open all day was not wanted.

A selected id that no longer resolves — a user theme whose file was renamed or
removed — falls back to `light` for rendering while the stored id is kept, so
restoring the file restores the choice.

### Custom themes

Custom themes are read from `<data directory>/themes/*.json` and are a desktop
capability: the browser build has no folder to read and shows a note saying so
instead of a path. The folder is created on launch so that the path shown is
one the user can open.

The folder is re-read every time the Appearance section is opened, so a theme
added or edited while the application is running appears without a restart and
without a control to press. There is no manual reload: the only screen where
the list is visible is the one that refreshes it.

Files that fail to load are listed under the picker by file name with the
reason, rather than being silently skipped — the person who wrote the file is
the one who can repair it.

See [Common UI](ui-common.md#3-design-tokens) for the file format and the full
token list.

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

A stacked option listing every pedigree theme as a tile: a swatch, its name and
a one-line description. It uses the same picker control as the application
theme (§5) — one grid, one tile, one active state — because the two are the
same choice over different things.

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
