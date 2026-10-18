---
type: "Product Specification"
title: "Tags"
description: "Planned tree-wide tags on persons, couples, events, places, sources, citations, repositories, media and notes: one vocabulary per tree with a colour per tag, a shared tag picker reached from each record's menu instead of its form, chips and dots that take no room on untagged records, the Tags dictionary tab, search filters, highlighting in the tree views, bulk tagging, GEDCOM round trip, API, and delivery phases."
tags: [oxidgene, specification, tags, data-model, api, ui]
generated: { by: human:maintainer, at: 2026-10-18T14:00:33Z }
---

# Tags

> Part of the [OxidGene Specifications](index.md).
> See also: [Data Model](data-model.md) · [API Contract](api.md) ·
> [Common UI](ui-common.md) · [Dictionary](ui-dictionary.md) ·
> [Genealogy Tree](ui-genealogy-tree.md) · [Roadmap](roadmap.md)

**Status: planned, nothing is implemented.** Today only media carry tags
([Data Model](data-model.md), MediaTag). This document fixes the contract the
generalization will follow; [Roadmap](roadmap.md) tracks it. As each phase
lands, the parts it delivers move into the data-model, API and UI
specifications, and this document keeps only what is still planned.

---

## 1. Purpose

A tag is a short label a genealogist puts on records to find them again and
to work on them together. Three uses drive the design:

- **Research follow-up**: *to verify*, *record not found*, *blocked*,
  *transcribed*, *hypothesis* — the state of a piece of research, on the
  person, couple, event, source or citation it concerns.
- **Projects and branches**: *Soldiers 1914–1918*, *Emigration to America*,
  *Maternal branch* — a set of persons gathered across the tree, often many
  at once (§6.4).
- **Classification of documents**: what media tags do today.

Tags are the user's own vocabulary. OxidGene ships no predefined tag and never
translates one ([Cross-cutting Rules](cross-cutting.md)): a tag is user
content, like a note.

### 1.1 Principles

- **One vocabulary per tree.** A tag exists once in a tree and is shared by
  every kind of record. Renaming, recolouring, merging or deleting it acts
  everywhere at once.
- **Tagging is not editing.** Tags are never fields of an edit form. They are
  added and removed from the record's menu, through one shared picker (§6.1),
  and each change is written at once.
- **No room taken when unused.** A record without tags looks exactly as it
  does without this feature. The only permanent addition to the interface is
  one menu entry per record kind.
- **Flat.** Tags have no hierarchy: a colour and a name are enough, and the
  Tags tab groups them by use (§6.3).

### 1.2 What can carry a tag

Every record that exists on its own in the interface:

| Record | Example |
|---|---|
| Person | *to verify*, *Soldiers 1914–1918* |
| Couple (family) | *marriage not found* |
| Event | *approximate date to refine* |
| Place | *vanished parish*, *to locate* |
| Source | *fully abstracted* |
| Citation | *record not consulted*, *doubtful reading* |
| Repository | *closed*, *online* |
| Media (a document) | what media tags hold today |
| Note | *hypothesis*, *to write* |

Parts of a record do not carry tags: a person's names, a family's spouse and
child links, an event's witnesses and spouse ages, vignettes, media links and
kept GEDCOM. They have no page or list where a tag could be shown or found,
and the record that holds them covers the need — a doubtful given name is
tagged on the person, or explained in a note. A multi-page document carries
its tags on the document, never on its pages, as media tags do today.

The tree itself carries none: tags belong to a tree, and tagging trees would
need a second vocabulary above them all.

---

## 2. Data model

Until the first release the schema is the single initial migration
([Architecture §9](architecture.md)): the tables below replace `media_tag`
there, and existing databases are recreated and reimported.

### 2.1 `tag`

| Column | Type | Notes |
|---|---|---|
| `id` | UUID v7 | PK |
| `tree_id` | UUID v7 | FK → Tree |
| `label` | String | The trimmed spelling, 1–80 characters |
| `normalized_label` | String | The label folded as every word of the application is ([Cross-cutting Rules §3.6](cross-cutting.md)); unique with `tree_id` |
| `color` | i16 | Index of the tag's colour in the palette (§6.6), 0–11 |
| `created_at` / `updated_at` | DateTime | Auto |

