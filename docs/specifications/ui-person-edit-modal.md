---
type: "UI Specification"
title: "Visual & Functional Specifications — Person Edit Modal"
description: "UI behavior and interaction specification for Visual & Functional Specifications — Person Edit Modal."
tags: [oxidgene, specification, ui, ux]
timestamp: 2026-06-17T00:00:00Z
---


# Visual & Functional Specifications — Person Edit Modal

> Part of the [OxidGene Specifications](index.md).
> See also: [Tree View](ui-genealogy-tree.md) (action picker opens this modal) · [Settings](ui-settings.md) (privacy rules, date display, entry options) · [Data Model](data-model.md) (Person, PersonName, Event, Family, Media) · [API Contract](api.md) (Persons, Events, Media endpoints)

---

## 1. Overview

A single modal is used for both **creating** and **editing** a person. The form content is identical — the only differences are in the header, footer actions, and optional pre-filled fields.

| Aspect | Create mode | Edit mode |
|---|---|---|
| **Header title** | Varies by context (see §3) | Person's full name |
| **Header subtitle** | "New person" | "Edit individual" |
| **Footer actions** | Cancel · **Create** | Delete · Cancel · **Save** |
| **Pre-filled fields** | Depends on context (see §3) | Current person data |

The modal opens in **edit mode** when the user selects "Edit individual" from the action picker. It opens in **create mode** when the user triggers any "Add person" action (see §3).

---

## 2. Modal Structure

### Container

- **Type**: centered overlay modal, not a drawer
- **Size**: fixed width ~720px, max-height 90vh, internally scrollable
- **Backdrop**: dark semi-transparent blur overlay; click outside closes without saving
- **Scroll**: the modal body scrolls independently; the header and footer remain fixed

```
┌─────────────────────────────────────────────────┐  ← fixed header
│  <person A>                      [×]            │
│  Edit individual / New person                   │
├─────────────────────────────────────────────────┤
│                                                 │  ← scrollable body
│  ── Civil Status ───────────────────────────    │
│  ...fields...                                   │
│                                                 │
│  ── Birth ──────────────────────────────────    │
│  ...fields...                                   │
│                                                 │
│  ── Death ──────────────────────────────────    │
│  ...fields...                                   │
│                                                 │
│  ── Privacy ────────────────────────────────    │
│  ...                                            │
│                                                 │
│  ── Additional fields ──────────────────────    │
│  [+ Show supplementary fields]                  │
│                                                 │
│  ── Other events ───────────────────────────    │
│  [+ Add an event]                               │
│                                                 │
│  ── Media ──────────────────────────────────    │
│  [gallery + upload]                             │
│                                                 │
│  ── Delete ─────────────────────────────────    │  ← edit mode only
│  [Delete this person]                           │
│                                                 │
├─────────────────────────────────────────────────┤  ← fixed footer
│  [Cancel]                       [Create / Save] │
└─────────────────────────────────────────────────┘
```

### Fixed Header

- **Title**: person's current full name (edit mode) or context-specific title (create mode, see §3)
- **Subtitle**: "Edit individual" (edit mode) or "New person" (create mode)
- Close button `×` top-right — closes without saving, prompts confirmation if there are unsaved changes

### Fixed Footer

- **Cancel** button (ghost style) — closes without saving
- **Create** button (orange gradient, create mode) or **Save** button (orange gradient, edit mode)

### Button Hierarchy

The modal holds a lot of controls — one "add" per list, two or three actions per
list row — so their styling is a fixed three-tier rule rather than a per-case
choice. Without it every control reads as equally urgent and the one that
matters, the footer save, is lost among them.

