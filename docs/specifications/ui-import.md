---
type: "UI Specification"
title: "Visual & Functional Specifications — Import"
description: "The import modal for GEDCOM, GEDZIP, GeneWeb, and Geneanet trees with media."
tags: [oxidgene, specification, ui, ux]
timestamp: 2026-08-18T00:00:00Z
---


# Visual & Functional Specifications — Import

> Part of the [OxidGene Specifications](index.md).
> See also: [Geneanet Media Import](geneanet-media-import.md) (the pipeline
> behind the Geneanet tab) · [Geneanet Upload API](geneanet-upload-api.md) ·
> [Homepage](ui-home.md) · [Settings](ui-settings.md) (export) ·
> [API Contract](api.md)

---

## 1. Overview

Everything that brings data *into* a tree arrives through one modal, opened
from a tree card's `⋮` → **Import** on the [Homepage](ui-home.md) or from
[Settings](ui-settings.md) → Tools. It never creates a tree: it is opened from
a tree's own menu, so the destination is already chosen.

Two tabs, because there are two genuinely different jobs:

| Tab | What it is |
|---|---|
| **A file** | Pick or drop a `.ged`, `.gdz` or `.gw` and import it. Seconds, one decision. |
| **From Geneanet** | Five guided steps that import a GeneWeb tree and recover its media through an authenticated desktop session. |

Splitting them across a modal and a page was tried and rejected: they are one
decision made at one moment, and separating them made the cheap route feel like
the real one and the complete route like an excursion.

---

## 2. The modal

~820 px wide, `max-height: 90vh`, header + tab strip fixed while the body
scrolls.

```
┌─ Import into "<tree name>" ───────────────────────────── [×] ─┐
│  ● A file        ○ From Geneanet                              │
├───────────────────────────────────────────────────────────────┤
│  ┌─────────────────────────────────────────────────────────┐  │
│  │   📄  Drop a file here, or click to browse               │  │
│  │       .ged · .gdz (with media) · .gw                    │  │
│  └─────────────────────────────────────────────────────────┘  │
│                                              [ Import ]       │
└───────────────────────────────────────────────────────────────┘
```

**It cannot be dismissed while an import is running.** Neither the backdrop nor
the ✕ responds — dismissing would throw away however many steps had been
settled and orphan a request still writing to the tree, and a stray click on
the page behind is all it would take.

Dismissal is on *press*, not click: a click fires on the common ancestor of
mousedown and mouseup, so selecting text inside the modal and releasing outside
it would otherwise close the modal.

---

## 3. The file tab

A drop zone that is also a button. Accepts one `.ged`, `.gdz` or `.gw`;
drag-and-drop and the native picker both work.

The bytes are read with the file handle's `read()`, never a path. A file picked
in a browser has no path at all, and this used to read one — which quietly made
"the shared web/desktop import" desktop-only, with nothing saying so.

The three are told apart by extension and sent to three endpoints, because
each arrives differently: a `.ged` is a UTF-8 string, a `.gw` is ISO-8859-1
unless it declares otherwise so only its reader can decode it, and a `.gdz` is
a ZIP. Anything with an unrecognised extension is read as GEDCOM — the reader
says so soon enough if it is not, and a renamed `.ged` is common. See
[API Contract §Import](api.md).

**Only `.gdz` brings the photographs.** A GEDZIP is a `gedcom.ged` and the
media files it references in one archive, so the media arrive held: stored,
thumbnailed, croppable, exactly as if they had been uploaded. A `.ged` and a
`.gw` name files nobody handed us, and those stay unheld records the user can
attach bytes to later. A file the archive turns out not to carry, or one no
`OBJE` names, is a warning on the result — never a failed import.

The association between an event and its media is standard GEDCOM: the event
carries an `OBJE` reference to the global multimedia record. It therefore
survives both formats. The difference is only the bytes: a `.ged` keeps the
`FILE` reference, while a `.gdz` embeds each stored file it names. Remote and
unheld media remain references in either format.

