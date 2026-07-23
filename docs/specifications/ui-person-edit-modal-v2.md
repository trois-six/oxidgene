---
type: "UI Specification"
title: "Person Edit Modal V2 — Complete Redesign"
description: "Comprehensive redesign of the person edit modal: civil status, events, media, and couple management."
tags: [oxidgene, specification, ui, ux, person-editing]
timestamp: 2026-07-19T00:00:00Z
---

# Person Edit Modal V2 — Complete Redesign

> Part of the [OxidGene Specifications](index.md).
> See also: [Tree View](ui-genealogy-tree.md) · [Person Detail](person_detail.md) · [Error Handling](error-handling.md)

---

## 1. Overview

This specification completely redesigns the Person Edit Modal to provide a superior UX for editing genealogical data. The modal serves two contexts:

1. **Person Create** (from tree view or person list)
2. **Person Edit** (from person card or person detail page)

The new design prioritizes:
- Clear, scannable form structure (grouped sections)
- Staged edits (form data held locally until save)
- Inline validation with clear error messages
- Keyboard shortcuts (Cmd+S to save, Esc to cancel)
- Media attachment (placeholder for Sprint F integration)
- Couple editing workflow (launch union form from this modal)

---

## 2. Modal Structure

### 2.1 Header

```
╭─ [X close]                              [? help]  [⚙️ more actions] ─╮
│  Creating a new person                                               │
├─────────────────────────────────────────────────────────────────────┤
│ [...tabs: Person | Couple | Events | Media]                        │
│ [...scroll content]                                                  │
├─────────────────────────────────────────────────────────────────────┤
│                                [Cancel]  [Save]  [Save & Create New] │
╰─────────────────────────────────────────────────────────────────────╯
```

**Components:**
- Close button (X) — top right, close without saving (confirm if unsaved)
- Title — "Creating a new person" or "Editing [First Name] [Surname]"
- Help icon (?) — opens a tooltip explaining the form
- More actions (⋮) — dropdown with "Save & Create New", "Duplicate", "Delete" (if editing)
- Tab bar — Person | Couple | Events | Media
- Footer actions: Cancel, Save, Save & Create New (conditional)

### 2.2 Tab System

Organized into 4 tabs to avoid overwhelming the user with fields:

| Tab | Purpose | Fields |
|-----|---------|--------|
| **Person** | Core identity | Names, gender, civil status, birth, death |
| **Couple** | Spousal data | Marriage/union events, both spouses' info |
| **Events** | Life events | Birth, death, occupation, education, etc. |
| **Media** | Attachments | Photos, documents, vignettes (Sprint F) |

**Behavior:**
- Tabs are visible only if relevant data exists (Media tab hidden until Sprint F)
- Unsaved indicator on tab (e.g., "Person *" if tab has changes)
- Switching tabs auto-saves current tab's data (or confirm if unsaved)

---

## 3. Person Tab — Core Identity

### 3.1 Section: Names

```
NAMES
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

Full Name (Surname, Given Names)
[____________________] [____________________]
 ^Surname              ^Given Names

Alternate Names (nicknames, maiden names, etc.)
+ Add another name
  [____________________] [____________________]
   ^Surname              ^Given Names
```

**Behavior:**
- First name is the "Full Name" (required)
- "Alternate Names" are optional
- Each name row has surname + given names (two separate fields)
- "Add another name" button to add more names
- Remove button (X) on each alternate name row

**Validation:**
- First name (surname + given) required
- Error below field: "Surname is required" (red text, red border)

### 3.2 Section: Gender

```
GENDER
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

( ) Male    ( ) Female    ( ) Unknown    ( ) Other
```

**Behavior:**
- Radio buttons, single-select
- Optional field (default: Unknown)

### 3.3 Section: Civil Status & Residence

```
CIVIL STATUS
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

Status: [Dropdown: Single | Married | Divorced | Widowed | Unknown]
Residence: [text input]
Nationality: [text input]
Religion: [text input]

Notes: [textarea, 3 lines]
```

