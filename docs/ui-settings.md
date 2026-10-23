---
type: "UI Specification"
title: "Visual & Functional Specifications — Tree Settings Page"
description: "Tree settings page for roots, privacy, the change history, export, and the global preferences it shares with App Settings."
tags: [oxidgene, specification, ui, ux]
generated: { by: human:maintainer, at: 2026-10-24T09:00:00Z }
---


# Visual & Functional Specifications — Tree Settings Page

> Part of the [OxidGene Specifications](index.md).
> See also: [Homepage](ui-home.md) (settings button on cards) · [Tree View](ui-genealogy-tree.md) · [Person Edit Modal](ui-person-edit-modal.md) (privacy per person) · [Dictionary](ui-dictionary.md) (family names / sources / places / occupations index, moved out of this page) · [Data Model](data-model.md) · [API Contract](api.md) (GEDCOM export)

---

## 1. Overview

The settings page (`/trees/{id}/settings`) is a dedicated full-page interface for configuring a single genealogy tree. It covers tree identity, privacy rules, the change history, and export, and repeats the device's global preferences (§11). The tools that check a tree or work on it are on the [Tools](ui-tools.md) page. It is accessed via the **gear icon** in the [Tree View](ui-genealogy-tree.md) left sidebar or via tree card menus on the [Homepage](ui-home.md).

---

## 2. Layout

Uses the standard `sub-page` layout pattern (see [General](general.md) section 8).

```
+----------------------------------------------------------------------+
| NAVBAR                                                                |
+----------------------------------------------------------------------+
| [logo] tree_name / Settings                                          |  <- td-topbar
+----------------------------------------------------------------------+
|                                                                       |
|   +------------------+---------------------------------------------+ |
|   |                  |                                              | |
|   | LEFT NAVIGATION  |   CONTENT AREA                              | |
|   | (200px)          |                                              | |
|   |                  |   Section title                              | |
|   | Settings         |   Form fields / toggles / tools              | |
|   | - Tree & Roots   |                                              | |
|   | - Privacy        |                                              | |
|   |                  |                                              | |
|   | Tools            |                                              | |
|   | - History        |                                              | |
|   |                  |                                              | |
|   | Export           |                                              | |
|   | - Export tree    |                                              | |
|   |                  |                                              | |
|   | Global prefs     |                                              | |
|   | - Appearance …   |                                              | |
|   +------------------+---------------------------------------------+ |
|                                                                       |
+----------------------------------------------------------------------+
```

Content is constrained by `sub-page-content` (`max-width: 1200px`, centered, scrollable). The left navigation + content area use a flex row layout (`.settings-layout`).

Below `768px`, the navigation becomes a compact, horizontally scrollable row
above the content. Group labels and section buttons remain on one line so the
settings content begins in the first viewport rather than below a tall menu.

---

## 3. Topbar

Uses the shared `td-topbar` + `td-bc` breadcrumb component:

```
[logo] tree_name / Settings
```

- Logo icon links to the homepage
- Tree name (`.td-bc-link`) links to the tree view
- `/` separator (`.td-bc-sep`)
- "Settings" (`.td-bc-current`) — not clickable

---

## 4. Left Navigation

Fixed width: 200px. Divided into four labeled groups.

Each group has an uppercase orange label. Each item is a text button. The active item has an orange text color, bold weight, and a subtle background (`var(--bg-card)`).

### Group 1 — Settings
| Item | Section ID |
|---|---|
| Tree & Roots | `tree-roots` |
| Privacy | `privacy` |

### Group 2 — Tools
| Item | Section ID |
|---|---|
| History | `history` |

Anomalies, ancestry completeness, potential duplicates and date conversion
are tabs of the [Tools](ui-tools.md) page, reached from the sidebar's wrench
icon, not sections of this page.

### Group 3 — Export
| Item | Section ID |
|---|---|
| Export tree | `export` |

