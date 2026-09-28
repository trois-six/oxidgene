---
type: "UI Specification"
title: "Visual & Functional Specifications — Statistics"
description: "Tree statistics page: a heat map of where the tree's events happened with its ten most used places, demographic charts per period, and the notable lists of births, unions, deaths and long lives."
tags: [oxidgene, specification, ui, statistics]
generated: { by: claude-code/claude-opus-5-5, at: 2026-09-28T11:20:00Z }
---

# Visual & Functional Specifications — Statistics

> Part of the [OxidGene Specifications](index.md).
> See also: [Genealogy Tree](ui-genealogy-tree.md) · [Dictionary](ui-dictionary.md) · [Place Dictionary](place-dictionary.md) · [API Contract](api.md) · [Common UI](ui-common.md) · [Cross-cutting Rules](cross-cutting.md)

---

## 1. Overview

The Statistics page (`/trees/{id}/statistics`) shows what a tree says as a
whole: where its people lived, how they were named, what they did, how long
they lived, when they married and how many children they had, century after
century, and the records worth a look (the latest events, the longest lives).

It is reached from the **chart icon** of the shared left icon sidebar
(`TreeIconSidebar`), between the Book/index and Gear buttons, so it opens the
same way from the pedigree canvas and from a person or couple profile. Like
the [Dictionary](ui-dictionary.md), it uses the `sub-page` layout with no
sidebar of its own, and it is read-only.

Everything is computed by the backend from the tree's person projections and
place usages on each visit ([API](api.md)); nothing is stored. The page asks
once per visit: the time series come filed by year, and the page itself
groups them into periods, so changing the interval or the years shown needs
no new request.

## 2. Layout

```
+----------------------------------------------------------------------+
| NAVBAR                                                                |
| [logo] tree_name / Statistics                 Interval: [25 years v] |
+----------------------------------------------------------------------+
| 1 234 persons · 612 women · 598 men · 431 unions · 287 places        |
+----------------------------------------------------------------------+
|  Places                                                               |
|  +---------------------------------------+  1. Place A          412  |
|  |   heat map over country borders       |  2. Place B          201  |
|  |   (numbered markers for the top 10)   |  ...                       |
|  +---------------------------------------+  38 places not located     |
+----------------------------------------------------------------------+
|  Years 1712–1850  |--1700---[====1750=====1800====]--1850--| [All]  |
|  Persons            [donut] [donut] [line] [donut] [lines] [lines]   |
|  Families           [lines] [lines] [lines] [line] [line] ...        |
|  Notable records    tabs: latest births · unions · deaths ·           |
|                     oldest possibly alive · longest lives · pyramid  |
+----------------------------------------------------------------------+
```

Charts sit two per row on wide screens and one per row below 900px. Each has
a title and a `?` hint explaining how it is computed (§5).

The **interval** selector (10, 25, 50 or 100 years; 25 by default) sets the
width of the periods every time chart groups its values by. The choice is
kept per viewer in local storage.

The **year ruler**, above the Persons and Families sections, chooses the
first and the last year their period charts cover. It spans the years any
of those charts has a value for; two handles move to the year, by pointer
or by the arrow keys, and never cross. Its ticks mark the multiples of the
interval, labelled so that no more than twelve labels show. The chosen
years are printed beside it and **All years** restores the whole span. The
ruler stays pinned at the top while those two sections scroll by. The
range lasts for the visit; it is not stored. The donuts, the map, the
records and the pyramid do not depend on it.

## 3. Places

### 3.1 Heat map

- The background is an offline map: the country borders of the
  [basemap](api.md), drawn as SVG in a Mercator projection. No tile server
  and no network access are involved.
- The heat is the tree's place usages (events and media that name a place):
  each located place adds a soft radial spot weighted by its usage count,
  and overlapping spots add up, from the theme's cool to its warm color.
  Precision is deliberately coarse: the map shows regions of activity, not
  villages.
- The view fits the located places with a margin, and can be zoomed and
  panned by wheel, drag and the `+`/`-`/fit buttons.
