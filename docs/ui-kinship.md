---
type: "UI Specification"
title: "Visual & Functional Specifications — Kinship"
description: "Every way two persons of a tree are related, each path drawn generation by generation from the ancestors they share, or through unions when they share none."
tags: [oxidgene, specification, ui, ux, kinship]
generated: { by: claude-code/claude-opus-5-5, at: 2026-09-27T00:00:00Z }
---

# Visual & Functional Specifications — Kinship

> Part of the [OxidGene Specifications](index.md).
> See also: [Genealogy Tree](ui-genealogy-tree.md) · [Person Profile](ui-person-profile.md) · [Search Results](ui-search-results.md) · [Common UI](ui-common.md) · [API Contract](api.md)

---

## 1. Purpose and entry

The kinship page answers "how are these two persons related?" for any two
persons of a tree, not only those the pedigree window happens to show. It is a
routed page, `/trees/{tree_id}/kinship?from={person_id}&to={person_id}`, so a
result can be bookmarked, shared, and left with the browser's back button.

It is opened from the pedigree: right-clicking a card, or its pencil action
picker, offers **Relationship with…**. The second person is chosen right there,
in the pedigree's person panel, before the page opens:

- shortcuts for **Myself** — the person the tree identifies as the user — and
  the tree's **SOSA root**, each shown only when set and different from the
  clicked person;
- the shared person search for anyone else.

Picking one opens the page with the clicked person as `from` and the chosen
one as `to`. Cancelling closes the panel and stays on the pedigree. Opened
without a `from` or a `to`, as from a hand-edited URL, the page asks for the
missing person itself.

## 2. Layout

The page uses the shared sub-page chrome: the topbar breadcrumb (logo, tree
name, **Relationship**) and the tree icon sidebar with no active view.

```
+-------------------------------------------------------------+
|  FROM            [Change]      TO                 [Change] |
|  [person row]           [⇄]    [person row]                 |
+-------------------------------------------------------------+
  2 relationships found
+-------------------------------------------------------------+
| ▌1  First cousin                        2 up, 2 down        |
|     A and B                                                 |
|  2  Second cousin – 1 generation apart  4 up, 3 down        |
+-------------------------------------------------------------+
+-------------------------------------------------------------+
|  1  First cousin                                            |
|  +2          [ancestor]   [ancestor]                        |
|  +1   [person row]            [person row]                  |
|   0   [person row] (from)     [person row] (to)             |
+-------------------------------------------------------------+
```

### 2.1 The two ends

Both persons are drawn with the shared person row of
[Search Results §7](ui-search-results.md), outlined in the accent colour, and
link to their profiles.

- **Change**, on either end, replaces that person with the shared person
  search; picking a result updates `from` or `to`. Cancelling keeps the
  current person.
- **⇄** swaps the two persons. Relations are worded from the first person's
  point of view, so a swap turns an uncle into a nephew.

The URL is replaced rather than pushed on either change, so the back button
returns to the pedigree.

On screens narrower than 640px the two ends stack, and the grid keeps its two
lines side by side with only each person's name and years: the portrait and
the relatives line are dropped from the rows so each still fits half a phone.
Summary rows put the generation count under the title.

### 2.2 Paths

A line states how many relationships were found, and adds that more exist
when the answer was truncated (§3).

When there are several, a summary lists them, closest first, one row each:
its number, its title, how many generations it climbs from the first person
and then descends to the second (`2 up, 3 down`, plus the number of unions for
a relationship by marriage), and the names of the common ancestors. Only the
chosen relationship is drawn below the summary; the closest is chosen when the
page opens and whenever either person changes, and choosing another row
replaces it. A single relationship is drawn without a summary.

A relationship is a card, numbered as in the summary and titled with what the
second person is to the first.

The body is a grid, one row per generation:

- a narrow left column gives each row's generation relative to the first
  person: `+2` for their grandparents' generation, `0` for their own, `−1` for
  their children's;
- the top row holds the common ancestor, or both spouses of the common
  ancestral couple, centred across the two lines;
- below it, the first person's line on the left and the second person's on the
  right, each person linked to their profile, with a short connector from the
  generation above. When one person is the other's direct ancestor, the grid
  has a single line.

A relationship through only one of the two parents at the top — half-siblings,
half-cousins — is noted beside the title.

### 2.3 Relationships by marriage

When the two persons share no ancestor, the paths run through unions instead.
Such a card is titled **Related by marriage** and gives the steps as a chain,
each a relative of the one before starting from the first person, for example
`Uncle › Wife › Brother`. The body draws each blood stretch as its own grid,
separated by a **⚭ Union** line where one person married the next.
Generations stay counted from the first person across the whole path: spouses
stand on the same generation.

### 2.4 States

| State | Display |
|---|---|
| A person missing | A prompt to choose one, with the search open |
| The same person at both ends | A prompt to choose two different persons |
| Loading | The shared loading message |
| No path | "No recorded link connects these two persons." |
| Error | The shared error message with the failure |

## 3. Relationship rules

Paths come from `GET /trees/{tree_id}/persons/{person_id}/kinship/{other_person_id}`
([API Contract](api.md#persons)), which searches the whole tree:

- A **blood relationship** is a pair of lines descending from a common ancestor
  to the two persons that share nobody but that ancestor. When both lines
  descend from the same union, both spouses are the top. A farther common
  ancestor reached through a nearer one is not another relationship; pedigree
  implex, where the same ancestors are reached through different lines, is.
- Only when there is no blood relationship are the **shortest paths through
  unions** listed, fewest unions first.
- At most 32 paths are listed; heavy implex can produce more, in which case
  the closest are kept and the answer says it was truncated.
- Deleted persons and families link nobody.

## 4. Relation labels

A label says what the last person of a stretch is to its first, from the
number of generations it climbs (`up`) and descends (`down`), worded for the
last person's sex, with a neutral form for an unknown sex.

| `up`, `down` | Label |
|---|---|
| 0, 1 / 2 / 3 | child / grandchild / great-grandchild |
| 0, n ≥ 4 | descendant, n generations down |
| 1 / 2 / 3, 0 | parent / grandparent / great-grandparent |
| n ≥ 4, 0 | ancestor, n generations up |
| 1, 1 | sibling, or half-sibling |
| 2 / 3, 1 | uncle or aunt / great-uncle or great-aunt |
| n ≥ 4, 1 | sibling of an ancestor n − 1 generations up |
| 1, 2 / 3 | nephew or niece / grandnephew or grandniece |
| 1, n ≥ 4 | descendant of a sibling, n − 1 generations down |
| otherwise | cousin of degree `min(up, down) − 1` — first and second cousins by name — followed, when `up ≠ down`, by how many generations apart they are |

Every label, including the spouse at each union of a relationship by marriage,
comes from the translation tables; the page composes no wording of its own.
