---
type: "UI Specification"
title: "Visual & Functional Specifications — Person Profile"
description: "UI behavior and interaction specification for Visual & Functional Specifications — Person Profile."
tags: [oxidgene, specification, ui, ux]
timestamp: 2026-06-17T00:00:00Z
---


# Visual & Functional Specifications — Person Profile

> Part of the [OxidGene Specifications](index.md).
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
| [logo] <tree name> / <person A>                                      |  <- td-topbar
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
[logo] <tree name> / <person A>
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
|  |          |  <surname A>                                   |
|  | avatar   |  <given names A>                               |
|  |          |  * 12/03/1842, <place A> - + 07/11/1918, <place B>|
|  |          |  Male - 76 years old                           |
|  +----------+                                                |
|                                                               |
|  [Edit]   [View in tree]   [Export]                          |
+-------------------------------------------------------------+
```

**Avatar**: 120x160px rectangle. If no profile image is set, a large initials circle placeholder is displayed with gendered background color.

**Name**: surname in uppercase (bold, Cinzel), first name(s) below. If the person has alternate names (married, maiden, alias), they are listed below the primary name in muted text.

**Dates**: birth and death with symbols (* / +), place names included. Calculated age displayed if both dates are known.

The dates are written out in full through `format_date`, qualifier included — « vers 1796 », « entre 11 nov. 1691 et 20 août 1693 » — not reduced to a year. Birth falls back to **baptism** and death to **burial** when the primary event carries no date, matching the pedigree card and its side panel; see [Tree View](ui-genealogy-tree.md). The label follows the event actually displayed: a baptism reads « Baptisé(e) le » rather than « Né(e) le », and a burial reads « Inhumé(e) le » rather than « Décédé(e) le ».

**A clause that would say nothing is not rendered.** A birth event carrying
neither a date nor a place omits the clause entirely. A birth with a place but
no date reads `Born in <place A>`, following the same treatment as death.

**The participle agrees with the sex.** `Sex::Male` and `Sex::Female` use their
localized forms; the parenthesized form is reserved for `Sex::Unknown`.
English keeps the same three keys even where their values are identical. See
[Cross-cutting Rules §3.4](cross-cutting.md).

**Gender**: label + colored dot (blue male, pink female, grey unknown).

**Identity badges.** A person with a SOSA number carries the green SOSA badge.
When this profile is the person selected by Settings → Tree & Roots → Who am I?,
a second blue **Me** badge appears beside it. The badge links to Tree & Roots so
the user can change or clear that cosmetic identity preference directly from the
person it identifies.

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
|  [avatar] <person B>         |
|           * 1810  + 1878     |
|                              |
|  [avatar] <person C>         |
|           * 1815  + 1890     |
+------------------------------+
```

Each parent is clickable — navigates to that person's profile. If a parent is unknown, a muted "Unknown father" / "Unknown mother" row is shown with a `+ Add` button.

### Spouses & Children

One sub-section per union, ordered chronologically by marriage date (if known).

```
+--------------------------------------+
|  UNION WITH <person D>               |
|  (ring) 1865, <place A>              |
+--------------------------------------+
|  Children:                           |
|  [avatar] <person E>      * 1868     |
|  [avatar] <person F>      * 1871     |
|  [avatar] <person G>      * 1875     |
+--------------------------------------+
```

Spouse and children names are clickable links. An **"Edit union"** link in the sub-header opens the [couple edit modal](ui-person-edit-modal.md).

### Siblings

Listed below the parents section, grouped by shared parents.

