+++
title = "Tags"
description = "Label persons, couples and documents with tags of your own, colour them, find them in search and manage them in the Dictionary."
weight = 75
+++

A tag is a short label you put on records to find them again: *to verify*, *record not found*, *Soldiers 1914–1918*. Tags are your own vocabulary. OxidGene ships none, and it never translates the ones you write. A tree has one vocabulary: a tag created on one person is the same tag on every other record.

For now, tags go on **persons**, **couples** and **documents** (photos, scans and PDFs). Events, sources, citations, places, repositories and notes will be taggable later.

## Add or remove a tag

On a person's page, click the **Tags…** button in the header. On a couple's page, it is in the actions row. On a document, use **Tags…** in the media viewer's menu or in the tile's menu.

The picker lists the tree's tags, with the ones already on the record first:

- Type in the field to narrow the list. Accents and capital letters do not matter.
- **Check** a tag to put it on the record, and **uncheck** it to take it off. Each change is saved at once: there is no **Save** button.
- When no tag matches what you typed, choose **Create** to make a new tag and put it on the record. A new tag gets a colour that the tree's other tags do not use yet. You can change the colour later in the Dictionary.
- With the keyboard, the arrow keys move, Space or Enter checks a tag, and Escape closes the picker.

Two spellings that differ only by case or accents are one tag: typing *a verifier* finds the tag you created as *À vérifier*.

## Where tags show

Tags take no room on a record that has none.

- On a person's or couple's page, their tags appear as chips under the header, each with its colour dot.
- In search results, each person's tags appear as small dots after the name. Hover over a dot to read its tag.
- Click a chip or a dot to open the Dictionary's **Tags** tab on that tag.

## The Tags tab

Open the **Dictionary** from the left sidebar and choose the **Tags** tab. Each row is a tag, with its colour and the number of records that carry it, by kind: persons, couples, documents and so on. Click the arrow of a row to list those records, grouped by kind, and open any of them.

- **Edit** (the pencil) changes the label and the colour. If the new label is the same as another tag's, OxidGene offers to **merge** the two: every record carrying either tag then carries the merged tag once.
- **Delete** (the ×) removes the tag from every record that carries it. The confirmation says how many records that is. It cannot be undone.
- **New tag** creates a tag that you will put on records later.

The **Media** tab keeps its tag cloud, which lists the tags carried by documents.

## Find persons by tag

In **Search**, open the filters and choose **Tags**. Type a tag name and choose it: each chosen tag becomes a chip, and the results keep only the persons carrying **every** chosen tag. See [Search](@/guide/search.md).

## Tags and files

A GEDCOM or GEDZIP export keeps your tags, and importing that file into OxidGene brings them back, with their colours. Other genealogy programs ignore them. A GeneWeb `.gw` file has no place for tags, so it carries none.

Learn more: [tags specification](@/tech/tags.md) and [data model](@/tech/data-model.md).
