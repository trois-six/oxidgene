---
type: "UI Specification"
title: "Visual & Functional Specifications — Common UI"
description: "Shared layout, navigation, design tokens, components, accessibility, and responsive behavior."
tags: [oxidgene, specification, ui, ux, design-system]
generated: { by: claude-code/claude-opus-5-5, at: 2026-09-26T00:00:00Z }
---

# Visual & Functional Specifications — Common UI

> Part of the [OxidGene Specifications](index.md).
> See also: [Cross-cutting Rules](cross-cutting.md) ·
> [Homepage](ui-home.md) · [Tree View](ui-genealogy-tree.md)

---

## 1. Scope and rules

This is the only cross-page UI specification. Page and modal specifications
reference these rules instead of redefining them.

- Shared interactions have one canonical component and one style definition.
- Every user-visible string, including tooltips, placeholders, validation,
  empty states, accessibility labels, and backend-originated messages, is an
  i18n key with English and French parity.
- Documentation, screenshots, tests, fixtures, and examples use fictitious,
  anonymized people, trees, accounts, places, and archive references.
- Colors are defined only by the active theme (§3.1), never as literals in a
  stylesheet. Dimensions, typography, and derived tokens are defined in
  `crates/oxidgene-ui/src/components/layout.rs` (`LAYOUT_STYLES`). Pages do not
  duplicate literal colors, spacing, shadows, or typography.
- Removing a component or state also removes its CSS selectors, translations,
  tests, and obsolete API calls.

## 2. Shared page layout

The application has two top-level bars:

1. `app-nav`: the branding navbar displayed on every page.
2. `td-topbar`: the contextual breadcrumb and actions on tree-scoped pages.

Non-pedigree pages use `sub-page` with a scrollable `sub-page-content` area,
centered at `max-width: 1200px` with `24px` padding. The pedigree owns its
canvas layout and side panels. The homepage uses the same reading width without
the contextual topbar.

### 2.1 Navbar

- Compact, approximately 48px high, full width, in normal document flow.
- Background: `var(--nav-bg)` with `backdrop-filter: blur(12px)`.
- Bottom border: `1px solid var(--border)`.
- The OxidGene logo links to `/` and is the only MVP content.
- Future account, notification, and global navigation controls must not be
  documented as current behavior until implemented.

### 2.2 Contextual topbar

- Compact, approximately 40px high, full width, `10px 16px` padding.
- Transparent background and `1px solid var(--border)` bottom border.
- Left zone: home logo, linked tree name, separator, localized current page.
- Right zone: page-specific search or actions.

| Page | Breadcrumb | Right zone |
|---|---|---|
| Tree | tree name / Tree | Person search |
| Settings | tree name / Settings | Empty |
| Dictionary | tree name / Dictionary | Page actions |
| Search | tree name / Search | Pre-filled person search and fit action |
| Person | tree name / person display name | Page actions |
| App settings | Home / Settings | Empty |

Breadcrumb links use `var(--text-secondary)`, switch to `var(--orange)` on
hover, and truncate from the oldest intermediate crumb on narrow screens.

### 2.3 Person search

Tree and search pages share two compact fields for family names and given
names, plus a search icon button. Either field may be used independently.
Submitting navigates to the search page and preserves both values. `/` focuses
the family-name field when focus is not already in an editable control.

## 3. Design tokens

### 3.1 Themes

A theme is a set of colours and nothing else. Fonts, spacing, radii and
component geometry are the same under every theme: switching theme repaints
the application, it never relayouts it.

Themes are JSON documents. The shipped set is whatever
`crates/oxidgene-ui/assets/themes/` holds, listed in the order
`BUILTIN_SOURCES` declares; `light` comes first and is the only complete one,
and every other theme resolves from an earlier entry. Adding or removing one
is a matter of a file and a line, and is not tracked here.

A theme whose name is an ordinary interface word — `Light`, `Dark` — is
translated through `app_settings.theme_<id>`. Any other name, shipped or
written by a user, is shown verbatim in every language: a theme named after a
place, a product or a site has no translation.

