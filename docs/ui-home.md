---
type: "UI Specification"
title: "Visual & Functional Specifications — Homepage"
description: "Tree dashboard with tree cards listing recently modified persons, search and sort, and the create and delete modals."
tags: [oxidgene, specification, ui, ux]
generated: { by: claude-code/claude-opus-5-5, at: 2026-09-29T16:45:00Z }
---


# Visual & Functional Specifications — Homepage

> Part of the [OxidGene Specifications](index.md).
> See also: [Tree View](ui-genealogy-tree.md) · [Settings](ui-settings.md) · [App Settings](ui-app-settings.md) · [Data Model](data-model.md) (Tree entity, change history) · [API Contract](api.md) (Trees and Persons endpoints)

---

## 1. Overview

The homepage is the user's personal workspace. It lists all their genealogy trees and provides access to tree-level actions. There is no marketing or onboarding content — it is a productivity-focused interface.

---

## 2. Layout

```
+----------------------------------------------------------------------+
|                           NAVBAR                                      |
+----------------------------------------------------------------------+
|                                                                       |
|   Page title                                          [Gear icon]    |
|   Subtitle                                                            |
|                                                                       |
|   [Search ____________]   [Sort v]   [Grid] [List]   [+ New tree]   |
|                                                                       |
|   +------------+  +------------+  +-------------------+              |
|   | Tree card  |  | Tree card  |  | + Create a new    |              |
|   |            |  |            |  |   tree             |              |
|   +------------+  +------------+  +-------------------+              |
|                                                                       |
+----------------------------------------------------------------------+
```

Content area: `max-width: 1200px`, centered horizontally, responsive padding.

---

## 3. Navbar

Minimal shared navbar, always visible at the top. See [Common UI](ui-common.md).

- Logo (`OxidGene.svg`) on the left, acts as a link to the homepage
- No navigation links in MVP
- **Future (post-MVP)**: user avatar, notifications icon, theme toggle in the navbar right zone

---

## 4. Page Header

Displayed below the navbar, above the toolbar.

- Page title: "My **Genealogy Trees**" (Cinzel font, large). The accent word is styled in orange
- Subtitle: "Explore, enrich and share the history of your family lines."
- **Gear icon button** (top-right of header): links to [App Settings](ui-app-settings.md) (`/settings`)

---

## 5. Toolbar

Single row below the page header. Contains from left to right:

**Search box** — single input with a magnifying glass icon, placeholder "Search a tree...". Filters the grid in real time as the user types. Matches on tree name and description.

**Sort selector** — dropdown with options:
- Recently modified (default)
- Name A -> Z
- Name Z -> A

**View toggle** — two icon buttons: grid view (default) and list view. In list view, `grid-template-columns` collapses to a single column.

**"+ New tree" button** — rightmost element. Visually prominent: orange gradient background, Cinzel font, white text, subtle shadow. Opens the new tree modal on click. Always visible regardless of the number of existing trees.

---

## 6. Tree Card

Cards are displayed in a responsive grid (`minmax(280px, 1fr)`). The last card in the grid is always the "+ Create a new tree" placeholder card.

### Anatomy (top to bottom)

The card opens straight on its padded body; there is no illustration above it.

1. **Header row** — tree name (Cinzel, bold, uppercase) on the left; three-dot menu button (vertical dots) on the right. It is the top of the card.
2. **Description** — one line, truncated, when the tree has one.
3. **Recently modified people** — a small uppercase muted label, then the five persons of the tree modified most recently, newest first, each drawn by the shared quick-search row on a single line: a small portrait (or the sex silhouette), the name, truncated when too long, and the dates with their precision. The relatives and birth place the [search results](ui-search-results.md) add are left out, so the five persons fit the card. The rows are static: no hover wash — the card is what reacts to hover — only the hovered row's name turns orange. A tree with nobody to list shows the label and "No person modified yet"; nothing is shown while the list loads or when it cannot be read.
4. **Footer row** — "Modified X ago" date on the left, followed by an optional "Recent" badge (shown for trees modified within the last 24 hours); the Open link on the right. The list above grows, so the footer sits at the bottom of a card stretched to a taller neighbour's height.

