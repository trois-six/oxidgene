---
type: "UI Specification"
title: "Visual & Functional Specifications — Person Merge"
description: "UI behavior and interaction specification for Visual & Functional Specifications — Person Merge."
tags: [oxidgene, specification, ui, ux]
timestamp: 2026-06-17T00:00:00Z
---


# Visual & Functional Specifications — Person Merge

> Part of the [OxidGene Specifications](index.md).
> See also: [Tree View](ui-genealogy-tree.md) (action picker "Merge with…") · [Person Edit Modal](ui-person-edit-modal.md) · [Settings](ui-settings.md) (potential duplicates tool) · [Data Model](data-model.md) (Person, PersonName, Event, Family)

---

## 1. Overview

The merge flow allows combining two person records that represent the same individual into a single unified record. It is a **multi-step wizard** that guides the user through selecting the duplicate, comparing data side by side, and choosing which values to keep.

The merge flow can be triggered from:

- The [Tree View](ui-genealogy-tree.md) **action picker**: "Merge with…" on a selected person
- The [Settings](ui-settings.md) **Potential Duplicates** tool: "Merge" button on a detected pair

---

## 2. Wizard Steps

```
Step 1: Select          Step 2: Compare          Step 3: Confirm
┌──────────────┐       ┌──────────────┐         ┌──────────────┐
│ Search for    │  →    │ Side-by-side │   →     │ Review &     │
│ the duplicate │       │ field picker │         │ confirm      │
└──────────────┘       └──────────────┘         └──────────────┘
```

The wizard is displayed in a large modal (~900px wide, max-height 90vh). A progress bar at the top indicates the current step. Back navigation is available at each step.

---

## 3. Step 1 — Select Duplicate

### When triggered from the action picker

The source person is already selected (the person on which "Merge with…" was clicked). The user must select the **target person** to merge into.

```
┌─────────────────────────────────────────────────────┐
│  Merge MARTIN Jean-Baptiste                    [×]  │
│  Step 1 of 3 — Select the duplicate                 │
│  ═══════════●─────────────────────                  │
├─────────────────────────────────────────────────────┤
│                                                      │
│  SOURCE PERSON (kept)                               │
│  ┌────────────────────────────────────────────────┐ │
│  │ [photo] MARTIN Jean-Baptiste  ✦ 1842  ✝ 1918  │ │
│  └────────────────────────────────────────────────┘ │
│                                                      │
│  Search for the duplicate to merge:                  │
│  [Last name ________] [First name ________]         │
│                                                      │
│  Results:                                            │
│  ○ [photo] MARTIN Jean-Bapt.   ✦ 1842  ✝ 1918     │
│  ○ [photo] MARTIN Jean         ✦ 1843              │
│  ○ [photo] MARTIN J.B.         ✦ c.1840  ✝ 1918   │
│                                                      │
├─────────────────────────────────────────────────────┤
│                               [Cancel]   [Next →]   │
└─────────────────────────────────────────────────────┘
```

- Search uses the same person search as the [tree topbar](ui-genealogy-tree.md), filtered to the current tree
- The source person is excluded from results
- The user selects one person via radio button
- **Next** is disabled until a target is selected

### When triggered from Potential Duplicates

Both persons are pre-selected (source and target). Step 1 is skipped — the wizard opens directly at Step 2.

---

## 4. Step 2 — Compare & Choose

A side-by-side comparison of all fields from both persons. For each field or group of fields, the user chooses which value to keep in the merged result.

```
┌─────────────────────────────────────────────────────────────────┐
│  Merge MARTIN Jean-Baptiste                                [×]  │
│  Step 2 of 3 — Compare and choose                              │
│  ───────────═══════════●───────                                │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  ┌── Person A (source) ────┐  ┌── Person B (target) ────────┐  │
│  │  [photo]                 │  │  [photo]                     │  │
│  │  MARTIN Jean-Baptiste    │  │  MARTIN Jean Bapt.           │  │
│  │  ✦ 12/03/1842            │  │  ✦ 1842                      │  │
│  │  ✝ 07/11/1918            │  │  ✝ 1918                      │  │
│  └──────────────────────────┘  └──────────────────────────────┘  │
│                                                                  │
│  SURNAME                                                         │
│  ● [A] MARTIN                  ○ [B] MARTIN               ═     │
│                                                                  │
│  FIRST NAME(S)                                                   │
│  ● [A] Jean-Baptiste           ○ [B] Jean Bapt.                 │
│                                                                  │
│  GENDER                                                          │
│  ● [A] Male                    ○ [B] Male                  ═    │
│                                                                  │
│  BIRTH DATE                                                      │
│  ● [A] 12/03/1842              ○ [B] 1842                       │
│                                                                  │
│  BIRTH PLACE                                                     │
│  ○ [A] Beaune                  ● [B] Beaune, Côte-d'Or          │
│                                                                  │
│  DEATH DATE                                                      │
│  ● [A] 07/11/1918              ○ [B] 1918                       │
│                                                                  │
│  ... (more fields)                                               │
│                                                                  │
│  EVENTS (combined)                                               │
│  ☑ ✦ Birth — 12/03/1842, Beaune              from A             │
│  ☑ 💍 Marriage — 1865                         from A             │
│  ☑ ⚒ Occupation — Vigneron                    from B            │
│  ☑ ✝ Death — 07/11/1918                       from A            │
│  ☐ ✦ Birth — 1842 (duplicate)                 from B            │
│                                                                  │
│  FAMILY LINKS (combined)                                         │
│  ☑ Spouse: LEMAIRE Marguerite                 from A             │
│  ☑ Child: MARTIN Henri                        from A & B         │
│                                                                  │
├─────────────────────────────────────────────────────────────────┤
│                          [← Back]   [Cancel]   [Next →]         │
└─────────────────────────────────────────────────────────────────┘
```