Two labels that fold alike are one tag: adding *À vérifier* where *a
verifier* exists picks the existing tag, spelled as it was first entered.

A new tag takes the palette colour the fewest of the tree's tags use, the
lowest index on a tie, so the first twelve tags all differ.

### 2.2 `record_tag`

| Column | Type | Notes |
|---|---|---|
| `id` | UUID v7 | PK |
| `tree_id` | UUID v7 | FK → Tree |
| `tag_id` | UUID v7 | FK → `tag` (cascade) |
| `person_id`, `family_id`, `event_id`, `place_id`, `source_id`, `citation_id`, `repository_id`, `media_id`, `note_id` | UUID v7? | FK → the tagged record; **exactly one** is set, enforced by a `CHECK` |
| `created_at` | DateTime | Auto |

- One partial unique index per record column, `(tag_id, <column>)` where the
  column is set: adding a tag twice is idempotent, and concurrent additions
  of one tag cannot duplicate it.
- One index per record column leading with that column, so a record's tags
  are one index range.
- A foreign key per kind, rather than a `(kind, id)` pair, lets the database
  guarantee that a link never names a missing record, and lets the tree purge
  remove a record's links with it.
- The record whose tags these are is never written by a tag change: adding a
  tag does not touch the person's `updated_at` nor version them.

### 2.3 Deletion

- **A soft-deleted record keeps its links.** They are excluded wherever the
  record is excluded, and come back with it when it is restored. The tree
  purge removes them with the record.
- **Deleting a tag removes it and all its links for good.** A tag is
  classification, not genealogy: like a media tag today it is audited, never
  versioned (§2.5), so there is no deleted state to restore. The confirmation
  names how many records carry it (§6.3).
- **Merging two persons** gives the kept person the union of both persons'
  tags. **Duplicating a tree** copies its tags and links.

### 2.4 Projections

- `PersonProfile` gains `tags` — `[{ id, label, color }]`, by label — and so
  does each event of its timeline. `PROJECTION_SCHEMA_VERSION` is bumped with
  that change. A tag change refreshes the projection of every person whose
  profile shows the tagged record, in the same transaction; a rename,
  recolour or merge refreshes every person carrying the tag or one of their
  tagged events.
- `person_search_fts` is not changed: the tag filter (§5.3) joins
  `record_tag` on the persons the text search returns.

### 2.5 Change history