"Modified" for a person is what the [change history](data-model.md#5-change-history) records: a write that stored a new version of them — a change to their names, events, notes, citations or unions. Imports and the history baseline version every person at once and are left out, so a tree nobody has edited since its import lists nobody. Deleted persons are left out. The list is one `GET /trees/{id}/persons/recently-modified?limit=5` per card, then one portrait request for its rows (see [API Contract](api.md#persons)).

### Card interactions

| Action | Behavior |
|---|---|
| Click anywhere on card | Navigates to the tree view (`/trees/{id}`) |
| Click the Open link | Same destination; it is the keyboard-reachable control for it |
| Click a recently modified person | Navigates to the tree view centred on that person (`/trees/{id}?person={person_id}`); the click does not reach the card |
| Click three-dot menu | Opens the card menu (see below), does not propagate |
| Click a menu entry or outside an open menu | Runs that entry, or dismisses the menu; never also opens the tree |

An importing card takes no clicks at all.

### Card states

| State | Visual |
|---|---|
| Default | Neutral border, subtle shadow |
| Hover | Orange border, lifted shadow, raised 4px — by its `top`, never a `transform`, which would make the card the containing block of its fixed action menu and open the menu offset by the card's position |
| New (< 24h) | Green "Recent" badge in footer |
| Importing | Full-card translucent overlay, activity indicator, and localized “Import in progress” status |

An importing card is inert: it renders neither the Open link, nor the three-dot
menu, nor the recently modified persons, including for keyboard navigation; the
persons are read once the import is over. The state comes from the backend's
active job registry through `GET /trees`, so reloading the page cannot expose a
tree between database persistence and projection completion. While at least one
card is importing, the home page refreshes the tree list once per second; it
stops polling and restores the card only after the job becomes terminal.

### Create-a-tree card

The last card in the grid. Dashed border, centered `+` icon in a green circle, "Create a new tree" label, and a subtitle. Clicking it opens the new tree modal (same as the "+ New tree" button).

---

## 7. Three-Dot Menu (vertical dots)

Appears in the top-right of each card. Opens a dropdown with:

- **Open** — navigates to the tree view
- **Rename** — inline rename or modal
- **Duplicate** — creates a copy of the tree
- **Import** — opens the GEDCOM import flow for this tree
- **Settings** — navigates to tree settings (`/trees/{id}/settings`)
- **Delete** — destructive action, always shown in red with a tinted hover,
  requires confirmation

The dropdown closes on outside click or Escape.

---

## 8. New Tree Modal

Triggered by the "+ New tree" button or the create-a-tree card. Centered overlay with blur backdrop.

**Fields:**
- Tree name (required) — text input, auto-focused on open
- Description (optional) — text input, placeholder "Origins, region, period..."

**Actions:**
- Cancel — closes modal, clears fields
- Create — validates name, adds tree to the list, closes modal

Keyboard: Escape closes the modal.

---

## 9. Empty State

When the search filter yields no results:

- Centered within the grid area
- Tree icon in a rounded container
- Title: "No tree found"
- Subtitle: "Try a different search term."

When the user has no trees at all (first login), a different empty state encourages creating the first tree, with an icon and a "Create your first tree" button.

---

## 10. Responsive

- Content max-width: 1200px, padding: `3rem 24px 5rem`
- Below 640px: padding reduces to `2rem 1rem 4rem`; the search field takes the
    first toolbar row, while the full-width sort selector and compact icon-only
    new-tree action share the second row
- Below 640px: cards use one shrinkable column and never exceed the available width; a person row truncates its name and relatives rather than widening the card
- Topbar navigation collapses (future, post-MVP)

---

## 11. Design Tokens (reference)

See [Common UI §3](ui-common.md) for the full token list. Key tokens used on this page:

| Token | Purpose |
|---|---|
| `--bg-deep` | Page background |
| `--bg-card` | Card background |
| `--bg-card-hover` | Card hover background |
| `--border` | Card borders |
| `--orange` | Primary accent (buttons, hover borders, title accent) |
| `--green` | "Recent" badge, birth dates |
| `--text-primary` | Card titles, body text |
| `--text-secondary` | Metadata, subtitles |
| `--text-muted` | Dates, placeholders |

Typography: **Cinzel** for titles and branded elements. **Lato** for body text.
