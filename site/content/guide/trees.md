+++
title = "Trees and the home page"
description = "Find, create, rename, duplicate, import into, configure and delete your trees, and set each tree's SOSA root and “Who am I?” person."
weight = 20
+++

The home page lists all your genealogy trees. This page explains how to find a tree, create a new one, manage it from its card, and choose the two persons a tree is built around: its SOSA root and you.

{{ <screenshot name="home" alt="Home page with three tree cards" lang={lang} /> }}

## The home page

The home page, titled **My Genealogy Trees**, shows one card per tree. Each card gives:

- the tree's name, and its description when it has one;
- **Recently modified people**: the five persons of the tree you changed most recently, newest first, with their portrait and dates. Imports do not count as changes, so a tree nobody has edited since its import shows **No person modified yet**;
- when the tree was last changed (**Modified today**, **Modified 3 days ago**…), with a **Recent** badge when it was changed in the last 24 hours;
- an **Open** link.

Click anywhere on a card to open the tree. Click one of the recently modified people to open the tree centred on that person.

The gear icon at the top right of the page opens the application settings (see [Settings](@/guide/settings.md)).

## Find a tree

When you have many trees, use the toolbar above the cards:

- Type in **Search a tree…** to keep only the trees whose name or description contains what you typed.
- Choose an order: **Recently modified** (the default), **Name A → Z** or **Name Z → A**.
- Switch between **Grid of trees** and **List of trees** (one card per row) with the two layout buttons.

If nothing matches your search, the page says **No tree found**. When you have no tree at all, it shows **No trees yet** and a **New Tree** button.

## Create a tree

1. Click **New Tree**, or the **Create a new tree** card at the end of the grid.
2. Type a **Name**. It is required.
3. Optionally, add a **Description (optional)**, such as the family, region or period the tree covers.
4. Click **Create**.

The new tree is empty. You can add its first person from the tree view (see [Editing](@/guide/editing.md)), or bring in a file from another genealogy program (see [Import and export](@/guide/import-export.md)).

## Manage a tree from its card

Each card has a three-dot menu at its top right. It offers:

| Entry | What it does |
|---|---|
| **Open** | Opens the tree. |
| **Rename** | Opens **Rename Tree**, where you type the new **Name**, then **Save**. You can also rename a tree from its settings. |
| **Duplicate** | Creates a full copy of the tree, named after it with “ (copy)” added. |
| **Import** | Opens the import window for this tree (see [Import and export](@/guide/import-export.md)). |
| **Settings** | Opens the tree's settings (see [Settings](@/guide/settings.md)). |
| **Delete** | Deletes the tree after you confirm in **Delete Tree**. This cannot be undone. |

Duplicating a tree is a good way to try something risky, such as a large import or a series of merges, on a copy first.

### While an import runs

While a file is being imported into a tree, its card is covered by **Import in progress** and cannot be opened, renamed or deleted. The card comes back by itself once the import has finished, even if you reload the page in the meantime. When an import is cancelled (see [Cancel an import](@/guide/import-export.md#cancel-an-import)), the card shows **Cancelling import…** while the import removes what it added, then comes back with the tree exactly as it was.

## Set the SOSA root

The SOSA root is the person from whom the tree's ancestors are numbered with the Sosa-Stradonitz system: the root is 1, the father of person *n* is 2*n* and the mother 2*n* + 1. The tree view opens on this person, the ancestors show their SOSA number, and the ancestry completeness tool counts generations from them.

1. Open the tree's **Settings**, from the card menu or from the gear icon in the tree's left sidebar.
2. In **Tree & Roots**, find the **Root Person (SOSA 1)** card.
3. Click **Change**, search for the person and select them.

The choice is saved immediately. **Clear** removes it; the card then reads **No root person selected**.

## Say who you are

The **Who am I?** card, just below, designates the person who represents you in the tree. It changes nothing in the genealogy or in the SOSA numbering, but it:

- marks your person with a badge in the tree view, and with a **Me** badge on their profile;
- offers you as a shortcut when you look for the relationship between two persons (see [Kinship](@/guide/kinship.md));
- names you as the sender of exported GEDCOM files when no other name is set.

Click **Change** to choose the person, or **Clear** to remove the choice. The setting belongs to the tree, so everyone using the tree sees the same person.

Learn more: [home page specification](@/tech/ui-home.md).
