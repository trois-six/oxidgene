# Visual & Functional Specifications — Tree Settings Page

> Part of the [OxidGene Specifications](README.md).
> See also: [Homepage](ui-home.md) (settings button on cards) · [Tree View](ui-genealogy-tree.md) · [Person Edit Modal](ui-person-edit-modal.md) (privacy per person) · [Data Model](data-model.md) · [API Contract](api.md) (GEDCOM export)

---

## 1. Overview

The settings page (`/trees/{id}/settings`) is a dedicated full-page interface for configuring a single genealogy tree. It covers tree identity, privacy rules, display preferences, data entry options, diagnostic tools, and export. It is accessed via the **gear icon** in the [Tree View](ui-genealogy-tree.md) left sidebar or via tree card menus on the [Homepage](ui-home.md).

---

## 2. Layout

Uses the standard `sub-page` layout pattern (see [General](general.md) section 8).

```
+----------------------------------------------------------------------+
| NAVBAR                                                                |
+----------------------------------------------------------------------+
| [logo] tree_name / Settings                                          |  <- td-topbar
+----------------------------------------------------------------------+
|                                                                       |
|   +------------------+---------------------------------------------+ |
|   |                  |                                              | |
|   | LEFT NAVIGATION  |   CONTENT AREA                              | |
|   | (200px)          |                                              | |
|   |                  |   Section title                              | |
|   | Settings         |   Form fields / toggles / tools              | |
|   | - Tree & Roots   |                                              | |
|   | - Privacy        |                                              | |
|   | - Date display   |                                              | |
|   | - Entry options  |                                              | |
|   |                  |                                              | |
|   | Tools            |                                              | |
|   | - History        |                                              | |
|   | - Anomalies      |                                              | |
|   | ...              |                                              | |
|   |                  |                                              | |
|   | Export           |                                              | |
|   | - Export tree    |                                              | |
|   +------------------+---------------------------------------------+ |
|                                                                       |
+----------------------------------------------------------------------+
```

Content is constrained by `sub-page-content` (`max-width: 1200px`, centered, scrollable). The left navigation + content area use a flex row layout (`.settings-layout`).

---

## 3. Topbar

Uses the shared `td-topbar` + `td-bc` breadcrumb component:

```
[logo] tree_name / Settings
```

- Logo icon links to the homepage
- Tree name (`.td-bc-link`) links to the tree view
- `/` separator (`.td-bc-sep`)
- "Settings" (`.td-bc-current`) — not clickable

---

## 4. Left Navigation

Fixed width: 200px. Divided into three labeled groups.

Each group has an uppercase orange label. Each item is a text button. The active item has an orange text color, bold weight, and a subtle background (`var(--bg-card)`).

### Group 1 — Settings
| Item | Section ID |
|---|---|
| Tree & Roots | `arbre` |
| Privacy | `confidentialite` |
| Date Display | `affichage` |
| Entry Options | `saisie` |

### Group 2 — Tools
| Item | Section ID |
|---|---|
| History | `historique` |
| Anomalies | `anomalies` |
| Research Tracking | `recherches` |
| Missing Ancestors | `ancetres` |
| Potential Duplicates | `doublons` |
| Dictionary | `dictionnaire` |
| Date Conversion | `conversion` |

### Group 3 — Export
| Item | Section ID |
|---|---|
| Export tree | `export` |

---

## 5. Content Area

The content area renders one section at a time based on the active nav item. Scrollable independently from the left nav.

Each section begins with:
- Small eyebrow label (group name, uppercase, orange)
- Section title (Cinzel font)
- Short descriptive subtitle

---

## 6. Save Behavior

Settings changes are **auto-saved** on interaction. For example, selecting a SOSA root person saves immediately upon selection from the person picker. A transient success message appears briefly to confirm the save.

The tree cache is invalidated after each save so that navigating back to the tree view reflects the changes immediately.

---

## 7. Section: Tree & Roots

