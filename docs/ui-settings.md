---
type: "UI Specification"
title: "Visual & Functional Specifications — Tree Settings Page"
description: "Tree settings page for roots, privacy, date display, entry options, tools, and export."
tags: [oxidgene, specification, ui, ux]
generated: { by: claude-code/claude-opus-5-5, at: 2026-10-02T07:00:02Z }
---


# Visual & Functional Specifications — Tree Settings Page

> Part of the [OxidGene Specifications](index.md).
> See also: [Homepage](ui-home.md) (settings button on cards) · [Tree View](ui-genealogy-tree.md) · [Person Edit Modal](ui-person-edit-modal.md) (privacy per person) · [Dictionary](ui-dictionary.md) (family names / sources / places / occupations index, moved out of this page) · [Data Model](data-model.md) · [API Contract](api.md) (GEDCOM export)

---

## 1. Overview

The settings page (`/trees/{id}/settings`) is a dedicated full-page interface for configuring a single genealogy tree. It covers tree identity, privacy rules, display preferences, data entry options, the change history, and export. The tools that check a tree or work on it are on the [Tools](ui-tools.md) page. It is accessed via the **gear icon** in the [Tree View](ui-genealogy-tree.md) left sidebar or via tree card menus on the [Homepage](ui-home.md).

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
|   | - Date display   |                                              | |
|   | - Entry options  |                                              | |
|   |                  |                                              | |
|   | Tools            |                                              | |
|   | - History        |                                              | |
|   |                  |                                              | |
|   | Export           |                                              | |
|   | - Export tree    |                                              | |
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
| Date Display | `date-display` |
| Entry Options | `entry-options` |

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
submitter when none is set (§12). Changing or clearing it invalidates the tree
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

## 9. Section: Date Display

