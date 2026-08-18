---
type: "UI Specification"
title: "Visual & Functional Specifications — Import"
description: "The import modal: choosing a file, or importing a Geneanet tree with its media."
tags: [oxidgene, specification, ui, ux]
timestamp: 2026-08-18T00:00:00Z
---


# Visual & Functional Specifications — Import

> Part of the [OxidGene Specifications](index.md).
> See also: [Geneanet Import](ui-geneanet-import.md) (the Geneanet tab in full) ·
> [Geneanet Media Import](geneanet-media-import.md) (the pipeline behind it) ·
> [Homepage](ui-home.md) · [Settings](ui-settings.md) (export) ·
> [API Contract](api.md)

---

## 1. Overview

Everything that brings data *into* a tree arrives through one modal, opened
from a tree card's `⋮` → **Import** on the [Homepage](ui-home.md) or from
[Settings](ui-settings.md) → Tools. It never creates a tree: it is opened from
a tree's own menu, so the destination is already chosen.

Two tabs, because there are two genuinely different jobs:

| Tab | What it is |
|---|---|
| **A file** | Pick or drop a `.ged` or `.gw` and import it. Seconds, one decision. |
| **From Geneanet** | Five steps, most of them instructions. The user has work to do on another website first, and two of the three inputs cannot be downloaded at all. Specified in full in [Geneanet Import](ui-geneanet-import.md). |

Splitting them across a modal and a page was tried and rejected: they are one
decision made at one moment, and separating them made the cheap route feel like
the real one and the complete route like an excursion.

---

## 2. The modal

~820 px wide, `max-height: 90vh`, header + tab strip fixed while the body
scrolls.

```
┌─ Import into "Famille Dupont" ────────────────────────── [×] ─┐
│  ● A file        ○ From Geneanet                              │
├───────────────────────────────────────────────────────────────┤
│  ┌─────────────────────────────────────────────────────────┐  │
│  │   📄  Drop a file here, or click to browse               │  │
│  │       .ged (GEDCOM) · .gw (GeneWeb)                      │  │
│  └─────────────────────────────────────────────────────────┘  │
│                                              [ Import ]       │
└───────────────────────────────────────────────────────────────┘
```

**It cannot be dismissed while an import is running.** Neither the backdrop nor
the ✕ responds — dismissing would throw away however many steps had been
settled and orphan a request still writing to the tree, and a stray click on
the page behind is all it would take.

Dismissal is on *press*, not click: a click fires on the common ancestor of
mousedown and mouseup, so selecting text inside the modal and releasing outside
it would otherwise close the modal.

---

## 3. The file tab

A drop zone that is also a button. Accepts one `.ged` or `.gw`; drag-and-drop
and the native picker both work.

The bytes are read with the file handle's `read()`, never a path. A file picked
in a browser has no path at all, and this used to read one — which quietly made
"the shared web/desktop import" desktop-only, with nothing saying so.

`.gw` and `.ged` are told apart by extension and sent to different endpoints,
because a `.gw` is ISO-8859-1 unless it declares otherwise and only its reader
can decode it. See [API Contract §Import](api.md).

### What this tab does not do

An earlier version of this spec described a three-step wizard — upload and
validate, preview with sample persons and stat cards, then import — and **none
of it was built**. What exists is: choose, import, see the counts.

That is recorded rather than quietly dropped, because the preview step is
genuinely useful for a GEDCOM from an unknown source. It is unbuilt, not
rejected. The Geneanet tab has its own preview (step 4) because there the
question — *do these two halves belong to each other?* — has a real answer that
cannot be guessed at.

### Result

The counts every import reports, whatever the source: people, families, events,
sources, places, media. Warnings collapse behind a disclosure with a count.

---

## 4. Errors

| Condition | Message |
|---|---|
| A `.ged` that is not UTF-8 | *"This GEDCOM file is not valid UTF-8. Re-export it as UTF-8 from your genealogy software, or import the GeneWeb (.gw) version instead."* |
| The file cannot be read | The reader's own message |
| The import fails | The API's message, and the modal stays open with the file still chosen |

---

## 5. Data mapping & fidelity

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

## 6. File size handling

| File size | Behavior |
|---|---|
| < 1 MB | Instant upload, synchronous import |
| 1–10 MB | Upload progress bar visible, synchronous import |
| 10–50 MB | Upload progress bar + streaming import with step-by-step progress |
| > 50 MB | Rejected at upload with error message |

---

## 7. Keyboard & accessibility

| Key | Behavior |
|---|---|
| `Escape` | Close the modal — inert while an import is running |
| `Enter` | Activate the focused control |
| `Tab` | Move between controls |

The Geneanet tab's own keyboard behaviour — collapsed steps as buttons, focus
into the expanded step, live regions on the progress bars — is in
[Geneanet Import §10](ui-geneanet-import.md).

---

## 8. Responsive

- The modal is `min(820px, 94vw)` wide; its body is the only thing that scrolls
- Stat rows wrap 4 → 2 → 1 at 900 px and 560 px
- The drop zone is always full width
