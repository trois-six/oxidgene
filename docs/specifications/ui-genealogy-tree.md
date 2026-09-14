---
type: "UI Specification"
title: "Visual & Functional Specifications — Genealogy Tree"
description: "UI behavior and interaction specification for Visual & Functional Specifications — Genealogy Tree."
tags: [oxidgene, specification, ui, ux]
timestamp: 2026-06-17T00:00:00Z
---


# Visual & Functional Specifications — Genealogy Tree

> Part of the [OxidGene Specifications](index.md).
> See also: [Person Edit Modal](ui-person-edit-modal.md) · [Person Merge](ui-merge.md) · [Person Profile](ui-person-profile.md) · [Search Results](ui-search-results.md) · [Dictionary](ui-dictionary.md) · [Import](ui-import.md) · [Homepage](ui-home.md) · [Settings](ui-settings.md) · [Data Model](data-model.md) · [API Contract](api.md)

---

## 1. General Structure

### Layout

The canvas displays a **mixed tree**: the focus person is at the vertical center, ancestors go upward, descendants go downward. Each generation occupies a **strict horizontal row**. All cards in the same generation are aligned on the same Y axis.

The number of generations displayed is fixed at any given time, but can be changed via the depth selector. The maximum is **10 ascending generations + 10 descending generations**.
The global pedigree preferences initialize the window to **4 ascending generations + 3 descending generations** by default. They are editable from both [App Settings](ui-app-settings.md) and the global-preferences group in [Tree Settings](ui-settings.md). A saved per-tree view state supplies its own depths instead.

### Always a Connected Tree

The canvas **never** displays isolated persons or disconnected subtrees. A person is visible only if they are reachable from the focus through a continuous chain of relationships (ascending, descending, couple) within the requested depth.

Persons with no link to the current tree are accessible only via **search**.

### Initial Data Loading

The pedigree is loaded once for each combination of tree, focus person, and
requested ancestor and descendant depths. Filling the shared tree metadata
cache during that initial load must not trigger an identical second pedigree
request. Explicit cache invalidation after a mutation still refreshes the
pedigree. Persisting visual state such as pan, zoom, or automatic centering does
not reload pedigree data; only a depth change in the saved view changes the
server query.

---

## 2. Spatial Layout

### Reingold-Tilford placement

Cards are **not** placed on a fixed grid. They are positioned by the
Reingold-Tilford algorithm in Buchheim's linear-time variant, run twice per
render: once over the ascending tree and once over the descending one. The two
results are then translated so both roots land on the same point, which is what
makes the focus person the hinge of the canvas.