### SOSA 1 (Root person)

A **person picker** component enclosed in a card: displays the currently selected person as a row with avatar (initials circle), full name. Two buttons on the right:

- **Change** — opens a person search modal to select a different root person
- **Clear** — removes the SOSA root assignment

When no root person is selected, a muted message is shown: "No root person selected".

Help text explains the Sosa-Stradonitz numbering system: "The root person is the starting point of the Sosa-Stradonitz numbering system. All ancestors are numbered relative to this person."

When a SOSA root is set, all direct ancestors visible in the tree view display a **SOSA badge** on their avatar circle (see [Tree View](ui-genealogy-tree.md) section 3).

### Who am I?

Second person picker to designate the current user's own person in the tree. Used to display relationship labels in profile views. Displayed as a separate card below the SOSA card, with the same picker UI.

*Note: Personal identification will be available in a future update.*

---

## 8. Section: Privacy

### Tree visibility

| Toggle | Effect |
|---|---|
| Private tree | The tree is hidden from all other members |
| Show SOSA 1 ancestors to visitors | Non-authenticated visitors can only see the direct lineage of the root person |

### Contemporary persons

**Age threshold slider** — range 50-120 years, default 80. Persons born less than N years ago without a known death date are treated as contemporary.

**Display mode (radio group, 3 options):**

| Option | Description |
|---|---|
| Fully hidden | Name, dates and photo all hidden. Card shows "Private person" only. |
| Semi-hidden | First name and last initial shown, dates and photo hidden. |
| Visible | No restrictions. Recommended only for private trees. |

**Additional toggles:**

| Toggle | Description |
|---|---|
| Allow navigation to hidden persons | Visitors can follow connections without seeing personal details |
| Show photos of contemporary persons | Photos can be shown or hidden independently of the display mode |

---

## 9. Section: Date Display

### Date format

Dropdown with four options:
- `dd/mm/yyyy` — e.g. 12/03/1842
- `dd Mmm yyyy` — e.g. 12 Mar 1842
- `Mmm yyyy` — e.g. Mar 1842
- `yyyy` — e.g. 1842

A **live preview** below the dropdown updates immediately to show how the dates will appear on a person card.

### Toggles

| Toggle | Description |
|---|---|
| Show event type symbols | Uses distinct symbols for birth (*), baptism, death (+), burial |
| Show "circa" prefix for approximate dates | Adds "c." before dates entered as approximate — e.g. *c. 1842* |

### Default calendar

Dropdown: Gregorian (default) / Julian / Republican / Hebrew.
Dates entered in another calendar are automatically converted for display.

---

## 10. Section: Entry Options

### Data entry assistance

| Toggle | Description |
|---|---|
| Place name autocomplete | Suggests place names as the user types (from tree places + offline database). See [PlaceInput](ui-shared-components.md) section 5 |
| Automatic uppercase for surnames | Surname field is auto-uppercased on input |
| Suggest existing persons | When adding a parent or partner, suggests persons already in the tree |

### Input date format

Dropdown for the expected date format during editing:
- `dd/mm/yyyy`
- `dd-mm-yyyy`
- `yyyy-mm-dd` (ISO 8601)
- `dd Mmm yyyy` (e.g. 12 Mar 1842)

### Default calendar for input

Same options as the display section. The calendar can be overridden field by field during editing.

### Offline place databases

When "Place name autocomplete" is enabled, the user can download city databases for supported countries to enable autocomplete without network access.

```
+-----------------------------------------------------+
|  Offline place databases                            |
|                                                     |
|  [x] France           12 MB   [Downloaded]         |
|  [x] Belgium           3 MB   [Downloaded]         |
|  [ ] Switzerland        2 MB   [Download]           |
|  [ ] United States     18 MB   [Download]           |
|  [ ] United Kingdom     8 MB   [Download]           |
|  [ ] Germany           10 MB   [Download]           |
|                                                     |
|  Last updated: 2025-01-15     [Update all]          |
+-----------------------------------------------------+
```

