+++
title = "Tools"
description = "Check and work on a tree with the tools page: anomalies, unlocated places, ancestry completeness, duplicates, merging, and dates."
weight = 130
+++

The tools page gathers what you use to check a tree and work on it rather than to read it: the anomalies of dates and relationships, the places that cannot be located, the completeness of your ancestry, the potential duplicates and how to merge them, and two date tools. This page covers each tab in turn.

## Open the tools

Click the wrench icon in the left icon sidebar of any tree page, just below the statistics chart icon. The page shows one tool per tab: **Anomalies**, **Places not located**, **Ancestry completeness**, **Potential duplicates**, **Date converter** and **Date in words**. It reopens on the tab you last used.

Each tab only loads its data when you open it, and recomputes it from the current tree every time, so a correction you make shows up as soon as you come back.

## Find anomalies

The **Anomalies** tab checks the whole tree for dates, filiations, unions, witnesses and records that are impossible (**Error**) or unlikely (**Warning**).

1. Open the **Anomalies** tab. A row of tiles shows how many anomalies each category holds: **Dates**, **Filiation**, **Unions**, **Witnesses and godparents** and **Records**.
2. Click a tile to show only that category; click it again to show them all.
3. Under each category, every rule that found something is a folded block with its severity, title and count. Open a block to read what the rule checks and see one row per anomaly.
4. Each row names the persons concerned, each linked to their profile, with **Open in the tree** to centre the pedigree on them, and **Couple** for a union's anomaly. Beside them is what the rule measured: an age, a gap, the recorded text.
5. Click anywhere else on a row, or press **Enter** on it, to open the profile of its first person, where you correct the anomaly.
5. Correct the record, then come back to the tab: the list is computed again.

A rule lists at most 500 anomalies and says how many more it found. A tree without any anomaly says so, with the number of persons checked.

Use the **Tag** list above the anomalies to keep only those naming a person who carries the tag. The counts follow the choice.

### How dates are read

The checks are designed not to cry wolf:

- The birth is the birth, or the baptism when the birth has no date; the death is the death, or the burial when the death has no date.
- An imprecise date stands for every day it may mean: a year alone for the whole year, a month for the whole month. A date about, calculated or estimated widens by two years each way. Dates before, after, perhaps, or between two dates are not used.
- A rule fires only when it holds for every day those dates may stand for. A vague date therefore never raises an anomaly that its vagueness could explain.
- Adoptive, foster and step parents are not held to the age and death rules that apply to biological parents.

### The catalogue of rules

| Category | What is checked |
|---|---|
| **Dates** | Death before birth, burial before death, an event before birth, baptism after death, an event after death or after the burial (burial, cremation, funeral, probate and free-form events excepted), death over 105, a centenarian born before 1900, a date in the future, and an age recorded at an event that contradicts the dates by more than two years |
| **Filiation** | A parent or an ancestor born after their child or descendant, a person among their own ancestors, a parent younger than 11, a father older than 70 or a mother older than 55 at a birth, a child born after the mother's death or more than 300 days after the father's, siblings born 11 days to 7 months apart, or more than 50 years apart |
| **Unions** | A union before a spouse's birth or after their death, a spouse younger than 12 or older than 100, spouses born more than 50 years apart, the same two persons married in several unions, several spouses of the same name, a union with one's own parent, a union with more than two spouses |
| **Witnesses and godparents** | A witness or godparent not yet born or already dead at the event, a godfather recorded as a woman or a godmother as a man |
| **Records** | A husband recorded as a woman or a wife as a man, two husbands or two wives in one union, an unreadable date, a date range that ends before it begins, a person without any name |

Godparents are recognised from the relation recorded on a witness, in several languages (godfather, parrain, Taufpate, padrino…). The full list of rules, with their thresholds and the matching checks of Geneanet, is in the [tools specification](@/tech/ui-tools.md).

{{ <screenshot name="tools-anomalies" alt="Anomalies tab of the tools page" lang={lang} /> }}

## Locate places

The **Places not located** tab lists the places used in your tree that the [statistics map](@/guide/statistics.md) cannot place: places without coordinates whose name the place dictionary does not recognise. They are listed most used first, with their number of uses.

1. Click **Who uses it** to see the persons whose events or media name the place; each opens the pedigree on that person.
2. Click **Correct**. The name becomes a place field with suggestions from the place dictionary.
3. Pick a dictionary suggestion, or type the corrected name, and click **Save**.

The place is renamed, not replaced: its events and media keep it. Choosing a dictionary suggestion also gives the place its coordinates, so it is located from then on and leaves the list. The rename is recorded in the tree's history like any other change.

## Check ancestry completeness

The **Ancestry completeness** tab follows your ancestry generation by generation from the tree's SOSA root (SOSA 1): who is known at each SOSA number and which key facts are recorded. If the tree has no SOSA root yet, the tab says so and offers **Choose a SOSA root**, which leads to the tree settings.

- **Generations** sets how deep to look, from 4 to 15 generations (8 by default).
- The summary has one row per generation: ancestors found against ancestors expected (1, 2, 4, 8…), the share as a bar (green above 70%, orange from 40%, red below), and how many of those found have a birth, a death and a union recorded.
- Below, each generation lists its ancestors by SOSA number, each linked to their profile with their dates and a mark per key fact: green when recorded, red and struck through when missing. A generation with something to fill opens by itself; a complete one stays folded.
- **Show only what is missing** keeps the missing ancestors and those lacking a fact.

{{ <screenshot name="tools-ancestry" alt="Ancestry completeness tab: ancestors found generation by generation from the SOSA root" lang={lang} /> }}

