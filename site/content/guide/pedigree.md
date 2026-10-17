+++
title = "The genealogy tree"
description = "Read and move around the pedigree, act on a person from their card, follow their events, switch to another chart, and print."
weight = 40
+++

The tree view is where you spend most of your time: it draws the person you are looking at, their ancestors above and their descendants below, and lets you move from one relative to the next. This page explains how to read the cards, move around, act on a person, read the events panel, choose another chart or theme, and print.

{{ <screenshot name="pedigree" alt="Pedigree of the Landrevel family with portraits and the events panel" lang={lang} /> }}

## Read the tree

The person in focus sits in the middle of the canvas. Each generation of ancestors takes one row above them, each generation of descendants one row below. Every card shown is connected to the focus person by a chain of parents, children or spouses: a person with no link to them is reached through [search](@/guide/search.md), never drawn loose on the canvas.

Lines join each couple and drop to their children. When someone has several unions, each union has its own line, and the children of each hang from the right partner.

### What a card shows

- **Portrait.** The person's profile photo, or a silhouette for a man, a woman or a person of unknown sex.
- **Names.** Given names, then the surname in capitals.
- **Lifespan.** Birth and death years only. When the birth has no date, the baptism is used; when the death has no date, the burial is used.
- **Sex.** A short coloured rule beside the portrait in the classic theme; the outline of the shield in the medieval theme.
- **Focus.** The focus person's card is filled with colour rather than outlined.
- **Spouses.** Spouse cards have a slightly different background.
- **More relations.** A small blue **+** marks a person who has relatives outside what the chart currently draws.

A year alone can hide how uncertain a date is, so the card adds the precision mark the record carries:

| Mark | Meaning | Example |
|---|---|---|
| none | exact | `1849` |
| `ca` | about, calculated, estimated, or derived from an age | `ca 1849` |
| `?` | perhaps | `? 1849` |
| `<` | before | `< 1917` |
| `>` | after | `> 1912` |
| `\|` | one date or the other | `1849\|1852` |
| `..` | between two dates | `1691..1693` |

So `ca 1849-< 1917` reads "born about 1849, died before 1917". When a range does not fit on the card, only its first year is kept with its mark (`.. 1691`). Hover the dates to read them spelled out in your language.

### Badges

- **SOSA badge.** A small disc at the bottom-right of the portrait marks the SOSA root of the tree (it carries the digit `1`) and each of their ancestors.
- **"Me" badge.** The person you chose under **Settings** → **Tree & Roots** → **Who am I?** carries a disc with a solid centre. It wins over the SOSA badge so you can always find yourself. See [settings](@/guide/settings.md).

### Empty slots

A dashed card with a **+** stands for a parent the tree does not record yet, at every generation, and for a spouse a couple does not name. Click its **+** to create that parent: see [Editing persons](@/guide/editing.md) and the workflow [Record a new ancestor](@/workflows/record-a-new-ancestor.md).

## Move around the tree

- **Change the focus.** Click a card. The chart recentres on that person, a pencil appears under their card, and the events panel shows their events.
- **Pan.** Drag anywhere on the canvas, cards included.
- **Zoom.** Use the mouse wheel or a pinch to zoom around the pointer, or the **Zoom in** and **Zoom out** buttons of the left sidebar. The tree view zooms from 0.3× to 4×.
- **Fit.** **Fit to screen** frames the whole chart in the window. When the tree is too large to fit, it is centred on the focus person.
- **Change the depth.** Hover **Generation depth** in the left sidebar. A small panel opens with two counters, ancestors (up arrow) and descendants (down arrow), each from 0 to 10. Every **+** or **−** redraws the chart at once.

The depth a new tree opens with is 4 generations of ancestors and 3 of descendants; change it under **Pedigree depth** in the application [settings](@/guide/settings.md). Once you have moved around a tree, its view (focus person, depths, pan and zoom) is kept for as long as the window is open: leave for another page and come back, and the chart is as you left it. The **Back** button also brings you back to the person the chart was drawn around.

## Find a person from the top bar

The top bar has two fields, **Last name / # SOSA** and **First name**. Use either or both.

1. Start typing. A panel lists up to five matching names from the tree, with the number of persons who bear them, then up to six matching persons.
2. Pick a name to complete the field, or a person to centre the chart on them.
3. Press Enter, or click the magnifying glass, to open the full search results.

Type a number alone in **Last name / # SOSA** to jump to that SOSA number. The [search page](@/guide/search.md) covers the results and their filters.

## Act on a person

Click the pencil under the focus card, or right-click any card, to open the action picker:

| Action | What it does |
|---|---|
| **Edit individual** | Opens the person form |
| **Merge with…** | Opens the merge wizard to fold another record of the same person into this one |
| **Edit union** | Opens the couple form; with several unions, first choose which one |
| **Add spouse**, **Add child**, **Add sibling** | Search the tree for the relative to link, or create a new person |
| **Relationship with…** | Opens the kinship page between this person and another |
| **Go to…** | In the charts other than the tree: lists relatives the chart does not draw, such as spouses and children in an ancestor chart |

