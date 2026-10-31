+++
title = "Settings"
description = "Tree settings (roots, privacy, anomaly thresholds, history, export) and application settings (themes, language, pedigree, names, date display, entry options, API and AI assistant)."
weight = 150
+++

OxidGene has two kinds of settings. **Tree settings** belong to one tree and apply to everyone who uses it. **Application settings** are your own preferences on this device: the look of the application, its language, how pedigrees are drawn, how names are sorted, how dates are written and how forms help you type. This page goes through both.

Every setting is saved as soon as you change it; there is no save button to remember, except for the tree name, the submitter and the anomaly thresholds.

## Open the settings

- **Tree settings**: on the home page, open the tree card's three-dot menu and choose **Settings**, or click the gear icon in the left sidebar of any page of the tree.
- **Application settings**: click the gear icon at the top right of the home page.

{{ <screenshot name="tree-settings" alt="Settings page of a tree, with its left menu" lang={lang} /> }}

The tree settings' left menu has four groups: **Settings** (**Tree & Roots**, **Privacy**, **Anomaly thresholds**), **Tools** (**History**), **Export** (**Export Tree**), and **Global Preferences**, which repeats the application settings so you can reach them from a tree.

## Tree & Roots

- **Tree name**: type the new name and click the disk icon beside it.
- **Root Person (SOSA 1)** and **Who am I?**: the persons the tree's numbering starts from and who represents you. See [Trees](@/guide/trees.md#set-the-sosa-root) for what each does.
- **Default privacy**: the same control as in the Privacy section below.

## Privacy

**Default privacy** says what “follows the tree” means for every person, couple and document of the tree: **Private** or **Public**. Each person, couple or document can still choose otherwise in its own form; the tree answers only for those that have not.

A new tree starts as Private: a genealogy holds living people, and publishing should be a deliberate choice.

Privacy is recorded today but does not hide anything yet. As the section says: **Recorded now. Nothing is hidden yet — this takes effect when accounts and publishing arrive.**

## Anomaly thresholds

{{ <screenshot name="tree-settings-anomalies" alt="Anomaly thresholds section of the tree settings, its thresholds in four cards: dates, filiation, unions and places" lang={lang} /> }}

