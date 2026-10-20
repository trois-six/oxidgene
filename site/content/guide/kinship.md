+++
title = "Kinship"
description = "Find every way two persons of a tree are related, drawn generation by generation."
weight = 100
+++

The relationship page answers the question "how are these two persons related?" for any two persons of a tree. It lists every relationship it finds, closest first, and draws each one generation by generation from the ancestors the two persons share, or through marriages when they share none.

## Find how two persons are related

1. In the pedigree, right-click the card of the first person, or click the **⋮** under the selected card, and choose **Relationship with…**.
2. Choose the second person in the panel that opens:
   - **Myself**, the person set as you in the tree settings (**Who am I?**);
   - **SOSA root**, the tree's SOSA root;
   - or anyone else, through the search field.

   The two shortcuts appear only when they are set and are not the clicked person.
3. The relationship page opens with the first person under **From** and the second under **To**.

Click **Cancel** in the panel to stay on the pedigree.

The page has its own address, so you can bookmark a result or share it. The browser's back button, or OxidGene's back button, returns to the pedigree.

{{ <screenshot name="kinship" alt="Relationship page showing two members of the Landrevel family as first cousins, with their shared grandparents at the top" lang={lang} /> }}

## Change or swap the two persons

- Click **Change** beside either person to pick someone else with the person search. Cancelling the search keeps the current person.
- Click the **⇄** button between them to swap the two persons.

Relationships are always named from the first person's point of view: the label says what the second person is to the first. Swapping the two persons turns an uncle into a nephew, a grandfather into a grandson.

## Read the result

A line first says how many relationships were found, for example "2 relationships found".

When there are several, a summary lists them, closest first. Each row gives:

- its number and its name, such as "First cousin" or "Second cousin – 1 generation apart";
- how many generations the path climbs from the first person to the common ancestors, then descends to the second, for example "2 up, 2 down";
- the names of the common ancestors.

Click a row to draw that relationship below the summary. The closest one is drawn when the page opens and each time you change a person. When there is only one relationship, it is drawn directly, without a summary.

The drawing is a grid, one row per generation:

- The narrow column on the left gives each row's generation compared with the first person: `0` for their own generation, `+1` for their parents', `+2` for their grandparents', `−1` for their children's.
- The top row holds the common ancestor, or both members of the common ancestral couple.
- Below, the first person's line runs down on the left and the second person's on the right, each person linked to their profile.
- When one person is the other's direct ancestor, there is a single line.

A relationship through only one of the two parents at the top — half-siblings, half-cousins — is marked **Through one parent only** beside its name.

Several relationships between the same two persons are common in old trees: when the same ancestors appear through different lines (pedigree collapse), each line is a separate relationship. A more distant common ancestor reached through a closer one does not count as another relationship.

## Relationships by marriage

When the two persons have no common ancestor, OxidGene looks for the shortest paths through marriages instead, those with the fewest unions first.

Such a relationship is titled **Related by marriage** and shows the steps as a chain, each step being a relative of the one before, starting from the first person — for example "Uncle › Wife › Brother". The summary row also gives the number of unions.

The drawing shows each blood stretch as its own grid, separated by a **⚭ Union** line where one person married the next. Generations stay counted from the first person along the whole path, and spouses stand on the same generation.

## How relationships are named

The name depends on how many generations the path climbs and then descends, and on the sex of the second person (with a neutral form when the sex is unknown):

| Path | Name |
|---|---|
| Straight down | Child, grandchild, great-grandchild, then "Descendant, n generations down" |
| Straight up | Parent, grandparent, great-grandparent, then "Ancestor, n generations up" |
| One up, one down | Brother or sister, or half-brother or half-sister |
| Up two or three, one down | Uncle or aunt, great-uncle or great-aunt; further up, "Sibling of an ancestor n generations up" |
| One up, down two or three | Nephew or niece, grandnephew or grandniece; further down, "Descendant of a sibling, n generations down" |
| Any other path | First cousin, second cousin, then "Cousin of degree n", followed by the number of generations apart when the two persons are not of the same generation |

## When no relationship appears

- "No recorded link connects these two persons." means that neither a common ancestor nor a chain of marriages links them in your tree. Persons and families you have deleted link nobody.
- "Choose two different persons." appears when the same person is at both ends.
- At most 32 relationships are listed. In a tree with heavy pedigree collapse, the closest ones are kept and the page says **Only the closest relationships are listed; more exist.**

Learn more: [kinship specification](@/tech/ui-kinship.md).
