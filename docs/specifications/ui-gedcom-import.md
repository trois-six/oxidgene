---
type: "UI Specification"
title: "Visual & Functional Specifications — GEDCOM Import"
description: "UI behavior and interaction specification for Visual & Functional Specifications — GEDCOM Import."
tags: [oxidgene, specification, ui, ux]
timestamp: 2026-06-17T00:00:00Z
---


# Visual & Functional Specifications — GEDCOM Import

> Part of the [OxidGene Specifications](index.md).
> See also: [Tree View](ui-genealogy-tree.md) (import button in topbar) · [Settings](ui-settings.md) (export section) · [API Contract](api.md) (GEDCOM endpoints) · [Data Model](data-model.md)

---

## 1. Overview

The GEDCOM import flow allows users to upload a GEDCOM file (`.ged`) and import its contents into an existing tree or a new tree. The flow is a **multi-step wizard** covering file selection, preview, and import results.

The import can be triggered from:

- The [Tree View](ui-genealogy-tree.md) **topbar**: "Import" button (imports into the current tree)
- The [Homepage](ui-home.md) **"+ New tree" modal**: an additional option "Import from GEDCOM" (creates a new tree from the file)

---

## 2. Wizard Steps

```
Step 1: Upload          Step 2: Preview          Step 3: Results
┌──────────────┐       ┌──────────────┐         ┌──────────────┐
│ Select file   │  →    │ Review data  │   →     │ Import       │
│ & validate    │       │ before import│         │ summary      │
└──────────────┘       └──────────────┘         └──────────────┘
```

The wizard is displayed in a large modal (~800px wide, max-height 90vh). A progress bar at the top indicates the current step.

---

## 3. Step 1 — Upload & Validate

```
┌─────────────────────────────────────────────────────────────┐
│  Import GEDCOM                                         [×]  │
│  Step 1 of 3 — Select file                                 │
│  ═══════════●─────────────────────                         │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  ┌────────────────────────────────────────────────────────┐ │
│  │                                                        │ │
│  │         📄                                             │ │
│  │                                                        │ │
│  │    Drag and drop a GEDCOM file here                    │ │
│  │    or click to browse                                  │ │
│  │                                                        │ │
│  │    Accepted: .ged (GEDCOM 5.5.1 or 7.0)               │ │
│  │    Maximum size: 50 MB                                 │ │
│  │                                                        │ │
│  └────────────────────────────────────────────────────────┘ │
│                                                              │
├─────────────────────────────────────────────────────────────┤
│                                    [Cancel]   [Next →]      │
└─────────────────────────────────────────────────────────────┘
```

### Upload zone

- Drag-and-drop area with dashed border
- Clicking opens the system file picker (filter: `.ged`)
- On drag-over: border turns orange, background tints slightly

### After file selection

The upload zone is replaced by a file summary:

```
┌────────────────────────────────────────────────────────────┐
│  📄 famille-martin.ged                          [Remove]   │
│     Size: 2.4 MB                                           │
│     Format: GEDCOM 5.5.1 (auto-detected)                  │
│     Encoding: UTF-8                                        │
│                                                            │
│  ✓ File is valid                                           │
│                                                            │
│  ⚠ 3 warnings found during validation:                    │
│    • Line 1245: Unknown tag _CUSTOM ignored                │
│    • Line 3021: Date "ABT 1842" normalized to "c. 1842"   │
│    • Line 4102: Place "Beaune" has no country qualifier    │
└────────────────────────────────────────────────────────────┘
```

**Validation** is performed client-side (format detection, basic structure check) and server-side (full parse). Validation happens immediately after file selection.

**States**:
- ✓ **Valid**: green check, "Next" button enabled
- ⚠ **Valid with warnings**: amber warning icon, warnings listed, "Next" button enabled
- ✗ **Invalid**: red error icon, error message displayed, "Next" button disabled

**Possible errors**:
- File is not a valid GEDCOM file
- File exceeds maximum size (50 MB)
- File encoding is not supported
- File is corrupted or truncated

### Import mode (when importing into an existing tree)

When triggered from the tree view, a radio group appears below the file summary:

| Option | Description |
|---|---|
| **Add to existing data** (default) | Imported persons and families are added alongside existing data. No existing records are modified. |
| **Replace all data** | All existing data in the tree is deleted before import. Requires confirmation. |

---

## 4. Step 2 — Preview

A summary of what the GEDCOM file contains, before committing the import.

