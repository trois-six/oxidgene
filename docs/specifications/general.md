---
type: "Product Specification"
title: "General — Vision, Users & Features"
description: "Product vision, target users, feature scope, and MVP boundaries for OxidGene."
tags: [oxidgene, specification, product, mvp]
timestamp: 2026-06-17T00:00:00Z
---


![OxidGene](../assets/OxidGene.png)

# General — Vision, Users & Features

> Part of the [OxidGene Specifications](index.md).
> See also: [Architecture](architecture.md) · [Data Model](data-model.md) · [API Contract](api.md) · [Roadmap](roadmap.md)

---

## 1. Context and Project Objectives

### 1.1 General Context

The project aims to develop a multiplatform genealogy application, built entirely in Rust, based on:

- a **Dioxus** frontend compiled to WebAssembly (WASM) for web and desktop, and
- a backend powered by **Axum** exposing an API simultaneously in REST (JSON) and GraphQL, with all features available through both protocols.

The application is designed to be:

- compiled as a **desktop client** running on Windows, Linux and macOS (single binary embedding an Axum server + SQLite + Dioxus WebView via Wry), and
- deployable as a **web application** through Docker containers:
    - frontend container (static WASM assets served by a lightweight HTTP server),
    - backend container (Axum server),
    - database container (PostgreSQL),
    - queuing application container (for EPIC F — Asynchronous Pipeline, post-MVP).

For technical details, see [Architecture](architecture.md).

### 1.2 Nature of the Application

OxidGene is a genealogy platform enabling users to create, view, edit, and share family trees and associated genealogical data (individuals, relationships, events, sources, media).

### 1.3 Main Objectives

- Deliver a modern, high-performance, portable genealogy application.
- Provide an open API (REST + GraphQL) aligned with the design principles of the FamilySearch API. → see [API Contract](api.md)
- Ensure a user experience comparable to leading genealogy platforms.
- Allow progressive evolution toward advanced and paid features.

### 1.4 Differentiation

- Made in Rust — performance, safety, and a single language across the entire stack.
- A theme-based UX system reproducing the experience of Geneanet, Filae, Ancestry, or MyHeritage.
- A unified Rust + WASM architecture with a single Dioxus codebase for web and desktop.
- A dual REST/GraphQL API.
- A fully offline-capable desktop client.
- Advanced collaboration and tree-matching features (post-MVP).

---

## 2. Target Users and Roles

### 2.1 Target Users

- Individuals practicing genealogy.
- Genealogy associations.
- Professional or advanced users.
- Paid subscribers (future phases).

### 2.2 User Roles (per tree)

- **Guest**: limited access, contemporary individuals hidden. → see [Settings](ui-settings.md) (privacy section)
- **Full Read-only**: full tree access.
- **Editor**: read + create/modify/delete.

### 2.3 Access Control

- Trees can be private, shared, or public.
- Access rights defined per tree.
- Authentication deferred to EPIC E (not in MVP). → see [Roadmap](roadmap.md)

---

## 3. Core Features

### 3.1 Tree Management

- Create trees from scratch or via GEDCOM import.
- Manage multiple trees.
- → see [Homepage spec](ui-home.md)

### 3.2 GEDCOM Import/Export

- Full import/export using Rust crate `ged_io` (v0.12+). Also Support exporting a subpart of the tree by selecting a root person when exporting.
- Support for GEDCOM 5.5.1 and 7.0 (auto-detected).
- Streaming parser for large files.
- Error logging and normalization.
- GeneWeb `.gw` import (crate `geneweb`), converted through the same `ged_io` model.
- **Geneanet trees keep their photos.** A Geneanet export carries at most one medium per individual, as a URL that `403`s for anyone not logged in — losing ~55 % of the person↔photo links and every group photo. The full mapping is recovered from Geneanet's media API and joined onto the `.gw` by GeneWeb key, then imported straight into a tree as `Media` + `MediaLink` rows, so a group photo lands on everyone in it. A guided single-page flow walks the user through the Geneanet side, including an in-app login window. Exporting that tree to `.gdz` afterwards is the existing export path. See [Geneanet Media Import](geneanet-media-import.md) for the mechanism and [Geneanet Import](ui-geneanet-import.md) for the flow. Depends on Sprint F.1 for media storage.
- → see [API Contract](api.md) (GEDCOM endpoints) · [Settings](ui-settings.md) (export section)

### 3.3 Collaborative Editing (Web) — Post-MVP

- Simultaneous editing (deferred to post-MVP).
- Conflict detection and resolution.

### 3.4 Tree Matching — Post-MVP

- Suggest merges between user trees.

### 3.5 Themes / UX

- Switch between multiple UX themes inspired by major genealogy platforms from the settings.
- → see [Settings](ui-settings.md)

### 3.6 Interface Language

- Configurable UI language, without restart.
- User-level (web) or app-level (desktop).

### 3.7 REST & GraphQL APIs

- Full feature parity between both protocols.
- FamilySearch-inspired structure.
- Available from EPIC A onward.
- → see [API Contract](api.md)

### 3.8 Media Management

- Upload images/PDF/videos.
- Metadata and viewer integration.
- Post-MVP: identify someone in a subpart of an image, the selection will be seen as a media for the identified person.
- Async upload pipeline (post-MVP).
- → see [Person Edit Modal](ui-person-edit-modal.md) (media section)

### 3.9 Statistics & Reports

- Frequent last/first names, frequent occupations, birth distribution by months, parents age at birth, avg date at first union, birth/death stats, demographic pyramid, distribution of marriage days, avg duration of an union, avg children per union, avg duration between two children, avg age difference between first and last child in a couple, age diff between spouses, geographic distribution, last 100 births, last 100 deaths, last 100 unions, top 100 alive oldest, top 100 older...
- Graphs, tables, PDF export.