- A place is located by its own latitude and longitude, or else by its label
  in the [place dictionary](place-dictionary.md). The part before the first
  comma must be a dictionary name (case and accents aside); among its
  homonyms, the one whose code, subdivision, region or country the rest of
  the label names wins. A bare name whose homonyms all stand within a fifth
  of a degree is located there. Any other bare name is read in the country
  the tree's located places use most among those where the name exists,
  preferring a living commune to a settlement and a settlement to a former
  name: "Brest" in a mostly French tree is the city in Finistère. A bare
  name found in none of the tree's countries stays unlocated. The lookup
  happens at each visit and changes nothing in the tree.

### 3.2 Top ten

Beside the map, the ten places with the most usages, each with its count and
a number repeated as a marker on the map. A place that could not be located
is still listed, without a marker. Below the list, the number of places that
could not be located.

## 4. Charts

| Section | Chart | Kind |
|---------|-------|------|
| Persons | 10 most common family names | Donut |
| Persons | 10 most common given names (first given name) | Donut |
| Persons | Average age at death, men and women, per period of death | Lines |
| Persons | 10 most common occupations | Donut |
| Persons | Births by month, share of each month per period | Lines (12) |
| Persons | Parents' average age at the first and at the last child, per period of that birth | Lines (4) |
| Families | Average age at the first union, men and women, per period of the union | Lines |
| Families | Unions by weekday, share per period | Lines (7) |
| Families | Unions by month, share per period | Lines (12) |
| Families | Average duration of a union, per period of the union | Line |
| Families | Average number of children per union, per period of the union | Line |
| Families | Average time between two births in a family, in months | Line |
| Families | Average gap between the first and the last child, in months | Line |
| Families | Average age difference between spouses, in months | Line |

Donuts show a legend with each value and its count. Line charts share one
x-axis of periods, skip periods without data rather than drawing them as
zero, and show the value of a point on hover. Their value axis runs from
zero in at most four round steps (1, 2, 2.5 or 5 times a power of ten), and
a chart with a single line has no legend: its title names it. Series colors come from the
theme (men and women use the pedigree's male and female colors; months and
weekdays the chart palette).

## 5. Rules

- **Birth and death** fall back to baptism and burial when those carry the
  only date, as everywhere else ([Architecture invariants](architecture.md)).
- **Ages** need both dates with at least a year; an age is the difference
  of the two dates, in years or months. Dates with a qualifier other than
  exact, and ranges, are left out of ages and averages.
- **Months and weekdays** need a date precise to the month (for months) or
  the day (for weekdays), exact, whatever its calendar: the date is read in
  its Gregorian equivalent.
- **Periods** are aligned on multiples of the interval (1700–1724,
  1725–1749…) and a value is counted in the period of the event it depends on.
  Only the years the ruler shows count: the first and the last period are
  cut at those years, and the first is labelled by the first year shown
  (1712, 1725, 1750… for a range from 1712).
- **Averages over a period** weigh every value alike: the period's sum over
  its count, whichever years they come from. **Shares** are each category's
  part of all the period's counts.
- **Union** is a family with at least one spouse; its date is the marriage,
  or the first dated family event. Its duration runs to the first death of
  a spouse or to a divorce or annulment, whichever comes first.
- **First union** is a person's earliest dated union.
- **Possibly alive**: no death or burial recorded and born fewer than 120
  years ago.
- **Given name** is the first word of the primary name's given names;
  **occupation** every occupation of a person counted once.

## 6. Notable records

Tabs, each a list of up to 100 persons with their name (a link to their
profile), the date and the place:

- **Latest births**, **latest unions** (both spouses) and **latest deaths**,
  most recent first.
- **Oldest possibly alive**: the persons of §5, oldest first, with their age.
- **Longest lives**: the greatest ages at death, with birth and death dates.
- **Age pyramid**: persons by age at death in five-year bands, men to the
  left and women to the right.

## 7. States

- While loading, the charts show the shared loading placeholder.
- A chart without any data shows "Not enough dated records" instead of an
  empty frame.
- A tree without any located place shows the map empty with the reason.

## 8. i18n and accessibility

Every title, hint, legend, axis label, month and weekday name goes through
i18n in every interface language. Charts carry an accessible title and a
text summary of their values; colors are never the only way to tell series
apart (the legend and the hover label name them).
