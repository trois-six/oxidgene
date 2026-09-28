---
type: "UI Specification"
title: "Visual & Functional Specifications — Statistics"
description: "Tree statistics page: an overview with completeness and averages, a heat map of places with births by country, region and subdivision, names, demographic charts per period under a year ruler, event and family distributions, the tree's records, and notable lists."
tags: [oxidgene, specification, ui, statistics]
generated: { by: claude-code/claude-opus-5-5, at: 2026-09-28T11:47:00Z }
---

# Visual & Functional Specifications — Statistics

> Part of the [OxidGene Specifications](index.md).
> See also: [Genealogy Tree](ui-genealogy-tree.md) · [Dictionary](ui-dictionary.md) · [Place Dictionary](place-dictionary.md) · [API Contract](api.md) · [Common UI](ui-common.md) · [Cross-cutting Rules](cross-cutting.md)

---

## 1. Overview

The Statistics page (`/trees/{id}/statistics`) shows what a tree says as a
whole: how complete it is, where its people lived, how they were named, what
they did, how long they lived, when they married and how many children they
had, century after century, and the records worth a look (the extremes of
the tree, the latest events, the longest lives, the largest families).

It is reached from the **chart icon** of the shared left icon sidebar
(`TreeIconSidebar`), between the Book/index and Gear buttons, so it opens the
same way from the pedigree canvas and from a person or couple profile. Like
the [Dictionary](ui-dictionary.md), it uses the `sub-page` layout with no
sidebar of its own, and it is read-only.

Everything is computed by the backend from the tree's person projections and
place usages on each visit ([API](api.md)); nothing is stored. The page asks
once per visit: the time series come filed by year, and the page itself
groups them into periods, so changing the interval or the years shown needs
no new request. Only switching approximate dates on or off, or the interface
language (which names the countries), asks again.

## 2. Layout

```
+----------------------------------------------------------------------+
| NAVBAR                                                                |
| [logo] tree / Statistics   [x] Approximate dates  Interval: [25 v]  |
+----------------------------------------------------------------------+
|  Overview       [persons] [unions] [places] [sources] [years] ...    |
|                 [% dated births] [% dated deaths] [% no parent] ...  |
|                 [age at death] [first union] [generation] [children] |
+----------------------------------------------------------------------+
|  Places                                                               |
|  +---------------------------------------+  1. Place A          412  |
|  |   heat map over country borders       |  2. Place B          201  |
|  |   (numbered markers for the top 10)   |  ...                       |
|  +---------------------------------------+  38 places not located     |
|  [births by country] [births by region] [births by subdivision]      |
+----------------------------------------------------------------------+
|  Names and occupations  [donut] [donut] [donut] [donut] [rare names] |
+----------------------------------------------------------------------+
|  Years 1712–1850  |--1700---[====1750=====1800====]--1850--| [All]  |
|  Persons            [lines] [line] [lines] [lines] [lines] ...       |
|  Families           [lines] [lines] [lines] [line] [line] ...        |
+----------------------------------------------------------------------+
|  Events and families    [event types donut] [children per union]     |
|  Records                [card] [card] [card] ...                     |
|  Notable records    tabs: latest births · unions · deaths · oldest   |
|        possibly alive · longest lives · largest families · pyramid   |
+----------------------------------------------------------------------+
```

Charts sit two per row on wide screens and one per row below 900px. Each has
a title and a `?` hint explaining how it is computed (§7).

The **interval** selector (10, 25, 50 or 100 years; 25 by default) sets the
width of the periods every time chart groups its values by. The
**approximate dates** box lets ages and averages also use dates about,
calculated or estimated (§7); it is off by default. Both choices are kept
per viewer in local storage.

The **year ruler**, above the Persons and Families sections, chooses the
first and the last year their period charts cover. It spans the years any
of those charts has a value for; two handles move to the year, by pointer
or by the arrow keys, and never cross. Its ticks mark the multiples of the
interval, labelled so that no more than twelve labels show. The chosen
years are printed beside it and **All years** restores the whole span. The
ruler stays pinned at the top while those two sections scroll by. The
range lasts for the visit; it is not stored. The overview, the map, the
donuts, the records and the lists do not depend on it.

## 3. Key figures

Three rows of tiles, each a figure with what it counts:

- **Counts**: persons (with men, women and unknown sex), unions, places,
  sources, the years covered (the first and the last year of any dated
  event), and the distinct family names and first given names.
- **Completeness**, as a share of all persons: with a dated birth (or
  baptism), with a dated death (or burial), without a known parent, without
  children (parent of no one, through a family or a union with children),
  and never in a union.
- **Averages**, each with its median, standard deviation and range: the age
  at death and the age at the first union (both also for men and for
  women), the generation interval (the parents' age at the birth of each of
  their children) and the children per union.

## 4. Places

### 4.1 Heat map

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

### 4.2 Top ten

Beside the map, the ten places with the most usages, each with its count and
a number repeated as a marker on the map. A place that could not be located
is still listed, without a marker. Below the list, the number of places that
could not be located.

### 4.3 Births by area

Three donuts count the births (or baptisms) by the country, the region and
the subdivision (département, county, district…) the
[place dictionary](place-dictionary.md) puts their place in, the way §4.1
locates it, the ten largest each. A bare homonym read in the tree's main
country has its country, and its region and subdivision when all its
candidates there share them. Countries are named in the interface language.
Under each donut, how many distinct countries, regions or subdivisions the
tree's used places span.

