+++
title = "Editing persons"
description = "Create and edit persons and couples: civil status, events, dates, places, notes, sources, witnesses, and the checks that keep the tree free of duplicates."
weight = 60
+++

One form creates or edits a person, whatever the context: a new spouse, child, sibling or parent, a person on their own, or an existing record. A second form edits a couple. This page explains how to open them, how to fill in each part, and how OxidGene helps you avoid entering someone twice.

{{ <screenshot name="person-edit" alt="Person form of a member of the Landrevel family, with the civil status and birth sections" lang={lang} /> }}

## Open the form

| To… | Do this |
|---|---|
| Edit a person | In the tree view, click the **⋮** under their selected card (or right-click the card) and choose **Edit individual**; on their page, click **Edit** |
| Add a spouse, a child or a sibling | In the tree view's action picker, choose **Add spouse**, **Add child** or **Add sibling** |
| Add a parent | In the tree view, click the **+** of the dashed empty slot where the father or mother should be |
| Add a person with no family link | Click **Add person** in the left sidebar |

**Add spouse**, **Add child** and **Add sibling** first open a small panel: "Search for an existing person to link, or create a new one:". Type a name to find someone already in the tree and link them, or click the button below (for example **Create New Person as Child**) to open the form for a new person. When the tree's **Suggest existing persons** option is off, only the new person is offered.

The form fills in what it can guess, and you can change all of it:

- a new father gets the child's surname and the male sex; a new mother gets the female sex;
- a new child or sibling gets the selected person's surname;
- a new spouse gets the opposite sex of the selected person;
- when the selected person has several unions, adding a child first asks which union the child belongs to.

After **Create**, the new person is linked to the family and becomes the focus of the tree.

## Link a parent who is already in the tree

When you add a parent from an empty slot and the tree's **Suggest existing persons** option is on, a panel above the form lists up to five persons whose name matches what you type ("Existing persons matching this name:"). For a father, the panel appears at once, since his surname is already filled in.

- Click **Link this person** to make that person the parent. No new person is created and the form closes.
- Or ignore the panel and go on filling in the form: "Or continue creating a new person below."
- Close the panel with its **✕** if it is in the way.

The child, the parents already recorded and persons of the other sex are left out of the list. The option lives in the tree [settings](@/guide/settings.md), under **Entry Options**.

## Civil status

The **Civil Status** section holds the person's identity:

- **Birth name** and **Given Names**, marked with an asterisk, must be filled in before saving. The surname is written in capitals as you type, unless **Automatic uppercase for surnames** is off in the [application settings](@/guide/settings.md#entry-options). The given names field completes one name at a time: in "Jean Ma", it suggests names for "Ma".
- **Sex**: **Male**, **Female** or **Unknown**.
- **Source**: the source of the identity as a whole (see [Sources](#sources) below).

The section also offers the lists below. On a person you are creating, their add buttons — like **Add Event** and **Add Media** further down — first create the person from what the form holds, then the form goes on as their edit form with that part open: the person exists from then on, even if you then cancel.

- **Profession(s)**: click **Add Profession**, type the trade as the record names it, add a date if you have one, then **Create Profession**. Each profession is stored as an occupation event, so it appears in the timeline and travels with GEDCOM exports.
- **More information**: click **Add Information** to record another name: a married name, a maiden name, an alias, a byname or sobriquet, a religious name, a prefix or a suffix, among others.
- **Notes**: click **Add Note**, write the note, then **Create Note**. Each note can be edited or deleted on its own.

## Birth and death

The **Birth** and **Death** sections each have a **Date**, a **Place**, **Notes** and a **Source**, all saved with the form. When the birth date is missing, OxidGene shows the baptism date instead on cards and pages; the same goes for the burial when the death date is missing.

### Witnesses and godparents

Under **Witnesses**, link the persons who attended the birth or the death: witnesses, godfather, godmother.

1. Click **Add a witness**.
2. Type their role in the relation field, for example "Godmother".
3. Search for the person in the tree and pick them.

The witness must already be a person of the tree, and the event must already be saved: on a new person, the section reads "Save this event first to add witnesses." Click × beside a witness to remove them. Every other event links its witnesses the same way, in its panel (see below).

## Other events

**Other Events** lists their baptism, burial, census, residence, military service, will and every other event. To add one:

1. Click **Add Event**.
2. Choose its **Type**. Types are grouped by kind: sacraments, civil life, legal acts, attributes and others.
3. Fill in the **Description** (the event's own value, such as an occupation or an address), the **Date** and the **Place**.
4. Add a **Cause** or the **Age** the record gives when relevant. Ages are typed as GEDCOM writes them: `34`, `1y 6m` (y years, m months, w weeks, d days), `< 1y`, `> 80y`, `CHILD`, `INFANT` or `STILLBORN`.
5. Add **Notes** and a **Source**. **More details** reveals the **Responsible agency**.
6. Click **Create Event**.

A saved event shows its type, description, date and place on one line. Click **Edit** on the line to open its panel: the same fields with their own **Save** button, the **Source reliability** of its citation (from **Unreliable** to **Certain**, or **Not assessed**) under **More details** once a source is entered, **Witnesses**, linked as for the birth (an occupation has none), and **Evidence**, the documents that prove the event. Click **Delete** to remove the event.

## Dates

Every date field works the same way, in persons, couples and documents.

1. Choose the calendar: **Gregorian**, **Julian**, **Hebrew** or **Republican**.
2. Choose the precision: **Exact**, **About**, **Calculated**, **Estimated**, **Perhaps**, **Before**, **After**, **Or**, **Between** or **From an age**.
3. Fill in the day, month and year. Partial dates are fine: leave the day, or the day and the month, empty when the record gives only a month or a year.

**Or** and **Between** show a second date ("1841 or 1842", "between 1840 and 1845"). **From an age** asks for the age and the year it was observed in, for instance an age given by a census.

Hebrew and Republican months are picked from a list of their own names, which starts with a blank entry for a date known only by its year (« an VI »). Switching the calendar of a date already entered converts it rather than relabelling it: 11 March 1796 becomes 21 ventôse an IV. A date the target calendar cannot express stays as typed.

A date that cannot exist is flagged right under the field, for example "That date never existed." or "The second date must not come before the first."

The order of the day, month and year fields, their separators, and the calendar an empty field starts in are set per tree under **Entry Options** in the tree [settings](@/guide/settings.md). How dates are displayed is also a tree setting.

## Places

Place fields suggest as you type, and always accept free text (placeholder: "Town, village, parish…").

- The tree's own places come first, most used first.
- From the third character, the built-in place dictionary follows. It covers France, the United Kingdom, Germany, Italy, Spain, Switzerland, Poland, the United States, Portugal, Belgium, Luxembourg and the Netherlands, and works without a network connection.
- Each suggestion shows the place's name, then the rest of its label: code, département or county, region, country. A commune that has since been merged or renamed is offered under its former name too, marked "until" the year it ended.

When you save, a place you picked is used as it is. Text you typed becomes the tree's place of the same name, ignoring case; if the tree has none, it is created. A place created from the dictionary takes its coordinates, so it appears on the statistics map.

## Suggestions in other fields

Surname, given-name, occupation and source fields also suggest values as you type: the tree's values first, each with the number of persons using it, then terms from the reference sheets in any language. A row marked **info** has a sheet explaining it, which you can read by hovering the term on the person page. Picking a suggestion enters it as the tree or the sheet writes it.

Suggestions never restrict what you type. Turn them off with **Entry suggestions** under **Entry Options** in the [application settings](@/guide/settings.md#entry-options).

## Sources

Each **Source** field is plain text: type the source the way you read it off the record, for example "AD29, état civil de Quimper, naissances 1843, vue 12, acte 26". The field suggests the tree's existing source titles.

When you save, a title that matches an existing source of the tree, ignoring case, reuses it; any other title creates a new source. Correcting a typo moves the citation to the corrected title, and the old source is removed if nothing else uses it.

A source typed in the **Birth**, **Death** or an event's panel is attached to that event. When it names an archive register OxidGene knows, the person page turns it into a button that opens the register: see [Person and couple pages](@/guide/persons.md#open-the-cited-register) and [Sources and archives](@/guide/sources-archives.md).

## Privacy

**Privacy** records whether the person should be **Default**, **Public** or **Private**. It is recorded now but hides nothing yet, as the form states: it will take effect when accounts and publishing arrive.

## Save, or leave without saving

- Click **Create** (new person) or **Save** (existing person).
- **Cancel**, the ×, Escape, or a click outside the form close it without saving. If you changed something, OxidGene first asks "Discard changes?".

Photos and documents are not part of the form's save: they are written as soon as you add them. See [Photos and documents](@/guide/media.md).

### The homonym check

After every save, OxidGene looks for other persons of the tree with exactly the same surname and given names, ignoring case and accents. If it finds any, a **Same name found** dialog asks whether the person you just saved is one of them.

- Leave **A different person** selected and click **Keep separate**: both records stay, and the question will not be asked again for them.
- Choose one of the persons listed, each shown with their dates, family and birth place, and click **Merge**: the record you just saved is merged into that person and removed. This cannot be undone.
- Click **Decide later** (or press Escape) to answer another time; the question comes back on the next save.

## Delete a person

1. Open the person's form.
2. At the bottom, click **Delete this person**.
3. Read the warning and click **Confirm deletion**.

The person and their events are removed, along with their links to parents, children and spouses. You can still restore a deleted person from their [history](@/guide/history.md).

## Edit a couple

Open the couple form with **Edit union** in the tree view's action picker (choose the union first if the person has several), or **Edit couple** on the couple page.

- **Each spouse** has a collapsed block with the same fields as the person form. Open it to change the spouse without leaving the couple.
- **Union Events**: click **Add union event** and choose the type: marriage, divorce, annulment, engagement, banns, contract, license, settlement, or another event such as a residence. Each event has a date, a place, notes and a source. A saved union event's panel also offers **Age of** each spouse, as the marriage record gives it.
- **Children**: click **Add Child** to link an existing person or create one. **Detach** removes a child from this union without deleting them; it is applied only when you save, and **Undo** cancels it.
- **Privacy** and **Media** work as in the person form.

Click **Save** to apply everything at once. **Delete couple** removes the union and its events after a confirmation; both persons stay in the tree, and their children are no longer linked to that union.

Learn more: [person form specification](@/tech/ui-person-edit-modal.md).
