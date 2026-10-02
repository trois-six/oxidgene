---
type: "UI Specification"
title: "Visual & Functional Specifications — Genealogy Tree"
description: "Pedigree canvas with person cards, connectors, navigation, the events sidebar, and the other charts it can draw."
tags: [oxidgene, specification, ui, ux]
generated: { by: claude-code/claude-opus-5-5, at: 2026-10-02T02:06:56Z }
---


# Visual & Functional Specifications — Genealogy Tree

> Part of the [OxidGene Specifications](index.md).
> See also: [Person Edit Modal](ui-person-edit-modal.md) · [Person Merge](ui-merge.md) · [Person Profile](ui-person-profile.md) · [Search Results](ui-search-results.md) · [Kinship](ui-kinship.md) · [Dictionary](ui-dictionary.md) · [Import](ui-import.md) · [Homepage](ui-home.md) · [Settings](ui-settings.md) · [Data Model](data-model.md) · [API Contract](api.md)

---

## 1. General Structure

### Layout

This is the default drawing, the **tree**. The viewer can choose another one
in the settings — an ancestor wheel, a fan chart, or the lineage of Gramps' Pedigree view — described in
[§10 Views](#10-views); everything below describes the tree unless it says
otherwise.

The canvas displays a **mixed tree**: the focus person is at the vertical center, ancestors go upward, descendants go downward. Each generation occupies a **strict horizontal row**. All cards in the same generation are aligned on the same Y axis.

The number of generations displayed is fixed at any given time, but can be changed via the depth selector. The maximum is **10 ascending generations + 10 descending generations**.
The global pedigree preferences initialize the window to **4 ascending generations + 3 descending generations** by default. They are editable from both [App Settings](ui-app-settings.md) and the global-preferences group in [Tree Settings](ui-settings.md). A saved per-tree view state supplies its own depths instead. That view state (root person, depths, pan and zoom) lasts as long as the window: leaving the tree for any other page, the home page and the application settings included, and coming back reopens it on the same root and framing.

### Always a Connected Tree

The canvas **never** displays isolated persons or disconnected subtrees. A person is visible only if they are reachable from the focus through a continuous chain of relationships (ascending, descending, couple) within the requested depth.

Persons with no link to the current tree are accessible only via **search**.

### Initial Data Loading

The pedigree is loaded once for each combination of tree, focus person, and
requested ancestor and descendant depths. Filling the shared tree metadata
cache during that initial load must not trigger an identical second pedigree
request. Explicit cache invalidation after a mutation still refreshes the
pedigree. Persisting visual state such as pan, zoom, or automatic centering does
not reload pedigree data or rebuild the card-and-connector layout; only its CSS
transform and zoom readout react during direct manipulation. Only a depth change
in the saved view changes the server query.

A depth change sends that query at once. The chart draws no deeper than the
pedigree it holds, so raising the depth leaves the cards as they are until the
deeper pedigree arrives, then lays it out and refits; lowering it within what is
already loaded redraws immediately. The layout is recomputed only when the
pedigree, focus, SOSA data, drawn depth or theme changes — never for the depth
popover, the events sidebar or a selection. Portraits are not part of the
layout either: they load after the pedigree and reach each card's picture and
the events panel on their own, so their arrival redraws the pictures alone.

Opening a tree with no person chosen asks the server for the pedigree around
the tree's default root (its SOSA root, else its first person), so the chart
waits on no other request; the tree's own record loads beside it. Each node
carries its portrait's source and whether it is the SOSA root or one of its
ancestors, so the pictures come from one further request and the SOSA badges
need no ancestry of their own. The circular views (wheel, fan and their
descendant forms) draw no portrait and ask for none.

Only the cards and connectors near the viewport are in the DOM, in every view
(the wheel's and the fan's segments, the lineage view's boxes and lines
alike): everything within one viewport of the visible area on each side. Panning or zooming
redraws that set once the view comes within three quarters of a viewport of
its edge, adding only the strip of cards uncovered, so drawn content always
extends well past what can be seen. An animated move (zoom buttons, fit,
re-centering) keeps the view it starts from as well as the one it ends on,
since every frame of the transition lies between the two. Before the viewport
has been measured a 3840 × 2160 screen is assumed. The layout itself is
unchanged: sizes, fit, zoom limits and the exported SVG still cover the whole
tree.

Initial fitting waits until the saved pedigree theme and the right events
sidebar state have been applied. Opening, closing, or resizing that sidebar
refits the graph against the canvas space that remains visible.

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

The card is drawn as SVG primitives inside one `<g>`, with no HTML; the
`.ped-card*` classes carry its hover and theming.

**Portrait**:
- 50×50px, square in the classic theme; shape, size, and whether a mat is
  painted behind it are set by the theme
- Displays a **default portrait silhouette** when no profile photo is available, chosen by gender: male, female or unknown (`assets/portraits/`) — embedded as data URIs in the binary
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
- **Additional relations**: a small blue `+` marks a person whose other
  relationships are outside the current layout. It stays clear of connectors;
  the medieval theme places it outside the cartouche on the left, level with
  its bottom point rather than above the crown

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
| **Merge with...** | Opens the [merge wizard](ui-merge.md) on its search for the other record of this person |
| **Edit union** | See below — expands into a sub-list if multiple unions exist |
| **Add spouse** | Opens a new person form pre-linked as spouse |
| **Add child** | Opens a new person form pre-linked as child |
| **Add sibling** | Opens a new person form pre-linked as sibling |
| **Relationship with…** | Asks for a second person — with shortcuts for the user and the SOSA root — then opens the [Kinship](ui-kinship.md) page between the two |
| **Go to…** | In the wheels, fans and lineage (§10): opens a list of the person's relatives the chart does not draw around them — spouses and children in an ancestor chart, parents and spouses in a descendant one — under a heading per kind, each with their lifespan; choosing one makes them the focus. The chart's own focus is left out, and so is the entry when the list would be empty. Not offered on the tree, which draws them all |

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

**Print**: the shared print action of [Common UI §7](ui-common.md#7-printing),
in the left sidebar. When the chart runs past the screen and is larger than a
sheet at the zoom shown, the action first offers to print it whole over
several sheets, assembled by
their overlap (Common UI §7.3). Otherwise, or on choosing *What the screen
shows*, the chart prints as it is framed on screen, at the zoom
shown — the area the canvas shows, less
what the events panel covers, trimmed to the cards drawn — scaled to one
landscape sheet under the printed header. The sidebars, the depth and zoom
controls, and the events panel do not print, nor do the chart's own controls
— the pencil under the focus card, the "+" of an empty slot for a missing
parent, the lineage view's children button — while the empty slot's dashed
outline still shows the missing parent. To print more or less of the tree,
zoom or pan before printing.

### Search

Two independent fields in the topbar, aligned to the right: **Last name(s)** and **First name(s)**. Either field can be used alone, or both combined. The **Last name(s)** field can be used to search a name or a SOSA number, if the element searched is a number it is a SOSA number. A magnifying glass button triggers the search.

**While typing**, a suggestion panel opens beneath the fields, in two levels:

- First, at most five of the tree's names completing the field being typed —
  surnames for **Last name(s)**, the given name being typed for
  **First name(s)** — each with the number of persons carrying it among those
  the other field finds (with a surname typed, a given name counts only the
  bearers of that surname), as the entry forms suggest them ([Common UI](ui-common.md) §4.4). Picking one
  completes the field and keeps the panel open, so the persons narrow down to
  it. The list follows the field being typed in, and a tree whose entry
  suggestions are off lists none
- Then at most six matching persons, ranked by relevance, each drawn with the
  shared result row from [Search Results](ui-search-results.md) §7 — portrait,
  name, years, and the relation line that tells two people of the same name
  apart
- A footer leading to the full results page, shown only when more matches
  exist than the panel lists
- When the **Last name(s)** field holds a bare number, the panel resolves it as
  a SOSA number and previews that one person, so it shows where Enter would go
- Requests are debounced, and input shorter than two characters across both
  fields queries nothing. Fields filled in by the page rather than typed — the
  results page pre-fills them — query nothing until the panel opens
- The panel closes when the window is resized, as every anchored overlay
  does ([Common UI](ui-common.md) §4.8)
- **Down** / **Up** move the highlight through the names then the persons,
  wrapping at either end; **Enter** completes the field with the highlighted
  name or opens the highlighted person; **Escape** or a click outside closes
  the panel

The panel keeps one width whatever it lists, so it does not jump as the names
and persons change under the typing; long lines are truncated. It is a fixed
overlay, because the topbar clips its overflow. It reuses
the shared contextual-surface component, whose backdrop also handles dismissal
— a blur handler would close the panel before a click on a row could register.

**On Enter with nothing highlighted** (or click magnifying glass):
- Navigation to a dedicated **results page** (`/trees/{id}/search`)
- All matching persons displayed as a list
- Additional filters available (dates, location, gender...)
- Each result is clickable and returns to the tree centered on that person

The same component serves the person profile and the results page itself. On
the results page it searches in place rather than navigating, and shares its
field state with the duplicate surname and given-name inputs in the filter
panel, so typing in either updates both.

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
| Two silhouettes | Two heads + bodies, side by side | [Couple view](ui-couple-profile.md) of the selected person's earliest couple; absent when the person has no known spouse |
| Stacked layers | 3 horizontal paths with decreasing width | Depth selector |
| Magnifying glass + | Magnifying glass with plus sign | Zoom in |
| Four corners | 4 corner arrows pointing outward (maximize) | Fit to screen |
| Magnifying glass - | Magnifying glass with minus sign | Zoom out |
| Person + plus | Person silhouette with a small plus | Add a person |
| **separator** | Thin horizontal line | Visual divider |
| Book/index | Open book (two overlapping page shapes) | Opens [Dictionary](ui-dictionary.md) for this tree |
| Chart | Axes with a rising line | Opens [Statistics](ui-statistics.md) for this tree |
| Wrench | Lucide wrench | Opens [Tools](ui-tools.md) for this tree |
| Printer | Lucide printer | Prints the page ([Common UI §7](ui-common.md#7-printing)); shown on every page that prints |
| Gear | Gear/cog icon (Lucide gear path) | Opens [Settings](ui-settings.md) for this tree |

The Book/index, Chart, Wrench and Gear buttons are shown on every tree page,
which they find from the page's route, and show as current on their own page
([Common UI §6.3](ui-common.md#63-shared-left-icon-sidebar)).

This left sidebar (`TreeIconSidebar`) is a component shared with the [Person Profile](ui-person-profile.md) and [Couple Profile](ui-couple-profile.md) pages, so the **Book/index**, **Chart**, **Wrench** and **Gear** buttons are reachable identically whether the user is currently viewing the pedigree canvas or a person's profile — not just from the tree view. Its profile and pedigree buttons act on the person being shown: the selected card, the open profile or couple. On pages about the tree as a whole (Settings, Dictionary, Statistics, Tools, search results) that is the person last shown in this tree during the session, or the SOSA root when none has been, so leaving a profile for the settings and pressing the profile button comes back to the same person.

**Depth selector — hover panel**:

Appears to the right of the button on hover. No text, no Apply button. Changes are applied immediately.

```
+----------+
|  ^ - 2 + |
|  v - 2 + |
+----------+
```

- `^`: number of ascending generations (0-10)
- `v`: number of descending generations (0-10); absent in a view that draws
  ancestors only ([§10](#10-views)), where the value is kept but unused
- Layout recalculated immediately on each `+` or `-`
- The panel stays open as long as the mouse is over the button or the panel
- Closes on mouseout with a 150ms delay

**Profile view**: switches the canvas to a detailed profile of the selected person. A back button returns to the tree.

### Canvas Interactions

| Action | Behavior |
|---|---|
| Click on a card | New focus + pencil icon + events sidebar updated |
| Right-click on a card | Opens the same action picker at the pointer |
| Click on placeholder `+` | Opens add-parent form |
| Drag on canvas | Free pan — starting on a card, a segment or the root disc as well: a press that moves more than 5 px before its release pans and is not a click, in every view |
| Scroll wheel / pinch | Zoom about the pointer, range 0.3x-4x (up to 16x in the wheel and the fan, as their narrowest labels need) |
| Zoom in / out buttons | Zoom about the middle of the free canvas, same range |
| FIT button | Reframes the entire tree in the window; a tree too large to fit at 0.3x is centred on the focus person — in the lineage view, whose focus person is its left edge, it keeps them centred vertically and starts at the left margin when wider than the window (centred across otherwise) |
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

Header with avatar (default portrait or profile photo), full name and dates of the selected person. Then a chronological list of their events, grouped by year — the year of the normalized date, in any calendar — with the undated events first under *Undated*.

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

When a mini-pedigree has no descendants, its root card is anchored near the
bottom of the fragment. The anchor reserves the active theme's scaled card
half-height plus a 20px bottom margin, so tall frames such as the medieval
escutcheon are never clipped by the viewport.

Mini-pedigrees are static fragments. They automatically reduce their scale
below the context's preferred maximum when needed to show the selected person
and up to two available ancestor generations without clipping, in every theme.
Each fragment measures its own viewport and fits again whenever that viewport
changes size, for example when the window is resized or a two-column layout
collapses to one. Several fragments on one page never share a measurement. A
fragment stays hidden until its viewport has been measured, so it is never
drawn at the wrong position first.
Their person cards remain clickable for navigation, but the fragment itself
does not pan or zoom. Root siblings are omitted from this focused ascending
view, and no duplicate descending root is rendered when descendants are not
requested.

Hovering a mini-pedigree card shows its full name and lifespan in an HTML
tooltip next to the pointer and constrained to the browser viewport. Keyboard
focus shows the same tooltip at a stable position above the SVG. The tooltip is
rendered at screen size and is not affected by the fragment's fitted scale, so
medieval cards remain identifiable when three generations require a small zoom.

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
the theme's own class on the pedigree viewport. The canvas itself keeps the
application's light or dark background in every pedigree theme.

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
- Uses the application's canvas background, without a dedicated parchment
  ground
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

---

## 10. Views

The tree view draws the pedigree it has loaded in one of several ways, chosen
by the viewer under [App Settings > Pedigree](ui-app-settings.md#7-section-pedigree)
and persisted in `localStorage('oxidgene-pedigree-view')` under the view's own
name. The tree (§1–§9) is the default, and a name no longer shipped falls back
to it. The choice applies to the tree canvas only: the mini-pedigrees of the
profile and the search results stay trees.

Every view reads the same pedigree, fetched as §1 describes; none needs data
the tree does not. Every view keeps what surrounds the canvas — topbar, icon
sidebar, events sidebar, action picker — and the canvas interactions of §5:
drag to pan, wheel and buttons to zoom, fit, and the events sidebar showing
the selected person. Changing view refits the canvas. Every view is drawn in
SVG from the pedigree theme's CSS variables, so it follows the light or dark
palette and the pedigree theme's colours and type, and prints like the tree.

### What a view draws, and its controls

| View | Draws | Ancestor depth | Descendant depth |
|------|-------|----------------|------------------|
| Tree | Ancestors above, descendants below (§1–§9) | yes | yes |
| Ancestor wheel | Ancestors only, a full circle | yes | hidden |
| Fan chart | Ancestors only, the upper half circle | yes | hidden |
| Descendant wheel | Descendants only, a full circle, their unions between the generations | hidden | yes |
| Descendant fan | Descendants only, the lower half circle, their unions between the generations | hidden | yes |
| Lineage | Ancestors only, one column per generation; spouses and children listed from a button (Gramps) | yes | hidden |
| Descendant lineage | Descendants only, one column per generation, spouses under each person (Gramps' Descendant Tree) | hidden | yes |
| Hourglass | Descendants on the left, the root in the middle, ancestors on the right (webtrees' horizontal hourglass) | yes | yes |
| Bowtie | Ancestors only: the father's line on the left, the mother's on the right, the root between them | yes | hidden |

### Ancestor wheel and fan chart

The focus person sits in a disc at the centre — a half disc on the base of the
fan — and each generation of ancestors is a ring around it, one ring further
out per generation. A ring is divided into one segment per SOSA position
counted from the focus person: generation *g* has 2^*g* equal segments, and
SOSA *n* takes the segment *n* − 2^*g* in order. The father's side therefore
fills the first half of every ring and the mother's side the second, so a
line of ancestors stays in one wedge: on the wheel the father's side is the
left half (from six o'clock round to twelve), on the fan the left quarter
circle. The root's disc is filled with `var(--pn-root-bg)` and set in white.

- **Segments.** Tinted by sex from `var(--pn-male-line)` and
  `var(--pn-female-line)` mixed into `var(--pn-bg)`, outlined in
  `var(--pn-border)`, hovered in `var(--pn-hover-bg)`.
- **SOSA and self marks.** The SOSA 1 carries a band in `var(--pn-sosa-root)`
  along the inner edge of its segment, the user's own person one in
  `var(--pn-self)` (which wins, as on the card). A direct ancestor of the SOSA
  1 carries no mark here: everyone an ancestor view draws is an ancestor of
  its focus person, so that mark would say nothing. When the focus person *is* the SOSA 1, a
  segment's hover also gives its SOSA number, which is then its position.
- **Missing parents.** A person short of the last generation whose father or
  mother the tree does not record gets a dashed empty segment in that
  parent's place, with a `+` when it is wide enough; clicking it opens the
  add-parent form as the empty card does. Nothing is drawn beyond it.
- **Text.** Names follow the card's pieces — given name, surname in
  uppercase, lifespan with its precision marks (§3). While a ring's segments
  are wide enough for a straight line (a chord of at least 110 px), text runs
  across the segment, square to its radius, and is turned over on the lower
  half so it never reads upside down. Further out it runs along the radius,
  outwards on the right half and inwards on the left, always in the classic
  three lines. Each line is truncated with an ellipsis to the room it has,
  and a lifespan that overruns even in its narrow form is compressed rather
  than cut. The hover text always has the full name and the spelled-out
  lifespan.
- **Legibility at depth.** Every ring is only as deep as its names need, so
  the chart stays compact however many generations it holds. A segment too
  narrow for the three lines at their own size writes them smaller, in
  proportion, with as many more characters as the smaller type leaves room
  for along the radius: the deep rings read as a printed wheel does, and
  zooming in on a part of them shows its segments with the classic label.
  The chart's zoom therefore goes past the tree view's 4x, as far as its most
  reduced label needs to reach its classic size (at most 16x).
- **Interactions.** Clicking an ancestor's segment makes them the focus and
  redraws the chart around them, as clicking a card does; a right click opens
  the action picker. The root's disc has nowhere to navigate to, so clicking
  it opens the action picker.
- Only the segments near the viewport are drawn, as the tree view's cards
  are (§1); the root's disc always is.

### Descendant wheel and descendant fan

The focus person sits in the disc at the centre — a half disc hanging from
the top of the descendant fan, which opens downwards as descendants sit below
the root in the tree — and each generation of descendants is a ring further
out: the children on the first, the grandchildren on the next, as deep as the
descendant depth. The descendant wheel starts at twelve o'clock and runs
clockwise; the fan runs from nine o'clock round to three, so siblings read
left to right as in the tree.

- **Unions.** Between a generation and the next, a 20 px ring holds the
  unions of the persons inside it, in the order of their families: one
  neutral segment per couple, spanning the children it had and naming the
  spouse (`SURNAME Given`), as Gramps' descendant fan does. A union with no
  children is drawn too, a union's children always together under it. An
  unknown spouse is named *Unknown spouse* on hover and leads nowhere.
- **Shares.** A person's arc is divided among their unions, and a union's
  among its children, in proportion to how many descendants each holds within
  the depth drawn (a person or a childless union counts one), so a large
  family gets room and an empty one does not take it.
- **Segments and text** follow the ancestor wheel: tinted by sex, written
  across a segment with room for a straight line and along the radius
  otherwise, the classic three lines reduced in a narrow segment and read by
  zooming in, the zoom going as far as that needs. A union's name is reduced
  the same way when its band is short. The SOSA 1 and self marks are drawn,
  and so is the direct-ancestor mark: among descendants it traces the line
  that leads to the tree's SOSA 1.
- **Interactions.** Clicking a person makes them the focus; clicking a
  union makes the spouse the focus; a right click opens the action picker;
  the root's disc opens the action picker. Only the segments near the
  viewport are drawn.
- A person who turns out to be their own descendant, in erroneous data, is
  not expanded again, so the chart always ends.

### Descendant lineage, hourglass and bowtie

Three more horizontal charts, built from the lineage view's parts: its cards
and slim boxes, its elbow lines, its culling, its action picker. A side of
ancestors places each by SOSA number as the lineage does; a side of
descendants gives each person as much height as their descendants take.

- **Descendant lineage** (Gramps' Descendant Tree): the focus person on the
  left, each generation of descendants a column to the right. Under each
  person's card, one slim box per union names the spouse (`⚭ SURNAME Given`,
  *?* when unknown), leaving room for the pencil under the focus card; a line
  joins that box to the union's children in the next column, stacked in the
  order of the unions and centred on their part, which the person's block is
  centred on too. Full cards while the chart stays within 1,600 px, slim
  two-line boxes beyond. Clicking a spouse makes them the focus. The fit
  starts it at the left margin, as the lineage.
- **Hourglass** (webtrees' horizontal hourglass): the focus person in the
  middle, their ancestors to the right exactly as the lineage view draws them,
  their descendants to the left as the descendant lineage does, mirrored and
  centred on the focus person's row. The focus person is drawn once, their
  spouses under their card. The direct-ancestor mark is left off the ancestor
  side and kept on the descendant side, where it traces the line to the
  SOSA 1.
- **Bowtie**: the focus person in the middle, level with their parents; the
  father's ancestors to the left and the mother's to the right, each side a
  lineage of its own over half the last generation's rows — to compare the
  two branches. Missing parents are empty slots, as in the lineage.

### Lineage

The lineage view reproduces the principle of the *Pedigree* view Gramps opens
its Charts category on: the focus person on the left, ancestors extending to
the right one column per generation, each child joined to its father above
and its mother below by right-angled elbow lines.

- **Fixed rows.** As in Gramps, a position does not move with what is known
  about the others. The last column is divided evenly and every child is
  centred halfway between its two parents' rows, so the focus person sits in
  the middle of the first column and SOSA *n* in row *n* − 2^*g* of column
  *g*.
- **Boxes.** A column whose rows have room for them holds the pedigree
  theme's own cards (§3): portrait, names, lifespan with its precision marks,
  the SOSA 1 and self badges (not the direct-ancestor badge, which every box
  would carry, as in the wheel), the pencil under the focus card and the dashed empty
  card for a missing parent. While the whole chart stays within 1,600 px at
  full cards, every column holds them. Deeper, the last column's rows tighten
  to 30 px and the columns without room for a card hold slim boxes instead,
  the way Gramps' boxes shrink with the generations: two lines (name, then
  lifespan) where a row has room for them, one line (name) beyond. A slim box
  keeps the sex-coded rule on its left edge and carries the SOSA or self mark
  as a dot on its right; its hover gives the full name, the spelled-out
  lifespan and, when the focus person is the SOSA 1, the SOSA number.
- **Lines.** Connectors use the pedigree theme's connector style class. A
  child who is not the birth child of the family it descends through —
  adopted, fostered — is joined by a dashed line, as Gramps draws a non-birth
  relationship.
- **Spouses and descendants.** Gramps draws neither in this view: a button
  beside the active person lists their children, children who have children
  of their own set in bold, and picking one makes them the active person. The
  lineage view does the same, spouses included. The button, `‹`, stands left
  of the focus card when the focus person has a spouse or a child; it opens a
  menu of their spouses, in the order of their unions, then of their
  children, in the order of the unions and then of births, each with their
  lifespan. Choosing one makes them the focus. The children listed are those the pedigree already carries
  for the focus person's own families, which it always does whatever the
  descendant depth. The descendant depth control is therefore hidden, as in
  the wheel and the fan.
- **Interactions.** Clicking a card or a box makes that person the focus — the
  same as Gramps' buttons jumping to a father or mother; a right click, or the
  pencil under the focus card, opens the action picker.
- Only the boxes and lines near the viewport are drawn, as the tree view's
  cards are (§1); a chart ten generations deep is read by zooming and
  panning, as a deep tree is.
