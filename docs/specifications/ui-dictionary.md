---
type: "UI Specification"
title: "Visual & Functional Specifications — Dictionary"
description: "UI behavior and interaction specification for Visual & Functional Specifications — Dictionary."
tags: [oxidgene, specification, ui, ux]
timestamp: 2026-07-14T00:00:00Z
---


# Visual & Functional Specifications — Dictionary

> Part of the [OxidGene Specifications](index.md).
> See also: [Genealogy Tree](ui-genealogy-tree.md) (left sidebar icon) · [Person Profile](ui-person-profile.md) (left sidebar icon) · [Search Results](ui-search-results.md) (family-name drill-down) · [Settings](ui-settings.md) (this page replaces the former §16 Dictionary tool) · [Data Model](data-model.md) (PersonName, Source, Place, Event) · [API Contract](api.md) · [i18n](i18n.md)

---

## 1. Overview

The Dictionary page (`/trees/{id}/dictionary`) is a dedicated full-page view for browsing the distinct values entered across a tree for four fields: **family names**, **sources**, **places**, and **occupations**. Each value is shown once alongside a usage count, so recurring or inconsistent entries (a surname spelled two ways, a source cited from a dozen places, a place name entered slightly differently each time) are easy to spot.

This is a **read-only V1**. It exists to surface where mass-editing would help; the actual merge/rename-everywhere actions are a V2 (see section 11).

It is reached via the **Book/index icon** in the shared left icon sidebar (`TreeIconSidebar`), which makes it accessible identically from the [Genealogy Tree](ui-genealogy-tree.md) (pedigree canvas) and from the [Person Profile](ui-person-profile.md) page — the same component renders that icon in both places.

This page uses the standard `sub-page` layout pattern (see [General](general.md) section 8). There is **no left sidebar (ISB)** on this page itself — same convention as [Settings](ui-settings.md) and [Search Results](ui-search-results.md).

---

## 2. Layout

```
+----------------------------------------------------------------------+
| NAVBAR                                                                |
+----------------------------------------------------------------------+
| [logo] tree_name / Dictionary                                        |  <- td-topbar
+----------------------------------------------------------------------+
|  [Family Names]   Sources   Places   Occupations                     |  <- dict-tabs
+----------------------------------------------------------------------+
|  A B C D E F G H I J K L M N O P Q R S T U V W X Y Z   [All]         |  <- alphabet index
|  [ filter... ]                    142 entries   [25 v] per page      |  <- toolbar
+----------------------------------------------------------------------+
|  A ───────────────────────────────────────────────────────────       |
|   Aubert                                              12 persons  →  |
|   Auger                                                3 persons  →  |
|  B ───────────────────────────────────────────────────────────       |
|   Bernard                                             47 persons  →  |
|   ...                                                                 |
+----------------------------------------------------------------------+
|                    <  1  2  3 ... 6  >                                |  <- dict-pagination
+----------------------------------------------------------------------+
```

Content: `max-width: 1200px`, centered, scrollable (`sub-page-content`), matching [Search Results](ui-search-results.md) section 2.

---

## 3. Topbar

Uses the shared `td-topbar` + `td-bc` breadcrumb component. No search fields here (unlike Search Results) — this page has no query form, only in-page filtering (section 6).

```
[logo] tree_name / Dictionary
```

- Logo icon links to the homepage
- Tree name (`.td-bc-link`) links to the tree view
- `/` separator (`.td-bc-sep`)
- "Dictionary" (`.td-bc-current`) — not clickable

---

## 4. Tabs

Four tabs, text-labeled (icons alone are ambiguous at four items), styled as a segmented control (`.dict-tabs` / `.dict-tab`, active state same visual language as `.sr-view-btn.active`):

| Tab | Source field | Default active |
|---|---|---|
| Family Names | `PersonName.surname` (normalized) | Yes |
| Sources | `Source.title` | |
| Places | `Place.name` | |
| Occupations | `Event.description` where `event_type = Occupation` | |

Switching tabs resets the alphabet filter, quick filter, and page to their defaults (page 1, letter "All").

---

