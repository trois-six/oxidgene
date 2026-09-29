---
type: "UI Specification"
title: "Visual & Functional Specifications — Person Merge"
description: "Three-step wizard to find the other record of a person, compare the two, choose the record kept and the events and media taken from the other, and confirm the merge."
tags: [oxidgene, specification, ui, ux]
generated: { by: claude-code/claude-opus-5-5, at: 2026-09-29T20:19:37Z }
---


# Visual & Functional Specifications — Person Merge

> Part of the [OxidGene Specifications](index.md).
> See also: [Tree View](ui-genealogy-tree.md) (action picker "Merge with…") · [Person Profile](ui-person-profile.md) · [Person Edit Modal](ui-person-edit-modal.md) · [Tools](ui-tools.md) (potential duplicates) · [Data Model](data-model.md#person-merge) · [API Contract](api.md)

---

## 1. Overview

The merge wizard combines two records of one person into one. It is a
**three-step modal**: find the other record, compare the two and choose what
to keep, then confirm. One component, `MergeDialog`, serves every entry:

| Entry | Opens on |
|---|---|
| **Merge with…** in the [Tree View](ui-genealogy-tree.md) action picker | Step 1, the selected person as the source |
| **Merge with…** in the [Person Profile](ui-person-profile.md) header | Step 1, the profile's person as the source |
| **Compare** on a pair of the [Tools](ui-tools.md) potential duplicates (§6) | Step 2, the pair's first record as the source, with **Two different people** |

Two lighter flows merge without the wizard, through the shared homonym
picker and the operation of §6 with nothing left out: the homonym check that
follows every save of the [Person Edit Modal](ui-person-edit-modal.md) §13,
and the receipt of a Geneanet import ([Import](ui-import.md) §9.7). Both keep
the pre-existing person.

---

## 2. Layout

A modal card up to 760px wide, scrolling within 90% of the viewport height.
Under the title, a line reads the step: "Step 2 of 3 · Compare and choose".
The footer holds, in order: **Two different people** (Step 2 from Tools
only), **Back** (Step 3, and Step 2 when Step 1 was shown), **Cancel**, and
the step's own action. Clicking the backdrop does nothing; Cancel closes.

---

## 3. Step 1 — Select

A hint and the shared person search of the
[tree topbar](ui-genealogy-tree.md), limited to the current tree. Choosing a
result other than the source person moves to Step 2; choosing the source
itself is ignored.

---

## 4. Step 2 — Compare and choose

Both records are loaded together — each one's projection and detail bundle —
and drawn side by side as the search results draw a person (portrait, name,
dates, relatives, birthplace), the source first.

- **The record kept.** Each card carries a "Keep this record" radio; the
  source is kept by default. The other record is *absorbed*. The record kept
  keeps its identifier, its links and whatever the rows below do not change.
- **The comparison.** One table row per field, the source's value on the
  left; a row whose two values differ is highlighted in orange. Every row
  where the absorbed record has something to offer carries a radio in each
  column, and the value picked is the merged record's:

  | Row | Choice | Picked by default |
  |---|---|---|
  | Surname (particle included) | When the absorbed record has one that differs | The kept one's, unless it has none |
  | Given names | When the absorbed record has some that differ | The kept one's, unless it has none |
  | Sex | When both are known and differ; a sex only one record knows wins without a choice | The kept one's |
  | Birth, baptism, death, burial, cremation — one row each, shown when either record has one — written as date, place and description | When the absorbed record has one | The one that says more: the pieces of its date, an exact date, a place; the kept one's when equal |

  A person has each of those events once, so the row keeps one record's and
  leaves out the other's. The father, mother, spouses and number of children
  follow for reference: every family link moves to the record kept.
- **Other events.** The absorbed record's own repeatable events (not its
  unions'), one checkbox each, written as type, date, place and description.
  One the kept record already has — same type, date and place — is marked
  "already on the record kept" and left unticked; everything else is ticked.
- **Media to attach.** The absorbed record's media linked to the person
  directly (not through a union), one checkbox each by title or file name. A
  medium the kept record is already linked to is marked the same way; the
  others are ticked by default.
- Choosing the other record as kept recomputes every default for it.
- A note says that names, family links, notes and sources always move to the
  record kept.

**Next** moves to Step 3.

---

## 5. Step 3 — Confirm

A summary — the record kept; the primary name it will bear; the events and
media taken from the other, counted; those left out of either record,
counted — and the warning that the merge cannot be undone, naming the record
deleted and the record kept. **Merge** is the destructive button. Failure
leaves the wizard open with an error line.

---

## 6. Merge execution

**Merge** calls the merge operation —
`POST /trees/{tree_id}/persons/{kept}/merge` with `{duplicate_id, choices}`,
or `mergePersons` ([API Contract](api.md)). `choices` names the events left
out of either record (the absorbed record's unticked or unpicked events, and
the kept record's events replaced by a row picked from the absorbed one), the
absorbed record's unticked media links, and whether the surname, the given
names and the sex come from the absorbed record. In one transaction the
operation drops what was left out (events soft-deleted, media links removed,
media kept in the library), moves everything else the absorbed record
carried onto the kept one, applies the picked sex, and gives the kept record
the primary name composed of the picked surname and given names — the former
primary name staying as a secondary one — then soft-deletes the absorbed
record, following [Data Model — Person merge](data-model.md#person-merge):
names, events, family links, witness and media links, notes, citations,
identification boxes, the tree roots and the distinct-person confirmations.
The projections of both persons' relatives are rebuilt and the history
records the merge.

Afterwards:

| Entry | Then |
|---|---|
| Tree View | The pedigree reloads; if the absorbed record was its root, the kept one becomes the root |
| Person Profile | The profile of the kept record, reloaded or navigated to in place of the absorbed one |
| Tools | The duplicates are computed again |

**Two different people** records a distinct-person confirmation, as the
homonym check does, and closes the wizard.

---

## 7. Edge cases

| Case | Behavior |
|---|---|
| Both persons are children of the same family | The merge proceeds; the family keeps one child link |
| The absorbed record is a tree root | The kept record takes its place |
| The two persons are spouses of the same family, or one is the other's ancestor | The merge is refused: it would leave a person married to, or descended from, themselves |

---

## 8. Responsive

At **640px** and below, the two cards stack; the comparison table and the
lists keep their single column and wrap long values.
