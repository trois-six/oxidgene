+++
title = "Frequently asked questions"
description = "Short answers to the questions people ask before and after installing OxidGene."
weight = 170
+++

## Is OxidGene free?

Yes. OxidGene is free software under the GNU Affero General Public License
v3.0: there is no licence to buy and no subscription, and its source code is
public on GitHub.

## Does it work without an internet connection?

The desktop application works offline: your trees, their media and the place
dictionary are all on your computer. It goes online only when you ask it to,
to open a cited register on an archive's portal or to import a tree from
Geneanet. See [Install OxidGene](@/guide/install.md).

## Where are my trees kept, and how do I back them up?

On your computer, in the application's data folder: the
[installation page](@/guide/install.md#where-your-data-is-kept) gives its
place on each system. To keep a copy you can open anywhere, export the tree
as a GEDZIP archive, which carries its photos and documents
([Export a tree](@/guide/import-export.md#export-a-tree)). You can also copy
the whole data folder while OxidGene is closed.

## I use another genealogy program. Can I move my tree?

Yes. Export your tree from that program as GEDCOM (or GEDZIP, if it offers
it), or as a GeneWeb `.gw` file, then import it into a new tree. The
[Coming from another program](@/workflows/first-tree-from-gedcom.md)
workflow walks through it. Geneanet trees can be imported with their photos
on the desktop ([Geneanet with photos](@/workflows/geneanet-with-photos.md)).

## Can I send my tree back to another program?

Yes. The export writes GEDCOM 5.5.1 or 7.0, or GEDCOM tuned for the program
that will read it — Geneanet, Ancestry, Heredis, Gramps and others — and
reports what that program cannot take
([Choose the destination program](@/guide/import-export.md#choose-the-destination-program)).

## I made a mistake. Can I undo it?

Every change is kept. Open the person's history to compare their versions and
restore an earlier one, even after a deletion ([History](@/guide/history.md)).
A merge, however, cannot be undone from the merge wizard.

## Does OxidGene hide living persons?

Not yet. You can already mark a tree, a person, a couple or a media item as
private, and the setting is kept, but nothing is hidden for now: hiding
private and contemporary persons will come with user accounts
([Settings](@/guide/settings.md)).

## Can my family use the same tree?

The web version runs on a server and is used from a browser, so several
people can open the same trees. It has no user accounts yet: anyone who can
reach it can change every tree, so keep it on a private network
([Run the web version](@/guide/install.md#run-the-web-version-for-your-family)).

## Is there a mobile application?

No. The web version adapts its layout to phones and tablets.

## In which languages is OxidGene available?

English, French, German, Spanish, Italian, Dutch, Polish and Portuguese.
You can change the language at any time in the application settings, without
restarting ([Settings](@/guide/settings.md)).

## macOS says the application cannot be opened.

The macOS application is not yet signed by Apple. Open it once, close the
warning, then choose **Open Anyway** in **System Settings → Privacy &
Security** ([Start it on macOS](@/guide/install.md#start-it-on-macos)).

## Where do I report a problem or suggest an idea?

Open an issue on the [GitHub repository](https://github.com/trois-six/oxidgene/issues).
Describe what you did and what you saw, but never attach your real tree:
use invented names if you need an example.
