# Visual & Functional Specifications — Search Results

> Part of the [OxidGene Specifications](README.md).
> See also: [Tree View](ui-genealogy-tree.md) (search fields in topbar) · [Person Profile](ui-person-profile.md) · [Data Model](data-model.md) (Person, PersonName, Event) · [API Contract](api.md) (Persons endpoint with search)

---

## 1. Overview

The search results page is a dedicated view for browsing persons matching a search query. It is reached by pressing **Enter** in the [Tree View](ui-genealogy-tree.md) topbar search fields. It provides a filterable, sortable list of matching persons with the ability to navigate back to the tree or to a person's profile.

This page is distinct from the real-time dropdown (which shows 7–8 inline results as the user types). The dropdown is described in [Tree View](ui-genealogy-tree.md) §5.

---

## 2. Layout

```
┌──────────────────────────────────────────────────────────────────────┐
│                        TOPBAR + SEARCH (pre-filled)                  │
├──────┬───────────────────────────────────────────────────────────────┤
│      │                                                               │
│  S   │  Search results for "Martin" · "Jean"                        │
│  I   │  42 persons found                                             │
│  D   │                                                               │
│  E   │  ┌─ FILTERS ────────────────────────────────────────────────┐ │
│  B   │  │ Gender [All ▾]  Born [____]–[____]  Place [________]    │ │
│  A   │  │ Event type [All ▾]   [Clear filters]                    │ │
│  R   │  └──────────────────────────────────────────────────────────┘ │
│      │                                                               │
│      │  ┌──────────────────────────────────────────────────────────┐ │
│      │  │ Sort: [Relevance ▾]                          [≡] [⊞]   │ │
│      │  ├──────────────────────────────────────────────────────────┤ │
│      │  │ [photo] MARTIN Jean-Baptiste   ✦ 1842  ✝ 1918  Beaune │ │
│      │  │ [photo] MARTIN Jean            ✦ 1790  ✝ 1855  Dijon  │ │
│      │  │ [photo] MARTIN Jeanne          ✦ 1838  ✝ 1910  Beaune │ │
│      │  │ ...                                                     │ │
│      │  └──────────────────────────────────────────────────────────┘ │
│      │                                                               │
│      │  [← 1 2 3 ... 5 →]                                          │
│      │                                                               │
└──────┴───────────────────────────────────────────────────────────────┘
```

The left sidebar remains visible (same as tree view). Max-width of the content area: 960px.

---

## 3. Topbar

Same topbar as the [Tree View](ui-genealogy-tree.md). The search fields are **pre-filled** with the query that triggered the navigation. Modifying the fields and pressing Enter again updates the results in place (no page reload).

The breadcrumb reads:

```
My trees › Famille Martin — Bourgogne › Search
```

---

## 4. Page Header

- **Title**: "Search results for ..." with the query terms highlighted in orange
- **Count**: total number of matching persons (e.g. "42 persons found")

---

## 5. Filters

A collapsible filter bar below the page header. Filters refine the result set in real time (200ms debounce after each change).

| Filter | Type | Options / Format |
|---|---|---|
| **Gender** | Dropdown | All (default) · Male · Female · Unknown |
| **Born between** | Two date inputs | `yyyy` or `dd/mm/yyyy` — start and end |
| **Died between** | Two date inputs | `yyyy` or `dd/mm/yyyy` — start and end |
| **Place** | Text input with autocomplete | Matches on birth, death, or any event place |
| **Event type** | Dropdown | All (default) · Birth · Death · Marriage · Baptism · Residence · Occupation · Other |
| **Has media** | Toggle | When enabled, only shows persons with at least one attached media |

A **"Clear filters"** link resets all filters to their default state.

Active filters are shown as removable chips above the results list.

---

## 6. Sort

A sort selector in the toolbar row above the results:

| Option | Description |
|---|---|
| Relevance (default) | Best name match first (fuzzy matching score) |
| Name A → Z | Alphabetical by surname, then first name |
| Name Z → A | Reverse alphabetical |
| Birth date ↑ | Oldest first |
| Birth date ↓ | Most recent first |

---

## 7. View Modes

Two view mode buttons in the toolbar row:

### List View (default)

Each result is a horizontal row:

```
┌──────────────────────────────────────────────────────────────────┐
│ [photo]  MARTIN Jean-Baptiste    ✦ 12/03/1842    ✝ 07/11/1918  │
│          Beaune, Côte-d'Or                                       │
│          Spouse: LEMAIRE Marguerite · 3 children                 │
└──────────────────────────────────────────────────────────────────┘
```

Each row shows:
- **Photo thumbnail** (48×48px square, or gendered silhouette)
- **Full name** (surname uppercase + first name), with search term matches highlighted in orange
- **Birth / death dates** with symbols
- **Place** of birth (or main residence if no birth place)
- **Family summary** (one line): spouse name + child count
- **Sex indicator**: colored left border (blue/pink/grey)

### Card View

Results displayed as a responsive grid of cards (same `minmax(280px, 1fr)` pattern as the [Homepage](ui-home.md) tree cards). Each card contains the same information as a list row, but with a larger photo area (80×80px).

---

## 8. Result Interactions

| Action | Behavior |
|---|---|
| **Click a result** | Navigates to the [Tree View](ui-genealogy-tree.md) centered on that person |
| **Ctrl+Click / Cmd+Click** | Opens the [Person Profile](ui-person-profile.md) for that person |
| **Hover** | Subtle highlight, pointer cursor |
| **Right-click** | Browser context menu (no custom menu) |

A small tooltip on hover clarifies: "Click to view in tree · Ctrl+Click for full profile".

---

## 9. Pagination

Results are paginated with 25 results per page (matching the API default).

Pagination controls at the bottom of the results:
- Previous / Next arrow buttons
- Page number buttons (first, last, current ± 2)
- Current page indicator: "Page 2 of 5"

Changing page scrolls the content area to the top.

---

## 10. Empty States

### No results

```
┌──────────────────────────────────────┐
│        🔍                            │
│  No person found                     │
│                                      │
│  Try adjusting your search terms     │
│  or clearing some filters.           │
│                                      │
│  [Clear filters]                     │
└──────────────────────────────────────┘
```

### No search query

If the user navigates to the search page directly without a query, both search fields are empty and focused. The content area shows:

```
┌──────────────────────────────────────┐
│  Enter a name to search              │
│  in this tree.                       │
└──────────────────────────────────────┘
```

---

## 11. Responsive

- Below **900px**: card view is forced (list view disabled), single column
- Filters collapse behind a "Filters" toggle button
- Pagination switches to Previous/Next only (no page numbers)

---

## 12. Keyboard

| Key | Behavior |
|---|---|
| `Escape` | Returns to the tree view |
| `↑` / `↓` | Navigate between results in list view |
| `Enter` (on focused result) | Opens tree view centered on that person |
| `Tab` | Moves focus between filter fields and results |
