---
type: "UI Specification"
title: "Visual & Functional Specifications — Couple Profile"
description: "Side-by-side view of both spouses of a couple, with the union, its events, media, and notes shared across the two."
tags: [oxidgene, specification, ui, ux]
generated: { by: claude-code/claude-opus-5-5, at: 2026-09-26T00:00:00Z }
---


# Visual & Functional Specifications — Couple Profile

> Part of the [OxidGene Specifications](index.md).
> See also: [Person Profile](ui-person-profile.md) (the sections reused here) · [Tree View](ui-genealogy-tree.md) (couple button in the left sidebar) · [Person Edit Modal](ui-person-edit-modal.md#16-couple-edit-modal) (couple edit modal) · [API Contract](api.md) (Families, person detail bundle)

---

## 1. Overview

The couple profile shows one couple, a `Family`, on a single page. Each spouse
has a column with the same sections as their [Person Profile](ui-person-profile.md).
What the two spouses share is drawn once, across both columns: the union, its
events, its media, and its notes.

The page lives at `/trees/{tree_id}/couples/{family_id}`.

---

## 2. Access

The **couple** button in the shared left icon sidebar, drawn as two person
silhouettes, sits between the profile and tree buttons. It opens the selected
person's earliest couple. Couples are ordered by their earliest dated union
event, usually the marriage, and undated couples follow in their recorded
order.

A couple is a union with a known partner. A family that records the person
without a partner is not a couple. **When the selected person has no couple,
the button is not shown.**

The button appears on the tree view, the person profile, and the couple page
itself, where it is active. The search results, dictionary, and settings pages
do not show it, because they do not load the selected person's unions.

---

## 3. Layout

Uses the `sub-page` layout with the shared left icon sidebar. The content is
up to `1600px` wide.

```
+----------------------------------------------------------------------+
| [logo] <tree name> / <person A> & <person B>                          |  <- td-topbar
+----------------------------------------------------------------------+
| [<person A> — date ▾]  ⚭  [<person B> — date ▾]      [Edit couple]    |  <- couple bar
+-----------------------------------+----------------------------------+
| IDENTITY A                        | IDENTITY B                       |
+-----------------------------------+----------------------------------+
| COUPLE NOTES                                                          |
| NOTES A                           | NOTES B                          |
+-----------------------------------+----------------------------------+
| COUPLE MEDIA                                                          |
| MEDIA A                           | MEDIA B                          |
+-----------------------------------+----------------------------------+
| UNION: married on …, and had: <children>                              |
| FAMILY A                          | FAMILY B                         |
+-----------------------------------+----------------------------------+
| COUPLE EVENTS                                                         |
| EVENTS A                          | EVENTS B                         |
+-----------------------------------+----------------------------------+
| ANCESTORS A                       | ANCESTORS B                      |
+-----------------------------------+----------------------------------+
```

The sections follow the person profile's order. Each shared section comes
first in its group, across the full width. The two spouses' versions of the
same section follow on one row, so their cards line up and stretch to the
taller of the two.

A notes row is omitted when neither spouse has notes, and the couple notes
card is omitted when the couple has none. A notes card that failed to load
still shows its error.

### Placement

**The husband is always on the left and the wife on the right.** The spouse
role decides first. A `Partner` is placed by sex: male on the left, female on
the right, unknown between them. Spouses the rule cannot tell apart keep
their recorded order.

A couple recorded with a single spouse keeps that spouse on their side. The
other column's identity card reads **Unknown spouse**, and its other rows stay
empty.

---

## 4. Couple Bar

The bar at the top holds one selector per spouse, the ⚭ sign between them,
and the actions.

- **Left selector**: lists the couples of the spouse on the right. Each option
  is named after the partner it would put on the left, followed by the
  marriage date when known.
- **Right selector**: lists the couples of the spouse on the left, the same
  way.
- **Choosing an option opens that couple.** The URL is replaced rather than
  pushed. Choosing another husband for the wife shown on the right therefore
  opens that couple, and the right selector then lists the new husband's
  couples.
- A selector whose opposite spouse is unknown, or who has only this couple,
  shows the current spouse and is disabled.
- Each selector's accessible name is **Spouses of <name>**, naming the spouse
  whose couples it lists.

**Actions:**
- **Edit couple** opens the existing
  [couple edit modal](ui-person-edit-modal.md#16-couple-edit-modal). That modal
  also detaches children from the union and deletes the couple. Deleting the
  couple removes the union only, and the page then returns to the tree view,
  centered on the spouse that was displayed.
- **Refresh**, on the web build only, reloads the page's data.

Below 640px, each selector takes its own line and the ⚭ sign is hidden.

---

## 5. Spouse Columns

Each column reuses the person profile sections for its spouse, with the
couple's own union left out:

| Section | Content in a spouse column |
|---|---|
| Identity | Avatar, names, vitals, SOSA and **Me** badges, as on the [Person Profile](ui-person-profile.md#4-identity-header). Its single action, **Profile**, opens the spouse's person profile. The actions sit below the identity, as on a narrow screen. |
| Notes | The spouse's own notes. |
| Media | **Only media attached to the spouse directly**, plus the vignettes identifying them. Media attached to this couple or to the spouse's other couples are not shown here. |
| Family | Parents, siblings, half-siblings, and the spouse's **other** unions with their children. |
| Events | The spouse's individual and parental-family events, and the events of their other unions. |
| Ancestors | The spouse's mini-pedigree. |

**Every person named on the page links to that person's own profile.** This
includes parents, children, siblings, and partners in other unions.

---

## 6. Shared Sections

The union is drawn from either spouse's detail bundle. Both bundles contain
it.

- **Union**: the union sentence without its partner clause, for example
  "Married on <date> in <place A>, divorced on <date>, and had:". It is
  followed by the children of this union.
- **Couple events**: the union's own events (marriage, contract, divorce…) and
  its children's births, baptisms, deaths, and burials. A family event carries
  no partner name here. The timeline otherwise matches the person profile's,
  with its sources and evidence galleries.
- **Couple media**: the canonical read-only `MediaGallery` for the family. Its
  `+` opens `DocumentForm` for the family, with the union's events offered as
  evidence targets.
- **Couple notes**: the notes attached to the family.

The page separates a spouse's own media from the couple's with the detail
bundle's `family_id` on each profile media tile. See the
[API Contract](api.md). A media attached both to a spouse and to the couple
appears in the spouse's column and in the couple's gallery.

---

## 7. Loading

The page fetches the family and its spouses, then both spouses' detail bundles
concurrently. A family that no longer exists fails the load.

The page also loads the following:
- the notes of each spouse and of the family;
- both portraits, in one request;
- each spouse's two-generation pedigree;
- the tree and the SOSA ancestor set, once.

While the couple loads, the page shows a single loading message. Selecting
another couple scrolls the content back to the top.

---

## 8. Responsive

- Above **1080px**: two spouse columns.
- At **1080px** and below: a single column. Each row's left spouse comes
  before the right spouse, and shared sections keep their place between the
  rows.
- Identity cards and family narratives inherit the person profile's narrow
  treatments.