**Behavior:**
- Dropdown for civil status (controlled via Privacy enum)
- Free-text fields for residence, nationality, religion
- Multi-line notes field at bottom

### 3.4 Section: Birth

```
BIRTH
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

Date: [dd/mm/yyyy ________] [Calendar icon]

Qualifier: [Dropdown: Certain | About | Before | After | From | To]
  ^Allows "About 1842" or "Before 1850"

Place: [Place autocomplete ________]
           ^Async lookup from places table
```

**Behavior:**
- Date input with calendar picker (Dioxus native or Web component)
- Date qualifier dropdown (how certain is the date?)
- Place lookup (async autocomplete with recent places shown first)
- Optional fields (all nullable on DB level)

**Validation:**
- If date provided, must be valid format
- If qualifier is "Between", require two dates (not shown here for brevity)

### 3.5 Section: Death

```
DEATH
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

Date: [dd/mm/yyyy ________] [Calendar icon]

Qualifier: [Dropdown: Certain | About | Before | After]

Place: [Place autocomplete ________]

Age at Death: [Auto-calculated from birth date]
              ^Read-only, computed field (age = death year - birth year)
```

**Behavior:**
- Same structure as Birth
- Age at death is auto-calculated (read-only)

---

## 4. Couple Tab — Spousal & Union Data

This tab is for managing union information (marriage, spouses, children). It launches into a **separate modal** if "Edit Union" is clicked.

```
COUPLE / UNIONS
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

Unions of [Full Name]

Union 1: [Spouse Name] (Married 1950)
  [Edit]  [Remove from union]

Union 2: [Spouse Name] (Divorced 1990)
  [Edit]  [Remove from union]

+ Add new union
```

