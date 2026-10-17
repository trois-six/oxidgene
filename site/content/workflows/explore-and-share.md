+++
title = "Explore and share your family history"
description = "Look at your tree through charts, find how two persons are related, map where your family lived, export the tree, and let an assistant read it."
weight = 50
+++

Once your tree has taken shape, you will want to look at it from new angles and share it with relatives. This workflow shows how to draw it as a fan or an hourglass, find how two persons are related, see on a map where your family lived, print a chart, export the tree for another program, and let an AI assistant read it.

## 1. Draw the tree as a fan, an hourglass or a wheel

The tree view can draw your pedigree in several ways. The choice is a display preference of your device, not of the tree.

1. Open the application settings with the gear icon in the home page header (the same options are under **Global Preferences** in a tree's settings), then choose **Pedigree**.
2. Under **Chart**, pick a view:
   - **Tree**: ancestors above, descendants below;
   - **Ancestor wheel** and **Fan chart**: ancestors in rings around the person, a full circle or a half circle, the father's side always on the left;
   - **Descendant wheel** and **Descendant fan**: descendants in rings, their unions between the generations;
   - **Lineage** and **Descendant lineage**: one column per generation, as in Gramps;
   - **Hourglass**: descendants on the left, the person in the middle, ancestors on the right;
   - **Bowtie**: the father's line on the left, the mother's on the right.
3. Go back to the tree: the new view applies immediately.

In every view, click a person to make them the centre of the chart, drag to move, and zoom with the wheel or the buttons. In the wheels and fans, deep generations are written smaller; zoom in to read them.

{{ <screenshot name="fan-chart-dark" alt="Fan chart of the Landrevel family in the dark theme" lang={lang} /> }}

## 2. Choose a look

Two settings change the look of the charts, both in the application settings:

- **Pedigree theme**, under **Pedigree**: **Classic** (plain cards and curved connectors) or **Medieval** (an engraved chart with heraldic cards and ruled lines). It applies to every pedigree on the device.
- **Theme**, under **Appearance**: the colours and typefaces of the whole application, such as **Light** or **Dark**. The charts follow it.

{{ <screenshot name="hourglass-medieval" alt="Hourglass chart drawn with the medieval pedigree theme" lang={lang} /> }}

## 3. Print a chart

1. Zoom and pan the chart to frame what you want on paper.
2. Click the printer icon (**Print**) in the left icon sidebar, or press Ctrl+P (Cmd+P on a Mac).
3. If the chart is larger than one sheet at this zoom, choose **What the screen shows** (one sheet) or **The whole chart at this zoom**, which says how many sheets it takes. Sheets overlap by 1 cm with a dashed line to cut or align on, so you can assemble a large wall chart.

Paper always uses the light colours, whatever the theme on screen. Through the print dialog you can also save the chart as a PDF.

## 4. Find how two persons are related

1. In the tree, right-click a person's card (or click its pencil) and choose **Relationship with…**.
2. Pick the other person: **Myself**, the tree's **SOSA root**, or anyone found with the search.
3. The relationship page opens. It names what the second person is to the first ("First cousin", "Second cousin – 1 generation apart") and draws the path generation by generation, from the common ancestors down to both persons.

When there are several relationships (cousins through two lines, for instance), a summary lists them, closest first, with how many generations each climbs and descends; click one to draw it. Use **Change** to replace either person, or the swap button to see the relationship from the other side: an uncle becomes a nephew. When the two share no ancestor, the page shows how they are **Related by marriage**, step by step through the unions.

See [Kinship](@/guide/kinship.md).

{{ <screenshot name="kinship" alt="Relationship page showing the path between two cousins" lang={lang} /> }}

## 5. See where your family lived

1. Open the statistics with the chart icon in the left icon sidebar.
2. Open the **Places** tab.

The heat map shows the regions where the events of your tree happened, with the ten most used places beside it and the births by country, region and subdivision below. Click a place of the list to zoom in on it. The other tabs tell the rest of the story: names, length of life, age at marriage, family sizes century by century. See [Statistics](@/guide/statistics.md).

{{ <screenshot name="statistics-map" alt="Heat map of the places of the Landrevel family" lang={lang} /> }}

## 6. Export the tree for another program or a relative

1. Open the tree settings with the gear icon in the left icon sidebar and choose **Export Tree**.
2. Choose the format:
   - **GEDCOM (.ged)**, the exchange format every genealogy program reads. Under **Destination program**, choose the program that will import it (Geneanet, Ancestry, Heredis, Gramps and others), or **Standard GEDCOM 5.5.1** (the default) or **Standard GEDCOM 7.0**. The file is then written the way that program reads it.
   - **GEDZIP (.gdz)**, a single archive holding a GEDCOM 7.0 file and your photos and documents. It is ideal for a full backup or to share with attachments, with software that supports GEDCOM 7.0.
3. Set the options: **Include notes and sources**, and for a GEDZIP **Include media**. They apply to this export only.
4. Optionally, fill in the **Submitter** card (name, email, address): it says who the file comes from.
5. Click **Download .ged** or **Download .gdz**.

After a GEDCOM export for a specific program, the page reports what that program could not hold, or confirms that nothing was lost. A GEDZIP archive stays available for one hour: **Download again** saves it again without rebuilding it.

Keep in mind:

- An export always holds the whole tree. Exporting only part of it (a person's ancestors or descendants) is not available yet.
- Living persons are exported like everyone else: there is no option yet to leave out contemporary persons. Check what you share.

See [Import and export](@/guide/import-export.md).

## 7. Let an AI assistant read your tree

On the desktop application, an AI assistant such as Claude Desktop or Claude Code can read your trees through the Model Context Protocol (MCP), to answer questions about your family, draft a biography from a profile, or help find inconsistencies.

1. Open the application settings and choose **API**.
2. Under **AI assistant (MCP)**, read the warning, then copy the command or the client configuration into your assistant.

Access is read-only: the assistant cannot change your trees. But it can read every tree in the application, living persons, notes and sources included, and it sends what it reads to the provider of the model it uses. Only connect an assistant you trust with that data. See [AI assistant](@/guide/assistant.md) for the full setup.
