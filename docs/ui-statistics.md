---
type: "UI Specification"
title: "Visual & Functional Specifications — Statistics"
description: "Tree statistics page in tabs: an overview with completeness and averages, a heat map of places with births by country, region and subdivision, names, demographic charts per period under a year ruler, event and family distributions, the tree's records, notable lists, and the number of persons the tree held over the days it was worked on."
tags: [oxidgene, specification, ui, statistics]
generated: { by: claude-code/claude-opus-5-5, at: 2026-10-01T11:02:14Z }
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
the tree, the latest events, the longest lives, the largest families). It
also shows how the tree itself grew: how many persons it held on each day
it was worked on.

It is reached from the **chart icon** of the shared left icon sidebar
(`TreeIconSidebar`), between the Book/index and Gear buttons, so it opens the
same way from the pedigree canvas and from a person or couple profile. Like
the [Dictionary](ui-dictionary.md), it uses the `sub-page` layout with the
shared left icon sidebar, whose chart button shows as current here, and it
is read-only.

Everything is computed by the backend on each visit ([API](api.md));
nothing is stored. Two requests feed the tabs, each asked the first time a
tab shown needs it: the statistics, from the tree's person projections and
place usages, for every tab but Growth, and the growth (§10) for the Growth
tab alone. The statistics are asked once per visit: the time series come
filed by year, and the page itself groups them into periods, so changing the
interval or the years shown needs no new request. Only switching approximate
dates on or off, or the interface language (which names the countries),
asks again.

## 2. Layout

```
+----------------------------------------------------------------------+
| [logo] tree / Statistics                        [x] Approximate dates |
+----------------------------------------------------------------------+
| Overview | Population | Families | Places | Names | Records | Growth |
+----------------------------------------------------------------------+
|  (Population and Families tabs only)                                  |
|  Years 1600–2026 |--750--[=====1600=====1800====]--| Interval [25 v] [All] |
+----------------------------------------------------------------------+
|  Block title                                                          |
|  [chart] [chart]                                                      |
|  Block title                                                          |
|  [chart] [chart] ...                                                  |
+----------------------------------------------------------------------+
```

The page is split into **tabs**, one per kind of statistics, each made of
titled blocks:

| Tab | Blocks |
|-----|--------|
| Overview | Counts, Completeness, Averages (§3), Events (the event types donut) |
| Population | Births and deaths (events per period, sex ratio at birth, births by month, mortality); Length of life (age at death by period of death and by generation, age pyramid) |
| Families | Unions (age at the first union, duration, weekdays, months, spouses' age gap); Children (children per union, unions by number of children, birth spacing, first–last child gap, parents' age at the first and last child and at every child) |
| Places | Map of the places (§4.1, §4.2), Births by area (§4.3) |
| Names and occupations | Family names and occupations; Given names (§5) |
| Records and lists | Records (§8), Notable records (§9) |
| Growth | Persons over time: the persons in the tree, and those added and removed, per period of calendar time (§10) |

The tab shown is kept per viewer in local storage; the first visit opens
the Overview. Charts sit two per row on wide screens and one per row below
900px. Each has a title and a `?` hint explaining how it is computed (§7).

At phone width (640px and below) the page stays within the screen: the tabs
scroll sideways instead of shrinking, the counts sit two per row, a donut's
legend goes under its ring, and the period bar puts the years and the
"all years" button on one line, the interval on the next, then the ruler.
Charts are drawn at about half their size there, so their axis text and
marker badges (§10) are drawn larger and the charts and the ruler label
every other period only. A legend
truncates a long value rather than push its count out of the card.

The **approximate dates** box, in the topbar, lets ages and averages also
use dates about, calculated or estimated (§7); it is off by default and
applies to every tab.

The Population and Families tabs open on a bar that governs their period
charts (§6). Its **interval** selector (10, 25, 50 or 100 years; 25 by
default) sets the width of the periods the charts group their values by.
Both choices are kept per viewer in local storage.