## 5. Names and occupations

Donuts of the ten most common family names, occupations, men's first given
names and women's first given names, then the **given names carried once**:
the first given names only one person carries, for men and for women, with
how many there are.

## 6. Charts

The period charts sit under the year ruler (§2):

| Section | Chart | Kind |
|---------|-------|------|
| Persons | Births, baptisms, unions, deaths and burials per period | Lines (5) |
| Persons | Sex ratio at birth: boys born per 100 girls born | Line |
| Persons | Average age at death, men and women, per period of death | Lines |
| Persons | Average age at death, men and women, per period of birth (by generation) | Lines |
| Persons | Infant and child mortality: share of the births followed by a death before one and before five years | Lines (2) |
| Persons | Births by month, share of each month per period | Lines (12) |
| Persons | Parents' average age at the first and at the last child, per period of that birth | Lines (4) |
| Persons | Fathers' and mothers' average age at each child's birth, per period of that birth | Lines (2) |
| Families | Average age at the first union, men and women, per period of the union | Lines |
| Families | Unions by weekday, share per period | Lines (7) |
| Families | Unions by month, share per period | Lines (12) |
| Families | Average duration of a union, per period of the union | Line |
| Families | Average number of children per union, per period of the union | Line |
| Families | Average time between two births in a family, in months | Line |
| Families | Average gap between the first and the last child, in months | Line |
| Families | Average age difference between spouses, in months | Line |

Below them, **Events and families** holds a donut of the ten most frequent
event types (each event counted once, a family event once however many
spouses carry it, with the total under it) and a bar chart of the unions by
number of children.

Donuts show a legend with each value and its count. Line charts share one
x-axis of periods, skip periods without data rather than drawing them as
zero, and show the value of a point on hover. Their value axis runs from
zero in at most four round steps (1, 2, 2.5 or 5 times a power of ten), and
a chart with a single line has no legend: its title names it. Series colors come from the
theme (men and women use the pedigree's male and female colors; months and
weekdays the chart palette).

## 7. Rules

- **Birth and death** fall back to baptism and burial when those carry the
  only date, as everywhere else ([Architecture invariants](architecture.md)).
- **Ages** need both dates with at least a year; an age is the difference
  of the two dates, in years or months. Only exact dates count, and also
  dates about, calculated or estimated when approximate dates are switched
  on; before, after, perhaps and ranges never do.
- **Counts of events** per period (the events chart, the sex ratio) take
  every dated event whatever its qualifier, in the year of its date.
- **Mortality** counts, among the persons whose birth is dated by the
  rule above, those whose death is too and comes before their first or
  their fifth birthday. A person without a recorded death counts as
  surviving.
- **Months and weekdays** need a date precise to the month (for months) or
  the day (for weekdays), by the same rule, whatever its calendar: the date
  is read in its Gregorian equivalent.
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
  **occupation** every occupation of a person counted once. Names are
  compared case aside and shown in their most frequent spelling.

## 8. Records

Cards, one per record the tree has, each with its holder (links to their
profiles), its value and the date it is about. Ages and durations follow
§7 and read in days under a month, in months under a year, else in years.
Ties go to the first found.

| Record | Holder | Value |
|--------|--------|-------|
| Longest-lived man, longest-lived woman | The person | Age at death |
| Earliest birth, latest birth | The person | The birth date, any qualifier |
| Youngest and oldest at a union | The spouse | Age at the union |
| Longest union | The couple | Duration (§7) |
| Most unions (more than one) | The person | Unions |
| First and last recorded union | The couple | The union date, any qualifier |
| Most children | The parent | Children, and the unions with children they had them in |
| Oldest and youngest at a first child | The parent | Age at the first child's birth |
| Youngest death | The person | Age at death |
| Largest age gap between spouses | The couple | Gap between their births |
| Most places | The person | Distinct places of their events and their unions' events |
| Longest widowhood | The survivor, then the spouse they outlived | From the first death to the second, for a dated union before the first death that no divorce or annulment ended first |
| Largest gap between siblings | The eldest and the youngest of one family | Gap between their births |
| Most generations of descendants | The person | Generations below them, children counting one |

## 9. Notable records

Tabs, each a list of up to 100 persons with their name (a link to their
profile), the date and the place:

- **Latest births**, **latest unions** (both spouses) and **latest deaths**,
  most recent first.
- **Oldest possibly alive**: the persons of §7, oldest first, with their age.
- **Longest lives**: the greatest ages at death, with birth and death dates.
- **Largest families**: the unions with the most children, with both
  spouses, the union date and the number of children.
- **Age pyramid**: persons by age at death in five-year bands, men to the
  left and women to the right.

## 10. States

- While loading, the charts show the shared loading placeholder.
- A chart without any data shows "Not enough dated records" instead of an
  empty frame.
- A tree without any located place shows the map empty with the reason.
- A tree without any record shows no Records section.

## 11. i18n and accessibility

Every title, hint, legend, axis label, month and weekday name goes through
i18n in every interface language. Charts carry an accessible title and a
text summary of their values; colors are never the only way to tell series
apart (the legend and the hover label name them).