| Tier | Role | Treatment | Class |
|---|---|---|---|
| 1 | The modal's action | Filled orange gradient + shadow. **Exactly one per modal** | `.btn.btn-primary` |
| 2 | Commit an open sub-form ("Create the occupation", an inline row editor's Save) | Orange border and text on a 10% orange tint. At most one sub-form is open at a time, so two never coexist | `.pf-confirm-btn` |
| 3 | Everything else — open a sub-form, edit or delete a row | Monochrome at rest, colour only on hover. Labels stay legible when idle: a control revealed only on hover is unreachable by touch | `.pf-add-btn`, `.pf-row-btn` |

- `.pf-add-btn` prefixes a `+` glyph, swapped for `×` via `.is-open` while its
  sub-form is showing.
- `.pf-row-btn.is-danger` is a row's own delete: muted like its neighbours,
  turning red **on hover only**.
- `.pf-row-btn.is-active` marks a row whose "Notes & source" panel is expanded.
- **Filled red (`.btn-danger`) is reserved for a destructive action already
  behind a confirmation** — the final button of the delete-person prompt (§9)
  and the discard-unsaved-changes dialog. A row-level delete never uses it.
- Section headings (`.pf-section-title`) keep the orange. Uppercase,
  letterspaced and 0.68rem, they cannot be mistaken for a control, and they are
  what makes the form's spine scannable — the rule above governs buttons only.
- An open sub-form (`.pf-subform`) sits on `--bg-card` with a border, not on
  `--bg-deep`: in the light palette the latter is a hair off the modal's own
  `--bg-panel`, leaving the box edgeless and its fields indistinguishable from
  the surrounding form.

---

## 3. Create Mode — Context Variants

In create mode, the modal adapts its title and pre-filled fields based on the trigger:

### Add Spouse

**Trigger**: "Add spouse" from the action picker on a selected person.

| Aspect | Behavior |
|---|---|
| Title | "Add spouse to <person A>" |
| Gender | Pre-selected to the opposite of the existing person (if Male → Female, and vice versa). Editable. |
| Relationship created on save | A new Family is created (or the existing one is used if the person has no union yet). The new person is added as a FamilySpouse. |
| Union section | A collapsed "Union details" section is available (date, place, note, source for the marriage). Same fields as the union block in the [couple edit modal](#14-couple-edit-modal). |

### Add Child

**Trigger**: "Add child" from the action picker.

| Aspect | Behavior |
|---|---|
| Title | "Add child to <person A> & <person B>" (if the person has a union) or "Add child to <person A>" (if no union) |
| Surname | Pre-filled with the selected person's surname. Editable. |
| Gender | Not pre-selected. |
| Union selector | If the selected person has **multiple unions**, a dropdown at the top of the modal asks which union this child belongs to. |
| Relationship created on save | The new person is added as a FamilyChild to the selected union. |

### Add Sibling

**Trigger**: "Add sibling" from the action picker.

| Aspect | Behavior |
|---|---|
| Title | "Add sibling of <person A>" |
| Surname | Pre-filled with the selected person's surname. Editable. |
| Gender | Not pre-selected. |
| Relationship created on save | The new person is added as a FamilyChild to the **same Family** as the selected person (i.e. the Family where the selected person is a child). If the selected person has no parent family, a new Family is created with the selected person's parents (if known). |

### Add Parent (from placeholder)

**Trigger**: clicking the `+` on an unknown parent placeholder card at the top of the tree.

| Aspect | Behavior |
|---|---|
| Title | "Add father of <person A>" or "Add mother of <person A>" (depending on the placeholder position) |
| Gender | Pre-selected (Male for father, Female for mother). Editable. |
| Surname | Pre-filled with the child's surname (for father) or empty (for mother). Editable. |
| Relationship created on save | The new person is added as a FamilySpouse to the child's parent Family (creating one if it doesn't exist). |

### Add Person (standalone)

**Trigger**: ＋👤 button in the left sidebar.

| Aspect | Behavior |
|---|---|
| Title | "Add a person" |
| Pre-filled fields | None. |
| Relationship created on save | None — the person is added to the tree without any family link. |

---

## 4. Section: Civil Status

Displayed as the first block in the scrollable body, with a section divider label "Civil Status".

### Family Name

Single text input. Automatically converted to uppercase on input.

### First Names

Dynamic list of first name entries. Each entry is a text input with a remove button (`×`). An **"+ Add a first name"** button appends a new entry at the bottom of the list. Order is significant (the first entry is the used first name). Entries can be reordered via drag handle.

### Gender

Radio group with three options displayed as labeled buttons:

| Value | Label |
|---|---|
| `M` | Male |
| `F` | Female |
| `?` | Unknown |

### Occupations

Occupations are stored as **Occupation events** (EventType `Occupation`), each with a date and place — not as free-text fields. However, for convenience, the civil status section presents them as a simplified dynamic list:

- Each entry has a text input (occupation title), an optional date field, and an optional place field
- An **"+ Add an occupation"** button appends a new entry
- Under the hood, each entry creates an Event of type `Occupation` with the title in the `description` field

This ensures GEDCOM round-trip fidelity (GEDCOM `OCCU` tag maps to `EventType::Occupation`).

### First Name Aliases

Dynamic list of text inputs. Represents known alternate first names (e.g. a common name vs. a registered name). Same add/remove pattern.

### Notes on this person

A list of notes rather than a single field: each row is a `Note` carrying `person_id`, added through **"+ Add a note"** (multi-line textarea), then editable in place (**Edit** expands the row into a textarea with Save / Cancel, writing through `PUT /notes/{id}`) or removable. Notes attached to an event are *not* listed here — they belong to that event's own panel (§5, §9).

The block sits at the end of Civil Status, after Additional Information. Its heading uses the same weight and colour as a field label such as "Gender", not a section title: these blocks are peers of the fields around them.

### Source

Free-text input, persisted as a `Citation` carrying `person_id`. Saved with the modal's footer button along with the rest of the section.

**Source fields are text, not pickers.** A source is typed the way it is read off the record — "AD44 — Vigneux-de-Bretagne — N — 1913 — 3E217/46" — and requiring the `Source` row to exist first would put a detour in the middle of entering an event. The typed title is reconciled against the tree's sources on save: a case-insensitive match on the trimmed title reuses that row, anything else creates one. Sources are only touched when the typed title actually changed, so an unrelated save never creates a `Source` row as a side effect, and changing the title repoints the existing citation (`PUT /citations/{id}` with `source_id`) rather than deleting and recreating it. The source it just let go is then collected — `DELETE /sources/{id}?only_if_unused=true` drops it only when no citation, note and media link still names it — so correcting a typo does not leave its `Source` behind, while a source still in use anywhere is kept.

There is deliberately **no completion dropdown**: a `<datalist>` holding every source in the tree had to be re-diffed on each keystroke, which made the field unusable on an imported tree. Completion belongs on a debounced prefix query against `dictionary_sources`, not on a list of everything.

Notes and sources are stored separately on purpose — a `Citation` always needs a `source_id`, so it cannot hold sourceless notes, and folding the notes into `Citation.text` loses them the moment the source is cleared.

---

## 5. Section: Birth

Displayed as the second block with a section divider label "Birth".

### Date

A **date qualifier selector** + one or two date input fields, depending on the qualifier.

**Qualifier options** (dropdown or segmented control):

| Qualifier | Fields shown | Example |
|---|---|---|
| Exact | 1 date field | 12/03/1842 |
| Around (circa) | 1 date field | c. 1842 |
| Perhaps | 1 date field | ? 1842 |
| Before | 1 date field | before 1842 |
| After | 1 date field | after 1842 |
| Or | 2 date fields | 1841 or 1842 |
| Between | 2 date fields | between 1840 and 1845 |
| From age | 1 numeric field | age 35 (→ calculated year) |

**Date input field**: text input accepting `dd/mm/yyyy`, `mm/yyyy`, or `yyyy`. Partial dates are valid.

When **two fields** are shown (Or / Between), they are displayed side by side with a label between them ("or" / "and").

### Place

Single text input with **place autocomplete** (see
[Common UI §4.4](ui-common.md)). Its localized placeholder describes the
expected hierarchy without embedding a specific location.

### Description

Single-line text input, free text — the event's own `description` field. Named "Description" rather than "Note" so it is not confused with the Notes block below, which is backed by `Note` rows.

### Notes

Multi-line textarea, persisted as a `Note` carrying `event_id`.

### Source

Free-text input (see §4 — Source), persisted as a `Citation` carrying `event_id`. Both fields are saved with the modal's footer button; if no birth event exists yet, they are attached to the event that same save creates.

---

## 6. Section: Death

Identical structure to the Birth section. Same date qualifier options, same place / description / notes / source fields.

Section divider label: "Death".

---

## 7. Section: Privacy

A single selector displayed below the Death section.

**Options** (radio group):

| Value | Label | Description |
|---|---|---|
| `default` | Default | Follows the tree-level privacy settings |
| `public` | Public | Always visible regardless of tree settings |
| `private` | Private | Always hidden regardless of tree settings |

---

## 8. Section: Additional Fields

Collapsed by default. Revealed by clicking **"+ Show supplementary fields"**. Once expanded, this button becomes **"− Hide supplementary fields"**.

### Civil Status supplements

| Field | Type | Notes |
|---|---|---|
| Nickname | Text input | Informal name used in daily life |
| First name alias | Text input | Alternative registered first name |
| Family name alias | Text input | Maiden name, name before adoption, etc. |

### Birth supplements

**Calendar selector** — dropdown to specify the calendar system used for the birth date entry:

| Option |
|---|
| Gregorian (default) |
| Julian |
| Hebrew |
| French Republican |

The calendar governs how the rest of the date row behaves:

- **Month entry** — Gregorian and Julian months are typed as a number, since
  everyone reads those. Hebrew and Republican months are picked from a
  dropdown listing their own names (vendémiaire … fructidor, plus the jours
  complémentaires; Tishrei … Elul), because a bare "3" says nothing in a
  calendar nobody counts in. The list opens with a **blank entry**: a date
  known to the year alone — « an VI », no month in the record — is ordinary,
  and must stay enterable exactly as leaving MM empty allows in Gregorian.
  Adar II is offered only in a Hebrew leap year, the only kind that has one
  (a month already entered is always listed, so the control never shows blank
  over a month the date still carries).
- **Switching calendars converts the date.** The selector says which calendar
  a date was *recorded* in, so an existing entry is renumbered onto the new
  one rather than relabelled: 11 March 1796 becomes 21 ventôse an IV, not
  11 frimaire 1796 (a different day, three years later). Both ends of an
  `Or` / `Between` range move together. A date the target calendar cannot
  express — anything before 22 September 1792 for the Republican one — is
  left exactly as typed.
- **Validation follows the calendar** — month count and month length come from
  the calendar itself, so 6 complémentaires is a date only in a sextile year
  and Adar II only in a leap one. The plausible-year window is counted in each
  calendar's own era too (Hebrew 1–6800, Republican 1–1210, both framing the
  same span as the Gregorian −9999…2999): judged on the Gregorian numbers, a
  Hebrew year would be rejected as a typo for every date that calendar can
  express.

