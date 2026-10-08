---
type: "UI Specification"
title: "Visual & Functional Specifications — Common UI"
description: "Shared layout, navigation, design tokens, components, accessibility, and responsive behavior."
tags: [oxidgene, specification, ui, ux, design-system]
generated: { by: claude-code/claude-opus-5-5, at: 2026-10-08T10:30:00Z }
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
  i18n key present in every language's table.
- Documentation, screenshots, tests, fixtures, and examples use fictitious,
  anonymized people, trees, accounts, places, and archive references.
- Colours, typefaces, corner radii, shadows and density come from the active
  theme (§3.1), never from literals in a stylesheet. The scales derived from
  them, the component dimensions, and every style shared by more than one
  page are defined in `crates/oxidgene-ui/src/components/layout.rs`
  (`LAYOUT_STYLES`). Pages keep only their own layout and reach sizes,
  spacing, radii, shadows and families through the tokens of §3.3 and §3.4.
  Hairline offsets (`1px`, odd pixel nudges) and the geometry of the chart
  canvases and of print, which Rust computes or paper fixes, stay literal.
- Removing a component or state also removes its CSS selectors, translations,
  tests, and obsolete API calls.

## 2. Shared page layout

The application has two top-level bars:

1. `app-nav`: the branding navbar, displayed on the homepage and the
   application settings.
2. `td-topbar`: the contextual breadcrumb and actions on every other page,
   and under the navbar on the application settings.

