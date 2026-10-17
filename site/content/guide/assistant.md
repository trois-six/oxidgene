+++
title = "AI assistant"
description = "Let an AI assistant such as Claude read your trees through the desktop application, read-only and with your explicit consent."
weight = 160
+++

The OxidGene desktop application can let an AI assistant read your genealogy trees, so that you can ask it questions about a tree, have it draft a biography from a person's profile, or look for inconsistencies. This page explains what the assistant can see, how to connect it, and how to disconnect it.

The connection uses the Model Context Protocol (MCP), a standard way for assistants to use outside tools. Any assistant program that supports MCP can use it, for example Claude Desktop or Claude Code.

## What the assistant can and cannot do

- **It only reads.** The assistant can search persons, open profiles and pedigrees, list events, citations and notes, look up places and sources, and browse the dictionary of a tree. It cannot add, change or delete anything.
- **It sees every tree** of the application. Each question it asks OxidGene names the tree it is about, so answers never mix two trees.
- **It sees everything in them**, including living people, notes and sources. The privacy settings of your persons and trees are not applied to it.
- **What it reads leaves your computer.** The assistant sends what it reads to the company that runs its language model. OxidGene does not control what that company keeps.
- It does not receive your photos or documents, and it cannot import, export or reach Geneanet.

This is available in the desktop application only. The web version shows a note saying so instead.

## Before you start

- Install and open the OxidGene desktop application at least once, so that your database exists (see [Install OxidGene](@/guide/install.md)).
- Install the assistant program you want to use, and make sure it supports MCP servers.

## Connect an assistant

1. Click the gear icon at the top right of the home page to open the application settings.
2. Open the **API** section and find the **AI assistant (MCP)** card.
3. Read the warning at the top of the card: it is the consent you are about to give.
4. Copy what your assistant program asks for:
   - **Command**: the full command, `…/oxidgene-desktop mcp`, with the location of the program on your computer already filled in;
   - **Client configuration (JSON)**: the same command in the form many assistant programs expect in their configuration file.
5. Add it to your assistant program's MCP servers, following its documentation, then restart the assistant if it asks you to.

For example, with Claude Code you would run a command such as:

```bash
claude mcp add oxidgene -- /path/to/oxidgene-desktop mcp
```

and a program that uses a configuration file would receive:

```json
{
  "mcpServers": {
    "oxidgene": {
      "command": "/path/to/oxidgene-desktop",
      "args": ["mcp"]
    }
  }
}
```

Always copy the version shown in your own settings: it contains the real location of the program.

Copying the command changes nothing in OxidGene. The assistant gains access only once you add the command to it.

## Use it

Ask your assistant about your trees in plain language, for example:

- “Which trees do I have in OxidGene?”
- “In the Landrevel tree, summarise the life of Étienne Marie Landrevel.”
- “List the ancestors of SOSA 1 in the Landrevel tree who have no recorded death.”

The assistant first lists your trees, then asks OxidGene for what it needs from the tree you named. Dates come with their precision (“about”, “before”…), and when a birth or a death has no date, the baptism or the burial may stand in for it, as in the rest of OxidGene. SOSA numbers count from the tree's root person.

The OxidGene window can stay open while the assistant works; they share the same database.

Names, notes and sources often come from imported files written by other people, and may contain text that tries to give the assistant instructions. Because access is read-only, nothing written in them can make it change your trees through OxidGene.

## Disconnect it

Remove the OxidGene entry from your assistant program's MCP servers. From then on, it can no longer read your trees. There is nothing to undo in OxidGene.

Learn more: [assistant access specification](@/tech/mcp.md).
