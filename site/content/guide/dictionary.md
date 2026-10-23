+++
title = "Dictionary"
description = "Browse the family names, sources, repositories, places and occupations of a tree, and rename or merge them in bulk."
weight = 110
+++

The dictionary lists every distinct value entered across a tree — family names, sources, repositories, places and occupations — once each, with how often it is used. It is the place to spot a surname spelled two ways, a source cited from a dozen places or a place entered slightly differently each time, and to fix them for every record that uses them at once: rename or merge a family name or an occupation, and edit or merge a place or a repository.

## Open the dictionary

Click the **book icon** (**Dictionary**) in the left icon sidebar of any tree page: the pedigree, a person or couple profile, the statistics. The page opens on the **Family Names** tab.

The tabs along the top are **Family Names**, **Sources**, **Repositories**, **Places**, **Occupations**, **Media** and **Tags**. The **Media** tab is the tree's media library, described in [Media](@/guide/media.md). The **Tags** tab lists the tree's tags, described in [Tags](@/guide/tags.md).

If you come back to the dictionary with the back button, it reopens exactly as you left it: same tab, filter, letter, page and open row.

{{ <screenshot name="dictionary" alt="Family Names tab of the dictionary, filtered on LANDREVEL, with the persons carrying the name listed below it" lang={lang} /> }}

## Browse and filter a list

The **Family Names**, **Places** and **Occupations** tabs work the same way:

- **Letters**: click a letter to show only the entries starting with it, or **All** to show everything again. Letters with no entry are greyed out.
- **Filter**: type in the **Filter...** field to narrow the list as you type. The filter combines with the selected letter.
- **Count**: the number of matching entries appears at the right of the letters, for example "142 entries".
- **Page size**: choose 25, 50, 100 or **All** entries per page. Showing all the entries of a very large list can be slow, and the page warns you when it has more than 500.

The entries are grouped under their first letter. Click an entry, or the arrow at the end of its row, to unfold the persons who use it. Each person is a link.

When nothing matches, **Clear filter** brings the whole list back.

## Family names

Each row is one family name exactly as spelled, with the number of persons who carry it, whether as their main name or as another name (an alias, a married name). The list does not merge spellings: "Martin" and "MARTIN" are two rows, and so are "Cruz" and "de la Cruz". This is deliberate, so you can see the variants; renaming (below) is how you unify them.

Unfold a family name to list the persons who carry it; click a person to open them in the tree.

Where a name with a particle such as "de la" or "d'" is filed depends on the **Surname sorting** option of the [application settings](@/guide/settings.md). With **Particle included**, "de la Cruz" files under D. With **Particle ignored**, it files under C and reads "Cruz (de la)", so the C group can be read by the word it is sorted on.

## Rename or merge a family name

Open the **⋮** menu at the end of a family name's row and choose **Edit** to open **Edit family name**.

1. Change the **Name** field. If family names are written in capitals (**Entry Options** in the application settings), the field does too.
2. Read the line below it: it says how many persons will be renamed.
3. Click **Rename**.

Only the persons who carry this name as their **main** name are renamed. Persons who carry it only as a secondary name (an alias, a married name) keep it, and the dialog tells you how many they are; the name then stays in the dictionary for them.

If the new name already exists in the tree, the dialog warns you that the two will be merged. The renamed persons join the existing name and take its particle; to change that particle, edit the existing name's own row.

A rename is recorded in the tree's [history](@/guide/history.md) as one entry, and each renamed person gets a new version. There is no undo for the whole rename: to put a person back, restore them from their own history.

## Correct where a particle splits

When a tree is imported, OxidGene guesses which leading words of a family name are a particle ("de", "le", "van der"…). A wrong guess affects every person with that name at once, for example "Le …" names filed under their second word. To fix it:

1. Open the **⋮** menu of the family name's row and choose **Edit**.
2. Leave **Name** unchanged and edit the **Particle** field. Empty it when the leading word is part of the surname.
3. Check the preview, which shows the **Particle**, the **Surname** and the letter it **Files under**.
4. Click **Apply to all**.

The particle must already be at the start of the name: OxidGene never adds a word the name does not contain. The name itself is unchanged, only where it splits and the letter it files under.

## Sources

Trees often hold thousands of sources, many starting the same way ("AD44 …"). Instead of a plain A–Z list, the **Sources** tab lets you narrow them step by step:

