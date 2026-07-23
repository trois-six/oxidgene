---
type: "UI Specification"
title: "Visual & Functional Specifications — Dictionary"
description: "UI behavior and interaction specification for Visual & Functional Specifications — Dictionary."
tags: [oxidgene, specification, ui, ux]
timestamp: 2026-07-22T00:00:00Z
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
|  A B C D E F G H I J K L M N O P Q R S T U V W X Y Z   142 entries  |  <- alphabet index
|  [ filter... ]                                           per page     |  <- toolbar
|                                                         [25 v]        |
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

The alphabet row also shows the **Count** aligned to the right: total entries matching the current letter + quick filter (e.g. "142 entries").

The toolbar shows the **Page size selector** with the "Per page" label above the select: `25 / 50 / 100 / All` — "All" disables pagination and renders every matching entry. When the tab has more than 500 entries and "All" is selected, a small warning banner is shown above the list ("Showing all N entries may be slow") since large trees could otherwise render thousands of DOM rows at once.

---

## 7. Family Names Tab

Grouped by first letter of the normalized surname, with a sticky letter header (`A ──`) per group. Each row:

- Surname, in its most common original casing
- Usage count badge: number of persons carrying that surname (as primary or any `PersonName`)
- A chevron — clicking the row expands it inline, listing the persons carrying that surname in a smaller nested list. Each person is clickable and opens the [Genealogy Tree](ui-genealogy-tree.md) focused on that person.

---

## 8. Sources Tab — Intelligent Navigation Drill-Down

### 8.1 Overview

Many genealogy trees have hundreds or thousands of sources, most starting with "AD" (Archives Départementales — French departmental archives). The standard A–Z alphabet index would show nearly 30 unused letters and lump thousands of "AD" sources under a single letter, making navigation difficult.

The Sources tab instead uses an **intelligent drill-down** approach: at each level, only letters or prefixes that have actual sources are shown. Users drill down through increasingly specific categories until reaching <= 250 sources, at which point all are displayed at once without pagination.

### 8.2 First Level: Letter Index (Smart)

Instead of showing all 26 letters, show only the **first letters that actually appear in the tree's source titles**:

```
Source letters present in tree: A AD AN AR AT B C D E F G H I J K L M N O P Q R S T U V W X Y Z
(disable letters with 0 sources)
```

Example for a French tree:
- Most sources start with "A" (Archives Départementales prefix)
- A few start with "B" (Bibliothèques)
- A few start with "C" (Church records)
- Others rare

Display only the letters present: `A  B  C  D  E  F  ...  Z` (disabled letters are muted/non-interactive).

**Count display**: Shows the total sources matching the selected letter (e.g., "12,502 sources starting with 'A'").

### 8.3 Second Level: Prefix Drill-Down (when > 250 results)

If a letter contains more than 250 sources, show a **prefix selector** instead of the full list:

```
Showing 12,502 sources starting with "A"

Select a prefix:
AD  (12,277 sources)
AE  (  89 sources)
AF  ( 136 sources)
AG  (   0 sources) — disabled
...
AZ  (  12 sources)
```

The prefixes are derived from the actual source titles in the tree. Only prefixes with >= 1 source are shown (disabled prefixes are muted).

**User interaction**: Clicking a prefix filters to sources starting with that prefix.

### 8.4 Third+ Level: Further Subdivision (if needed)

If a prefix still has > 250 sources, subdivide further:

```
Showing 4,878 sources starting with "AD4"

Select a sub-prefix:
AD41  (1,205 sources)
AD42  (  890 sources)
AD43  (  834 sources)
AD44  (  949 sources)
...
```

Continue this pattern recursively until reaching <= 250 sources.

### 8.5 Final Level: List Display (when <= 250 sources)

Once filtered down to <= 250 sources, display them as a flat list **without pagination**:

```
Showing 127 sources starting with "AD44"

[Sources list — all 127 rows visible, no page breaks]

AD44 - Actes d'état civil (1800–1900)    45 citations  →
AD44 - Cadastral records (1850–1950)      12 citations  →
AD44 - Church registers (1700–1850)       70 citations  →
...
```

Each row shows:
- Source title
- Author / Repository (secondary muted text, if present)
- Usage count badge: number of `Citation` rows referencing this source
- Chevron — clicking **expands the row inline** to show full metadata and drill-down to citing persons/events

