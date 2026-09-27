---
type: "UI Specification"
title: "Visual & Functional Specifications — Person History"
description: "Every recorded version of a person, compared field by field side by side, with the restore of an earlier one."
tags: [oxidgene, specification, ui, ux, history]
generated: { by: claude-code/claude-opus-5-5, at: 2026-09-27T19:00:00Z }
---


# Visual & Functional Specifications — Person History

> Part of the [OxidGene Specifications](index.md).
> See also: [Person Profile](ui-person-profile.md) · [Settings §11](ui-settings.md#11-section-history) (the tree's audit log) · [Common UI §4.10](ui-common.md#410-versiondiff) · [Data Model §5](data-model.md#5-change-history) · [API Contract](api.md)

---

## 1. Overview

The person history page (`/trees/{id}/persons/{person_id}/history`) lists every
version recorded for one person, newest first, and compares any of them with an
earlier one field by field. From it, the person can be put back as any earlier
version had them.

What a version holds — names, own events with their witnesses, notes and
sources, notes, sources, parents, and unions with their events — and when one is
recorded are defined in [Data Model §5](data-model.md#5-change-history). Media
are never part of a version.

It is reached from the **History** button of the
[Person Profile](ui-person-profile.md) header, and from the audit log of
[Settings §11](ui-settings.md#11-section-history). Only this page's own data is
read — never the person's profile — so it stays available after the person has
been deleted, which is how a deleted person is brought back.

---

## 2. Layout

Uses the standard `sub-page` layout with the shared left icon sidebar
([Common UI §6.3](ui-common.md)); the profile icon is disabled while the person
is deleted.

```
+----------------------------------------------------------------------+
| [logo] tree_name / <person> / History                                 |
+----------------------------------------------------------------------+
| isb |  History of <person>                     [Deleted]              |
|     |  +--------------+  +-------------------------------------------+ |
|     |  | Version 5    |  | Compare with [Version 4 v] [x] Changes only| |
|     |  | 27 Sep, 14:32|  |                         [Restore this ver.]| |
|     |  | Changed —    |  |  Names                                     | |
|     |  |  event (Birth)|  |           Version 4     Version 5         | |
|     |  +--------------+  |  Given    <old>          <new>              | |
|     |  | Version 4    |  |  ...                                        | |
|     |  | ...          |  +-------------------------------------------+ |
|     |  [Load more]                                                     |
+----------------------------------------------------------------------+
```

Below `768px` the version list becomes a horizontally scrolling row above the
comparison.

---

## 3. Topbar

`[logo] tree_name / <person> / History`. The person's name is taken from the
newest version's primary name, and links to their profile unless they are
deleted, when it is plain text.

---

## 4. Version list

One button per version, newest first, loaded 100 at a time with **Load more**.
Each shows:

- **Version N**;
- when it was recorded, in the reader's language and local time;
- what produced it, worded from its audit entry: *Changed — event (Birth)*,
  *Import*, *Restored*, *State at the start of history*;
- the entry's details when it has any: format, file name and person count of an
  import, *version 3 restored*, *merged with <name>*.

The newest version is selected on arrival. Selecting another shows it and resets
the comparison to the version just before it.

---

## 5. Comparison

A card holding a toolbar and a [VersionDiff](ui-common.md#410-versiondiff):

- **Compare with** — every version older than the one shown, defaulting to the
  one immediately before it. A first version has none and is compared with
  nothing: everything reads as *Added*.
- **Changes only** — on by default.
- **Restore this version** — shown only for a version that is neither the
  newest nor a deletion.

A version recorded by a deletion is shown like any other, under the deletion
banner, but cannot be restored: the version before it is the state to go back
to.

---

## 6. Restoring

**Restore this version** opens a `ConfirmDialog` stating that the person will be
put back exactly as that version recorded them — names, events, notes, sources,
parents and unions — and that the restore is itself recorded, so it can be
undone. Confirming calls the revert operation of the [API](api.md).

On success the list reloads, the restore appears as the newest version with
*Restored* and *version N restored*, and the page selects it. Restoring a
deleted person undeletes them: the topbar name becomes a link to their profile
again. A failure keeps the dialog open with the error.

What a restore writes, and what it deliberately leaves alone — another person
who no longer exists is never brought back — is defined in
[Data Model §5.3](data-model.md#53-restoring-a-version).

---

## 7. States

| State | Display |
|---|---|
| Loading | The shared loading line |
| Error | The shared error line with the message |
| No version | An empty state: *No version has been recorded for this person yet.* |
| Deleted person | A *Deleted* badge beside the title |

---

## 8. Accessibility

- Every version is a button; the selected one carries the active style and the
  comparison follows the selection.
- The comparison is a real table with a row header per field and a column
  header per version.
- The restore dialog is the shared `ConfirmDialog`, focus-trapped and
  cancellable with Escape.
