+++
title = "History"
description = "Browse every recorded state of a person, compare versions field by field, and restore an earlier one."
weight = 140
+++

OxidGene records every change made to a tree and keeps the earlier states of your records. This page explains how to read a person's history, compare two versions, restore an earlier one, bring back a deleted person, and use the tree's change log.

## What the history keeps

Every write to a tree is recorded in its change log: data, settings, media, imports and exports, and restores.

For a person, each change keeps the state they had before it. A version holds everything their profile shows except media: names, sex, their own events with witnesses, notes and sources, their notes and sources, their parents, and their unions with their events. Media are logged in the change log, but never versioned.

Some things to know:

- A person nobody has changed since they were created or imported has a single version, their current one. An import itself stores no earlier state.
- A change that touches several persons keeps a version for each one it actually changed. Adding a child to a family keeps the earlier state of both spouses and of the child; renaming the father only keeps his.
- Places, sources, repositories and the tree settings keep versions too.
- The change log does not record who made a change.

## Open a person's history

1. Open the person's profile.
2. Click **History** in the profile header.

The page is titled **History of** followed by the person's name. On the left, the versions are listed newest first, 100 at a time (**Load more** shows older ones). Each shows:

- **Version** and its number, with *current* for the person as they are now;
- when the change happened, in your language and local time;
- what produced it, such as *Changed — event (Birth)*, *Import* or *Restored*; the oldest version, when no recorded change produced it, reads **Initial state**;
- details when there are any: the file of an import, *version 3 restored*, *merged with* another person.

{{ <screenshot name="history" alt="A person's versions compared side by side on the history page" lang={lang} /> }}

## Compare two versions

The current version is selected when the page opens. Click another version in the list to show it.

- **Compare with** chooses the earlier version to compare against; by default, the one just before.
- **Changes only**, on by default, hides the fields that did not change. Untick it to see the whole record.

The comparison is a table, grouped by section (**Identity**, **Names**, **Events**, **Notes**, **Sources**, **Parents**, **Unions**), with the older version on the left and the newer on the right. Each change is marked **Added**, **Changed** or **Removed**. The oldest version has nothing to compare with, so everything in it reads as added.

Places, sources and relatives appear with the names they had when the version was stored, so an old version still reads as it did, even if a place or a relative was renamed since.

## Restore an earlier version

1. Select the version you want to go back to.
2. Click **Restore this version**.
3. Read the confirmation: the person will be put back exactly as that version recorded them (names, events, notes, sources, parents and unions). Confirm.

The restore appears at the top of the list as the new current version, marked *Restored* and *version N restored*. Because a restore is itself recorded, you can undo it the same way: restore the version it replaced.

A restore never brings back another person. If a witness, spouse or child named in the old version has since been deleted, that link is dropped rather than the person resurrected.

## Bring back a deleted person

A deleted person's profile is gone, but their history stays available.

1. Open the tree settings with the gear icon in the left icon sidebar, then choose **History** in the left menu.
2. Find the deletion in the log and click the person's name (or **Person history**) to open their history.
3. The page shows a **Deleted** badge beside the title. The current version is the deleted state; it cannot be restored itself.
4. Select the version just before the deletion and click **Restore this version**.

The person comes back as they were before the deletion, and their name in the top bar links to their profile again.

## Use the tree's change log

The **History** section of the tree settings is the change log of the whole tree, newest first, 100 entries at a time.

- Filter it with **All**, **Data**, **Settings**, **Media**, **Imports**, **Exports** or **Restores**.
- Each entry gives the time, a category badge, what was done (for example *Changed — event (Birth)*, *Added — name*, *Import*) and the name of what it was about, as it read at the time. A person's name links to their history.
- An entry that changed records offers **Show the change** (or **Show the N changes**). Open it to compare, for each record changed, its state before and after.
- Each change offers **Restore the previous version**, which undoes this change for that record after a confirmation. A person's change also links to their full history with **Person history**.

Creations, imports, exports and media changes replace no earlier state, so they have nothing to show or restore.

A merge cannot be undone; the merge wizard warns about it before you confirm (see [Tools](@/guide/tools.md)). A family-name rename in the dictionary is recorded as one entry but gives each renamed person a new version, so you undo it person by person (see [Dictionary](@/guide/dictionary.md)).

Learn more: [person history specification](@/tech/ui-person-history.md).
