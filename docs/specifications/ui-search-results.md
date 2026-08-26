---
type: "UI Specification"
title: "Visual & Functional Specifications — Search Results"
description: "UI behavior and interaction specification for Visual & Functional Specifications — Search Results."
tags: [oxidgene, specification, ui, ux]
timestamp: 2026-06-17T00:00:00Z
---


# Visual & Functional Specifications — Search Results

> Part of the [OxidGene Specifications](index.md).
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
|   Search results for "<surname>" "<given name>"                     |
|   42 persons found                                                    |
|                                                                       |
|   [Filters v]                                                        |
|                                                                       |
|   Sort: [Relevance v]                              [list] [grid]    |
|   +--------------------------------------------------------------+   |
|   | [avatar] <person A>             * 1842  + 1918               |   |
|   |          Spouse: <person B> - 3 children                     |   |
|   +--------------------------------------------------------------+   |
|   | [avatar] <person C>             * 1790  + 1855               |   |
|   |          Spouse: <person D> - 2 children                     |   |
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

## 4. Results Toolbar

The toolbar reports the exact total returned by the server, exposes the sort
selector, and switches between list and pedigree-grid views.

---

## 5. Filters

A collapsible filter bar below the page header, toggled by a "Filters" button with a dropdown arrow. Filters refine the result set in real time (200ms debounce after each change).

| Filter | Type | Options / Format |
|---|---|---|
| **Surname / given names** | Text inputs | Case- and accent-insensitive partial matching |
| **Gender** | Dropdown | All (default) / Male / Female / Unknown |
| **Occupation** | Text input | Matches occupation-event descriptions |
| **Born between** | Two year inputs | Inclusive start and end years |
| **Died between** | Two year inputs | Inclusive start and end years |
| **Place** | Text input | Matches any individual or family event place |
| **Event type** | Dropdown | All (default) / Birth / Death / Baptism / Burial / Marriage / Residence / Occupation / Census |
| **Event between** | Two year inputs | Inclusive start and end years for matching events |
| **Spouse** | Surname and given-name inputs | Matches another spouse in the same family |
| **Father / mother** | Surname and given-name inputs | Matches the corresponding parent through family links |
| **Has media** | Toggle | When enabled, only shows persons with at least one attached media |

A **"Clear filters"** link resets all filters to their default state.

Active filters are shown as removable chips above the results list.

---

## 6. Sort

A sort selector in the toolbar row above the results:

| Option | Description |
|---|---|
| Relevance (default) | Stable surname and given-name order; no fuzzy score is currently computed |
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
| [avatar]  <person A>              * 12/03/1842    + 07/11/1918|
|           Spouse: <person B> - 3 child(ren)                    |
+--------------------------------------------------------------+
```

Each row shows:
- **Avatar** (circular, ~40px) — initials with gendered background color
- **Full name** (surname uppercase + first name), with search term matches highlighted in orange
- **Birth / death dates** with green/blue symbols
- **Family summary** (one line): spouse name + child count
- **Sex indicator**: colored left border (blue/pink/grey)

### Card View (pedigree grid)

Results displayed as a responsive grid of cards (`minmax(340px, 1fr)`). Each card contains:

- **Header** (clickable, same navigation target as a list row): full name (surname + first name) and birth/death years
- **Mini-pedigree**: a small pannable pedigree fragment (self + parents + grandparents, `GET /cache/pedigree/{id}?ancestor_depth=2`) rendered with the same `MiniPedigree` component as the person profile's Ancestors section, at a denser fixed scale (0.5). Clicking any person card inside the fragment navigates to that person. Its cards draw the same date precision marks as the main canvas (`ca 1849-< 1917`) — same renderer, so they cannot drift apart; see [Tree View](ui-genealogy-tree.md)
- **Sex indicator**: colored top border (blue/pink)

Pedigrees are assembled lazily per card from family links and durable person
projections; loading cells show a placeholder message.

---

## 8. Result Interactions

| Action | Behavior |
|---|---|
| **Click a result** | Navigates to the [Person Profile](ui-person-profile.md) for that person |
| **Hover** | Subtle highlight, pointer cursor |

---

## 9. Pagination

Results are paginated with 25 results per page in list view (matching the API default) and 20 per page in card view — each card embeds a mini-pedigree, so a larger page would overload the layout and fire as many pedigree fetches. Switching view mode resets to page 1.

Pagination controls at the bottom of the results:
- Previous / Next arrow buttons
- Page number buttons (first, last, current +/- 2)
- Current page indicator: "Page 2 of 5"

The server computes the exact total and applies sorting before `limit` and
`offset`; the UI never truncates to a fixed client-side result set.

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
|  Enter at least one search criterion.|
+--------------------------------------+
```

---

## 11. Responsive

- Content max-width: 1200px, responsive padding
- Below **760px**: relation fields use a single column and content padding is reduced
- Filters collapse behind a "Filters" toggle button (already the default)

---

## 12. Keyboard

| Key | Behavior |
|---|---|
| `Enter` in either topbar search field | Applies the surname and given-name criteria |
| `Enter` on a focused result | Opens the result through its link |
| `Tab` | Moves focus between filter fields and results |
