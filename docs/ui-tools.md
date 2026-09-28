---
type: "UI Specification"
title: "Visual & Functional Specifications — Tools"
description: "Tree tools page in tabs, one tool each: the completeness of the ancestry generation by generation from the SOSA root, and a converter of dates between the calendars the application records dates in."
tags: [oxidgene, specification, ui, tools]
generated: { by: claude-code/claude-opus-5-5, at: 2026-09-28T17:00:00Z }
---

# Visual & Functional Specifications — Tools

> Part of the [OxidGene Specifications](index.md).
> See also: [Statistics](ui-statistics.md) · [Common UI](ui-common.md) · [Person Edit Modal](ui-person-edit-modal.md) (date input) · [Cross-cutting Rules](cross-cutting.md)

---

## 1. Overview

The Tools page (`/trees/{id}/tools`) gathers what the author of a tree uses
to check it and to work on it, rather than to read it.

It is reached from the **wrench icon** of the shared left icon sidebar
(`TreeIconSidebar`), right after the Statistics chart icon and before the
Gear, so it opens the same way from the pedigree canvas, a profile, and every
other tree page. Like [Statistics](ui-statistics.md), it uses the `sub-page`
layout with no sidebar of its own.

## 2. Layout

```
+----------------------------------------------------------------------+
| [logo] tree / Tools                                                   |
+----------------------------------------------------------------------+
| Ancestry completeness | Date converter                                |
+----------------------------------------------------------------------+
|  Tool title                                                           |
|  What the tool does, in a sentence or two                             |
|  [the tool]                                                           |
+----------------------------------------------------------------------+
```

The page is split into **tabs**, one per tool, so the tools are never all on
screen together:

| Tab | Tool |
|-----|------|
| Ancestry completeness | Generation by generation from the SOSA root, who is known and which key facts are recorded (§3) |
| Date converter | A date in one calendar, read in every other (§4) |

The tabs are the [Statistics](ui-statistics.md) strip: at phone width
(640px and below) they scroll sideways instead of shrinking. The tab shown
is kept per viewer in local storage; the first visit opens the first tab.

Each tab is mounted only while it is shown, and asks the server for its data
when it is opened: opening one tab never loads another's data. A tool that
needs no data from the server, such as the date converter, asks for none.
While the browser has not yet said which tab the viewer left the page on,
no tab is shown.

Each tab opens with the tool's title and a sentence saying what it does.

## 3. Ancestry completeness

Generation by generation from the tree's SOSA root (set in the
[Settings](ui-settings.md) §7), which ancestors are known and which of their
key facts are recorded. The SOSA numbers are those of the pedigree: 1 is the
root, 2n the father and 2n + 1 the mother of n.

- **Depth**: from 4 to 15 generations, the root's included; 8 by default.
  The choice is kept per viewer in local storage.
- **Summary**: one row per generation with the ancestors found against the
  ancestors expected (2^(n − 1) in generation n), the share as a bar (green
  above 70%, orange from 40%, red below), and how many of those found have a
  birth, a death (or may be alive) and a union.
- **Generations**: below the summary, each generation (the first three open)
  lists by SOSA number either the ancestor, a link to their profile with
  their birth and death dates, and three marks, or "missing ancestor".
- **Key facts**, each recorded when its event exists with a date or a place:
  - *birth*: a birth or a baptism;
  - *death*: a death or a burial; a person with neither, born fewer than 120
    years ago, may be alive and is marked so rather than missing (the
    [Statistics](ui-statistics.md) §7 rule);
  - *union*: an event attesting the union of the ancestor with the other
    parent of their child in the line (the family that makes them a parent
    of the ancestor below): a marriage, a civil union, banns, a contract, a
    licence or a settlement, or a separation, a divorce filed, a divorce or
    an annulment. The root has no such union and shows no mark.
- **Missing ancestors**: only the missing parents of known ancestors are
  listed. The ancestors of a missing ancestor are missing too; they are
  counted under each generation ("12 more ancestors are missing because their
  children are too") instead of being listed, so an empty branch costs
  nothing whatever the depth.
- **Pedigree collapse**: an ancestor reached through two lines appears, and
  counts, at each of their SOSA numbers.
- "Show only what is missing" keeps the missing ancestors and those lacking
  a fact.
- A tree without a SOSA root shows why, with a link to the settings where
  one is chosen.

The data comes from `GET /trees/{id}/ancestry-completeness` or the
`ancestryCompleteness` query ([API](api.md)), computed from the person
projections on each request.

## 4. Date converter

A date entered in any calendar the application records dates in, read in
all of them: Gregorian, Julian, Hebrew and French Republican.

- The date is entered with the shared date input of the
  [Person Edit Modal](ui-person-edit-modal.md): calendar, qualifier, day,
  month and year, and a second date for "or" and "between". Its own checks
  apply, and a date it refuses converts to nothing.
- One tile per calendar shows the date written as the application writes
  dates in that calendar, qualifier included ("about 21 ventôse 4"). The
  calendar the date was entered in is marked "as entered".
- A partial date keeps its precision: a year alone is read as the year it
  overlaps most, a month as the month, the same rule as when the calendar of
  an entered date is changed in the date input.
- A calendar that cannot express the date — the Republican calendar before
  its first year (22 September 1792), a range one of whose ends it cannot
  express — says "not expressible in this calendar" instead of a date.
- Under the tiles, a single complete date that is neither approximate nor a
  range gives its day of the week.
- Everything is computed in the browser from the calendar arithmetic of
  `oxidgene-core`; nothing is sent to the server, and nothing is stored.

## 5. i18n and accessibility

Every title, explanation, label and message goes through i18n in every
interface language. The tabs are a `tablist` whose buttons say which is
selected.
