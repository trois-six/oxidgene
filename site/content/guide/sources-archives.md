+++
title = "Sources and archives"
description = "Cite your sources, record what the record says, and open a cited register on the archive's own portal in one click."
weight = 80
+++

This page explains how to cite the sources of your research in OxidGene — the age a record gives, the agency that kept it and how far you trust it — and how a cited register opens directly on the portal of the archive that holds it.

## Sources, citations and repositories

OxidGene keeps three related things apart:

- A **source** is the document you consulted: a parish register, a civil-status register, a census, a book. It has a title and, optionally, an author, an abbreviation and a publication.
- A **citation** links a person, an event or a union to a source, and says where in the source the information is (a page, a view, an act number). It can also hold the text you extracted and a reliability level.
- A **repository** is the archive, library or office that holds a source, with the call number under which it is kept there.

One source is usually cited many times: the same register proves the births of several children. A citation belongs to one person, event or union.

## Cite a source

You cite sources from the person edit form (see [Edit persons and families](@/guide/editing.md)).

1. Open the person's edit form.
2. Find the event you are documenting: the **Birth** or **Death** section, or an event of the form's list of events (expand a saved event to see its fields).
3. Type the source in its **Source** field, as you read it off the record: a register, an archive reference or a document title.
4. Save the form, or the event's own panel.

The **Source** field suggests the titles of the sources already in your tree as you type. When you save, a title that matches an existing source (ignoring case) reuses it; any other title creates a new source. If you later correct the title, the citation moves to the corrected source, and the old source is removed if nothing else uses it any more.

There is also a **Source** field in the civil-status part of the form, for a source about the person as a whole rather than a single event, and one on each event of a union in the couple form.

## Record the age, the agency and the reliability

Records often say more than the date and place. In the edit form's list of events, each event can also keep:

- **Age**: the age the record gives for the person, typed the way GEDCOM writes it: `34` or `34y`, `1y 6m`, `< 1y`, `> 80y`, `CHILD`, `INFANT` or `STILLBORN` (`y` years, `m` months, `w` weeks, `d` days). A value that is not an age is refused with a message when you save.
- On a marriage or another union event, an **Age of …** field for each spouse, since such a record gives one age per spouse.
- Under **More details**, the **Responsible agency**: the authority that kept the record, such as a parish or a registry office.
- Under **More details**, once the event is saved and has a source, the **Source reliability** of its citation: **Not assessed** (the default), **Unreliable**, **Questionable**, **Secondary**, **Direct** or **Certain**.

**More details** opens by itself when an event already has an agency or an assessed citation, so nothing you recorded stays hidden.

## Where your sources appear

On a person's profile, each event of the timeline lists its sources after **Sources:** — the source title followed by where in the source the information is. The recorded age appears in words after the place, for example "aged 34 years", "infant" or "stillborn", and the agency appears on its own line as **Responsible agency: …**.

The [Dictionary](@/guide/dictionary.md) lists every source of the tree with its number of citations and the persons it documents. That is where you edit a source itself — its title, author, abbreviation, publication, responsible agency — and the repositories that hold it with their call numbers.

## Open a cited register on the archive's portal

Most genealogical sources are pages of registers that public archives have digitized and published on their own portals. When a citation names such a register, its source becomes a button on the person's profile. Hover over it: the tooltip reads **Open the cited view at …** with the name of the archive. Click it, and OxidGene finds the register on the archive's portal and opens it at the cited view, instead of leaving you to repeat the portal search by hand.

{{ <screenshot name="archive-register" alt="A birth's source naming a register of the Loire-Atlantique archives, outlined on a person's profile, and the archive portal's tab it opened on the cited register, whose page is blurred" lang={lang} /> }}

The catalogue covers the French departmental archives and the overseas civil status of the Archives nationales d'outre-mer. It knows parish and civil-status registers, decennial and parish tables, population censuses, military registers, conscription lists, tables of successions and absences, and cemetery burial registers.

### How a cited register is recognised

You do not need to follow one convention. OxidGene reads the source title and its other fields, the citation's page, the repositories holding the source with their call numbers, and the kind, year and place of the cited event. These all work for the same act:

```text
AD72 - Exampleville - (aucun) - N - 1872 - 9E 999/9 - acte 31 - vue 45/200
AD72, état civil de Exampleville, naissances 1872, cote 9E 999/9, acte 31, vue 45/200
Registres paroissiaux et d'état civil — vue 45, acte 31
```

The first line is the normalized form (archive code, locality, parish, act, period, call number, act number, view), read first and most strictly. The second is the usual written description. The third works when the source is held by the archive as its repository and the citation is attached to the birth in Exampleville in 1872: the event supplies the kind of act, the year and the locality that the text leaves out.

A few rules help:

