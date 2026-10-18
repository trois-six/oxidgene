+++
title = "Photos and documents"
description = "Add photos, scans and multi-page documents, choose portraits, identify people on a picture, and browse the whole media library."
weight = 70
+++

OxidGene keeps photos, scans of records, PDFs and videos beside the persons, couples and events they concern. This page explains how to add them, how to choose a portrait and identify someone on a picture, how to use the media viewer, and how to browse every document of a tree in the media library.

## One kind of thing: a document with pages

A single photograph and a forty-page notarial act are the same kind of record in OxidGene: a **document** that describes it (title, date, place, kind of record…) and one or more **pages** that hold the files. A page is either a file you upload or the address of a file held elsewhere. Each page can have its own transcript.

## Add a document

You can add a document from several places; each opens the same form:

- on a person's page or a couple page, the **+** beside **Media** (or **Couple media**);
- in the person form or the couple form, the **Add a document** cell of the **Media** section (the person must be saved once first);
- in the panel of a saved event, under **Evidence**, to add the documents that prove that event.

Then:

1. Fill in what describes the document: **Title**, **Description**, **Tags** (**Add a tag**), **Kind of record**, **Physical medium**, **Privacy**, **Date**, **Place** and **Note**. All are optional. Choosing a kind of record (for example **Civil registration**, **Parish register**, **Census**, **Portrait** or **Group photo**) also fills in the physical medium, unless you set it yourself.
2. Under **Documents these events**, tick the events the document proves.
3. Under **Pages**, add the files: click **Select files** or drop files on the cell, one image per page in reading order. To use a file held on another website, click **Link an address** and paste its URL: OxidGene records the address and never downloads the file.
4. Reorder pages with **Move earlier** and **Move later**, or remove one with **Remove from this document**.
5. Click **Save**.

Nothing is written until you save, and cancelling leaves nothing behind. A document needs at least one page or link: "Add at least one page or link before saving."

A document added from the person or couple form is written when you save the document form, independently of that form: cancelling the person or couple form afterwards does not remove it.

## Arrange the documents of a person or couple

In the **Media** section of the person form or the couple form, each tile has small buttons (their name shows on hover):

- **Use as profile image** (★): makes the image the person's portrait, shown on their tree card, page and events panel. A PDF cannot be a portrait, even when its tile shows the scan it holds.
- **Crop a region** (✂): drag a rectangle on the image to keep only part of it, such as a face on a group photo or one entry on a register page, then click **Save region**. You can say which event the region documents.
- **Edit** (✎): changes the document's title, description, date, place and other details.
- **Remove from this record** (🗑): detaches the document from this person or couple after a confirmation. The document itself stays in the tree.

On the person page, the gallery is read-only, but you can right-click a tile and choose **Link an event** to say which event it documents, or **Unlink an event**. Under the tile's title, each linked event shows its date and type.

## Identify someone on a photo

A family photo or a register page often shows several people. Rather than copying the picture, you mark a region of it for each person. The region, a **vignette**, then appears in that person's gallery as the cropped detail, and can be used as their portrait.

1. Open the photo in the viewer.
2. Click the target icon beside the zoom buttons and choose **Identify a person**.
3. Drag on the image to select the region ("Drag on the image to select a region.").
4. Choose the person, then save the region.

Back in the viewer, hover a region or its row in the facts column to see its frame and the person's name.

## The media viewer

Click any tile to open the document in the viewer. It fills the window and shows the page with a column of facts beside it.

- **Zoom in**, **Zoom out** and **Fit** size the image; drag it when it overflows.
- A PDF opens as a document to read: its pages follow one another as you scroll, and you can select and copy its text and follow its links. Type in **Search the document** to find a word: Enter, or the arrows beside the field, move from one match to the next, and the count says which one is shown. **Zoom in**, **Zoom out** and **Fit** (the width of the page) redraw the pages sharply, and the page you are reading is shown with the number of pages. A PDF that cannot be opened offers its download instead. In a gallery, the tile of a PDF shows its first page.
- A multi-page document has a pager to move between its pages. Each page can have its own transcript, shown with the page number.
- **Attachments / identifications** lists everyone the document is attached to and everyone identified on the page shown, five at a time. The × at the end of a row removes that link only. A row whose medium cannot be shown as a picture, such as a PDF of text, shows the person's portrait, or a silhouette when they have none; a couple shows a silhouette.
- **Edit** changes the document's details and the page's transcript. **Delete** deletes the document permanently, with its files, after a confirmation.
- The target icon opens a menu: **Identify a person**, **Attach to a person** and **Attach to a couple**. For a multi-page document it offers to attach the whole document or only the page shown, to a person or a couple. To attach to a couple, search for one of the spouses, then choose the couple. An existing link is never duplicated.
- **Manage pages** (multi-page documents): **Add pages**, move pages, or delete one page with its transcript and links.
- **Download file** saves the file; for a multi-page document, **Download page** saves the page shown and **All pages (ZIP)** saves the whole document. A page held elsewhere offers **Open the original link** instead.

## The media library

{{ <screenshot name="media-library" alt="Media library of the Landrevel tree with its tag cloud" lang={lang} /> }}

To browse every document of a tree, open the **Dictionary** from the left sidebar and choose the **Media** tab. A multi-page document is one tile.

- **Tag cloud.** Every tag in use, with the number of documents carrying it; the more used, the larger. Click tags to keep only the documents carrying all of them; click a selected tag again to drop it, or **All** to clear the selection.
- **Name filter.** "Filter by title or file name…" matches a document's title or the file name of one of its pages.
- **Per page.** Show 25, 50 or 100 documents per page. The count beside it is the number of documents matching every filter.
- **Filters.** Open the panel to narrow the list by **File type** (image, PDF, video, audio, other), **Category**, **Linked person** (attached to the person, one of their couples or one of their events, or identified on the document), **Linked event between** two years, and **Added between** two days.

Each active filter is shown as a chip: click it to remove that filter, or click **Clear all filters**. Under each tile, the caption gives the kind of record and how much the document is used, "Linked to 3 records" or "Not linked".

Click a tile to open the viewer, where you can edit the document's details and tags. From the library, a document can only be deleted when nothing references it any more.

The [Dictionary](@/guide/dictionary.md) page describes the other tabs.

Learn more: [common components specification](@/tech/ui-common.md) and [dictionary specification](@/tech/ui-dictionary.md).