The **year ruler**, in the same bar, chooses the first and the last year
the period charts cover. It spans the years any of those charts has a
value for; two handles move to the year, by pointer or by the arrow keys,
and never cross. Its ticks mark the multiples of the interval, labelled so
that no more than twelve labels show. The chosen years are printed beside
it. The first view starts at the first 25-year span holding at least 1% of
the tree's dated events and runs to the last year, so a few early records
(a medieval line of ancestors) do not stretch the axis over sparse
centuries; **All years** widens it to the whole span. The bar stays pinned
at the top while the charts scroll by. The range is shared by the two tabs
and lasts for the visit; it is not stored. The age pyramid and the unions
by number of children cover every year and do not depend on it.

The page prints through the print action of
[Common UI §7](ui-common.md#7-printing). A printout holds the tab shown,
under its name, with the charts as drawn for the page width. The year ruler
prints with the chosen years; its handles, the interval selector and the
approximate-dates box do not, and the `?` hints stay closed.

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
- **Place names** help the reader find their bearings, as on web maps:
  each populated place of the basemap is named from the zoom level Natural
  Earth gives it (a capital from afar, a regional town up close), the view
  standing for a web map zoom of `log2(843.75 / width)`, its width in
  projected degrees. The most important come first (lowest zoom, then the
  most populated); a name is left out where it would overlap one already
  placed or a numbered marker, or not fit in the view, and no more than 20
  show at once. Names are in the interface language when Natural Earth
  has one.
- A place is located by its own latitude and longitude, or else by its label
  in the [place dictionary](place-dictionary.md). The part before the first
  comma must be a dictionary name (case and accents aside); among its
  homonyms, the one whose code, subdivision, region or country the rest of
  the label names wins. A bare name whose homonyms all stand within a fifth
  of a degree is located there. Any other bare name is read in the country
  the tree's located places use most among those where the name exists,
  preferring a living commune to a settlement and a settlement to a former
  name: "Brest" in a mostly French tree is the city in Finistère. A bare
  name found in none of the tree's countries stays unlocated.
- A label whose first part is no dictionary name — a hamlet, a farm, a
  lieu-dit written before its municipality, or a former name the dictionary
  lacks — is located by the first following part that places it, at that
  municipality's spot: a municipality code (INSEE, BFS…) on its own, homonym
  codes told apart by what follows it; a municipality name only when a part
  after it confirms it (its code, subdivision, region or country), so a
  namesake elsewhere never catches a hamlet. "Le Brossais, Vigneux-de-Bretagne,
  44217, Loire-Atlantique" is put at Vigneux-de-Bretagne.
- A part that mixes words and numbers is read without its numbers — a
  postcode or a street number: "22 Rue A, 50700 Valognes, France" is put at
  Valognes, "Fort Lee, New Jersey 07024, USA" at Fort Lee. A part made of
  numbers alone stays a code. The lookup
  happens at each visit and changes nothing in the tree; it keeps no
  dictionary in memory ([Place dictionary §2.1](place-dictionary.md)).

### 4.2 Top ten

Beside the map, the ten places with the most usages, each with its count and
a number repeated as a marker on the map. A place that could not be located
is still listed, without a marker. At the bottom of the column, level with
the bottom of the map, the number of places that could not be located. The
[Tools](ui-tools.md) page lists those places, by the same rule, with a way
to correct each (Tools §4).

Clicking a numbered marker, or a located place of the list, centres the map
on that place and zooms in to its region (about three degrees wide) unless
the view is already closer; the place stays highlighted in the list until
another is chosen or the fit button shows every place again.

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
names and women's first given names.

## 6. Charts

The period charts sit under the year ruler (§2):

| Tab | Block | Chart | Kind |
|-----|-------|-------|------|
| Population | Births and deaths | Births, baptisms, unions, deaths and burials per period | Lines (5) |
| Population | Births and deaths | Sex ratio at birth: boys born per 100 girls born | Line |
| Population | Births and deaths | Births by month, share of each month per period | Lines (12) |
| Population | Births and deaths | Infant and child mortality: share of the births followed by a death before one and before five years | Lines (2) |
| Population | Length of life | Average age at death, men and women, per period of death | Lines |
| Population | Length of life | Average age at death, men and women, per period of birth (by generation) | Lines |
| Families | Unions | Average age at the first union, men and women, per period of the union | Lines |
| Families | Unions | Average duration of a union, per period of the union | Line |
| Families | Unions | Unions by weekday, share per period | Lines (7) |
| Families | Unions | Unions by month, share per period | Lines (12) |
| Families | Unions | Average age difference between spouses, in months | Line |
| Families | Children | Average number of children per union, per period of the union | Line |
| Families | Children | Average time between two births in a family, in months | Line |
| Families | Children | Average gap between the first and the last child, in months | Line |
| Families | Children | Parents' average age at the first and at the last child, per period of that birth | Lines (4) |
| Families | Children | Fathers' and mothers' average age at each child's birth, per period of that birth | Lines (2) |

Beside them, the Population tab holds the **age pyramid** (persons by age
at death in five-year bands, men to the left and women to the right) and
the Families tab a bar chart of the unions by
number of children. The Overview holds a donut of the ten most frequent
event types, each event counted once (a family event once however many
spouses carry it), with the total under it.

Donuts show a legend with each value and its count. Line charts share one
x-axis of periods, skip periods without data rather than drawing them as
zero, and show the value of a point on hover. Their lines are smooth
monotone curves, which never overshoot their points, and a chart of at most
four series is filled beneath them. Their value axis runs from
zero in at most four round steps (1, 2, 2.5 or 5 times a power of ten), and
a chart with a single line has no legend: its title names it. Series colors come from the
theme (men and women use the pedigree's male and female colors; persons
added and removed the theme's green and red; months and weekdays the chart
palette). A chart with markers (§10) keeps a band above its plot for their
numbered badges.

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
- **Enough values**: a period's average, share or ratio is drawn only when
  it rests on at least 10 values (10 unions for their weekdays, 10 girls
  born for the sex ratio, 10 births for mortality); fewer make noise, not a
  trend. Counts are drawn whatever they are.
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

## 10. Growth

The Growth tab follows the tree itself rather than its ancestors: how many
persons it held over the calendar days it was worked on, from the day the
first person was added to today.

| Chart | Kind |
|-------|------|
| Persons in the tree at the end of each period, imports marked | Line |
| Persons added (created, imported or restored) and removed (deleted or merged into another) per period | Lines (2) |

- **Source.** The person table, not the change history: every person keeps
  the time it was created, and a deletion or a merge only stamps the time
  it was deleted ([soft deletion](data-model.md)), so the count at any
  instant is exact, deleted persons included, for trees older than the
  change history as well. An import stamps everyone it brings with one
  time, so it is one step of the curve on the import's day; a tree
  duplication is the new tree's import. The history baseline
  ([Data Model §5.2](data-model.md)) only versions the persons a tree
  already held and adds nobody.
- **Restores.** Restoring a deleted person clears their deletion time, so
  the spell they spent deleted is read back from their versions: the
  deleted version and the restore that followed it count as a removal and
  an addition on their days.
- **Days** are UTC days; a period holds the days from its first to the day
  before the next one's.
- **Granularity** follows the span from the first day to today: one point
  per day up to 31 days, per week (from Monday) up to 182 days, per month
  up to five years (1,826 days), and per year beyond, so a chart has from
  one to about sixty points. A note above the charts names it with the
  first day and today. Every period is drawn, those without a change
  keeping the count, and the axis labels a day or a week by its first day
  and month, a month by its month and year, a year by itself.
- **Imports** recorded in the [audit log](data-model.md) are marked on the
  persons chart: a dashed line at the period holding the import under a
  numbered badge, which names on hover the persons each import brought.
  Under the chart, the imports are listed with their badge number, time,
  persons and file name (or source tree name). An import made before the
  change history existed has no entry, so no marker, but still its step.
- A tree created and filled today is a single point; a tree without any
  person, ever, shows "No person has been added to this tree yet" instead
  of the charts.

## 11. States

- While loading, the charts show the shared loading placeholder.
- A chart without any data shows "Not enough dated records" instead of an
  empty frame.
- A tree without any located place shows the map empty with the reason.
- A tree without any record shows no Records section.

## 12. i18n and accessibility

Every title, hint, legend, axis label, month and weekday name goes through
i18n in every interface language. Charts carry an accessible title and a
text summary of their values; colors are never the only way to tell series
apart (the legend and the hover label name them).