### 3.10 Visualization & Printing

- Multiple tree layouts (ancestor chart, descendant chart, fan chart).
- Export high-resolution PDFs.
- → see [Tree View spec](ui-genealogy-tree.md)

---

## 4. Security & Privacy

- Mask contemporary individuals (< 100 years old) for guest users. → see [Settings](ui-settings.md) (privacy section)
- Optional last/first name masking.
- Full audit logging.
- Authentication and authorization in EPIC E. → see [Roadmap](roadmap.md)

---

## 5. Performance

- Lazy loading of tree branches.
- Server-side caching.
- Recursive CTE over the family links for ancestor/descendant queries. → see [Data Model](data-model.md) (Ancestry traversal)
- Streaming GEDCOM parser for large files.
- Cursor-based pagination to avoid expensive offset scans. → see [API Contract](api.md) (pagination)

---

## 6. Premium Features — Post-MVP

- Assisted tree matching.
- OCR on scanned documents.
- Image enhancement.
- External data source plugins.

---

## 7. MVP Scope

The MVP covers EPICs A through D (see [Roadmap](roadmap.md)):

- Interactive tree visualization. → [Tree View](ui-genealogy-tree.md)
- Person selection and detail view.
- Full CRUD editing (persons, families, events, sources, media, places, notes). → [Person Edit Modal](ui-person-edit-modal.md)
- GEDCOM import/export.
- Language switching.
- Theme support. → [Settings](ui-settings.md)
- REST + GraphQL API. → [API Contract](api.md)
- Desktop and web deployment. → [Architecture](architecture.md)

**Not in MVP**: authentication, access control, collaborative editing, tree matching, async pipeline.

---

## 8. Consistent Page Layout

All pages share a common layout structure to ensure visual consistency across the application.

### Navbar

A minimal branding bar at the very top of every page. Contains only the logo (linking to homepage) in MVP. See [Topbar](ui-topbar.md) for full specification.

### Page types

The application has two distinct page layout patterns:

#### 1. Homepage (`/`)

Full-page scrollable layout. Content is constrained by `.home-main` (`max-width: 1200px`, centered, responsive padding). No topbar breadcrumb — the page header contains the title and subtitle directly.

#### 2. Tree-scoped pages (`/trees/{id}/...` and `/settings`)

All tree-scoped and app settings pages use the **`sub-page`** layout pattern:

```
+----------------------------------------------------------------------+
| NAVBAR                                                                |
+----------------------------------------------------------------------+
| td-topbar (breadcrumb + optional actions)                            |
+----------------------------------------------------------------------+
|                                                                       |
|   sub-page-content (max-width: 1200px, centered, scrollable)        |
|                                                                       |
|   Page-specific content here                                         |
|                                                                       |
+----------------------------------------------------------------------+
```

**CSS classes:**

| Class | Purpose |
|---|---|
| `.sub-page` | Flex column container, fills available height (`flex: 1`), hides overflow |
| `.td-topbar` | Full-width breadcrumb bar with bottom border. Contains `.td-bc` breadcrumb navigation |
| `.sub-page-content` | Scrollable content area. `max-width: 1200px`, centered with `margin: 0 auto`, `padding: 24px` |

**Exception — Pedigree tree view** (`/trees/{id}`): Uses its own layout with left sidebar (ISB), canvas, and events panel. Does not use `sub-page-content`. See [Tree View](ui-genealogy-tree.md) for details.

### Breadcrumb pattern

All pages (except homepage) display a breadcrumb in the `td-topbar`:

| Page | Breadcrumb |
|---|---|
| Tree view | `logo` tree_name `/` Tree |
| Tree settings | `logo` tree_name `/` Settings |
| Search results | `logo` tree_name `/` Search |
| Person profile | `logo` tree_name `/` Person Name |
| App settings | Home `/` Settings |

### Responsive behavior

| Breakpoint | Behavior |
|---|---|
| >= 1200px | Full layout, content at max-width |
| < 640px | `sub-page-content` padding reduces to `16px 12px`. `td-topbar` padding reduces to `10px 12px`. Homepage padding reduces to `2rem 1rem` |

### Max-width consistency

All content areas use `max-width: 1200px` for a unified reading width across all pages. This applies to:
- Homepage (`.home-main`)
- Tree settings, app settings, person profile, search results (`.sub-page-content`)

---

## 8b. Development Status

> For sprint details see [Roadmap](roadmap.md).

| EPIC | Title | Status |
|------|-------|--------|
| A | Technical Foundation | ✅ Complete |
| B | GEDCOM Engine | ✅ Complete |
| C | Tree Editing (Frontend) | ✅ Complete |
| D | UX, Languages, Performance | ✅ Complete |
| E | Read Projections & Search | ✅ E.9 Complete; 🔄 E.8 (dictionary descent view) planned |
| F | Media Management | 🔄 F.1–F.3 + the Geneanet import shipped (S3 backend, PostgreSQL verification, PDF page rendering outstanding); F.4 planned |
| G | Security & Deployment | ⏳ Post-Media |
| H | Asynchronous Pipeline | ⏳ Post-MVP |