- The archive must be named: by its code (`AD72`), its name, a repository holding the source, or the address of its website. The place of an event never designates an archive, because an act is not always recorded where the event happened.
- A citation's page completes its source: the views and act number given in the page win over those in the title.
- If the source or citation contains the address of a page on the archive's portal, that address opens as it is, without any search.
- Citations may be written in any language of the interface — English, French, German, Spanish, Italian, Dutch, Polish or Portuguese: `AD72, civil registration of Exampleville, births 1872, image 45` opens the same register as its French counterpart.

A citation that names no archive, an archive outside the catalogue, or a kind of document the archive does not publish stays plain text.

### On the desktop: the archive window

On the desktop application, the register opens in a separate archive window beside the main window, which stays where you were. Archive portals do not allow their pages to be shown inside another application, so the window shows the portal itself.

While OxidGene searches, the window shows a progress cover with the archive, the citation and the current step: **Connecting to the archive portal…**, **Searching for the register…**, then **Opening view …**. After a few seconds, the time spent is shown beside the step. Click **Cancel** to stop the search: the window then shows the portal's search page instead.

Once the register is shown, you browse the portal freely. A banner over the page tells you when something needs your attention:

- No register, or several registers, match the citation: the window shows the portal's search results filtered by the citation, so you can choose.
- The view numbering has changed since the citation was written (the register was digitized or bound again): the banner gives the old and new counts and the estimated view, which may not be the cited page.
- The portal's viewer always opens on the first view: OxidGene types the cited view in the viewer's own page box for you, and only asks you to go to the view yourself if that fails.

The archive window takes care of the portal's small hurdles the way you would, but never decides for you:

- **Cookie banners**: OxidGene refuses the portal's cookie consent on your behalf and never accepts it. A banner that only offers to accept is left to you.
- **Anti-robot checks**: some portals check that a visitor is not a robot. A check that clears itself within a few seconds is simply waited out. Otherwise the banner asks you to complete the check in the window, and the search continues once you have. OxidGene never answers a check itself.
- **Reuse licences**: when a register sits behind the archive's reuse licence, the window opens the licence page and asks you to accept it there. Once you have, it goes on to the register by itself.

The archive window keeps the portals' cookies between uses, so a licence or an anti-robot check usually has to be passed only once. On Linux, if a portal's security certificate cannot be verified, the window says so and offers **Open in the browser**.

### On the web: a new tab

In the web application, the register opens in a new browser tab. The tab opens at once with **OxidGene is looking for the cited register…**, then moves to the register when it is found. Some portals can only be searched from a browser: the tab then opens on the portal's search results filtered by the citation, and you open the register from there.

The tab cannot carry OxidGene's banner, so the same messages appear beside the source on the profile instead: no register or several, a changed numbering, the view to go to. For a register behind a reuse licence, the tab opens the licence: accept it on the portal, then look up the register there. A strict pop-up blocker may prevent the tab from opening; allow pop-ups for OxidGene if it does.

### When the search fails

If the portal does not answer in time, cannot be reached, or asks for an anti-robot check that is left unanswered, the window or tab opens the portal's search page filtered by the citation, with a message, so you can continue from there. For any other failure, it opens the archive's website. Nothing is retried behind your back: click the source again to try once more.

## Complete a partial citation

Sometimes OxidGene recognizes the archive but not the act or the locality, for example a citation that reads only "AD72, vue 45". Its source is still a button, but it opens the **Find in the archives** dialog instead.

1. Click the source on the person's profile.
2. In **Find in the archives**, check what was recognized. The archive is shown and cannot be changed.
3. Fill in what is missing: **Locality**, **Kind of register** (only the kinds that archive publishes are offered), **Year** and **View**.
4. Click **Search**. The register opens exactly as for a complete citation, in the archive window or a new tab.

The locality and the kind of register are required; without them the dialog stays open and says what is missing.

Searching saves nothing. To keep what you typed, tick **Add these details to the citation** before clicking **Search**: your details are added at the end of the citation's page, in the language of the interface (for example `Exampleville, birth 1872, image 45`), the change is recorded in the [history](@/guide/history.md), and the next click opens the register directly. The source itself, which other citations may share, is never changed.

## What OxidGene sends to the archives

Archive portals are public services, and OxidGene uses them the way a reader does:

- One search per click. OxidGene never browses, collects or prefetches portal pages on its own.
- The portal receives what its search form needs: the locality, the kind of record, the year or period and the call number. A collection searched by person's name, such as military registers, also receives the surname, and only when nothing else can find the person's row.
- OxidGene does not download, copy or store archive images. To keep a copy of an act in your tree, add it yourself as a document (see [Media](@/guide/media.md)).

Learn more: [archive portals specification](@/tech/archives.md).
