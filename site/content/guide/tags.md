+++
title = "Tags"
description = "Label persons, couples, documents, events, sources, citations, places, repositories and notes with tags of your own, colour them, find them in search and manage them in the Dictionary."
weight = 75
+++

A tag is a short label you put on records to find them again: *to verify*, *record not found*, *Soldiers 1914–1918*. Tags are your own vocabulary. OxidGene ships none, and it never translates the ones you write. A tree has one vocabulary: a tag created on one person is the same tag on every other record.

Tags go on **persons**, **couples**, **documents** (photos, scans and PDFs), **events**, **sources**, **citations**, **places**, **repositories** and **notes**.

## Add or remove a tag

On a person's page, click the **Tags…** button in the header. On a couple's page, it is in the actions row. On a document, use **Tags…** in the media viewer's menu or in the tile's menu.

On an event, open **Tags…** from the event's row in the timeline: right-click the row, or press the context-menu key or Shift+F10 when the row has the focus. A citation in the timeline's sources line and a source in the Dictionary's **Sources** tab open the same menu. A place in the **Places** tab and a repository in the **Repositories** tab do too, and so does a note in a person's or a couple's page: right-click it, or press the context-menu key or Shift+F10 when it has the focus.

The picker lists the tree's tags, with the ones already on the record first:

- Type in the field to narrow the list. Accents and capital letters do not matter.
- **Check** a tag to put it on the record, and **uncheck** it to take it off. Each change is saved at once: there is no **Save** button.
- When no tag matches what you typed, choose **Create** to make a new tag and put it on the record. A new tag gets a colour that the tree's other tags do not use yet. You can change the colour later in the Dictionary.
- With the keyboard, the arrow keys move, Space or Enter checks a tag, and Escape closes the picker.

Two spellings that differ only by case or accents are one tag: typing *a verifier* finds the tag you created as *À vérifier*.

## Tag several persons at once

Two actions tag a group of persons in one step.

- **Tag a branch…** is in the pencil menu of a person in the tree. It asks which persons to tag: the ancestors, the descendants, or both. You also choose how many generations (empty for all, up to 30), whether to include the person and, for descendants, whether to include their spouses. **Continue** opens the picker on those persons. A person who is both an ancestor and a spouse of a descendant is counted once.
- **Tag the results…** is in the search results toolbar. It tags every person that the search and its filters return, not only the page on screen. It is unavailable when there is no result.

The picker then lists each tag with how many of these persons carry it, as *3 of 12*. **Add to all** puts the tag on every person of the group who lacks it, and **Remove from all** takes it off those who carry it. Each button applies at once, and the audit log records one entry for it. When more than 100 persons are concerned, OxidGene asks you to confirm first. Tab moves between the buttons and Enter presses the focused one. To create a tag, use the picker of a single person first: the bulk picker only lists the tags that exist.

## Highlight a tag in the tree

To see where the persons of a tag sit in the tree, press **Highlight a tag** in the tree's left sidebar and choose a tag from the list. The list shows the tags that at least one person carries. Each of these persons gets a dot in the tag's colour at the top corner of their card and a border in that colour, in every chart: the tree, the wheel, the fan, the lineage, the hourglass and the bowtie. The other cards are dimmed; untick **Dim the others** to leave them as they are. A chip under the button shows the tag, and its × ends the highlight.

The highlight stays while the page is open, and when you come back to the page with the history buttons. It is not saved in the settings. Printing keeps the marks, not the dimming.

## Where tags show

Tags take no room on a record that has none.

- On a person's or couple's page, their tags appear as chips under the header, each with its colour dot.
- In search results, each person's tags appear as small dots after the name. Hover over a dot to read its tag.
- On the timeline, an event or a citation shows its dots after its text. In the Dictionary's **Sources** tab, a source shows its dots after its title. In the **Places** and **Repositories** tabs, a place or a repository shows its dots after its name.
- A note shows its dots under its text in a person's or a couple's page, and nothing at all when it has no tag.
- Click a chip or a dot to open the Dictionary's **Tags** tab on that tag.

## The Tags tab

Open the **Dictionary** from the left sidebar and choose the **Tags** tab. Each row is a tag, with its colour and the number of records that carry it, by kind: persons, couples, documents and so on. Click the arrow of a row to list those records, grouped by kind, and open any of them. An event or a citation opens the profile of its person or couple, a source opens the **Sources** tab, a place the **Places** tab and a repository the **Repositories** tab. A note opens the profile of the person or couple it belongs to.

- **Edit** (the pencil) changes the label and the colour. If the new label is the same as another tag's, OxidGene offers to **merge** the two: every record carrying either tag then carries the merged tag once.
- **Delete** (the ×) removes the tag from every record that carries it. The confirmation says how many records that is. It cannot be undone.
- **New tag** creates a tag that you will put on records later.

The **Media** tab keeps its tag cloud, which lists the tags carried by documents.

## Find persons by tag

In **Search**, open the filters and choose **Tags**. Type a tag name and choose it: each chosen tag becomes a chip, and the results keep only the persons carrying **every** chosen tag. See [Search](@/guide/search.md).

## Tags and files

A GEDCOM or GEDZIP export keeps your tags, and importing that file into OxidGene brings them back, with their colours. Other genealogy programs ignore them. A GeneWeb `.gw` file has no place for tags, so it carries none.

Learn more: [tags specification](@/tech/tags.md) and [data model](@/tech/data-model.md).
