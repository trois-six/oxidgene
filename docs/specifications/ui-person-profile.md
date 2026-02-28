# Visual & Functional Specifications — Person Profile

> Part of the [OxidGene Specifications](README.md).
> See also: [Tree View](ui-genealogy-tree.md) (profile button in left sidebar) · [Person Edit Modal](ui-person-edit-modal.md) · [Data Model](data-model.md) (Person, PersonName, Event, Family, Media) · [API Contract](api.md) (Persons, Events endpoints)

---

## 1. Overview

The person profile is a full-page detailed view of a single individual, replacing the tree canvas. It is accessed via the **profile icon** (👤) in the [Tree View](ui-genealogy-tree.md) left sidebar, and displays the currently selected person's complete information: identity, life events timeline, family connections, media gallery, notes, and sources.

A **back button** returns to the tree view, centered on the same person.

---

## 2. Layout

```
┌──────────────────────────────────────────────────────────────────────┐
│                        TOPBAR + BREADCRUMB                           │
├──────┬───────────────────────────────────────────────────────────────┤
│      │                                                               │
│  S   │  ┌─────────────────────────────────────────────────────────┐  │
│  I   │  │  IDENTITY HEADER                                        │  │
│  D   │  │  [photo]  Name · Dates · Gender                        │  │
│  E   │  │           [Edit] [View in tree]                        │  │
│  B   │  └─────────────────────────────────────────────────────────┘  │
│  A   │                                                               │
│  R   │  ┌──────────────────────┐  ┌──────────────────────────────┐  │
│      │  │  FAMILY CONNECTIONS  │  │  TIMELINE                     │  │
│      │  │  Parents             │  │  Events chronological list    │  │
│      │  │  Spouses & children  │  │                               │  │
│      │  │  Siblings            │  │                               │  │
│      │  └──────────────────────┘  └──────────────────────────────┘  │
│      │                                                               │
│      │  ┌──────────────────────┐  ┌──────────────────────────────┐  │
│      │  │  MEDIA               │  │  NOTES & SOURCES             │  │
│      │  └──────────────────────┘  └──────────────────────────────┘  │
│      │                                                               │
└──────┴───────────────────────────────────────────────────────────────┘
```

The left sidebar remains visible (same as in tree view). The content area scrolls vertically. Max-width: 1080px, centered within the available space.

---

## 3. Topbar

Same topbar as the [Tree View](ui-genealogy-tree.md). The breadcrumb extends to include the person:

```
My trees › Famille Martin — Bourgogne › MARTIN Jean-Baptiste
```

Each crumb is a clickable link. Clicking the tree name returns to the tree view.

---

## 4. Identity Header

Full-width card at the top of the content area.

### Layout

```
┌─────────────────────────────────────────────────────────────────┐
│  ┌──────────┐                                                   │
│  │          │  MARTIN                                           │
│  │  photo   │  Jean-Baptiste                                    │
│  │          │  ✦ 12/03/1842, Beaune  ·  ✝ 07/11/1918, Pommard  │
│  │          │  Male · 76 years old                              │
│  └──────────┘                                                   │
│                                                                  │
│  [✏ Edit]   [🌳 View in tree]   [⬇ Export]                     │
└─────────────────────────────────────────────────────────────────┘
```

**Photo**: 120×160px rectangle. If no profile image is set, the gendered silhouette placeholder is displayed.

**Name**: surname in uppercase (bold, Cinzel), first name(s) below. If the person has alternate names (married, maiden, alias), they are listed below the primary name in muted text.

**Dates**: birth and death with symbols (✦ / ✝), place names included. Calculated age displayed if both dates are known.

**Gender**: label + colored dot (blue male, pink female, grey unknown).

**Action buttons**:
- **Edit** — opens the [Person Edit Modal](ui-person-edit-modal.md)
- **View in tree** — returns to the tree view, centered on this person
- **Export** — downloads a mini GEDCOM of this person and their immediate family

---

## 5. Family Connections

Displayed as a card in the left column of the two-column layout.

### Parents

```
┌──────────────────────────────┐
│  PARENTS                     │
├──────────────────────────────┤
│  [avatar] MARTIN Pierre      │
│           ✦ 1810  ✝ 1878    │
│                              │
│  [avatar] DUBOIS Marie       │
│           ✦ 1815  ✝ 1890    │
└──────────────────────────────┘
```

Each parent is clickable — navigates to that person's profile. If a parent is unknown, a muted "Unknown father" / "Unknown mother" row is shown with a `+ Add` button.

### Spouses & Children

One sub-section per union, ordered chronologically by marriage date (if known).

```
┌──────────────────────────────────────┐
│  UNION WITH LEMAIRE Marguerite       │
│  💍 1865, Beaune                     │
├──────────────────────────────────────┤
│  Children:                           │
│  [avatar] MARTIN Henri    ✦ 1868     │
│  [avatar] MARTIN Louise   ✦ 1871     │
│  [avatar] MARTIN Pierre   ✦ 1875    │
└──────────────────────────────────────┘
```

Spouse and children names are clickable links. An **"Edit union"** link in the sub-header opens the [couple edit modal](ui-person-edit-modal.md).