**Recently shipped (Aug 2026):**
- **A Geneanet tree can be imported with its photos, which no Geneanet export can carry.** The tree card's `⋮` → Import used to open a native file dialog and nothing else; it now opens a **modal with two tabs**. The file tab is the old behaviour, fixed: it read the picked file through `path()` + `tokio::fs::read`, which has no meaning in a browser, so the "shared web/desktop" import had in fact been desktop-only. It now reads the bytes the one way that means the same thing in both.

  The second tab is the interesting one, and it is [as much a set of instructions as a form](ui-geneanet-import.md): the user has work to do on another website, and most of them have never heard of a `.gw` file. Five steps, exactly one expanded, each settled step collapsing to a one-line receipt. **Step 1** takes the `.gw` — not the GEDCOM, because only the `.gw` carries the occurrence number that says which of two same-named people is which — and parses it *on selection*, so picking the wrong export fails in the first second rather than four steps later. **Step 2** takes the data-export ZIPs **without unzipping them**: a ZIP's central directory states every entry's uncompressed length, which is exactly what the matching needs, so several gigabytes cost a few kilobytes to read. **Step 3** opens a real Geneanet login window and issues the collection requests *inside it* — geneanet.org sits behind Cloudflare, which challenges HTTP clients on their TLS fingerprint, and using a browser engine is not a way around that check but the thing the check is asking for. **Step 4** joins the two halves and says what would happen before anything is written, blocking outright when under 10 % of keyed references find a person, because "the export and the account are different trees" is a common mistake that is invisible until it is expensive. **Step 5** writes one `media` row per photo and one `media_link` per person on it — so a group photo is stored once and attached to everyone in it, which is precisely what the export could not express and what `MediaLink`'s shape was always for.

  Steps 2, 3 and the photo half of 5 are **desktop-only**, and the tab says so plainly rather than offering controls that cannot work: a browser has no path to read a multi-gigabyte archive by, and a window a web page opens is a different origin it may not read back. On web the `.gw` still imports — the genealogy arrives, the photos do not. The pipeline itself moved out of the CLI into `crates/oxidgene-geneanet` so the headless and the interactive paths cannot drift apart on the join, the key folding or the size matching.