The key facts are a birth or baptism, a death or burial, and a union with the other parent in the line (a marriage, a civil union, banns, a contract and the like). A person with no death recorded and born fewer than 120 years ago may still be alive, so no death is expected for them.

Only the missing parents of known ancestors are listed as **Missing ancestor**. Their own ancestors are counted under each generation instead ("12 more ancestors are missing because their children are too"). An ancestor reached through two lines appears at each of their SOSA numbers.

## Review potential duplicates

The **Potential duplicates** tab lists pairs of persons that may be the same person recorded twice, best first.

Two persons are compared when their surnames and first given names sound alike ("Martins" and "Martin", "Dupond" and "Dupont"); only persons with both a surname and a given name are compared. A pair is ruled out when the sexes differ, when the births or deaths are more than 5 years apart, when one died before the other was born, when one is the other's spouse, parent or child, when they are siblings born on different known days, or when you have already said they are different people.

Each pair gets a score from the clues the two share: the same name, the same birth date or year, the same birthplace, the same death year, the same parents or parents of the same name, a spouse of the same name. A pair is only listed from 40 points, so a common name alone is never enough. The confidence reads **Very likely** from 70 points, **Likely** from 55, and **Possible** below. At most 500 pairs are listed.

For each pair, you see both persons with their dates, relatives and birthplace, the confidence, the score, and what the two share. Then:

- Click **Two different people** if they are not the same person. The pair leaves the list for good.
- Click **Compare** to open the merge wizard on the pair and decide there.

After either answer, the list is computed again.

{{ <screenshot name="tools-duplicates" alt="Potential duplicates tab with pairs of records to compare" lang={lang} /> }}

## Merge two records of one person

The merge wizard combines two records of the same person into one. Open it:

- with **Compare** on a pair of potential duplicates (it opens directly on the comparison);
- with **Merge with…** in the pedigree's action menu (right-click a card, or its **⋮**);
- with **Merge with…** in the header of a person's profile.

The wizard has three steps.

1. **Choose the other record**. Search for the other record of the person in the current tree. Choosing it moves to the next step.
2. **Compare and choose**. The two records stand side by side. Pick the one to keep with **Keep this record** (the first one by default); the other is absorbed. In the comparison table, rows whose values differ are highlighted; where the absorbed record has something to offer, pick which value the merged record keeps: surname, given names, sex, and the birth, baptism, death, burial and cremation. By default, the value that says more (a fuller date, a place) is picked. Below, tick the other events and the media of the absorbed record to take onto the record kept. Those it already has are marked "already on the record kept" and left unticked. Names, family links, notes and sources always move to the record kept. Click **Next**.
3. **Review and confirm**. A summary shows the record kept, the primary name it will bear, and what is taken and left out. Read the warning, then click **Merge**.

{{ <screenshot name="merge" alt="Merge wizard comparing two records side by side" lang={lang} /> }}

What happens next:

- The absorbed record is deleted; everything you kept moves to the other. Events left out are deleted; media left out stay in the media library, just unlinked. The kept record's former primary name stays as a secondary name.
- The merge cannot be undone, as the wizard warns. Take your time on step 2.
- If the absorbed record was the tree's root, the kept one takes its place.
- Two persons who are spouses in the same family, or of whom one is the other's ancestor, cannot be merged.
- From the duplicates list, the wizard also offers **Two different people**, and **Cancel** leaves the pair listed.

## Convert a date between calendars

The **Date converter** reads a date in every calendar the application records dates in: Gregorian, Julian, Hebrew and French Republican.

1. Enter the date with the usual date field: calendar, qualifier, day, month and year (and a second date for "or" and "between").
2. One tile per calendar shows the date as the application writes it in that calendar, qualifier included. The calendar you entered it in is marked **As entered**.

A partial date keeps its precision: a year alone converts to the year it overlaps most. A calendar that cannot express the date, such as the Republican calendar before 22 September 1792, says **Not expressible in this calendar**. A single, exact, complete date also gives its day of the week.

## Write a date in words, and read one back

The **Date in words** tab writes a date out as registers and deeds did, and reads a written date back into figures. It is handy for transcribing a parish or notarial record.

To write a date out:

1. Enter the date with the date field. A Hebrew or Republican date is written from the same day in the Gregorian calendar, and the page says so. Years from 1 to 3999 can be written.
2. Choose the **Form**: **All in words** or **Figures and month name**.
3. Choose when the **Year starts on**: **1 January**, or **25 March (Annunciation style)**, which many registers kept. In that style, a date from 1 January to 24 March bears the previous year's number.
4. Read the date in each language — English, French, German, Spanish, Italian, Dutch, Polish and Portuguese — and in **Latin**. Each row has a copy button.

The Latin rows add the **Latin, Roman reckoning** of a complete date (counted to the next Kalends, Nones or Ides, such as *ante diem IV Nonas Februarii*) and **Latin, part by part**, which breaks the day, month and year into words and Roman numerals.

To read a written date, paste it under **Read a written date** and click **Read**. The date field above is filled with it. The tool understands numbers in words in any of these languages or in Latin (cardinal or ordinal), figures, Roman numerals, month names in any case, GEDCOM abbreviations such as *FEB*, and the Roman reckoning. If it cannot read the text, it says why: nothing to read, no year found, or a day that the month does not have. A date read is taken as Gregorian.

## Print a tab

Click the printer icon (**Print**) in the left icon sidebar to print the tab shown with its results; the inputs and buttons are left out.

Learn more: [tools specification](@/tech/ui-tools.md) and [merge specification](@/tech/ui-merge.md).