### Siblings

Listed below the parents section, grouped by shared parents.

```
┌──────────────────────────────┐
│  SIBLINGS                    │
├──────────────────────────────┤
│  [avatar] MARTIN Jeanne      │
│           ✦ 1838  ✝ 1910    │
│  [avatar] MARTIN Louis       │
│           ✦ 1845  ✝ 1920    │
└──────────────────────────────┘
```

Each sibling is clickable. If no siblings are known, this section is hidden.

---

## 6. Timeline

Displayed as a card in the right column. A vertical chronological list of all events associated with this person (individual events + family events where this person is involved).

### Structure

```
┌─────────────────────────────────────────────┐
│  TIMELINE                                    │
├─────────────────────────────────────────────┤
│                                              │
│  1842  ✦  Birth                              │
│            Beaune, Côte-d'Or, France         │
│            📎 Acte de naissance n°42         │
│                                              │
│  1842  ✟  Baptism                            │
│            Église Notre-Dame, Beaune         │
│                                              │
│  1860  ⚒  Occupation: Vigneron              │
│                                              │
│  1865  💍 Marriage with Marguerite LEMAIRE   │
│            Mairie de Beaune                  │
│            Witnesses: Pierre DUVAL, ...      │
│                                              │
│  1918  ✝  Death                              │
│            Pommard, Côte-d'Or               │
│                                              │
└─────────────────────────────────────────────┘
```

Each event shows:
- **Year** on the left, bold
- **Icon** — same symbols as [Tree View](ui-genealogy-tree.md) events sidebar (✦ birth, ✟ baptism, ✝ death, ⚰ burial, 💍 marriage, ⚖ divorce, 🏡 residence, ⚒ occupation, 📜 source)
- **Event type** label, bold
- **Place** if known, in muted text
- **Source reference** if attached, with 📎 icon, clickable
- **Note excerpt** if present, truncated to 2 lines with "Show more" expansion

Events are ordered by `date_sort`. Events without dates are grouped at the bottom under a "Date unknown" label.

Clicking an event expands it inline to show full details (complete note, all sources, attached media thumbnails).

---

## 7. Media Gallery

Displayed as a full-width card below the two-column layout.

```
┌──────────────────────────────────────────────────────────────────┐
│  MEDIA (4)                                              [+ Add] │
├──────────────────────────────────────────────────────────────────┤
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐        │
│  │  [img]  ★│  │  [img]   │  │  [pdf]   │  │  [img]   │        │
│  │          │  │          │  │          │  │          │        │
│  └──────────┘  └──────────┘  └──────────┘  └──────────┘        │
│  Portrait      Wedding       Baptism cert.  Vineyard            │
│  (profile)     1865          1842           c. 1880             │
└──────────────────────────────────────────────────────────────────┘
```

Same thumbnail grid as the [Person Edit Modal](ui-person-edit-modal.md) media section, but read-only by default. The ★ badge marks the profile image. Clicking a thumbnail opens a **lightbox overlay** with the full-size image, title, description, and associated event.

The **"+ Add"** button opens the [Person Edit Modal](ui-person-edit-modal.md), scrolled to the media section.

---

## 8. Notes & Sources

Displayed as a card alongside or below the media gallery.

### Notes

All notes associated with this person, displayed as expandable blocks:

```
┌──────────────────────────────────────┐
│  NOTES (2)                           │
├──────────────────────────────────────┤
│  Personal notes, anecdotes…         │
│  (first 3 lines visible)            │
│  [Show more]                         │
│                                      │
│  Research notes on birth date…      │
│  (first 3 lines visible)            │
│  [Show more]                         │
└──────────────────────────────────────┘
```

### Sources

All citations linked to this person, grouped by source:

```
┌──────────────────────────────────────┐
│  SOURCES (3)                         │
├──────────────────────────────────────┤
│  📎 Archives départementales 21     │
│     Page: 3E 42/128, f. 12          │
│     Confidence: High                │
│                                      │
│  📎 Registre paroissial Beaune      │
│     Page: Baptêmes 1842, n°15       │
│     Confidence: High                │
└──────────────────────────────────────┘
```

Each citation shows the source title, page reference, confidence level, and extracted text if any.

---

## 9. Responsive

- Below **1080px**: the two-column layout (family connections + timeline) collapses to a single column, with family connections above timeline.
- Below **900px**: identity header photo shrinks to 80×106px. Action buttons become icon-only.
- The left sidebar remains fixed (same behavior as tree view).

---

## 10. Keyboard & Accessibility

| Key | Behavior |
|---|---|
| `Escape` | Returns to the tree view |
| `E` | Opens the edit modal for the current person |
| `←` / `→` | Navigate between persons (previous/next sibling or chronological order) |

---

## 11. Navigation Flow

```
Tree View (canvas)
  │
  ├─ Click person card → selected in tree (events sidebar updates)
  │
  └─ Click profile icon (👤) in left sidebar
       │
       └─ Person Profile (this page)
            │
            ├─ Click family member → navigates to their profile
            ├─ "View in tree" button → returns to tree view, centered on person
            ├─ "Edit" button → opens Person Edit Modal
            └─ Escape → returns to tree view
```