```
+------------------------------+
|  SIBLINGS                    |
+------------------------------+
|  [avatar] <person H>         |
|           * 1838  + 1910     |
|  [avatar] <person I>         |
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
|           <place A, region, country>    |
|           (clip) <source A>             |
|                                         |
|  1842  (cross)  Baptism                 |
|           <place B>                     |
|                                         |
|  1860  (tool)  Occupation: <label A>    |
|                                         |
|  1865  (ring) Marriage with <person B>  |
|           <place C>                     |
|           Witnesses: <person C>, ...    |
|                                         |
|  1918  +  Death                         |
|           <place D, region>             |
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

Attached event media appear inline as compact `44 x 44 px` thumbnails without
titles. Clicking a thumbnail opens the media viewer; its context menu remains
available on right-click for media and event-link actions.

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

This uses the canonical `MediaGallery` rendered with `read_only: true`, not a
second grid that looks similar. The ★ badge marks the profile image; a tile's
↗ opens the file. Editing takes place in the dedicated media manager rather
than in the person form. The gallery combines media attached directly to the
person, media attached to any couple in which the person is a spouse, and
every vignette identifying that person. Direct and couple attachments are
de-duplicated by media id. An identification is rendered as its cropped
vignette rather than as the complete source image.
Clicking an attachment or identification that targets one page of an assembled
multi-page document opens the document viewer directly on that page, with the
document pager and the surrounding pages available. This applies equally to a
direct person attachment and to an attachment inherited from a conjugal family.

Right-clicking a media tile opens **Link an event**, listing this person's own
events and the events of their conjugal families. A linked event is removable
from the same menu through **Unlink an event**. Under the media title, each
linked event shows its date (as `dd/mm/yyyy` for an exact Gregorian date) and
its event type. The title is centered; the event date and type are centered
below it, italicized, and use the same font size as the title. The event picker
shows at most five events; vertical previous and next controls scroll its
five-row window by one event when more are available.

The section remains visible when the person has no media. A compact `+` button
beside its title opens `MediaManagerModal` for this person. The modal owns
uploading, document creation, cropping, retitling, portrait selection,
event-link management, detaching, and deletion.

A tile opens the media viewer. The viewer fills the available viewport inside
its backdrop (edge-to-edge on narrow screens), and a fitted image may use the
full height of its media stage. Once the image loads, the viewer initializes
its fitted dimensions and enables **Zoom in** and **Zoom out** immediately;
the user does not have to activate **Fit** first. While zooming, the image stays
centered on each axis until it actually overflows on that axis; only then does
the stage expose scrolling without making either edge unreachable. Its facts
column keeps **Edit** and **Delete** as content-width actions on the same row.
One compact target icon beside the zoom controls has the same square dimensions
as those controls and opens the shared contextual menu. For an ordinary image,
the menu offers **Identify a person**, **Attach to a person**, and **Attach to a
couple**. For a multi-page document, it offers exactly five actions:
**Identify a person**, **Attach the multi-page document to a person**, **Attach
the multi-page document to a couple**, **Attach the page to a person**, and
**Attach the page to a couple**. The two page actions target the page displayed
when the person or couple is finally selected. It remains immediately above
the image on narrow screens. **Identify a person** starts drawing an
identification region on the displayed page. Its instruction and source-pixel rectangle
readout use the same status-row typography and line box, so beginning a valid
selection does not move the image vertically. **Attach to a person** links the
selected whole document or displayed page without creating a vignette. **Attach to a couple** first
searches for one person, then requires an explicit choice among that person's
conjugal families before linking the whole image to the family. Both attachment
actions detect an existing link and do not create duplicates. The facts column persistently lists
whole-image attachments and cropped identifications together under
**Attachments / identifications**. The label occupies its own row and the
combined list spans the full width of the facts column below it. The list has a
fixed five-row viewport; the previous control sits at its top and the next
control at its bottom. They scroll that window by one row without making the
facts column taller. The two relation types keep
their distinct semantics: each attachment is a compact row with a `36 × 28 px` whole-image
thumbnail, an ellipsized person or couple label, a **Document** or **Page N**
scope badge on multi-page documents, and a trailing remove control that deletes
only that link. An identification always carries the **Page N** badge. When the displayed page has a person
identification and a whole-media attachment for the same person, the
identification replaces the attachment in this list; both rows are never shown
together. For a
multi-page document, the list combines document-level attachments with those
of the displayed page; **Identify a person** creates a region on that page. The facts column
keeps the document's general note separate from the current page's transcript.
It labels the latter with the displayed page number and reloads it when the
pager moves. **Edit** changes both fields in one form but writes them to their
respective media records: the document id for the general note and the page id
for the transcript. Paging is disabled while that form contains unsaved edits.
An empty transcript removes the page note.
For an assembled multi-page document, the facts action row also exposes
**Manage pages** independently from metadata editing. It opens a page strip in
the viewer's facts column. Files selected or dropped on **Add pages** are
appended in upload order; each existing page has move-up, move-down, and remove
controls. Removing requires confirmation and detaches the page from the
document without deleting its media record, stored bytes, or transcript. Every
attachment, identification, and portrait reference targeting that page is
removed. Remaining pages close the numbering gap and `page_count` is
recomputed. Permanently deleting an ordinary single media applies the same
relation cleanup before deleting its record and unshared stored objects. Every
addition, move, or removal reloads the open viewer
immediately; if the current last page is removed, the viewer clamps to the new
last page.
The facts column
uses dense key/value rows without framed value boxes so its metadata and
relations remain visible together. The **Edit** and
**Delete** action group is centered in the facts column. Each identified person
is shown on one compact row with a `36 × 28 px` crop thumbnail, a single-line
ellipsized name, and a trailing remove control; the facts column omits this list
when it is empty.
The embedded edit form centers its **Cancel** and **Save** action group on the
same axis as the facts column's **Edit** and **Delete** actions.
Identification regions remain aligned over the fitted or zoomed image;
hovering either a region on the image or its identification row in the facts
column reveals its frame and person label.

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
|  (clip) <source A>                   |
|     Page: <reference A>              |
|     Confidence: High                |
|                                      |
|  (clip) <source B>                   |
|     Page: <reference B>              |
|     Confidence: High                |
+--------------------------------------+
```

Each citation shows the source title, page reference, confidence level, and extracted text if any.

---

## 9. Responsive

- Content max-width: 1200px, responsive padding
- Below **1080px**: the two-column layout (family connections + timeline) collapses to a single column, with family connections above timeline
- Below **900px**: the identity header actions move to a separate horizontal
     row. The SOSA and **Me** badges stay at its leading edge, while **Delete**,
     **Edit**, and **Refresh** become accessible icon-only buttons at its trailing
     edge.
- Below **640px**: reduced padding. The primary name remains beside the compact
     avatar, while alternate names and the birth, death, age, place, and occupation
     summary use the full card width below them. Family narratives and person chips
     wrap within the family card; long names and lifespan years never extend beyond
     the viewport.

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