How the tree's pages write the dates of its records: event lists, the person,
couple and history pages, the pedigree and its events panel, search results,
the dictionary and tool lists, statistics, and their printed forms. The stored
dates never change; every page writes them through the one shared formatter
([Common UI §4.3](ui-common.md#43-dateinput)), which reads these settings from
the tree. Each control saves on the click, like the other settings, and the
pages follow at once.

Dates the application itself records — when an audit entry was written, when a
page was printed — keep the default form, as does the date editor's preview,
which reads the entry back in full, and the [date converter](ui-tools.md).

### Date format

A dropdown whose four options are the same day written each way:

| Option | Stored as | A day | A month | Qualified |
|---|---|---|---|---|
| `12 Mar 1842` (default) | `day_month_year` | 12 Mar 1842 | Mar 1842 | before 12 Mar 1842 |
| `12/03/1842` | `numeric` | 12/03/1842 | 03/1842 | before 12/03/1842 |
| `Mar 1842` | `month_year` | Mar 1842 | Mar 1842 | before Mar 1842 |
| `1842` | `year` | 1842 | 1842 | before 1842 |

The month's name is the reader's language's. A date's qualifier is always
written, whatever the format. The numeric format writes a Gregorian or Julian
month as its number; a Republican or Hebrew month keeps its name, its number
telling nobody which month it is. A sentence reporting a date written without
its day joins it as a period (« en 1842 »).

A **live preview** below the dropdown shows a fictitious person's dates as the
pages will write them with every setting of the section: a birth recorded in
the Republican calendar, a marriage, a death known only roughly, and the
lifespan a pedigree card draws. It follows each change before the save
returns.

### Event symbols

Yes / No (default No). With Yes, a lifespan — on pedigree cards, the wheels,
fans and lineages, the events panel, the person narrative, the dictionary and
tool lists — writes each year behind its event's symbol, `*` for the birth and
`+` for the death (`* 1842 + 1907`, `* 1842`, `+ 1907`), instead of joining
them with a dash (`1842-1907`, `1842-`, `-1907`). As without symbols, the
birth year may come from a baptism and the death year from a burial; the
symbol names the end of the life, not the record. Search results and pickers
already mark both years with their own glyphs.

### « Circa » for approximate dates

Yes / No (default No). With Yes, a date entered as approximate reads with the
short « c. » of the reader's language — *c. 1842*, *v. 1842* in French — rather
than its qualifier's word (*about 1842*). Calculated and estimated dates keep
their words. Year-only surfaces keep their GeneWeb marks (`ca 1842`) either way.

### Display calendar

Dropdown: Gregorian (default) / Julian / Republican / Hebrew. A date recorded
in another calendar is followed by its equivalent in this one, in
parentheses: the day, or for a year or a month alone the span it covers, which
reads as one value when both ends write the same (a year-only format). A date
recorded in this calendar, a range, a free-text phrase, and a date this
calendar cannot express (anything before the Republic in the Republican
calendar) are written as recorded, alone. Every calendar the application
records has a converter. Years shown alone — lifespans — stay Gregorian.

Stored as `tree.date_format`, `tree.date_symbols`, `tree.date_circa` and
`tree.date_calendar` ([Data Model](data-model.md#tree)); each change is a
*Settings* entry of the [history](#11-section-history).

---

## 10. Section: Entry Options

How the tree's forms help with entry. Each control saves on the click and is
handed back to the tree cache, so a form opened afterwards follows it; each
change is a *Settings* entry of the [history](#11-section-history).

### Data entry assistance

Three Yes / No cards:

| Toggle | Description |
|---|---|
| Entry suggestions | Yes (default) / No. Place, surname, given-name, occupation and source fields, the search filters' included, suggest what the tree holds and the built-in dictionaries. Stored as `tree.entry_suggestions`. See [Common UI §4.4](ui-common.md) |
| Automatic uppercase for surnames | Yes (default) / No. The surname entry fields — the person form's birth name and its other names, the dictionary's family-name rename — write what is typed in capitals and suggest surnames in capitals. With No, a surname is stored as typed. Search filters never change case. Stored as `tree.surname_uppercase` |
| Suggest existing persons | Yes (default) / No. When a parent, partner, child or sibling is added from the [tree view](ui-genealogy-tree.md)'s action picker, the panel searches the tree's persons before offering to create one; the person form creating a parent from an empty slot lists the persons matching the name being typed, to link instead ([Person Edit §12](ui-person-edit-modal.md)). With No, only a new person is offered. Stored as `tree.suggest_persons` |

### Input date format

A dropdown, each option named with the reader's own placeholders:

| Option | Stored as | Fields |
|---|---|---|
| `DD/MM/YYYY` (default) | `slashes` | day `/` month `/` year, the month typed as a number |
| `DD-MM-YYYY` | `dashes` | day `-` month `-` year |
| `YYYY-MM-DD` (ISO 8601) | `iso` | year `-` month `-` day |
| `DD Mar YYYY` | `month_name` | day, month picked by name, year |

It lays out every [DateInput](ui-common.md#43-dateinput) of the tree. A
Republican or Hebrew date always picks its month by name, without
separators, whatever the format. Stored as `tree.date_input_format`.

### Default calendar for input

Same options as the display calendar. An empty date field starts in it; the
field's own calendar selector still changes it, and a date already entered
keeps its calendar. Stored as `tree.date_input_calendar`.

### Place dictionary

The [place dictionary](place-dictionary.md) (France, the United Kingdom, Germany,
Italy, Spain, Switzerland, Poland, the United States, Portugal, Belgium,
Luxembourg and the Netherlands)
is built into the application and needs neither a download nor network
access. There is nothing to manage here.

---

## 11. Section: History

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

## 12. Section: Export

Two export format options, each displayed as a card with icon, name, description and an action button:

| Format | Extension | Description |
|---|---|---|
| GEDCOM 5.5.1 | `.ged` | Universal standard. Compatible with Ancestry, Geneanet, MyHeritage, MacFamilyTree and most genealogy software. |
| GEDZIP | `.gdz` | GEDCOM archive including associated media files (photos, documents). Ideal for full backups or sharing with attachments. |

### Export options (toggles)

Checkboxes under the format row. Each applies to the next export only: none
is stored, on the tree or elsewhere, and a reload restores the defaults. The
section shows only the options of the chosen format.

| Toggle | Description |
|---|---|
| Include notes and sources | On by default. When disabled, the export leaves out every note (of a person, a family, an event, a source, a repository or a medium), every source with its citations, and every repository, which only sources reach. Nothing written points at a record left out, so the file still imports cleanly. A medium's own description stays: it describes the medium, it is not a note |
| Include media (GEDZIP only) | On by default: the archive carries the photos and documents. When disabled, the archive holds `gedcom.ged` alone, and that GEDCOM has no `OBJE` record, no link to one and no identification (vignette) — not even a `FILE` reference to a file left behind, which an import would keep as a medium without its bytes. A plain GEDCOM never carries file bytes and always writes its media's references, so the option is not shown for it |
| Merge occupations into a single field (GEDCOM only) | Off by default (one `OCCU` tag per profession, lossless). When enabled, collapses a person's multiple `OCCU` tags back into one, comma-separated, for compatibility with importers such as Geneanet that only support a single profession field. See [API Contract](api.md) (GEDCOM) |
| Merge name aliases into a single field (GEDCOM only) | Off by default (one `NAME`/`SURN` structure per name, lossless). When enabled, collapses a person's non-primary names into the primary name's `SURN` tag, comma-separated, for compatibility with importers such as Geneanet that only read the first `NAME` structure. See [API Contract](api.md) (GEDCOM) |
| Include contemporary persons (planned) | Leaves out the persons the contemporary-person rules hide. It waits for those rules ([§8](#contemporary-persons-planned)) and is not shown |

The options travel with the export request — the GEDCOM request, or the
GEDZIP export job, which keeps them until a worker packs the archive
([API Contract](api.md)). The export's audit entry records its format only.

Export is triggered directly by the format buttons.

### Submitter

A card under the export options says who the exported files are from —
GEDCOM's submitter (`SUBM`), which GEDCOM 5.5.1 requires: **Name**, **Email**
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

## 13. Section: Global Preferences

Appearance, language, pedigree, and name-display preferences are
application-level settings shared with [App Settings](ui-app-settings.md).
Changing them from the tree settings page immediately updates the same global
preference; they are not stored on the current tree.

The Pedigree section is the one [App Settings](ui-app-settings.md#7-section-pedigree)
shows: the chart the tree view draws (tree, ancestor or descendant wheel, ancestor or descendant fan, lineage or descendant lineage, hourglass or bowtie), the
pedigree theme, and the shared ancestor and descendant depth controls. The
chart is a per-device display preference like the theme, never a tree
setting, so nothing about it reaches the server.

The depth defaults are 4 ascending generations and 3 descending generations,
bounded independently from 0 through 10. A tree with saved view depths keeps
those depths; the global values initialize trees without a saved view.

---

## 14. Design Consistency

The settings page uses the shared `sub-page` layout and interaction states from
[Common UI](ui-common.md). The light/dark theme applies globally.