Tag writes are recorded in the audit log ([Data Model §5.1](data-model.md)),
category `data` (`media` for a medium's tags, as today):

| Write | `entity` | `subject` | `details` |
|---|---|---|---|
| Create, rename, recolour a tag | `tag` | — | `new_label` on a rename |
| Merge a tag into another | `tag` | — | `other_label`, `count` of links moved |
| Delete a tag | `tag` | — | `count` of links removed |
| Add or remove one tag on one record | `record_tag` | the tagged record, or the person or family owning an event, citation or note | — |
| A bulk change (§5.4) | `record_tag` | — | `count` of records changed |

Tags are not part of any version snapshot, and restoring a version leaves
tags as they are.

---

## 3. GEDCOM, GEDZIP and GeneWeb

No GEDCOM version defines tags. OxidGene writes them as an extension that
other programs ignore and an OxidGene import restores; it is declared in
`HEAD.SCHMA` like the existing ones ([API Contract](api.md), OxidGene
extensions).

- **`_OXIDGENE_TAG`**, one per tag, beneath each tagged structure: `INDI`,
  `FAM`, each event or attribute structure, `SOUR`, `REPO`, `OBJE`,
  `NOTE` / `SNOTE`, the citation's `SOUR` pointer structure, and every `PLAC`
  of a tagged place — a GEDCOM place is a string repeated in each event, not a
  record a tag could sit beneath once. Its value is a versioned JSON value,
  `{ "v": 1, "label": …, "color": … }`, so each line is whole on its own and
  the file needs no vocabulary in its header.
- On import, labels that fold alike are one tag, spelled and coloured as the
  first one read; a place takes the union of the tags its `PLAC` structures
  carry. A tag that no record carries is not exported.
- The `tags` of `_OXIDGENE_DOC`, which carries a document's tags today, are
  replaced by `_OXIDGENE_TAG` lines beneath the document's first `OBJE`.
- A plain `.ged` and a GEDZIP carry the same. An export for a
  [destination](api.md) treats `_OXIDGENE_TAG` as it treats every OxidGene
  extension: as the destination's `oxidged` profile does, and reported the
  same way. A `.gw` has no room for them and carries none; its import brings
  none.

---

## 4. Tags in reads

Every read of a taggable record — REST and GraphQL alike — carries its
`tags`, `[{ id, label, color }]` sorted by folded label: `Person`, `Family`,
`Event`, `Place`, `Source`, `Citation`, `Repository`, `Media` (replacing its
list of strings) and `Note`. A list endpoint reads the tags of its page in one
grouped query, whatever the page size. A search result row carries its
person's tags from the projection.

---

## 5. API

REST and GraphQL offer the same operations with the same validation, errors
and tests ([Cross-cutting Rules](cross-cutting.md)). REST paths are relative
to `/api/v1`.

### 5.1 The vocabulary

| Method | Path | Description |
|---|---|---|
| `GET` | `/trees/{tree_id}/tags` | Every tag of the tree with its counts per record kind, `{ id, label, color, counts: { persons, families, events, places, sources, citations, repositories, media, notes } }`, sorted by folded label. Answered whole: a vocabulary is a handful of short rows that the Tags tab and the picker filter as the reader types; it joins the documented exceptions of [API Contract](api.md), Pagination |
| `POST` | `/trees/{tree_id}/tags` | `{ label, color? }`. Creates the tag, or returns the existing one when the label folds to one (`200` instead of `201`). `color` defaults as §2.1 says |
| `PUT` | `/trees/{tree_id}/tags/{tag_id}` | `{ label?, color? }`. Rename or recolour. A label folding to another tag's is `409 tag_exists`: joining two tags is a merge, which says so |
| `POST` | `/trees/{tree_id}/tags/{tag_id}/merge` | `{ into: tag_id }`. Moves every link to `into`, dropping those it already has, then deletes the tag. Returns `into` with its counts |
| `DELETE` | `/trees/{tree_id}/tags/{tag_id}` | Deletes the tag and its links (§2.3). `204` |
| `GET` | `/trees/{tree_id}/tags/{tag_id}/records` | `kind` (required), `first`, `after`. The live records of that kind carrying the tag, cursor-paginated, each as `{ id, kind, label }` with, for an event, citation or note, the person or family it belongs to |

GraphQL: `tags(treeId)`, `taggedRecords(treeId, tagId, kind, first, after)`,
`createTag`, `updateTag`, `mergeTag`, `deleteTag`.

### 5.2 Tagging one record

| Method | Path | Description |
|---|---|---|
| `PUT` | `/trees/{tree_id}/tags/{tag_id}/records/{kind}/{record_id}` | Adds the tag. Idempotent: `204` whether or not it was already there |
| `DELETE` | `/trees/{tree_id}/tags/{tag_id}/records/{kind}/{record_id}` | Removes it. `204`, also when it was not there |

`kind` is `person`, `family`, `event`, `place`, `source`, `citation`,
`repository`, `media` or `note`. A record of another tree or a deleted record
is `404`. A page of a multi-page document is tagged on its document, as media
tags are today. GraphQL: `tagRecord` and
`untagRecord`. The media endpoints `POST …/media/{id}/tags` and
`DELETE …/media/{id}/tags/{tag}` and GraphQL `addMediaTag` and
`removeMediaTag` are removed.

### 5.3 Filtering by tag

- **Person search** (`/persons/search`, `searchPersons`) gains `tag`, a tag
  id, repeatable: the persons carrying every tag given. The MCP
  `search_persons` tool takes it with the rest of `PersonSearchFilters`
  ([Assistant Access](mcp.md)).
- **The media library** (`GET …/media`, `mediaList`) keeps its repeatable
  filter, now by tag id; `…/media/facets` (`mediaFacets`) lists the tree's
  tags carried by at least one document, as `{ id, label, color, count }`.

### 5.4 Bulk tagging

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
`bulkTag(treeId, tagId, action, target)`.

Bulk tagging is about persons only: the other kinds are tagged one at a time,
and none of their lists offers a selection.

### 5.5 Errors

The shared error codes of [Cross-cutting Rules §4](cross-cutting.md), with no
tag-specific code: a rename onto another tag's label is `conflict` (409) — the
interface, which holds the whole vocabulary, offers the merge before sending
it; a label empty or longer than 80 characters after trimming, a colour
outside 0–11 and a bulk target past its bound are `validation_error` (400);
an unknown tag, kind or record is `not_found` (404). Every one is translated
in the interface.

---

## 6. Interface

### 6.1 The tag picker

One component, `TagPicker` in [Common UI](ui-common.md), used for every
record kind and for bulk tagging. It is an anchored overlay on the shared
`ContextMenuSurface`, opened from the record's menu or its tag button:

```
┌ Tags ───────────────────────┐
│ [ sold|                   ] │  search in the tree's vocabulary
│ ☑ ● Soldiers 1914–1918      │  checked: the record carries it
│ ☐ ● Solent branch           │
│ + Create "sold"             │  only when no tag folds to the text
└─────────────────────────────┘
```

- The field filters the vocabulary as the reader types, folded as every word
  is. The record's own tags come first, then the others, each group by label;
  at most 14rem tall, then it scrolls.
- **Checking a tag adds it, unchecking removes it**, written at once (§5.2);
  the record's chips or dots follow. A failed write unchecks the line again
  and shows the translated error under the field.
- **Creating** takes the typed text as the label and the default colour
  (§2.1), then checks it. There is no creation dialog: the colour is changed
  later in the Tags tab.
- Keyboard: arrows move, Space toggles, Enter toggles or creates, Escape
  closes and gives the focus back to what opened the picker. The list is a
  `listbox` of `option`s with `aria-checked`.
- **Bulk mode** (§6.4) shows, for each tag, how many of the target's persons
  carry it, and two actions per tag, **Add to all** and **Remove from all**,
  in place of the checkbox. It asks for confirmation when the target holds
  more than 100 persons.

### 6.2 Where each kind is tagged

Each kind gets one entry, **Tags…**, in the menu it already has. Where a
record has no menu yet, the shared [ContextMenu](ui-common.md) is added to
it, opened by a right click, the context-menu key or Shift+F10 on the focused
row, and by a long press on touch screens.

| Record | Where **Tags…** is |
|---|---|
| Person | A **Tags** icon button among the identity header's icon actions ([Person Profile §4](ui-person-profile.md)); the tree's pencil action picker and person-card context menu ([Genealogy Tree](ui-genealogy-tree.md)) |
| Couple | A **Tags** icon button in the couple's actions row ([Couple Profile §4](ui-couple-profile.md)) |
| Event | The context menu of its timeline row ([Person Profile §6](ui-person-profile.md)) and of its row in the couple's shared sections |
| Citation | The context menu of its entry on its event's **Sources** line |
| Note | The context menu of the note in the **Notes** card |
| Media | The viewer's relation menu and the tile's context menu; the viewer's **Add a tag** button and form are removed |
| Source, Repository, Place | The context menu of the row in their Dictionary tab |

No edit form changes: the person, couple, event, source, repository and
document forms keep their fields.

### 6.3 Showing tags

| Where | What is drawn |
|---|---|
| Headers: person identity header, couple actions row, media viewer facts column | A row of **chips** — colour dot and label — only when the record has tags; nothing otherwise |
| Rows: timeline events, citations, notes, Dictionary rows, search results, media tiles | **Dots** in the tags' colours after the row's text, at most three and then *+n*; the labels in a tooltip and in the accessible name (*Tags: to verify, hypothesis*) |
| Tree cards | Nothing, except while a tag is highlighted (§6.5) |

A chip or a dot opens the Tags tab on its tag. Chips carry no remove button:
removal goes through the picker, so a stray click cannot remove a tag.

### 6.4 Bulk tagging

- **From the tree**: the pencil action picker gains **Tag a branch…**. It asks
  for the scope — ancestors, descendants, or both, a depth (default: all
  generations, up to 30), whether to include the person, and for descendants
  whether to include their spouses — then opens the picker in bulk mode on
  those persons.
- **From search results**: a **Tag the results…** action in the results
  toolbar opens the picker in bulk mode on every person the current search
  and filters return (§5.4), not only the page shown. It is disabled when
  there is no result.

### 6.5 Highlighting a tag in the tree views

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
Printing keeps the marks, not the dimming.

### 6.6 Colours

Twelve colour slots, `--tag-0` to `--tag-11`. Each theme declares them in its
JSON with the other tokens, chosen to read on its card and page backgrounds;
a theme that does not declare them takes the default set. A dot or chip never
relies on its colour alone: its label is always in the chip, the tooltip or
the accessible name.

### 6.7 The Tags tab of the Dictionary

A seventh tab, **Tags**, after **Media** ([Dictionary §4](ui-dictionary.md)):

- one row per tag: colour dot, label, and the counts per kind it has
  (*12 persons · 3 sources · 40 media*), sorted by label and narrowed by the
  quick filter; no alphabet index and no pagination (§5.1);
- the chevron unfolds the records carrying it, grouped by kind, each linking
  to its page (a person's profile, a couple's, the media viewer, the source in
  the Sources tab…), paged a kind at a time (§5.1);
- **Edit** opens a small dialog: the label and the twelve colours. Saving a
  label that folds to another tag's offers to **merge** the two, naming the
  number of records that will carry the merged tag;
- **Delete** asks for confirmation naming the number of records carrying the
  tag (§2.3);
- **New tag** creates one without putting it on any record.

The Media tab's tag cloud stays: it lists the tags carried by documents, with
their media counts and a dot of their colour.

### 6.8 Search results

The filter panel of [Search Results §5](ui-search-results.md) gains **Tags**:
a field suggesting the tree's person tags, each choice added as a chip; the
results carry every chosen tag. Each result row shows its person's dots.

### 6.9 Internationalization

Every interface string — menu entries, picker, dialogs, errors, accessible
names, the bulk confirmations — goes through the translation tables in all
eight languages. Tag labels are user content and are never translated.

---

## 7. Assistant access (MCP)

- `search_persons` filters by tag (§5.3) and `get_person_profile` returns the
  tags of the person and of their events, through the types they already
  share with REST.
- A `list_tags` tool returns the vocabulary with its counts (§5.1).

---

## 8. Tests

- The `record_tag` constraint: a link naming no record or two records is
  refused by the database.
- Every REST operation and its GraphQL twin, with the same cases: folding of
  labels, idempotent add and remove, `tag_exists`, merge dropping duplicate
  links, deletion removing links, cross-tree and deleted records, a page of a
  document refused, each bulk target and its bounds.
- Projection: a tag change on a person and on one of their events is in their
  profile; a rename reaches every profile showing it; the projection shape
  test carries the bump.
- A merge of persons keeps both persons' tags; a duplicated tree carries them.
- GEDCOM, GEDZIP: a round trip keeps every kind's tags and the vocabulary's
  colours; a Geneanet or GeneWeb destination drops them and reports it.
- Interface: the picker's keyboard behaviour; nothing drawn on an untagged
  record; the highlight in the tree and in one chart.
- Test data is fictitious ([Cross-cutting Rules](cross-cutting.md)).

---

## 9. Phases

| Phase | Content |
|---|---|
| 1 | `tag` and `record_tag` replacing `media_tag`; persons, couples and media; the picker, chips and dots; the Tags tab; the search filter; GEDCOM and GEDZIP; history; `list_tags` |
| 2 | Highlighting a tag in the tree views; bulk tagging from the tree and from search results |
| 3 | Events, sources and citations, with the context menus they need |
| 4 | Places, repositories and notes |
| 5 | Uses beyond finding: statistics and anomalies limited to a tag, and an export of the persons carrying a tag, with the subtree export ([Roadmap](roadmap.md)) |

The model accepts the nine kinds from phase 1; the interface offers them
phase by phase.