```
┌─────────────────────────────────────────────────────────────┐
│  Import GEDCOM                                         [×]  │
│  Step 2 of 3 — Preview                                     │
│  ───────────═══════════●───────                            │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  FILE SUMMARY                                                │
│  ┌───────────────────┐ ┌───────────────────┐                │
│  │  👤 Persons        │ │  👨‍👩‍👧‍👦 Families    │                │
│  │     142            │ │     48             │                │
│  └───────────────────┘ └───────────────────┘                │
│  ┌───────────────────┐ ┌───────────────────┐                │
│  │  📅 Events         │ │  📎 Sources        │                │
│  │     387            │ │     23             │                │
│  └───────────────────┘ └───────────────────┘                │
│  ┌───────────────────┐ ┌───────────────────┐                │
│  │  📍 Places         │ │  🖼 Media refs     │                │
│  │     65             │ │     12             │                │
│  └───────────────────┘ └───────────────────┘                │
│                                                              │
│  SAMPLE PERSONS (first 10)                                   │
│  ┌────────────────────────────────────────────────────────┐ │
│  │  MARTIN Jean-Baptiste    M   ✦ 1842   ✝ 1918         │ │
│  │  LEMAIRE Marguerite       F   ✦ 1845   ✝ 1920         │ │
│  │  MARTIN Henri             M   ✦ 1868   ✝ 1942         │ │
│  │  ...                                                   │ │
│  │  [Show all 142 persons]                                │ │
│  └────────────────────────────────────────────────────────┘ │
│                                                              │
│  WARNINGS (3)                                                │
│  ⚠ 3 non-critical issues (same as Step 1)                  │
│  [Show details]                                              │
│                                                              │
├─────────────────────────────────────────────────────────────┤
│                  [← Back]   [Cancel]   [Import]             │
└─────────────────────────────────────────────────────────────┘
```

### Stat cards

Six metric cards in a 2×3 grid showing counts for persons, families, events, sources, places, and media references.

### Sample persons

A collapsible list showing the first 10 persons that will be imported, with basic info (name, sex, birth/death). A **"Show all"** link expands to a scrollable list of all persons.

### Warnings

Same warnings from Step 1, collapsed by default. A count badge is shown.

### Import button

The **"Import"** button is prominent (orange gradient). Clicking it starts the import process and advances to Step 3.

---

## 5. Step 3 — Import Progress & Results

### During import

```
┌─────────────────────────────────────────────────────────────┐
│  Import GEDCOM                                         [×]  │
│  Step 3 of 3 — Importing…                                  │
│  ─────────────────────═══════════●                         │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  ┌────────────────────────────────────────────────────────┐ │
│  │  ████████████████████░░░░░░░░░░░░  67%                │ │
│  └────────────────────────────────────────────────────────┘ │
│                                                              │
│  Importing persons… (95 / 142)                              │
│                                                              │
│  ✓ Persons: 95 imported                                     │
│  ◌ Families: pending                                         │
│  ◌ Events: pending                                           │
│  ◌ Sources: pending                                          │
│  ◌ Places: pending                                           │
│  ◌ Media references: pending                                 │
│  ◌ Family links: pending                                     │
│  ◌ Ancestry closure table: pending                           │
│                                                              │
├─────────────────────────────────────────────────────────────┤
│  Import in progress — please wait…                          │
└─────────────────────────────────────────────────────────────┘
```

- **Progress bar**: overall progress percentage
- **Step-by-step log**: each entity type shows its status (✓ done / ◌ pending / ⟳ in progress)
- The close button `×` and Cancel are **disabled** during import
- The modal cannot be dismissed until import completes or fails

### After completion — Success

```
┌─────────────────────────────────────────────────────────────┐
│  Import GEDCOM                                         [×]  │
│  Step 3 of 3 — Complete                                    │
│  ─────────────────────═══════════●                         │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  ✓ Import completed successfully                            │
│                                                              │
│  ┌───────────────────┐ ┌───────────────────┐                │
│  │  👤 Persons        │ │  👨‍👩‍👧‍👦 Families    │                │
│  │     142 imported   │ │     48 imported    │                │
│  └───────────────────┘ └───────────────────┘                │
│  ┌───────────────────┐ ┌───────────────────┐                │
│  │  📅 Events         │ │  📎 Sources        │                │
│  │     387 imported   │ │     23 imported    │                │
│  └───────────────────┘ └───────────────────┘                │
│  ┌───────────────────┐ ┌───────────────────┐                │
│  │  📍 Places         │ │  🖼 Media refs     │                │
│  │     65 imported    │ │     12 imported    │                │
│  └───────────────────┘ └───────────────────┘                │
│                                                              │
│  ⚠ 3 warnings during import                                │
│  [Show details]                                              │
│                                                              │
├─────────────────────────────────────────────────────────────┤
│                                      [View tree]            │
└─────────────────────────────────────────────────────────────┘
```

