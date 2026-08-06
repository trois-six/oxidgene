---
type: "UI Specification"
title: "Visual & Functional Specifications — Geneanet Import"
description: "UI behavior and interaction specification for importing a Geneanet tree together with its photos."
tags: [oxidgene, specification, ui, ux]
timestamp: 2026-08-06T00:00:00Z
---


# Visual & Functional Specifications — Geneanet Import

> Part of the [OxidGene Specifications](index.md).
> See also: [Geneanet Media Import](geneanet-media-import.md) (the technical half) · [GEDCOM Import](ui-gedcom-import.md) · [Homepage](ui-home.md) · [Data Model](data-model.md)

---

## 1. Overview

Importing a Geneanet tree is not a file import. Geneanet hands out its data in
three pieces that only make sense together, and **two of them cannot be
downloaded at all** — the link between a photo and a person exists only behind
a logged-in session. See [Geneanet Media Import](geneanet-media-import.md) for
why.

That makes this flow different from [GEDCOM import](ui-gedcom-import.md) in one
decisive way: **the user has work to do on another website**, and most of them
have never heard of a `.gw` file. The interface is therefore as much a set of
instructions as a form.

**Desktop only.** Step 3 needs an embedded browser window. The web build shows
the flow with a note pointing at the desktop app or the CLI.

**Depends on Sprint F.1.** There is nowhere to store image bytes until media
storage exists. Steps 1–4 are buildable before F.1; step 5 is not.

---

## 2. Shape: one page, one open step

Not a modal wizard. A **single page** whose steps fill in from top to bottom.

- Exactly **one step is expanded at a time** — the current one.
- A completed step **collapses to a one-line summary** with its result, a green
  check, and an "Edit" affordance to reopen it.
- Reopening a step collapses whichever was open.
- Steps not yet reachable are visible but dimmed, so the whole journey is
  legible from the first second.

The point is that at any moment the page shows *one* thing to do, while the
lines above are a receipt of what has already been settled.

```
┌────────────────────────────────────────────────────────────┐
│  Import from Geneanet                                      │
├────────────────────────────────────────────────────────────┤
│  ✓  1. Your family tree file      myaccount_2026-08-01.gw     │
│        10 254 people                              [Edit]   │  ← collapsed
├────────────────────────────────────────────────────────────┤
│  ✓  2. Your photo archive         3 archives, 613 files    │
│                                                   [Edit]   │  ← collapsed
├────────────────────────────────────────────────────────────┤
│  ▼  3. Connect to Geneanet                                 │  ← expanded
│                                                            │
│     Your photos are private. OxidGene opens a Geneanet      │
│     login window — the same one as in your browser. Your    │
│     password is never seen by OxidGene.                     │
│                                                            │
│     ┌──────────────────┐                                   │
│     │  [ screenshot ]  │  Sign in as you normally would.    │
│     └──────────────────┘                                   │
│                                                            │
│              [ Open the Geneanet login window ]            │
├────────────────────────────────────────────────────────────┤
│     4. What will be imported                        (dim)  │
├────────────────────────────────────────────────────────────┤
│     5. Import                                       (dim)  │
└────────────────────────────────────────────────────────────┘
```

### Entry points

- [Homepage](ui-home.md) → **"+ New tree"** → *Import from Geneanet*
- [Settings](ui-settings.md) → **Tools** → *Import from Geneanet* (into the
  current tree)

---

## 3. Instructional screenshots

Steps 1, 2 and 3 each carry a **mini-screenshot** of the Geneanet page being
described, cropped to the relevant control, with a highlight on what to click.

- Stored as base64 next to the other embedded assets
  (`crates/oxidgene-ui/assets/geneanet-*.b64`, included with `include_str!`),
  matching how the logo and default portraits are handled.
- Cropped tight. They are wayfinding aids, not documentation of Geneanet's UI —
  the smaller the crop, the less often a redesign invalidates them.
- Clicking one opens it full-size in a lightbox.
- Each has descriptive `alt` text carrying the same instruction as the caption,
  so the step remains usable if the image fails to load or is not seen.

> **These will go stale.** Geneanet redesigns without warning us. Every
> screenshot is paired with a text instruction that must stand on its own, and
> no step may be completable *only* by following an image.