### Field comparison rules

| Field type | Selection | Default |
|---|---|---|
| **Single-value fields** (name, gender, dates, places) | Radio: choose A or B | The more complete/precise value is pre-selected (e.g. full date over year-only) |
| **Identical values** | Shown with `═` indicator, no choice needed | Auto-kept |
| **Events** | Checkbox list: keep or discard each | All kept by default. Probable duplicates (same type + similar date) are flagged and the less precise one is unchecked |
| **Family links** | Checkbox list: keep or discard | All kept by default. Conflicting links (e.g. two different spouse sets for the same union) are highlighted in orange |
| **Media** | Checkbox list | All kept by default |
| **Notes** | Checkbox list | All kept by default (concatenated) |

### Conflict detection

When both persons have events of the same type with similar dates (within 1 year), the system flags them as **probable duplicates**:
- A warning icon appears next to the pair
- The less precise entry is unchecked by default
- The user can override by checking both (they will both be kept)

---

## 5. Step 3 — Review & Confirm

A preview of the merged result, shown as a read-only person profile card.

```
┌─────────────────────────────────────────────────────────────────┐
│  Merge MARTIN Jean-Baptiste                                [×]  │
│  Step 3 of 3 — Review and confirm                              │
│  ─────────────────────═══════════●                             │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  MERGED RESULT                                                   │
│  ┌────────────────────────────────────────────────────────────┐  │
│  │  [photo]  MARTIN Jean-Baptiste                             │  │
│  │           Male                                             │  │
│  │           ✦ 12/03/1842, Beaune, Côte-d'Or                 │  │
│  │           ✝ 07/11/1918                                     │  │
│  │                                                            │  │
│  │  Events: Birth, Marriage (1865), Occupation, Death         │  │
│  │  Spouse: LEMAIRE Marguerite                                │  │
│  │  Children: MARTIN Henri, MARTIN Louise, MARTIN Pierre     │  │
│  │  Media: 3 items                                            │  │
│  │  Notes: 2 notes                                            │  │
│  └────────────────────────────────────────────────────────────┘  │
│                                                                  │
│  ⚠ This action cannot be undone. Person B will be permanently   │
│    removed and all references will point to the merged record.  │
│                                                                  │
├─────────────────────────────────────────────────────────────────┤
│                  [← Back]   [Cancel]   [Confirm merge]          │
└─────────────────────────────────────────────────────────────────┘
```

**"Confirm merge"** is styled as a destructive action (orange background, bold text).

---

## 6. Merge Execution

On confirmation:

1. **Person A** (source) is updated with the chosen field values
2. All kept events, media, notes, and citations from Person B are re-linked to Person A
3. All family links from Person B are transferred to Person A (FamilySpouse, FamilyChild)
4. **Person B** is soft-deleted
5. The `PersonAncestry` closure table is recalculated for affected subtrees
6. The modal closes
7. The tree view refreshes, centered on the merged person (Person A)

---

## 7. Edge Cases

| Case | Behavior |
|---|---|
| Both persons are in the same family | The merge proceeds; duplicate family links are deduplicated automatically |
| Person B is the current tree root (SOSA 1) | Warning: "This person is the tree root. After merge, Person A will become the new root." |
| Person B has unions that Person A doesn't | All unions are transferred to Person A |
| Merging would create an invalid relationship (e.g. person becomes their own parent) | The merge is blocked with an error message explaining the conflict |

---

## 8. Keyboard & Accessibility

| Key | Behavior |
|---|---|
| `Escape` | Close wizard (with confirmation if on step 2 or 3) |
| `Tab` | Navigate between radio buttons / checkboxes |
| `←` / `→` | Switch between A and B for the focused field |
| `Enter` | Proceed to next step |

---

## 9. Responsive

- Below **900px**: the side-by-side comparison in Step 2 switches to a **stacked layout** (Person A fields above, Person B below, with radio buttons between)
- Below **600px**: the wizard modal becomes a full-screen view
