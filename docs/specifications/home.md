# Visual & Functional Specifications — Homepage

---

## 1. Overview

The homepage is the authenticated user's personal workspace. It lists all their genealogy trees and provides access to tree-level actions. There is no marketing or onboarding content — it is a productivity-focused interface.

---

## 2. Layout

```
┌──────────────────────────────────────────────────────────────────────┐
│                           TOPBAR                                     │
├──────────────────────────────────────────────────────────────────────┤
│                                                                      │
│   Page title                                                         │
│                                                                      │
│   [Search ____________]   [Sort ▾]   [⊞] [≡]   [+ New tree]        │
│                                                                      │
│   ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐           │
│   │ Tree card│  │ Tree card│  │ Tree card│  │ Tree card│           │
│   └──────────┘  └──────────┘  └──────────┘  └──────────┘           │
│                                                                      │
└──────────────────────────────────────────────────────────────────────┘
```

---

## 3. Topbar

Height: fixed 56px, full width, sticky.

**Left side:**
- Logo + brand name "OxidGene" (Cinzel font, orange-tinted)
- Vertical divider
- Main navigation links: My Trees · Sources · Places · Help

**Right side:**
- Theme toggle (light / dark)
- Notifications icon button
- Settings icon button
- Vertical divider
- User avatar (initials, orange gradient circle)

---

## 4. Page Header

Displayed below the topbar, above the toolbar.

- Small eyebrow label: "Personal workspace" (uppercase, orange, small)
- Page title: "My Genealogy Trees" (Cinzel font, large)
- Subtitle: tree count only — e.g. "6 trees" — **no total person count**

---

## 5. Toolbar

Single row below the page header. Contains from left to right:

**Search box** — single input, placeholder "Search a tree…". Filters the grid in real time as the user types. Matches on tree name and description.

**Sort selector** — dropdown with options:
- Recently modified (default)
- Name A → Z
- Number of people

**View toggle** — two icon buttons: grid view (default) and list view. In list view, `grid-template-columns` collapses to a single column.

**"+ New tree" button** — rightmost element. Visually prominent: orange gradient background, Cinzel font, white text, subtle shadow. Opens the new tree modal on click. Always visible regardless of the number of existing trees.

---

## 6. Tree Card

Cards are displayed in a responsive grid (`minmax(280px, 1fr)`). Each card has a fixed structure with no thumbnail preview.

### Anatomy (top to bottom)

**Color accent bar** — 3px-high strip at the very top of the card. Color is unique per tree (set at creation or editable in settings). Provides quick visual identification.

**Card body** (padded):

1. **Header row** — tree name (Cinzel, bold) on the left; three-dot menu button (⋮) on the right.
2. **Description** — short optional subtitle in muted text below the name.
3. **Action buttons row** — three equal-width buttons, always visible:
   - ⚙ **Settings** — links to `/tree/settings`
   - 🔧 **Tools** — links to `/tree/tools`
   - ↓ **Export** — links to `/tree/export` or opens an export panel
4. **Stats row** — person count and generation count with small icons, in muted text.
5. **Footer row** — "Modified X ago" date on the left; optional "Recent" badge on the right (shown for trees modified within the last 24 hours).

### Card states

| State | Visual |
|---|---|
| Default | Neutral border, subtle shadow |
| Hover | Orange border, lifted shadow, 2px upward translate |
| New (< 24h) | Green "Recent" badge in footer |

---

## 7. Three-Dot Menu (⋮)

Appears in the top-right of each card. Opens a dropdown with:

- **Open** — navigates to the tree view
- **Rename** — inline rename or modal
- **Duplicate** — creates a copy of the tree
- **Delete** — destructive action, shown in red on hover, requires confirmation

The dropdown closes on outside click or Escape.

---

## 8. Action Buttons Row

Three equal buttons permanently visible below the tree name, before the stats. Each is a small text+icon button with a neutral bordered style.

| Button | Icon | Destination |
|---|---|---|
| Settings | Gear | `/tree/{id}/settings` |
| Tools | Wrench | `/tree/{id}/tools` |
| Export | Download arrow | `/tree/{id}/export` |

On hover: orange border, orange-tinted text and background. Clicks do not propagate to the card (which would open the tree view).

---

## 9. New Tree Modal

Triggered by the "+ New tree" button. Centered overlay with blur backdrop.

**Fields:**
- Tree name (required) — text input, auto-focused on open
- Description (optional) — text input, placeholder "Origins, region, period…"

**Actions:**
- Cancel — closes modal, clears fields
- Create — validates name, adds tree to the list, closes modal

Keyboard: Escape closes the modal.

---

## 10. Empty State

When the search filter yields no results:

- Centered within the grid area
- Search icon in a rounded container
- Title: "No tree found"
- Subtitle: "Try a different search term."

When the user has no trees at all (first login), a different empty state encourages creating the first tree.

---

## 11. Responsive

- Below 900px: cards reduce to a single column
- Topbar navigation collapses to a hamburger menu
- Action buttons row stacks or uses icon-only mode

---

## 12. Design Tokens (reference)

| Token | Dark | Light |
|---|---|---|
| `--bg` | `#0d0f14` | `#f4f2ee` |
| `--bg2` | `#111318` | `#ede9e2` |
| `--bg3` | `#16191f` | `#ffffff` |
| `--bdr` | `#252d3d` | `#d4ccc0` |
| `--txt` | `#ddd8cc` | `#1e1a14` |
| `--txt2` | `#7a8da8` | `#5c5447` |
| `--txt3` | `#404f65` | `#9e9488` |
| `--orange` | `#e07820` | `#e07820` |
| `--ora-lt` | `#f5a03a` | `#f5a03a` |

Typography: **Cinzel** for titles and branded elements · **Lato** for body text.
