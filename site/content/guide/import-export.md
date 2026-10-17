+++
title = "Import and export"
description = "Bring GEDCOM, GEDZIP, GeneWeb and Geneanet trees into OxidGene, and export your trees for other programs or for safekeeping."
weight = 30
+++

OxidGene reads the files other genealogy programs and websites produce, and writes files they can read. This page covers importing a GEDCOM, GEDZIP or GeneWeb file, importing a Geneanet tree with its photos, and exporting a tree.

## Which format to choose

| Format | Extension | What it carries |
|---|---|---|
| GEDCOM | `.ged` | The genealogy as text. Photos and documents are only named by their file path; the files themselves are not inside. |
| GEDZIP | `.gdz` | A GEDCOM 7.0 file and the media files it refers to, in one archive. |
| GeneWeb | `.gw` | The genealogy as GeneWeb and Geneanet export it. |

When the program you are leaving can write a GEDZIP, prefer it: it is the only file format that brings your photos and documents along with the genealogy.

## Import a file

An import always goes into an existing tree. Create the tree first (see [Trees](@/guide/trees.md)); importing into a new, empty tree keeps the imported data apart from anything you already have.

{{ <screenshot name="import" alt="Import window of a tree, with its two tabs" lang={lang} /> }}

1. On the home page, open the tree card's three-dot menu and choose **Import**.
2. The window **Import into “…”** opens on the **A file** tab.
3. Drop your file onto **Drop a file here, or click to browse**, or click it to pick the file. It accepts `.ged`, `.gdz` and `.gw` files up to 1 GiB.
4. Click **Import**.
5. Wait while the progress bar goes through its stages: the upload, **Reading genealogy…**, **Storing media** for a GEDZIP, **Writing genealogy…** and **Preparing the tree…**. The window cannot be closed while the import runs.
6. When **Import complete** appears, read the counts: people, families, events, sources, places, images and documents.

If the import produced warnings, a line such as **3 warnings** appears under the counts; click it to read them. Warnings name what the import could not read or connect, such as an age that is not written as an age, a note that points to nothing, or a file a GEDZIP refers to but does not contain. The rest of the file is imported normally.

If the import fails, the window shows the reason and keeps your file selected, so you can try again.

While the import runs, the tree's card on the home page shows **Import in progress** and cannot be opened. You can reload the page or come back later; the card becomes available again once the import has finished.

### What is imported

OxidGene reads GEDCOM 5.5.1, 7.0 and 7.1, in any character set, and recognises the version on its own. It imports persons and their names, families with their spouses and children (adopted and foster children included), events and attributes with their dates, places, ages, causes and agencies, sources and citations with their quality, repositories, notes, witnesses and associations, and media records.

What OxidGene has no place for — a tag specific to one program, a religion recorded inside an event rather than on the person, the temple of an LDS ordinance — is not shown, but it is kept with the record it belongs to and written back on every export: a file imported and exported again loses nothing but its header.

A GeneWeb `.gw` file is converted the same way. A damaged block in it is skipped and reported as one warning, so the rest of the file still imports.

### Photos and documents

- From a **GEDZIP**, the photos and documents arrive with the genealogy, ready to be viewed, cropped and linked, as if you had added them yourself.
- From a **GEDCOM** or **GeneWeb** file, each medium becomes a record that remembers its file name but has no file yet. You can attach the file to it later (see [Media](@/guide/media.md)).

The first picture linked to a person in the file becomes their portrait, and documents of several pages exported by OxidGene come back as one document with their pages in order.

## Import a Geneanet tree with its photos

Geneanet's own exports do not say which photo belongs to which person. The **From Geneanet** tab of the same window rebuilds those links by signing in to Geneanet from the desktop application, in a few guided steps.

In the web version, this tab can import the genealogy of a `.gw` file, but not the photos: the steps that read your Geneanet media need the desktop application.

The whole procedure is described step by step in [Bring a Geneanet tree with its photos](@/workflows/geneanet-with-photos.md).

## Export a tree

Exports are in the tree's settings.

1. Open the tree's **Settings**, then **Export Tree** in the left menu.
2. Choose the format: **GEDCOM (.ged)** or **GEDZIP (.gdz)**.
3. For a GEDCOM, choose the **Destination program** (see below).
4. Set the options.
5. Click **Download .ged** or **Download .gdz**, and choose where to save the file.

The message **Export completed successfully.** confirms the export, and every export is recorded in the tree's history.

### Choose the destination program

A GEDCOM file is written for the program or website that will read it. The **Destination program** list offers:

- **Standard GEDCOM 5.5.1**, the default: the version every genealogy program and website reads, with nothing reshaped.
- **Standard GEDCOM 7.0**: the latest version, for programs that read it.
- A program or website by name, such as Geneanet, Ancestry, Heredis or Gramps: the file is shaped the way that program reads it. A line under the list says what changes. Some programs carry a warning that their profile still has to be checked against their latest version.

After an export for a named program, OxidGene tells you what was lost on the way: either “Written for … without losing anything.”, or one or two lists you can unfold, such as **3 limits of … exceeded** and **12 changes lost information for …**, naming each record concerned.

The choice of destination applies to that export only; it is not saved. A GEDZIP has no destination: it always contains standard GEDCOM 7.0.

### Export options

- **Include notes and sources**, on by default. When off, the file contains no notes, no sources with their citations, and no repositories.
- **Include media**, GEDZIP only, on by default. When off, the archive contains the genealogy alone, without any reference to photos or documents.

These options apply to the next export only, and come back to their defaults when you reload the page.

### The submitter

The **Submitter** card says who the exported files come from: **Name**, **Email** and **Address**, saved with **Save**. Without a name, the file names the person you set as **Who am I?** (see [Trees](@/guide/trees.md)), or otherwise “Not Provided”. When you import a file, it fills in those fields that are still empty from the file's own submitter.

### Download a GEDZIP again

A GEDZIP archive is kept for one hour after it is created. During that hour, the export section shows its size and until when it is available, with a **Download again** button: use it if the save failed or if you need a second copy, without packing the tree again. After the hour, export the tree again.

### What other programs see

- A GEDCOM file does not contain the photos themselves, only their paths. Use a GEDZIP to move or back up a tree with its media.
- A document of several pages is written as separate pictures, each linked to the same persons. OxidGene puts them back together when it imports the file again; other programs keep them as separate pictures.
- A portrait cropped from a group photograph cannot be expressed in GEDCOM. After a round trip, that person is shown with their first whole picture; the crop is kept on the photograph.

Learn more: [import specification](@/tech/ui-import.md).
