---
type: "UI Specification"
title: "Visual & Functional Specifications — Common UI"
description: "Shared layout, navigation, design tokens, components, accessibility, and responsive behavior."
tags: [oxidgene, specification, ui, ux, design-system]
timestamp: 2026-08-26T00:00:00Z
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
- CSS custom properties are defined in
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

The light theme is the CSS default. On first use, the app follows
`prefers-color-scheme`; `:root.dark` overrides tokens for dark mode. The choice
is stored as `oxidgene-theme` and changes at runtime from app settings.

### 3.1 Colors

| Token | Light | Dark | Purpose |
|---|---|---|---|
| `--bg-deep` | `#f4f2ee` | `#0d0f14` | Page background |
| `--bg-panel` | `#ede9e2` | `#111318` | Panels and topbars |
| `--bg-card` | `#ffffff` | `#16191f` | Cards and inputs |
| `--bg-card-hover` | `#f5f3ef` | `#1c2030` | Hovered cards |
| `--border` | `#d4ccc0` | `#252d3d` | Borders and dividers |
| `--border-glow` | `#e07820` | `#e07820` | Focus border |
| `--sel-bg` | `#e8e0d4` | `#192038` | Selection |
| `--connector` | `#a0937f` | `#2e4a6a` | Pedigree connectors |
| `--nav-bg` | `rgba(244,242,238,0.92)` | `rgba(10,11,13,0.92)` | Navbar |
| `--text-primary` | `#1e1a14` | `#ddd8cc` | Primary text |
| `--text-secondary` | `#5c5447` | `#7a8da8` | Secondary text |
| `--text-muted` | `#9e9488` | `#404f65` | Placeholder and disabled text |
| `--color-danger-text` | `#dc2626` | `#f87171` | Destructive text |

Theme-independent accents:

| Token | Value | Purpose |
|---|---|---|
| `--orange` | `#e07820` | Primary actions and focus |
| `--orange-light` | `#f5a03a` | Primary hover |
| `--green` | `#4ea832` | Birth and success |
| `--blue` | `#4a90d9` | Death and information |
| `--pink` | `#c4587a` | Female indicator |
| `--color-danger` | `#e05252` | Destructive actions |

Semantic aliases map generic component names to these core tokens:
`--color-bg`, `--color-surface`, `--color-primary`,
`--color-primary-hover`, `--color-text`, `--color-text-muted`, and
`--color-border`.

### 3.2 Typography and sizing

| Token | Value | Usage |
|---|---|---|
| `--font-heading` | `'Cinzel', Georgia, serif` | Brand and headings |
| `--font-sans` | `'Lato', sans-serif` | Body, controls, and metadata |
| `--sb` | `46px` | Tree icon sidebar |
| `--evw` | `275px` | Tree events panel |
| `--radius` | `8px` | Cards, buttons, inputs, modals |

Reference type scale: page title `1.3rem`, section heading `1.05rem`, card
title `0.95rem`, body `0.85rem`, metadata `0.78rem`, small text `0.72rem`, and
badge text `0.65rem`. Spacing follows 4, 8, 12/16, 20/24, and 32px steps.

### 3.3 Elevation and interaction

- `--shadow-sm`: cards and dropdowns.
- `--shadow-md`: modals, popovers, and the navbar.
- Buttons use card background and border by default, orange focus/hover, solid
  accent for primary actions, and danger tokens for destructive actions.
- Inputs use card background and border, orange focus, red validation state,
  and `0.5` opacity while disabled.
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

### 4.5 MediaInput, MediaGallery, and MediaManagerModal

The canonical upload cell accepts clicks and drag-and-drop and reports
per-file progress. Files are processed through the same upload API regardless
of entry point. The canonical gallery owns tiles, viewer opening, edit actions,
document paging, portraits, and context menus. Pages do not implement alternate
media grids.

`MediaManagerModal` is the only editable container for a person's or family's
gallery. It wraps the canonical `MediaGallery`, saves every media mutation
immediately, and closes independently of person and couple forms. Its owner is
explicitly a person or family; its event options allow the same media to be
linked as evidence without embedding a second gallery in an event editor.

Person profiles open it from the compact `+` action beside **Media**. Couple
forms open it from the fixed header. Person forms, embedded person blocks, and
event editors never render their own upload or media-management controls.

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
| `< 600px` | Single column, full-screen modals, compact icon actions. |

At widths below 640px, page padding becomes `16px 12px` and topbar padding
becomes `10px 12px`. Fixed-format controls define stable dimensions so labels,
loading states, badges, and hover actions cannot resize their containers.