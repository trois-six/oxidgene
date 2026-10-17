+++
title = "Statistics"
description = "What the statistics page tells you about a tree as a whole, tab by tab."
weight = 120
+++

The statistics page shows what your tree says as a whole: how complete it is, where your ancestors lived, how they were named, how long they lived, when they married and how many children they had, the records worth a look, and how the tree itself grew. This page walks through it tab by tab.

## Open the statistics

Click the chart icon in the left icon sidebar of any tree page: the pedigree, a person or couple profile, the dictionary. The page opens on **Overview** the first time; after that it reopens on the tab you last used.

The page is read-only. Everything is computed from your tree each time you visit, so it always reflects your latest changes.

Each chart has a title and a **?** hint that explains how its values are counted. Hover a point of a line chart to read its value.

## Count approximate dates

By default, ages and averages only use exact dates. Tick **Approximate dates** in the top bar to also use dates recorded as about, calculated or estimated. The setting applies to every tab. Dates recorded as before, after, perhaps, or between two dates are never used for ages.

Wherever a birth date is missing, the baptism stands in for it; wherever a death date is missing, the burial does.

## Overview

The **Overview** tab gathers the key figures in three rows of tiles:

- **Counts**: persons (with men, women and unknown sex), unions, places, sources, the years covered from the first to the last dated event, and the number of distinct family names and first given names.
- **Completeness**, as a share of all persons: with a dated birth, with a dated death, without a known parent, without children, and never in a union. These tell you at a glance where research is still needed.
- **Averages**: age at death and age at the first union (also for men and for women), the generation interval (the parents' age at the birth of each child) and the number of children per union. Each comes with its median, standard deviation and range.

Below them, the **Events** block shows a donut of the ten most frequent event types. A family event, such as a marriage, is counted once however many spouses share it.

{{ <screenshot name="statistics-overview" alt="Overview tab of the statistics page with counts, completeness and averages" lang={lang} /> }}

## Places

The **Places** tab opens with the **Map of the places**: a heat map of where the events of your tree happened. Each located place adds a soft spot weighted by how often it is used, so the warm areas show the regions where your family was active. The precision is deliberately coarse: the map shows regions, not villages. The map works offline; no map service is contacted.

- Zoom with the mouse wheel or the **Zoom in** and **Zoom out** buttons, drag to move, and use **Fit to the places** to see every place again.
- Beside the map, the **10 most used places** are listed with their number of uses and a number repeated as a marker on the map. Click a marker or a place in the list to centre the map on it.
- At the bottom of the list, a line tells you how many places could not be located. The [tools page](@/guide/tools.md) lists them and lets you correct them.

A place is located by its own coordinates or, failing those, by recognising its name in the built-in place dictionary. A hamlet written before its municipality ("Le Moulin, Saint-Aubin, …") is placed at the municipality.

Under the map, **Births by area** shows three donuts: births by country, by region and by subdivision (département, county, district…), the ten largest of each, with the number of distinct countries, regions or subdivisions your tree spans.

{{ <screenshot name="statistics-map" alt="Heat map of the places used in the Landrevel family tree" lang={lang} /> }}

## Names and occupations

The **Names and occupations** tab shows donuts of the ten most common family names, occupations, men's first given names and women's first given names. Only the first word of the given names counts, and names that differ only in capitals are counted together, shown in their most frequent spelling.

## Population and Families: charts over time

The **Population** and **Families** tabs hold charts that follow your family century after century. Both open on a bar that controls their time axis:

- **Interval**: the width of each period, 10, 25, 50 or 100 years (25 by default).
- The year ruler: drag its two handles, or use the arrow keys, to choose the first and last year shown. The first view skips the few very early records that would stretch the axis over empty centuries; **All years** widens it to the whole span.

The bar stays at the top while you scroll. The interval is remembered on your device; the years shown last for the visit.

**Population** holds:

- **Births and deaths**: births, baptisms, unions, deaths and burials per period; the sex ratio at birth (boys per 100 girls); births by month; infant and child mortality (deaths before one and before five years).
- **Length of life**: the average age at death by period of death and by generation (period of birth), for men and women, and the age pyramid at death in five-year bands.

**Families** holds:

- **Unions**: average age at the first union, average duration of a union, unions by weekday and by month, and the age difference between spouses.
- **Children**: children per union, a bar chart of unions by number of children, the time between two births, the gap between the first and last child, and the parents' age at the first, last and every birth.

An average, share or ratio is only drawn for a period with at least 10 values, so a handful of records does not pass for a trend. Periods without data are skipped rather than drawn as zero. A chart with nothing to show reads "Not enough dated records".

{{ <screenshot name="statistics-charts" alt="Charts per period under the year ruler" lang={lang} /> }}

## Records and lists

The **Records and lists** tab first shows **Records**: one card per extreme your tree holds, with the person or couple concerned (linked to their profile), the value and its date. Among them: the longest-lived man and woman, the earliest and latest births, the youngest and oldest at a union, the longest union, the most children, the longest widowhood, the largest gap between siblings, and the most generations of descendants.

Below, **Notable records** offers lists of up to 100 persons each, with date and place: **Latest births**, **Latest unions**, **Latest deaths**, **Oldest possibly alive** (no death recorded and born fewer than 120 years ago), **Longest lives** and **Largest families**.

## Growth of the tree

The **Growth** tab follows your work rather than your ancestors: how many persons the tree held on each day since the first was added.

- **Persons in the tree** counts the persons at the end of each period. Each import shows as a step, marked by a dashed line and a numbered badge; hover the badge to see how many persons it brought. The imports are listed under the chart with their date, number of persons and file name.
- **Persons added and removed** shows, per period, the persons created, imported or restored, and those deleted or merged into another.

The time scale adapts to the age of the tree: one point per day, week, month or year, as a note above the charts says.

## Print a tab

Click the printer icon (**Print**) in the left icon sidebar, or press Ctrl+P (Cmd+P on a Mac), to print the tab shown, with its charts and the chosen years. Through your system's print dialog you can also save it as a PDF.

Learn more: [statistics specification](@/tech/ui-statistics.md).