The first of them carries the back and forward buttons ([§2.4](#24-back-and-forward)).

Non-pedigree pages use `sub-page` with a scrollable `sub-page-content` area,
centered at `max-width: 1200px` with `24px` padding. The pedigree owns its
canvas layout and side panels. The homepage uses the same reading width without
the contextual topbar.

### 2.1 Navbar

- Compact, approximately 48px high, full width, in normal document flow.
- Background: `var(--nav-bg)` with `backdrop-filter: blur(12px)`.
- Bottom border: `1px solid var(--border)`.
- The back and forward buttons ([§2.4](#24-back-and-forward)), then the
  OxidGene logo linking to `/`, are its only MVP content.
- Future account, notification, and global navigation controls must not be
  documented as current behavior until implemented.

### 2.2 Contextual topbar

- Compact, approximately 40px high, full width, `10px 16px` padding.
- Transparent background and `1px solid var(--border)` bottom border.
- Left zone: the back and forward buttons on a page without the navbar
  ([§2.4](#24-back-and-forward)), home logo, linked tree name, separator,
  localized current page.
- Right zone: page-specific search or actions, then the print action
  ([§7](#7-printing)) on every page that prints.

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

### 2.4 Back and forward

The application keeps the reader's path through its pages as a browser does,
and offers it on every page: the desktop window has no browser around it.

- **Buttons.** A back and a forward button lead the page's first bar: the
  navbar where the page shows it, else the contextual topbar, before the
  logo. Each is an arrow icon button with a localized accessible name and a
  tooltip naming its shortcut and its list; one with nowhere to go is
  disabled. They do not print.
- **Several steps at once.** A long press (half a second) or a secondary
  click on either button, or **Down** while it has the focus, lists the pages
  that way, nearest first, on the shared contextual surface
  ([§4.8](#48-contextmenu)), which takes the focus on the nearest; choosing
  one goes there in one move, and **Escape** closes the list. Each page reads
  as its kind — its breadcrumb label, or *Home*, *Person*, *Couple* — then
  what it is about, as the page itself named it from the data it shows: the
  person a pedigree is drawn around or a profile shows, the couple, the person
  whose versions are compared, the dictionary tab and the entry whose usage is
  open, the search query, the tree on the pages about the tree as a whole.
  Nothing is fetched to name an entry: a page left before it loaded reads as
  its kind alone.
- **One history.** The reader's path is one model (`nav_history.rs`), kept in
  step with the router: a navigation adds a page and drops the pages ahead of
  the current one, a replacement rewrites the current page, and back, forward
  and a jump move within it. It holds the latest 50 pages, dropping the
  oldest. It stands in front of the history the router uses, the platform's,
  which still does the navigating.
- **Web.** The platform's history is the browser's, so the in-app buttons,
  the browser's buttons and history menu, Alt+Left and Alt+Right, and the
  mouse's back and forward buttons all move through the one browser stack.
  The application adds no shortcut of its own there — the browser's already
  moves, once — and its history follows the browser to the page it lands on,
  preferring the move it announced when the same page is found both ways. The
  browser tab keeps the titles and the place in them in its session storage,
  as it keeps its own history, so a reload finds them again.
- **Desktop.** The window's in-memory history does the navigating, and the
  application handles Alt+Left, Alt+Right and the mouse's back and forward
  buttons itself — except in a text field, list or editable area, where the
  keys keep their editing meaning.
- **What going back finds.** A history entry reopens its page as the reader
  left it. What the page is about is in its route: the pedigree names the
  person it is drawn around (`?person=`), replacing its route as the reader
  moves about the chart rather than adding pages, so going back to it reopens
  it on that person ([Tree View §1](ui-genealogy-tree.md#1-general-structure)). The view of a
  page — its tab, filters, page and open rows — is not an address: it is kept
  with the page's history entry, as a browser keeps a page's state, and taken
  up again when the reader comes back to that entry, while a page opened anew
  starts from its defaults. The [Dictionary](ui-dictionary.md#4-tabs) keeps
  its view this way. That view is held in memory, and a reload of the web page
  forgets it.

## 3. Design tokens

### 3.1 Themes

A theme sets the colours and, through a few presets, the typefaces, the
corner radii, the elevation and the density. Component geometry — the
structure of each page, its columns and fixed dimensions — is the same under
every theme: a theme changes how the application looks, not how it is
organized. Switching to a theme of another density does reflow text and
spacing.

Themes are JSON documents. The shipped set is whatever
`assets/themes/` holds, listed in the order
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
An id that no longer resolves resets the selected and stored value to `light`.

#### File format

```json
{
  "id": "sepia",
  "name": "Sepia",
  "base": "light",
  "colors": { "orange": "#8a5a2b", "bg-deep": "#f6f0e4" },
  "style": { "heading_font": "serif", "corners": "round" }
}
```

| Field | Rule |
|---|---|
| `id` | Lowercase letters, digits and `-`. Must not be a built-in id. |
| `name` | Shown in the picker, verbatim unless a translation key exists. |
| `base` | Optional. Id of a built-in theme to inherit from. |
| `colors` | Token name (without `--`) to colour. Unknown names are rejected. |
| `style` | Optional. Any of the presets below; the others come from `base`, or the defaults without one. Unknown properties and values are rejected. |

| Style property | Presets (default first) | Sets |
|---|---|---|
| `body_font` | `lato`, `cinzel`, `system`, `serif`, `mono` | `--font-sans` |
| `heading_font` | `cinzel`, `lato`, `system`, `serif`, `mono` | `--font-heading` |
| `corners` | `soft` (3/4/8/12px), `square` (0/2/3/4px), `round` (6/10/14/20px) | `--radius-xs`, `--radius-sm`, `--radius`, `--radius-lg` |
| `elevation` | `soft`, `flat` (borders, overlays only), `raised` | `--shadow-sm`, `--shadow-md`, `--shadow-lg` |
| `density` | `regular` (2px unit, text ×1), `compact` (1.75px, ×0.95), `airy` (2.3px, ×1.05) | `--space-unit`, `--text-scale` |

`lato` and `cinzel` are the bundled faces; `system`, `serif` and `mono` name
the platform's own interface, book and monospace fonts, so no theme makes the
application fetch a font. The shipped themes each choose presets that suit
the site or palette they are named after; `light`, the default, keeps every
default.

Without `base`, every token in §3.2 must be given. With it, only the
differences need listing — which is how `dark` is written, and what keeps an
existing theme working when a token is added.

Colour values are hexadecimal only: `#rgb`, `#rgba`, `#rrggbb` or
`#rrggbbaa`, and style values are preset names only. A theme file is user
input that ends up inside a `<style>` element, so CSS functions, named
colours, free lengths and anything else are refused rather than passed
through.

Custom themes live in `<config directory>/themes/*.json` and are read by the
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

Two semantic aliases map generic component names to core tokens: `--white`
(`--on-accent`) and `--shadow-black` (`--shadow`). Every other rule reads the
core tokens directly; a page or component stylesheet never adds an alias of
its own, and no rule reads a token no theme or stylesheet defines.

### 3.3 Typography and sizing

| Token | Value | Usage |
|---|---|---|
| `--font-heading` | Theme preset, default Cinzel | Brand and headings |
| `--font-sans` | Theme preset, default Lato | Body, controls, and metadata |
| `--font-mono` | Platform monospace | Tokens, identifiers, code |
| `--text-N` | `N/100 rem × --text-scale` | Font sizes |
| `--space-N` | `N × --space-unit` (2px by default) | Padding, margins, gaps |
| `--radius-xs` … `--radius-lg` | Theme preset | Badges, controls, cards and modals, large panels |
| `--radius-pill` | `999px` | Pills and chips |
| `--sb` | `46px` | Tree icon sidebar |
| `--evw` | `275px`, then a ratio once resized | Tree events panel width |

Cinzel and Lato ship with the application: their Latin and Latin Extended
subsets live in `assets/fonts/` with their SIL Open Font License texts and are
embedded as `@font-face` rules over `data:` URLs (`FONT_FACES` in
`components/layout.rs`), so neither build requests a font from a third party.

The type scale has fourteen steps, `--text-65` to `--text-300`: badge text
`--text-65`, small text `--text-70`, metadata `--text-80`, body `--text-85`,
card title `--text-95`, section heading `--text-110`, page title
`--text-130`, display sizes above. Spacing uses the even steps of the
2px unit — 4, 8, 12, 16, 20, 24, 32px at regular density — with
`--space-1` to `--space-32` named after their number of units. A theme's
density scales both together.

### 3.4 Elevation and interaction

- `--shadow-sm`: cards and dropdowns.
- `--shadow-md`: popovers, menus, and the navbar.
- `--shadow-lg`: modal dialogs and other overlays.
- Accent glows and focus rings mix the accent colour and are not elevation.
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
`var(--danger)`. It is drawn in the shared `Modal` ([§4.13](#413-modal)):
`Escape` and backdrop press cancel unless an operation is already running.

### 4.2 PersonPicker

The one picker of a person (`components/person_picker.rs`), used by the
[settings](ui-settings.md)' SOSA root and *Who am I?* cards and by the two
ends of the [kinship](ui-kinship.md) page. It displays the selected person
with the same canonical summary as each person-search result: profile photo
or sex-specific placeholder portrait, surname and given names, birth and death
years, and birth place when known — the caller passes that row, a plain
summary or a link to the profile. The row and its buttons sit in one frame;
below 768px the buttons move under the row. **Change** opens the shared
person search in its place, and cancelling it keeps the current person;
**Clear** is offered only when the field is optional, and an optional field
with nobody chosen says so beside a choose button. A required field with
nobody chosen opens on the search. It receives `tree_id`, the selected row,
the labels, and an `EventHandler<Option<Uuid>>` (`None` on clear).

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
with a localized inline error. The tree's entry options
([Tree Settings §10](ui-settings.md#10-section-entry-options)) set the order of
the day, month and year fields, their separators, whether a Gregorian or
Julian month is typed or picked by name, and the calendar an empty field
starts in.

Display formatting uses the shared date formatter; year-only surfaces use
`qualified_year()` so precision is not discarded. The formatter writes in the
date style of the tree being read — its format, « circa », display calendar
and lifespan symbols ([Tree Settings §9](ui-settings.md#9-section-date-display))
— which the interface's translation helper carries beside the language, so no
page formats a date on its own. The formatter:

- reads a GEDCOM date as day, month, year, the year being its number of four
  digits or more, else its last number, so a short year after a month reads
  as a year (« BRUM 8 », « COMP 7 », a Julian « MAR 850 »);
- writes a Republican year in Roman numerals, as the calendar's own records
  do: « an VII », « 18 brumaire an VIII »;
- follows a date written in another calendar than the tree's display
  calendar (Gregorian by default) by its equivalent there, in parentheses:
  the day (« 15 mars 1582 (25 mars 1582) »), or, for a year or a month alone,
  the span it covers (« an VII (entre 22 sept. 1798 et 22 sept. 1799) »). A
  date already in that calendar, a range, a free-text phrase and a month the
  year lacks (Adar II in a common Hebrew year) carry none.

A sentence that reports a date joins it the way its precision allows: « le »
and the day for a full date (« Né le 8 déc. 1776 »), « en » for a year or a
month alone (« Marié en an VII »), and nothing but the date for a qualified
one, whose own word leads it (« Né vers 1776 », « entre 1800 et 1810 ») — in
each interface language its own way (`date.in`).

### 4.4 Fields with suggestions

Every free-text field that suggests values is drawn by the one shared
`SuggestInput` (`components/suggest_input.rs`). Suggestions are helpful,
never restrictive: free text is always accepted.

- **List.** The list opens under the field while it has the focus and there
  is something to suggest. Each row reads a name, then muted details. It
  scrolls past half the window's height. Arrow keys move through the list,
  keeping the highlighted row in view, Enter picks, Escape closes it.
- **Matching.** A value is suggested when one of its words starts with the
  typed text, ignoring case, accents and punctuation; ligatures and letters
  such as `ł` or `ß` read as their plain spelling (`l`, `ss`). The place field
  matches the tree's places in the interface and the dictionaries are
  matched on the server, both with the one folding of
  [Cross-cutting Rules §3.6](cross-cutting.md). The backend is asked
  after a 300 ms pause, and only about what the user typed: a form opening
  on a filled field asks for nothing.
- **Layout.** The list opens in place under the field, not as a
  context-menu layer: these fields live in dialogs, which sit above those
  layers.
- **Setting.** A tree whose [Entry suggestions](ui-settings.md) are off
  suggests nothing, and its fields ask the backend for nothing.

#### Place fields

Every place field is the shared `PlaceInput` (`components/place_input.rs`):
event places in the person and couple forms, and document and media places.

- **Suggestions.** One list, with no groups: the tree's places with a word
  starting with the text come first, most used first (the `places` value
  suggestions, asked of the server: no form reads the tree's whole place
  list); from three characters, the built-in
  [place dictionary](place-dictionary.md) follows, best match first. Every
  row reads alike: the place's name, then the rest of its label (code,
  département or county, region, country), and for the dictionary the year
  a former commune or name ended. Every filing of a place is offered, today's
  and former ones (a region before 2016, a former département name); a
  dictionary label the tree already holds is offered once, as the tree's
  place.
- **Picking.** Picking a tree place or a dictionary place fills in its name
  or label. A field opened on a record's place shows that place's name — the
  form reads the names of the places its record sits on, by id — and editing
  the text drops that link.
- **Saving.** Nothing is written while the form is open. On save, a linked
  place is used as is; text becomes the tree's place of the same name,
  trimmed and ignoring case, which the server finds (`GET /places?name=`),
  and that place is created when the tree has none. A place created from a
  dictionary label takes the dictionary's coordinates. A typed source title
  is resolved the same way (`GET /sources?title=`).

#### Name, occupation and source fields

The shared `ValueInput` suggests what a field holds across the tree, from
the [value suggestions](api.md) endpoint:

| Field | Suggests |
|---|---|
| Surnames: birth name, a name's surname, a surname information | The tree's surnames |
| Given names: birth given names, a name's given names, a given-name information | The tree's given names, then the given-name sheets' names |
| Occupations: a profession, an occupation event's description | The tree's occupations, then the occupation sheets' terms |
| Sources of the person, of an event, of a union event | The tree's source titles, up to 50: titles often share a long head, so the list shows as many as the API returns |

- **Rows.** A row reads the value, its count and the sheet badge, spaced
  apart, the badge at the right edge. The tree's values come first, those starting with the text, then
  the most used, each with the number of persons carrying it (citations for
  a source). The terms of the [reference sheets](api.md)
  follow, in any language's spelling, the interface language's first. A
  row a sheet explains, from the tree or not, carries an `info` badge.
- **Given names** are completed word by word: in `Jean Ma`, the list
  suggests for `Ma`, and picking replaces that word only.
- **Picking** fills in the value as the tree or the sheet writes it, never a
  sheet's label: an occupation is entered as the record names it. A surname
  field writing in capitals — while the tree's automatic uppercase for
  surnames is on — capitalizes what it picks, and lists each spelling once.
- **Search filters.** The surname, given-name and occupation criteria of
  [Search Results](ui-search-results.md), the relatives' included, use the
  same field but list only the tree's values: a term no record carries
  would find nobody.
- **Topbar search.** The [tree view](ui-genealogy-tree.md) search fields
  list the same surnames and given names, the tree's only, as the first level
  of their suggestion panel, above the matching persons. Each counts only the
  persons the other field also finds, so a count is what the search would
  then find. The panel is a fixed overlay, since the topbar clips its
  overflow, but its name rows are drawn and picked as here.

Canonical display is comma-separated from the most specific to the least
specific unit, ending with the country, but the number of levels varies by
country. Documentation examples use placeholders rather than real addresses or
archive locations.

### 4.5 MediaInput, MediaGallery, and DocumentForm

The canonical upload cell accepts clicks and drag-and-drop and reports
per-file progress. Files are processed through the same upload API regardless
of entry point. It also has a deferred mode in which it uploads nothing and
hands the chosen `(file name, bytes)` pairs to its caller; that mode exists for
`DocumentForm`, which has no document to hang pages off until the user saves.
The canonical gallery owns tiles, viewer opening, edit actions, document
paging, portraits, and context menus. Pages do not implement alternate media
grids. A listing of documents that are nobody's gallery — the
[Dictionary](ui-dictionary.md)'s Media tab — uses the same module's library
grid: the same tiles, bundle and viewer, the tiles read-only with optional
footnote lines under the caption. With no attachment to spare, the viewer's
delete there only ever removes a document nothing references.

The initial grid loads tile thumbnails, the first four document-page previews
— a generated thumbnail, or for a remote page its thumbnail address when its
server serves one (`thumbnail_url`) and its own image URL otherwise —
vignette crops, and
linked event ids through one bounded gallery bundle. It
must not mount one image, page-list, or reverse-link resource per tile. Larger
sets are split into as many batches of 1,024 identifiers as necessary. Viewer
and editor panels may use an individual endpoint after the user opens one
asset; a document's page list in its editor resolves every page's thumbnail
in one request too.

On the web, where pictures are fetched and inlined as `data:` URLs, a screen
asks for all of its pictures at once, each distinct address once. The client
keeps those it fetched for the session, keyed by tree and address, so a page
visited again draws them without downloading them: at most 24 MB, the oldest
dropped first, each reused for ten minutes, and a tree's dropped whenever the
client writes to it, as its cached reads are. The desktop serves pictures from
its own origin and needs none of this.

A tile draws its document's page previews when there are any, its own
thumbnail when the tile is a stored page, and the address when it is a remote
image — its thumbnail address when it has one, here and in the page lists of
the editors, so a tile of an archive's view loads a few kilobytes rather than
the full view; only a tile with none of those falls back to a labelled file
icon. A
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

The viewer draws a picture on one shared stage (`components/media_stage.rs`):
fitted to the space, zoomed by the wheel and its controls, dragged when it
overflows, with the caller's overlays — identified regions — over it in
percentages of the picture. Moving to another page starts it fitted again.
Archive registers are shown by the archives' own portals
([Archive Portals §6](archives.md#6-display)), not here.

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

A caller that has already assembled a document opens the same form
prefilled rather than a form of its own: attaching an archive's cited views
— from the desktop's archive window or beside the cited source on the web
([Archive Portals §6.4](archives.md#64-attaching-and-cropping)) —
fills the title, description, kind of record, medium, the event checked, the
source the document is linked to, and one remote page per view, each with its
pixel size and thumbnail address. For such a register the page list also
offers the previous and the next view, each resolved on its click, and the
title and description follow the views until the reader writes them.

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
below a bounded five-item list, not tiny vertical scroll arrows: the shared
`Pager` ([§4.12](#412-pager)) without numbers, its range announced as a live
status. Short lists do not reserve five empty rows.

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

One component (`components/event_icon.rs`) maps event types to an icon and a
semantic tone (birth, death, union, other — the `--green`, `--blue` and
`--orange` tints of `.ev-ic`). Framed in its tone's badge it decorates an
event whose name is written beside it, as in the pedigree's events panel, and
is hidden from assistive technology. Bare, it marks a date on its own — the
birth and death years of person-search rows, search results and the tools'
lists (`✦ 1842 ✝ 1907`) — and is then named by the event type's localized name
(`role="img"`, `aria-label`, tooltip). Either way the event type is never
conveyed by the glyph alone.

### 4.7 EmptyState

The one empty state (`components/empty_state.rs`, `.empty-state`), used only
for genuinely empty content — no tree, no event, no match for a filter — with
an optional icon, localized title, localized explanation, and one relevant
action (clearing the filter, creating the first tree, choosing a root).
Loading and error states never reuse the empty state: they are written with
`.loading` and `.error-msg`. A page may give it a class of its own for
spacing, as the homepage does.

### 4.8 ContextMenu

One shared context menu implementation serves tree cards, person cards, media,
and vignettes. It supports keyboard navigation, focus restoration, viewport
collision handling, disabled actions, separators, and destructive styling.

Its surface (`ContextMenuSurface`) is also the one every anchored overlay uses,
the topbar search panel included. It closes on a click outside, on a context
click, and when the window is resized: it is placed at coordinates measured
when it opened, which a resize moves out from under it, so it closes as a
native menu does rather than float away from its anchor.

### 4.9 Theme picker

A grid of tiles used wherever a theme is chosen: the application palette and
the pedigree chart style both use it, and any later one must. Each tile holds a
swatch, a name, and optionally a hint line or a tag; the active tile carries an
orange border, the selection background and `aria-pressed`.

Only the swatch differs between uses — a miniature of the theme for the
application theme, an SVG card pair for a chart style — and both occupy the
same box so the pickers line up. The application swatch declares the custom
properties of the theme it shows on itself (`Theme::declarations`), so its
rules resolve `var(--…)` against that theme: it previews the theme's colours,
heading typeface, corners and shadow, not the active theme's. Tracks use `auto-fill`, so tile size does not depend on how many
themes happen to exist.

### 4.10 VersionDiff

The one comparison of two versions of a record, used by the
[Person History](ui-person-history.md) page and the audit log of
[Settings §11](ui-settings.md#11-section-history). It takes the version shown
and the one it is compared with — absent for a record's first version — and
lays them out as one table per section, the older version on the left and the
newer on the right. Column headings read *Version N*, with *(current)* for the
live record.

- **Sections** follow the record: for a person Identity, Names, Events, Notes,
  Sources, Parents and Unions; a place, a source, or the tree's settings is one
  section of its fields. Sections with nothing to show are omitted.
- **Groups** are the items of a section — one name, one event, one union and
  each of its events — paired across the two versions by identity, never by
  position, so a removed item keeps its place in the list. A group is badged
  *Added*, *Removed*, *Changed* or *Unchanged*.
- **Rows** are fields, each value worded as the rest of the UI words it:
  enumerations translated, dates through `format_date`, witnesses, notes and
  citations joined into one cell. A reference — a place, a source, a relative —
  reads through the labels its own version recorded, so each side shows the
  name as it was then. Whether such a row changed is decided on the records it
  names, not on their labels: a place or relative renamed in between is not a
  change of this record.
- A changed row tints the old value with the danger colour and the new one with
  green; a removed group strikes its old values through. Colour never carries
  the change alone: the badge and the two values say it too.
- **Changes only**, on by default, hides unchanged sections, groups and rows.
  With nothing left to show, the table reads *No difference between these two
  versions*.
- A deleted state opens with a banner saying so and has nothing to list.
  Compared against, it is empty: everything the newer version holds reads as
  *Added*.

Below `768px` the label column narrows and cells tighten; values wrap rather
than scroll.

### 4.11 Tree page frame

Every page of a tree but the pedigree, which owns its canvas layout, is drawn
in one frame, `ToolPageFrame`: the contextual topbar — breadcrumb ending in
the page's title, the page's own controls, and the heading it prints under
([§7.2](#72-printed-page)) — then the shared left icon sidebar
([§6.3](#63-shared-left-icon-sidebar)) beside the scrollable content. The tool
pages use the sidebar acting on their selected person; the person and couple
pages bring theirs, with their active view and their add-person action.

The tool pages load their tree through one hook, `use_tree_page`, which reads
it through the tree cache again whenever the cache is invalidated or the
route names another tree. While the tree loads, its cached copy names it, so
the breadcrumb never flashes empty. The person the sidebar acts on is the one
last shown in this tree during the session, else the tree's SOSA root.

### 4.12 Pager

One `Pager` moves through every paged list and document: search results, the
dictionary tabs, the media library, a document's pages in the media viewer and
the viewer's list of relations. It draws previous and next buttons and,
between them, the page numbers: both ends and two pages either side of the
current one, a gap standing for two pages or more (a single skipped page is
shown instead). The document viewer adds buttons to the first and last pages.
A list read through a cursor, which cannot jump to a page, shows *Page n of m*
— or its own range — as a live status between its two step buttons instead.

Every button has a localized accessible name and tooltip (*Previous page*,
*Page 4*…); the current page is marked `aria-current="page"`; a single page
draws no pager. The numbers scroll sideways rather than wrap; below `640px` a
list's pager keeps only its step buttons, while the document viewer keeps its
numbers, registers being cited by page. Pagers do not print.

### 4.13 Modal

Every dialog — confirmations, the person and couple forms, the import, the
merge wizard, the homonym choice, the print choice, the family-name editor and
the homepage's tree forms — is drawn in one `Modal`: a blurred backdrop and a
card that is a `dialog` with `aria-modal="true"`, named by its title.

- **Escape** and a **press on the backdrop** close it, through the same
  handler as its Cancel or close button, so a form with unsaved changes asks
  before discarding them. The backdrop reacts to the press, not the click: a
  selection started in the card and released outside does not close it.
- A dialog whose operation is running (`busy`) ignores both, and says so with
  `aria-busy`: an import or a merge cannot be abandoned half-way.
- A dialog whose question must be answered, or whose steps a stray press
  would throw away, ignores the backdrop and keeps Escape: the homonym choice
  (Escape is *Decide later*) and the merge wizard.
- The card takes the focus when it opens unless one of its fields already has
  it, so Escape works straight away; Escape inside an open list or picker
  closes that list first.

### 4.14 Tabs

One tab bar serves the Statistics and Tools pages, the Statistics records
lists and the Dictionary: a `tablist` of `tab` buttons, the selected one
marked `aria-selected` and highlighted. A page that remembers the tab a
viewer left it on (Statistics, Tools) reads it from the browser before any
tab mounts, so no tab asks for its data before the one shown; choosing a tab
stores it. On narrow screens the bar scrolls sideways rather than squeezing
its labels.

### 4.15 Year range

One "between two years" filter serves the search filters (born, died, any
event) and the media library (event years): two year fields under one
label, each named *From* and *To* for assistive technology. What is typed is
kept as typed until it reads as a year, so a half-typed year is never
rejected mid-keystroke.

### 4.16 ViewToggle

One list / grid switch serves the [homepage](ui-home.md)'s trees and the
[search results](ui-search-results.md): two icon buttons, list then grid, the
one shown pressed (`aria-pressed`, the orange fill of an active choice). Each
page names its own grid, whose cards differ, in the buttons' accessible names
and tooltips. Pressing the shown view does nothing.

### 4.17 Spinner

One spinner, `.spinner` (`SPINNER_STYLES` in `components/layout.rs`), marks
work that blocks what is behind it: the homepage's duplication overlay, a tree
card while its import runs, and the progress overlay of a desktop archive
window ([Archive Portals §6.1](archives.md#61-desktop)), which draws it over
the portal's page with the active theme's custom properties. It is a ring of
the border colour turning its accent arc, `--spinner-size` scaling ring and
all (48 pixels by default, 28 on a tree card). A busy button keeps its own
inline ring in the button's text colour (`.btn-spinner`), which turns with
the same animation. For a reader who asks for reduced motion it turns three
times slower.

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
renders it, including the pedigree, person profile, couple profile, search
results, dictionary, statistics, tools, and settings pages. Pages must not override these dimensions independently.

| Viewport | Sidebar width | Icon buttons | Separators | Padding / gap |
|---|---:|---:|---:|---:|
| Above 400px | 46px (`--sb`) | 34x34px | 28px | 6px / 2px |
| 400px and below | 36px (`--sb`) | 30x32px | 24px | 4px / 1px |

The compact state keeps the same 16x16px icons, order, actions, accessible
names, active state, and tooltips. The sidebar remains visible and fixed-width;
only its horizontal footprint and vertical spacing change.

Its last group leads to the pages of the tree as a whole — Dictionary,
Statistics, Tools, then Print and Settings — on every page of a tree. The
sidebar finds the tree from the page's route, so no page wires these
buttons. A button is never hidden on its own page: it shows as current
(highlighted, `aria-current="page"`) and pressing it does nothing. The views
of a person — profile, couple, pedigree — follow the same rule through the
sidebar's `active_view`.

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

## 7. Printing

Every page that shows the tree's content prints. The homepage, the tree
settings, the application settings and the not-found page do not: they are
controls, not content. The decision is `is_printable` in
`crates/oxidgene-ui/src/components/print.rs`, an exhaustive match on the
route, so a new route does not compile until it is made.

### 7.1 Print action

- One component, `PrintAction`, is a button of the shared icon sidebar (§6.3),
  just above Settings, so it sits in the same place on every page that
  prints: a printer icon, the localized accessible name *Print* and the
  tooltip *Print this page*. On a route that does not print — the settings
  page shares the sidebar — it renders nothing.
- Each printable page places a `PrintHeading` last in its contextual topbar
  (§2.2): the header its sheet prints under, invisible on screen.
- On the web it calls `window.print()`. On the desktop the shell installs a
  `PrintBridge` (the UI trait `PagePrinter`) that opens the platform's print
  dialog on the WebView itself — a WebKitGTK print operation on Linux, an
  `NSPrintOperation` on macOS, WebView2's dialog on Windows — because a
  WebView's `window.print()` cannot be relied on (WKWebView ignores it).
- Ctrl+P, or Cmd+P, presses the page's print button, so the shortcut and the
  button print the same thing. On a page without the button the key is left
  alone: a browser prints the page as it stands, the desktop does nothing.
- The browser's own Print command goes through the same stylesheet; nothing
  the action does before printing depends on the button having been pressed.

### 7.2 Printed page

- **Palette.** Paper uses the `light` theme whatever theme is on screen. Its
  colour block is emitted a second time for print media, after the active
  palette, so the stylesheet keeps reading theme variables and no colour is
  written for print. Backgrounds print exactly as designed, so badges, SOSA
  marks and chart fills stay legible on white; shadows and page glows do not
  print.
- **Header.** The topbar prints as the sheet's header: the page title as its
  breadcrumb names it (a person's or couple's name on their profiles, the
  query on search results, whose fields do not print), the
  tree name, and *Printed on* followed by the day in the reader's language,
  written as every calendar day of the interface is.
- **Hidden.** The navbar, the icon sidebar, the events panel, every topbar
  control, buttons, form fields, filter and sort toolbars, pagination
  controls, dialogs, context menus, tooltips and hover cards. Any other
  element that only serves the pointer — a chart's edit marker, say — carries
  the `no-print` class.
- **Flow.** Page-level scroll containers give way to the document flow, so a
  long page prints in full over as many sheets as it needs. Cards, table and
  list rows, figures and images are not split across sheets, headings stay
  with what follows them, and a table's header row repeats on each sheet.
- **Tabs.** A tab strip prints only its active tab, as the section heading.
- **Links** print as text. A link that leaves the application prints its
  address after it.
- **Pictures.** Portraits, thumbnails and galleries print as displayed.
- **Paginated lists** print the page on screen and never fetch the rest, so a
  printout is bounded by the page size. The pagination controls are replaced
  by *Page n of m*; printing another page means turning to it first.

### 7.3 Charts

A chart view draws only what its viewport reaches and pans and zooms with a
transform sized for the screen, so it cannot be reflowed onto paper. Instead,
just before printing — on the `beforeprint` event, and from the action before a
desktop dialog opens — the largest SVG in `.pedigree-viewport` is copied with
its `viewBox` narrowed to the area the reader sees (less any part the events
panel covers) and trimmed to what is drawn. Printing hides the live chart and
scales the copy to fit one landscape sheet of the chosen paper, as vectors.
What prints is what is on screen: zoom and pan first to choose it.

Any chart view drawn as one SVG in the pedigree viewport prints this way. The
copy is rendered outside that viewport, so a view's styles must not depend on
its container; custom properties set inline on its ancestors are carried
over. Identifiers inside the copy are renamed so its references do not point
into the hidden original. The copy is removed after printing.

**Over several sheets.** A chart all on screen prints on one sheet at once:
what the screen shows is then the whole chart. When part of it is off screen
and, at the zoom shown, it is larger than one sheet, the print action first
asks what to print: *What the screen shows* (one sheet, as above) or *The
whole chart at this zoom*, with the number of sheets it takes (*6 sheets
(3 × 2)*). Both are measured on what the chart draws, not on its canvas'
margins, with the chart drawn whole for the measure. The whole chart keeps on
paper the size it has on screen — 96 CSS pixels to the inch — cut into tiles
of 255 × 175 mm, row by row, each overlapping the next by 1 cm; when printing
it up to 15 % smaller takes fewer sheets, it is printed just that much smaller,
so that a chart a centimetre taller than a sheet does not leave a strip on a
second one. The tile size fits the printable
area of A4 and of US Letter in landscape within the sheet's 10 mm margins, so
nothing is lost in the 3–6 mm at the paper's edge that printers cannot reach.
On each sheet a dashed line, inside the printed area, marks where the next
sheet to the right and the next below start: the sheets are assembled by
cutting or aligning on it. Each sheet carries one caption line — the page, the
tree, *Sheet n of total · row r, column c* — in place of the printed header.
Beyond 50 sheets the option is offered disabled, with a note to zoom out. While
the sheets are prepared, every view draws its whole chart rather than only the
part near the viewport, so the copy holds all of it; the tiles are one hidden
copy shown through an SVG `use` per sheet, as vectors.