**Behavior:**
- List all unions for this person (marriages, common-law, etc.)
- Each union row shows: spouse name, union date (if available)
- [Edit] button opens Union Form Modal (separate spec)
- [Remove from union] removes the link (doesn't delete the spouse)
- "+ Add new union" launches Union Form Modal in "create" mode

**Union Form Modal** (launched from here, separate spec: `ui-union-form.md`):
- Create or edit a union
- Manage both spouses' info
- Manage children
- Union events (marriage, divorce, etc.)

---

## 5. Events Tab — Life Events

```
EVENTS
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

Life events for [Full Name]

Event 1: Occupation (1920–1980)
  Laborer (French: Journalier)
  [Edit]  [Remove]

Event 2: Education (1940–1945)
  Primary School, Maine, France
  [Edit]  [Remove]

Event 3: Residence (1950)
  Paris, France
  [Edit]  [Remove]

+ Add event
```

**Behavior:**
- List all events for this person
- Each event shows: type, date range (or single date), brief description
- [Edit] opens event inline editor or modal (TBD: inline vs. modal)
- [Remove] deletes the event (with confirmation)
- "+ Add event" launches event form

**Event Form** (could be inline or modal, TBD):
- Event type dropdown (Birth, Death, Occupation, Education, Residence, Marriage, Divorce, etc.)
- Date (start + end if range-based event)
- Place lookup
- Description / Details text
- Witnesses (optional, Sprint 2+)
- Source citations (optional, Sprint 2+)

---

## 6. Media Tab — Attachments (Placeholder for Sprint F)

```
MEDIA / ATTACHMENTS
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

Photos & Documents for [Full Name]

[Placeholder: "Media management coming in Sprint F.1"]

+ Upload media (when Sprint F is ready)
```

**Behavior (Sprint F.1):**
- Display uploaded media (photos, documents)
- Upload button
- Thumbnail gallery
- Vignette cropping UI (if implemented)
- Evidence linking (media supports which events?)

---

## 7. Form Behavior & State Management

### 7.1 Staged Edits

All form changes are **staged locally** — not saved to DB until [Save] is clicked.

- User types in fields → only update component state
- Switching tabs → auto-save current tab (or confirm if unsaved)
- [Cancel] → discard all changes, close modal
- [Save] → validate all tabs, submit to API, close modal

### 7.2 Validation & Errors

**Real-time validation** (as user types):
- Name fields: required (show error below field if empty and blurred)
- Date fields: must be valid format (dd/mm/yyyy)
- Place lookup: async validation (does place exist?)

**On save**:
- Validate all tabs
- Show first error in a red banner at top of modal
- Highlight invalid field(s) with red border
- Do NOT close modal until valid

**Error format**:
```
─ Error
│
│ Surname is required (on Person tab)
│ [Person tab highlight in red]
└─
```

### 7.3 Keyboard Shortcuts

- **Cmd+S** (Mac) / **Ctrl+S** (Windows/Linux) → Save (if form valid)
- **Esc** → Cancel (confirm if unsaved changes)
- **Tab** → Move to next field
- **Shift+Tab** → Move to previous field
- **Enter** (in textarea) → New line (not submit)

### 7.4 Unsaved Indicator

- Close button (X) shows alert if unsaved: "You have unsaved changes. Close without saving?"
- Tab indicator shows "*" if tab has unsaved changes
- Modal title may show "(modified)" or similar

---

## 8. Footer Actions

### 8.1 Cancel

- Discard all changes
- Close modal
- Confirm if unsaved changes: "You have unsaved changes. Discard them?"

### 8.2 Save

- Validate all tabs
- POST/PUT to API: `/persons` (create) or `/persons/{id}` (update)
- On success: close modal, refresh tree/person-list
- On error: show error banner, keep modal open

### 8.3 Save & Create New

- Save current person
- Reset form to empty state (create mode)
- User can quickly create multiple persons

### 8.4 More Actions (⋮)

Available only in edit mode (not create):

- **Save & Create New** (also available as footer button)
- **Duplicate Person** → Open new modal with copy of current person's data
- **Delete Person** → Soft delete (moved to deleted_at), confirm action

---

## 9. Responsive Design

### Desktop (>= 900px)

- Modal width: 600px
- All fields in 1 column
- No wrapping

### Tablet (600–899px)

- Modal width: 500px
- Smaller padding, tighter spacing

### Mobile (< 600px)

- Full-screen modal (no close button visible initially, swipe-down to close)
- Names section: single-column
- Tabs: horizontal scrollable, or stacked vertical
- Buttons: full-width at bottom

---

## 10. Accessibility

- All form fields have associated `<label>` elements
- Error messages linked to fields via `aria-describedby`
- Modal has `role="dialog"` and `aria-modal="true"`
- Focus trap: keyboard navigation stays within modal
- Close button (X) is keyboard accessible
- Keyboard shortcuts documented (? help icon)

---

## 11. Future Enhancements (Post-Sprint 1)

### Sprint 2 (Media Management)
- [ ] Media upload UI in Media tab
- [ ] Image cropper for vignettes
- [ ] Media gallery with thumbnails

### Sprint 3+ (Events & Witnesses)
- [ ] Inline event editor (currently TBD)
- [ ] Event witnesses selection
- [ ] Source citation linking

### Sprint F (Security)
- [ ] Access control checks (only editors can save)
- [ ] Audit logging (who edited what, when)

---

## 12. Comparison: V1 → V2

| Aspect | V1 | V2 |
|--------|-----|-----|
| Layout | Single long form | Tabbed structure |
| Names | Single name field | Surname + Given Names, alternate names |
| Events | Modal within modal | Tab, inline editor |
| Media | Placeholder | Tab, will be integrated in Sprint F |
| Keyboard | No shortcuts | Cmd+S, Esc, Tab navigation |
| Validation | After save | Real-time + on save |
| Error UX | Toast notifications | Inline + banner |
| Couple | Separate modal | Tab, launches union form |
| Staged edits | Not staged | Staged locally, save on confirm |
