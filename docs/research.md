---
type: "Product Specification"
title: "Research"
description: "Planned research items of a tree — a question, its status and the log of the steps taken to answer it, whatever the lead: asking a relative, examining a photograph, a repository, a visit, a website or a letter — kept off the pages until asked for, and the records still to find shown by the ancestry completeness without a request to any archive: principles, model, the research button and its panel, the Tools tab, privacy, history, API outline, GEDCOM and delivery phases."
tags: [oxidgene, specification, research, data-model, api, ui, tools, privacy]
generated: { by: human:maintainer, at: 2026-11-21T10:24:00Z }
---

# Research

> Part of the [OxidGene Specifications](index.md).
> See also: [Tools](ui-tools.md) · [Common UI](ui-common.md) ·
> [Data Model](data-model.md) · [API Contract](api.md) ·
> [Privacy](privacy.md) · [Archive Portals](archives.md) ·
> [Roadmap](roadmap.md)

**Status: planned, nothing is implemented.** This document fixes the
contract the implementation will follow; [Roadmap](roadmap.md) tracks the
phases. When a phase is delivered, what it built moves to the specification
of the surface it belongs to — the entities to [Data Model](data-model.md),
the endpoints to [API Contract](api.md), the tab to [Tools](ui-tools.md), the
shared components to [Common UI](ui-common.md) — and this document keeps the
principles, as [Tags](tags.md) does.

---

## 1. Principles

- **A question, not a note.** A research item states what is looked for, has
  a status, and keeps the log of what was tried. A step that found nothing is
  as much a result as one that found something: it is what keeps the same
  search from being done twice.
- **Any lead.** The steps of a research are not consultations of an archive.
  Asking a relative, examining a photograph, going to a fair, visiting a
  cemetery, writing a letter and reading a website are steps like reading a
  register. The archive catalogue only saves typing when a step happens to be
  one of its registers (§4.3); nothing in the model or the interface assumes
  it.
- **Off the page until asked for.** Someone who opens a person to look at
  them sees no research. A record's research shows in a panel its button
  opens, and nowhere else on the page (§4).
- **Attached to what it is about, or to nothing.** A research may concern
  several records at once — two persons, a couple and a photograph — or none:
  a task of the tree as a whole.
- **Working notes are private.** Research is never shown to a Guest or a
  Visitor, never reaches a share link, and leaves the tree only in an export
  its owner asks it for (§7).
- **User content.** Titles, texts and the names typed in a step are the
  user's and are never translated. Every interface string — statuses, kinds
  of step, results, menu entries, empty states, accessible names — goes
  through the translation tables in all eight languages.

## 2. Model

Three tables, all carrying `tree_id`, removed with the tree and copied by a
tree duplication.

### 2.1 Research

| Column | Type | Notes |
|---|---|---|
| `id` | UUID v7 | PK |
| `tree_id` | UUID v7 | FK → Tree (cascade) |
| `title` | String | The question, plain text, 1–200 characters |
| `text` | String? | Details, plain text |
| `status` | Enum | `todo` (default), `in_progress`, `found`, `not_found` |
| `due` | Date? | An optional day the research is meant for: a visit, an appointment, a fair |
| `created_at` / `updated_at` | DateTime | Auto |

### 2.2 ResearchLink