The forms are described in [Editing persons](@/guide/editing.md), the merge in [Tools](@/guide/tools.md), and the relationship page in [Kinship](@/guide/kinship.md).

## Follow a person's events

The panel on the right shows the selected person: their portrait, their name, and the same lifespan as their card. Below, their events are listed by year, undated events first under **Undated**. The list also gives the context of their life: their unions and their children's births and deaths, their parents' family events, their parents' deaths and burials, and their siblings' births and deaths. An event that belongs to someone else names that person in brackets; a union event names the partner.

Click an event to open the person's page, where its sources and notes are shown.

- **Resize** the panel by dragging its left edge. The chart refits when you let go.
- **Hide** it with the toggle on its left edge (**Hide events**), and click again to reopen it. Its width and whether it is open are remembered.
- On a narrow window the panel collapses by itself; on a very narrow one it is hidden.

## The left sidebar

From top to bottom: **Profile view** (the selected person's page), **Couple view** (their earliest couple, shown only when they have a known spouse), **Tree view**, **Generation depth**, **Zoom in**, **Zoom out**, **Fit to screen**, **Add person** (a person with no family link yet), then **Dictionary**, **Statistics**, **Tools**, **Print** and **Settings**.

The same sidebar appears on the person and couple pages and on the other pages of the tree, so you can always switch between the profile, the couple and the chart of the same person.

## Choose another chart

The tree is one of nine ways to draw the same pedigree. Pick one in the application settings, under **Pedigree** → **Chart**. The choice applies to the tree view on this device; the small pedigrees of the person page and of search results stay trees.

| Chart | What it draws |
|---|---|
| **Tree** | Ancestors above, descendants below, one row per generation |
| **Ancestor wheel** | The person at the centre, each generation of ancestors a ring around them |
| **Fan chart** | The ancestors over a half circle, the person at its base |
| **Descendant wheel** | The person at the centre, each generation of descendants a ring, their unions between |
| **Descendant fan** | The descendants over a half circle opening downwards |
| **Lineage** | The person on the left, one column of ancestors per generation, as in Gramps |
| **Descendant lineage** | The person on the left, one column of descendants per generation, spouses under each |
| **Hourglass** | Descendants on the left, the person in the middle, ancestors on the right |
| **Bowtie** | The father's line on the left, the mother's on the right, the person between them |

{{ <screenshot name="fan-chart-dark" alt="Fan chart of the Landrevel ancestors in the dark theme" lang={lang} /> }}

Every chart keeps the top bar, the sidebar, the events panel and the action picker, and works the same way: drag to pan, zoom, fit, click a person to make them the focus, right-click for the action picker. A few differences:

- Charts that draw only ancestors hide the descendant counter of **Generation depth**, and those that draw only descendants hide the ancestor counter.
- In the wheels and fans, the father's side fills the first half of every ring and the mother's side the second. A missing parent is a dashed segment with a **+** when there is room for it. Deep rings use smaller text: zoom in (up to 16×) to read them. Hover a segment for the full name and dates.
- In the descendant wheel and fan, a thin ring between generations names each spouse; click it to move to that spouse.
- In the lineage chart, a **‹** button beside the focus card lists their spouses and children; pick one to move to them. In the other charts, use **Go to…** in the action picker.

{{ <screenshot name="hourglass-medieval" alt="Hourglass chart of the Landrevel family in the medieval theme" lang={lang} /> }}

## Change the chart theme

Under **Pedigree** → **Pedigree theme** in the application settings, choose how cards and lines are drawn:

- **Classic**: plain cards and curved connectors, following the application theme.
- **Medieval**: an engraved chart, each person in a heraldic shield with a round portrait, joined by ruled lines.

The theme applies on this device to every pedigree: the tree view, the small pedigrees of the person page and the search results. It is separate from the application's colour theme, which still sets light or dark.

## Print the chart

Click **Print** in the left sidebar, or press Ctrl+P (Cmd+P on a Mac).

- When the whole chart is on screen, it prints at once on one landscape sheet, with the tree name and the date as a header.
- When part of it is off screen and it is larger than a sheet at the current zoom, OxidGene first asks what to print: **What the screen shows** (one sheet) or **The whole chart at this zoom**, with the number of sheets it takes. The sheets overlap by 1 cm up to a dashed line, so you can cut or align them on that line to assemble the chart. Beyond 50 sheets, zoom out to print fewer.

The sidebars, the zoom and depth controls, the events panel and the pencil do not print. To print more or less of the tree, zoom or pan first.

Learn more: [genealogy tree specification](@/tech/ui-genealogy-tree.md).
