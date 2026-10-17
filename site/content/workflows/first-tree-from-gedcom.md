+++
title = "Move your tree from another program"
description = "Export your tree from your current genealogy software, import it into OxidGene, set its roots and check the result."
weight = 10
+++

This walkthrough takes you from a tree kept in another genealogy program or website to a checked tree in OxidGene: exporting it there, creating a tree, importing the file, setting the SOSA root and “Who am I?”, and looking for what needs attention.

If your tree is on Geneanet and you want its photos too, follow [Bring a Geneanet tree with its photos](@/workflows/geneanet-with-photos.md) instead.

## 1. Export your tree from your current program

Use your current program's export function. Pick the best format it offers, in this order:

1. **GEDZIP (`.gdz`)**, sometimes called “GEDCOM 7 with media”: the only format that carries your photos and documents along with the genealogy.
2. **GEDCOM (`.ged`)**, in version 5.5.1 or 7. OxidGene reads both, in any character set, so you do not need to change the export's encoding.
3. **GeneWeb (`.gw`)**, if you use GeneWeb.

With a plain GEDCOM file, your photos stay behind: OxidGene creates a record for each one, with its file name, but without the picture. Keep your media folder; you can attach each file to its record later (see [Media](@/guide/media.md)).

The file can be up to 1 GiB.

## 2. Create a tree

1. Install and open OxidGene (see [Install OxidGene](@/guide/install.md)).
2. On the home page, click **New Tree**.
3. Give it a **Name**, for example “Landrevel family”, and optionally a description.
4. Click **Create**.

Import into a new, empty tree: an import adds what it reads to the tree, it does not merge it with persons already there.

## 3. Import the file

1. On the new tree's card, open the three-dot menu and choose **Import**.
2. On the **A file** tab, drop your file onto the drop zone, or click it to pick the file.
3. Click **Import** and wait. The window stays open until the import is over; a large tree with many media can take several minutes.
4. When **Import complete** appears, compare the counts (people, families, events, sources, places, images, documents) with what your old program reported.
5. If a line such as **12 warnings** appears, click it and read the warnings. They name what could not be read or connected, such as an age written as free text or a note pointing to nothing. Everything else was imported.

If the result is not what you expected, for example because the export was incomplete, delete the tree, export again from your old program, and start over in a new tree. See [Import and export](@/guide/import-export.md) for what is and is not imported.

## 4. Set the SOSA root and say who you are

1. Open the tree's **Settings** from its card menu.
2. In **Tree & Roots**, under **Root Person (SOSA 1)**, click **Change** and choose the person your ancestry is numbered from, often yourself or one of your children.
3. Under **Who am I?**, click **Change** and choose your own person in the tree.

Both are saved immediately. The tree view now opens on the SOSA root, its ancestors carry their SOSA numbers, and relationships can be traced from you. See [Trees](@/guide/trees.md#set-the-sosa-root).

While you are there, check **Date Display** and **Entry Options** and choose how you want dates written and typed (see [Settings](@/guide/settings.md)).

## 5. Look around

Open the tree from the home page. Browse a few branches you know well in the pedigree, open some profiles, and check that names, dates, places, sources and notes came through as you expect (see [The pedigree](@/guide/pedigree.md) and [Persons](@/guide/persons.md)).

## 6. Check for anomalies

Every program records data a little differently, and a move is a good moment to find old mistakes.

1. In the tree's left sidebar, click the wrench icon to open **Tools**.
2. Open the **Anomalies** tab. Tiles count the anomalies by category: **Dates**, **Filiation**, **Unions**, **Witnesses and godparents** and **Records**. An **Error** is an impossible situation, such as a death before the birth; a **Warning** is an unlikely one, such as a mother over 55 at a child's birth.
3. Open a rule to see each case, with the persons concerned. Click a person to open their profile and correct the record.

A vague date never raises an anomaly its vagueness could explain, so what is listed is worth a look.

## 7. Locate your places

1. In **Tools**, open the **Places not located** tab. It lists the places the statistics map cannot place, most used first.
2. Click **Who uses it** to see which persons refer to a place.
3. Click **Correct** to fix the name. Choosing a suggestion from the built-in place dictionary also gives the place its coordinates, and it leaves the list. Its events and media keep it: the place is renamed, not replaced.

## 8. Look for duplicates

Imports often reveal the same person entered twice. The **Potential duplicates** tab of **Tools** lists pairs of records that may be one person, so you can merge them or mark them as different people. See [Tools](@/guide/tools.md) and [Clean up a tree](@/workflows/clean-up-tree.md).

## 9. Keep a copy

Once you are happy with the tree, export it as a GEDZIP from **Settings → Export Tree**: one file with your genealogy and its media, which OxidGene and other programs supporting GEDCOM 7 can read back (see [Import and export](@/guide/import-export.md#export-a-tree)).
