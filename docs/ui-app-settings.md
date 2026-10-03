---
type: "UI Specification"
title: "Visual & Functional Specifications — App Settings"
description: "Application-level preferences page for appearance, language, pedigree, names, API connection details, and the AI assistant connection."
tags: [oxidgene, specification, ui, ux]
generated: { by: claude-code/claude-opus-5-5, at: 2026-10-03T10:19:34Z }
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

Switching sidebar sections resets the scrollable content container to offset
zero after the displayed section is updated. A short section must not inherit
the scroll position of a long
section. Editing a preference within the current section does not reset its
scroll position. App Settings and Tree Settings use the same section-dependent
scroll hook, regardless of whether content elements are remounted.

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
| Pedigree | Chart, pedigree theme, initial ancestor and descendant depths |
| Names | Surname-particle sorting |
| API | REST endpoints with how to connect a client, and GraphQL in the web build; AI assistant (MCP) connection in the desktop build |

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
|  How the whole application looks: its colours, typefaces,  |
|  corners and spacing.                                      |
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
|  `/home/<user>/.config/oxidgene/themes`                    |
+-----------------------------------------------------------+
```

- Built-in themes come first, in the order they ship; themes read from the
  user's folder follow, sorted by file name.
- The grid is the shared theme picker, also used by the pedigree theme choice
  (§7). Tracks are laid out with `auto-fill` rather than `auto-fit`, so a tile
  is the same size whether the picker holds two entries or twelve.
- Each tile shows a miniature of that theme — page background, navigation
  bar, a card with a heading-typeface specimen, two text rules and an
  accent, with the theme's corners and shadow. The tile declares the theme's
  custom properties on itself, so its `var(--token)` rules preview that
  theme instead of the active one.
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
removed — resets the selection and stored preference to `light`. Restoring
the file makes it available again but does not select it automatically.

### Custom themes

Custom themes are read from `<config directory>/themes/*.json` —
`~/.config/oxidgene/themes/` on Linux ([Architecture §8.3](architecture.md#83-local-files)) — and are a desktop
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

See [Common UI](ui-common.md#3-design-tokens) for the file format, the style
presets and the full token list.

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
|  [flag] Français                                          |
|  [flag] Deutsch                                           |
|  [flag] Español                                           |
|  [flag] Italiano                                          |
|  [flag] Nederlands                                        |
|  [flag] Polski                                            |
|  [flag] Português                                         |
+-----------------------------------------------------------+
```

- Each language is a full-width button with: flag emoji, the language's own name, optional checkmark for active
- Active language: orange border, subtle orange tint background
- Clicking a language immediately switches the UI language (no save step)
- The preference is persisted in `localStorage('oxidgene-lang')`

The list comes from locale JSON documents, not a fixed Rust language list.
Names and flags are document metadata. Embedded files are ordered by filename;
valid personal files follow, also ordered by filename.

On desktop the Language section scans `<config directory>/languages/*.json`
every time it is opened, and shows a localized hint above the folder explaining
that a JSON file can be added and the page reopened to offer its language.
The list, hint and folder use the same stacked spacing as the theme picker.
On Linux this
is `~/.config/oxidgene/languages/`, respecting `XDG_CONFIG_HOME`. The folder is
created at launch, but startup does not scan its contents: it reads only the
selected `<code>.json`, if the stored preference names a personal language.
The browser has no personal filesystem catalogue.

Adding or editing a file requires no restart or reload button. Invalid files
are listed by filename with a localized error. A missing or invalid selected
file resets the choice and persisted preference to English, including when
discovered on returning to this section. See
[Cross-cutting Rules §3.5](cross-cutting.md#35-adding-a-language) for the common
embedded/custom schema, including plural and date rules.

---

## 7. Section: Pedigree

The Pedigree section contains two separate blocks with equally prominent
headings. *Pedigree type* contains the chart and theme choices. *Pedigree depth*
has its own block below, containing only the two generation counters. A shared
section gap separates the blocks; depth is not a subsection inside the type
card. Both App Settings and Tree Settings > Global preferences render the
same component and spacing.

### Chart

A stacked option listing every way the tree view can draw the pedigree — the
tree, the ancestor wheel and fan chart, the descendant wheel and fan, the lineage and the descendant lineage, the hourglass, the bowtie — as tiles of the same picker as the
themes: a miniature, a name and a one-line description. The miniatures of the
wheels and the fans are drawn from the chart's own rings and segments three
generations deep, and the horizontal charts' from their own boxes and elbow lines, in the
current pedigree theme's colours; the tree's is a
schematic of parents above the root and children below it.

Choosing a view applies immediately, with no save step, to the tree view on
this device. It is persisted in `localStorage('oxidgene-pedigree-view')` as
the view's own name (`tree`, `wheel`, `fan`, `descendant-wheel`, `descendant-fan`, `lineage`, `descendant-lineage`, `hourglass`, `bowtie`); the tree is the default and an
unknown name falls back to it. It is a display preference like the theme, not
a property of any tree, so it is not stored on the server. See
[Views](ui-genealogy-tree.md#10-views) for what each one draws and which
depth controls it offers.

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
- **Descendant generations**: 0–10, default 3. Unused while a chart that draws
  ancestors only is chosen, but kept for the tree.

Each value uses a bounded minus/value/plus stepper and is persisted immediately
in `localStorage('oxidgene-pedigree-defaults')`. The same shared controls appear
under **Global preferences > Pedigree** in [Tree Settings](ui-settings.md), and
both surfaces update the same application-level preference. An existing saved
view keeps its per-tree depths and therefore takes precedence over these
defaults.

---

## 8. Section: API

Both builds serve REST; only the web build serves GraphQL, the desktop
compiling the API crate without its optional `graphql` feature. The section
presents what the current build serves. Every URL is absolute and derived from the frontend client's own API base
URL: in the web build, the address the deployment gives the frontend; on
desktop, the embedded server's `http://127.0.0.1:<port>`, whose port the
operating system picks at each launch.

```
+-----------------------------------------------------+
|  OpenAPI specification                          [↗] |
|  http://127.0.0.1:8080/api/v1/openapi.json          |
|-----------------------------------------------------|
|  GraphQL explorer (GraphiQL)                    [↗] |   (web only)
|  http://127.0.0.1:8080/graphql                      |
+-----------------------------------------------------+
+-----------------------------------------------------+
|  Connect a client                                   |
|  (!) Anyone holding this token …      (desktop only)|
|  Access token        [ 3f9c…                ] [Copy]|
|  Send it as Authorization: Bearer <token> …         |
|  REST base URL       [ …/api/v1             ] [Copy]|
|  Example (GET)       [ curl '…/openapi.json'] [Copy]|
|  GraphQL endpoint    [ …/graphql            ] [Copy]|  (web only)
|  Example (POST)      [ curl -X POST … \      ] [Copy]|  (web only)
+-----------------------------------------------------+
```

- The endpoint list opens each entry in the system browser:
  `GET /api/v1/openapi.json`, the generated OpenAPI 3.1 document, and, in the
  web build, `GET /graphql`, GraphiQL, which answers only where the deployment
  enables it (`OXIDGENE_GRAPHIQL`; development stacks do, production defaults
  do not).
- The **Connect a client** card states, as read-only fields with a copy button,
  the REST base URL (`/api/v1`, every HTTP method the contract uses), a `curl`
  `GET` of the OpenAPI document and, in the web build, the GraphQL endpoint
  (`POST` with a JSON body) and a `curl` `POST` of a minimal query against the
  executable schema: `{ trees(first: 1) { edges { node { id name } } } }`. The examples use POSIX
  shell quoting.
- **Desktop.** The embedded server listens on loopback only and answers only
  requests carrying the bearer token generated at launch
  ([Cross-cutting Rules §7.1](cross-cutting.md#71-backend-exposure-before-authentication)).
  The card shows that token above the fields, behind an always-visible warning
  that whoever holds it can read and change every tree while the application
  runs. A hint says the token goes, as `Authorization: Bearer`, with every
  request except the OpenAPI document, and that the address and the token
  change at every launch. Nothing is saved:
  handing the token to another program is the consent, and quitting the
  application revokes it.
- **Web.** The frontend's client carries no token, so neither the warning nor
  the token appears, and the examples have no `Authorization` header. Browser
  pages on another origin cannot write through the standalone server
  (Cross-cutting Rules §7.1); `curl` and scripts send no `Origin` and are not
  affected. GraphiQL posts every operation from the API's own origin, so it
  runs them only where the frontend and the API share one origin, as behind a
  same-origin gateway; elsewhere the standalone server refuses them
  (`403 forbidden`).
- Labels, hints, the warning, and the copy feedback go through i18n in every
  interface language. URLs, the token, header names, and the example commands
  are protocol values and are not translated.

### AI assistant (MCP)

Shows how to let an
MCP client, such as Claude Desktop or Claude Code, read the application's
trees. The server and its contract are specified in
[Assistant Access](mcp.md).

```
+-----------------------------------------------------+
|  AI assistant (MCP)                                 |
|                                                     |
|  (!) An assistant configured with this command can  |
|      read every tree in this application, living    |
|      people, notes and sources included, and sends  |
|      what it reads to the model provider it uses.   |
|                                                     |
|  Command                                            |
|  [ /…/oxidgene-desktop mcp               ] [Copy]   |
|                                                     |
|  Client configuration (JSON)                        |
|  [ { "mcpServers": { … } }               ] [Copy]   |
+-----------------------------------------------------+
```

- The warning is always visible above the command, not behind a disclosure.
- The command contains the absolute path of the running executable. The JSON
  block is the same command in `mcpServers` form. Both are read-only fields
  with a copy button.
- Access is read-only, and each request names the tree it reads. Privacy
  values are not applied, as [Assistant Access §7](mcp.md) explains.
- The desktop binary injects the executable path through a UI capability,
  following the Geneanet collector pattern
  ([Architecture §9.2](architecture.md)). The browser build finds none and
  shows a note that the assistant is available in the desktop application.
- Nothing is saved: copying the command changes no setting. Configuring the
  client is the consent; removing the entry from the client revokes it.
- Every label, the warning, the note, and the copy feedback go through i18n,
  in every interface language. The command and the JSON are not translated.

## 9. Responsive

- Outer content max-width: 1200px, responsive padding; settings content max-width: 860px
- At or below **768px**: the left navigation stacks above the content area as one
	compact, non-wrapping row. It scrolls horizontally when necessary instead of
	growing into a tall menu, so settings content remains in the first viewport.
- Theme, language, and pedigree controls remain full-width cards
- Theme swatches use two equal, compact columns down to a 320px viewport and
  remain centred by filling the available card width. Below 300px, they may
  reflow to a single column.

---

## 10. Future Sections

Additional sections may be added in future EPICs:

| Section | Description |
|---|---|
| AI | Providers and keys each user brings for the AI features on media — see [AI Features](ai.md) §4 |
| Account | User profile, email, password (EPIC G) |
| Notifications | Notification preferences (EPIC G) |
| Data & Privacy | Data export, account deletion (EPIC G) |
