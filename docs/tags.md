---
type: "Product Specification"
title: "Tags"
description: "Planned tag extensions: highlighting a tag in the tree views, bulk tagging of persons from the tree and from search results, tags on events, sources, citations, places, repositories and notes, and the uses beyond finding (statistics, anomalies, export by tag)."
tags: [oxidgene, specification, tags, data-model, api, ui]
generated: { by: human:maintainer, at: 2026-10-18T17:09:22Z }
---

# Tags

> Part of the [OxidGene Specifications](index.md).
> See also: [Data Model](data-model.md) · [API Contract](api.md) ·
> [Common UI](ui-common.md) · [Dictionary](ui-dictionary.md) ·
> [Genealogy Tree](ui-genealogy-tree.md) · [Roadmap](roadmap.md)

**Status: phase 1 is delivered** (tags on persons, couples and documents, the
shared picker, chips and dots, the Dictionary's Tags tab, the person-search
filter, GEDCOM and GEDZIP, the audit log, the `list_tags` tool). Its contract
is in [Data Model](data-model.md) (Tag, RecordTag), [API Contract](api.md)
(Tags), [Common UI §4.18](ui-common.md#418-tags-picker-chips-and-dots),
[Dictionary §20](ui-dictionary.md#20-tags-tab) and the record pages. This
document keeps only what phases 2 to 5 still deliver ([§9](#9-phases)).

---

## 1. Principles

These hold for every phase:

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

Every record that exists on its own in the interface can carry a tag. The
kinds still to come:

| Record | Phase | Example |
|---|---|---|
| Event | 3 | *approximate date to refine* |
| Source | 3 | *fully abstracted* |
| Citation | 3 | *record not consulted*, *doubtful reading* |
| Place | 4 | *vanished parish*, *to locate* |
| Repository | 4 | *closed*, *online* |
| Note | 4 | *hypothesis*, *to write* |

Parts of a record do not carry tags: a person's names, a family's spouse and
child links, an event's witnesses and spouse ages, vignettes, media links and
kept GEDCOM. A multi-page document carries its tags on the document, never on
its pages. The tree itself carries none.

## 2. Remaining kinds

The model already accepts the nine kinds of `record_tag`, and the generic
endpoints already tag them ([API Contract](api.md), Tags). What each later
phase adds is the interface and the reads:

- **Phase 3** adds the `tags` field to the reads of events, sources and
  citations ([Data Model](data-model.md), Event, Source, Citation), and the
  **Tags…** entries of their menus: the context menu of the event's timeline
  row ([Person Profile](ui-person-profile.md)) and of its row in the couple's
  shared sections ([Couple Profile](ui-couple-profile.md)), the context menu of
  the citation on its event's **Sources** line, and of the source's row in the
  Dictionary's Sources tab ([Dictionary](ui-dictionary.md)). Events and
  citations show their dots in their rows ([Common UI §4.18](ui-common.md#418-tags-picker-chips-and-dots)).
  Where a record has no menu yet, the shared [ContextMenu](ui-common.md) is
  added to it, opened by a right click, the context-menu key or Shift+F10 on
  the focused row, and by a long press on touch screens.
- **Phase 4** adds the same for places, repositories and notes: the `tags`
  field of their reads, and their menu entries (the Dictionary's Places and
  Repositories tabs, and the **Notes** card of a record); notes show their dots
  in the card.

The rules of [Data Model](data-model.md) (RecordTag) apply to these kinds too: a
soft-deleted record keeps its links, which return with it, and a tag change
never versions the tagged record. A duplicated tree carries its tags and links
along with its records.

### 2.1 GEDCOM for the remaining kinds

Phase 1 writes `_OXIDGENE_TAG` beneath `INDI`, `FAM` and the first `OBJE` of
a document ([API Contract](api.md), OxidGene extensions). The later phases extend it to, following the same rules
(one value `{"v":1,"label":…,"color":…}` per tag, labels folding alike being
one tag on import, a tag no record carries not exported):

- each event or attribute structure, `SOUR`, `REPO`, `NOTE` and `SNOTE`
  (phases 3 and 4);
- the citation's `SOUR` pointer structure (phase 3);
- every `PLAC` of a tagged place (phase 4). A GEDCOM place is a string repeated
  in each event, not a record a tag could sit beneath once, so an import gives
  a place the union of the tags its `PLAC` structures carry.

A `.gw` has no room for tags and carries none, as today.

## 3. Bulk tagging (phase 2)

Bulk tagging is about persons only: the other kinds are tagged one at a time,
and none of their lists offers a selection.

### 3.1 Interface

- **Bulk mode of the picker.** The picker ([Common UI §4.18](ui-common.md#418-tags-picker-chips-and-dots))
  is opened in bulk mode on a set of persons. It shows, for each tag, how many
  of the target's persons carry it, and two actions per tag, **Add to all** and
  **Remove from all**, in place of the checkbox. It asks for confirmation when
  the target holds more than 100 persons. Each action is written at once, and
  a failed one shows its translated error under the field.
- **From the tree**: the pencil action picker of a person ([Genealogy Tree](ui-genealogy-tree.md))
  gains **Tag a branch…**. It asks for the scope (ancestors, descendants, or
  both), a depth (default: all generations, up to 30), whether to include the
  person, and for descendants whether to include their spouses. It then opens
  the picker in bulk mode on those persons.
- **From search results**: a **Tag the results…** action in the results
  toolbar ([Search Results](ui-search-results.md)) opens the picker in bulk mode
  on every person the current search and filters return, not only the page
  shown. It is disabled when there is no result.

### 3.2 API

| Method | Path | Description |
|---|---|---|
| `POST` | `/trees/{tree_id}/tags/{tag_id}/bulk` | `{ action: "add" \| "remove", target }`. Applies the tag to, or takes it off, every person of `target` in one transaction and one audit entry. Returns `{ count }`, the persons changed |

`target` is exactly one of:

- `{ persons: [id, …] }`, at most 1,000;
- `{ ancestors_of: id, depth, include_self }` or
  `{ descendants_of: id, depth, include_self, include_spouses }`, `depth` 1–30
  generations;
- `{ search: { q, …PersonSearchFilters } }`, every person the search would
  return, not one page of it.

A target of more than 10,000 persons is `400 bulk_too_large`. GraphQL:
`bulkTag(treeId, tagId, action, target)`. The bulk change is audited as one
`record_tag` entry with the `count` of records changed ([Data Model](data-model.md),
Audit log). The REST and GraphQL surfaces share the validation, errors and
tests ([Cross-cutting Rules](cross-cutting.md)).

## 4. Highlighting a tag in the tree views (phase 2)

The tree's left sidebar gains a **Highlight a tag** button. It opens the
vocabulary as a list (the persons' tags only, those carried by at least one
person); choosing a tag:

- marks every card of a person carrying it with a dot in the tag's colour at
  the card's top corner and a border in that colour, in the tree and in every
  other chart (wheels, fans, lineage, hourglass);
- dims the other cards to 45 % opacity, unless the reader turns **Dim the
  others** off in the same list;
- shows the tag as a chip under the sidebar button, whose × ends the
  highlight.

The highlight lasts while the tree page is open and is kept in its history
entry ([Common UI §2.4](ui-common.md)); it is not saved in the settings.
Printing keeps the marks, not the dimming. Tree cards draw nothing for tags
except while a tag is highlighted.

## 5. Uses beyond finding (phase 5)

- Statistics and anomalies limited to a tag ([Statistics](ui-statistics.md),
  [Tools §3](ui-tools.md#3-anomalies)).
- An export of the persons carrying a tag, with the subtree export of the
  [Roadmap](roadmap.md) (§7b).

## 6. Errors

Phase 2 adds one code to the shared set of [Cross-cutting Rules §4](cross-cutting.md):
a bulk target past its bound is `validation_error` (400) and above 10,000
persons `bulk_too_large` (400). Every message is translated in the interface.

## 7. Interface strings

Every interface string of the phases below — menu entries, bulk confirmations,
the highlight list, errors, accessible names — goes through the translation
tables in all eight languages. Tag labels are user content and are never
translated.

## 8. Tests still to write

- Phases 2: bulk tagging through REST and GraphQL, with the same cases: each
  target and its bounds, `bulk_too_large`, a search target covering more than
  one page, one audit entry per bulk change; a `persons` target of another tree
  refused.
- Phases 2: the highlight in the tree and in one chart; the picker's bulk mode
  keyboard behaviour.
- Phases 3 and 4: every REST operation and its GraphQL twin for the new kinds'
  reads; the GEDCOM round trip of each kind's tags and the vocabulary's colours;
  a Geneanet or GeneWeb destination drops them and reports it.
- Test data is fictitious ([Cross-cutting Rules](cross-cutting.md)).

## 9. Phases

| Phase | Content |
|---|---|
| 2 | Highlighting a tag in the tree views; bulk tagging from the tree and from search results (§3, §4) |
| 3 | Events, sources and citations: the reads, the menus and the GEDCOM `_OXIDGENE_TAG` of those kinds (§2, §2.1) |
| 4 | Places, repositories and notes: the same, including the `PLAC` rule of §2.1 |
| 5 | Uses beyond finding: statistics and anomalies limited to a tag, and an export of the persons carrying a tag, with the subtree export (§5) ([Roadmap](roadmap.md)) |

Until its phase, the reads of events, sources, citations, places, repositories
and notes do not yet carry `tags` ([Data Model](data-model.md)); the generic
endpoints already tag them.
