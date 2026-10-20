+++
title = "Clean up a tree after an import"
description = "Check an imported tree, fix its anomalies and places, merge duplicates, normalise family names, and undo a wrong change."
weight = 40
+++

An imported tree rarely arrives spotless: dates typed in a hurry, places written three different ways, the same ancestor entered twice from two branches. This workflow takes you through checking and cleaning a tree, in an order that saves work: fix the obvious errors first, then the places, then the duplicates, then the names.

Nothing here is urgent or all-or-nothing. Every step can be done a few records at a time, and every list is recomputed when you come back to it.

## 1. Take stock

Open the [statistics](@/guide/statistics.md) with the chart icon in the left icon sidebar. The **Overview** tab tells you how much work lies ahead:

- the **Completeness** tiles show the share of persons with a dated birth, a dated death, a known parent;
- the **Counts** give the number of persons, places and distinct family names. Many more family names than you expect often means spelling variants.

## 2. Fix the anomalies

1. Open the tools with the wrench icon in the left icon sidebar, then the **Anomalies** tab.
2. Start with the **Error** rules: a death before a birth, a parent born after their child, a person among their own ancestors. These are almost always typing errors or a person attached to the wrong family.
3. Open a rule, follow a person's link to their profile, and correct the record.
4. Then go through the **Warning** rules. These are unlikely, not impossible: a mother of 56 may be a typing error or a genuine late birth. Leave as it is what the records confirm.

Imprecise dates never raise an anomaly that their vagueness could explain, so what you see deserves a look. An anomaly often points at a duplicate or a wrong link rather than a wrong date: keep that in mind for step 4.

{{ <screenshot name="tools-anomalies" alt="Anomalies tab listing the rules that found something" lang={lang} /> }}

## 3. Locate the places

1. On the tools page, open **Places not located**. The places are listed most used first, so the first rows give the biggest gain.
2. Click **Who uses it** if you are unsure which place is meant.
3. Click **Correct**, pick the right suggestion from the place dictionary, and click **Save**.

The place is renamed, not replaced, so every event and media item that uses it follows. Once it bears a dictionary name, it is located on the statistics map and leaves the list.

## 4. Merge the duplicates

1. On the tools page, open **Potential duplicates**. Pairs are listed best first, with a confidence (**Very likely**, **Likely**, **Possible**) and what the two records share.
2. For a pair that is clearly two people (a father and son of the same name, say), click **Two different people**. It will not be suggested again.
3. For a pair that may be one person, click **Compare**. The merge wizard opens on the two records side by side.
4. Choose the record to keep with **Keep this record**, then, row by row, the values the merged record keeps. Tick the events and media to take from the other record, and click **Next**.
5. Check the summary and click **Merge**.

A merge cannot be undone, so check the comparison carefully. The list is computed again after each answer. You can also merge two records you find yourself, with **Merge with…** on a person's profile or in the pedigree's action menu. See [Tools](@/guide/tools.md) for every detail of the wizard.

{{ <screenshot name="merge" alt="Merge wizard on two records of the same person" lang={lang} /> }}

## 5. Normalise family names in bulk

The dictionary lists each family name exactly as spelled, so variants stand out: "Landrevel" and "LANDREVEL", or "Landrevel" and "Landreval", are separate rows.

1. Open the [dictionary](@/guide/dictionary.md) with the book icon in the left icon sidebar. It opens on the **Family Names** tab.
2. Scan the list, using the letter index and the filter. Open a row to see who carries the name.
3. Open the **⋮** menu of the variant's row to correct and choose **Edit**. The **Edit family name** dialog opens.
4. Type the correct spelling in **Name**. If that name already exists, the dialog warns that the two will be merged.
5. Click **Rename**. Every person who carries the variant as their main name now carries the correct one.

Persons who carry the variant only as a secondary name (an alias, a married name) keep it; the dialog tells you how many.

The same dialog fixes a particle filed in the wrong place, such as a "Le" or a "de" that was taken for part of the surname or the other way round. Leave the name unchanged, set the **Particle** field, and click **Apply to all**.

{{ <screenshot name="dictionary" alt="Family Names tab of the dictionary" lang={lang} /> }}

## 6. Undo a wrong change

If a correction turns out to be wrong, the history lets you go back.

- For one person: open their profile, click **History**, select the version before the mistake, and click **Restore this version**.
- For a change seen in the tree's log: open the tree settings with the gear icon, choose **History**, find the change, and click **Restore the previous version**.

A family-name rename gives each renamed person a new version, so you undo it person by person through their history. A restore is itself recorded, so it can be undone too. See [History](@/guide/history.md).

## Keep going

Cleaning is never quite finished. Come back to the **Anomalies** and **Potential duplicates** tabs after each import or long editing session: both are recomputed from the current tree every time you open them. To see which ancestors are still missing, open **Ancestry completeness**; [Record a new ancestor](@/workflows/record-a-new-ancestor.md) shows how to fill the gaps.
