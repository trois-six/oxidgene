+++
title = "Search"
description = "Find a person by name or SOSA number, then narrow the results with filters."
weight = 90
+++

This page explains how to find persons in a tree: the quick search in the top bar, which suggests names and persons as you type, and the search results page, with its filters, sorting and two layouts.

## Quick search

The top bar of the pedigree, of a person's profile and of the search results page holds two fields, **Last name / # SOSA** and **First name**, and a magnifying-glass button. Use either field alone or both together. Press `/` anywhere outside a text field to jump to the last-name field.

As you type, a panel opens under the fields:

- First, up to five of the tree's names that complete the field you are typing in — family names in the first field, given names in the second — each with the number of persons who carry it. The counts take the other field into account: with a family name typed, a given name counts only the persons with that family name. Picking a name completes the field and keeps the panel open, so the list of persons narrows down.
- Then up to six matching persons, each with portrait, name, years and a line saying who they are ("married to …", "son of …"), which tells two persons of the same name apart.
- When there are more matches than the panel shows, a **See all … results** link at the bottom leads to the full results page.

Use the **Down** and **Up** arrow keys to move through the names and persons, **Enter** to pick the highlighted one, and **Escape** to close the panel. Picking a person opens the pedigree centred on them; from a person's or a couple's page, it opens the person's profile instead.

Nothing is searched until you have typed at least two characters across the two fields. If the tree's entry suggestions are turned off in the [tree settings](@/guide/settings.md), the panel lists persons only, without names.

## Jump to a SOSA number

Type a number alone in **Last name / # SOSA** to find a person by their SOSA number, counted from the tree's SOSA root. The panel shows the person at that number; press **Enter** to go straight to them. If the tree has no SOSA root or nobody holds that number, Enter runs an ordinary search instead.

## Open the search results page

Press **Enter** with nothing highlighted in the panel, or click the magnifying glass, to open the search results page. Its top bar keeps your search in the two fields: change them and press **Enter** again to update the results in place.

Opened without any criterion, the page shows **Enter at least one search criterion.**

{{ <screenshot name="search" alt="Search results for the LANDREVEL family name, filtered to persons with media" lang={lang} /> }}

The line above the results gives the exact number of persons found. Each result shows:

- the portrait, or a placeholder for the person's sex, and a coloured border for the sex;
- the family name in capitals and the given names;
- the birth and death years, with their precision as on the pedigree (`ca 1849`, `< 1917`); the baptism stands in for a missing birth date and the burial for a missing death date;
- who the person is: their spouses if they have any, otherwise their parents, followed by the number of children;
- the birth place, when known.

Click a result to go to that person in the kind of view you searched from: the pedigree, centred on them, when you searched from the tree; their profile when you searched from a person's or a couple's page.

## Filter the results

Click **Filters** to open the filter panel. Filters apply as you type, and every active filter also appears as a chip above the results: click a chip's × to remove that filter, or click **Clear filters** to remove them all.

The panel has three groups, **Person**, **Event** and **Relations**:

| Filter | What it keeps |
|---|---|
| **Surname**, **Given names** | Persons whose names contain the text, ignoring case and accents. These are the same fields as the top bar's: typing in one updates the other. |
| **Gender** | **All**, **Male**, **Female** or **Unknown** |
| **Occupation** | Persons with an occupation matching the text |
| **Born between**, **Died between** | Persons born or dead within the two years, both included |
| **Place** | Persons with any event, their own or a couple's, at a matching place |
| **Event type** | Persons with an event of that type: Birth, Death, Baptism, Burial, Marriage, Residence, Occupation or Census |
| **Event between** | Persons with an event within the two years, both included |
| **Spouse** | Persons with a spouse of that family name and given names |
| **Father**, **Mother** | Persons whose father or mother has that family name and given names |
| **With media** | Persons with at least one attached media |

The name, occupation and relatives' fields suggest the names and occupations found in your tree. They list only the tree's own values, since a term nobody carries would find nobody.

If nothing matches, the page says so and offers **Clear filters**.

## Sort and change the layout

The sort selector above the results offers:

- **Relevance**: family names beginning with what you typed first, then matching given names, then matches found further in;
- **Name A → Z** (the default) and **Name Z → A**, by family name then given names;
- **Birth date ↑** (oldest first) and **Birth date ↓** (most recent first).

The two buttons on the right switch between the **List view** and the **Pedigree grid view**. In the grid, each person is a card with their name, years and a small pedigree of their parents and grandparents. Click the card's header, or any person in the small pedigree, to go to that person the same way.

The list shows 25 persons per page and the grid 20. The page buttons at the bottom move between pages; switching layout returns to the first page.

## Print the results

The print button in the top bar prints the page of results on screen, in the current layout and order. The sheet is titled with your search, lists the result count and the active filters, and says which page it holds. To print other results, go to that page first.

Learn more: [search results specification](@/tech/ui-search-results.md).
