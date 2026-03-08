# Visual & Functional Specifications — Search Results

> Part of the [OxidGene Specifications](README.md).
> See also: [Tree View](ui-genealogy-tree.md) (search fields in topbar) · [Person Profile](ui-person-profile.md) · [Data Model](data-model.md) (Person, PersonName, Event) · [API Contract](api.md) (Persons endpoint with search)

---

## 1. Overview

The search results page (`/trees/{id}/search`) is a dedicated full-page view for browsing persons matching a search query. It is reached by pressing **Enter** in the [Tree View](ui-genealogy-tree.md) topbar search fields or clicking the magnifying glass button. It provides a filterable, sortable list of matching persons with the ability to navigate back to the tree or to a person's profile.

This page uses the standard `sub-page` layout pattern (see [General](general.md) section 8). There is **no left sidebar** on this page — the content fills the full width within the `sub-page-content` container.

---

## 2. Layout

```
+----------------------------------------------------------------------+
| NAVBAR                                                                |
+----------------------------------------------------------------------+
| [logo] tree_name / Search       [Last name] [First name] [Q] [fit]  |  <- td-topbar
+----------------------------------------------------------------------+
|                                                                       |
|   Search results for "Martin" "Jean"                                 |
|   42 persons found                                                    |
|                                                                       |
|   [Filters v]                                                        |
|                                                                       |
|   Sort: [Relevance v]                              [list] [grid]    |
|   +--------------------------------------------------------------+   |
|   | [avatar] MARTIN Jean-Baptiste   * 1842  + 1918               |   |
|   |          Spouse: LEMAIRE Marguerite - 3 children             |   |
|   +--------------------------------------------------------------+   |
|   | [avatar] MARTIN Jean            * 1790  + 1855               |   |
|   |          Spouse: DUBOIS Marie - 2 children                   |   |
|   +--------------------------------------------------------------+   |
|   ...                                                                 |
|                                                                       |
+----------------------------------------------------------------------+
```

Content: `max-width: 1200px`, centered, scrollable. No left sidebar (ISB is only on the tree view and person profile pages).

---

## 3. Topbar

Uses the shared `td-topbar` + `td-bc` breadcrumb component. The search fields are **pre-filled** with the query that triggered the navigation. Modifying the fields and pressing Enter again updates the results in place.

```
[logo] tree_name / Search       [Last name] [First name] [Q] [fit]
```

- Logo icon links to the homepage
- Tree name (`.td-bc-link`) links to the tree view
- `/` separator (`.td-bc-sep`)
- "Search" (`.td-bc-current`)
- Search fields + magnifying glass button right-aligned
- Fit-to-screen button returns to the tree view

---

## 4. Page Header

- **Title**: "Search results for ..." with the query terms highlighted in orange
- **Count**: total number of matching persons (e.g. "18 person(s) found")

---

## 5. Filters

A collapsible filter bar below the page header, toggled by a "Filters" button with a dropdown arrow. Filters refine the result set in real time (200ms debounce after each change).

| Filter | Type | Options / Format |
|---|---|---|
| **Gender** | Dropdown | All (default) / Male / Female / Unknown |
| **Born between** | Two date inputs | `yyyy` or `dd/mm/yyyy` — start and end |
| **Died between** | Two date inputs | `yyyy` or `dd/mm/yyyy` — start and end |
| **Place** | Text input with autocomplete | Matches on birth, death, or any event place |
| **Event type** | Dropdown | All (default) / Birth / Death / Marriage / Baptism / Residence / Occupation / Other |
| **Has media** | Toggle | When enabled, only shows persons with at least one attached media |

A **"Clear filters"** link resets all filters to their default state.

Active filters are shown as removable chips above the results list.

---

## 6. Sort

A sort selector in the toolbar row above the results:

| Option | Description |
|---|---|
| Relevance (default) | Best name match first (fuzzy matching score) |
| Name A -> Z | Alphabetical by surname, then first name |
| Name Z -> A | Reverse alphabetical |
| Birth date (oldest first) | Oldest first |
| Birth date (newest first) | Most recent first |

---

## 7. View Modes

Two view mode buttons in the toolbar row (list icon and grid icon):

### List View (default)

Each result is a horizontal row:

```
+--------------------------------------------------------------+
| [avatar]  MARTIN Jean-Baptiste    * 12/03/1842    + 07/11/1918|
|           Spouse: LEMAIRE Marguerite - 3 child(ren)           |
+--------------------------------------------------------------+
```

Each row shows:
- **Avatar** (circular, ~40px) — initials with gendered background color. When a profile photo is available, it replaces the initials
- **Full name** (surname uppercase + first name), with search term matches highlighted in orange
- **Birth / death dates** with green/blue symbols
- **Family summary** (one line): spouse name + child count
- **Sex indicator**: colored left border (blue/pink/grey)

### Card View

Results displayed as a responsive grid of cards (same `minmax(280px, 1fr)` pattern as the [Homepage](ui-home.md) tree cards). Each card contains the same information as a list row, with a larger avatar area.

---

## 8. Result Interactions

| Action | Behavior |
|---|---|
| **Click a result** | Navigates to the [Person Profile](ui-person-profile.md) for that person |
| **Hover** | Subtle highlight, pointer cursor |

---

## 9. Pagination

Results are paginated with 25 results per page (matching the API default).

Pagination controls at the bottom of the results:
- Previous / Next arrow buttons
- Page number buttons (first, last, current +/- 2)
- Current page indicator: "Page 2 of 5"

Changing page scrolls the content area to the top.

---

## 10. Empty States

### No results

```
+--------------------------------------+
|  (search icon)                       |
|  No person found                     |
|                                      |
|  Try adjusting your search terms     |
|  or clearing some filters.           |
|                                      |
|  [Clear filters]                     |
+--------------------------------------+
```

### No search query

If the user navigates to the search page directly without a query, both search fields are empty and focused. The content area shows:

```
+--------------------------------------+
|  Enter a name to search              |
|  in this tree.                       |
+--------------------------------------+
```

---

## 11. Responsive

- Content max-width: 1200px, responsive padding
- Below **640px**: card view is forced (list view disabled), single column, reduced padding
- Filters collapse behind a "Filters" toggle button (already the default)
- Pagination switches to Previous/Next only (no page numbers)

---

## 12. Keyboard

| Key | Behavior |
|---|---|
| `Escape` | Returns to the tree view |
| `Up` / `Down` | Navigate between results in list view |
| `Enter` (on focused result) | Opens person profile for that person |
| `Tab` | Moves focus between filter fields and results |