What a GEDZIP round trip does **not** carry back is the structure GEDCOM has no
tag for: a multi-page document's pages export as separate `OBJE` records, so
re-importing the archive gives back that many separate media rather than one
document with its pages, and a medium's original file name is lost where the
archive path is `media/{uuid}.{ext}` — which is our own exporter's, chosen
because two scans called `photo.jpg` in one tree is routine.

### What this tab does not do

An earlier version of this spec described a three-step wizard — upload and
validate, preview with sample persons and stat cards, then import — and **none
of it was built**. What exists is: choose, import, see the counts.

That is recorded rather than quietly dropped, because the preview step is
genuinely useful for a GEDCOM from an unknown source. It is unbuilt, not
rejected. The Geneanet tab has its own preview (step 4) because there the
question — *do these two halves belong to each other?* — has a real answer that
cannot be guessed at.

### Result

The counts every import reports, whatever the source: people, families, events,
sources, places, media. Warnings collapse behind a disclosure with a count.

---

## 4. Errors

| Condition | Message |
|---|---|
| A `.ged` that is not UTF-8 | *"This GEDCOM file is not valid UTF-8. Re-export it as UTF-8 from your genealogy software, or import the GeneWeb (.gw) version instead."* |
| The file cannot be read | The reader's own message |
| The import fails | The API's message, and the modal stays open with the file still chosen |

---

## 5. Data mapping & fidelity

The import uses `ged_io` 0.12 to parse GEDCOM files. See [API Contract](api.md) §3 for the full round-trip fidelity table.

### What imports cleanly

| GEDCOM records | Mapping |
|---|---|
| INDI (individuals) | → Person + PersonName(s) + Events |
| FAM (families) | → Family + FamilySpouse + FamilyChild links |
| All standard event tags (BIRT, DEAT, BAPM, MARR, etc.) | → Event with matching EventType |
| SOUR (sources) | → Source (title, author, publisher, abbreviation) |
| Citation references with QUAY | → Citation with Confidence mapping |
| NOTE (notes) | → Note linked to the parent record |
| OBJE (multimedia) | → Media (file path + MIME type + title, metadata only) |
| PLAC with MAP coordinates | → Place (name + latitude + longitude) |
| Event CAUS (cause) | → Event.cause field |
| FAMC PEDI (pedigree type) | → FamilyChild.child_type (Biological / Adopted / Foster) |

### What is skipped (not imported)

These GEDCOM tags are parsed by ged_io but not mapped to the OxidGene data model. They are listed in the import warnings.

| GEDCOM tag | Description | Reason |
|---|---|---|
| REPO | Repository records | Not in current data model |
| SUBM | Submitter records | Not relevant (single-user MVP) |
| AGE | Age at event | Not stored; can be calculated from dates |
| RELI | Religion of event | Not in current data model |
| AGNC | Agency responsible | Not in current data model |
| ASSO | Associations between individuals | Not in current data model |
| `_CUSTOM` tags | Vendor-specific extensions | Silently ignored |

### GEDCOM version handling

- **Import**: ged_io auto-detects GEDCOM 5.5.1 and 7.0 formats
- **Export**: always produces GEDCOM 5.5.1 (LINEAGE-LINKED format)
- Files imported from GEDCOM 7.0 will export as 5.5.1 — no data is lost in the conversion for supported tags

---

## 6. File size handling

| File size | Behavior |
|---|---|
| < 1 MB | Instant upload, synchronous import |
| 1–10 MB | Upload progress bar visible, synchronous import |
| 10–50 MB | Upload progress bar + streaming import with step-by-step progress |
| > 50 MB | Rejected at upload with error message |

---

## 7. Keyboard & accessibility

| Key | Behavior |
|---|---|
| `Escape` | Close the modal — inert while an import is running |
| `Enter` | Activate the focused control |
| `Tab` | Move between controls |

The Geneanet tab uses collapsed steps as buttons, moves focus into the expanded
step, and announces progress stage changes through a live region.

