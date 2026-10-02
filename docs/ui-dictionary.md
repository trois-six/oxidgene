---
type: "UI Specification"
title: "Visual & Functional Specifications — Dictionary"
description: "Index of family names, sources, places, and occupations with usage counts, and the bulk family-name editor (rename, merge, particle)."
tags: [oxidgene, specification, ui, ux]
generated: { by: claude-code/claude-opus-5-5, at: 2026-10-02T02:06:56Z }
---


# Visual & Functional Specifications — Dictionary

> Part of the [OxidGene Specifications](index.md).
> See also: [Genealogy Tree](ui-genealogy-tree.md) · [Person Profile](ui-person-profile.md) · [Search Results](ui-search-results.md) · [Settings](ui-settings.md) · [Data Model](data-model.md) · [API Contract](api.md) · [Cross-cutting Rules](cross-cutting.md)

---

## 1. Overview

The Dictionary page (`/trees/{id}/dictionary`) is a dedicated full-page view for browsing the distinct values entered across a tree for four fields: **family names**, **sources**, **places**, and **occupations**. Each value is shown once alongside a usage count, so recurring or inconsistent entries (a surname spelled two ways, a source cited from a dozen places, a place name entered slightly differently each time) are easy to spot.

The page is read-only except for the family-name editor (§7.1), which renames
a family name across the persons carrying it and corrects where it splits
between particle and root. The other tabs surface inconsistent values without
offering a merge or rename operation.

It is reached via the **Book/index icon** in the shared left icon sidebar (`TreeIconSidebar`), which makes it accessible identically from the [Genealogy Tree](ui-genealogy-tree.md) (pedigree canvas) and from the [Person Profile](ui-person-profile.md) page — the same component renders that icon in both places.