- **Photographs and scans are now something you can see, attach and crop.** F.1 gave media somewhere to live; this is the half a user touches. Every person's and every couple's edit modal gained a **Media** section — a thumbnail grid ending in an upload cell that takes a click or a drop — and the person profile page shows the same grid read-only, so a reader who clicks Edit finds the gallery they were just looking at rather than a different one. Uploads run **one at a time**: sending a folder of scans over a connection you do not control should read as "3 of 12", not as twelve stalled requests finishing in an unpredictable order, and one rejected file names itself and lets the other eleven through. A tile is drawn from a **single request per gallery** — the media-links endpoint now returns the media alongside its link, because the MIME type and whether a thumbnail exists are what decide how a tile looks, and asking for them per tile would make a grid of twenty scans twenty-one round trips. A missing thumbnail is the server saying it could not rasterise the file, so the tile draws a labelled `PDF` icon instead of the broken image an `<img>` onto a 404 gives you. The **★ profile photo** finally has a write path: `is_profile` existed on `media_link` since EPIC A and nothing could ever set it. Setting one clears the person's others *in the same statement* — the invariant is "at most one", and leaving that to two calls means a failure between them puts two stars in the tree — and rebuilds the person's projection, since the portrait is embedded in `person_denorm`. Only a person may have one; a couple's card shows its spouses'. The **cropper** is the piece with real substance behind it: the user drags in whatever pixels the image happens to occupy on screen, a vignette is stored in the source image's own pixels, and keeping those two frames apart is the whole job. The ratio comes from `media.width`/`media.height` — recorded at upload precisely so the frontend never has to decode an image to learn how big it is — against the element's measured client rect, converted once on save so dragging never accumulates rounding error. Crops already on the page are drawn while you draw the next one, because without them the same register entry gets cropped twice; a drag under sixteen source pixels is treated as a click, since otherwise every stray tap would try to save a 1×1 vignette and be rejected, which reads as the app throwing errors at you for touching it. Saving keeps the cropper open — a parish-register page is four crops in a row, and closing after each would mean reopening the same scan four times. Attribution is deliberately *not* in the cropper: which entry belongs to whom is decided afterwards looking at several crops together, so it lives in a list beside them. **Still missing, and listed as such:** there is no page-by-page viewer for a multi-page document (the count is known and shown, but rendering page 7 of a PDF needs the rasteriser F.1 declined to take on as a C dependency), a media's date and place are not yet editable, and none of this has been exercised by hand in the running app — it is verified by compiling and by 32 API integration tests. See [Sprint F.2](roadmap.md), [Person Edit Modal §10](ui-person-edit-modal.md).
- **Media files have somewhere to live, and a crop of a scan is a thing you can keep.** `Media` rows have existed since EPIC A but held metadata only: `file_path` was whatever a GEDCOM `OBJE.FILE` tag said — a path on someone else's machine — and no byte of any photo ever reached OxidGene. `POST /trees/{id}/media/upload` now takes a file and stores it, **content-addressed**: the SHA-256 of the bytes is the key (`{tree_id}/{aa}/{bb}/{sha256}.{ext}`), which gets deduplication, a free strong `ETag`, and detection of a corrupted transfer out of one decision — a census page documenting eight siblings is uploaded once per person and stored once. Keys are scoped per tree rather than globally so purging a tree is one directory removal, with no reference counting and no chance of pulling a file out from under another tree; the price is that two trees holding the same photo store it twice. Files sit under the platform's user-data directory by default (`~/.local/share/oxidgene/media` on Linux, `OXIDGENE_MEDIA_ROOT` to override), which is the directory a user's backup already covers. `file_path` was **not** repurposed as the storage location: it is what an export has to write back, so `storage_key` is a second column, and a GEDCOM-imported record keeps its foreign path with a null key — precisely the "we know this file's name and not its content" state the UI needs, and one that `upload` with a `media_id` fills in. What the file *is* is decided from its magic bytes, never from the declared MIME type or the extension, so a renamed executable is a `400` rather than something we store and later hand to a decoder. Thumbnails are generated on upload (400 px longest edge, EXIF orientation applied so a portrait photo is not served sideways, alpha kept as PNG so a transparent background is not flattened onto black, and a decode limit so a few kilobytes of crafted PNG cannot claim 13 GiB). **PDFs deliberately get none**: rasterising a page needs pdfium or mupdf, a C dependency on a project shipping a desktop binary for three platforms — the endpoint answers `404` and the gallery branches on a status code. Documents are counted instead of rendered: `lopdf` for PDF, a hand-written IFD-chain walk for TIFF (classic and BigTIFF, with a loop guard), both reading headers rather than pixels, so a 300 MB register scan costs a few hundred bytes to count. **Vignettes** are the other half: a parish-register page routinely documents four unrelated families, and each entry is now a rectangle recorded against the one stored scan — `GET .../vignettes/{id}/image` crops on read — rather than four copies of a 40 MB file, so replacing the scan with a better one does not orphan the crops. A rectangle that does not fit its media is refused at write time, so a stored vignette always describes a region that exists. Serving honours `If-None-Match` (a reloaded gallery gets `304`s), and `Content-Disposition` is sent in both RFC 6266 forms so `acte_thérèse.jpg` survives the trip. Storage sits behind a `MediaStore` trait; the S3 backend the server deployment wants is not written yet, and the PostgreSQL path remains unexercised. See [Sprint F.1](roadmap.md), [API Contract](api.md), [Data Model](data-model.md).
- **Changing a date's calendar now converts the date, and named months are picked rather than counted.** The calendar selector records which calendar a date was *written* in, but it only relabelled what was already typed: an entry made as 11 March 1796 and then switched to French Republican became « 11 frimaire 1796 » — a date three years and nine months away from the one the user had entered, and one that exports under a `@#DFRENCH R@` escape as a fact about a different day. Switching calendars now renumbers every date the widget holds onto the new one (11 March 1796 → 21 ventôse an IV), both ends of an `Or`/`Between` range together so a range never ends up with one foot in each calendar. A partial date is a period, not a day, so it converts through the middle of that period and comes back at the precision it was entered with — « 1900 » Gregorian is « 1900 » Julian, not 1899, which is what converting its first day would have said. A date the target calendar cannot express (anything before 22 September 1792, for the Republican one) is left exactly as typed rather than replaced by an invention. The **month field** is now a dropdown of the calendar's own month names wherever those names are what the reader knows the month by — vendémiaire…fructidor plus the jours complémentaires, Tishrei…Elul — while Gregorian and Julian keep the numeric field, since "3" is legible there and typing it is faster than opening a list. Its first entry is blank, not a "MM" prompt: a date known to the year alone is ordinary in a register (« an VI »), so the month has to stay omissible exactly as an empty MM allows in Gregorian; Adar II is offered only in a Hebrew leap year, the only kind that has one. Two bugs surfaced building it. The list is generated by a loop, so it is appended *after* the select's own attributes are applied and the `value` binding every other select uses set nothing at all — the field came back reading "MM" over a month it knew perfectly well; binding through `selected` on each option fixes it, and the place picker, which has the same shape, was corrected with it. And the plausible-year window was framed in Gregorian numbers alone (−9999…2999), so every real Hebrew year — 5784 is 2023 — was turned away as a slipped keystroke; each calendar is now judged in its own era. The conversion lives in **`oxidgene_core::calendar`**, not in `oxidgene-gedcom`: the editor runs in WASM, where `ged_io` — which the server uses to derive `date_sort` — cannot be reached. Each calendar maps onto the Julian Day Number and nothing converts to anything else directly. The Republican new year is *tabulated* for the fourteen years the calendar was in force, because it was the observed equinox at the Paris meridian and no arithmetic rule reproduces it; those are the same documented dates `oxidgene-gedcom`'s tests pin `ged_io` to, so the editor and the server's sort order cannot disagree. Hebrew dates go through the classical molad calculation with its four dehiyyot. Every day of a fifty-year stretch is asserted to survive a round trip through all four calendars. **Validation follows the calendar too**, now that the month lengths are real: six jours complémentaires is a date only in a sextile year, and Adar II only in a leap year — where before every Hebrew and Republican month was assumed 30 days long. See [`ui-person-edit-modal.md` §8](ui-person-edit-modal.md).
- **Dates can be entered from an age, are shown with their qualifier, and no longer mangle the non-Gregorian calendars.** Three problems in one control. (1) `DateQualifier::FromAge` was in the qualifier `<select>` but had nothing behind it — it rendered the same JJ/MM/AAAA fields as every other entry, so picking it did nothing. It is now a real entry mode: the triplet is replaced by an age and the year that age was observed in (pre-filled with the current year, which is what an age usually comes from), and `DateParts::resolved` collapses the pair into the `About <year − age>` it stands for. It is deliberately **never persisted as `FromAge`** — neither GEDCOM, nor GeneWeb, nor our own schema can record "aged 14 in 2026", only the birth year it implies — so re-editing shows « Vers 2012 » and a legacy `from_age` row reads back as the `About` date it always meant. (2) Every view printed an event's bare `date_value`, dropping the qualifier that gives it its meaning: an `About` date read "2012" rather than « vers 2012 », and a `Between` range showed only its first date. One `format_date`/`format_event_date` now renders every date the reader sees — profile vitals, unions, the event list, both edit modals' event rows, the pedigree events panel — and the editor's own literal preview goes through it as well, so what the preview promises is exactly what the page shows. (3) The widget wrote `JAN`/`FEB`/… whatever calendar was selected, so a French Republican date was stored as `2 FEB 14` and exported under a `@#DFRENCH R@` escape as a line no reader can take back, while an imported `2 BRUM 14` lost its month entirely (the token was neither a Gregorian month nor a number, so it was skipped). Each calendar now carries its own vocabulary — `VEND…COMP`, `TSH…ELL`, thirteenth month included — for both the canonical stored value and the localized display (« 2 brumaire 14 »). **BCE years** came with it, stored the GEDCOM way (`15 MAR 44 BCE`, not `-44`, so exports stay readable) and parsed back from `BCE`/`BC`/`B.C.` or a leading minus; the range is 9999 BCE – 2999, which reaches well past the first dynasties of Egypt, excluding year 0 since there is none. Finally the fields are **guarded**: a keystroke guard turns away non-digits (a minus is allowed in year fields only), paste and IME are caught by digit-stripping, and validation rejects entries that are not dates — 30 February, a thirteenth Gregorian month, a `Between` range running backwards, an age past 130 — with leap rules following the calendar, so 29 Feb 1900 is valid Julian and invalid Gregorian. An out-of-range value is now kept and explained inline under the widget instead of being silently blanked, which used to look like the app eating keystrokes. **`date_sort` moved to the server with it.** It orders events against one another, so it has to be Gregorian whatever calendar the date was written in — and that conversion needs `ged_io`, which a WASM frontend cannot reach, so the frontend (which was filling the column in) simply read the month number as if it were Gregorian: a Republican `2 BRUM 14` sorted in year 14, thirteen centuries adrift, and a thirteenth month produced no key at all. `oxidgene_gedcom::date::sort_key` now exposes the conversion the import path had been using all along, and `service::event_date` wraps it for both write surfaces; a patch re-derives from the stored event for whichever of calendar/value it leaves alone, the two being meaningless apart. `date_sort` is gone from the REST and GraphQL request shapes and from the frontend's bodies — a field the server overwrites is worse than no field, since it invites a client to compute what it cannot compute correctly. **One upstream bug surfaced and is corrected locally:** `ged_io` converts a Republican date from the *start* of the Republican day, and that calendar is anchored to Paris, so the instant falls 9m21s inside the previous Gregorian day and every Republican date came back one day early (its epoch, 1 Vendémiaire An I, is 22 September 1792; it answered the 21st). The correction is *measured* against that epoch rather than hardcoded, so it reads zero and stops applying the day the conversion is fixed upstream — a literal "+1 day" would start overshooting instead. Julian and Hebrew were checked against documented dates (Rosh Hashanah 5784, Julian 15 Mar 1582) and are correct, so they are left untouched.
- **A GEDCOM `TYPE` that only restates its event type is no longer kept as the description.** `type_text_restates_event_type` already dropped a bare tag name (`2 TYPE EDUC` under an `Education` event), but not the same thing spelled out in words: an `EVEN` with `TYPE Military service` resolved to `MilitaryService` *and* kept "Military service" as its description, so the person page showed an English phrase beside a badge already reading « Service militaire » — the badge again, in the exporter's language rather than the reader's. Whole-phrase restatements are now recognised too, across both the `geneweb` crate's labels and the ordinary GEDCOM tags an exporter chose to spell out. A `TYPE` that says *more* is still kept: "PACS" and "Concubinage" both arrive as `CivilUnion` and only the description tells them apart, and "Military service in Algeria" carries a fact the type does not. Note this only affects new imports — rows already stored keep their description.
- **Alt-tabbing back to the desktop app no longer resets the tree.** The pedigree canvas re-fitted the graph — discarding the reader's pan and zoom — on every `resize` event, and WebKitGTK fires one whenever the window is remapped. Returning to the app therefore snapped the tree home for no visible reason, which reads as the whole app having reloaded itself. The handler now compares the viewport's actual dimensions and ignores a `resize` that did not change them.
- **Every event now carries its own notes and source, and the person edit modal's Civil Status block was reorganised.** Notes and sources existed in the data model (`Note` and `Citation` both carry `event_id`) but only the Occupation create form ever wrote them, and it wrote them wrong: the notes went into `Citation.text`, so clearing the source deleted them, and a sourceless entry could not be represented at all. Both are now written as what they are — a `Note` row for the notes, a `Citation` row for the source — and are editable on birth, death, occupations and every other event. Birth and death expose the two fields inline and save them with the modal's footer button (attaching them to the event that same save creates, if there was none); rows in the occupation and other-event lists get a **"Notes & source"** toggle with its own Save, mounted only while open so a long list costs nothing. Legacy rows whose notes sat in `Citation.text` are still read, and migrated to a `Note` on the next save. **Source fields are free text**, not a picker over existing `Source` rows: a source is typed the way it is read off the record ("AD44 — Vigneux-de-Bretagne — N — 1913 — 3E217/46"), and requiring the row to exist first would put a detour in the middle of entering an event; the typed title is reconciled against the tree's sources on save (case-insensitive match on the trimmed title reuses that row, anything else creates one), Sources are only touched when the typed title actually changed, so an unrelated save never creates a `Source` row as a side effect. Deliberately no completion dropdown: a `<datalist>` of every source in the tree had to re-diff thousands of `<option>` nodes on each keystroke, which made the field unusable — completion belongs on a debounced prefix query (`dictionary_sources`), not on a list of everything. Changing the source now **edits the citation in place**: `PUT /citations/{id}` (and GraphQL `updateCitation`) gained `source_id`, since a citation records which document backs a fact and correcting that document is an edit of the same citation — deleting and recreating it stranded the row every reference pointed at. Notes and citations are likewise only created when there is none and only deleted when their field is cleared, and each save returns the state it wrote so a second save reconciles against those rows instead of leaving a duplicate behind. The source a citation just let go is then collected: `DELETE /sources/{id}?only_if_unused=true` (GraphQL `deleteSource(onlyIfUnused:)`) soft-deletes it *only* when no citation, note **and** media link still names it — otherwise a corrected typo would leave its `Source` in the tree, and in the source dictionary, forever, while a source that is genuinely in use is never at risk. The person's own notes moved up into Civil Status as the last block — and became editable in place, having until now been add-or-delete only — next to a person-level source field; per-`PersonName` notes were left out, since neither `Note` nor `Citation` has a `person_name_id` and adding one is a schema change. The event's one-line `description` field is now labelled "Description" rather than "Note", which it shared with the block underneath it. Two cosmetic fixes came with it: the date-qualifier `<select>` sat off the date input beside it, because while the engine draws the native control it positions the selected option with its own metrics and the CSS `padding`/`line-height` are advisory at best — the modal's selects now set `appearance: none` (with a CSS chevron, per palette) so they are ordinary boxes that obey both. And the "Profession(s)" / "Additional information" / "Notes" block headings dropped their bold `<h3>` for the same weight and colour as a field label such as "Gender", each with its add button on the same line. **Finally, the modal's buttons were given a hierarchy**, because all these lists had put eight orange controls and five solid-red ones on screen at once, leaving no way to tell which button to press. Buttons now fall into exactly three tiers: the footer save is the only filled orange gradient; committing an open sub-form is an orange outline on a tint (`.pf-confirm-btn`, and at most one sub-form is open at a time); everything else — the per-list add, the per-row edit and delete — is monochrome at rest and colours only on hover (`.pf-add-btn`, `.pf-row-btn`), with labels kept legible when idle since a control revealed only on hover is unreachable by touch. Filled red is now reserved for a destructive action already behind a confirmation, so a row's own delete turns red on hover instead of shipping as a red block. Section headings keep the orange — uppercase, letterspaced and 0.68rem, they cannot be mistaken for a control, and they are what makes the form's spine scannable — so the rule governs controls only. Two theming bugs surfaced doing it: the section-title rule and the gender-button hover were hardcoded to `rgba(255,255,255,…)`, i.e. invisible on the light palette, and an open sub-form sat on `--bg-deep`, a hair off the modal's own `--bg-panel`, so the box had no visible edge and its fields ran into the surrounding form. See [`ui-person-edit-modal.md` §2/§4/§5/§9](ui-person-edit-modal.md).
- **Detection no longer splits Breton and Norman surnames, and a bad particle can now be repaired for a whole family at once.** `split_surname_particle`'s flat particle list matched a bare leading article the same as a preposition, so a name like "LE BRANCH" was cut into particle "LE" + root "BRANCH" — wrong for every person carrying it, since a welded article ("Le …", "La …", Italian "Lo …", Dutch "Den …") is part of the name, not a particle, while the same words *are* particles once a preposition introduces them ("de **la** Cruz", "van **der** Berg"). The particle list is now two-tiered — `HEAD_PARTICLES` (prepositions, may open a run) and `TAIL_PARTICLES` (bare articles, count only immediately after a head particle) — so a leading article stays welded to the surname while "de la Cruz" still splits correctly; same split for elided forms (`d'`/`l'`). This only fixes detection going forward; existing rows keep whatever they were cut to. To repair those, the Dictionary's Family Names tab gained a bulk particle editor (pencil icon on each row): pick a new particle for a surname (empty = none) and it re-cuts every `PersonName` row carrying that exact surname in one call (`PATCH /trees/{id}/dictionary/family-names/particle`, mirrored as GraphQL `setFamilyNameParticle` — both reject a particle that isn't at the head of the surname, since accepting one would inject a word the tree never had and inserting-then-clearing it back out its way to escape wouldn't be reversible). Rows already cut that way are skipped, making a repeat call a no-op. Since a surname reaches every projection embedding a display name, a real change triggers a full tree projection rebuild, same trade-off as GEDCOM import. Also: when a surname files under its root (viewer's "sort particles" preference off), its dictionary row now reads root-first with the particle parenthesised — "d'Aubigné" under A shows as "Aubigné (d')" — instead of leaving the particle stranded at the front of a row it no longer alphabetizes by. See [`ui-dictionary.md` §7/§7.1](ui-dictionary.md), [API Contract](api.md).
- **First run adopts the OS language and appearance.** Both defaults were already meant to follow the system, but the language only read `navigator.language` — the *single* top entry — so a user whose OS prefers German then French landed in English despite the French UI existing. Detection now walks the whole ordered `navigator.languages` list and takes the first entry OxidGene is translated into, matching on the primary subtag (`fr-FR`, `fr_CA` → French). The stored choice from `/app-settings` is simply the head of that same list, so an explicit pick still wins and a corrupted stored value falls through to detection instead of pinning English. The theme keeps reading `prefers-color-scheme`, now with storage and `matchMedia` probed independently so that blocked storage (private browsing) no longer skips the OS query and a webview without `matchMedia` no longer throws. English and the light theme are the fallbacks whenever detection yields nothing. See [i18n](i18n.md) §3, [Design Tokens](ui-design-tokens.md) §2.
- **Surname particles are structured, and every information type keeps its identity.** `person_name` gained `surname_prefix` (GEDCOM `SPFX`) and `sort_order`. Four problems were fixed at once. (1) The particle used to be glued into `surname`, so "de la Cruz" and "Cruz" were unrelated dictionary entries and the former could only file under D; `SPFX` is now split off, and since `ged_io` had parsed and written it all along, this was OxidGene dropping it on import and never emitting it on export. The particle is **derived, not typed** — the UI keeps one surname field and shows the detected split (`split_surname_particle`, a known-particle list excluding `Mac`/`Mc`/`O'` which bind to their root) so a wrong guess is correctable; GEDCOM and `.gw` import derive it the same way when the file carries no `SPFX`. Display always rejoins the parts, so only *filing* changes, and whether the particle counts when sorting is a per-viewer preference (`/app-settings` → Noms, default "included"). (2) The picker's Alias / Surnom / Sobriquet / Prénom all collapsed onto `AlsoKnownAs` on save, so the user's choice was unrecoverable on reload — each now has its own `NameType` variant (`Alias`, `Byname`, `Sobriquet`, `GivenName`), all exporting as GEDCOM `aka` since `NAME.TYPE` has no finer enumeration. Editing a name also fed the `Debug` spelling back through `parse_name_type`, silently downgrading it to `Other`. (3) `prefix`/`suffix` (`NPFX`/`NSFX`) were hardcoded to `None` in the add form and had no picker entry at all, making them unreachable. (4) Names now carry an explicit order instead of arriving unsorted. The export `NAME` line is unchanged — it still carries the full surname between slashes — with `SPFX` added beside `SURN`. `.gdz` is unaffected: it is a zip wrapper over the same GEDCOM, and export-only. See [Data Model](data-model.md) (PersonName).
- **Note bodies render as HTML, with one canonical line break.** The formats OxidGene imports put markup in their notes, so note bodies are sanitized on write (`ammonia` allowlist in `oxidgene_db::html`, applied at the repo and import persistence layers; pre-existing rows cleaned by migration) and rendered rather than escaped. That exposed a second problem: the *same* note is spelled three ways depending on where it came from — GEDCOM `CONT` lines give `\n`, GeneWeb `.gw` ends its note lines with `<br/>` *and* the file's newline, the app's own textarea gives `\n` — which as HTML rendered as no break, a double break, and no break, for text the author meant identically (both spellings of one real note are visible in `samples/myaccount_2026-08-01.ged` line 713 and `.gw` line 1505). The sanitizer now folds every break to a single `\n` — a `<br>` glued to a newline counts once, two `<br>` stay a blank line, runs cap at two, and breaks against a block element or either end of the note are dropped — and the UI turns `\n` back into `<br>` at display. Storing the plain-text form rather than the markup one is what keeps GEDCOM export writing real `CONT` lines instead of a literal `<br>`, keeps the note textarea showing text instead of tags, and gives previews and any future full-text index clean input. The cost: a `<br>` the author genuinely typed is no longer distinguishable from a newline.
- **Tree deletion no longer freezes the app, and the closure table is gone.** Deleting an imported tree used to block the whole UI for ~8 s. Measured cause: 98.6 % of the request sat in `TreeRepo::delete`, because SQLite resolves `ON DELETE CASCADE` one row at a time (~7x the cost of the raw deletes), and in the default `journal_mode=delete` that write transaction takes an EXCLUSIVE lock, so it blocked *readers* too. Three plausible causes were measured and ruled out: the FTS5 search table (25 ms), the missing FK indexes (no measurable effect), and pre-purging the large derived tables (0–12 %); neither `defer_foreign_keys` nor reordering helps, so there is no SQL-level fix. Deletion is now two-stage — the request flips `tree.deleted_at` and returns (8360 ms → **6.2 ms**), a background worker does the cascade. `deleted_at IS NOT NULL` *is* the queue, so a purge interrupted by a crash resumes at the next start; no job table. SQLite switched to **WAL**, which is what actually fixes the freeze (readers no longer wait behind a writer) and is ~22 % faster besides. A router-level guard rejects requests scoped to a deleted or unknown tree, closing the window where a deleted tree's children stayed readable.
- **`person_ancestry` dropped for a recursive CTE.** The closure table held 364k rows and, with its four indexes, **62 % of the database**, to encode 15 704 parent-child edges — while being ~12x *slower* to read than a recursive CTE over the family links (160 ms against 13 ms for a depth-10 pedigree), and needing a rebuild on every re-parenting. `AncestryRepo` now walks `family_child` ⋈ `family_spouse` directly. Validated against the real table on the 200 deepest pedigrees of a 15k-person database: identical ancestor sets at identical depths, no exceptions. Traversal is bounded at 64 generations since the schema does not prevent cycles. Migrations now `VACUUM` when they free real space, which is what returns the reclaimed pages to the filesystem: a real database went **238 MB → 91 MB**. See [Data Model](data-model.md) (Ancestry traversal).
- **GeneWeb `.gw` import.** OxidGene now reads the textual interchange format of [GeneWeb](https://geneweb.tuxfamily.org) (what `gwu` writes), including the `gwplus` extension, via the [`geneweb`](https://github.com/trois-six/rust-geneweb) crate. That crate converts a `.gw` file into the same `ged_io` model the GEDCOM importer already consumes, so the whole domain mapping is shared: `import_gedcom` was split into a parse step and a reusable `import_gedcom_data(&GedcomData)`, and `oxidgene_gedcom::geneweb::import_geneweb` feeds the latter. Reading is lenient — a malformed block is skipped and reported as a warning rather than failing the file. Transport carries raw bytes end to end (`POST /trees/{id}/geneweb/import` takes `application/octet-stream`; the GraphQL `importGeneweb` mutation takes base64) because `.gw` is ISO-8859-1 unless a file opts into UTF-8 mid-stream — decoding it early would mangle accented names. Import is one-way: OxidGene reads `.gw`, it does not write it. Verified against a real 10K-person GeneWeb base (10,254 persons / 2,507 families / 23,201 events, 7 warnings). The import summary type is now format-neutral across both surfaces (`ImportGedcomResponse` → `ImportResponse`, `ImportGedcomResult` → `ImportResult`). See [API Contract](api.md) §3.

**Sprint E.9 (Jul 2026):**
- **The cache layer is gone.** `oxidgene-cache` (~4,100 lines, three storage backends: Redis, DashMap, disk) was deleted and replaced by denormalization in the database: person projections are materialized in a new `person_denorm` table and refreshed on every mutation, and pedigrees are assembled per request from `person_ancestry` ⋈ `person_denorm`. Consequences: no stale reads (the mutation and its projection refresh share one transaction and commit together — a rollback undoes both), no cold start (projections are durable across restarts), one code path for desktop and web, no Redis to deploy, and no disk snapshot to flush when the desktop app closes. The projection types moved to `oxidgene_core::projection`, so `oxidgene-ui` no longer pulls the database layer into the WASM build. REST routes renamed off `/cache/*` → `/profiles*` and `/pedigree/*`, with GraphQL renamed in step so the two surfaces stay symmetric (`cachedPerson` → `personProfile`, `rebuildTreeCache` → `rebuildTreeProfiles`, `invalidateTreeCache` → `dropTreeProfiles`, `GqlCached*` → `GqlPersonProfile`/`GqlProfile*`). See [`read-projections.md`](read-projections.md).

**Sprint E.7 — earlier in Jul 2026:**
- Dictionary page launched ([`ui-dictionary.md`](ui-dictionary.md)): read-only V1 index of family names, sources, places, occupations with usage counts (person/citation/reference drill-down via aggregation endpoints). Search results grid view also shipped: each result is a card embedding a pannable mini-pedigree (self + parents + grandparents, server-side), 20 per page vs 25 list mode. See [`ui-search-results.md` §7](ui-search-results.md).
- SOSA number search: numeric-only family-name queries resolve as SOSA numbers with direct tree navigation (e.g. `search("2")` → `GET /persons/sosa/2`).
- GEDCOM round-trip fidelity: `ADOP` (adoption) now recognized as individual event with nested adoptive-family `FAMC`; 12 new individual-attribute `EventType` variants (education, property, religion, SSN, etc.) map to native GEDCOM tags instead of generic `EVEN`. Event witnesses (`ASSO`/`RELA`) moved from free-text to proper `event_witness` join table (real `Person` references + optional relation text). Exports declare `CHAR UTF-8` in header. Imports capture both Gramps encodings (`ASSO` nested in event AND top-level) and deduplicate.
- DB migrations reconsolidated: all schema in `m20250101_000001_initial.rs`, future changes add new files (no squashing). **Note:** ged_io upstream `main` pinned (breaking change in `Individual.name` → `names`); see [Architecture](architecture.md) §1.
- Multi-profession `OCCU` handling: import now splits a Geneanet-style multi-profession `OCCU` value on `,` (each part trimmed) into one case-normalized `Occupation` event per profession; export gained an opt-in `merge_occupations` option to re-concatenate them into a single `OCCU` tag for Geneanet compatibility. See [API Contract](api.md) §3, [Settings](ui-settings.md) §18.
- Multi-alias `SURN` handling: import now trusts the `NAME` line's surname (not `SURN`) for the primary `PersonName`, and splits a Geneanet-style multi-alias `SURN` value (e.g. `"LE NADEN,NADAM"`) on `,` into one `AlsoKnownAs` `PersonName` per alias instead of dropping the primary surname or importing the raw concatenation; export gained a matching opt-in `merge_names` option to re-concatenate non-primary names back into the primary `NAME`'s `SURN` tag for Geneanet compatibility. See [API Contract](api.md) §3, [Settings](ui-settings.md) §18.
- Compound-name display fix: `SearchEntry`/`CachedFamilyMember` now carry original-cased `surname`/`given_names` fields end-to-end (DB → cache → GraphQL/REST → UI) instead of the UI guessing the split by breaking `display_name` on the first/last space — which mangled compound surnames like Breton "LE NADAN" (e.g. showing surname "NADAN" with "LE" stuck onto the given names). `person_search_fts` gained `surname_display`/`given_names_display` columns (migration `m20260724_000001_search_display_names`, table is rebuilt on demand so no data migration needed). See [API Contract](api.md) §2.
- Reference content (occupation sheets, given-name meanings): new `oxidgene-api::reference` module resolves free-text GEDCOM occupation/given-name values to short fiches, one JSON file per language per data type (gzip-compressed at build time via `build.rs`, decompressed once into an in-memory table). Served read-only at `GET /api/v1/reference/{lang}/occupations?term=...` and `GET /api/v1/reference/{lang}/given-names?term=...` (404 when no fiche exists yet); HTTP responses gzip-compressed via `tower-http::CompressionLayer`. The person profile page shows a hover tooltip (`ReferenceHover`/`ReferenceBubble`, `components/reference_tooltip.rs`) over the occupation and the given name in the header. Seeded with 5 occupations + 5 given names (fr/en) — content data set still needs growing. See [API Contract](api.md) §9.

**Sprint E.6 (desktop cache simplification) — earlier in Jul:**
- Search moved to DB-native `person_search_fts` (SQLite FTS5 on desktop, plain indexed table on PostgreSQL). `GET /cache/search` removed in favour of `GET /persons/search?q=...`. This was the proof of concept for E.9.
- `PersonCache` removed from `MemoryCacheStore`: desktop built persons on demand with targeted queries (~1–9 ms per person). *(The whole store is gone as of E.9.)*
- Benchmarks (20K-person release tree): person load ~9 ms, search ~10 ms, full rebuild ~0.7 s.

**Earlier (Jun 2026):**
- Person edit modal fully implemented (date qualifiers, create mode, couple modal, staged child detach, keyboard shortcuts).
- Desktop binary size: 560 MB (debug) → 13.5 MB (release, via LTO + feature flags).

**Deferred:**
- E.7 media management (binary upload/download endpoints) — not yet implemented.
- E.8 dictionary descent view (nested genealogical list for family names, with SOSA badges) — planned but not started.
- Performance testing with 100K-person trees.

---

## 9. Respect of norms and standards

The project must respect the norms and standards:

- GEDCOM 5.5 and 7.0
- XDG base directories for cache, config...
- REST and GraphQL
- OpenAPI
- OAuth 2.0 / OpenID Connect (eventually SAML if we decide to use it)