### 8.6 Behavior: Breadcrumb & Back Button

While drilling down, show a **breadcrumb** indicating the current filter level. Because forced single-choice levels are auto-skipped (section 8.10), a breadcrumb segment is only ever a *real* branch point — never an intermediate character that had no alternative:

```
All sources  >  AD44  >  AD44 - HOTEL - (
```

Each breadcrumb segment is clickable, allowing the user to jump back to a higher level without clicking "Back" multiple times. If the backend auto-skipped ahead of the last segment the user clicked, the resolved (skipped-to) prefix is appended as one extra, non-clickable "active" segment representing where that skip landed — this is what lets the trailing segment above read "AD44 - HOTEL - (" in one step instead of one crumb per letter.

Alternatively, show a **"Back" button** at the top of the results area.

### 8.7 Quick Filter (Across all Levels)

A text quick-filter input continues to work across all drill-down levels, narrowing the current level's results as-you-type:

```
Showing 1,205 sources starting with "AD41"

Filter: [état civil______]     227 matching sources

AD41 - Actes d'état civil (1800–1900)     45 citations  →
AD41 - Actes d'état civil (1900–1950)     38 citations  →
...
```

Clearing the filter restores the full list at the current level.

### 8.8 No Pagination on the Sources Tab

The Sources tab never shows a page-size selector or pagination controls (unlike section 6, which still applies to the other three tabs). While drilling down (> 250 matches at the current level), the UI shows branch-choice buttons, not a list — there is nothing to paginate. Once a level resolves to <= 250 matches, every matching source is rendered at once.

### 8.9 Non-Sources Tabs (No Change)

**Family Names, Places, Occupations** tabs continue to use the standard A–Z alphabet index (section 5) — they do not use smart drill-down, since their distribution across the alphabet is sufficiently varied.

### 8.10 Compression: Auto-Skip Forced Single-Choice Levels

A naive one-character-at-a-time drill-down forces the user through every level even when a level offers no real choice. For example, a department archive whose sources are all titled `"AD44 - <town> - ..."` has exactly one possible continuation at each of `"AD44"`, `"AD44 "`, `"AD44 -"`, `"AD44 - "` — there is nothing to pick between until the town names actually diverge. Within a single town, the same problem recurs: if only one town in the current branch starts with a given letter, every subsequent letter of that town's name is also a forced, single-choice step until either the town name is exhausted or another real branch appears (e.g. distinct record types once inside that town's records).

**Rule**: a drill-down level is shown to the user — as a breadcrumb segment, a set of clickable choices, or a stop before the final list — only when it is a **genuine branch point** (more than one possible next character) **or** the count has already dropped to <= 250. Any level with exactly one possible next character is skipped automatically and folded into the next request; the user never clicks through it.

**Example** — a department (`AD44`) containing two single-record towns (`ALPHA`, `BETA`) and one town (`HOTEL`) with six records split across two record types:

```
Level "" (root):            single choice ("A") → skip
Level "A":                  single choice ("AD") → skip
Level "AD":                 single choice ("AD4") → skip
Level "AD4":                single choice ("AD44") → skip
Level "AD44":                single choice ("AD44 ") → skip
Level "AD44 ":               single choice ("AD44 -") → skip
Level "AD44 -":              single choice ("AD44 - ") → skip
Level "AD44 - ":              REAL BRANCH: "AD44 - A" (ALPHA, 1) / "AD44 - B" (BETA, 1) / "AD44 - H" (HOTEL, 6)
  → user sees and picks from these 3 choices (breadcrumb gains "AD44 - ")
```

Picking `"AD44 - H"` (6 sources, still > 250 in a real tree — here just illustrating the shape) continues resolving on the backend:

```
Level "AD44 - H":            single choice ("AD44 - HO") → skip
Level "AD44 - HO":           single choice ("AD44 - HOT") → skip
Level "AD44 - HOT":          single choice ("AD44 - HOTE") → skip
Level "AD44 - HOTE":         single choice ("AD44 - HOTEL") → skip
Level "AD44 - HOTEL":         single choice ("AD44 - HOTEL ") → skip
Level "AD44 - HOTEL ":        single choice ("AD44 - HOTEL -") → skip
Level "AD44 - HOTEL -":       single choice ("AD44 - HOTEL - ") → skip
Level "AD44 - HOTEL - ":      single choice ("AD44 - HOTEL - (") → skip
Level "AD44 - HOTEL - (":     REAL BRANCH: "AD44 - HOTEL - (N" (3) / "AD44 - HOTEL - (M" (3)
  → user sees and picks from these 2 choices (breadcrumb gains "AD44 - HOTEL - (")
```