---

## 8. Responsive

- The modal is `min(820px, 94vw)` wide; its body is the only thing that scrolls
- Stat rows wrap 4 → 2 → 1 at 900 px and 560 px
- The drop zone is always full width

## What a GEDZIP round trip does to the portrait

GEDCOM has no primary-photo flag, so the choice cannot be *stated* — but it can
be implied, because order survives. A person's `OBJE` links are written portrait
first — the first `OBJE` under an `INDI` being the primary one by long
convention — and the import reads it back into `person.portrait_media_id`, so
the choice arrives *stored* rather than merely drawn. Link order is recorded
explicitly on import (`media_link.sort_order` follows the file) rather than left
to insertion order.

Storing it matters beyond the avatar: the gallery's star marks the stored
choice, so a portrait that was only implied came back looking right and with no
star — and nothing for "remove as profile photo" to remove. An existing choice
in the file is never overwritten.

A portrait that is a **crop** cannot cross: GEDCOM cannot express a region of an
image as somebody's portrait at all. Those people come back represented by their
first whole picture, and the crop itself survives as a vignette on the scan.

## What a GEDZIP round trip does to a multi-page document

Nothing carries it across, and nothing pretends to. GEDCOM has no notion of a
document assembled from page images, so on export the container **dissolves**:
each page is written as an ordinary standalone `OBJE`, and everything that was
linked to the document is linked to every one of its pages, in reading order.
Re-importing yields *n* one-page media, each attached to the same people the
document was.

The document row itself is not exported at all. It holds no bytes — its
`file_path` is its title — so writing it produced a `FILE` naming something no
archive could contain, which is where the *"the archive holds no file named
'Dossier de naturalisation…'"* warnings came from. Writing it as its cover
instead was worse: the person kept page one and lost the other thirty-seven.

So the scans survive a round trip, with their bytes and their owners. The
grouping does not: a thirty-eight page dossier comes back as thirty-eight
pictures. Re-assembling them into a document is a manual step, and the pages
keep their order in the gallery because the links were written in it.

---

## 9. From Geneanet tab

Geneanet provides genealogy and media information separately. The `.gw` file
contains the GeneWeb key required to distinguish same-named people, while the
person-to-media links are available only through an authenticated Geneanet
session. This tab is both an instructional guide and an import form.

Every user-visible instruction, state, warning, error, tooltip, screenshot
description, and accessibility label uses the i18n mechanism with English and
French key parity.

### 9.1 Five-step structure

Exactly one of these steps is expanded at a time:

1. Select the GeneWeb tree file.
2. Select optional Geneanet data archives.
3. Connect to Geneanet.
4. Review and gather the import.
5. Write the import.

A completed step collapses to a one-line receipt with a status icon, aggregate
counts, and an **Edit** action. Reopening it collapses the current step.
Unreachable steps remain visible but disabled.

### 9.2 Platform capabilities

| Step | Web | Desktop | Reason |
|---|---|---|---|
| GeneWeb file | Yes | Yes | Small file read through the shared file API. |
| Data archives | No | Yes | Multi-gigabyte ZIPs are indexed by path without loading their contents. |
| Geneanet login | No | Yes | Collection runs in a second authenticated WebView. |
| Preview and gathering | Genealogy only | Yes | Media gathering depends on archives and login. |
| Import | Genealogy only | Yes | Media bytes are desktop-only. |

Unavailable web controls are replaced by an explanation naming the desktop
application. The web build can still import the `.gw` genealogy.

### 9.3 Step 1: GeneWeb tree

The UI explains how to export GeneWeb (`.gw`) from Geneanet and why GEDCOM is
insufficient for media recovery. The file is parsed immediately, before any
network operation.

- A valid file collapses to a sanitized filename and person count.
- A GEDCOM file explains that the GeneWeb export is required.
- A file containing no people is rejected.
- Recoverable skipped blocks are reported as warnings.