The passes work in abstract tree units and are converted to pixels at the end:
horizontal position is multiplied by the theme's card width, vertical position
comes from the generation's depth and the row heights the theme defines (see
[Themes](#9-themes)). A theme with wider cards therefore lays the whole tree out
differently, rather than drawing differently inside the same positions. Rows are
one card unit apart, except the deepest ancestor row, which packs at the
fraction of a unit its theme states (see
[How wide the deepest ancestor row packs](#how-wide-the-deepest-ancestor-row-packs)).

### What the placement guarantees

- Each generation is a **strict horizontal row**: every card at one depth shares
  a Y coordinate
- A couple is **centred over the row of its children**, and a child sits under
  its parents — this, not any global per-level centring, is what aligns the tree
- Two cards at the same depth **never overlap**; the regression suite asserts it
  on family shapes that once produced collisions

### What it does not guarantee

- **Levels are not centred against the widest level.** Each subtree is placed
  relative to its own parent, so a row's centre of mass follows its branch
- **Gaps do appear in the middle of a row.** Two adjacent subtrees are separated
  by their contours, not by a single step, so a wide branch beside a narrow one
  leaves space between cards that no card fills
- **Spacing between neighbours is not uniform.** It is whatever keeps the two
  subtrees clear of each other

### Corrections after placement

Two passes run after the main one:

- **Spouse-group overlap.** A card and its spouse cards form a group wider than
  the card the algorithm placed. When such a group would collide with its
  neighbour, the offending subtree is shifted clear and the parent couple is
  re-centred over its now-moved children row
- **Root siblings.** The focus person's own biological siblings are placed
  beside the tree rather than by the layout pass, at a fixed spacing set by the
  theme, and linked back to the parent card they belong to

---

## 3. Person Card

### Dimensions

Set by the active theme — see [Themes](#9-themes). For the classic theme:

- Standard size: **185x96px** (width x height), drawn rectangle 175x67px
- Deepest ancestor row drawn compact: **95x144px**
- First descendant row: **140px** tall

### Internal Layout

Horizontal arrangement: avatar on the left, text information on the right.

```
+----------------------------------+
| +------+  FAMILY NAME            |
| |      |  First name(s)          |
| | init |  * 12/03/1842           |
| |      |  + 07/11/1918           |
| +------+                         |
+----------------------------------+
```

The card is drawn as SVG primitives inside one `<g>`; the HTML card and its
`.pc-*` classes were removed when the pedigree became pure SVG, and only the
`.ped-card*` classes remain for hover and theming.

**Portrait**:
- 50×50px, square in the classic theme; shape, size, and whether a mat is
  painted behind it are set by the theme
- Displays a **default portrait silhouette** when no profile photo is available, chosen by gender: male (`portrait_male.png`), female (`portrait_female.png`), unknown (`portrait_unknown.png`) — embedded as data URIs in the binary
- When a profile photo is available it replaces the default portrait with `object-fit: cover`
- **SOSA badge**: a 15px disc at the portrait's **bottom-right corner**. An
  ancestor of SOSA 1 gets `var(--pn-sosa)` with a ring cut out of it; SOSA 1
  itself gets `var(--pn-sosa-root)` carrying the digit `1`
- **Self badge**: the person selected in Settings → Tree & Roots → Who am I?
  gets the same disc in `var(--pn-self)` with a solid centre. It is a
  display-only preference; when a person also has a SOSA badge the self badge
  takes precedence, so the selected identity stays visible.

**Text information**, three baselines whose type and spacing come from the
theme:
- First name(s)
- Family name in uppercase
- Lifespan, from Birth > Baptism and Death > Burial. **Years only** — a card has
  no room for a full date, and the precision marks below carry what the year
  alone would lose. Each piece is truncated to the column it must fit

**Date precision marks.** A card has room for a year and nothing else, so an
approximate date would otherwise be drawn as a bare number and read as a fact.
Each year carries the mark for its own `date_qualifier`, giving `ca 1849-< 1917`
— "born about 1849, died before 1917". The symbols are GeneWeb's
(`prec_text`, `lib/dateDisplay.ml`), which is what Geneanet draws, so a user
arriving from a Geneanet tree already reads them:

| Qualifier | Mark | Reads as |
|---|---|---|
| `Exact` | *(none)* | `1849` |
| `About`, `Calculated`, `Estimated`, `FromAge` | `ca ` | `ca 1849` |
| `Perhaps` | `? ` | `? 1849` |
| `Before` | `< ` | `< 1917` |
| `After` | `> ` | `> 1912` |
| `Or` | `\| ` | `1849\|1852`, or `\| 1849` |
| `Between` | `.. ` | `1691..1693`, or `.. 1691` |

GEDCOM's `CAL`/`EST` and our own `FromAge` have no GeneWeb counterpart and all
read as `ca`: each is an approximation reached by a different route, and a card
wants the same warning from all three. The distinction is not lost — it stays on
the event, and both the person edit modal and the events panel still name it in
full (« vers 1849 », « avant 1917 »).

**Ranges get both years when they fit.** `Or` and `Between` name two dates, and
the range is the fact — `1691..1693` says more than either year alone. Measured
against the project's own width estimator at 10px:

| Rendering | Width | Full card (105px) | Compact card (72px) |
|---|---|---|---|
| `1691..1693` | 49.8px | fits | fits |
| `ca 1620-1691..1693` | 92.2px | fits | too wide |
| `1691\|1693-1745\|1750` | 100.8px | fits | too wide |
| `1691..1693-1745..1750` | 105.8px | **too wide** | too wide |

So the wide form is used when it fits and the narrow one (`.. 1691`) when it
does not, rather than squeezing glyphs to illegibility. The narrow form keeps
the mark, so the card understates rather than misleads, and the full text is
one hover away. The side panel's header always uses the wide form: it is HTML
and wraps, so it never has to give a range's far end up.

**Falling back to the sacraments.** A parish register very often records a
baptism and no birth — frequently as an *empty birth stub* someone created to
hang a source on. The card is dated from `Birth > Baptism` and
`Death > Burial`, and the fallback triggers on a **missing date, not a missing
event**: testing `birth.is_none()` keeps the stub and draws a blank year while
a perfectly good "vers 1620" sits unused on the baptism. GeneWeb tests the date
for the same reason (`Date.od_of_cdate`, `Gutil.get_birth_death_date`).

What we deliberately do *not* copy from GeneWeb is its single `approx` flag
covering **both** ends of a life: there, a person whose birth came from a
baptism gets `ca` stamped on their death year too, which is how Geneanet shows
`ca 1691` for a death actually recorded as "entre 11 nov. 1691 et 20 août
1693". Each event keeps its own precision here.

Hovering the date shows the qualifiers spelled out in the current language, as
a native SVG `<title>` — including the far end a narrowed range had to drop
(« Entre 1691 et 1693 »). The tooltip is omitted when every year is exact and
there is nothing to explain. Because the marks include `<` and `>`, the tooltip
is injected as escaped markup — Dioxus's rsx `title` is the HTML element, and an
HTML-namespaced `<title>` inside an `<svg>` is inert.

The line is still compressed with `textLength`/`lengthAdjust` when even the
narrow form overruns: dropping characters off a date would change what it says.

### Visual Indicators

- **Sex-coded rule** beside the portrait: `var(--pn-male-line)` for male,
  `var(--pn-female-line)` for female, `var(--pn-border)` for unknown. A theme
  whose frame is heavy enough may carry the colour on the outline instead and
  draw no rule — the medieval theme does
- **Focus person**: filled with `var(--pn-root-bg)` and set in white, not
  outlined. Hovering any card fills it with `var(--pn-hover-bg)` and strokes it
  with `var(--pn-root-bg)`
- **Spouse cards** use `var(--pn-spouse-bg)`, other cards `var(--pn-bg)`

### Placeholder Card (Unknown Parent)

Appears at **every** ascending level, for each recorded person whose father or
mother is missing — one slot per missing parent — and on the descending side for
a spouse a couple does not name.

- Same dimensions as regular cards
- **Dashed border**, very subtle background
- Centered `+` icon, clickable to open the add-parent form
- Connected to the level below using the same connection rules as real cards

### Selected State

When a card is clicked:
- It becomes the new **focus** of the graph, the layout is recalculated centered on it
- The focus card is filled rather than outlined (see Visual Indicators)
- A **pencil icon** appears just below the card, centered
- The pencil icon disappears as soon as another card is selected or the canvas is clicked

### Pencil Icon — Action Picker

Clicking the pencil icon opens a small **action picker modal** (not a full-screen modal). It presents the available actions for the selected person as a list of labeled options:

| Action | Description |
|---|---|
| **Edit individual** | Opens the full person edit modal |
| **Merge with...** | Opens a person search to select a duplicate to merge |
| **Edit union** | See below — expands into a sub-list if multiple unions exist |
| **Add spouse** | Opens a new person form pre-linked as spouse |
| **Add child** | Opens a new person form pre-linked as child |
| **Add sibling** | Opens a new person form pre-linked as sibling |

The picker is a compact overlay anchored just below the pencil icon, with a subtle backdrop. It closes on outside click or Escape. Choosing an action closes the picker and opens the relevant modal.

### Edit Union — Sub-list

When the selected person has **exactly one union**, clicking "Edit union" immediately opens the couple edit modal.

When the selected person has **two or more unions**, clicking "Edit union" expands an inline sub-list within the picker, replacing the action row. Each union is listed as a single line showing:

```
[Partner name]   * birth year   (ring) marriage year (if known)
```

Clicking a union entry closes the picker and opens the couple edit modal for that specific union. A back arrow at the top of the sub-list returns to the main action list.

---

## 4. Connectors

### General Rules

- Connector shape is set by the active theme — see [Themes](#9-themes). Both
  themes use **right-angled bends, never diagonals**; the classic theme softens
  a bend into an S-curve where a connector has to step sideways, the medieval
  theme rules every one of them straight
- **Solid line only**, regardless of the type of relationship (marriage, cohabitation, other) — no visual distinction by line style
- Color: `var(--pn-border)`, restyled by themes that draw with ink
- Horizontal segments of a generation share one Y level, **except** where a
  person has several spouses: those rows are stepped apart by a few pixels each
  so the segments of different unions stay tellable apart

### Structure of a Couple -> Children Link

```
     [Parent 1]--------------[Parent 2]
                             |
                             |  <- departs from the spouse card's edge
                    ---------+---------
                    |                 |
                [Child 1]         [Child 2]
```

1. Horizontal segment between the two partner cards
2. Vertical line descends from the **spouse card's own edge** on the segment,
   not from its midpoint — with several unions this is what keeps each union's
   children attached to the right partner
3. Horizontal bar at the midpoint between the parents' row and the children's row
4. Vertical lines from the bar down to the top of each child card

### Case: One Parent Has Multiple Unions

Each union produces an **independent horizontal segment**, and the segments are
stepped a few pixels apart vertically so two unions of the same person do not
merge into one line.

```
[Mother B]------[Father]------[Mother A]
     |                             |
     |                             |
-----+-----                   -----+-----
|         |                   |         |
[Child B1][Child B2]      [Child A1][Child A2]
```

The shared parent card serves both segments. Each union's children hang from
**that union's own spouse card**, which is what keeps them attributed to the
right partner. A child recorded with only one parent hangs from the empty
placeholder standing in for the other.

### Case: Unknown Parent (Placeholder)

The placeholder counts as a full card, and children with no second parent
recorded are attached to it:

```
[Known parent]----[?]
                   |
                   |
               [Child]
```

### Alignment

- Vertical runs fall on **card centres**, and a connector attaches at offsets
  the theme defines — so both themes attach in the same places and differ only
  in how the line travels between them
- The children's horizontal bar is drawn between the two rows

---

## 5. Navigation and Controls

### Topbar

Fixed height, spans the full width above the canvas. Uses the shared `td-topbar` component.

```
+----------------------------------------------------------------------+
|  [logo] tree_name / Tree              [Last name] [First name] [Q]   |
+----------------------------------------------------------------------+
```

**Breadcrumb** (`.td-bc`): logo icon (links to homepage) + tree name (`.td-bc-link`) + `/` separator (`.td-bc-sep`) + "Tree" label (`.td-bc-current`). The tree name links to the tree view.

### Search

Two independent fields in the topbar, aligned to the right: **Last name(s)** and **First name(s)**. Either field can be used alone, or both combined. The **Last name(s)** field can be used to search a name or a SOSA number, if the element searched is a number it is a SOSA number. A magnifying glass button triggers the search.

**On Enter** (or click magnifying glass):
- Navigation to a dedicated **results page** (`/trees/{id}/search`)
- All matching persons displayed as a list
- Additional filters available (dates, location, gender...)
- Each result is clickable and returns to the tree centered on that person

### Left Sidebar (ISB)

Fixed vertical bar (`var(--sb)` = 46px wide, reduced to 36px at or below
400px). SVG stroke icon buttons stacked vertically, tooltip on hover. No text
displayed. All icons use a consistent style: `stroke: currentColor`,
`fill: none`, `strokeWidth: 2`, 16x16px viewBox.

**Buttons top to bottom**:

| Icon | SVG description | Action |
|---|---|---|
| Org-chart | 3 small rectangles connected by lines (sitemap) | Tree view (active by default) |
| Person silhouette | Circle head + body path | Detailed profile view |
| Stacked layers | 3 horizontal paths with decreasing width | Depth selector |
| Magnifying glass + | Magnifying glass with plus sign | Zoom in |
| Four corners | 4 corner arrows pointing outward (maximize) | Fit to screen |
| Magnifying glass - | Magnifying glass with minus sign | Zoom out |
| Person + plus | Person silhouette with a small plus | Add a person |
| **separator** | Thin horizontal line | Visual divider |
| Book/index | Open book (two overlapping page shapes) | Opens [Dictionary](ui-dictionary.md) for this tree |
| Gear | Gear/cog icon (Lucide gear path) | Opens [Settings](ui-settings.md) for this tree |

This left sidebar (`TreeIconSidebar`) is a component shared with the [Person Profile](ui-person-profile.md) page, so the **Book/index** and **Gear** buttons are reachable identically whether the user is currently viewing the pedigree canvas or a person's profile — not just from the tree view.

**Depth selector — hover panel**:

Appears to the right of the button on hover. No text, no Apply button. Changes are applied immediately.

```
+----------+
|  ^ - 2 + |
|  v - 2 + |
+----------+
```

- `^`: number of ascending generations (0-10)
- `v`: number of descending generations (0-10)
- Layout recalculated immediately on each `+` or `-`
- The panel stays open as long as the mouse is over the button or the panel
- Closes on mouseout with a 150ms delay

**Profile view**: switches the canvas to a detailed profile of the selected person. A back button returns to the tree.

### Canvas Interactions

| Action | Behavior |
|---|---|
| Click on a card | New focus + pencil icon + events sidebar updated |
| Click on placeholder `+` | Opens add-parent form |
| Drag on canvas | Free pan |
| Scroll wheel / pinch | Zoom about the pointer, range 0.3x-2x |
| Zoom in / out buttons | Zoom about the middle of the free canvas, same range |
| FIT button | Reframes the entire tree in the window |
| Depth selector | Recalculates layout, recenters on current focus |

A zoom holds one point of the canvas still and moves everything else around it.
The wheel holds the point under the pointer; the buttons have no pointer of
their own and hold the middle of the free canvas — the same point a fit centres
the graph on, so the two agree. "Free" means the canvas minus the events panel,
which is why zooming does not drift sideways when the panel is open.

### Focus Change

**Person already visible in the tree**: layout recalculated and recentered, animated transition.

**Person outside the current tree** (via search): tree entirely rebuilt around the new focus, no transition.

---

## 6. Events Sidebar (Right)

### General Behavior

- Default width: a fixed 275px, until the reader resizes it
- Resizable from its left edge with a 2px visual handle and an 8px pointer target
- Width is constrained to 220-640px, capped at 45% of the space remaining after
     the icon sidebar
- A resized panel is remembered locally as a ratio of that remaining space, so it
     stays proportional when the window is resized, within the same px bounds
- Releasing the handle runs the existing fit-to-viewport behavior so the full tree
     remains framed without introducing a separate zoom calculation
- The focused handle can also be adjusted with the left and right arrow keys
- Collapsible via a toggle button on its left edge
- Collapsed: only the button remains visible, the canvas reclaims the space
- Open/closed state is remembered

### Content

Header with avatar (default portrait or profile photo), full name and dates of the selected person. Then a chronological list of their events, grouped by year.

The header's dates are **the same lifespan string the card draws** — precision
marks and all — not the `n. 1620` / `d. 1691` abbreviations it used to carry.
The panel sits beside the card showing that very person, and two spellings of
one life read as two different facts.

The **events below keep their own full-text dates** (« entre 11 nov. 1691 et
20 août 1693 »), rendered through `format_date`. That needs the whole event, so
`PedigreeNode` carries `birth` / `death` as `ProfileEvent`s rather than an
extracted year: a year string cannot hold the day, the month, the far end of a
range, or the calendar, and dropping them is what once made a birth on 2 Nov
1788 show as a bare "1788".

```
+------------------------------+
| [avatar] FAMILY First name   |
|          * 1842  + 1918      |
+------------------------------+
| EVENTS                       |
+------------------------------+
| 1842                         |
|  *  Birth                    |
|     <place A, region>       |
|                              |
| 1865                         |
|  (ring) Marriage             |
|     with <person B>         |
|                              |
| 1918                         |
|  +  Death                    |
|     <place B>                |
+------------------------------+
```

### Event Types

Each event type has a colored circle icon (`.ev-ic-*`):

| Icon class | Color | Type |
|---|---|---|
| `ev-ic-birth` | Green | Birth |
| `ev-ic-death` | Blue | Death |
| `ev-ic-marry` | Orange | Marriage |
| `ev-ic-other` | Grey | Other events |

Each event is clickable to display full details (complete location, source, notes).

---

## 7. Overall Layout

```
+----------------------------------------------------------------------+
|                        TOPBAR + SEARCH                                |
+------+----------------------------------------------+----------------+
|      |                                              |                |
|  I   |                                              |    EVENTS      |
|  S   |           CANVAS -- TREE                     |   SIDEBAR      |
|  B   |                                              |   (275px)      |
|      |                                              |                |
|      |                                              |                |
+------+----------------------------------------------+----------------+
```

| Zone | Dimensions |
|---|---|
| Topbar | Auto height, full width |
| Left sidebar (ISB) | Fixed width 46px (`var(--sb)`), height = zone below topbar |
| Canvas | Remaining space, scrollable and zoomable |
| Right sidebar | Default width 275px, resizable (proportional afterwards) and collapsible |

---

## 8. Responsive

- Card sizes do not vary with viewport width: they are fixed by the active
	theme (see [Themes](#9-themes)), and a narrow viewport is handled by
	zooming and panning the canvas rather than by redrawing the cards
- At **600px wide and below**, the right events sidebar automatically collapses
     and its resize handle is hidden; the user can still reopen the collapsed panel
- At **400px wide and below**, the right events sidebar disappears entirely and
     the canvas reclaims its full width; the shared left icon sidebar narrows from
     46px to 36px on every page where it appears
- Left sidebar remains fixed but tooltips are replaced by visible labels below each icon

---

## 9. Themes

The pedigree is drawn by a **theme**, chosen by the viewer under
[App Settings > Pedigree](ui-app-settings.md) and persisted in
`localStorage('oxidgene-pedigree-theme')` under its own name (`classic`,
`medieval`). A name no longer shipped falls back to the default rather than
failing. The choice applies to every pedigree on the device — the tree canvas,
the fragments on the person profile, and the ones in search results — because
they are all the same chart.

A theme is not a palette swap. It owns three things:

| What | Effect |
|------|--------|
| **Metrics** | Card boxes, drawn rectangles, connector attachment offsets, canvas margin and sibling spacing. These feed the layout pass, so a theme with larger cards lays the whole tree out differently |
| **Link style** | The shape of a connector between two fixed attachment points |
| **Card style** | Frame, portrait mat, text column and baselines, type, badge, and whether a separate sex-coded rule is drawn |

Attachment points are derived from the cards and the metrics alone, so every
theme attaches its connectors in the same places: **a theme changes how a line
travels, never where it lands.**

Colors are not part of the theme object. They are CSS variables redefined under
the theme's own class on the pedigree viewport, which is what lets a theme with
its own ground sit inside either application theme without the two bleeding
into each other.

### Classic

The default, and what OxidGene drew before themes existed — specified to be
pixel-for-pixel identical to it.

- Card **185x96px**, drawn rectangle 175x67px, 5px corner radius
- Deepest ancestor row drawn compact: **95x144px** in **half** a column
- First descendant row **140px** tall
- Neutral card outline, with sex shown by a short coloured rule beside the
  portrait
- Square portrait mat
- Connectors are elbows, softened into an S-curve wherever one has to step
  sideways
- Follows the application's light or dark palette

### Medieval

An engraved pedigree, in the manner of a painted *Stammtafel*.

- Card **194x190px** — portrait, not landscape — drawn shape 158x162px, no
  corner radius: the outline is a path and its corners are cut by that path
- Every person stands in a **heraldic escutcheon**: arched crown, flared
  shoulders, straight flanks, and a foot drawn to a point. It is generated from
  the card box, so it stretches to whatever size a card is given
- Double-ruled: the outer rule carries the sex colour, and a second rule of the
  same shape sits 7px inside it
- Names sit **centred beneath** a circular portrait medallion, the arrangement
  these plates use, rather than beside a portrait as in the classic card. Both
  ranks share the baseline, which is set from the medallion: high enough that
  the lifespan clears the foot, low enough that a capital clears the portrait
- No ground is painted behind the portrait. A portrait keeps its aspect ratio
  and rarely fills its box, so a mat shows beside it as a shape that does not
  follow the photograph; the parchment shows through instead. The classic theme
  keeps its mat, where the card stands on a flat ground
- Deepest ancestor row compact: **145.5x188px**; first descendant row **230px**
- Connectors are ruled elbows — right angles throughout, no curve — drawn as a
  band: an ink stroke with a parchment core running down it
- Parchment ground built from repeating gradients rather than an image, so the
  canvas costs no request and tiles at any zoom
- Surnames in Cinzel, already loaded for headings, so the theme adds no font
  request

The card is larger in every direction because the second rule and the medallion
ring take real room. Taking it from the text column instead would truncate names
the classic theme shows whole, so a theme's name column may never be narrower
than the classic one at the same card size.

### How wide the deepest ancestor row packs

The deepest ancestor row is the widest row of the chart, so it is packed tighter
than the rest: a theme states that column as a fraction of `card_w`. The classic
theme uses **half** a column, which it can afford because its compact card is a
narrow portrait standing under landscape cards. A theme whose cards are portrait
at every rank has no such slack — the medieval theme takes **three quarters** of
a column, which leaves the same gap between two crowns up there as between two
cartouches anywhere else.

A card is drawn one `padding` inside its column, so `padding + drawn width` must
fit the column at both ranks or neighbouring cards overlap. This is checked per
theme, from the numbers alone, rather than per rendering.