- Each country shows: checkbox, name, approximate size, download status
- **Download**: clicking "[Download]" fetches the database file; a progress indicator replaces the button during download
- **Update all**: re-downloads all already-downloaded databases
- **Remove**: unchecking a downloaded country removes its database file
- Downloaded databases are stored in the app data directory (desktop) or IndexedDB (web)
- See [PlaceInput](ui-shared-components.md) section 5.1 for supported countries and data content

---

## 11. Section: History

A chronological log of all changes made to the tree, newest first.

Each entry shows:
- Relative timestamp (e.g. "2 hours ago", "Yesterday 14:32")
- Action description
- Author (user who made the change)

Read-only.

---

## 12. Section: Anomalies

A list of detected data inconsistencies: impossible death dates, marriages before birth, deaths before parent's birth, overly vague place names, probable duplicates.

Each anomaly shows:
- Warning icon
- Anomaly title (bold)
- Concerned person(s) in orange
- Detailed description

Clicking an anomaly navigates to the relevant person in the tree view.

Read-only.

---

## 13. Section: Research Tracking

A filterable list of incomplete events in the ancestry of the SOSA 1 person.

**Filters:**
- Generation range (All / G1-G3 / G4-G6 / G7+)
- Event type (All / Missing birth / Missing death / Missing marriage)

Each row shows:
- Generation badge (e.g. G5)
- Person name
- Missing event detail
- Event type tag

Read-only.

---

## 14. Section: Missing Ancestors

A generation-by-generation completeness report for the ascendance of the SOSA 1 person.

Each generation row shows:
- Generation number and label (e.g. G3 — Great-grandparents)
- Count: found / total possible
- Percentage with a color-coded progress bar:
  - Green (> 70%)
  - Orange (40-70%)
  - Red (< 40%)

Read-only.

---

## 15. Section: Potential Duplicates

Pairs of persons with similar names, dates, or places that may represent the same individual.

Each pair shows:
- Person A details (name, birth date, place)
- Confidence label (Very likely / Likely / Possible) in orange
- Person B details

Actions per pair:
- **Merge** — opens a merge confirmation flow
- **Not duplicates** — dismisses the pair from the list

---

## 16. Section: Dictionary

A vocabulary index of all values entered in specific fields across the tree. Displayed in three tabs: **Places**, **Sources**, **Occupations**.

Each entry shows:
- Value as entered
- Usage count (how many persons or events reference it)
- Validation status: (check) normalized / (warning) too vague / (warning) format to normalize

Clicking an entry allows batch-editing or normalizing all occurrences.

---

## 17. Section: Date Conversion

An interactive converter between calendar systems.

**Input:** text field for a date in the source calendar + a calendar selector (Republican / Gregorian / Julian / Hebrew).

**Output:** four read-only result tiles, one per calendar system, updating live as the user types.

---

## 18. Section: Export

Two export format options, each displayed as a card with icon, name, description and an action button:

| Format | Extension | Description |
|---|---|---|
| GEDCOM 5.5.1 | `.ged` | Universal standard. Compatible with Ancestry, Geneanet, MyHeritage, MacFamilyTree and most genealogy software. |
| GEDZIP | `.gdz` | GEDCOM archive including associated media files (photos, documents). Ideal for full backups or sharing with attachments. |

### Export options (toggles)

| Toggle | Description |
|---|---|
| Include contemporary persons | If disabled, persons subject to privacy rules are excluded from the export |
| Include notes and sources | Exports personal notes and source references |
| Include media (GEDZIP only) | Embeds photos and documents in the GEDZIP archive |

Export is triggered directly by the format buttons.

---

## 19. Design Consistency

The settings page uses the standard `sub-page` layout pattern shared with all non-pedigree pages (see [General](general.md) section 8). All interactive elements follow the same hover/focus/active patterns from the [Design Tokens](ui-design-tokens.md). The light/dark theme applies globally.