Users may add their own themes, which appear in the app-settings picker
alongside the shipped ones. The
selected theme is emitted as a `:root { … }` block ahead of the stylesheet, so
every `var(--token)` in the CSS resolves against it. There is no `:root.dark`
selector and no `prefers-color-scheme` branch: `light` is the default and the
only way to change it is to choose another theme.

The choice is stored as the theme's id in `localStorage('oxidgene-theme')`.
An id that no longer resolves falls back to `light` for rendering while the
stored value is kept.

#### File format

```json
{
  "id": "sepia",
  "name": "Sepia",
  "base": "light",
  "colors": { "orange": "#8a5a2b", "bg-deep": "#f6f0e4" }
}
```

| Field | Rule |
|---|---|
| `id` | Lowercase letters, digits and `-`. Must not be a built-in id. |
| `name` | Shown in the picker, verbatim unless a translation key exists. |
| `base` | Optional. Id of a built-in theme to inherit from. |
| `colors` | Token name (without `--`) to colour. Unknown names are rejected. |

Without `base`, every token in §3.2 must be given. With it, only the
differences need listing — which is how `dark` is written, and what keeps an
existing theme working when a token is added.

Values are hexadecimal only: `#rgb`, `#rgba`, `#rrggbb` or `#rrggbbaa`. A
theme file is user input that ends up inside a `<style>` element, so CSS
functions, named colours and anything else are refused rather than passed
through.