1. **Choose a letter:** only the letters that begin at least one source are offered.
2. While more than 250 sources remain, the tab offers the next possible continuations (for example "AD4", then "AD44 - "), each with its number of sources. Steps with only one possible continuation are skipped for you.
3. Once 250 sources or fewer remain, they are all listed at once, without pages.

The path you have taken is shown as a breadcrumb starting at **All sources**; click any part of it to go back up. The **Filter...** field narrows the current level.

A group whose label is exactly the one above it holds the sources titled exactly like that label, with no more characters. Click it to list only those sources: the breadcrumb then reads "Titled exactly “…”".

Each source row shows its title, its author and the repositories that hold it, and its number of citations. Unfold it to see the persons the source documents: those cited directly, the person of a cited event, and both spouses of a cited union or union event.

### Edit a source

Open the **⋮** menu at the end of a source's row and choose **Edit** to open **Edit source**:

- **Title** (required), **Author**, **Abbreviation** and **Publication**;
- under **More details**: the **Responsible agency**, and the **Repositories** holding the source. Choose a repository of the tree, or **New repository…** to create one, then give an optional **Call number** and **Medium**, and click **Add the repository**. A source held under two call numbers at the same repository has two rows.

Repository rows are added or removed immediately; the other fields wait for **Save**.

## Repositories

The **Repositories** tab lists the archives, libraries and offices that hold your sources, sorted by name, with the first line of their address, their phone and their website. Type in **Filter...** to narrow the list.

- Unfold a repository to see the sources it holds, with their call numbers and medium.
- Choose **Edit** in a repository's **⋮** menu to edit it, or **Add a repository** to create one. A repository has a **Name** (required), an **Address**, a **Phone**, an **Email**, a **Website** and notes.
- When you give a repository the name of another one (capitals, accents and punctuation aside), the editor offers to merge them: see [Merge two places or two repositories](#merge-two-places-or-two-repositories).
- **Delete the repository**, at the bottom of its editor, removes it after confirmation. The sources it held stay in the tree; they are simply no longer listed as held there. Undoing the deletion from the history brings the links back.

## Places

Each row is a place as entered, with a pin that is filled when the place has map coordinates and muted when it does not, and the number of events and media that use it. Unfold a place to list the persons concerned: the persons of its events (both spouses for a couple's event) and the persons linked to its media.

### Edit a place

Open the **⋮** menu of a place's row (or right-click it) and choose **Edit** to open **Edit place**. Change its **Name**, its **Latitude** and its **Longitude** (leave both empty for no coordinates), then click **Save**: every event and media at the place shows the change.

### Merge two places or two repositories

When a place or a repository was entered twice, give one of them the other's name in its editor. Capitals, accents and punctuation do not count, so "SAINT-ALBAN" matches "Saint Alban". The editor then:

1. Warns that the name is another place's (with its number of uses) or another repository's. When several match, choose the one to merge into in **Merge into**.
2. Ticks **Merge with "…"**. The box starts unticked when the two already had the same name, since two places can share a name and be different places: tick it yourself to merge them.
3. Lists the details the two hold differently, a place's name and coordinates, a repository's name, address, phone, email and website, each with a choice between **This one** and the other. A detail only one of them has is kept.
4. Click **Merge**.

The other record stays, with the details you chose. A merged place takes over every event and media of this one, and its tags. A merged repository takes over the sources this one held, its notes and its tags; a source already held there under the same call number is not listed twice. This one is then deleted. The merge is one entry of the tree's [history](@/guide/history.md).

## Occupations

Each row is an occupation as entered, with the number of persons who held it. Unfold it to list them.

### Rename or merge an occupation

Open the **⋮** menu at the end of an occupation's row and choose **Edit** to open **Edit occupation**. Change the **Occupation** field, read the line below it, which says how many persons will be renamed, and click **Rename**. Every occupation event recording it takes the new label. If the new label already exists, the dialog warns you that the two will be merged.

A rename is recorded in the tree's [history](@/guide/history.md) as one entry, and each renamed person gets a new version, which you can restore person by person.

## Print a list

The print button in the top bar prints the current tab's list. A paged list prints the page on screen; choose **All** as the page size first to print the whole list.

Learn more: [dictionary specification](@/tech/ui-dictionary.md).