## 5. Alphabet Index

A row of 26 letter buttons plus an **All** button, above the results in every tab. Clicking a letter filters the list to entries whose value starts with that letter (case/accent-insensitive) and resets to page 1. Letters with zero matching entries in the current tab are shown disabled (muted, non-interactive). "All" (default) clears the letter filter.

The letter filter combines with the quick filter (section 6) using AND logic.

---

## 6. Quick Filter & Toolbar

A single instant-filter text input (not a submit form, unlike the two-field last/first name search on [Search Results](ui-search-results.md)) narrows the current tab's list as-you-type (client-side, ~200ms debounce), matched against the already-loaded page of values. Clearing the input restores the letter-filtered list.

The toolbar also shows:
- **Count**: total entries matching the current letter + quick filter (e.g. "142 entries")
- **Page size selector**: `25 / 50 / 100 / All` — "All" disables pagination and renders every matching entry. When the tab has more than 500 entries and "All" is selected, a small warning banner is shown above the list ("Showing all N entries may be slow") since large trees could otherwise render thousands of DOM rows at once.

---

## 7. Family Names Tab

Grouped by first letter of the normalized surname, with a sticky letter header (`A ──`) per group. Each row:

- Surname, in its most common original casing
- Usage count badge: number of persons carrying that surname (as primary or any `PersonName`)
- A chevron (`→`) — clicking the row navigates to [Search Results](ui-search-results.md) (`Route::SearchResults { last: surname, .. }`), reusing the existing search rather than duplicating a person list here

---

## 8. Sources Tab

Sorted/grouped alphabetically by title (same letter-header pattern as section 7). Each row:

- Title
- Author / Repository, shown as secondary muted text when present
- Usage count badge: number of `Citation` rows referencing this source

Clicking a row **expands it inline** (accordion, no navigation — there is no dedicated Source detail page today) to show the full source metadata (author, publisher, abbreviation, repository) and a list of the persons/events/families that cite it, each a link to the relevant [Person Profile](ui-person-profile.md).

---

## 9. Places Tab

Grouped by first letter of the place name (same pattern as section 7). Each row:

- Place name (as entered — full free-text string, e.g. "Beaune, 21200, Côte-d'Or, Bourgogne-Franche-Comté, France")
- A small pin icon (📍-style, filled) when `latitude`/`longitude` are set, outline/muted when not
- Usage count badge: number of `Event` + `Media` rows referencing this place

Clicking a row expands it inline (same accordion pattern as Sources) listing the events/media referencing that place, each linking to the relevant person.

---

## 10. Occupations Tab

Grouped by first letter of the occupation label (same pattern as section 7). Each row:

- Occupation label (`Event.description` for `event_type = Occupation`)
- Usage count badge: number of persons holding that occupation

Clicking a row expands it inline listing the persons with that occupation, each a link to their [Person Profile](ui-person-profile.md). There is no dedicated search filter for occupation today, so — unlike Family Names — this does not redirect to Search Results.

---

## 11. Future: Bulk Editing (V2 — not built in this pass)

