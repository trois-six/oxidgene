---
type: "Product Specification"
title: "Tags"
description: "Planned uses of tags beyond finding: statistics, anomalies and export limited to a tag."
tags: [oxidgene, specification, tags, data-model, api, ui]
generated: { by: human:maintainer, at: 2026-10-19T12:13:38Z }
---

# Tags

> Part of the [OxidGene Specifications](index.md).
> See also: [Data Model](data-model.md) · [API Contract](api.md) ·
> [Common UI](ui-common.md) · [Dictionary](ui-dictionary.md) ·
> [Genealogy Tree](ui-genealogy-tree.md) · [Roadmap](roadmap.md)

The tags themselves, their menus, dots, reads and GEDCOM placements are
specified where they apply: the vocabulary and the link in
[Data Model](data-model.md) (Tag, RecordTag), the endpoints in
[API Contract](api.md) (Tags, OxidGene extensions), the picker, chips and dots in
[Common UI §4.18](ui-common.md#418-tags-picker-chips-and-dots), and each record's
menu and dots in [Person Profile](ui-person-profile.md),
[Couple Profile](ui-couple-profile.md) and [Dictionary](ui-dictionary.md). This
document keeps the principles that hold for every use of tags, and the uses
beyond finding that are still planned. Their delivery status is in
[Roadmap §11b](roadmap.md#11b-planned-tags).

---

## 1. Principles

- **One vocabulary per tree.** A tag exists once in a tree and is shared by
  every kind of record. Renaming, recolouring, merging or deleting it acts
  everywhere at once.
- **Tagging is not editing.** Tags are never fields of an edit form. They are
  added and removed from the record's menu, through the shared picker, and
  each change is written at once.
- **No room taken when unused.** A record without tags looks exactly as it
  does without this feature.
- **Flat.** Tags have no hierarchy: a colour and a name are enough.
- **User content.** OxidGene ships no predefined tag and never translates one
  ([Cross-cutting Rules](cross-cutting.md)).

Every record that exists on its own in the interface can carry a tag: persons,
couples, documents, events, places, sources, citations, repositories and notes.
Parts of a record do not carry tags: a person's names, a family's spouse and
child links, an event's witnesses and spouse ages, vignettes, media links and
kept GEDCOM. A multi-page document carries its tags on the document, never on
its pages. The tree itself carries none.

Every interface string of these uses (menu entries, confirmations, errors,
accessible names and empty states) goes through the translation tables in all
eight languages. Tag labels are user content and are never translated.

## 2. Uses beyond finding

- Statistics and anomalies limited to a tag ([Statistics](ui-statistics.md),
  [Tools §3](ui-tools.md#3-anomalies)).
- An export of the persons carrying a tag, with the subtree export of the
  [Roadmap](roadmap.md#7b-planned-subtree-export).

## 3. Tests still to write

- Statistics and anomalies limited to a tag, and the export of the persons
  carrying a tag, each with its REST and GraphQL coverage.
- Test data is fictitious ([Cross-cutting Rules](cross-cutting.md)).