**Anomaly thresholds** sets the limits the [anomaly rules](@/guide/tools.md#find-anomalies) of the Tools page apply to this tree: an age, a gap or a distance is reported only beyond them. The defaults suit most trees; change them to fit a population or a period, such as a tree of long-lived ancestors or of marriages made very young.

The thresholds are grouped in four cards, as the anomalies are: **Dates** (longest life, longest life before a given year, that year, oldest person without a death, recorded age tolerance, approximate date slack), **Filiation** (youngest parent, oldest father and mother, birth after the father's death, twins, siblings too close or too far apart), **Unions** (youngest and oldest spouse at the union, spouses' birth gap) and **Places** (farthest places of one day, in km).

1. Type the new values. A value out of its range says so under its field ("Between 80 and 150."), and two values that must agree say so above the buttons; **Save** stays disabled until they are fixed.
2. Click **Save**. The Tools page then applies the new thresholds, and its rules quote them in their titles and sentences.

**Restore the defaults** fills every field with its default value; click **Save** to keep them. Each change is recorded in the tree's [History](#history), threshold by threshold.

## History

**History**, in the **Tools** group, is the tree's log: every change, newest first, including settings, media, imports, exports and restores. Filter it with **All**, **Data**, **Settings**, **Media**, **Imports**, **Exports** and **Restores**, and click **Load more** to go further back. **Show the change** compares a record before and after, and **Restore the previous version** undoes that change for that record. See [History](@/guide/history.md).

The other tools of a tree (anomalies, places not located, duplicates, date converter…) are on the Tools page, reached from the wrench icon of the tree's sidebar. See [Tools](@/guide/tools.md).

## Export Tree

Download the tree as GEDCOM or GEDZIP, choose the destination program and the options, and set the submitter. See [Import and export](@/guide/import-export.md#export-a-tree).

## Application settings

The application settings have seven sections: **Appearance**, **Language**, **Pedigree**, **Names**, **Date Display**, **Entry Options** and **API**. All but API also appear under **Global Preferences** in every tree's settings; it is the same preference in both places, not a setting of that tree, and it applies to every tree you open on this device.

These preferences are kept on this device. On another computer, or in another browser, you choose them again.

{{ <screenshot name="app-settings-dark" alt="Theme picker of the application settings, in the dark theme" lang={lang} /> }}

### Appearance

The **Theme** card shows every theme as a small preview tile: its colours, typefaces, corners and spacing. **Light** and **Dark** are among them. Click a tile and the whole application changes at once.

OxidGene does not switch between light and dark on its own: it keeps the theme you chose.

#### Your own themes (desktop)

In the desktop application, you can add your own themes as JSON files. The Appearance section shows the folder to put them in (`~/.config/oxidgene/themes/` on Linux). Drop a file there and open the section again: the theme appears with a **Custom** tag. A file that cannot be read is listed under the picker with the reason.

A theme only needs to list what differs from a built-in theme:

```json
{
  "id": "sepia",
  "name": "Sepia",
  "base": "light",
  "colors": { "orange": "#8a5a2b", "bg-deep": "#f6f0e4" },
  "style": { "heading_font": "serif", "corners": "round" }
}
```

- `id`: lowercase letters, digits and `-`, different from the built-in themes.
- `name`: the name shown under the tile.
- `base`: the built-in theme to start from.
- `colors`: the colours to change, as hexadecimal values only.
- `style`: optional presets for the typefaces (`body_font`, `heading_font`), the `corners`, the `elevation` and the `density`.

The full list of colour names and presets is in the [common UI specification](@/tech/ui-common.md).

### Language

Click a language to switch the interface at once: English, Français, Deutsch, Español, Italiano, Nederlands, Polski or Português. Your genealogy itself (names, notes, sources) is never translated.

#### Your own languages (desktop)

In the desktop application, you can add a language as a JSON file named after its code, such as `ca.json`. The Language section shows the folder to put it in (`~/.config/oxidgene/languages/` on Linux). Drop the file there and open the section again to see it in the list. Start from one of the application's language files in its source repository (`assets/i18n/`): a text your file leaves out is shown in English, and a file that cannot be read is listed with an error.

### Pedigree

**Pedigree type** chooses how the tree view draws the pedigree on this device:

- **Chart**: **Tree**, **Ancestor wheel**, **Fan chart**, **Descendant wheel**, **Descendant fan**, **Lineage**, **Descendant lineage**, **Hourglass** or **Bowtie**. Each tile describes the chart in one line.
- **Pedigree theme**: **Classic** (plain cards and curved connectors) or **Medieval** (heraldic cards and ruled lines).

**Pedigree depth** sets how many generations a tree shows when you first open it: **Ancestor generations** (4 by default) and **Descendant generations** (3 by default), each from 0 to 10. A tree whose view you have already adjusted keeps its own depths.

See [The pedigree](@/guide/pedigree.md) for what each chart shows.

### Names

**Surname sorting** decides where a surname with a particle is filed in lists and in the dictionary:

- **Particle included** (the default): “de la Cruz” files under D.
- **Particle ignored**: “de la Cruz” files under C.

The name is always displayed in full; only its place in alphabetical order changes.

### Date Display

How the pages write dates, in every tree you open on this device. Your dates themselves are never changed, only the way they are shown. A **Preview** below the controls shows a sample of dates as the pages will write them.

- **Date format**: `12 Mar 1842` (the default), `12/03/1842`, `Mar 1842` or `1842`. A date's precision, such as “before” or “about”, is always written, whatever the format.
- **Event symbols**: with **Yes**, lifespans are written `* 1842 + 1907` instead of `1842-1907`.
- **“Circa” for approximate dates**: with **Yes**, an approximate date reads *c. 1842* rather than *about 1842*.
- **Display calendar**: **Gregorian** (the default), **Julian**, **Republican** or **Hebrew**. A date recorded in another calendar is followed by its equivalent in this one, in parentheses. Years shown alone, as on pedigree cards, stay Gregorian.

### Entry Options

How the forms help you when you type, in every tree you open on this device.

- **Entry suggestions** (Yes by default): place, surname, given name, occupation and source fields suggest what the tree already holds and the built-in dictionaries as you type. You can always type something else.
- **Automatic uppercase for surnames** (Yes by default): surnames are written in capitals as you type them. With **No**, a surname is kept as typed.
- **Suggest existing persons** (Yes by default): when you add a parent, partner, child or sibling from the tree view, the persons already in the tree are offered before a new one is created. With **No**, only a new person is offered.
- **Input date format**: the order of the day, month and year fields of every date field: `DD/MM/YYYY` (the default), `DD-MM-YYYY`, `YYYY-MM-DD`, or `DD Mar YYYY` with the month picked by name.
- **Default calendar for input**: the calendar an empty date field starts in. Each date field can still be switched to another one.

Place suggestions come from a place dictionary built into the application, covering twelve countries. It needs no download and no network access.

### API

This section is for connecting other programs to OxidGene; you do not need it for everyday genealogy.

- **OpenAPI specification** opens the description of the REST API.
- **Connect a client** gives the address of the API and an example command to copy.
- In the desktop application, it also shows an **Access token**. Anyone holding this token can read and change every tree while the application is running, so give it only to tools you trust. The address and the token change each time the application starts.
- In the web version, the GraphQL address and an example are shown as well.

The **AI assistant (MCP)** card, in the desktop application, gives the command that lets an AI assistant read your trees, and says how to let it change them too. See [AI assistant](@/guide/assistant.md).

Learn more: [tree settings specification](@/tech/ui-settings.md), [application settings specification](@/tech/ui-app-settings.md).
