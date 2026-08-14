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

**Unblocked by Sprint F.1**, which shipped media storage. All five steps are
built; see §2b for the three that only the desktop app can run.

---

## 2. Shape: one modal, two tabs, one open step

The import lives in a **modal** opened from a tree card's `⋮` menu, replacing
the native file picker that menu used to open directly. It has two tabs:

- **A file** — the old behaviour. Drop or pick a `.ged`/`.gw`, import it.
- **From Geneanet** — five steps, of which exactly **one is expanded** at a
  time.

> An earlier draft of this spec called for a dedicated page and said "not a
> modal wizard". That was overturned deliberately: the two import routes are
> one decision made at one moment, and splitting them across a modal and a page
> made the cheap route feel like the real one and the complete route like an
> excursion. The step-at-a-time discipline below is what mattered, and it
> survives intact.

Within the Geneanet tab:

- A completed step **collapses to a one-line summary** with its result, a green
  check, and an "Edit" affordance to reopen it.
- Reopening a step collapses whichever was open.
- Steps not yet reachable are visible but dimmed, so the whole journey is
  legible from the first second.

The point is that at any moment the modal shows *one* thing to do, while the
lines above are a receipt of what has already been settled.

```
┌─ Import into "Famille Dupont" ────────────────────────── [×] ─┐
│  ○ A file        ● From Geneanet                              │
├───────────────────────────────────────────────────────────────┤
│  ✓  1. Your family tree file   myaccount_2026-08-01.gw        │
│        10 254 people                                  [Edit]  │  ← collapsed
├───────────────────────────────────────────────────────────────┤
│  ✓  2. Your photo archive      3 archives · 613 files [Edit]  │  ← collapsed
├───────────────────────────────────────────────────────────────┤
│  3  3. Connect to Geneanet                                    │  ← expanded
│                                                               │
│     Your photos are private. OxidGene opens a Geneanet        │
│     login window — the same one as in your browser. Your      │
│     password is never seen by OxidGene.                       │
│                                                               │
│              [ Open the Geneanet login window ]               │
├───────────────────────────────────────────────────────────────┤
│  4  4. What will be imported                          (dim)   │
├───────────────────────────────────────────────────────────────┤
│  5  5. Import                                         (dim)   │
└───────────────────────────────────────────────────────────────┘
```

### Entry points

- [Homepage](ui-home.md) → a tree card's **`⋮`** → *Import*
- [Settings](ui-settings.md) → **Tools** → *Import* (into the current tree)

The modal never creates a tree: it is opened from a tree's own menu, so the
destination is already chosen.

---

## 2b. What the web build cannot do

Three of the five steps need capabilities a browser does not have, and the tab
says so rather than offering controls that cannot work.

| Step | Web | Desktop | Why |
|---|---|---|---|
| 1. `.gw` file | ✅ | ✅ | Picked with the same file dialog and read the same way |
| 2. Photo archive | ❌ | ✅ | Reads multi-gigabyte ZIPs **by path**, a few kilobytes each. A browser has no path and would have to upload the whole archive to learn what its central directory already states. |
| 3. Connect to Geneanet | ❌ | ✅ | Needs a second browser window whose session this app can then issue requests through. A window a web page opens is a different origin: nothing comes back out of it. |
| 4. Preview | ✅ | ✅ | Computed server-side from what the earlier steps produced |
| 5. Import | `.gw` only | ✅ | The photo half needs 2 and 3 |

On web, steps 2 and 3 render an explanation naming the desktop app, and the
`.gw` still imports — **the genealogy arrives, the photos do not**. This is the
same boundary the underlying pipeline already had; the tab makes it visible
instead of letting a button fail.

## 3. Instructional screenshots

Steps 1, 2 and 3 each carry a **mini-screenshot** of the Geneanet page being
described, cropped to the relevant control, with a highlight on what to click.

