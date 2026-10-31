+++
title = "Bring a Geneanet tree with its photos"
description = "Import a Geneanet tree into the desktop application together with its photos and documents, each attached to the right persons."
weight = 20
+++

This walkthrough imports a tree you keep on Geneanet into the OxidGene desktop application, with its photos and scanned documents attached to the right persons. It explains each step, what the Geneanet login window does, what you get at the end, and how to save your progress to finish later or on another computer.

## Why a special import

Geneanet's tree export carries at most one portrait per person, often as a link that only works when you are signed in. Its data export holds your original files, but without saying who is on them. Which photo belongs to whom exists only in your Geneanet account.

OxidGene's **From Geneanet** import combines the two: it reads your tree from a GeneWeb export, then signs in to Geneanet through a login window to read the list of your media and who they belong to.

You need:

- the OxidGene **desktop** application. The web version can import the genealogy of a `.gw` file, but not the photos;
- your Geneanet account, and its password;
- a GeneWeb (`.gw`) export of your tree;
- optionally, your Geneanet data archives (see step 2).

## Before you start

Create a new, empty tree for the import on the home page (see [Trees](@/guide/trees.md)). The import adds a complete tree; it does not merge with persons already there, and running it a second time into the same tree does not update what the first run brought in.

Then open that tree's card menu, choose **Import**, and select the **From Geneanet** tab. The steps appear one below the other; one is open at a time. A finished step folds into a one-line summary with an **Edit** button, should you need to go back.

## 1. Your family tree file

1. On Geneanet, export your tree as the step explains:
   - Go to Geneanet → My tree → Operations → Export.
   - Choose the GeneWeb (.gw) format.
   - Tick “Liens web vers les photos principales des individus” and “Images de la chronique familiale”.
   - Download the file.
2. In OxidGene, click **Choose the .gw file** and select it. OxidGene reads it at once and shows the number of persons it holds.

Use the `.gw` export, not the GEDCOM: only the `.gw` tells apart two persons of the same name the way Geneanet does, and that is what lets each photo land on the right person. If you pick a GEDCOM file, the step says so.

### Which media do you want?

The same step asks which copy of each photo to keep:

- **Geneanet’s own copies** (the default): each photo is imported as the largest copy Geneanet serves. Geneanet resizes and recompresses them, and a PDF you uploaded arrives as a JPEG image. Nothing else is needed: this is the quicker path, and the only one if you have never downloaded your Geneanet data.
- **The files you uploaded**: your original files, at full quality, taken from your Geneanet data archives where they can be identified there, and downloaded otherwise. This adds a step for the archives.

Both choices import the same genealogy, the same links between persons and media, and the same identification boxes on group photos. Only the picture files differ.

## 2. Your media archive (originals only)

This step appears only if you chose **The files you uploaded**.

1. On Geneanet, request your data as the step explains: Go to Geneanet → My data → Dashboard, request the download of your data, wait for the email, then download the ZIP files.
2. Click **Add archives…** and select all the ZIP files. Do not unzip them: OxidGene reads them where they are, and Geneanet splits a large export into several files.

You can remove an archive from the list, and adding the same one twice is ignored. If you do not have the archives yet, click **Skip this step**: the media will be downloaded instead. That is slower, but nothing is lost.

## 3. Connect to Geneanet

1. Click **Open the Geneanet login window**.
2. A real browser window opens on Geneanet. Sign in as you normally do, including any captcha or verification Geneanet asks for. OxidGene never sees your password.
3. Once you are in, the window turns into a status panel, **OxidGene is reading your Geneanet media**, while the import window shows the progress.

Do not close the login window yourself: closing it cancels the collection and resets this step. OxidGene hides it, then closes it, on its own once it no longer needs it. If your Geneanet session expires on the way, you can sign in again without losing what was already collected.

The login window is private: it keeps no cookies or sign-in once it closes, so the next import asks you to sign in again.

## 4. What will be imported

This step writes nothing yet. It shows what the import is about to do: the persons in your file, the media found, the persons with media, and notes such as:

- photos showing several persons, which will be attached to each of them;
- multi-page documents, which will be imported whole, with all their pages;
- media identifying persons Geneanet marks “hors de l’arbre”: named on a photo but deliberately not linked to anyone in your tree. A person is created for each of them, with their photos;
- media attached to nobody, which are skipped, and media naming persons your `.gw` file does not contain.

If almost none of the media match the persons of your file, the export and the Geneanet account are probably not the same tree. OxidGene stops and asks: **Go back to step 1** to choose another file, or **Import anyway** if you are sure.

OxidGene then fetches the media through the login window: all of them with Geneanet's own copies, only those your archives do not hold with your original files.

## 5. Import

Nothing more is needed from Geneanet at this point: your media are already on your computer. Click **Start the import**, and leave the window open until it finishes. Storing many media takes a few minutes. To stop it, click **Cancel import** under the progress bar and confirm: everything the import had added is removed, and the tree is left exactly as it was.

When **Import complete** appears, the receipt counts the persons, families, events, sources, places, images and documents imported, and the attachments made between media and persons, couples and events. It also says how many persons were created for people identified on photos but not in your tree, and how many identification boxes were kept to show who is who on a photo. Media that could not be imported are listed, and any warnings sit behind a line you can unfold.

### Same names in your tree

If someone created from a photo bears the same name as a person already in your tree, the receipt lists them under **Same names in your tree**. Geneanet says they are different people, which is why OxidGene never merges them on its own. For each one, either:

- leave **A new person** selected and click **Keep as a new person**, or
- choose the existing person from the list and click **Merge**.

## Finish later, or on another computer

Reading your media list from Geneanet takes a while, and with your original files, matching your archives costs one more request per medium. You can save what was collected and avoid asking Geneanet again. Next to **Reuse a saved connection**, in the connection step and in the review step, you will find:

- **Save what has been collected…** saves the media list after you signed in. A later import that loads it still opens the login window to fetch the media.
- **Save everything, ready to import offline…** appears once the media have been gathered. A later import that loads this file needs no Geneanet connection at all.
- **Load a saved file…** takes a saved file back. OxidGene sees what it holds and asks only for what is missing.

This lets you start again after an interruption, or collect on one computer and import on another. The saved file contains your media list and possibly your photos: keep it private, like your tree.

## Good to know

- An import is all or nothing. If you cancel it, or if it fails, nothing of it stays in the tree: no person and no media. Run it again, using a saved file to avoid asking Geneanet a second time.
- OxidGene never writes anything to Geneanet.
- Only one Geneanet login window can be open at a time.
- An event gets its photo only when the event can be identified exactly; otherwise the photo is attached to the person.

To keep a copy of the result with all its media, export the tree as a GEDZIP (see [Import and export](@/guide/import-export.md#export-a-tree)).
