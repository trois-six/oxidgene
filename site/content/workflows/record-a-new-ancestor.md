+++
title = "Record a new ancestor"
description = "From a register found in the archives to a new ancestor in the tree, with the event, its place, its source, the cited register one click away, and a photo of the act."
weight = 30
+++

You have just found, in the digitized registers of an archive, the birth act of an ancestor your tree does not have yet. This workflow takes you from that act to a new person in the tree: created in the right place, with the birth, its date and place, a source that opens the register again in one click, and a photo of the act.

The example follows the fictitious Landrevel family. Étienne Marie Landrevel, born in Quimper, is already in the tree, but his father is not. In the civil registers of Quimper for 1866, act 26 on view 12, you have found the birth of Jean-Marie Landrevel, his father.

## 1. Find the empty slot

1. Open the tree and centre it on Étienne Marie: type "Landrevel" in **Last name / # SOSA** and "Étienne" in **First name**, then pick him in the list.
2. Above his card, a dashed card with a **+** stands for each missing parent. If you do not see it, hover **Generation depth** in the left sidebar and raise the number of ancestor generations.

Before creating anyone, make sure the father is not already in the tree under another branch: the next step helps with that too.

## 2. Create the father from the empty slot

1. Click the **+** of the empty slot on the father's side. The person form opens with the surname LANDREVEL and the sex **Male** already filled in.
2. If the tree's **Suggest existing persons** option is on, a panel above the form lists the persons of the tree matching that name: "Existing persons matching this name:". If one of them is the father you are looking for, click **Link this person**: he becomes Étienne's father and no new person is created. You can then skip to step 4.
3. Otherwise, type "Jean-Marie" in **Given Names**. The panel narrows as you type; ignore it, or close it with its **✕**.

## 3. Enter the birth

In the **Birth** section:

1. **Date**: leave the calendar on **Gregorian** and the precision on **Exact**, then type the day, month and year of the act: 3, 3, 1866. If the act only gives an approximate date, choose the matching precision instead, for example **About** or **Between**.
2. **Place**: start typing "Quimp". The tree's places are suggested first; from the third character, the built-in place dictionary adds the communes it knows, with their département, region and country. Pick Quimper. A place taken from the dictionary brings its coordinates, so it will appear on the statistics map.
3. **Source**: type the source as you read it off the archive's portal, from the archive down to the view, for example:

   `AD29, état civil de Quimper, naissances 1866, acte 26, vue 12`

   Give the archive (here the departmental archive of Finistère, AD29), the locality, the kind of register and its year, then the act number and the view number. That is what OxidGene needs to open the register at the right view later. The field suggests the sources already in the tree; if this one exists, pick it.

4. Click **Create**.

If the register is a parish register rather than a civil one, the act is a baptism: leave the **Birth** section empty, create the person, then add the baptism as described in step 5.

## 4. Answer the homonym check

When the tree already holds another person called Jean-Marie LANDREVEL, a **Same name found** dialog opens as soon as you save. Look at the persons listed, with their dates, family and birth place:

- If one of them is the same man, choose him and click **Merge**: the record you just created is merged into the existing one. This cannot be undone.
- If they are different men, leave **A different person** selected and click **Keep separate**. The question will not be asked again for them.
- If you need to check first, click **Decide later**: the dialog will come back on the next save.

The form then closes. Jean-Marie now stands above Étienne in the tree and becomes the focus.

## 5. Add a baptism, witnesses or other details

Click the **⋮** under Jean-Marie's selected card and choose **Edit individual**. Now that he exists, the form offers more:

- **Baptism.** Under **Other Events**, click **Add Event**, choose **Baptism** as the **Type**, fill in the **Date** and **Place**, type the source as in step 3 (for example `Paroisse de Quimper, registre des baptêmes 1866, folio 12`), then click **Create Event**.
- **Witnesses.** If the act names witnesses who are already in the tree, click **Add a witness** in the **Birth** section, type their role in the relation field, then search for them and pick them.
- **Profession.** When a later record gives Jean-Marie's occupation, add it under **Profession(s)**.

Click **Save** when you are done.

## 6. Open the cited register from the person's page

Check that the source leads back to the act.

1. With Jean-Marie selected, click **Profile view** in the left sidebar.
2. In **Events**, find the birth. Under it, **Sources** shows the source you typed. Since it names a register of a catalogued archive, it is a button: hover it to read "Open the cited view at …" followed by the archive's name.
3. Click it. On the desktop, an archive window opens the register on the archive's portal at view 12. On the web, it opens in a new browser tab.

If OxidGene recognises the archive but not enough to find the register, the **Find in the archives** dialog opens instead. Fill in what is missing (the locality, the kind of register, the year, the view), tick **Add these details to the citation** to keep them, and click **Search**. The [Sources and archives](@/guide/sources-archives.md) page explains what to do when a portal does not answer.

## 7. Attach a photo of the act

Keep a copy of the act with the event it proves.

1. Save the view from the archive's portal, or take a photo of the register, as an image file.
2. On Jean-Marie's page, click the **+** beside **Media**.
3. Fill in a **Title**, such as "Birth act of Jean-Marie Landrevel", choose **Civil registration** as the **Kind of record** (or **Parish register** for a baptism), and give the **Date** and **Place**.
4. Under **Documents these events**, tick the birth.
5. Under **Pages**, click **Select files** and choose your image, or drop it on the cell.
6. Click **Save**.

The image now appears in Jean-Marie's **Media** and as a small thumbnail under the birth in his timeline. When a view shows several acts, open the person form, find the image in its **Media** section and use **Crop a region** to keep only Jean-Marie's act; you can tie the region to the birth.

## 8. Check the result

Return to the tree with **Tree view**. Jean-Marie's card shows his birth year, with a precision mark if the date was approximate, and the SOSA badge if Étienne is in the line of the tree's SOSA root. Click his card: the events panel lists his birth with its date and place.

Next, click the **+** of his own empty parent slots to go one generation further.

Learn more: [Editing persons](@/guide/editing.md), [Person and couple pages](@/guide/persons.md), [Photos and documents](@/guide/media.md).
