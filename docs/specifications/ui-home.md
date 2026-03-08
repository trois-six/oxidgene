# Visual & Functional Specifications — Homepage

> Part of the [OxidGene Specifications](README.md).
> See also: [Tree View](ui-genealogy-tree.md) · [Settings](ui-settings.md) · [App Settings](ui-app-settings.md) · [Data Model](data-model.md) (Tree entity) · [API Contract](api.md) (Trees endpoints)

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

Minimal shared navbar, always visible at the top. See [Topbar](ui-topbar.md) for full specification.

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

**Mini tree visual** — a decorative SVG illustration of a tree silhouette with colored dots, displayed in a rounded-top container with a subtle background (`var(--tree-visual-bg)`). Provides visual identity to each card.

**Card body** (padded):

1. **Header row** — tree name (Cinzel, bold, uppercase) on the left; three-dot menu button (vertical dots) on the right.
2. **Stats row** — person count and max generation depth with small icons, in muted text.
3. **Footer row** — "Modified X ago" date on the left; optional "Recent" badge on the right (shown for trees modified within the last 24 hours).

### Card interactions

| Action | Behavior |
|---|---|
| Click anywhere on card | Navigates to the tree view (`/trees/{id}`) |
| Click three-dot menu | Opens the card menu (see below), does not propagate |

### Card states

| State | Visual |
|---|---|
| Default | Neutral border, subtle shadow |
| Hover | Orange border, lifted shadow, 2px upward translate |
| New (< 24h) | Green "Recent" badge in footer |

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
- **Delete** — destructive action, shown in red on hover, requires confirmation

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
- Below 640px: padding reduces to `2rem 1rem 4rem`, cards go single-column
- Topbar navigation collapses (future, post-MVP)

---

## 11. Design Tokens (reference)

See [Design Tokens](ui-design-tokens.md) for the full token list. Key tokens used on this page:

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