One record a research is about, on the pattern of
[RecordTag](data-model.md#recordtag): exactly one record column is set,
enforced by a `CHECK`; one partial unique index per column on
`(research_id, <column>)`; one index per column leading with it, so a
record's research is one index range.

| Column | Type | Notes |
|---|---|---|
| `id` | UUID v7 | PK |
| `tree_id` | UUID v7 | FK → Tree (cascade) |
| `research_id` | UUID v7 | FK → Research (cascade), paired with `tree_id` |
| `person_id`, `family_id`, `event_id`, `media_id`, `source_id`, `place_id` | UUID v7? | FK → the record (cascade), paired with `tree_id`; exactly one is set |

A research with no link is a task of the tree. Deleting a linked record
removes the link and keeps the research. A person merge moves the links of
the record merged away to the record kept, dropping one that would duplicate
a link already there.

### 2.3 ResearchStep

One thing done, or to do, for a research: the rows of its log, ordered by
`date` then creation.

| Column | Type | Notes |
|---|---|---|
| `id` | UUID v7 | PK |
| `tree_id` | UUID v7 | FK → Tree (cascade) |
| `research_id` | UUID v7 | FK → Research (cascade), paired with `tree_id` |
| `kind` | Enum | See below |
| `date` | Date? | The day it was done, or is planned for |
| `text` | String? | What was looked at or asked, and what came of it; plain text |
| `where_text` | String? | Who or where, as typed, when no record below names it |
| `person_id` | UUID v7? | The person of the tree asked or met (`ask_person`); set null when that person is deleted |
| `media_id` | UUID v7? | The photograph or document examined (`examine_document`); set null likewise |
| `repository_id` | UUID v7? | The [Repository](data-model.md#repository) consulted (`repository`); set null likewise |
| `place_id` | UUID v7? | The place gone to (`visit`); set null likewise |
| `result` | Enum? | `nothing`, `partial`, `found`; null while the step is planned or awaits an answer |
| `created_at` / `updated_at` | DateTime | Auto |

At most one of the four record columns is set, enforced by a `CHECK`.

| `kind` | What it is | Record it may name |
|---|---|---|
| `ask_person` | Asking or meeting someone | A person of the tree, or a name in `where_text` |
| `examine_document` | Looking closely at a photograph, a paper, an object | A media of the tree |
| `repository` | An archive, a library, a town hall, a parish | A repository of the tree |
| `visit` | A fair, an association, a trip, a cemetery | A place of the tree |
| `online` | A website or a database | — |
| `correspondence` | A letter or a message sent or received | — |
| `other` | Anything else | — |

A kind decides the icon of the step and which picker its form offers; every
kind accepts `where_text` instead of a record.

## 3. What a research does not change

- Writing a research, a link or a step does not touch the `updated_at` of the
  records it is about, does not version them, and moves neither generation
  of `projection_state`: no projection and no redaction reads research (§7).
- A research takes no part in searches, statistics, anomalies or charts.
- Tags are not carried by research items; the Tools tab filters by the tags
  of the persons a research is linked to (§5).

## 4. The research button and its panel

### 4.1 Button

`ResearchButton` is an icon button titled and named *Research…*, placed
beside `TagsButton` wherever that one stands
([Common UI §4.18](ui-common.md#418-tags-picker-and-badges)): among the
header actions of the person and couple pages and of the media viewer's
facts, and as a *Research…* entry beside *Tags…* in the menus of event rows,
of the Dictionary's sources and places, and of the pedigree's action picker.

- It shows the number of the record's open research (`todo` and
  `in_progress`) as a small count, and nothing when there is none.
- It is not drawn for a viewer who may not read research (§7).
- It is the only trace of research on a page: no badge, row or section is
  added to a profile, a list or a chart.

### 4.2 Panel

The button opens the research panel on its record.

```
+--------------------------------------------+
| Research · <person A>        [pin]  [x]    |
+--------------------------------------------+
| [+ Research]                               |
|                                            |
| v <question 1>              [In progress]  |
|   also about: <person B>, <photograph>     |
|   <date>  (ask)   <person C> ..... nothing |
|   <date>  (repo)  <repository> ... partial |
|   <date>  (visit) <place> ........ planned |
|   [+ Step]            [Found...] [Delete]  |
|                                            |
| > <question 2>                   [To do]   |
| > 1 closed                                 |
+--------------------------------------------+
```

- **Where.** On the person and couple pages it is a side panel on the right
  of the content, with the behaviour of the pedigree's events sidebar
  ([Genealogy Tree §6](ui-genealogy-tree.md#6-events-sidebar-right)):
  resizable from its left edge within the same bounds, its width remembered
  on the device. Where a page has no room for it — the pedigree, the
  Dictionary, the media viewer, and any page at 900 px and below — it opens
  as a dialog through the shared `Modal`.
- **Content.** The research linked to the record, open ones first, then by
  `due` and last change; closed ones (`found`, `not_found`) folded under a
  count. A research unfolds to its other links, its details and its steps.
- **Writing.** *+ Research* creates one already linked to the record. The
  status is a select written at once. *+ Step* opens the step form: kind,
  date, the picker of that kind or free text, text, result. Links are added
  and removed through the shared person, media, source and place pickers.
  Each change is written at once, as tags are; a failed write restores the
  control and shows the translated reason.
- **Found.** *Found…* sets the status to `found`. When the research is linked
  to an event, it then offers *Add the source*, which opens that event's
  existing citation entry; nothing is written to the event by the research
  itself.
- **Pin.** A pin in the panel's header keeps the side panel open while moving
  from one person or couple page to another, each showing its own record's
  research, or the empty state with *+ Research*. The pin is a preference of
  the device, kept in `localStorage`, as the display preferences of
  [App Settings](ui-app-settings.md) are; it is not a setting of the tree,
  which its other members would share. Unpinned, the panel closes on
  navigation.
- **Keyboard.** Escape closes the dialog form and returns the focus to the
  button. In side-panel form the panel is a landmark reached by Tab; the
  disclosure of each research is a button with `aria-expanded`.
- **Print.** The panel does not print with its page.

### 4.3 A step noted from an archive

When the reader has opened a register from OxidGene — a cited source, or the
*Find in the archives* dialog
([Archive Portals §6.5](archives.md#65-find-in-the-archives)) — on a record
that has open research, the dialog, or the page the reader returns to, offers
*Note this step*: the step form with the kind `repository`, the day, the
repository and the register's description filled in, the result left to the
reader. It is an offer, never a write, it sends no request, and it is the
only place where research and the archive catalogue meet.

## 5. The Research tab of Tools

A tab *Research* of the [Tools](ui-tools.md#2-layout) page lists the research
of the whole tree. It follows the page's rules: mounted only while shown, its
choice of grouping and filters kept per viewer, printed through
[Common UI §7](ui-common.md#7-printing).

- **Group by** status (the default), person, or *where or who*: the
  repository, person, place or typed text of each research's planned steps,
  so that everything to ask one relative, to do at one fair or to look for in
  one repository comes together. A research with several persons or several
  planned steps appears in each of their groups; one with none comes under
  *Not attached* or *No step planned*.
- **Filters**: status (open ones by default), kind of step, a tag — keeping
  the research linked to a person carrying it, with the tag select of
  [Tags §2.1](tags.md#21-statistics-limited-to-a-tag) — and *due* (overdue,
  within thirty days).
- **Rows** give the title, the status, the linked records, the next planned
  step and the day it is due. A row opens the research in the panel's dialog
  form; a linked record is a link to its page.
- **+ Research** creates a task of the tree, with no link.
- The printout is the list as grouped and filtered, steps included: the sheet
  taken to a reading room, a relative's home or a fair.

## 6. Records to find

The ancestry completeness of [Tools §5](ui-tools.md#5-ancestry-completeness)
already lists the missing ancestors and the key facts not recorded. It gains
what turns that list into the records still to find:

- **A third state per key fact.** A fact is *sourced* (green) when its event
  carries at least one citation, *recorded without a source* (orange) when it
  has a date or a place and none, *missing* (red) as today. "Show only what
  is missing" keeps the orange ones too.
- **Find.** Beside an orange or red fact, *Find* opens the *Find in the
  archives* dialog ([Archive Portals §6.5](archives.md#65-find-in-the-archives))
  with what the tree knows: the fact's own place and year when the event
  records them, otherwise what the person's other events give, left for the
  reader to complete. The action is drawn only when the catalogue
  ([Archive Portals §3.1](archives.md#31-catalogue)) names an archive for
  that place.
- **Create a research.** Beside the same facts, *Research…* creates a research
  linked to the person, and to the event when it exists, titled from the fact.
- **Scope.** The ancestors of the SOSA root, as today, or the persons
  carrying a tag, chosen with the tag select of the page's other tools; the
  persons of a tag are listed by name, without generations.

**No request is sent to compute or to show this list.** Naming the archive of
a place is a lookup in the catalogue shipped with the application. A portal
is contacted only when the reader presses *Find* and then acts in the dialog,
once, exactly as when opening a cited register: nothing is prefetched, facts
are never resolved several at a time, and there is no "find everything"
action. The load OxidGene puts on an archive stays what one reader clicking
by hand would put.

## 7. Privacy and permissions

Research follows the rules of confidential notes
([Privacy §3.2](privacy.md#32-records)), with the permissions of
[Authorization §3.2](authorization.md#32-permission-matrix):

| Operation | Permission |
|---|---|
| Read research, links and steps; see the button, its count and the Tools tab | `read_confidential` |
| Create and change a research, a link or a step | `edit` (with `read_confidential`) |
| Delete a research or a step | `delete` (with `read_confidential`) |
| The *Find* action and the three states of §6 | `tools` |

- Owners and Editors therefore read and write research; Guests, Visitors and
  share links' holders never see that any exists: the button is not drawn,
  the counts are not sent, and a direct request answers as for a record that
  does not exist.
- A scoped Editor reads and writes the research whose every link lies in
  their scope; a research of the tree, with no link, is the Owner's and the
  unscoped Editors'.
- No redaction reads research, so no viewer class depends on it.
- The desktop has one local user, who reads and writes everything.

## 8. History

Every write — a research created, changed or deleted, a link added or
removed, a step added, changed or deleted — records an entry in the audit
log ([Data Model §5.1](data-model.md#51-audit-log-audit_entry)), on REST,
GraphQL and the assistant alike. Research items are not versioned: the audit
entry of a deletion keeps the title, and the content is gone with the row.

## 9. API outline

REST and GraphQL offer the same operations, with the same validation, errors
and tests; the detailed contract is written in [API Contract](api.md) with
the first phase.

| Operation | REST | GraphQL |
|---|---|---|
| List a tree's research, filtered and paged by cursor | `GET /trees/{tree_id}/research` | `research` |
| List the research of one record | the same, with a record filter | the same |
| Read one, with its links and steps | `GET /trees/{tree_id}/research/{id}` | `researchItem` |
| Create, change, delete | `POST`, `PATCH`, `DELETE` on those | `createResearch`, `updateResearch`, `deleteResearch` |
| Add and remove a link | `POST`, `DELETE` `…/research/{id}/links` | `addResearchLink`, `removeResearchLink` |
| Add, change, delete a step | `POST`, `PATCH`, `DELETE` `…/research/{id}/steps` | `addResearchStep`, `updateResearchStep`, `deleteResearchStep` |

The count the button draws comes with the record's own read (the person and
couple page bundles, the media viewer bundle), as `open_research`, present
only for a viewer with `read_confidential`; no page makes a request of its
own for it. The ancestry completeness answer carries the state of each key
fact and whether the catalogue names an archive for it.

The assistant ([Assistant Access](mcp.md)) gains read tools over research
with the fourth phase, and write tools behind its write opt-in.

## 10. GEDCOM and exports

- An export leaves research out unless its *Research* option is ticked. The
  option is not offered to an exporter without `read_confidential`, nor
  together with *Apply visitors' privacy rules*.
- When included, each research is written as an OxidGene extension record
  (`_OXIDGENE_RESEARCH`) holding its fields, its steps and pointers to the
  records it is linked to; a program that does not know the extension
  ignores it, and an OxidGene import reads it back. A subtree or tag export
  keeps a research when at least one of its links is exported, and drops its
  pointers to the rest.
- A tree duplication copies research, links and steps.

## 11. Delivery phases

1. The model, the REST and GraphQL operations, `ResearchButton` and the panel
   on the person and couple pages, and the Research tab of Tools.
2. The button in the menus of events, sources, places and the media viewer;
   the step noted from an archive (§4.3); *Found…* leading to the event's
   citation.
3. Records to find (§6): the three states, *Find*, *Research…* and the tag
   scope in the ancestry completeness.
4. The GEDCOM extension and the export option; the assistant's tools.

## 12. Decisions taken

- Research is not drawn on the pages, and there is no tree setting to draw it
  there: a button opens a panel, which a pin kept on the device holds open.
- Open research is not marked on the pedigree's cards. If the need appears,
  the highlight mode of tags is the mechanism to reuse.
- The log of steps is part of the first phase: without it a research is a
  note with a status.
