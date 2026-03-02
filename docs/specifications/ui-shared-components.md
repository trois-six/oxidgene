# Visual & Functional Specifications — Shared Components

> Part of the [OxidGene Specifications](README.md).
> See also: [Design Tokens](ui-design-tokens.md) · [Topbar](ui-topbar.md)

---

## 1. Overview

This document describes reusable UI components used across multiple pages and modals. Each component has a consistent appearance and behavior regardless of context.

---

## 2. ConfirmDialog

A modal confirmation prompt used for destructive or irreversible actions (delete person, delete tree, discard changes, detach child, etc.).

### Structure

```
┌─────────────────────────────────────────┐
│  Title                                  │
│                                         │
│  Description text explaining what       │
│  will happen if the user confirms.      │
│                                         │
│  [Cancel]              [Confirm]        │
└─────────────────────────────────────────┘
```

### Properties

| Prop | Type | Description |
|------|------|-------------|
| `title` | String | Bold heading (e.g. "Delete this person?") |
| `message` | String | Explanatory text |
| `confirm_label` | String | Label for the confirm button (default: "Confirm") |
| `danger` | bool | If true, confirm button uses destructive style (red) |
| `on_confirm` | EventHandler<()> | Called when the user confirms |
| `on_cancel` | EventHandler<()> | Called when the user cancels |

### Visual

- Centered overlay with dark semi-transparent backdrop (`rgba(0,0,0,0.5)`)
- Width: ~420px, compact
- Cancel button: ghost style (`var(--text-secondary)` text, transparent background)
- Confirm button: `var(--orange)` gradient background (or `var(--color-danger)` if `danger` is true)
- Closes on `Escape` or backdrop click (triggers cancel)

### Used by

- [Homepage](ui-home.md) — delete tree
- [Person Edit Modal](ui-person-edit-modal.md) — delete person, discard changes, remove media
- [Person Edit Modal](ui-person-edit-modal.md) — delete couple, detach child
- [Person Merge](ui-merge.md) — confirm merge

---

## 3. PersonPicker

A component for selecting a person from the current tree. Used in settings (SOSA 1 selection, "Who am I?") and in the merge flow.

### Structure

```
┌──────────────────────────────────────────────────┐
│  ┌──────┐  MARTIN Jean-Baptiste                  │
│  │ init │  ✦ 1842  ✝ 1918                       │
│  └──────┘                            [Change…]  │
└──────────────────────────────────────────────────┘
```

### Behavior

- **Display mode**: shows the currently selected person as a badge with avatar (initials circle), full name, and birth/death dates
- **"Change…" button**: opens a person search modal (same search as the [topbar](ui-topbar.md) §4, but in a modal overlay)
- **Search modal**: last name + first name fields, real-time results, click to select
- **Clear button** (×): removes the selection (if the field is optional)

### Properties

| Prop | Type | Description |
|------|------|-------------|
| `tree_id` | UUID | Tree to search in |
| `selected` | Option<Person> | Currently selected person |
| `on_change` | EventHandler<Option<Person>> | Called when selection changes |
| `required` | bool | If true, no clear button |
| `label` | String | Label displayed above the picker |

### Used by

- [Settings](ui-settings.md) — SOSA 1, "Who am I?"
- [Person Merge](ui-merge.md) — Step 1 duplicate selection

---

## 4. DateInput

A composite date input component handling GEDCOM-style date qualifiers and partial dates.

### Structure

```
[Qualifier ▾]  [dd/mm/yyyy________]  (   [dd/mm/yyyy________]  )
     ↑                ↑                           ↑
  qualifier       first date              second date (for Or/Between)
```

### Qualifier options

