+++
title = "Person and couple pages"
description = "Everything the application knows about one person or one couple, on a single page: identity, family, events, sources, media, notes and ancestors."
weight = 50
+++

The person page gathers everything recorded about one individual: who they were, their family, the events of their life with their sources, their photos and documents, their notes and their ancestors. The couple page shows two spouses side by side, with what they share drawn once. This page explains how to read both and what you can do from them.

{{ <screenshot name="person" alt="Person page of Étienne Marie Landrevel with portrait, notes and gallery" lang={lang} /> }}

## Open a person's page

- From the [tree view](@/guide/pedigree.md), select a card, then click **Profile view** in the left sidebar.
- From the [search results](@/guide/search.md), click a person.
- From another person or couple page, click any relative's name: every person named on these pages links to their own page.
- From the events panel of the tree view, click one of the person's events.

The top bar shows the tree name, which leads back to the tree view, and the person's name. The left sidebar is the same as in the tree view: **Tree view** returns to the chart centred on this person, and **Couple view** opens their earliest couple.

## The identity header

The header shows the portrait (or a silhouette when there is none), the surname in capitals, the given names, and any other names the person bore, such as a married or maiden name.

Below, a short summary gives their birth and death with the full date and the place, their age at death when both dates are known, their sex, and their occupations. Dates keep their precision: "about 1796", "between 11 Nov 1691 and 20 Aug 1693". When the birth has no date, the baptism is shown instead and the sentence says so ("Baptized on"); a burial likewise stands in for an undated death ("Buried on").

### Badges

- A person with a SOSA number carries the **SOSA** badge.
- The person you chose as **Who am I?** in the tree settings carries a **Me** badge beside it. Click it to change or clear that choice under **Tree & Roots**.

### Occupation and given-name sheets

Hover an occupation or a given name in the header. When OxidGene has a sheet for it, a bubble explains it: what the trade was, or where the given name comes from and what it means. More than 3,000 historical occupations and more than 2,000 given names are covered, recognised whatever the language of the record they were copied from. A sheet not yet written in your language is shown in English. Terms without a sheet stay plain text.

### Header actions

The small icon buttons at the right of the header (their name shows on hover):

- **Edit** opens the person form: see [Editing persons](@/guide/editing.md).
- **Tags…** opens the tag picker: check the tags that describe this person. See [Tags](@/guide/tags.md).
- **Merge with…** opens the merge wizard, to fold another record of the same person into this one. After the merge the page shows the record kept. See [Tools](@/guide/tools.md).
- **History** lists every recorded version of the person, compared side by side, and lets you restore an earlier one. See [History](@/guide/history.md).

## Family connections

**Family Connections** tells the person's family as sentences you can read aloud: whose son or daughter they are, then each union in turn, with the date and place of the marriage and the children it produced, then their siblings. Half-siblings are grouped by the parent they share. Unions are ordered by their earliest dated event, usually the marriage.

Every name is a link to that person's page. The SOSA root and their ancestors carry their mark here as well, so you can see at a glance which relatives are in your direct line.

## The timeline

{{ <screenshot name="person-timeline" alt="Events of Étienne Marie Landrevel with those of his parents, spouse and children" lang={lang} /> }}

**Events** lists, in date order, the person's own events and, around them, those of their close family: their unions, and events of their parents, siblings and children. Events without a date come last. Each line tells you whose event it is:

| Label | Means |
|---|---|
| **Individual** | The person's own event |
| **Conjugal family** | An event of one of their unions, such as the marriage |
| **Child** | An event of one of their children, whose name follows |
| **Parental family** | An event of their parents or siblings, whose name follows |

The person's own events and their unions' stand out from the rest, so the context does not drown their life.

Each event shows its date in full, its place, its description, and, when the record gives it, the person's age at the time ("aged 34 years", "infant", "stillborn"). The responsible agency, when recorded, follows on its own line. Under the event, **Sources** lists the source of each citation with the page or reference it cites.

Documents that prove the event appear as small thumbnails under it. Click one to open it in the media viewer.

### Open the cited register

When a citation names a register of an archive OxidGene knows (every French departmental archive is catalogued), its source becomes a button. Its tooltip names the archive.

1. Click the source.
2. On the desktop, the register opens on the archive's own website, in an archive window, at the cited view. On the web, it opens in a new browser tab.
3. If the citation names the archive but not enough to find the register, a **Find in the archives** dialog asks for the missing details (locality, kind of register, year, view). Fill them in and click **Search**. Tick **Add these details to the citation** to save them in the citation.

The [Sources and archives](@/guide/sources-archives.md) page explains how citations are recognised and what to do when a portal does not answer.

## Media

**Media** shows every photo and document linked to the person: those attached to them, those attached to their couples, and the details of photos where they have been identified, shown as the cropped region rather than the whole picture. A star marks the photo used as their portrait.

- Click a tile to open the media viewer.
- Click the **+** beside the title to add a document.
- Right-click a tile and choose **Link an event** to say which of the person's events it documents, or **Unlink an event** to undo it.

Rearranging what is attached (choosing the portrait, cropping, removing a document from the person) is done in the person form. See [Photos and documents](@/guide/media.md) for the viewer and the document form.

## Notes and ancestors

**Notes** shows the notes written about the person. The section is hidden when there are none.

**Ancestors** draws a small pedigree of the person and up to two generations above them, in the chart theme you chose. Hover a card to read the full name and dates, click it to open that person's page.

## The couple page

{{ <screenshot name="couple" alt="Couple page of a Landrevel couple with their wedding photograph" lang={lang} /> }}

The couple page shows one couple on a single page. Open it with **Couple view** in the left sidebar, from the tree view or a person page; the button is shown only when the person has a known spouse, and opens their earliest couple.

The husband is on the left and the wife on the right. Each spouse has a column with the same sections as their person page:

- their identity, with a **Profile** button to open their own page;
- their own notes;
- their own media only (what is attached to the couple is shown once, across both columns);
- their family: parents, siblings, and their **other** unions with their children;
- their events, without those of this couple;
- their ancestors.

What the couple shares is drawn once across the full width, at the head of each group:

- **Union**: a sentence such as "Married on … in …, and had:", followed by the children of this union;
- **Couple events**: the union's own events (marriage, contract, divorce…) and its children's births, baptisms, deaths and burials, with their sources and documents;
- **Couple media**: the photos and documents attached to the couple, with a **+** to add one;
- **Couple notes**: the notes about the couple.

### Switch to another couple

Above the columns, one selector stands over each spouse. The left selector lists the couples of the spouse on the right, and the right selector those of the spouse on the left. Choose a name to open that couple: to see a woman's second husband, choose him in the left selector. A selector is disabled when there is no other couple to choose.

When a couple records only one spouse, the other column reads **Unknown spouse**.

### Edit the couple

**Edit couple**, at the top right, opens the couple form: the union and its events, the children, privacy, and documents. From there you can also detach a child or delete the couple; deleting it removes the union only, never the persons, and returns you to the tree view. See [Editing persons](@/guide/editing.md#edit-a-couple).

## Print a page

Click **Print** in the left sidebar, or press Ctrl+P (Cmd+P on a Mac). The page prints in full over as many sheets as it needs, in light colours, under a header with the person's or couple's name, the tree name and the date. Buttons and the sidebar do not print.

Learn more: [person page specification](@/tech/ui-person-profile.md) and [couple page specification](@/tech/ui-couple-profile.md).