Instructions stand on their own. Optional screenshots may illustrate external
steps, but they use an anonymized account, tightly crop unrelated data, include
localized alt text, and are never required to complete the flow.

### 9.4 Step 2: data archives

This optional desktop step accepts multiple Geneanet data-export ZIP files.
The user is told not to extract them. Each archive's central directory is
indexed without reading media bytes.

- Selected archives can be removed individually.
- Duplicate archives are ignored.
- A corrupt archive affects only that archive.
- An archive with no supported images is accepted with a warning.
- Skipping the step means unmatched media will be downloaded in step 4.

### 9.5 Step 3: authenticated collection

The desktop app opens an incognito Geneanet WebView at the media manager. The
user authenticates directly in that window, including any captcha or
Cloudflare challenge. OxidGene never receives or stores the password.

- Requests execute inside the WebView session; the server and
	`oxidgene-geneanet` crate perform no direct Geneanet HTTP requests.
- The remember-me control may be preselected but remains visible and editable.
- The incognito context is destroyed when the window closes.
- After authentication, the window becomes a visible status panel. The import
	modal owns progress so two counters cannot disagree.
- Closing the window cancels collection and resets the step.
- Session expiry permits reauthentication without discarding completed work.
- Saved collection payloads are sensitive and must never be committed.

Collection first reads media links, then matches media against selected archive
indexes. The login window closes once no remaining request needs its session.

### 9.6 Step 4: preview and gathering

This step writes nothing. It shows aggregate counts for people, media, people
with media, and links, plus expandable mismatch summaries. It then asks the
login WebView for bytes that archives cannot supply.

If fewer than 10 percent of keyed references match people in the `.gw`, the
account and export are likely unrelated. The flow blocks by default and offers
actions to replace the file or explicitly continue.

Media resolution uses, in order:

1. exact archive size matches for single-page media;
2. perceptual matches against gathered renditions when size is unavailable;
3. gathered bytes when no archive match exists.

All direct Geneanet access ends before the final write.

### 9.7 Step 5: local write

The genealogy uses the shared GeneWeb persistence path. Media use the ordinary
storage path, preserving validation, deduplication, type detection, thumbnails,
and projection refresh.

Geneanet media bytes use the desktop filesystem as their data plane. The login
WebView writes each gathered medium to a temporary staging directory shared
with the embedded backend. The final REST or GraphQL call carries only the
`source URL -> local path` map and import metadata; it never uploads, embeds,
or base64-encodes those media bytes. The backend reads each staged file locally
and passes it to `MediaStore`. This workflow is desktop-only and requires the
UI and embedded backend to share a filesystem.

- Shared media are stored once and linked many times.
- Failed media are reported and skipped without rolling back the genealogy.
- Archive matches are never downloaded again.
- A multi-page deposit imports as one document plus ordered page media.
- Missing pages are reported by page number.

The receipt contains aggregate counts and skipped-item summaries, followed by
**Open the tree** and **Import another**. It never displays an account name.

### 9.8 Known limitations

- Per-media determinate write progress and cancellation are not implemented.
	Interrupting the media pass can leave complete genealogy with partial media.
- Unlinked media are counted but not imported.
- Event links are created only when type, date, and optional normalized place
	identify exactly one event; ambiguous references remain person-media links.
- Re-import is not incremental and does not merge existing data.
- The flow never writes to Geneanet or automates password entry.
- Only one Geneanet login window may run at a time.

### 9.9 Privacy and accessibility

- Import content, archive indexes, sessions, logs, screenshots, and error
	reports are sensitive.
- Diagnostics expose aggregate counts and sanitized filenames, not account or
	person identifiers.
- Tests, fixtures, screenshots, and documentation use fictitious people,
	accounts, trees, places, and archive references.
- Collapsed steps are buttons; `Enter` and `Space` reopen them.
- The first control receives focus when a step expands.
- Progress bars expose their values and announce stage changes, not every tick.
- The login WebView manages its own focus while open.