---

## 4. Step 1 — Your family tree file

### Explanation shown

> Geneanet can export your tree in two formats. OxidGene needs the **GeneWeb
> (`.gw`)** one — not the GEDCOM. The `.gw` records which of two people with
> the same name is which, and that is what lets OxidGene put each photo on the
> right person.

Then, numbered and paired with a screenshot:

1. Go to **gw.geneanet.org → My tree → Operations → Export**
2. Choose the **GeneWeb (`.gw`)** format
3. Tick **"Liens web vers les photos principales des individus"** and
   **"Images de la chronique familiale"**
4. Download the file

A collapsed *"Why not GEDCOM?"* disclosure repeats the reason in one sentence
for anyone who has a `.ged` already and wants to know why it will not do.

### Input

A single file picker (`.gw`). Drag-and-drop accepted.

### On selection

The file is parsed immediately — locally, no network — and the step collapses
to:

```
✓  1. Your family tree file      myaccount_2026-08-01.gw · 10 254 people   [Edit]
```

This is the first moment the user learns whether they picked the right file,
and it costs nothing.

### Errors

| Condition | Message |
|---|---|
| Not a `.gw` | *"This looks like a GEDCOM file. Geneanet can also export GeneWeb (`.gw`), which is what OxidGene needs — see the steps above."* |
| Parses, zero people | *"No person could be read from this file."* + parse-error count |
| Parses with skipped blocks | Proceed; show *"N blocks could not be read and will be skipped"* in the summary line |

---

## 5. Step 2 — Your photo archive *(optional)*

### Explanation shown

> If you have already downloaded your Geneanet data, point OxidGene at it and
> your photos are imported **without downloading them again**. Skip this and
> OxidGene will download them — slower, but nothing is lost either way.

1. Go to **www.geneanet.org → My data → Dashboard**
2. Request the download of your data
3. Wait for the email, then download the ZIP files

> **Do not unzip them.** Geneanet splits large exports into several ZIP files;
> add all of them. OxidGene reads them where they are.

### Input

A multi-file picker (`.zip`). Drag-and-drop of several files at once. Selected
archives are listed with a remove control each.

### On selection

Each archive's central directory is read — no extraction — and the step
collapses to:

```
✓  2. Your photo archive      3 archives · 613 files      [Edit]
```

If skipped:

```
—  2. Your photo archive      skipped — photos will be downloaded    [Edit]
```

### Errors

| Condition | Message |
|---|---|
| Not a ZIP / corrupt | *"This archive could not be read."* — that file only, the others stand |
| ZIP holds no images | Accept, warn: *"No images found in this archive — is it the right download?"* |
| Same archive added twice | Ignore silently |

---

## 6. Step 3 — Connect to Geneanet

### Explanation shown

> Your photos are private, and **which photo belongs to whom exists only on
> Geneanet** — it is not in the files you downloaded. OxidGene opens a Geneanet
> login window to read that list.
>
> It is a real browser window. You sign in exactly as you would normally, and
> **OxidGene never sees your password**.

### Behaviour

**[ Open the Geneanet login window ]** opens a second WebView window on
Geneanet's login page.

- The user authenticates interactively. If Geneanet shows a captcha or a
  Cloudflare check, it appears in that window and the user handles it — the
  same as in any browser.
- Once the session is established, the window closes on its own and collection
  starts.
- The window can be closed at any time; the step returns to its initial state.

> **Why a real browser window rather than asking for a cookie.** Two reasons.
> A normal user cannot copy a session cookie out of developer tools. And
> Geneanet sits behind Cloudflare, which challenges non-browser HTTP clients on
> their TLS fingerprint — a challenge OxidGene will not attempt to defeat.
> Using an actual browser engine is not a way around that check; it is the
> thing the check is asking for, and a human is present to satisfy it.

### Collection, with progress

Runs inside that window's session. Two stages, each with its own bar:

```
Stage 1 — Reading your photo list from Geneanet
          ████████████░░░░░░░░  6 / 19 requests

Stage 2 — Matching photos against your archives
          ██████████████████░░  341 / 378 photos
```

