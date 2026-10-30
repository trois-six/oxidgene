---
type: "Product Specification"
title: "Tags"
description: "Principles of tags shared by every kind of record, and where their uses beyond finding (statistics, anomalies and export limited to a tag) are specified."
tags: [oxidgene, specification, tags, data-model, api, ui]
generated: { by: human:maintainer, at: 2026-10-30T20:30:00Z }
---

# Tags

> Part of the [OxidGene Specifications](index.md).
> See also: [Data Model](data-model.md) · [API Contract](api.md) ·
> [Common UI](ui-common.md) · [Dictionary](ui-dictionary.md) ·
> [Genealogy Tree](ui-genealogy-tree.md) · [Roadmap](roadmap.md)

The tags themselves, their menus, badges, reads and GEDCOM placements are
specified where they apply: the vocabulary and the link in
[Data Model](data-model.md) (Tag, RecordTag), the endpoints in
[API Contract](api.md) (Tags, OxidGene extensions), the picker and the badges in
[Common UI §4.18](ui-common.md#418-tags-picker-and-badges), and each record's
menu and badges in [Person Profile](ui-person-profile.md),
[Couple Profile](ui-couple-profile.md) and [Dictionary](ui-dictionary.md). This
document keeps the principles that hold for every use of tags, and points to
the specifications of the uses beyond finding. All five phases of the tags
work are delivered ([Roadmap §11b](roadmap.md#11b-tags)).

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

Each use is specified on the surface it belongs to. Its counting and selection
rules are in the API contract, its page in the UI specification.

### 2.1 Statistics limited to a tag

The tag select of the [Statistics](ui-statistics.md) page, and the rules of
the figures a tag limits: [API Contract, Statistics](api.md#statistics).

The tree's tag list these selects and the pedigree's highlight list draw on is
read lazily: the first time a list that needs it opens (the select taking the focus,
the highlight button being pressed), or at once when the page opens on a tag kept
in its history entry. The read is kept for the page and not repeated on each
opening. A failed read is read again by the next opening; until then the list
names the failure, and a kept tag stays chosen.

### 2.2 Anomalies limited to a tag

The tag select of [Tools §3](ui-tools.md#3-anomalies), and the rule of an
anomaly that names a tagged person or a place such a person's events use:
[API Contract, Tools](api.md#tools).

### 2.3 Exporting part of the tree

This is also the subtree export of [Roadmap §7b](roadmap.md#7b-subtree-export).
The scope of an export is a bulk tagging target ([API Contract, Tags](api.md#tags))
with the addition of a tag, chosen in the export section of
[Tree Settings, Scope](ui-settings.md#scope). Its rules, what an export keeps and
the audit entry it writes are in [API Contract, Export scope](api.md#export-scope).
