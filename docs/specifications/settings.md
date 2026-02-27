# Visual & Functional Specifications — Tree Settings Page

---

## 1. Overview

The settings page (`/tree/{id}/settings`) is a dedicated full-page interface for configuring a single genealogy tree. It covers tree identity, privacy rules, display preferences, data entry options, diagnostic tools, and export. It is accessed via the "Settings" button on each tree card on the homepage.

---

## 2. Layout

```
┌──────────────────────────────────────────────────────────────────────┐
│                    TOPBAR + BREADCRUMB                               │
├────────────────────┬─────────────────────────────────────────────────┤
│                    │                                                 │
│   LEFT NAVIGATION  │   CONTENT AREA                                  │
│   (220px)          │                                                 │
│                    │   Section title                                 │
│   Settings         │   Form fields / toggles / tools                │
│   ─ Tree & Roots   │                                                 │
│   ─ Privacy        │                                                 │
│   ─ Date display   ├─────────────────────────────────────────────────┤
│   ─ Entry options  │   SAVE BAR (settings sections only)             │
│                    │   [Cancel]  [Save changes]                      │
│   Tools            │                                                 │
│   ─ History        │                                                 │
│   ─ Anomalies      │                                                 │
│   ...              │                                                 │
│                    │                                                 │
│   Export           │                                                 │
│   ─ Export tree    │                                                 │
└────────────────────┴─────────────────────────────────────────────────┘
```

---

## 3. Topbar

Same component as the homepage topbar. The breadcrumb replaces the main navigation:

```
My trees  ›  Famille Martin — Bourgogne  ›  Settings
```

Each crumb is a link. The last element ("Settings") is not clickable.

---

## 4. Left Navigation

Fixed width: 220px. Divided into three labeled groups separated by thin dividers.

Each item has an icon + label. The active item has an orange left border and a subtle orange background tint.

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

The content area renders one section at a time based on the active nav item. Max-width: 860px. Scrollable independently from the left nav.

Each section begins with:
- Small eyebrow label (group name, uppercase, orange)
- Section title (Cinzel font)
- Short descriptive subtitle

---

## 6. Save Bar

Appears **only** for the four Settings sections (Tree & Roots, Privacy, Date Display, Entry Options). Hidden for Tools and Export sections.

Sticky at the bottom of the content column. Contains:
- Left: transient status message ("Changes saved" / "Changes cancelled")
- Right: **Cancel** button (ghost style) + **Save** button (orange gradient)

---

## 7. Section: Tree & Roots

### SOSA 1 (Root person)

A **person picker** component: displays the currently selected person as a badge (avatar with initials, full name, birth/death dates) alongside a "Change…" button that opens a person search modal.

Help text explains the Sosa-Stradonitz numbering system and its role in the tree.

**Toggle:** "Identify ancestors of SOSA 1" — when enabled, a distinct icon appears on cards of all direct ancestors of the root person in the tree view.

### Who am I?

Second person picker to designate the current user's own person in the tree. Used to display relationship labels in profile views.

---

## 8. Section: Privacy

### Tree visibility

| Toggle | Effect |
|---|---|
| Private tree | The tree is hidden from all other members |
| Show SOSA 1 ancestors to visitors | Non-authenticated visitors can only see the direct lineage of the root person |

### Contemporary persons

**Age threshold slider** — range 50–120 years, default 80. Persons born less than N years ago without a known death date are treated as contemporary.

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
| Show event type symbols | Uses distinct symbols for birth (✦), baptism (✟), death (✝), burial (⚰) |
| Show "circa" prefix for approximate dates | Adds "c." before dates entered as approximate — e.g. *c. 1842* |

### Default calendar

Dropdown: Gregorian (default) · Julian · Republican · Hebrew.
Dates entered in another calendar are automatically converted for display.

---

## 10. Section: Entry Options

### Data entry assistance

| Toggle | Description |
|---|---|
| Place name autocomplete | Suggests geolocated place names as the user types |
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

---

## 11. Section: History

A chronological log of all changes made to the tree, newest first.

Each entry shows:
- Relative timestamp (e.g. "2 hours ago", "Yesterday 14:32")
- Action description
- Author (user who made the change)

No save bar. Read-only.

---

## 12. Section: Anomalies

A list of detected data inconsistencies: impossible death dates, marriages before birth, deaths before parent's birth, overly vague place names, probable duplicates.

Each anomaly shows:
- Warning icon
- Anomaly title (bold)
- Concerned person(s) in orange
- Detailed description

Clicking an anomaly navigates to the relevant person in the tree view.

No save bar. Read-only.

---

## 13. Section: Research Tracking

A filterable list of incomplete events in the ancestry of the SOSA 1 person.

**Filters:**
- Generation range (All · G1–G3 · G4–G6 · G7+)
- Event type (All · Missing birth · Missing death · Missing marriage)

Each row shows:
- Generation badge (e.g. G5)
- Person name
- Missing event detail
- Event type tag

No save bar. Read-only.

---

## 14. Section: Missing Ancestors

A generation-by-generation completeness report for the ascendance of the SOSA 1 person.

Each generation row shows:
- Generation number and label (e.g. G3 — Great-grandparents)
- Count: found / total possible
- Percentage with a color-coded progress bar:
  - Green (> 70%)
  - Orange (40–70%)
  - Red (< 40%)

No save bar. Read-only.

---

## 15. Section: Potential Duplicates

Pairs of persons with similar names, dates, or places that may represent the same individual.

Each pair shows:
- Person A details (name, birth date, place)
- Confidence label (Very likely · Likely · Possible) in orange
- Person B details

Actions per pair:
- **Merge** — opens a merge confirmation flow
- **Not duplicates** — dismisses the pair from the list

No save bar.

---

## 16. Section: Dictionary

A vocabulary index of all values entered in specific fields across the tree. Displayed in three tabs: **Places**, **Sources**, **Occupations**.

Each entry shows:
- Value as entered
- Usage count (how many persons or events reference it)
- Validation status: ✓ normalized · ⚠ too vague · ⚠ format to normalize

Clicking an entry allows batch-editing or normalizing all occurrences.

No save bar.

---

## 17. Section: Date Conversion

An interactive converter between calendar systems.

**Input:** text field for a date in the source calendar + a calendar selector (Republican · Gregorian · Julian · Hebrew).

**Output:** four read-only result tiles, one per calendar system, updating live as the user types.

No save bar.

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

No save bar for this section — export is triggered directly by the format buttons.

---

## 19. Design Consistency

The settings page uses the same design tokens, typography and component styles as the rest of the application (homepage, tree view). All interactive elements follow the same hover/focus/active patterns. The light/dark theme toggle in the topbar applies globally.
