# Visual & Functional Specifications — Genealogy Tree

> Part of the [OxidGene Specifications](README.md).
> See also: [Person Edit Modal](ui-person-edit-modal.md) · [Person Merge](ui-merge.md) · [Person Profile](ui-person-profile.md) · [Search Results](ui-search-results.md) · [GEDCOM Import](ui-gedcom-import.md) · [Homepage](ui-home.md) · [Settings](ui-settings.md) · [Data Model](data-model.md) · [API Contract](api.md)

---

## 1. General Structure

### Layout

The canvas displays a **mixed tree**: the focus person is at the vertical center, ancestors go upward, descendants go downward. Each generation occupies a **strict horizontal row**. All cards in the same generation are aligned on the same Y axis.

The number of generations displayed is fixed at any given time, but can be changed via the depth selector. The maximum is **10 ascending generations + 10 descending generations**.

### Always a Connected Tree

The canvas **never** displays isolated persons or disconnected subtrees. A person is visible only if they are reachable from the focus through a continuous chain of relationships (ascending, descending, couple) within the requested depth.

Persons with no link to the current tree are accessible only via **search**.

---

## 2. Grid and Spatial Layout

### Fixed-Step Grid

All cards are placed on a regular grid. The horizontal step is identical everywhere:

```
step = card_width + minimum_gap
```

No variable spacing between cards on the same level. A cell is either occupied by a card or empty. Empty cells can only appear at the **edges** of a level, never between two occupied cards.

### Centering per Level

Each level is centered relative to the **widest level** (the one occupying the most cells).

```
Level -2 (8 cards) :   [A1][A2][A3][A4][A5][A6][A7][A8]   <- reference
Level -1 (4 cards) :       [B1][B2][B3][B4]                <- centered
Level  0 (2 cards) :           [C1][C2]                    <- centered
Level +1 (3 cards) :          [D1][D2][D3]                 <- centered
Level +2 (2 cards) :           [E1][E2]                    <- centered
```

### Parity Handling

When two adjacent levels have different parity (one even, one odd), perfect centering is not possible. A **minimal left offset** is applied, always less than half a grid step. No artificial spacing is added to compensate.

### Placement Constraint Priority

1. A card's position is first determined by its **connections** (child centered under its parents, parents centered above their children)
2. Cards with no connection constraints fill available cells starting from the center
3. The global level centering is applied last, as an overall offset

### Horizontal Compaction

The goal is to **minimize the total width** of the graph:

- Children of the same couple are contiguous, with no empty cell between them
- Two adjacent subtrees are brought as close as possible, separated by exactly one grid step
- No empty column in the middle of a level

---

## 3. Person Card

### Dimensions

- Standard size: **180x80px** (width x height)
- Reduced size (viewport < 900px wide): **130x64px**
- Identical for all generations, no variation by depth

### Internal Layout

Horizontal arrangement: avatar on the left, text information on the right.

```
+----------------------------------+
| +------+  FAMILY NAME            |
| |      |  First name(s)          |
| | init |  * 12/03/1842           |
| |      |  + 07/11/1918           |
| +------+                         |
+----------------------------------+
```

**Avatar** (`.pc-ph`):
- Circular, ~38px diameter, with gendered background color (blue male, pink female, grey unknown)
- Displays the person's **initials** (first letter of surname + first letter of first name) in white text
- When a profile photo is available (future), it replaces the initials circle with `object-fit: cover`
- **SOSA badge**: when the person has a SOSA number (ancestor of SOSA 1), a small colored dot (12px, `var(--green)` for ancestors, `var(--orange)` for SOSA 1) is displayed at the **bottom-center of the avatar circle**, with a 2px card-background border

**Text information** (`.pc-body`):
- Family name in uppercase, bold (`.pc-last`)
- First name(s) (`.pc-first`)
- Dates in priority order: Birth > Baptism for start date, Death > Burial for end date (`.pc-dates`)
- Date format: `dd/mm/yyyy`, or year only if day/month is unknown

**Date indicators** (`.pc-born`, `.pc-died`):
| Symbol | Color | Meaning |
|---|---|---|
| * | Green (`var(--green)`) | Birth |
| (cross) | Blue (`var(--blue)`) | Death |

### Visual Indicators

- **Colored left border**: blue for male, pink for female, grey for unknown (`.male`, `.female`)
- **Orange border** for the focus person (currently selected)
- **Slightly different background** by role: ancestor, descendant, focus, lateral generation

### Placeholder Card (Unknown Parent)