Conversion lives in `oxidgene_core::calendar` (every calendar mapped onto the
Julian Day Number), not in `oxidgene-gedcom`: the editor runs in WASM, where
`ged_io` — which the server uses to derive `date_sort` — cannot be reached.
Both are pinned to the same documented Republican dates so they agree.

**Witnesses** — dynamic list of text inputs (free text, one per witness). An **"+ Add a witness"** button appends a new entry.

### Death supplements

Same structure as Birth supplements: calendar selector + witnesses dynamic list.

---

## 9. Section: Other Events

Located at the bottom of the scrollable body, below the additional fields section.

An **"+ Add an event"** button opens a small inline picker listing available event types. Selecting a type appends a new event block at the bottom of this section.

### Available event types

Event types are organized by category. Types marked with **⟷** have a direct GEDCOM tag mapping (lossless round-trip via `ged_io`). Types without the marker are app-specific and export as GEDCOM `EVEN` with a TYPE subrecord.

**Sacraments & religious**
- Baptism ⟷ `BAPM`
- Confirmation
- First communion
- Bar/Bat Mitzvah

**Civil & life**
- Census ⟷ `CENS`
- Residence ⟷ `RESI`
- Naturalization ⟷ `NATU`
- Emigration ⟷ `EMIG`
- Immigration ⟷ `IMMI`
- Graduation ⟷ `GRAD`
- Occupation ⟷ `OCCU` (also editable from civil status section)
- Retirement ⟷ `RETI`
- Military service