The user experiences exactly **two** navigation steps (the two "REAL BRANCH" points above), not fourteen.

**Backend contract**: `GET .../dictionary/sources/groups?prefix=...` performs this resolution server-side in a loop and returns the *resolved* prefix (which may be longer than the requested `prefix`) together with `total` and the real next-level `groups` — empty `groups` signals "the count is already <= 250; fetch the final list at this resolved prefix instead of drilling further." This keeps the compression to a single request per user click regardless of how many forced characters were skipped. See `DictionaryRepo::resolve_source_drill_down` (`oxidgene-db`).

---

## 8 (OLD — Archive Reference)

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

## 12. Future: Family Names — Genealogical Descent View (planned)

> Not built in this pass. Tracked as [Sprint E.8](roadmap.md) in the roadmap.

Today's flat, alphabetical-by-given-name usage list (section 7) is a good index but doesn't show how the people carrying a surname relate to each other. A planned V1.1 replaces that flat list, for the **Family Names** tab only, with a genealogical **descent view**: people sharing the surname are grouped into disjoint family branches and rendered as a nested, numbered list — each root ancestor (a surname carrier with no known parent who also carries the surname, within this tree) numbered at the top level, their spouse(s) and marriage date shown inline, and children indented one level per generation beneath them, recursively. Distinct, unconnected branches (e.g. two unrelated families that happen to share a spelling) get their own top-level number and sit one after another.

Each row keeps the existing per-person building blocks — "SURNAME Given" (section 7), the birth-death lifespan (`format_lifespan`) — and adds:

- **Marriage line**: a spouse marker (⚭) followed by the spouse's name and marriage year/date, on the same line as the person when they have a `FamilySpouse` link, sourced from `family_spouse` + `family` events (`EventType::Marriage`).
- **SOSA badge**: any person in the descent list who is also a direct ancestor of the tree's configured SOSA root person (`Tree.sosa_root_person_id`) gets the same green concentric-circle badge already used in the pedigree canvas and the [Person Profile](ui-person-profile.md) family narrative (`.pd-sosa-mark`, `var(--pn-sosa)`) — computed from the same `PersonAncestry` closure-table query as `person_detail.rs`'s `sosa_ancestors_resource` (`GET .../ancestors` from the SOSA root, collect `ancestor_id`), not a new algorithm.

Open design questions for the implementation sprint: whether a child who doesn't carry the surname (e.g. through a female line, depending on local naming convention) still appears indented under their parent, or is only listed as "m. {spouse}" without a further sub-tree; and whether this view fully replaces the flat list or is offered as a toggle next to it.

---

## 13. Empty States

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

## 14. Responsive

- Content max-width: 1200px, responsive padding, same as [Search Results](ui-search-results.md) section 11
- Below **640px**: the alphabet index becomes a horizontally scrollable strip (no wrapping); letter headers stay sticky
- Pagination switches to Previous/Next only (no page numbers), same convention as Search Results

---

## 15. Internationalization

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

## 16. Navigation & Access Point

- New route: `Route::Dictionary { tree_id: String }` → `/trees/:tree_id/dictionary`
- Entry point: a new **Book/index** button in the shared `TreeIconSidebar` component (`crates/oxidgene-ui/src/components/tree_icon_sidebar.rs`), grouped with the Gear/Settings button after the trailing separator (see [Genealogy Tree](ui-genealogy-tree.md) section "Left Sidebar (ISB)"). Because this component is reused by both the pedigree canvas and the person-profile page, the icon requires no separate wiring to be available from both.
- Not shown on the Settings page itself (same `show_settings`-style opt-out already used there for the other ISB buttons).

---

## 17. Data Sources (backend work required)

None of the following aggregations exist yet; they must be added before this page can be implemented:

| Tab | Aggregation needed |
|---|---|
| Family Names | `GROUP BY surname, COUNT(*)` over `person_search_fts` (already indexed per tree — see [Caching](caching.md), Sprint E.6) |
| Sources | `COUNT(Citation)` per `source_id`, joined onto the existing `SourceRepo::list` |
| Places | `COUNT(Event) + COUNT(Media)` per `place_id`, joined onto the existing `PlaceRepo::list` |
| Occupations | `GROUP BY description, COUNT(*)` over `Event` where `event_type = Occupation` — new aggregation, no existing index |

`SourceRepo` and `PlaceRepo` already expose paginated `list` (see [API Contract](api.md)); the occupation and family-name aggregations are new.

---

## 18. Backend Support for Sources Smart Drill-Down (implemented)

Two endpoints back the intelligent Sources navigation (section 8), both taking a `prefix` query parameter (absent/empty = top level):

- **`GET /dictionary/sources/groups?prefix={prefix}`** — Resolves the drill-down from `prefix`, auto-skipping forced single-choice levels server-side (section 8.10), and returns either the next real branch choices or an empty `groups` array once the count has dropped to <= 250:
  ```json
  {
    "prefix": "AD44 - HOTEL - (",
    "total": 6,
    "groups": [
      { "label": "AD44 - HOTEL - (M", "count": 3 },
      { "label": "AD44 - HOTEL - (N", "count": 3 }
    ]
  }
  ```
  `prefix` in the response is the *resolved* prefix — it may be longer than the request's `prefix` if single-choice levels were skipped. `groups` is empty when `total <= 250`; the frontend then fetches the final list using the response's `prefix`, not the one it originally requested.

- **`GET /dictionary/sources?prefix={prefix}`** — Returns every source whose title starts with `prefix` (case-insensitive), each paired with its citation count. Used both for the legacy unfiltered fetch (`prefix` absent) and as the final flat-list step once `groups` comes back empty.

### Backend Logic

- **Grouping**: `DictionaryRepo::source_group_counts(prefix)` groups all sources whose (uppercased) title starts with `prefix` by exactly one more character, returning only groups that actually occur — no prefix-format assumptions (no French-archive-specific parsing), it works for any title text.
- **Compression**: `DictionaryRepo::resolve_source_drill_down(prefix, threshold)` loops `source_group_counts`, extending `prefix` by the single available character while there is exactly one group and the total is still above `threshold` (`SOURCE_DRILL_THRESHOLD = 250`). It stops at whichever comes first — a genuine branch (`groups.len() != 1`) or `total <= threshold` — and returns `(resolved_prefix, total, groups)` with `groups` cleared in the latter case. A same-prefix guard prevents infinite loops if every remaining title is exactly `prefix` itself (no further characters to consume).
- **No caching layer**: both queries run directly against the `source` table per request (no precomputed prefix index) — acceptable given a single tree's source count is bounded in the tens of thousands and the query is a full-table scan grouped in Rust, not SQL. Revisit if this proves slow on very large trees.

### Data Model

No schema changes — prefixes are computed on the fly from `source.title` on every request.

---

## 19. Implementation Notes (implemented)

### UI State Management

- `source_history: Vec<String>` holds only the branch labels the user actually clicked (real, multi-way choices — never an auto-skipped level). Empty = "All sources" root.
- The query sent to the backend is always `source_history.last()` (or empty at root); the single combined resource re-resolves on every history change and returns either the next branch choices or the final list (section 18).
- The breadcrumb (section 8.6) renders `source_history` as clickable crumbs, plus one extra non-clickable "active" crumb for the resolved prefix when it differs from the last clicked label (i.e. when auto-skip occurred).
- Clicking an earlier breadcrumb crumb truncates `source_history` to that point; clicking a branch-choice button appends its label.
- The quick-filter text is reset on every navigation action (breadcrumb click, root click, branch click) so a stale filter can't silently hide the next level's results.

### Performance

- One HTTP round trip per user click, regardless of how many characters were auto-skipped (the backend loop in section 18 is server-side).
- No caching layer beyond the existing `ApiClient` response cache (30s TTL, keyed by URL + query params) — acceptable at current tree sizes; see section 18's "No caching layer" note.
- Lazy-load child sources only when expanding a row (existing behavior, no change).

### Responsive

- On mobile (<640px), breadcrumb becomes scrollable horizontally if too long.
- Drill-down buttons reuse the same `.dict-letter-btn` styling and responsive wrapping as the alphabet index (section 14), so no separate mobile handling was needed.