Appears only at the maximum ascending level, for each person whose parents are not recorded.

- Same dimensions as regular cards
- **Dashed border**, very subtle background
- Centered `+` icon, clickable to open the add-parent form
- Connected to the level below using the same connection rules as real cards

### Selected State

When a card is clicked:
- It becomes the new **focus** of the graph, the layout is recalculated centered on it
- Distinctive orange border
- A **pencil icon** appears just below the card, centered
- The pencil icon disappears as soon as another card is selected or the canvas is clicked

### Pencil Icon — Action Picker

Clicking the pencil icon opens a small **action picker modal** (not a full-screen modal). It presents the available actions for the selected person as a list of labeled options:

| Action | Description |
|---|---|
| **Edit individual** | Opens the full person edit modal |
| **Merge with...** | Opens a person search to select a duplicate to merge |
| **Edit union** | See below — expands into a sub-list if multiple unions exist |
| **Add spouse** | Opens a new person form pre-linked as spouse |
| **Add child** | Opens a new person form pre-linked as child |
| **Add sibling** | Opens a new person form pre-linked as sibling |

The picker is a compact overlay anchored just below the pencil icon, with a subtle backdrop. It closes on outside click or Escape. Choosing an action closes the picker and opens the relevant modal.

### Edit Union — Sub-list

When the selected person has **exactly one union**, clicking "Edit union" immediately opens the couple edit modal.

When the selected person has **two or more unions**, clicking "Edit union" expands an inline sub-list within the picker, replacing the action row. Each union is listed as a single line showing:

```
[Partner name]   * birth year   (ring) marriage year (if known)
```

Clicking a union entry closes the picker and opens the couple edit modal for that specific union. A back arrow at the top of the sub-list returns to the main action list.

---

## 4. Connectors

### General Rules

- Connectors use **L-shapes with 90-degree bends**, never diagonals
- **Solid line only**, regardless of the type of relationship (marriage, cohabitation, other) — no visual distinction by line style
- Color: `var(--connector)` (neutral blue-grey in dark theme, warm grey in light theme)
- All horizontal segments within the same generation are strictly at the **same Y level**

### Structure of a Couple -> Children Link

```
     [Parent 1]--------------[Parent 2]
                      |
                      |  <- departs from the exact midpoint of the segment
                 -----+-----
                 |         |
             [Child 1]  [Child 2]
```

1. Horizontal segment between the two partner cards
2. Vertical line descends from the **exact midpoint** of the horizontal segment
3. Horizontal bar at the midpoint between the parents' row and the children's row
4. Vertical lines from the bar down to the top of each child card

### Case: One Parent Has Multiple Unions

Each union produces an **independent horizontal segment**. All segments are at the same Y level. The vertical link to the children departs from the midpoint of each segment.

```
[Mother B]------[Father]------[Mother A]
          |             |
          |             |
     -----+-----   -----+-----
     |         |   |         |
[Child B1][Child B2] [Child A1][Child A2]
```

The shared parent card is used by both segments. The vertical departure points are respectively the midpoint of `[Mother B]--[Father]` and the midpoint of `[Father]--[Mother A]`.

### Case: Unknown Parent (Placeholder)

The placeholder counts as a full card for midpoint calculation:

```
[Known parent]----[?]
       |
  (midpoint of segment)
       |
   [Child]
```

### Grid Alignment

- The midpoint of a couple segment always falls on a **half grid step**
- The children's horizontal bar is drawn between the two rows, at the midpoint distance
- Vertical lines fall on the **column centers** of the grid

---

## 5. Navigation and Controls

### Topbar

Fixed height, spans the full width above the canvas. Uses the shared `td-topbar` component.

```
+----------------------------------------------------------------------+
|  [logo] tree_name / Tree              [Last name] [First name] [Q]   |
+----------------------------------------------------------------------+
```

**Breadcrumb** (`.td-bc`): logo icon (links to homepage) + tree name (`.td-bc-link`) + `/` separator (`.td-bc-sep`) + "Tree" label (`.td-bc-current`). The tree name links to the tree view.

### Search

Two independent fields in the topbar, aligned to the right: **Last name(s)** and **First name(s)**. Either field can be used alone, or both combined. The **Last name(s)** field can be used to search a name or a SOSA number, if the element searched is a number it is a SOSA number. A magnifying glass button triggers the search.

**On Enter** (or click magnifying glass):
- Navigation to a dedicated **results page** (`/trees/{id}/search`)
- All matching persons displayed as a list
- Additional filters available (dates, location, gender...)
- Each result is clickable and returns to the tree centered on that person