**"View tree"** closes the modal and navigates to the tree view. If a root person was identified in the GEDCOM file (first INDI record), the tree is centered on that person.

### After completion — Partial failure

If some records failed to import:

```
│  ⚠ Import completed with errors                             │
│                                                              │
│  142 persons imported · 2 failed                             │
│  48 families imported · 0 failed                             │
│  ...                                                         │
│                                                              │
│  ERRORS (2)                                                  │
│  ✗ Person at line 2045: invalid date format "32/13/1842"    │
│  ✗ Person at line 3891: circular family reference           │
│                                                              │
│  Successfully imported data is available in the tree.        │
│  Failed records were skipped.                                │
```

### After completion — Total failure

```
│  ✗ Import failed                                             │
│                                                              │
│  The file could not be imported. No data was modified.       │
│                                                              │
│  Error: Database connection lost during import.              │
│  Please try again.                                           │
│                                                              │
├──────────────────────────────────────────────────────────────┤
│                               [Close]   [Retry]             │
```

On total failure, the import is rolled back — no partial data is left in the tree.

---

## 6. Import into New Tree

When triggered from the [Homepage](ui-home.md):

- Step 1 includes an additional field: **Tree name** (required, text input above the upload zone)
- The tree is created first, then the import proceeds into it
- On failure, the empty tree is also deleted (full rollback)
- On success, the "View tree" button navigates to the new tree

---

## 7. Data Mapping & Fidelity

The import uses `ged_io` 0.12 to parse GEDCOM files. See [API Contract](api.md) §3 for the full round-trip fidelity table.

### What imports cleanly

| GEDCOM records | Mapping |
|---|---|
| INDI (individuals) | → Person + PersonName(s) + Events |
| FAM (families) | → Family + FamilySpouse + FamilyChild links |
| All standard event tags (BIRT, DEAT, BAPM, MARR, etc.) | → Event with matching EventType |
| SOUR (sources) | → Source (title, author, publisher, abbreviation) |
| Citation references with QUAY | → Citation with Confidence mapping |
| NOTE (notes) | → Note linked to the parent record |
| OBJE (multimedia) | → Media (file path + MIME type + title, metadata only) |
| PLAC with MAP coordinates | → Place (name + latitude + longitude) |
| Event CAUS (cause) | → Event.cause field |
| FAMC PEDI (pedigree type) | → FamilyChild.child_type (Biological / Adopted / Foster) |

### What is skipped (not imported)

These GEDCOM tags are parsed by ged_io but not mapped to the OxidGene data model. They are listed in the import warnings.

| GEDCOM tag | Description | Reason |
|---|---|---|
| REPO | Repository records | Not in current data model |
| SUBM | Submitter records | Not relevant (single-user MVP) |
| AGE | Age at event | Not stored; can be calculated from dates |
| RELI | Religion of event | Not in current data model |
| AGNC | Agency responsible | Not in current data model |
| ASSO | Associations between individuals | Not in current data model |
| `_CUSTOM` tags | Vendor-specific extensions | Silently ignored |

### GEDCOM version handling

- **Import**: ged_io auto-detects GEDCOM 5.5.1 and 7.0 formats
- **Export**: always produces GEDCOM 5.5.1 (LINEAGE-LINKED format)
- Files imported from GEDCOM 7.0 will export as 5.5.1 — no data is lost in the conversion for supported tags

---

## 8. File Size Handling

| File size | Behavior |
|---|---|
| < 1 MB | Instant upload, synchronous import |
| 1–10 MB | Upload progress bar visible, synchronous import |
| 10–50 MB | Upload progress bar + streaming import with step-by-step progress |
| > 50 MB | Rejected at upload with error message |

---

## 8. Keyboard & Accessibility

| Key | Behavior |
|---|---|
| `Escape` | Close wizard (disabled during import) |
| `Enter` | Proceed to next step / Confirm import |
| `Tab` | Navigate between controls |

---

## 9. Responsive

- Below **600px**: stat cards in Step 2 and 3 collapse to a 1-column list
- Upload zone is always full-width
- Progress log uses a compact single-line format per entity type
