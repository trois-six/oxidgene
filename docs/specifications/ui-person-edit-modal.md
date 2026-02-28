# Visual & Functional Specifications — Person Edit Modal

> Part of the [OxidGene Specifications](README.md).
> See also: [Tree View](ui-genealogy-tree.md) (action picker opens this modal) · [Settings](ui-settings.md) (privacy rules, date display, entry options) · [Data Model](data-model.md) (Person, PersonName, Event, Family, Media) · [API Contract](api.md) (Persons, Events, Media endpoints)

---

## 1. Overview

The person edit modal opens when the user selects **"Edit individual"** from the action picker (pencil icon below a selected card in the [Tree View](ui-genealogy-tree.md)). It is a single large scrollable modal that covers all editable data for one person: civil status, birth, death, privacy, optional supplementary fields, and additional events.

---

## 2. Modal Structure

### Container

- **Type**: centered overlay modal, not a drawer
- **Size**: fixed width ~720px, max-height 90vh, internally scrollable
- **Backdrop**: dark semi-transparent blur overlay; click outside closes without saving
- **Scroll**: the modal body scrolls independently; the header and footer remain fixed

```
┌─────────────────────────────────────────────────┐  ← fixed header
│  MARTIN Jean-Baptiste            [×]            │
│  Edit individual                                │
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
├─────────────────────────────────────────────────┤  ← fixed footer
│  [Cancel]                       [Save]          │
└─────────────────────────────────────────────────┘
```

### Fixed Header

- Person's current full name (family name + first name) as title
- Subtitle: "Edit individual"
- Close button `×` top-right — closes without saving, prompts confirmation if there are unsaved changes

### Fixed Footer

- **Cancel** button (ghost style) — closes without saving
- **Save** button (orange gradient) — validates and saves all changes

---

## 3. Section: Civil Status

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

### Note on this person

Multi-line textarea, free text. No character limit. Placeholder: "Personal notes, anecdotes, research notes…"

### Source

Single text input, free text. Placeholder: "Reference, archive, document title…"

---

## 4. Section: Birth

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

Single text input with **place autocomplete** (geolocated suggestions, if enabled in tree settings). Placeholder: "Town, département, country…"

### Note

Single-line text input, free text.

### Source

Single-line text input, free text.

---

## 5. Section: Death

Identical structure to the Birth section. Same date qualifier options, same place/note/source fields.

Section divider label: "Death".

---

## 6. Section: Privacy

A single selector displayed below the Death section.

**Options** (radio group):

| Value | Label | Description |
|---|---|---|
| `default` | Default | Follows the tree-level privacy settings |
| `public` | Public | Always visible regardless of tree settings |
| `private` | Private | Always hidden regardless of tree settings |

---

## 7. Section: Additional Fields

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

**Witnesses** — dynamic list of text inputs (free text, one per witness). An **"+ Add a witness"** button appends a new entry.

### Death supplements

Same structure as Birth supplements: calendar selector + witnesses dynamic list.

---

## 8. Section: Other Events

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
- **Note** — free text input
- **Source** — free text input
- **Cause** — single-line text input, free text. Relevant for death, burial, and other events where a cause is meaningful. Maps to GEDCOM `CAUS` tag.
- **Calendar** (supplementary, collapsed by default) — same calendar selector
- **Witnesses** (supplementary, collapsed by default) — same dynamic list

Blocks can be reordered via drag handle. They are collapsed by default after creation, showing only the event type label and its date summary.

---

## 9. Validation & Save Behavior

- No field is strictly required — a person can be saved with only a name, or even completely empty
- The **Save** button is always active
- On save: the modal closes, the tree card updates immediately to reflect the new name and dates
- On cancel or outside click with unsaved changes: a small confirmation prompt appears ("Discard changes?") with Confirm / Go back options

---

## 10. Keyboard & Accessibility

| Key | Behavior |
|---|---|
| `Escape` | Close modal (with discard prompt if unsaved changes) |
| `Tab` | Move focus between fields in document order |
| `Enter` in a text input | Move to the next field (does not submit) |
| `Enter` in the footer | Triggers Save |

---

## 10. Section: Media

Located at the bottom of the scrollable body, after the Other Events section. Accessible directly within the individual edit modal — no separate modal or action picker entry required.

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