### Left Sidebar (ISB)

Fixed vertical bar (`var(--sb)` = 46px wide). SVG stroke icon buttons stacked vertically, tooltip on hover. No text displayed. All icons use a consistent style: `stroke: currentColor`, `fill: none`, `strokeWidth: 2`, 16x16px viewBox.

**Buttons top to bottom**:

| Icon | SVG description | Action |
|---|---|---|
| Org-chart | 3 small rectangles connected by lines (sitemap) | Tree view (active by default) |
| Person silhouette | Circle head + body path | Detailed profile view |
| Stacked layers | 3 horizontal paths with decreasing width | Depth selector |
| Magnifying glass + | Magnifying glass with plus sign | Zoom in |
| Four corners | 4 corner arrows pointing outward (maximize) | Fit to screen |
| Magnifying glass - | Magnifying glass with minus sign | Zoom out |
| Person + plus | Person silhouette with a small plus | Add a person |
| **separator** | Thin horizontal line | Visual divider |
| Gear | Gear/cog icon (Lucide gear path) | Opens [Settings](ui-settings.md) for this tree |

**Depth selector — hover panel**:

Appears to the right of the button on hover. No text, no Apply button. Changes are applied immediately.

```
+----------+
|  ^ - 2 + |
|  v - 2 + |
+----------+
```

- `^`: number of ascending generations (0-10)
- `v`: number of descending generations (0-10)
- Layout recalculated immediately on each `+` or `-`
- The panel stays open as long as the mouse is over the button or the panel
- Closes on mouseout with a 150ms delay

**Profile view**: switches the canvas to a detailed profile of the selected person. A back button returns to the tree.

### Canvas Interactions

| Action | Behavior |
|---|---|
| Click on a card | New focus + pencil icon + events sidebar updated |
| Click on placeholder `+` | Opens add-parent form |
| Drag on canvas | Free pan |
| Scroll wheel / pinch | Zoom, range 0.3x-2x |
| FIT button | Reframes the entire tree in the window |
| Depth selector | Recalculates layout, recenters on current focus |

### Focus Change

**Person already visible in the tree**: layout recalculated and recentered, animated transition.

**Person outside the current tree** (via search): tree entirely rebuilt around the new focus, no transition.

---

## 6. Events Sidebar (Right)

### General Behavior

- Width: 275px (`var(--evw)`)
- Collapsible via a toggle button on its left edge
- Collapsed: only the button remains visible, the canvas reclaims the space
- Open/closed state is remembered

### Content

Header with avatar (initials circle), full name and dates of the selected person. Then a chronological list of their events, grouped by year.

```
+------------------------------+
| [avatar] FAMILY First name   |
|          * 1842  + 1918      |
+------------------------------+
| EVENTS                       |
+------------------------------+
| 1842                         |
|  *  Birth                    |
|     Beaune, Cote-d'Or       |
|                              |
| 1865                         |
|  (ring) Marriage             |
|     with Marguerite L.      |
|                              |
| 1918                         |
|  +  Death                    |
|     Pommard                  |
+------------------------------+
```

### Event Types

Each event type has a colored circle icon (`.ev-ic-*`):

| Icon class | Color | Type |
|---|---|---|
| `ev-ic-birth` | Green | Birth |
| `ev-ic-death` | Blue | Death |
| `ev-ic-marry` | Orange | Marriage |
| `ev-ic-other` | Grey | Other events |

Each event is clickable to display full details (complete location, source, notes).

---

## 7. Overall Layout

```
+----------------------------------------------------------------------+
|                        TOPBAR + SEARCH                                |
+------+----------------------------------------------+----------------+
|      |                                              |                |
|  I   |                                              |    EVENTS      |
|  S   |           CANVAS -- TREE                     |   SIDEBAR      |
|  B   |                                              |   (275px)      |
|      |                                              |                |
|      |                                              |                |
+------+----------------------------------------------+----------------+
```

| Zone | Dimensions |
|---|---|
| Topbar | Auto height, full width |
| Left sidebar (ISB) | Fixed width 46px (`var(--sb)`), height = zone below topbar |
| Canvas | Remaining space, scrollable and zoomable |
| Right sidebar | Width 275px (`var(--evw)`), collapsible |

---

## 8. Responsive

- Below **900px wide**: cards reduced to 130x64px, avatar 28px, smaller text
- Right sidebar switches to a **drawer** sliding over the canvas
- Left sidebar remains fixed but tooltips are replaced by visible labels below each icon