### Group 4 — Global Preferences
| Item | Section ID |
|---|---|
| Appearance | `appearance` |
| Language | `language` |
| Pedigree | `pedigree` |
| Names | `names` |
| Date Display | `date-display` |
| Entry Options | `entry-options` |

---

## 5. Content Area

The content area renders one section at a time based on the active nav item. Scrollable independently from the left nav.

Each section begins with:
- Small eyebrow label (group name, uppercase, orange)
- Section title (Cinzel font)
- Short descriptive subtitle

---

## 6. Save Behavior

Settings changes are **auto-saved** on interaction. For example, selecting a SOSA root person saves immediately upon selection from the person picker. A transient success message appears briefly to confirm the save.

The tree cache is invalidated after each save so that navigating back to the tree view reflects the changes immediately.

---

## 7. Section: Tree & Roots

### Tree name

The section starts with an editable **Tree name** field, pre-filled with the
current name. Saving validates that the name is not empty, updates the tree
immediately, and refreshes the breadcrumb and tree metadata without leaving
the settings page. The save action is a compact disk icon beside the input,
with an accessible label and tooltip, so the name retains the available width.
This is the same rename operation available from a tree card's overflow menu
on the homepage.

### SOSA 1 (Root person)

The shared [person picker](ui-common.md#42-personpicker), enclosed in a card, displays the currently
selected person with the same summary as a person-search result: profile photo
or sex-specific placeholder portrait, surname and given names, birth and death
years, and birth place when known. Two buttons appear on the right. On narrow
screens, the complete identity uses the card width and can wrap, while the
actions move to a separate row below it:

The settings page requests portraits only for the currently selected root and
self persons. Search results shown by either picker load portraits only for
their bounded result set; opening this section never loads the tree-wide
portrait inventory.

- **Change** — opens a person search modal to select a different root person
- **Clear** — removes the SOSA root assignment

When no root person is selected, a muted message is shown: "No root person selected".

Help text explains the Sosa-Stradonitz numbering system: "The root person is the starting point of the Sosa-Stradonitz numbering system. All ancestors are numbered relative to this person."

When a SOSA root is set, all direct ancestors visible in the tree view display a **SOSA badge** on their avatar circle (see [Tree View](ui-genealogy-tree.md) section 3).

### Who am I?

Second person picker to designate the current user's own person in the tree. Used to display relationship labels in profile views. Displayed as a separate card below the SOSA card, with the same picker UI and responsive identity/action layout.

The picker saves immediately, can be changed or cleared, and has no effect on
the genealogy data or SOSA numbering. It is stored on the tree
(`tree.self_person_id`), so everyone using the tree sees the same person: the
selected person receives the blue badge in the pedigree view, is offered as a
shortcut of the [kinship](ui-kinship.md) search, and names the GEDCOM
submitter when none is set (§10). Changing or clearing it invalidates the tree
metadata so the badge updates immediately, and is a *Settings* entry of the
history.

---

### Default privacy

A card controls the tree-wide default described in [§8](#8-section-privacy).

---

## 8. Section: Privacy

Privacy is stored but not enforced until authentication and authorization are
available. The UI must state this limitation and must not imply that records
are currently hidden from a viewer.

### Default privacy

Two buttons, Private and Public, saying what "follows the tree" means for every
person, couple and document in it. Each of those can still say otherwise — the
tree answers only for the records that have not chosen.

Private leads, and is the default for a new tree: a genealogy holds living
people, and a tree nobody has classified has not been cleared for publication.
Publishing is the deliberate act.

Saving is immediate — one click, one write — with a local override so the control
answers the click rather than the refetch. It carries the same line every other
privacy control does: *recorded now, nothing is hidden yet*.

Stored as `tree.default_privacy`; see [Data Model](data-model.md) (Privacy) for
why it is not a `Privacy` value itself.

### Tree visibility (planned)

| Toggle | Effect |
|---|---|
| Private tree | The tree is hidden from all other members |
| Show SOSA 1 ancestors to visitors | Non-authenticated visitors can only see the direct lineage of the root person |

### Contemporary persons (planned)

Like the tree's visibility, this waits for authentication and authorization
([Roadmap §8](roadmap.md#8-planned-security-release-and-deployment)): with
nobody to hide a record from, a rule hiding contemporaries could only lie about
what a viewer sees. Nothing below is built or stored yet, and the section shows
none of it.

**Age threshold slider** — range 50-120 years, default 80. Persons born less than N years ago without a known death date are treated as contemporary.

**Display mode (radio group, 3 options):**

| Option | Description |
|---|---|
| Fully hidden | Name, dates and photo all hidden. Card shows "Private person" only. |
| Semi-hidden | First name and last initial shown, dates and photo hidden. |
| Visible | No restrictions. Recommended only for private trees. |

**Additional toggles:**

| Toggle | Description |
|---|---|
| Allow navigation to hidden persons | Visitors can follow connections without seeing personal details |
| Show photos of contemporary persons | Photos can be shown or hidden independently of the display mode |

---

## 9. Section: History

The tree's audit log: every write to the tree, newest first — data, settings,
media, imports and exports. What is recorded, and what each entry carries, is
defined in [Data Model §5.1](data-model.md#51-audit-log-audit_entry).

**Filters.** A row of toggle buttons above the list — *All*, *Data*,
*Settings*, *Media*, *Imports*, *Exports*, *Restores* — keeps one category.
*All* is selected on arrival; the selected button carries the orange border and
`aria-pressed`.

**Entries.** One card per write, loaded 100 at a time with **Load more**:

- when it happened, in the reader's language and local time;
- a category badge;
- what it did: *Changed — event (Birth)*, *Added — name*, *Import*, *Export*;
- its subject's label as it read at the time. A person's links to their
  [history](ui-person-history.md) and a union's to its
  [couple view](ui-couple-profile.md); other subjects are plain text;
- its details when it has any: format, file name and person count of an import
  or export, *version 3 restored*, *merged with <name>*.

There is no author until authentication exists.

**Changes.** An entry that stored versions offers **Show the change** (or
*Show the N changes*). Opening it lists each record the write changed, titled
by record kind and name — *Person — <name>*, *Place — <name>* — each comparing
the state the write replaced with the version that follows it — the record's
current state when nothing changed it since — through
[VersionDiff](ui-common.md#410-versiondiff), changes only. A large write, such
as a family-name rename, loads its changes 100 at a time. Creations, imports,
exports and media writes replace no state and offer nothing to show.

Each change whose previous version is not a deleted state offers **Restore the
previous version**, confirmed through a `ConfirmDialog`: the record is put back
as that version had it, which undoes this write for that record. A person's
change also links to their full history. After a restore the log reloads, the
restore at its top.

---

## 10. Section: Export

A card with the format's name and description, a format select and the
download button. Two formats:

| Format | Extension | Writes | Description |
|---|---|---|---|
| GEDCOM | `.ged` | GEDCOM text written for the chosen destination program, its media referenced by their producers' paths | Genealogy exchange format, written for the program that will import it. |
| GEDCOM 7.0 (GEDZIP) | `.gdz` | A ZIP archive: a standard GEDCOM 7.0 `gedcom.ged`, as GEDZIP requires, and the media files OxidGene holds | Recent exchange format bundling the media into one archive, read by software supporting GEDCOM 7.0. Ideal for full backups or sharing with attachments. A medium OxidGene holds no file for, which only a path outside the archive names, cannot be a GEDCOM `OBJE` there: the archive keeps it, with its path and links, as an OxidGene extension that other software ignores and an OxidGene import restores ([API Contract §4](api.md#gedzip)). It takes no destination. |

### Destination program (GEDCOM only)

Under the format row, set off by a rule, a **Destination program** select
chooses what the `.ged` is written for. Its options are the [export
destinations](api.md#export-destinations) the server lists (`GET
/gedcom/export-destinations`), in its order:

| Destination | Option label | Writes |
|---|---|---|
| Standard GEDCOM 5.5.1 (default) | Translated ("Standard GEDCOM 5.5.1") | GEDCOM 5.5.1 as OxidGene builds it, nothing reshaped: the version every program and site reads |
| Standard GEDCOM 7.0 | Translated ("Standard GEDCOM 7.0") | The same records as GEDCOM 7.0, nothing reshaped |
| A program (Geneanet, Ancestry, Heredis, Gramps, …) | The program's name, never translated | The version, layout and structures that program reads, as `oxidged`'s export profile for it writes them; what it cannot read is folded into what it reads, and the folds that lose something are reported after the export |

Beneath the select, one translated line describes what writing for the
chosen destination does (`export_destination.desc.<id>`); a program whose
profile is still to be verified against its latest version
(`to_be_verified`) adds a warning hint saying so. Until the list has loaded,
or if it cannot be read, the select offers the default alone. The choice
applies to the next export only and is not stored; a GEDZIP has no
destination and the select is not shown for it.

After a `.ged` export to a program is saved, the section reports what
writing for it lost, under the success message:

- nothing lost: "Written for Geneanet without losing anything.";
- otherwise, a disclosure per kind, each with its translated count as its
  summary — "3 limits of Geneanet exceeded", then "1,204 changes lost
  information for Geneanet" — listing the items: a change as its translated
  kind (Merged, Moved, Relabelled, Added, Written as text, Left out,
  Reordered, Rewritten, Changed), the record's cross-reference and
  `oxidged`'s technical description, in English, as import warnings are
  shown (the first 500 listed); a limit as its record and description.

A standard destination reshapes nothing and reports nothing. The report
goes away when another export starts.

### Export options (toggles)

Checkboxes under the destination (under the format row for a GEDZIP). Each
applies to the next export only: none is stored, on the tree or elsewhere,
and a reload restores the defaults. The section shows only the options of
the chosen format.

| Toggle | Description |
|---|---|
| Include notes and sources | On by default. When disabled, the export leaves out every note (of a person, a family, an event, a source, a repository or a medium), every source with its citations, and every repository, which only sources reach. Nothing written points at a record left out, so the file still imports cleanly. A medium's own description stays: it describes the medium, it is not a note |
| Include media (GEDZIP only) | On by default: the archive carries the photos and documents. When disabled, the archive holds `gedcom.ged` alone, and that GEDCOM has no `OBJE` record, no link to one and no identification (vignette) — not even a `FILE` reference to a file left behind, which an import would keep as a medium without its bytes. A plain GEDCOM never carries file bytes and always writes its media's references, so the option is not shown for it |
| Include contemporary persons (planned) | Leaves out the persons the contemporary-person rules hide. It waits for those rules ([§8](#contemporary-persons-planned)) and is not shown |

### Scope

Under the options, a **Scope** select limits the export to part of the tree
([API Contract, Export scope](api.md#export-scope)). Its options:

| Scope | Option label | Keeps |
|---|---|---|
| Whole tree (default) | Translated ("Whole tree") | Every record |
| A branch | Translated ("A branch") | A **root person** (a person picker, required), the **side** of the branch (descendants by default, ancestors, or ancestors and descendants), the **generations** (a number from 1 to 30; empty for all of them), an **Include the root person** checkbox (on by default) and, for descendants, an **Include the spouses of the descendants** checkbox |
| Persons carrying a tag | Translated ("Persons carrying a tag") | The persons carrying the chosen tag, from a select of the tree's tags whose labels are shown as they were written |

A branch or a tag is required before the export starts: until one is chosen,
the export shows a message saying so and makes no request. The generations
field shows what was typed; a value that is neither empty nor a whole number
from 1 to 30 shows a message beside the field and blocks the export the same
way until it is corrected. The scope applies to
the next export only and is not stored, as the other options; a reload restores
the whole tree. The export's audit entry records its format and, when it is
limited, its scope.

Export is triggered by the download button beside the format select
("Download .ged" or "Download .gdz").

### Submitter

A card under the export options says who the exported files are from —
GEDCOM's submitter (`SUBM`), which GEDCOM 5.5.1 requires and both exports
write: **Name**, **Email**
and **Address** (several lines), saved together by their own **Save** and
stored on the tree (`submitter_name`, `submitter_email`,
`submitter_address`). A blank name falls back to the person set as **Who am
I?** (§7), else to `Not Provided`; the email and address are written only when
set. An import fills those that are empty from the file's submitter, never
overwriting one.

GEDZIP exports use the shared download transport described in
[Common UI](ui-common.md). On browsers with a file-system save picker, the
destination is requested immediately on click, before starting the export job;
cancelling starts neither a job nor a file transfer. Once the job completes,
its artifact is streamed to that destination. Browsers without that capability
use a native Blob fallback without moving archive contents through WASM or JSON.
Desktop artifact downloads stream to a temporary file and only replace the
selected destination on success.

The server keeps a completed GEDZIP for one hour, however often it is
downloaded ([API Contract](api.md)). For that hour the section describes the
archive — its format, its size, and when it stops being available ("GEDZIP
archive of 2.4 MB, available until …") — beside a **Download again** button,
which saves the same archive again — through a new save picker or dialog —
without packing another export; a lost or failed save needs no new job. The
offer outlives the page: whenever the export section opens, it asks the
server for the tree's latest export still kept (`GET
/export-jobs/downloadable`), so a reload, another tab or the desktop
application offers the same archive. The file name proposed is the tree's,
as for a new export. At the expiry the button goes away, also on a page left
open, and a click that comes too late says the export has expired and must
be run again. An archive recorded before sizes were (an export from before
an upgrade) is described without its size.

---

## 11. Section: Global Preferences

Appearance, language, pedigree, name-display, date display and entry
options preferences are application-level settings shared with
[App Settings](ui-app-settings.md), kept on the device. Changing them from the
tree settings page immediately updates the same global preference, for every
tree opened on this device; they are not stored on the current tree, and are
not entries of its history.

Date Display and Entry Options are the sections
[App Settings §8](ui-app-settings.md#8-section-date-display) and
[§9](ui-app-settings.md#9-section-entry-options) describe: the date format
with its live preview, event symbols, « circa » and the display calendar; entry
suggestions, surnames in capitals, suggested persons, the input date format and
the default input calendar.

The shared Language section includes the same localized personal-JSON hint,
folder path and stacked spacing as App Settings. There is no separate global
preferences implementation of these controls.

The Pedigree section is the one [App Settings](ui-app-settings.md#7-section-pedigree)
shows: the chart the tree view draws (tree, ancestor or descendant wheel, ancestor or descendant fan, lineage or descendant lineage, hourglass or bowtie), the
pedigree theme, and the shared ancestor and descendant depth controls. The
type choices (chart and theme) and generation counters occupy separate blocks,
titled *Pedigree type* and *Pedigree depth*, with the same heading treatment
and spacing on both settings pages. The
chart is a per-device display preference like the theme, never a tree
setting, so nothing about it reaches the server.

The depth defaults are 4 ascending generations and 3 descending generations,
bounded independently from 0 through 10. A tree with saved view depths keeps
those depths; the global values initialize trees without a saved view.

---

## 12. Design Consistency

The settings page uses the shared `sub-page` layout and interaction states from
[Common UI](ui-common.md). The selected theme applies globally.

Every sidebar section switch resets the scrollable content container to offset
zero after the displayed section is updated,
including switches between global preferences and tree-specific settings. Short
sections do not inherit a long section's scroll position. Preference changes
within a section leave its scroll position intact. The section-dependent scroll
hook is shared with App Settings and does not depend on remounting content.