> **Not yet built.** The steps ship with their numbered text instructions
> only. Cropping the screenshots requires a live Geneanet account and is
> deliberately left until the flow has been run against one, since a
> screenshot of the wrong page is worse than none. Everything below is the
> contract they must meet when they are added — and §3's own rule, that no
> step may be completable *only* by following an image, is what makes shipping
> without them a degradation rather than a hole.

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

A multi-file picker (`.zip`), desktop only. Selected archives are listed with a
remove control each.

**No drag-and-drop here**, unlike step 1. A dropped file reaches the app as
bytes with no path, and the whole point of this step is to read a few kilobytes
out of a file that may be several gigabytes — which needs the path. Step 1's
`.gw` is small enough to take either way, so it accepts both.

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

**[ Open the Geneanet login window ]** opens a second WebView window
(`wry`, on the desktop app's own event loop) at
`www.geneanet.org/media/manager`. Signed out, Geneanet redirects to login and
back — the journey the user would take anyway.

- The user authenticates interactively. If Geneanet shows a captcha or a
  Cloudflare check, it appears in that window and the user handles it — the
  same as in any browser.
- Once the session is established, the window closes on its own and collection
  starts.
- The window can be closed at any time; the step returns to its initial state.

**The requests are issued inside that window, not by OxidGene.** A small script
runs on each page load and reports whether the media API answers yet; once it
does, the collection and the size-matching pass run in the same window, on the
same session. This is not a detail of convenience — it is the whole reason the
window exists (see below), and it is why the metadata phase is out of reach of
the Cloudflare fingerprinting that challenges the CLI. The scripts are the same
ones `oxidgene-cli geneanet-media browser-script` prints for a user to paste
into their own console, shared from one place so the two cannot drift.

Afterwards the window's `gntsess5` cookie is read out for step 5 — and only if
the archives do not cover every photo, because a run that downloads nothing
needs no session at all.

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
- **Stage 2** asks Geneanet each single-page deposit's exact byte length with a
  `HEAD` — no body transferred — and the server matches those lengths against
  the archives' central directories. Runs only if step 2 supplied archives;
  otherwise it is skipped and the downloading happens in step 5 instead.
  Multi-page deposits are absent from this pass on purpose: their download is a
  ZIP Geneanet streams with no `Content-Length`, so there is no length to match.

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

Writes through the same [F.1 Media Storage](roadmap.md) path as any upload, so
a photo shared by several people is stored once and linked many times.

```
Importing…
  ✓  People and families        10 254 / 10 254
  ▶  Photos                        341 / 378
     Attaching photos to people
```

- A photo that fails is reported and skipped; it does not abort the run. By the
  time photos are being written the people are already in the database, and
  losing one scan is not a reason to throw away ten thousand persons.
- Photos already in the archives are never fetched.

> **Not cancellable yet, and it does not roll back.** The person import is one
> transaction and is all-or-nothing; the photo pass that follows is not, so
> interrupting it leaves a tree with some of its photos. That is a real gap
> against this spec's original wording, recorded here rather than quietly
> dropped. Closing it means a progress/cancel channel the write step does not
> have — the same missing piece as the per-photo progress bar below, which is
> currently an indeterminate one.

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
- **No account name in the summary.** Geneanet does not put it anywhere this
  flow reads, and scraping the page for it would be the first thing a redesign
  broke — so step 3 collapses to "signed in · N photos found".
- **One login window at a time.** A second sign-in while the first is still
  collecting would fight over the same session.

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
  expanded step (when §3's screenshots exist).
- **< 900 px** — screenshot above the instructions, full width; the step body
  loses the indent that lined it up under its title.
- **< 560 px** — a collapsed step's summary drops onto its own line below the
  title rather than competing with it for one that no longer fits both.
- Step summary lines truncate at the end, and the counts are written last so
  they survive — the filename is the part with room to spare.
- The stat row in step 4 wraps 4 → 2 → 1 per row at 900 px and 560 px.