The usage counts in V1 exist specifically to surface merge/rename candidates (a surname split across two spellings, a source or place duplicated with slightly different text). V2 adds, per row, a selection checkbox (hidden until a "Select" toggle is active, keeping V1's list visually unchanged); selecting two or more rows raises a floating action bar with **Merge** / **Rename everywhere**, applying the change to every underlying `PersonName` / `Source` / `Place` / `Event` row at once. No UI for this ships in V1 — this section only documents the design constraint that the row layout must leave room for a leading checkbox later.

---

## 12. Empty States

Reuses the shared `EmptyState` component (see [Shared Components](ui-shared-components.md) section 8).

### No entries at all (e.g. a brand-new tree with no sources yet)

```
+--------------------------------------+
|  (book icon)                         |
|  No entries yet                      |
|                                      |
|  Sources will appear here once you   |
|  add citations to persons or events. |
+--------------------------------------+
```

Message text is tab-specific (see i18n keys, section 14).

### No matches for the current letter/quick filter

```
+--------------------------------------+
|  No entries match                    |
|                                      |
|  [Clear filter]                      |
+--------------------------------------+
```

---

## 13. Responsive

- Content max-width: 1200px, responsive padding, same as [Search Results](ui-search-results.md) section 11
- Below **640px**: the alphabet index becomes a horizontally scrollable strip (no wrapping); letter headers stay sticky
- Pagination switches to Previous/Next only (no page numbers), same convention as Search Results

---

## 14. Internationalization

All labels are translated — no hardcoded UI strings — following the `page.section.element` key convention from [i18n](i18n.md) section 4, under the `dictionary.` prefix. Values themselves (surnames, source titles, place names, occupation labels) are user content and are **not** translated, per [i18n](i18n.md) section 2.

| Key | English | French |
|---|---|---|
| `dictionary.breadcrumb` | Dictionary | Dictionnaire |
| `dictionary.tab.family_names` | Family Names | Noms de famille |
| `dictionary.tab.sources` | Sources | Sources |
| `dictionary.tab.places` | Places | Lieux |
| `dictionary.tab.occupations` | Occupations | Professions |
| `dictionary.letter_all` | All | Tout |
| `dictionary.filter_placeholder` | Filter... | Filtrer... |
| `dictionary.page_size` | Per page | Par page |
| `dictionary.page_size_all` | All | Tout |
| `dictionary.count_one` | {count} entry | {count} entrée |
| `dictionary.count_other` | {count} entries | {count} entrées |
| `dictionary.person_count_one` | {count} person | {count} personne |
| `dictionary.person_count_other` | {count} persons | {count} personnes |
| `dictionary.citation_count_one` | {count} citation | {count} citation |
| `dictionary.citation_count_other` | {count} citations | {count} citations |
| `dictionary.reference_count_one` | {count} reference | {count} référence |
| `dictionary.reference_count_other` | {count} references | {count} références |
| `dictionary.large_list_warning` | Showing all {count} entries may be slow. | Afficher les {count} entrées peut être lent. |
| `dictionary.no_entries_family_names` | No family names yet. | Aucun nom de famille pour l'instant. |
| `dictionary.no_entries_sources` | No sources yet. | Aucune source pour l'instant. |
| `dictionary.no_entries_places` | No places yet. | Aucun lieu pour l'instant. |
| `dictionary.no_entries_occupations` | No occupations yet. | Aucune profession pour l'instant. |
| `dictionary.no_matches` | No entries match. | Aucune entrée ne correspond. |
| `dictionary.clear_filter` | Clear filter | Effacer le filtre |
| `dictionary.view_usage` | View usage | Voir les usages |

---

## 15. Navigation & Access Point

- New route: `Route::Dictionary { tree_id: String }` → `/trees/:tree_id/dictionary`
- Entry point: a new **Book/index** button in the shared `TreeIconSidebar` component (`crates/oxidgene-ui/src/components/tree_icon_sidebar.rs`), grouped with the Gear/Settings button after the trailing separator (see [Genealogy Tree](ui-genealogy-tree.md) section "Left Sidebar (ISB)"). Because this component is reused by both the pedigree canvas and the person-profile page, the icon requires no separate wiring to be available from both.
- Not shown on the Settings page itself (same `show_settings`-style opt-out already used there for the other ISB buttons).

---

## 16. Data Sources (backend work required)

None of the following aggregations exist yet; they must be added before this page can be implemented:

| Tab | Aggregation needed |
|---|---|
| Family Names | `GROUP BY surname, COUNT(*)` over `person_search_fts` (already indexed per tree — see [Caching](caching.md), Sprint E.6) |
| Sources | `COUNT(Citation)` per `source_id`, joined onto the existing `SourceRepo::list` |
| Places | `COUNT(Event) + COUNT(Media)` per `place_id`, joined onto the existing `PlaceRepo::list` |
| Occupations | `GROUP BY description, COUNT(*)` over `Event` where `event_type = Occupation` — new aggregation, no existing index |

`SourceRepo` and `PlaceRepo` already expose paginated `list` (see [API Contract](api.md)); the occupation and family-name aggregations are new.