| Qualifier | Fields shown | GEDCOM output |
|---|---|---|
| Exact | 1 date field | `15 JAN 1842` |
| Around (circa) | 1 date field | `ABT 1842` |
| Perhaps | 1 date field | `EST 1842` |
| Calculated | 1 date field | `CAL 1842` |
| Before | 1 date field | `BEF 1842` |
| After | 1 date field | `AFT 1842` |
| Or | 2 date fields | First date used (app-specific) |
| Between | 2 date fields | `BET 1840 AND 1845` |
| From–To | 2 date fields | `FROM 1840 TO 1845` |

### Date field

- Text input accepting `dd/mm/yyyy`, `mm/yyyy`, or `yyyy` (partial dates are valid)
- Input format follows the tree setting "Input date format" (see [Settings](ui-settings.md) §10)
- Validation: red border on invalid format, tooltip with expected format

### Properties

| Prop | Type | Description |
|------|------|-------------|
| `date_value` | String | Raw GEDCOM date string |
| `on_change` | EventHandler<String> | Called with the GEDCOM date string |

### Used by

- [Person Edit Modal](ui-person-edit-modal.md) — birth, death, and all event dates (create + edit modes)
- [Person Merge](ui-merge.md) — date comparison

---

## 5. PlaceInput

A text input with autocomplete for place names. The autocomplete is **never restrictive** — the user can always type or keep free text.

### Canonical place format

Place names follow a structured format adapted per country:

| Country | Format | Example |
|---|---|---|
| France | `City, Postal code, Département, Région, Country` | `Beaune, 21200, Côte-d'Or, Bourgogne-Franche-Comté, France` |
| Belgium | `City, Postal code, Province, Country` | `Bruxelles, 1000, Bruxelles-Capitale, Belgique` |
| Switzerland | `City, Postal code, Canton, Country` | `Genève, 1200, Genève, Suisse` |
| USA | `City, ZIP, County, State, Country` | `Springfield, 62704, Sangamon, Illinois, United States` |
| UK | `City, Postcode, County, Country` | `Oxford, OX1, Oxfordshire, United Kingdom` |
| Germany | `City, PLZ, Kreis, Bundesland, Country` | `München, 80331, Oberbayern, Bayern, Deutschland` |

The number of levels varies per country. The format is always comma-separated, from most specific to least specific, ending with the country name.

### Structure

```
[City, postal code, département, region, country…____]
  ┌──────────────────────────────────────────────────────┐
  │  📍 Beaune, 21200, Côte-d'Or, Bourgogne-F-C, France │
  │  📍 Beaune-la-Rolande, 45340, Loiret, Centre, France │
  │  📍 Beaune-sur-Arzon, 43500, Haute-Loire, France     │
  └──────────────────────────────────────────────────────┘
```

### Behavior

- Text input with placeholder "City, postal code, département, region, country…"
- **Autocomplete**: when enabled in [tree settings](ui-settings.md) §10, suggestions appear after 3 characters with 300ms debounce
- Suggestions come from, in priority order:
  1. **Existing places** in the current tree (always available)
  2. **Offline place database** — a downloadable database of cities for supported countries (see §5.1)
  3. **External geocoding service** (post-MVP, online only)
- Each suggestion shows a 📍 icon + formatted place name in canonical format
- Clicking a suggestion fills the input with the canonical place string and links to the Place entity
- **Free text is always accepted** — the autocomplete is optional and never restrictive; the user may type any string, ignore suggestions, or edit a suggestion after selecting it
- If the user modifies a suggestion after selection, the Place entity link is cleared (the value becomes free text)

### 5.1 Offline Place Database

To support autocomplete without network access (desktop mode, or web mode without external geocoding), the application ships with a downloadable database of cities per country.

- **Data source**: open datasets (e.g. GeoNames, OpenDataSoft, national postal code databases)
- **Storage**: SQLite database file per country, stored in the app data directory
- **Download**: managed from [Settings](ui-settings.md) §10 — the user selects which countries to download
- **Supported countries (MVP)**: France, Belgium, Switzerland, United States, United Kingdom, Germany
- **Database content per city**: city name, postal code, administrative subdivisions (adapted per country), latitude, longitude
- **Size**: ~5–20 MB per country (compressed)
- **Updates**: periodic re-download from the settings page (manual, not automatic)
- **Search**: matches on city name (prefix match), postal code (prefix match), or any administrative subdivision