This page uses the standard `sub-page` layout pattern (see [General](general.md) section 8) with the shared left icon sidebar, whose Book/index button shows as current here ([Common UI §6.3](ui-common.md#63-shared-left-icon-sidebar)).

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
|                    <  1  2  3 ... 6  >                                |  <- pager
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
- The print action of [Common UI §7](ui-common.md#7-printing), right-aligned.
  The active tab prints as the heading of its list; the letter index, filters
  and page-size selector do not print. A paginated list prints the page on
  screen with *Page n of m*; choosing *All* first prints the whole list.

---

## 4. Tabs

Six tabs, text-labeled (icons alone are ambiguous at six items), styled as a segmented control (`.dict-tabs` / `.dict-tab`, active state same visual language as `.sr-view-btn.active`):

| Tab | Source field | Default active |
|---|---|---|
| Family Names | `PersonName.surname_prefix` + `PersonName.surname`, exact spelling | Yes |
| Sources | `Source.title` | |
| Repositories | `Repository.name`, with the sources each holds ([§19](#19-repositories-tab)) | |
| Places | `Place.name` | |
| Occupations | `Event.description` where `event_type = Occupation` | |
| Media | `Media` documents, pages folded into their document ([§18](#18-media-tab)) | |

The bar is the shared tab bar ([Common UI §4.14](ui-common.md#414-tabs)): a `tablist` whose tabs say which is selected.

Switching tabs resets the alphabet filter, quick filter, and page to their defaults (page 1, letter "All").

Each tab loads its data the first time it is opened, not when the page opens: the page asks only for the family names, and a tab opened later shows "Loading dictionary…" while its aggregation arrives, then keeps it while the page stays open (until the tree changes or a family name is renamed).

At phone width the tabs share the row; when they do not fit, as on the [Statistics](ui-statistics.md) page, the strip scrolls sideways rather than squeezing their labels into each other.

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

Grouped by the first letter each surname files under (see below), with a sticky letter header (`A ──`) per group. Each row:

- The surname exactly as spelled. A row is one full surname — particle and root joined — compared character for character, so "Cruz" and "de la Cruz" are two rows, and so are "Martin" and "MARTIN". The list does not fold spellings together; the [family-name editor](#71-family-name-editor)'s rename is how the user unifies them.
- Usage count badge: number of persons carrying that surname on any `PersonName`, primary or not
- A pencil — opens the [family-name editor](#71-family-name-editor)
- A chevron — clicking the row expands it inline, listing the persons carrying that surname in a smaller nested list. The list matches the same full surname as the row, however each name is cut between particle and root, so a name whose particle was corrected by hand is listed like any other. Each person is clickable and opens the [Genealogy Tree](ui-genealogy-tree.md) focused on that person.

Which letter a surname files under depends on the viewer's "sort particles" preference. With particles included, "d'Aubigné" files under D and reads as written. With particles ignored it files under A, and the row then reads **root first, particle parenthesised** — `Aubigné (d')` — so the A group is scannable by the word it was actually sorted on. The boundary is taken from the entry's `sort_key` (the root the backend filed it under), never re-detected client-side, so a particle the user corrected by hand still displays where they put it. When the rows of one surname are cut in more than one way, the entry files under the cut most of them have.

### 7.1 Family-Name Editor

The pencil on a row opens the **Edit family name** modal. It does two things at the level of the name rather than person by person through the [person edit modal](ui-person-edit-modal.md): renaming the surname of everyone who carries it, and correcting where it splits between particle and root. Particle detection at import is a guess (see `oxidgene-core`'s `split_surname_particle`), and a wrong guess lands on every person carrying the name at once — a tree full of Breton "Le …" surnames wrongly filed under their root is the motivating case for the second.

The modal holds:

- A **Name** field, pre-filled with the surname as listed. It is the shared `ValueInput` with family-name suggestions, like the person form's surname field, and like it writes in capitals while the tree's automatic uppercase for surnames is on ([Settings §10](ui-settings.md#10-section-entry-options)): the first keystroke turns the whole field to upper case.
- A **Particle** field, pre-filled with the particle currently stored. Emptying it means "this name has no particle". While the name is unchanged it shows the name's current cut; once the name changes it follows the new name — detected, or the existing cut of the name it merges into — until the user types in it, and changing the name again drops what was typed.
- A live preview of the resulting particle, surname root, and the letter the name will file under.
- A scope line, and the button that applies the edit.

What the button does depends on the Name field:

- **Name unchanged — particle re-cut.** The scope reads "Applies to all N persons carrying this name"; **Apply to all** writes the new cut to every `PersonName` row carrying the surname, primary or not, via `PATCH /trees/{id}/dictionary/family-names/particle`.
- **Name changed — rename.** The scope reads "Renames N persons carrying it as their main name", N being the entry's `primary_count`. Only primary names are renamed: when some persons carry the name only as an alias, a married name or another secondary name, a note says so and that they keep it, and the name then stays in the dictionary for them. **Rename** calls `PATCH /trees/{id}/dictionary/family-names/rename`; it is disabled when no person carries the name as their main one.
- **Name changed to one already listed — merge.** A warning reads "« Y » already exists (M persons): the two will be merged". The renamed names join Y and take Y's existing cut, so one entry never holds two cuts: the Particle field is then read-only and says so. To change Y's cut, the user edits Y's own row.

Rules, all enforced server-side rather than only in the dialog:

- **The particle must already be at the head of the surname.** A particle that is absent is rejected instead of being prepended — otherwise the edit would inject a word the tree never contained, and clearing the particle afterwards could not take it back out.
- **A re-cut never changes the displayed surname**, only the boundary inside it, and rows already cut the requested way are skipped, so re-applying the same cut is a no-op rather than a pointless `updated_at` bump. Because a surname reaches every projection that embeds a display name, a re-cut triggers a full projection rebuild for the tree.
- **A rename matches exactly** — case and particle included — and stores the new name as sent; the capitals come from the Name field, as on the person form. A blank name is rejected, and renaming a name to itself changes nothing. The renamed persons' projections and search rows, and those of their spouses, children and parents, whose family links and relative filters show the name, are refreshed in the rename's own transaction.
- **Undo is per person.** A rename is recorded as one `family_name` entry in the audit log, reading "X · N persons · renamed to Y", and gives each renamed person a new version; there is no bulk undo, and a person is put back through their own [history](ui-person-history.md).

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
- Author, then the names of the repositories holding the source (secondary muted text, if present; the list endpoint returns them as `repositories`)
- Usage count badge: number of `Citation` rows referencing this source
- Edit (✎) — opens the source editor ([§8.11](#811-source-editor))
- Chevron — clicking **expands the row inline** to show full metadata and drill-down to citing persons/events

### 8.11 Source Editor

A dialog editing one source: **Title** (required), **Author**,
**Abbreviation** and **Publication** in the open; the rarer fields behind the
shared **More details** disclosure ([Person Edit Modal](ui-person-edit-modal.md)
uses the same control), which starts open when the source has any of them:

- **Responsible agency** (`SOUR.DATA.AGNC`);
- **Repositories**: one row per repository holding the source — "name — call
  number · medium" — each removable, and a row adding one: a repository of
  the tree, or **New repository…** with its name, then an optional call number
  and medium (the media types of [Data Model](data-model.md)). A source held
  under two call numbers at one repository has two rows.

The fields wait for **Save**; a repository row is written the moment it is
added or removed, like an event's witnesses. Saving refreshes the list.

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

### 8.9 Other Tabs

**Family Names, Places, Occupations** tabs use the standard A–Z alphabet index (section 5) — they do not use smart drill-down, since their distribution across the alphabet is sufficiently varied.

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

**Backend contract**: `GET .../dictionary/sources/groups?prefix=...` performs this resolution server-side in a loop and returns the *resolved* prefix (which may be longer than the requested `prefix`) together with `total` and the real next-level `groups` — empty `groups` signals "the count is already <= 250", and the final list at this resolved prefix then comes in the same answer. This keeps each user click, the final level included, to a single request regardless of how many forced characters were skipped. See `DictionaryRepo::resolve_source_drill_down` (`oxidgene-db`).

---

## 9. Places Tab

Grouped by first letter of the place name (same pattern as section 7). Each row:

- Place name as entered, shown as a full free-text hierarchy
- A small pin icon (📍-style, filled) when `latitude`/`longitude` are set, outline/muted when not
- Usage count badge: number of `Event` + `Media` rows referencing this place

Clicking a row expands it inline (same accordion pattern as Sources) listing the persons the place's events and media concern — a couple's event its spouses, a media the persons it is linked to or shows — each linking to the person, so a place with uses never lists nobody.

---

## 10. Occupations Tab

Grouped by first letter of the occupation label (same pattern as section 7). Each row:

- Occupation label (`Event.description` for `event_type = Occupation`)
- Usage count badge: number of persons holding that occupation

Clicking a row expands it inline listing the persons with that occupation, each a link to their [Person Profile](ui-person-profile.md). There is no dedicated search filter for occupation today, so — unlike Family Names — this does not redirect to Search Results.

---

## 11. Empty States

Reuses the shared `EmptyState` component (see [Common UI §4.7](ui-common.md)).

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

## 12. Responsive

- Content max-width: 1200px, responsive padding, same as [Search Results](ui-search-results.md) section 11
- Below **640px**: the four category tabs remain visible on one compact line
- Below **640px**: the alphabet index wraps onto as many rows as needed so every letter remains directly visible; letter headers stay sticky
- Below **640px**: the pager ([Common UI §4.12](ui-common.md#412-pager)) keeps
  only its Previous and Next controls; numbered page controls are hidden

---

## 13. Internationalization

All labels use i18n keys under the `dictionary.` prefix. Surnames, source
titles, place names, and occupation labels are user content and are not
translated; see [Cross-cutting Rules §3](cross-cutting.md).

The keys and their values in the eight interface languages are in the
translation tables of `crates/oxidgene-ui/src/i18n/` (`en.rs`, `fr.rs`, …),
which are authoritative and kept at exact key parity
([Cross-cutting Rules §3](cross-cutting.md)).

---

## 14. Navigation & Access Point

- Route: `Route::Dictionary { tree_id: String }` → `/trees/:tree_id/dictionary`
- Entry point: the **Book/index** button of the shared `TreeIconSidebar` component (`crates/oxidgene-ui/src/components/tree_icon_sidebar.rs`), first of the group after the trailing separator (see [Genealogy Tree](ui-genealogy-tree.md) section "Left Sidebar (ISB)"). The sidebar finds the tree from the route, so the button needs no wiring on any page; on the Dictionary itself it shows as current.

---

## 15. Data Sources

Each tab reads one aggregation of `DictionaryRepo` (`oxidgene-db`), run per
request ([API Contract](api.md#dictionary)):

| Tab | Aggregation |
|---|---|
| Family Names | Distinct full surnames (`surname_prefix` + `surname`, exact spelling) over the `person_name` rows of live persons, counted per person, with the count of persons carrying each as their primary name (`DictionaryRepo::family_names`) |
| Sources | The tree's sources, each with its citation count (`DictionaryRepo::sources_with_usage`, or `sources_with_usage_by_prefix` once the drill-down of section 8 has narrowed them) |
| Places | The tree's places, each with the count of events and media referencing it (`DictionaryRepo::places_with_usage`) |
| Occupations | The distinct descriptions of `Occupation` events, with their counts (`DictionaryRepo::occupations`) |

---

## 16. Backend Support for Sources Smart Drill-Down

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
  `prefix` in the response is the *resolved* prefix — it may be longer than the request's `prefix` if single-choice levels were skipped. `groups` is empty when `total <= 250`, and the response then also carries `sources`: the final list at the resolved `prefix` (each source with its citation count), so the tab renders it without a second request.

- **`GET /dictionary/sources?prefix={prefix}`** — Returns every source whose title starts with `prefix` (case-insensitive), each paired with its citation count; with `prefix` absent, every source. It serves API clients: the tab itself takes the final list from the drill-down response.

### Backend Logic

- **Grouping**: `DictionaryRepo::source_group_counts(prefix)` groups all sources whose (uppercased) title starts with `prefix` by exactly one more character, returning only groups that actually occur — no prefix-format assumptions (no French-archive-specific parsing), it works for any title text.
- **Compression**: `DictionaryRepo::resolve_source_drill_down(prefix, threshold)` loops `source_group_counts`, extending `prefix` by the single available character while there is exactly one group and the total is still above `threshold` (`SOURCE_DRILL_THRESHOLD = 250`). It stops at whichever comes first — a genuine branch (`groups.len() != 1`) or `total <= threshold` — and returns `(resolved_prefix, total, groups)` with `groups` cleared in the latter case. A same-prefix guard prevents infinite loops if every remaining title is exactly `prefix` itself (no further characters to consume).
- **Computed per request**: both queries read the `source` table on every request, grouping the titles in Rust; no prefix index or other data is stored for the drill-down.

---

## 17. Sources Drill-Down State

- `source_history` holds only the branch labels the user actually clicked
  (real, multi-way choices — never an auto-skipped level); empty is the "All
  sources" root.
- The query sent to the backend is `source_history`'s last label (empty at
  the root); one resource re-resolves on every history change and returns
  either the next branch choices or the final list (section 8.5), so each
  click costs one request however many levels are skipped (section 16).
- The breadcrumb (section 8.6) renders `source_history` as clickable crumbs,
  plus one non-clickable "active" crumb for the resolved prefix when it
  differs from the last clicked label (when levels were skipped).
- Clicking an earlier crumb truncates `source_history` to that point; clicking
  a branch-choice button appends its label.
- The quick-filter text is reset on every navigation action (crumb, root or
  branch click), so a stale filter cannot hide the next level's results.
- A row's usage is loaded only when the row is expanded.
- On mobile (<640px), a breadcrumb too long for the width scrolls
  horizontally; the drill-down buttons reuse the `.dict-letter-btn` styling
  and wrapping of the alphabet index (section 5).

---

## 18. Media Tab

### 18.1 Overview

Every media of the tree, shown as the tiles of the shared media gallery (see
[Common UI §4.5](ui-common.md)), narrowed by a tag cloud, a name filter and a
filter panel. A multi-page document is one tile, never one per page. Clicking a
tile opens the shared viewer.

A library can hold thousands of scans, so unlike the other tabs this one does
not load an aggregation and page through it locally: the list is the API's
cursor-paginated `GET /trees/{id}/media`, and every filter is sent to the
server, so it narrows the whole library rather than the page on screen (see
[API Contract](api.md), Media library). Documents are listed in the order they
were added.

```
+----------------------------------------------------------------------+
|  All  Census¹²  Parish register⁴  Survey¹  Village Alpha³⁰  ...      |  <- tag cloud
|  [ Filter by title or file name... ]          [25 v]   142 media items|
|  [v Filters]                                                         |
|  Tag: Village Alpha x   Tag: Survey x   Linked person: exemple x  [...]|  <- chips
+----------------------------------------------------------------------+
|  [tile] [tile] [tile] [tile] [tile] [tile]                           |
|  Title    Title    ...                                               |
|  Census   Linked to 2 records                                        |
+----------------------------------------------------------------------+
|                      <   Page 1 of 6   >                             |
+----------------------------------------------------------------------+
```

### 18.2 Tag Cloud

Above the grid, every tag carried by at least one document, from
`GET /trees/{id}/media/facets`, each followed by its document count:

- **Order**: alphabetical, ignoring case and accents.
- **Spelling**: a tag is one entry whatever its case — it is matched on the
  normalized key `media_tag` stores (trimmed, lowercased; accents are
  significant there) — and shown in the spelling most of its documents carry.
- **Sizing**: font size from 0.78rem to 1.5rem and weight from 400 to 700,
  scaled on the logarithm of the count between the least and the most used
  tag, so one tag on every document does not flatten all the others to the
  minimum. When every tag has the same count they all take the minimum.
- **Selection**: several tags at once. Clicking a tag adds it to the
  selection, clicking a selected tag takes it out, and the leading **All**
  button clears them all; the grid keeps the documents carrying *every*
  selected tag. Selected tags are highlighted like an active letter and each
  shown as a chip.
- **Counts**: with nothing selected, the whole library's. With tags selected,
  each tag is counted among the documents carrying all the selected ones, and
  the cloud lists only the tags those documents carry — the ones that can
  still narrow the selection — the selected tags included. The other filters
  never narrow the cloud: hiding a tag because the name or panel filters are
  set would hide the tag the user may want to switch to.

A large vocabulary scrolls inside the cloud (at most 14rem high) rather than
pushing the grid off the screen.

### 18.3 Name Filter, Page Size and Count

The quick filter becomes the **name** filter: it matches the document's title
or file name, or one of its pages' file names, ignoring case and accents, on
the server. Typing is debounced (250 ms) so a word is one request.

The page size selector offers `25 / 50 / 100` — no "All": the API serves at
most 100 per page, and a grid of thumbnails is where rendering everything at
once hurts most. The count beside it is the number of documents matching every
active filter.

There is **no alphabet index** on this tab: titles are often file names or
scan numbers, whose first letter says little; the tag cloud is this tab's
index.

### 18.4 Filter Panel

A **Filters** toggle, closed by default, opens a panel in the style of the
[Search Results](ui-search-results.md) filters (three columns, two below 900px,
one below 640px):

| Filter | Keeps the documents… |
|---|---|
| File type | with a page of that kind — image, PDF, video, audio or other, read from the page's MIME type. Only the kinds the tree holds are offered, with their counts |
| Category | filed under that document category. Only the categories the tree holds are offered, with their counts |
| Linked person | connected to a person whose primary name (given names and surname, either order) or maiden name contains the text, ignoring case and accents. Connected means attached to the person, to a family they are a spouse in (not a child of), or to an event of theirs or of such a family — on the document or one of its pages — or identified on it by a crop |
| Linked event between | linked, by a media link or a crop, to an event dated within the years, inclusive, placed by its sort date as the search's event filter does. An undated event matches no range |
| Added between | added on those days (UTC), inclusive, with the browser's own date picker — this is a calendar day, not a genealogical date |

All filters — tag, name and panel — combine with AND. A year or date still
being typed is no constraint until it parses; a range that ends before it
starts shows "A range ends before it starts." in place of the grid.

Each active filter is also shown as a chip under the toolbar; clicking a chip
removes that filter, and **Clear all filters** removes them all. Changing any
filter or the page size returns to the first page.

### 18.5 Grid

The shared library grid of the media gallery module: the same tiles, bundle
and viewer as a person's gallery, one bundle request per page shown. Under
each tile's caption (its title, else its file name):

- what the record is — its document category, else its physical medium when
  that is not "other";
- how much it is used — "Linked to N records", counting the distinct persons,
  families, events and sources it is attached to by media links on it or its
  pages, or "Not linked". Crops and portraits are not counted.

The tiles are read-only: nothing here is somebody's attachment to detach or
make a portrait. The viewer keeps its metadata and tag editing; any change
there refreshes the grid and the cloud. Its delete, like the tile's menu, only
removes a document nothing references.

### 18.6 Pagination

Cursor pagination cannot jump to page 5, so the shared pager
([Common UI §4.12](ui-common.md#412-pager)) shows Previous, "Page N of M" and
Next; Previous walks back through the cursors already seen.

### 18.7 States

- Loading: "Loading dictionary…", as the other tabs.
- A tree with no media: "No media in this tree yet."
- Filters matching nothing: "No media match these filters." with **Clear all
  filters**.
- A failed request: the dictionary's error message.

### 18.8 Responsive

Below 640px the cloud, the toolbar and the chips wrap; the filter panel is a
single column and stays collapsed until opened; the grid uses the gallery's
narrower 120px tiles.

### 18.9 Internationalization

Tags, titles and file names are user content and are not translated. The
tab's labels are the `dictionary.tab.media` and `dictionary.media.*` keys of
the translation tables (section 13).

---

## 19. Repositories Tab

Every repository of the tree — the archives, libraries and offices holding
its sources — sorted by name, narrowed by the quick filter, without the
alphabet index or pagination. Each row shows the name and, muted, the first
address line, the phone and the website; **Edit** (✎) opens the repository
editor, and the chevron unfolds the live sources it holds, "title — call
number · medium", sorted by title. **Add a repository** opens the editor
empty.

The repository editor holds the **Name** (required), the **Address** over
several lines, **Phone**, **Email**, **Website** and a **Notes** field (the
repository's first note; the others stay as imported). An existing
repository's editor ends with a delete confirmation: the sources it held stay
in the tree, no longer listed as held there (the links are kept and come back
if the deletion is undone from the history).

The tab is its own module (`pages/dictionary_repositories.rs`), like the Media
tab, and loads the repositories the first time it opens.