Custom themes live in `<data directory>/themes/*.json` and are read by the
desktop application; see
[App Settings](ui-app-settings.md#custom-themes). A file that fails to load is
reported in settings by name with the reason.

#### Deriving rather than adding a token

Tints of a token — a hover wash, a focus halo, a shadow — belong in the
stylesheet as `color-mix(in srgb, var(--token) N%, transparent)`, not in the
theme. A theme that had to enumerate every alpha of every accent would be
unwritable by hand, and a custom theme that set only `--orange` would still
show the stock orange everywhere else.

### 3.2 Colors

`light` and `dark` are the reference palettes below. Every other shipped theme
overrides these same tokens from one of them; their values live in their own
files rather than being repeated here.

| Token | Light | Dark | Purpose |
|---|---|---|---|
| `--bg-deep` | `#ffffff` | `#0d0f14` | Page background |
| `--bg-panel` | `#ede9e2` | `#111318` | Panels and topbars |
| `--bg-card` | `#ffffff` | `#16191f` | Cards and inputs |
| `--bg-card-hover` | `#f5f3ef` | `#1c2030` | Hovered cards |
| `--nav-surface` | `#f4f2ee` | `#0a0b0d` | Opaque colour behind the navbar |
| `--sel-bg` | `#e8e0d4` | `#192038` | Selection |
| `--border` | `#d4ccc0` | `#252d3d` | Borders and dividers |
| `--border-glow` | `#e07820` | `#e07820` | Focus border |
| `--text-primary` | `#1e1a14` | `#ddd8cc` | Primary text |
| `--text-secondary` | `#5c5447` | `#7a8da8` | Secondary text |
| `--text-muted` | `#9e9488` | `#404f65` | Placeholder and disabled text |
| `--on-accent` | `#ffffff` | `#ffffff` | Text drawn on an accent fill |
| `--orange` | `#e07820` | `#e07820` | Primary actions and focus |
| `--orange-light` | `#f5a03a` | `#f5a03a` | Primary hover |
| `--green` | `#4ea832` | `#4ea832` | Birth and success |
| `--green-light` | `#7ec45f` | `#7ec45f` | Text on a green wash |
| `--green-accent` | `#5aab3c` | `#5aab3c` | Green fills and washes |
| `--blue` | `#4a90d9` | `#4a90d9` | Death and information |
| `--pink` | `#c4587a` | `#c4587a` | Female indicator |
| `--red` | `#e05555` | `#e05555` | Destructive hover and emphasis |
| `--danger` | `#e05252` | `#e05252` | Destructive fills |
| `--danger-text` | `#dc2626` | `#f87171` | Destructive text |
| `--connector` | `#a0937f` | `#2e4a6a` | Pedigree connectors |
| `--tree-visual-bg` | `#e8e0d4` | `#0d1018` | Home card tree background |
| `--tree-visual-branch` | `#b0a898` | `#3a4458` | Home card tree branches |
| `--shadow` | `#000000` | `#000000` | Base shadow colour |
| `--shadow-weak` | `#00000014` | `#00000059` | Resting elevation |
| `--shadow-strong` | `#0000001f` | `#0000008c` | Raised elevation |
| `--scrim` | `#000000` | `#000000` | Backdrops and overlays |
| `--page-glow-warm` | `#00000000` | `#e078200a` | Warm light leak on the page |
| `--page-glow-cool` | `#00000000` | `#5aab3c08` | Cool light leak on the page |
| `--media-bg` | `#0a0b0d` | `#0a0b0d` | Media viewer backdrop |
| `--media-panel` | `#060709` | `#060709` | Media viewer toolbars |
| `--media-frame` | `#e8dfc8` | `#e8dfc8` | Vignette frames and glyphs |
| `--media-caption` | `#e8e3d8` | `#e8e3d8` | Labels drawn over media |
| `--media-tint` | `#12161f` | `#12161f` | Wash inside a vignette frame |
| `--pn-bg` | `#efefef` | `#1e2330` | Pedigree card background |
| `--pn-root-bg` | `#006ac4` | `#006ac4` | Root card background |
| `--pn-spouse-bg` | `#ffffff` | `#252d3d` | Spouse card background |
| `--pn-border` | `#888888` | `#888888` | Pedigree card outline |
| `--pn-male-line` | `#00a6c0` | `#00a6c0` | Male gender rule |
| `--pn-female-line` | `#ff6699` | `#ff6699` | Female gender rule |
| `--pn-sosa` | `#95c417` | `#95c417` | Sosa badge |
| `--pn-sosa-root` | `#6da118` | `#6da118` | Sosa badge on the root card |
| `--pn-self` | `#006ac4` | `#006ac4` | Selected-person marker |
| `--pn-text` | `#111111` | `#e8dfc8` | Pedigree card text |
| `--pn-text-muted` | `#555555` | `#7a8da8` | Pedigree card dates |
| `--pn-hover-bg` | `#cfe3fa` | `#2b4364` | Hovered pedigree card |

The pedigree chart themes ([Genealogy Tree](ui-genealogy-tree.md#9-themes))
are a separate choice that overrides `--pn-*` on the chart itself; they are
not part of the application theme.

Tokens derived in the stylesheet rather than set by a theme:

| Token | Derivation |
|---|---|
| `--nav-bg` | `--nav-surface` at 92% |
| `--shadow-sm` | `0 1px 3px var(--shadow-weak)` |
| `--shadow-md` | `0 4px 16px var(--shadow-strong)` |
| `--select-arrow` | Data URI of the select chevron, in `--text-secondary` |

Semantic aliases map generic component names to these core tokens:
`--color-border`, `--color-danger`, `--color-danger-text`, `--white`, and
`--shadow-black`.

### 3.3 Typography and sizing

| Token | Value | Usage |
|---|---|---|
| `--font-heading` | `'Cinzel', Georgia, serif` | Brand and headings |
| `--font-sans` | `'Lato', sans-serif` | Body, controls, and metadata |
| `--sb` | `46px` | Tree icon sidebar |
| `--evw` | `275px`, then a ratio once resized | Tree events panel width |
| `--radius` | `8px` | Cards, buttons, inputs, modals |

Reference type scale: page title `1.3rem`, section heading `1.05rem`, card
title `0.95rem`, body `0.85rem`, metadata `0.78rem`, small text `0.72rem`, and
badge text `0.65rem`. Spacing follows 4, 8, 12/16, 20/24, and 32px steps.

### 3.4 Elevation and interaction

- `--shadow-sm`: cards and dropdowns.
- `--shadow-md`: modals, popovers, and the navbar.
- Buttons use card background and border by default, orange focus/hover, solid
  accent for primary actions, and danger tokens for destructive actions.
- Text inputs, selects and textareas use card background and border, orange
  focus, red validation state, and `0.5` opacity while disabled.
- Checkboxes and radios are excluded from that styling and are sized once,
  centrally, with the orange accent colour. They keep their native appearance:
  a control given a text field's full width, padding, border and background
  renders as a large empty box beside a squeezed label. A row that reserves a
  column for one sets only its own layout, never the control's size.
- Cards may lift by 2px on hover only where motion does not move adjacent
  controls or impair repeated use.
- Gender cannot be communicated by color alone. Male uses blue, female pink,
  and unknown muted gray only as secondary cues.

## 4. Shared components

All component text properties receive localized strings or translation keys.
Callers do not embed user-visible literals.

### 4.1 ConfirmDialog

A focused modal for destructive or irreversible actions. It contains a title,
explanation, cancel action, and explicit confirm action. Danger mode uses
`var(--color-danger)`. `Escape` and backdrop press cancel unless an operation is
already running. Focus is trapped and restored to the triggering control.

### 4.2 PersonPicker

Displays an optional selected person with the same canonical summary as each
person-search result: profile photo or sex-specific placeholder portrait,
surname and given names, birth and death years, and birth place when known.
**Change** opens the shared person search; **Clear** is available only when the
field is optional. It receives `tree_id`, selected person, required state, and
an `EventHandler<Option<Person>>`.

Portrait maps used by the pedigree, person profile, search results, person
picker, and settings load their display-ready images in one bounded API
operation. These surfaces must not issue one thumbnail or vignette request per
person. Sets larger than the API limit are split into as many bounded batches
as necessary. A person profile requests only the person it renders;
single-image endpoints are reserved for media workflows that display one
selected asset.

### 4.3 DateInput

Edits partial dates, calendar, qualifier, and an optional second bound. It
supports exact, about, calculated, estimated, perhaps, before, after, or,
between, and age-derived input. Changing calendars converts representable dates
rather than relabeling values. Invalid or unrepresentable input remains visible
with a localized inline error.

Display formatting uses the shared date formatter; year-only surfaces use
`qualified_year()` so precision is not discarded.

### 4.4 PlaceInput

Autocomplete is helpful, never restrictive. Suggestions begin after three
characters with a 300ms debounce and prioritize existing tree places, then an
optional offline place database, then future external geocoding. Selecting a
suggestion stores its place ID; editing the text afterwards clears that link.
Free text is always accepted.

Canonical display is comma-separated from the most specific to the least
specific unit, ending with the country, but the number of levels varies by
country. Documentation examples use placeholders rather than real addresses or
archive locations.

Offline place databases are optional SQLite files in the application data
directory. They are downloaded and updated explicitly from settings; automatic
network access is not assumed.

### 4.5 MediaInput, MediaGallery, and DocumentForm

The canonical upload cell accepts clicks and drag-and-drop and reports
per-file progress. Files are processed through the same upload API regardless
of entry point. It also has a deferred mode in which it uploads nothing and
hands the chosen `(file name, bytes)` pairs to its caller; that mode exists for
`DocumentForm`, which has no document to hang pages off until the user saves.
The canonical gallery owns tiles, viewer opening, edit actions, document
paging, portraits, and context menus. Pages do not implement alternate media
grids.

The initial grid loads tile thumbnails, the first four document-page previews
— a generated thumbnail, or a remote page's own image URL — vignette crops, and
linked event ids through one bounded gallery bundle. It
must not mount one image, page-list, or reverse-link resource per tile. Larger
sets are split into as many batches of 1,024 identifiers as necessary. Viewer
and editor panels may use an individual endpoint after the user opens one
asset.

A tile draws its document's page previews when there are any, its own
thumbnail when the tile is a stored page, and the address when it is a remote
image; only a tile with none of those falls back to a labelled file icon. A
tile whose file is somebody else's — the document's page, or the tile itself —
carries the remote badge. A document is offered as a portrait exactly when it
draws a picture; a PDF is not, and the action is withheld rather than accepted
and then drawn as a silhouette.

A remote page whose MIME type nobody could establish counts as a remote image
everywhere a picture is drawn: the tile, the document mosaic, the page list,
the square beside each attachment, and the viewer. Such a page has an address
with no extension — a CDN naming its file `AF2bZy…=s64-c-mo` — and since we
never fetch it there is no second guess to make: the browser is the only reader
able to identify the bytes, so it is given the chance. When it refuses, the
viewer replaces the picture with the fallback panel, saying that nothing
declares what the file is and that the browser could not display it, and
offering the same action the footer carries. Turning the page tries afresh: the
refusal is remembered per page, not for the document. A portrait drawn from a
remote address falls back to the silhouette on the same refusal, on every
surface that draws one — pedigree card, search result, person header — rather
than to a broken-image glyph.

A remote page is offered as a link to its own address, opened in a new tab,
never as a download button: the bytes are somebody else's, fetching them from
here would make us a proxy for their bandwidth, and whether the transfer is
even allowed is their CORS policy's decision rather than ours. The whole
document still downloads as one archive, where such a page travels as a `.url`
shortcut — see [API](api.md).

Every viewer action, identification included, is available over a page held only
as a remote URL. The server cannot cut a region out of such a page — cutting
means re-decoding our own copy — so it sends the whole picture with the
rectangle to take out of it and the client does the cutting, in one shared
component used by every portrait and crop: an `svg` whose `viewBox` is the
rectangle and whose `preserveAspectRatio` is `xMidYMid slice`, which frames the
region exactly as `object-fit: cover` frames an image. CSS that sizes a portrait
therefore has to reach both elements — `.thing img, .thing svg` — since which
one is drawn depends on where the pixels are.

Cutting needs the picture's pixel size, and nothing here has ever opened that
file: the cropper measures it in the browser and records it on the page the
first time somebody identifies a person there. Until then a region of that page
has no scale to be cut at, and the whole picture is shown, marked as a region by
the crop badge.

The viewer and the edit panel describe the **page** on screen, not the document
above it: its format, dimensions, size, and whether the file is stored, remote,
or held by nobody. A remote page is rendered from its URL exactly as a stored
one is rendered from our copy, and the panel's URL field edits that page's
address. Only a row that names a file offers the field; a document names none.

#### Adding a document

There is exactly one way to add media, because there is one kind of thing to
add. A single photograph and a forty-page notarial act are the same record: a
document row that describes it and page rows that hold the files. An editable
gallery therefore ends in one **Add a document** cell, never in a separate
quick-upload cell beside a multi-page one — the second would be the first with
its fields withheld, which is how a photograph ends up with no date, no place,
and no kind while the register beside it has all three.

That cell opens `DocumentForm`: the document's own fields — title, description,
tags, kind of record, physical medium, privacy, date, place, note, and the
events it documents — followed by the page list, which sits last, immediately
above Save. The fields describe the document; the pages are the document, and
assembling them is the last thing done before committing.

A page is either a file or an address somebody else serves, and the two may be
mixed in one document. Both are added the same way, from two cells at the end of
the page grid: the shared upload cell, and a link cell drawn identically that
opens an address field when clicked. An address stays editable afterwards — a
remote page carries a pencil action that reopens the same field on that page —
because a mistyped URL is found after it has been added far more often than
while it is being typed. Pages may be reordered and removed before saving.

Nothing is written until the user saves. Cancelling issues no request, so
closing the form cannot leave an unnamed empty document attached to somebody.
Saving creates the document, writes its pages in list order, applies the
metadata, tags, note, and event links, and attaches the document to its owner
last. A failure at any step after creation purges the document — which takes
its pages, tags, notes, and links with it — so the gallery never shows a
half-written record. Save is refused while the page list is empty.

Every editable gallery is this same one. Person forms and couple forms both
render it as a **Media** section at the bottom of the form body, above the
delete action; a couple's papers are the same kind of thing as a person's, and
reaching them through a separate header button made them look like a different
feature. Person profiles, which have no form, open `DocumentForm` directly from
the compact `+` action beside **Media**. Event editors embed the gallery scoped
to the event rather than rendering controls of their own.

The shared viewer uses the app's sans-serif body typography throughout its
compact facts column, with readable secondary labels rather than monospace
metadata. Relation pagination uses one horizontal previous/range/next row
below a bounded five-item list, not tiny vertical scroll arrows. These controls
reuse the document pager styling and have localized accessible names, visible
focus, and live range announcements. Short lists do not reserve five empty rows.

One download control serves every media kind and document ZIPs. Media and GEDZIP
exports share a transfer implementation behind the typed client. On desktop,
response chunks are written to a temporary file alongside the chosen destination;
the destination is replaced only after a successful transfer. Failures and
cancellation discard temporary data and preserve any existing destination.

On the web, the save picker is opened during the initiating click, before any
network awaits. Browsers supporting `showSaveFilePicker` pipe the response stream
to the selected file with backpressure. Other browsers use `Response.blob()` and
a local blob URL; this fallback still buffers the complete download in the
browser and is not a bounded-memory path for large archives. Neither path
copies file contents into WASM or serializes them as numeric JSON arrays. The
typed client's browser transport uses Fetch internally; backend URLs never
become navigation targets. Picker cancellation does not fetch a file. Errors
are localized, and success is reported only after the transfer completes.

Download availability is independent of preview
success. File/page and all-pages ZIP labels are distinct, and the footer wraps
within narrow viewports. The detailed viewer contract is in
[Person Profile, Media Gallery](ui-person-profile.md#7-media-gallery).

### 4.6 EventIcon

One component maps event types to an icon and semantic token. Every icon has an
accessible localized label and is never the only representation of event type.

### 4.7 EmptyState

Used only for genuinely empty content, with an optional icon, localized title,
localized explanation, and one relevant action. Loading and error states never
reuse the empty state.

### 4.8 ContextMenu

One shared context menu implementation serves tree cards, person cards, media,
and vignettes. It supports keyboard navigation, focus restoration, viewport
collision handling, disabled actions, separators, and destructive styling.

### 4.9 Theme picker

A grid of tiles used wherever a theme is chosen: the application palette and
the pedigree chart style both use it, and any later one must. Each tile holds a
swatch, a name, and optionally a hint line or a tag; the active tile carries an
orange border, the selection background and `aria-pressed`.

Only the swatch differs between uses — a painted miniature for a palette, an
SVG card pair for a chart style — and both occupy the same box so the pickers
line up. Tracks use `auto-fill`, so tile size does not depend on how many
themes happen to exist.

## 5. Accessibility

- All controls have programmatic labels; icon-only actions have localized
  accessible names and tooltips where needed.
- Keyboard order follows visual order. Focus is visible and restored after a
  modal, menu, or picker closes.
- Errors are linked to their controls and announced without relying on color.
- Dynamic status uses polite live regions; rapid progress ticks are not each
  announced.
- Motion respects `prefers-reduced-motion`.
- Text and controls maintain sufficient contrast in both themes.

## 6. Responsive behavior

| Breakpoint | Behavior |
|---|---|
| `>= 1200px` | Full desktop layout and 1200px reading width. |
| `900–1199px` | Reduced desktop layout and narrower tree panels. |
| `600–899px` | Tablet stacking, reduced cards, collapsible side areas. |
| `< 600px` | Single-column layouts; forms usually become full-screen while workflow modals retain the safe inset their specification defines. |

At widths below 640px, page padding becomes `16px 12px` and topbar padding
becomes `10px 12px`. Fixed-format controls define stable dimensions so labels,
loading states, badges, and hover actions cannot resize their containers.

### 6.1 Performance observability

Every routed screen starts a root `ui.<page>.load` trace cycle. Resources owned
by nested shared components, including forms, search, reference tooltips,
media viewers, thumbnails, document pages, vignettes, and crop tools, join the
route's cycle instead of creating unrelated roots. A component mounted without
a route context uses `ui.component.load` as its fallback root.

Each Dioxus resource has its own `ui.resource.load` child and a stable resource
name. The trace therefore shows which requests and local tasks run in parallel,
which resource gates completion, how long response-body reading and JSON
deserialization take, and how much time remains in synchronous computation and
render stabilization. Deliberate debounce resources are named explicitly so
their wait is not mistaken for server latency.

The cycle ends only after all active resources have settled and two browser
animation frames have elapsed. A later resource refresh starts a new cycle.
Instrumentation must not include genealogy, search text, raw URLs, resource
identifiers, filenames, or rendered content.

### 6.2 Layout invariants

Responsive behavior is based on the space actually available to a component,
including space left after fixed sidebars, rather than on the viewport width
alone. Every page, card, grid track, form row, and toolbar remains bounded by
its containing block. Grid minimums are capped by the available width, flex and
grid children are allowed to shrink, and long user content wraps. A page-level
`scrollWidth` that matches the viewport is not sufficient if a child still
extends beyond its visible container.

Narrow layouts preserve information and direct access before preserving the
desktop arrangement:

- Complete names, places, lifespan years, relationship text, and other primary
  content wrap or move to a full-width row before being truncated or hidden.
- Identity markers stay grouped with the identity they qualify. Actions may
  move to a separate row so they do not separate a badge from its name.
- Familiar actions may become stable square icon buttons when their labels no
  longer fit. They retain the same behavior, localized accessible name, visible
  focus treatment, and a tooltip where the icon alone may be ambiguous.
- A small, finite set of choices remains directly visible and wraps when
  necessary. Horizontal scrolling is reserved for navigation strips whose
  items must stay on one compact line; it is not used to hide alphabetic or
  similarly bounded choices.
- Dynamic instructions, progress, validation, and measurements reserve a
  stable status area when they replace one another, so adjacent content does
  not move between states.

Modal content must never sit beneath a persistent application bar. Form modals
may use the documented full-screen mobile treatment. Longer workflow modals may
instead use a viewport-bounded inset surface: fixed header, tabs, and footer
remain reachable while only the body scrolls. The owning workflow specification
defines its safe offsets and margins.

### 6.3 Shared left icon sidebar

`TreeIconSidebar` has one responsive implementation shared by every page that
renders it, including the pedigree, person profile, search results, dictionary,
and settings pages. Pages must not override these dimensions independently.

| Viewport | Sidebar width | Icon buttons | Separators | Padding / gap |
|---|---:|---:|---:|---:|
| Above 400px | 46px (`--sb`) | 34x34px | 28px | 6px / 2px |
| 400px and below | 36px (`--sb`) | 30x32px | 24px | 4px / 1px |

The compact state keeps the same 16x16px icons, order, actions, accessible
names, active state, and tooltips. The sidebar remains visible and fixed-width;
only its horizontal footprint and vertical spacing change.

### 6.4 Pedigree events sidebar

Above 600px, the right events sidebar has these rules:

- Its default width is a fixed 275px, until the reader resizes it.
- Its left edge has an 8px pointer target with a 2px visible resize handle.
- Resizing is constrained to 220-640px, and never beyond 45% of the space
  remaining after the left icon sidebar.
- A resized panel is stored locally as a ratio of that remaining space and stays
  proportional when the application window is resized, still bounded by the
  220-640px range.
- The focused handle supports the left and right arrow keys in 16px steps.
- Releasing the handle invokes the pedigree's existing fit-to-viewport path;
  it does not introduce a separate graph or zoom calculation.
- The collapse toggle remains available. The user's manual open/closed state
  and selected width are remembered independently.

The responsive states are:

| Viewport | Events sidebar behavior |
|---|---|
| Above 600px | Uses the remembered open/closed state and stored ratio; the resize handle is available while open. |
| 401-600px | Automatically collapses on initial load or when crossing the 600px threshold; the resize handle is hidden, but the toggle can reopen the panel. |
| 400px and below | Hidden entirely, including its toggle and resize handle; the pedigree canvas reclaims the full available width. |

Automatic collapse does not overwrite the stored manual preference or stored
width. Hiding the sidebar below 400px likewise preserves both values for a
later wider viewport. Every real window-size transition continues through the
existing debounced pedigree resize handler so graph fitting and zoom behavior
remain consistent.
