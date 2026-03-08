# Visual & Functional Specifications — Person Profile

> Part of the [OxidGene Specifications](README.md).
> See also: [Tree View](ui-genealogy-tree.md) (profile button in left sidebar) · [Person Edit Modal](ui-person-edit-modal.md) · [Data Model](data-model.md) (Person, PersonName, Event, Family, Media) · [API Contract](api.md) (Persons, Events endpoints)

---

## 1. Overview

The person profile is a full-page detailed view of a single individual. It is accessed via the **profile icon** (person silhouette) in the [Tree View](ui-genealogy-tree.md) left sidebar, or by clicking a search result on the [Search Results](ui-search-results.md) page. It displays the currently selected person's complete information: identity, life events timeline, family connections, media gallery, notes, and sources.

---

## 2. Layout

Uses the standard `sub-page` layout pattern (see [General](general.md) section 8). There is **no left sidebar (ISB)** on this page — the content fills the full width within the `sub-page-content` container.

```
+----------------------------------------------------------------------+
| NAVBAR                                                                |
+----------------------------------------------------------------------+
| [logo] tree_name / MARTIN Jean-Baptiste                              |  <- td-topbar
+----------------------------------------------------------------------+
|                                                                       |
|   +-------------------------------------------------------------+    |
|   |  IDENTITY HEADER                                            |    |
|   |  [avatar]  Name - Dates - Gender                            |    |
|   |            [Edit] [View in tree]                            |    |
|   +-------------------------------------------------------------+    |
|                                                                       |
|   +----------------------+  +-----------------------------------+    |
|   |  FAMILY CONNECTIONS  |  |  TIMELINE                         |    |
|   |  Parents             |  |  Events chronological list        |    |
|   |  Spouses & children  |  |                                   |    |
|   |  Siblings            |  |                                   |    |
|   +----------------------+  +-----------------------------------+    |
|                                                                       |
|   +----------------------+  +-----------------------------------+    |
|   |  MEDIA               |  |  NOTES & SOURCES                  |    |
|   +----------------------+  +-----------------------------------+    |
|                                                                       |
+----------------------------------------------------------------------+
```

Content: `max-width: 1200px`, centered, scrollable.

---

## 3. Topbar

Uses the shared `td-topbar` + `td-bc` breadcrumb component:

```
[logo] tree_name / MARTIN Jean-Baptiste
```

- Logo icon links to the homepage
- Tree name (`.td-bc-link`) links to the tree view
- `/` separator (`.td-bc-sep`)
- Person name (`.td-bc-current`) — not clickable

---

## 4. Identity Header

Full-width card at the top of the content area.

### Layout

```
+-------------------------------------------------------------+
|  +----------+                                                |
|  |          |  MARTIN                                        |
|  | avatar   |  Jean-Baptiste                                 |
|  |          |  * 12/03/1842, Beaune  -  + 07/11/1918, Pommard|
|  |          |  Male - 76 years old                           |
|  +----------+                                                |
|                                                               |
|  [Edit]   [View in tree]   [Export]                          |
+-------------------------------------------------------------+
```

**Avatar**: 120x160px rectangle. If no profile image is set, a large initials circle placeholder is displayed with gendered background color.

**Name**: surname in uppercase (bold, Cinzel), first name(s) below. If the person has alternate names (married, maiden, alias), they are listed below the primary name in muted text.