**Death-related**
- Burial ⟷ `BURI`
- Cremation ⟷ `CREM`
- Probate ⟷ `PROB`
- Will ⟷ `WILL`

**Family** (also available as union events in the [couple edit modal](#14-couple-edit-modal))
- Engagement ⟷ `ENGA`
- Divorce / Separation ⟷ `DIV`
- Adoption

**Other**
- Custom event (free label) → exports as GEDCOM `EVEN` with TYPE

### Event block structure

Each added event appears as a collapsible block with:

- **Event type label** as block title (with remove button `×` top-right)
- **Date** — same date qualifier + field(s) as birth/death
- **Place** — text input with autocomplete
- **Description** — free text input, the event's own `description` field
- **Notes** — multi-line textarea, persisted as a `Note` carrying `event_id`
- **Source** — free-text input (see §4 — Source), persisted as a `Citation` carrying `event_id`
- **Cause** — single-line text input, free text. Relevant for death, burial, and other events where a cause is meaningful. Maps to GEDCOM `CAUS` tag.
- **Calendar** (supplementary, collapsed by default) — same calendar selector
- **Witnesses** (supplementary, collapsed by default) — same dynamic list

Blocks can be reordered via drag handle. They are collapsed by default after creation, showing only the event type label and its date summary.

Once saved, an event's row carries a **"Notes & source"** toggle that expands a panel with those two fields and its own Save button — the surrounding lists have no footer of their own. Only one row is open at a time, and the panel is mounted only while open, so a long event list costs nothing until one is expanded. The same toggle appears on each Occupation row in the Civil Status section.

---

## 10. Section: Media

Located after the Other Events section. Accessible directly within the modal — no separate modal or action picker entry required.

### Layout

A media gallery grid showing all media attached to this person, followed by an upload zone.

```
── Media ─────────────────────────────────────────────────

  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐
  │  [img]  ★│  │  [img]   │  │  [pdf]   │  │    +     │
  │          │  │          │  │          │  │  Upload  │
  └──────────┘  └──────────┘  └──────────┘  └──────────┘
  Portrait      Wedding        Baptism cert.
  (profile)     1865           1842

```

The ★ badge marks the current **profile image** (used to illustrate the person's card in the tree).

The shared `MediaGallery` component renders this section. A multi-page document
shows its page count and opens in the media viewer. Date and place metadata are
displayed when present but are not edited in this panel.

### Upload Zone

The last cell in the grid is always the upload trigger. Clicking it opens the
system file picker; dropping files onto it does the same thing without the
dialog. Accepted formats are decided by the file's magic bytes, not by its
extension: JPEG, PNG, GIF, BMP, TIFF, WebP, ICO and PDF. Multiple files can be
sent at once, and they upload **sequentially** — a folder of scans reads as
"3 of 12" rather than as twelve simultaneous stalls — with one rejected file
naming itself and the rest of the batch continuing.

### Media Item — Display

Each media item is shown as a thumbnail tile:
- **Images**: rendered as a cropped square thumbnail
- **PDFs / documents**: generic icon with file type label (e.g. "PDF"). The tile
  branches on the server saying it has no thumbnail — a `404` from
  `.../thumbnail` — rather than on a list of formats, so a format we later learn
  to rasterise needs no change here
- **Profile badge** ★: orange star overlay on the active profile image tile
- **Page badge**: a multi-page document shows its page count in the corner

Hovering a tile reveals its controls: ★ (set as profile image, images only),
✂ (crop a region), ✎ (edit), ↗ (open the file) and 🗑 (remove from this record).
They also appear on keyboard focus, and are always visible on narrow screens —
a control revealed only on hover is unreachable by touch.

The trash **detaches**, it does not delete: the file may document three other
people, and "not this person's" is what the button on a person's tile can
honestly mean.

### Media Item — Edit Panel

Clicking the edit button on a tile expands an inline edit panel below the tile (or opens a small overlay on mobile). Fields:

**For all media types:**

| Field | Type | Notes |
|---|---|---|
| Title | Text input | Short descriptive label |
| Description | Textarea | Free text, notes about the media |
| Date | Date qualifier + field | Same qualifier options as birth/death (exact, circa, between…) |
| Place | Text input with autocomplete | Location where the media was created or applies to |
| Link to event | Dropdown | List of this person's events; select one to associate the media |
| Use as source for | Dropdown | List of this person's events; marks this media as a source document for the selected event |

**For images only:**

| Field | Type | Notes |
|---|---|---|
| Set as profile image | ★ button on the tile | Marks this image as the person's profile photo; clears the ★ from the previous one in the same statement, so two stars are never briefly visible. Only a person may have one — a couple's card shows its spouses' portraits |
| Crop a region | ✂ button on the tile | Opens the cropper: drag a rectangle and optionally say which event it documents. Regions already cropped on the file are drawn while you draw the next one. Coordinates are stored in the source image's own pixels, so a better scan can replace this one without orphaning them |

**For PDFs / documents:**

| Field | Type | Notes |
|---|---|---|
| View / Download | Button | Opens the file in a new tab or triggers download |

### Profile Image Selection

Only one image per person can be the profile image at a time. Setting a new one automatically unsets the previous. The profile image is used:
- As the photo on the person card in the tree
- As the avatar in the events sidebar
- As the thumbnail in search results

If no profile image is set, the card falls back to the gendered silhouette placeholder.

### Remove Media

Clicking the trash icon on a tile shows a confirmation prompt inline ("Remove this media?") with Confirm / Cancel. Removal is not applied until the modal is saved.

This control only **detaches** the current person or family link. A shared file
may document other records, so the edit form never deletes it globally.

On a person or family profile, right-clicking a tile offers **Delete media**.
After confirmation it is permanently deleted only when the current gallery
link is its only reference; otherwise it remains untouched and the reader is
told that it is referenced elsewhere. The viewer also has a red **Delete
media** button beside its details controls. After confirmation, that action
permanently deletes the media, its files and all associated data (links, tags,
vignettes, portraits and document pages), even when it is referenced elsewhere.

---

## 11. Deleting a Person (edit mode only)

Not shown in create mode.

A **"Delete this person"** button is available at the bottom of the modal body, visually separated from the rest of the form by a divider. It uses a destructive style (red text, subtle red border).

### Confirmation flow

Clicking the button does not delete immediately. A confirmation prompt appears inline within the modal:

```
┌─────────────────────────────────────────────────┐
│  Delete <person A>?                             │
│                                                 │
│  This will permanently remove this person and   │
│  all their events and media. Their connections  │
│  to other persons (parents, children, spouses)  │
│  will also be removed.                          │
│                                                 │
│  [Cancel]              [Confirm deletion]       │
└─────────────────────────────────────────────────┘
```

On confirmation: the modal closes, the card is removed from the tree, and the layout is recalculated. If the deleted person was the current focus, the focus shifts to the nearest connected person.

---

## 12. Suggest Existing Persons (create mode only)

When the [tree setting](ui-settings.md) "Suggest existing persons" is enabled (§10) and the modal is in create mode, the modal offers to **link to an existing person** instead of creating a new one.

### Behavior

As the user types in the surname and first name fields, a suggestion dropdown appears below the form header:

```
┌─────────────────────────────────────────────────┐
│  💡 Existing persons matching this name:        │
│                                                  │
│  [photo] <person B>          ✦ 1845  ✝ 1920    │
│          Already in this tree, no family link    │
│          [Link this person]                      │
│                                                  │
│  [photo] <person C>          ✦ 1850             │
│          Already in this tree, no family link    │
│          [Link this person]                      │
│                                                  │
│  Or continue creating a new person below.        │
└─────────────────────────────────────────────────┘
```

- Suggestions are debounced (300ms) and filtered by name similarity
- Only persons **not already linked** in the target relationship are shown (e.g. when adding a child, persons already children of this union are excluded)
- Clicking **"Link this person"** links the existing person and closes the modal (no new person is created)
- The suggestion panel can be dismissed and does not block the form

---

## 13. Validation & Save Behavior

- No field is strictly required — a person can be saved with only a name, or even completely empty
- The **Create / Save** button is always active
- On save:
  1. The person is created (create mode) or updated (edit mode) via the API
  2. In create mode: the relationship link is created (FamilySpouse, FamilyChild, etc.) if applicable
  3. The modal closes
  4. The tree layout is recalculated
  5. In create mode: the new person becomes the selected focus in the tree
- On cancel or outside click with unsaved changes: a small confirmation prompt appears ("Discard changes?") with Confirm / Go back options

---

## 14. Keyboard & Accessibility

| Key | Behavior |
|---|---|
| `Escape` | Close modal (with discard prompt if unsaved changes) |
| `Tab` | Move focus between fields in document order |
| `Enter` in a text input | Move to the next field (does not submit) |
| `Enter` in the footer | Triggers Create / Save |

---

## 15. Responsive

- Below **600px**: modal becomes full-screen drawer (slides up from bottom)
- Union details section (for "Add spouse" in create mode) is initially collapsed on mobile

---

## 16. Couple Edit Modal

### Overview

The couple edit modal opens when the user selects a union from the **"Edit union"** flow in the action picker. It allows editing both persons of a couple simultaneously, along with the union's own data, in a single save operation.

### Container

Same dimensions and behavior as the person edit modal: centered overlay, ~720px wide, max-height 90vh, internally scrollable, fixed header and footer.

### Fixed Header

- Title: both persons' names separated by ` & `, for example `<person A> & <person B>`
- Subtitle: "Edit union"
- Close button `×` — closes without saving, prompts confirmation if unsaved changes

### Body Structure

The scrollable body is divided into three blocks:

```
┌─────────────────────────────────────────────────┐  ← fixed header
│  <person A> & <person B>                     [×] │
│  Edit union                                     │
├─────────────────────────────────────────────────┤
│                                                 │
│  ── Union ──────────────────────────────────    │  ← union block
│  Events / Date / Place / Note / Source          │
│                                                 │
│  ── Children ───────────────────────────────    │  ← children block
│  [child list with detach option]                │
│                                                 │
│  ── Person 1: <person A> ────────────────────   │  ← person 1 block
│  (same fields as individual edit modal)         │
│                                                 │
│  ── Person 2: <person B> ────────────────────   │  ← person 2 block
│  (same fields as individual edit modal)         │
│                                                 │
├─────────────────────────────────────────────────┤  ← fixed footer
│  [Delete couple]          [Cancel]  [Save]      │
└─────────────────────────────────────────────────┘
```

### Union Block

Displayed first, before the children block and either person's fields.

**Union events** — dynamic list of event blocks. Each event has the same structure as the "Other events" blocks in the individual edit modal (date qualifier + place + note + source + optional calendar + optional witnesses).

**Core union event types** (always available). Types marked with **⟷** have a direct GEDCOM tag mapping via `ged_io`:

- Marriage ⟷ `MARR`
- Divorce / Separation ⟷ `DIV`
- Annulment ⟷ `ANUL`
- Engagement ⟷ `ENGA`
- Marriage Bann ⟷ `MARB`
- Marriage Contract ⟷ `MARC`
- Marriage License ⟷ `MARL`
- Marriage Settlement ⟷ `MARS`

**Optional event types** (same pool as individual events, applicable to the couple context):

- Residence / Domicile ⟷ `RESI`
- Census ⟷ `CENS`
- Emigration / Immigration ⟷ `EMIG` / `IMMI`
- Will / Probate ⟷ `WILL` / `PROB`
- Custom event (free label)

An **"+ Add a union event"** button appends a new event block. Each block is collapsible after creation, showing only the event type and date summary when collapsed.

**Date** — shorthand date field for the main union date (separate from the events list, used for display in the tree and sidebar). Same date qualifier selector as birth/death.

**Place** — text input with autocomplete.

**Note** — free text textarea.

**Source** — free text input.

### Children Block

Displayed between the union block and the person blocks. Lists all children currently linked to this union.

Each child is shown as a single row:

```
[avatar] <person C>   ✦ 1868   ✝ 1942     [Detach]
[avatar] <person D>   ✦ 1871              [Detach]
[avatar] <person E>   ✦ 1875   ✝ 1875     [Detach]
```

**Detach button** — removes the parent→child link between this couple and that specific child, one at a time. The child person is not deleted — they remain in the tree but are no longer linked to this union. A confirmation prompt appears inline before detaching:

```
Detach <person C> from this union?
This will remove the parent link. <person C> will remain in the tree.
[Cancel]   [Confirm]
```

Detach operations are staged: they are not applied until the modal is saved.

If the union has no children, the block shows a muted "No children linked to this union." message.

### Person 1 & Person 2 Blocks

Each person block contains exactly the same fields as the individual edit modal (civil status, birth, death, privacy, supplementary fields, other events, media), collapsed into a clearly labeled section divider showing the person's name.

Each block is **independently expandable/collapsible** via a toggle on the section divider. Collapsed by default; the union block and children block are always expanded.

### Footer

- **Delete couple** — destructive action on the far left, red style (see below)
- **Cancel** — closes without saving
- **Save** — saves all changes across the union block, children detachments, and both person blocks in a single operation

### Deleting a Couple

The **"Delete couple"** button in the footer removes the union relationship between the two persons. It does **not** delete either person from the tree — only the union link is removed.

Confirmation prompt appears inline:

```
┌─────────────────────────────────────────────────┐
│  Delete this union?                             │
│                                                 │
│  The union between <person A> and <person B>     │
│  will be permanently removed, along with all    │
│  its events. Both persons will remain in the    │
│  tree. Their children will no longer be linked  │
│  to this union.                                 │
│                                                 │
│  [Cancel]              [Confirm deletion]       │
└─────────────────────────────────────────────────┘
```

### Validation

Same rules as the individual modal: no field is required, save is always available. On save, both person cards and all connectors in the tree update simultaneously.

---

## 17. Relationship to Other Flows

This spec covers **"Edit individual"**, **"Create person"** (all context variants), and **"Edit union"**. The other actions from the action picker are covered in their own specs:

- **Merge with…** → see [Person Merge](ui-merge.md)