### Properties

| Prop | Type | Description |
|------|------|-------------|
| `tree_id` | UUID | Tree for local place search |
| `value` | String | Current place text |
| `place_id` | Option<UUID> | Linked Place entity (if selected from suggestions) |
| `on_change` | EventHandler<(String, Option<UUID>)> | Text + optional Place ID |

### Used by

- [Person Edit Modal](ui-person-edit-modal.md) — birth, death, and event places (create + edit modes)
- [Search Results](ui-search-results.md) — place filter

---

## 6. MediaUploader

A file upload component for attaching media to a person or event.

### Structure

```
┌────────────────────────────────┐
│                                │
│         📄                     │
│   Click or drag to upload      │
│   JPEG, PNG, WebP, GIF, PDF   │
│                                │
└────────────────────────────────┘
```

### Behavior

- Drag-and-drop zone with dashed border (`var(--border)`)
- On drag-over: border turns `var(--orange)`, background tints
- Clicking opens the system file picker
- Accepted formats: JPEG, PNG, WebP, GIF, PDF
- Multiple files can be uploaded at once
- Upload progress shown as an inline progress bar per file
- On completion, the file appears as a thumbnail in the media grid

### Properties

| Prop | Type | Description |
|------|------|-------------|
| `tree_id` | UUID | Target tree |
| `on_upload` | EventHandler<Media> | Called for each successfully uploaded file |
| `accept` | String | MIME types (default: `image/*,application/pdf`) |
| `multiple` | bool | Allow multi-file selection (default: true) |

### Used by

- [Person Edit Modal](ui-person-edit-modal.md) — media section
- [Person Profile](ui-person-profile.md) — media gallery "Add" button

---

## 7. EventIcon

A small inline component rendering the appropriate symbol for an event type.

### Mapping

| EventType | Symbol | Color |
|---|---|---|
| Birth | ✦ | `var(--green)` |
| Baptism | ✟ | `var(--green)` |
| Death | ✝ | `var(--blue)` |
| Burial | ⚰ | `var(--blue)` |
| Cremation | ⚰ | `var(--blue)` |
| Marriage | 💍 | `var(--orange)` |
| Divorce | ⚖ | `var(--orange)` |
| Engagement | 💍 | `var(--orange)` |
| Residence | 🏡 | `var(--text-secondary)` |
| Occupation | ⚒ | `var(--text-secondary)` |
| Census | 📋 | `var(--text-secondary)` |
| Other | 📜 | `var(--text-secondary)` |

Used throughout: [Tree View](ui-genealogy-tree.md) (events sidebar, person cards), [Person Profile](ui-person-profile.md) (timeline), [Person Edit Modal](ui-person-edit-modal.md) (event blocks).

---

## 8. EmptyState

A placeholder component displayed when a list or area has no content.

### Structure

```
┌────────────────────────────────┐
│            [icon]              │
│                                │
│        Title text              │
│    Subtitle / description      │
│                                │
│       [Optional action]        │
└────────────────────────────────┘
```

### Properties

| Prop | Type | Description |
|------|------|-------------|
| `icon` | String | Emoji or icon to display |
| `title` | String | Bold heading |
| `subtitle` | String | Muted description |
| `action_label` | Option<String> | Button label (if an action is relevant) |
| `on_action` | Option<EventHandler<()>> | Action callback |

### Used by

- [Homepage](ui-home.md) — no trees, no search results
- [Search Results](ui-search-results.md) — no results
- [Person Profile](ui-person-profile.md) — no events, no media
- [Settings](ui-settings.md) — empty anomalies list, empty duplicates list