**Dates**: birth and death with symbols (* / +), place names included. Calculated age displayed if both dates are known.

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
+------------------------------+
|  PARENTS                     |
+------------------------------+
|  [avatar] MARTIN Pierre      |
|           * 1810  + 1878     |
|                              |
|  [avatar] DUBOIS Marie       |
|           * 1815  + 1890     |
+------------------------------+
```

Each parent is clickable — navigates to that person's profile. If a parent is unknown, a muted "Unknown father" / "Unknown mother" row is shown with a `+ Add` button.

### Spouses & Children

One sub-section per union, ordered chronologically by marriage date (if known).

```
+--------------------------------------+
|  UNION WITH LEMAIRE Marguerite       |
|  (ring) 1865, Beaune                |
+--------------------------------------+
|  Children:                           |
|  [avatar] MARTIN Henri    * 1868     |
|  [avatar] MARTIN Louise   * 1871     |
|  [avatar] MARTIN Pierre   * 1875    |
+--------------------------------------+
```

Spouse and children names are clickable links. An **"Edit union"** link in the sub-header opens the [couple edit modal](ui-person-edit-modal.md).

### Siblings

Listed below the parents section, grouped by shared parents.

```
+------------------------------+
|  SIBLINGS                    |
+------------------------------+
|  [avatar] MARTIN Jeanne      |
|           * 1838  + 1910     |
|  [avatar] MARTIN Louis       |
|           * 1845  + 1920     |
+------------------------------+
```

Each sibling is clickable. If no siblings are known, this section is hidden.

---

## 6. Timeline

Displayed as a card in the right column. A vertical chronological list of all events associated with this person (individual events + family events where this person is involved).

### Structure

```
+-----------------------------------------+
|  TIMELINE                               |
+-----------------------------------------+
|                                         |
|  1842  *  Birth                         |
|           Beaune, Cote-d'Or, France     |
|           (clip) Acte de naissance n42  |
|                                         |
|  1842  (cross)  Baptism                 |
|           Eglise Notre-Dame, Beaune     |
|                                         |
|  1860  (tool)  Occupation: Vigneron     |
|                                         |
|  1865  (ring) Marriage with M. LEMAIRE  |
|           Mairie de Beaune              |
|           Witnesses: Pierre DUVAL, ...  |
|                                         |
|  1918  +  Death                         |
|           Pommard, Cote-d'Or            |
|                                         |
+-----------------------------------------+
```

Each event shows:
- **Year** on the left, bold
- **Icon** — colored circle matching event type (same as [Tree View](ui-genealogy-tree.md) events sidebar)
- **Event type** label, bold
- **Place** if known, in muted text
- **Source reference** if attached, with clip icon, clickable
- **Note excerpt** if present, truncated to 2 lines with "Show more" expansion

Events are ordered by `date_sort`. Events without dates are grouped at the bottom under a "Date unknown" label.

Clicking an event expands it inline to show full details (complete note, all sources, attached media thumbnails).

---

## 7. Media Gallery

Displayed as a full-width card below the two-column layout.

```
+--------------------------------------------------------------+
|  MEDIA (4)                                            [+ Add] |
+--------------------------------------------------------------+
|  +----------+  +----------+  +----------+  +----------+      |
|  |  [img]  *|  |  [img]   |  |  [pdf]   |  |  [img]   |     |
|  |          |  |          |  |          |  |          |      |
|  +----------+  +----------+  +----------+  +----------+      |
|  Portrait      Wedding       Baptism cert.  Vineyard          |
|  (profile)     1865          1842           c. 1880           |
+--------------------------------------------------------------+
```

Same thumbnail grid as the [Person Edit Modal](ui-person-edit-modal.md) media section, but read-only by default. The star badge marks the profile image. Clicking a thumbnail opens a **lightbox overlay** with the full-size image, title, description, and associated event.

The **"+ Add"** button opens the [Person Edit Modal](ui-person-edit-modal.md), scrolled to the media section.

---

## 8. Notes & Sources

Displayed as a card alongside or below the media gallery.

### Notes

All notes associated with this person, displayed as expandable blocks:

```
+--------------------------------------+
|  NOTES (2)                           |
+--------------------------------------+
|  Personal notes, anecdotes...        |
|  (first 3 lines visible)            |
|  [Show more]                         |
|                                      |
|  Research notes on birth date...     |
|  (first 3 lines visible)            |
|  [Show more]                         |
+--------------------------------------+
```

### Sources

All citations linked to this person, grouped by source:

```
+--------------------------------------+
|  SOURCES (3)                         |
+--------------------------------------+
|  (clip) Archives departementales 21  |
|     Page: 3E 42/128, f. 12          |
|     Confidence: High                |
|                                      |
|  (clip) Registre paroissial Beaune   |
|     Page: Baptemes 1842, n15        |
|     Confidence: High                |
+--------------------------------------+
```

Each citation shows the source title, page reference, confidence level, and extracted text if any.

---

## 9. Responsive

- Content max-width: 1200px, responsive padding
- Below **1080px**: the two-column layout (family connections + timeline) collapses to a single column, with family connections above timeline
- Below **900px**: identity header avatar shrinks to 80x106px. Action buttons become icon-only
- Below **640px**: reduced padding

---

## 10. Keyboard & Accessibility

| Key | Behavior |
|---|---|
| `Escape` | Returns to the tree view |
| `E` | Opens the edit modal for the current person |
| `Left` / `Right` | Navigate between persons (previous/next sibling or chronological order) |

---

## 11. Navigation Flow

```
Tree View (canvas)
  |
  +- Click person card -> selected in tree (events sidebar updates)
  |
  +- Click profile icon (person silhouette) in left sidebar
       |
       +- Person Profile (this page)
            |
            +- Click family member -> navigates to their profile
            +- "View in tree" button -> returns to tree view, centered on person
            +- "Edit" button -> opens Person Edit Modal
            +- Escape -> returns to tree view
```