- **Stage 1** collects the person↔photo mapping — roughly 19 requests, seconds.
- **Stage 2** matches each photo to an entry in the archives by exact size. Runs
  only if step 2 supplied archives; otherwise it is skipped and the downloading
  happens in step 5 instead.

### On completion

```
✓  3. Connect to Geneanet     signed in as myaccount · 378 photos found   [Edit]
```

### Errors

| Condition | Message |
|---|---|
| Window closed before signing in | Return to initial state, no error |
| Session expires mid-collection | *"The Geneanet session ended. Sign in again to continue."* — collection resumes, already-collected data kept |
| Cloudflare challenge inside the window | Nothing to report — it is displayed and the user answers it |
| Geneanet returns an error | Show the stage and the HTTP status; **[Retry]** |

---

## 7. Step 4 — What will be imported

Computed with no further network access, from the `.gw` and the collected
mapping. This is the moment the user finds out whether the two halves belong to
each other, **before** anything is written.

```
┌──────────────┬──────────────┬──────────────┬──────────────┐
│    10 254    │     378      │     234      │     482      │
│    people    │   photos     │    people    │    photo     │
│   in file    │   found      │ with a photo │ attachments  │
└──────────────┴──────────────┴──────────────┴──────────────┘

   ✓  613 of 613 photos found in your archives — nothing to download
   ⓘ  62 photos show several people and will be attached to each of them
   ⓘ  235 photos are attached to nobody on Geneanet and will be skipped
   ⚠  35 photos name people who are not in this tree
```

The third and fourth lines are expandable, listing the affected names.

Then: **[ Import into a new tree ]** / **[ Import into "<tree>" ]**.

### The mismatch guard

If **fewer than 10 %** of keyed references find a person, the `.gw` and the
Geneanet account are almost certainly not the same tree. Block the import:

> *"Almost none of these photos match the people in this file. Are the export
> and the Geneanet account the same tree?"*

with **[ Go back to step 1 ]** as the primary action and a secondary
**[ Import anyway ]**.

---

## 8. Step 5 — Import

Blocked on [F.1 Media Storage](roadmap.md); the rest of the flow is not.

```
Importing…
  ✓  People and families        10 254 / 10 254
  ▶  Photos                        341 / 378
     Attaching photos to people
```

- Cancellable. Cancelling rolls the whole import back — a half-imported tree is
  worse than none.
- A photo that fails is reported and skipped; it does not abort the run.
- Photos already downloaded are not fetched again if the step is re-run.

### Result

```
✓  Imported into "Famille Dupont"

   10 254 people · 2 510 families · 378 photos · 482 attachments

   35 photos named people outside this tree and were skipped   [Details]
```

**[ Open the tree ]** (primary) · **[ Import another ]**

---

## 9. What this flow does not do

Named here so nobody looks for them:

- **Unlinked photos are not imported.** 235 of 614 on the reference account are
  attached to nobody on Geneanet. They are counted in step 4 and skipped.
- **Pages of multi-page documents come in downsized**, unless the full-resolution
  option is on. Geneanet exposes no per-page original — see
  [Geneanet Media Import §5](geneanet-media-import.md).
- **No incremental re-import.** Running it twice creates a second tree. Merging
  an updated Geneanet export into an existing tree is [Person Merge](ui-merge.md)
  territory and is not attempted here.
- **No writing to Geneanet.** Every request this flow makes is a read.

---

## 10. Keyboard & accessibility

- The expanded step receives focus when it opens; its first control is the tab
  stop.
- Collapsed steps are buttons — `Enter`/`Space` reopens them.
- Progress bars carry `aria-valuenow`/`aria-valuemax` and an adjacent live
  region announcing stage changes, not every tick.
- Screenshots are decorative *only* where the caption already carries the
  instruction; otherwise they take descriptive `alt` text (§3).
- The Geneanet login window is a real browser window with its own focus
  handling; OxidGene does not trap focus while it is open.

---

## 11. Responsive

- **≥ 900 px** — screenshot on the left, instructions on the right, within each
  expanded step.
- **< 900 px** — screenshot above the instructions, full width.
- Step summary lines truncate the filename from the middle
  (`myaccount_2026…-01.gw`), never the count that follows it.
- The stat row in step 4 wraps 4 → 2 → 1 per row.