### Upload Zone

The last cell in the grid is always the upload trigger. Clicking it or dragging files onto it opens the system file picker. Accepted formats: JPEG, PNG, WebP, GIF, PDF, and common document formats. Multiple files can be uploaded at once.

### Media Item — Display

Each media item is shown as a thumbnail tile:
- **Images**: rendered as a cropped square thumbnail
- **PDFs / documents**: generic icon with file type label (e.g. "PDF")
- **Profile badge** ★: orange star overlay on the active profile image tile

Hovering a tile reveals two icon buttons: **Edit** (pencil) and **Remove** (trash).

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
| Set as profile image | Toggle / button | Marks this image as the person's profile photo; removes the ★ from the previous one |

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

---

## 11. Deleting a Person

A **"Delete this person"** button is available at the bottom of the individual edit modal body, visually separated from the rest of the form by a divider. It uses a destructive style (red text, subtle red border).

### Confirmation flow

Clicking the button does not delete immediately. A confirmation prompt appears inline within the modal:

```
┌─────────────────────────────────────────────────┐
│  Delete MARTIN Jean-Baptiste?                   │
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

## 12. Validation & Save Behavior

- No field is strictly required — a person can be saved with only a name, or even completely empty
- The **Save** button is always active
- On save: the modal closes, the tree card updates immediately to reflect the new name, dates, and profile image
- On cancel or outside click with unsaved changes: a small confirmation prompt appears ("Discard changes?") with Confirm / Go back options

---

## 13. Keyboard & Accessibility

| Key | Behavior |
|---|---|
| `Escape` | Close modal (with discard prompt if unsaved changes) |
| `Tab` | Move focus between fields in document order |
| `Enter` in a text input | Move to the next field (does not submit) |
| `Enter` in the footer | Triggers Save |

---

## 14. Couple Edit Modal

### Overview

The couple edit modal opens when the user selects a union from the **"Edit union"** flow in the action picker. It allows editing both persons of a couple simultaneously, along with the union's own data, in a single save operation.

### Container

Same dimensions and behavior as the person edit modal: centered overlay, ~720px wide, max-height 90vh, internally scrollable, fixed header and footer.

### Fixed Header

- Title: both persons' names separated by " & " — e.g. **"MARTIN Jean-Baptiste & LEMAIRE Marguerite"**
- Subtitle: "Edit union"
- Close button `×` — closes without saving, prompts confirmation if unsaved changes

### Body Structure

The scrollable body is divided into three blocks:

```
┌─────────────────────────────────────────────────┐  ← fixed header
│  MARTIN Jean-Baptiste & LEMAIRE Marguerite  [×] │
│  Edit union                                     │
├─────────────────────────────────────────────────┤
│                                                 │
│  ── Union ──────────────────────────────────    │  ← union block
│  Events / Date / Place / Note / Source          │
│                                                 │
│  ── Children ───────────────────────────────    │  ← children block
│  [child list with detach option]                │
│                                                 │
│  ── Person 1: MARTIN Jean-Baptiste ──────────   │  ← person 1 block
│  (same fields as individual edit modal)         │
│                                                 │
│  ── Person 2: LEMAIRE Marguerite ─────────────  │  ← person 2 block
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
[avatar] MARTIN Henri   ✦ 1868   ✝ 1942     [Detach]
[avatar] MARTIN Louise  ✦ 1871              [Detach]
[avatar] MARTIN Pierre  ✦ 1875   ✝ 1875     [Detach]
```

**Detach button** — removes the parent→child link between this couple and that specific child, one at a time. The child person is not deleted — they remain in the tree but are no longer linked to this union. A confirmation prompt appears inline before detaching:

```
Detach MARTIN Henri from this union?
This will remove the parent link. Henri will remain in the tree.
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
│  The union between Jean-Baptiste and Marguerite │
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

## 15. Relationship to Other Modals

This spec covers **"Edit individual"** and **"Edit union"**. The other actions from the pencil icon picker are out of scope here and covered in their own specs:

- **Merge with…** → see [Person Merge](ui-merge.md)
- **Add spouse / Add child / Add sibling** → see [Add Person](ui-add-person.md)
