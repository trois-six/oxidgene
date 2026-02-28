# Visual & Functional Specifications — Add Person Modal

> Part of the [OxidGene Specifications](README.md).
> See also: [Person Edit Modal](ui-person-edit-modal.md) (full edit form) · [Tree View](ui-genealogy-tree.md) (action picker triggers) · [Data Model](data-model.md) (Person, Family, FamilySpouse, FamilyChild)

---

## 1. Overview

The add person modal is a **lighter version** of the [Person Edit Modal](ui-person-edit-modal.md). It is used when creating a new person and simultaneously linking them to an existing person in the tree. It is triggered from:

- The [Tree View](ui-genealogy-tree.md) **action picker**: "Add spouse", "Add child", "Add sibling"
- The [Tree View](ui-genealogy-tree.md) **placeholder card** (`+` icon on unknown parents)
- The [Tree View](ui-genealogy-tree.md) **left sidebar**: "Add a person" button (＋👤) — standalone, no pre-linked relationship

---

## 2. Modal Structure

Same container as the [Person Edit Modal](ui-person-edit-modal.md): centered overlay, ~720px wide, max-height 90vh, scrollable body, fixed header and footer.

### Fixed Header

- **Title** varies by context (see §3)
- **Subtitle**: "New person"
- Close button `×` — closes without saving

### Fixed Footer

- **Cancel** button (ghost style)
- **Create** button (orange gradient) — replaces "Save" to clarify this creates a new person

---

## 3. Context Variants

The modal adapts its title, pre-filled fields, and relationship setup based on the trigger:

### Add Spouse

**Trigger**: "Add spouse" from the action picker on a selected person.

| Aspect | Behavior |
|---|---|
| Title | "Add spouse to MARTIN Jean-Baptiste" |
| Gender | Pre-selected to the opposite of the existing person (if Male → Female, and vice versa). Editable. |
| Relationship created on save | A new Family is created (or the existing one is used if the person has no union yet). The new person is added as a FamilySpouse. |
| Union section | A collapsed "Union details" section is available (date, place, note, source for the marriage). Same fields as the union block in the [couple edit modal](ui-person-edit-modal.md). |

### Add Child

**Trigger**: "Add child" from the action picker.

| Aspect | Behavior |
|---|---|
| Title | "Add child to MARTIN Jean-Baptiste & LEMAIRE Marguerite" (if the person has a union) or "Add child to MARTIN Jean-Baptiste" (if no union) |
| Surname | Pre-filled with the selected person's surname. Editable. |
| Gender | Not pre-selected. |
| Union selector | If the selected person has **multiple unions**, a dropdown at the top of the modal asks which union this child belongs to. |
| Relationship created on save | The new person is added as a FamilyChild to the selected union. |

### Add Sibling

**Trigger**: "Add sibling" from the action picker.

| Aspect | Behavior |
|---|---|
| Title | "Add sibling of MARTIN Jean-Baptiste" |
| Surname | Pre-filled with the selected person's surname. Editable. |
| Gender | Not pre-selected. |
| Relationship created on save | The new person is added as a FamilyChild to the **same Family** as the selected person (i.e. the Family where the selected person is a child). If the selected person has no parent family, a new Family is created with the selected person's parents (if known). |

### Add Parent (from placeholder)

**Trigger**: clicking the `+` on an unknown parent placeholder card at the top of the tree.

| Aspect | Behavior |
|---|---|
| Title | "Add father of MARTIN Jean-Baptiste" or "Add mother of …" (depending on the placeholder position) |
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

## 4. Form Fields

The add person modal contains a **subset** of the [Person Edit Modal](ui-person-edit-modal.md) fields. Only the most commonly needed fields are shown. The full edit modal can be used after creation for additional details.

### Always shown

| Section | Fields |
|---|---|
| **Civil status** | Surname · First name (single field, not dynamic list) · Gender |
| **Birth** | Date (with qualifier) · Place |
| **Death** | Date (with qualifier) · Place |

### Shown only for "Add spouse"

| Section | Fields |
|---|---|
| **Union details** | Date (with qualifier) · Place · Note |

### Not shown (available in full edit modal)

- Occupations, first name aliases, nickname
- Privacy selector
- Additional fields (calendar, witnesses)
- Other events
- Media
- Notes and sources
- Delete button

A subtle link at the bottom of the form reads: **"Need more fields? You can edit this person after creation."**

---

## 5. Suggest Existing Persons

When the [tree setting](ui-settings.md) "Suggest existing persons" is enabled (§10), the modal offers to **link to an existing person** instead of creating a new one.

### Behavior

As the user types in the surname and first name fields, a suggestion dropdown appears below the form header:

```
┌─────────────────────────────────────────────────┐
│  💡 Existing persons matching this name:        │
│                                                  │
│  [photo] LEMAIRE Marguerite  ✦ 1845  ✝ 1920    │
│          Already in this tree, no family link    │
│          [Link this person]                      │
│                                                  │
│  [photo] LEMAIRE Marie       ✦ 1850             │
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

## 6. Validation & Save

- **Surname or first name** is recommended but not strictly required
- The **Create** button is always active
- On save:
  1. The new person is created via the API
  2. The relationship link is created (FamilySpouse, FamilyChild, etc.)
  3. The modal closes
  4. The tree layout is recalculated to include the new person
  5. The new person becomes the selected focus in the tree

---

## 7. Keyboard & Accessibility

| Key | Behavior |
|---|---|
| `Escape` | Close modal (with discard prompt if fields are filled) |
| `Tab` | Move focus between fields |
| `Enter` in a text input | Move to the next field |
| `Enter` in the footer | Triggers Create |

---

## 8. Responsive

- Below **600px**: modal becomes full-screen drawer (slides up from bottom)
- Union details section (for "Add spouse") is initially collapsed on mobile
