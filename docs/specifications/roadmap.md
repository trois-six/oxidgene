---
type: "Roadmap Specification"
title: "Roadmap — EPICs, Sprints & Milestones"
description: "Delivery roadmap with EPICs, sprint milestones, and completion status for OxidGene."
tags: [oxidgene, specification, roadmap, planning]
timestamp: 2026-07-19T00:00:00Z
---


# Roadmap — EPICs, Sprints & Milestones

> Part of the [OxidGene Specifications](index.md).
> See also: [Architecture](architecture.md) · [General](general.md) (MVP scope)

---

## Recently shipped

Moved here from [General §8b](general.md) — the sprints below say what is
planned, and this says what actually landed. Newest first.

**Shipped 2026-08-21 — a stored projection says which build wrote it:**
- **A projection change used to ship invisible.** Every field added to `PersonProfile` carries `#[serde(default)]` so the rows already in `person_denorm` keep deserializing — right for compatibility, and precisely wrong for visibility: an old payload comes back *looking complete*, and nothing anywhere could tell "this person genuinely has no date qualifier" from "this row predates qualifiers". That is not a hypothetical. The date-qualifier work the day before shipped a feature that did not appear on any existing install; the cards went on drawing bare years and the only cure was knowing to re-import. Whoever added the next projection field would have walked into the same trap.
- **`person_denorm.schema_version`, compared on every read.** `PROJECTION_SCHEMA_VERSION` stamps every write, and `get` / `get_many` / `count_current` filter on it, so a row from an older build reads as **absent**. That is the whole trick: the callers that already rebuild a projection they could not find rebuild a stale one too, with no second code path and no way to forget one. `ensure_materialized` asks `count_current` rather than `count_tree`, so a tree an older build wrote is rebuilt once, on first read.
- **A column, not a field in the payload.** The version is metadata *about* the row and has to be queryable in one indexable comparison, the same statement on SQLite and PostgreSQL; inside the JSON it would need each backend's own JSON functions, and counting stale rows would mean decoding every payload in the tree to answer a question asked on every read path.
- **Two places where the obvious thing is wrong.** `list_tree` deliberately does *not* filter — it answers "who is in this tree", and dropping the stale rows would return a short list; its one caller checks `count_current` first, so nothing stale reaches it. And `upsert`'s `ON CONFLICT` must update `schema_version` alongside the payload: overwriting a stale row while leaving its old version behind would rebuild it forever, once per read.
- **The migration backfills nothing.** Existing rows default to 0, which is exactly right — they *are* stale, and the lazy rebuild picks them up. Rebuilding inside the migration would re-derive every projection in the database up front, need the whole builder there, and redo work the first read does anyway.
- [ ] Verified by `cargo fmt`, `clippy --workspace --all-targets` (clean) and 663 tests, one new: it ages a built tree's rows to version 0, doctors one payload so serving it would be unmistakable, and asserts the read path returns the *rebuilt* name rather than the doctored one. Confirmed to fail with the version filter removed, so it is testing the mechanism and not the happy path. **Not yet exercised by hand in the running app.**

**Shipped 2026-08-20 — an approximate date stops being drawn as a fact:**
- **A pedigree card said `1849` whether the register said 1849 or "about 1849".** `date_qualifier` has been on `Event` since the initial schema and GEDCOM import has filled it correctly all along — `ABT 1849` parses to `About` — but every surface that shows a *year alone* went through `Event::year()`, which returns an `i32` and cannot carry "about". So the one place a hedge matters most, the card with room for four digits and nothing else, was the one place it was guaranteed to be dropped. Cards now read `ca 1849-< 1917`, `< 1907-`, `> 1912-`, using **GeneWeb's own symbols** (`prec_text`, `lib/dateDisplay.ml`) rather than invented ones — that is what Geneanet draws, so a user arriving from a Geneanet tree reads them already. `QualifiedYear` in `oxidgene-core` is the year and its precision travelling together, which is what stops the pair being separated at the last step.
- **The projection was the real leak.** Every pedigree render goes through `PedigreeData::from_pedigree`, which rebuilds synthetic events out of `PedigreeNode` — and that node held a year *string* and no qualifier, so `from_pedigree` hardcoded `DateQualifier::default()`. A card fix alone would have been dead code. `ProfileEvent`, `PedigreeNode` and `PedigreeFamilyMember` carry the qualifier now, `#[serde(default)]` so the payloads already sitting in `person_denorm` still deserialize and read as `Exact`, which is what they meant when they were written. The qualifier stays *beside* the year rather than folded into the string: the year is parsed back to an integer for sorting and for the search grid, and a `"ca 1849"` in that field would break both.
- **The events panel was fixed by the same change and needed no code.** It already rendered qualifiers in full through `format_date` (« vers 1849 », « avant 3 janv. 1900 ») — it was simply being handed `Exact` for everything the projection produced.
- **`CAL`, `EST` and `FromAge` all read as `ca`.** GeneWeb has no symbol for them; each is an approximation reached by a different route, and once the arithmetic is done a *card* wants the same warning from all three. The distinction survives on the event, where the edit modal and the events panel still name it in full.
- **The tooltip could not be written in rsx.** A native SVG tooltip is a `<title>` child, and dioxus-html defines `title` as the HTML element with its SVG twin commented out — an HTML-namespaced `<title>` inside an `<svg>` is inert. It is injected as markup instead, which the browser parses in the SVG namespace; verified in Chrome (`namespaceURI` comes back as the SVG one). The strings are ours, but the marks are literally `<` and `>`, so they are escaped rather than trusted — unescaped, `< 1917` would be swallowed as a bogus tag and the card would silently lose its death year.
- **The date line is squeezed, never truncated.** A bare `1849-1917` always fitted, so it was the one card text never measured; the marks make it longer, which overruns a compact card's 72px. `textLength`/`lengthAdjust` compress it — dropping characters off a date would change what it says.
- **The projection stopped extracting years and started carrying events.** Fixing the qualifier exposed the same loss twice more: a birth on 2 Nov 1788 reached the events panel as "1788", and a death recorded as "between 11 Nov 1691 and 20 Aug 1693" as "between 1691" — a qualifier promising a second date the projection could not hold. `PedigreeNode` and `PedigreeFamilyMember` held a *year string* plus a place string, and everything that did not fit those two was gone before the frontend saw it. They carry `birth` / `death` as whole `ProfileEvent`s now (which gained `date_value2` and `calendar`), so `from_pedigree` is a conversion that can lose nothing instead of a reconstruction that always did. `birth_year` / `birth_qualifier` / `birth_place` are gone, replaced by one field that subsumes all three; `place_id` survives the trip too, where it used to be forced to `None` with the place name smuggled through `description`. Marriage dates had always displayed correctly, which is exactly the tell — they came from `family_events` as real `ProfileEvent`s.
- **Ranges show both of their years** — `1691..1693`, `1691|1693` — because the range is the fact. Two of them run to 105.8px, past even the full card's 105px column, so the wide form is used when it fits and the narrow `.. 1691` when it does not, rather than compressing to illegibility; the narrow form keeps the mark, so the card understates rather than misleads, and the tooltip spells the whole thing out. The side panel always uses the wide form, being HTML that wraps.
- **A card is dated from the baptism when the birth has none**, and from the burial when the death has none, as GeneWeb does. The subtlety is that the fallback triggers on a missing **date**, not a missing event: a parish tree is full of empty birth stubs created to hang a source on, and testing `birth.is_none()` keeps the stub and draws a blank year while a perfectly good "vers 1620" sits unused on the baptism. What is deliberately *not* copied is GeneWeb's single `approx` flag covering both ends of a life — that is how Geneanet ends up showing `ca 1691` for a death actually recorded as a range, and it conflates two different facts. Each event keeps its own precision.
- **The side panel's header shows the card's lifespan** instead of `n. 1620` / `d. 1691`. It sits beside the card showing that same person, and two spellings of one life read as two different facts. The events below are unchanged.
- **« Né(e) le » with nothing after it.** The person page emitted a birth clause for an event carrying neither date nor place. It is now omitted entirely, and a dateless birth reads « Né » alone — the treatment « Décédé » already had. The participle also agrees: `Sex::Male` / `Sex::Female` pick their own key and `Né(e)` is kept for `Sex::Unknown`, which is precisely what that value records. English has no agreement, so its three keys carry one string and this costs nothing there.
- Also on the dictionary usage lists and the person page's family narrative, both of which draw the same lifespan; the dictionary's years come from their own SQL path, which was already loading the event rows and simply discarding the column.
- **Breaking:** `GqlPedigreeNode` exposes `birth` / `death` (`GqlProfileEvent`) in place of `birthYear` / `birthPlace` / `deathYear` / `deathPlace`, mirroring REST. And because `person_denorm` is a stored projection, **existing rows had to be rebuilt** — a re-import or `POST /trees/{id}/profiles/rebuild` — before any of this appeared. Nothing detected that staleness; closed the next day by the entry above.
- [ ] Verified by `cargo fmt`, `clippy --workspace --all-targets` (clean) and 662 tests, 18 of them new — four through a real SQLite database asserting a `ProfileService`-built node keeps `About`/`Before`, keeps whole dates and both ends of a range, prefers a dated baptism over an empty birth stub, and mirrors all of it on death/burial. The card widths and the SVG-namespace tooltip were measured in Chrome. **Not yet exercised by hand in the running app.**

**Shipped 2026-08-19 — a `.gdz` can be imported, not only written:**
- **The one import format that arrives with its photographs.** OxidGene has written GEDZIP since F.1 and packed the media into it since 2026-08-18, but could not read one back: a user handed a `.gdz` — by us, or by any GEDCOM 7.0 program — had to unzip it, import the `gedcom.ged`, and then upload every scan by hand against the record naming it. `POST /trees/{id}/gedzip/import` (GraphQL `importGedzip`) reads the archive whole. The genealogy half is the existing importer unchanged — `import_gedzip` parses `gedcom.ged` and hands it to the same `import_gedcom_data` the `.ged` and `.gw` paths use — and what the format adds is that each medium whose `FILE` names an entry in the archive is ingested through the ordinary upload path, so it lands stored, sniffed, thumbnailed, measured and croppable, exactly as if it had been uploaded. `file_path` stops being the producer's path for those and carries the name our own export writes back, which is what an uploaded file gets.
- **Nothing in the archive can fail the import.** A `FILE` naming an entry the ZIP does not hold leaves an unheld record — precisely what a plain `.ged` produces — and says so in `warnings`; so does a file no `OBJE` names, which is usually how somebody learns their exporter dropped the links. A file the store refuses (an unsupported type, one over the upload ceiling) costs that one medium its bytes and nothing else. Failing ten thousand people over a stray file would be the wrong trade.
- **Matching folds separators and case**, because the `FILE` value comes from whoever wrote the archive: a Windows producer's `.\Media\Portrait.JPG` finds `media/portrait.jpg`. A `FILE` that is a URL is left alone rather than hunted for — GEDCOM 7 allows one there and we deliberately never fetch it.
- **Sized for what it is.** 512 MiB body limit against the 350 MiB the text formats get: this is the one import whose size tracks how much media somebody owns rather than how many people, and the whole archive has to be in memory before a ZIP central directory can be read. Ingestion runs at the machine's parallelism, capped, sharing the Geneanet importer's width for the same reason — each decode in flight holds a full-size image.
- **The file tab takes all three.** Extension picks the reader (`.gw` → GeneWeb, `.gdz` → GEDZIP, anything else → GEDCOM, since a renamed `.ged` is common and its reader says so soon enough), and the drop zone names the formats including what only `.gdz` brings.
- **What a round trip still loses**, recorded rather than implied: a multi-page document's pages export as separate `OBJE` records, so re-importing the archive gives back that many separate media instead of one document with its pages, and the original file name goes with the archive path (`media/{uuid}.{ext}` — keyed on the id because two scans called `photo.jpg` in one tree is routine). GEDCOM has no tag for either fact.
- [ ] Verified by `cargo fmt`, `clippy --workspace --all-targets` (clean) and the full suite (633 passing), six tests new round-trips through `export_gedzip` → `import_gedzip`. **Not yet exercised by hand in the running app**, and not yet run against a `.gdz` written by another program.

**Shipped 2026-08-19 — a couple and a document can be marked private:**
- **`privacy` on `family` and `media`.** `person.privacy` has existed since the initial schema and the other two never had one, so there was no way to say "this union is private" or "do not publish this scan" — and a tree published with a living couple's marriage, or a photograph of living children, could withhold neither. Same column shape and default as the person's, so one enum, one picker and one set of translations serve all three: a segmented control in the couple modal, a select in the media panel, beside the one the person form already had.
- **The tree says what `default` means.** Every record defaulted to `Privacy::Default`, documented as "follows the tree-level privacy settings" — and there were none, so the commonest value in the model pointed at something that did not exist. `tree.default_privacy` is that setting, offered as two buttons in Tree & Roots. It is deliberately *not* a `Privacy`: that enum's own `Default` variant would make a tree follow itself, so `TreeDefaultPrivacy` has two variants and the circular state cannot be written down. It defaults to **private** — a genealogy holds living people, and a tree nobody has classified has not been cleared for publication.
- **Deliberately stubbed, and it says so.** Nothing enforces any of it: privacy is meaningful only against a viewer, and there are no viewers until authentication lands. Every picker carries the same line — *recorded now, nothing is hidden yet* — rather than letting a control imply a protection that does not exist. What it buys is that the intent is stored, so classifying a tree today survives into the release that enforces it, and that work becomes a read-path change rather than a schema change plus a data-entry campaign.

**Shipped 2026-08-19 — a portrait can be a face in a group photograph:**
- **"Which image represents this person" moved onto the person.** It lived on `media_link.is_profile`, which could only ever name a whole media file — but a person is very often identified *inside* a larger photograph, and that region was already a first-class row here: a `vignette`, stored as coordinates on the one scan. There was no way to say "her portrait is that face", which is the portrait most people in an old family archive actually have. The alternative, a second `is_profile` on `vignette`, spreads the invariant "at most one portrait per person" across two tables where it can no longer be established in a single statement — the very thing the original single-statement design existed to prevent. `person.portrait_media_id` / `portrait_vignette_id`, written through one `Portrait` value, make it structural instead: one row, and "media or vignette, never both" is a check on that row. `PUT /persons/{id}/portrait` and `GET /portraits` replace `PUT /media-links/{id}/profile`, with `setPersonPortrait` beside them on GraphQL.
- **A third bug in the same place.** `person_denorm.primary_media` never consulted `is_profile` at all — it took whichever media had the lowest `sort_order`. A person could star a photograph and have their pedigree card go on drawing a different one, and no amount of rebuilding would fix it. It reads the pointer now, resolving a crop through the scan it sits on; `TreeData` carries the portrait vignettes only, since every vignette in a tree is a large slice to hold for a field usually null.
- **Crops appear as pictures.** A vignette existed only inside the scan it was drawn on: you could crop your grandmother out of a wedding party, attribute the crop to her, and find nothing on her profile to show for it. A person's gallery now lists crops beside the whole photographs, cropped by the server on read — one `<img>`, no second copy. They read as *regions*, dashed and badged, because otherwise a crop and its source look like two pictures of the same scene; opening one opens the scan it is part of, since the point of a region is the document behind it. The tile carries the portrait action and nothing else — moving the rectangle belongs to the cropper, over the full scan, where the coordinates mean something.
- **Every card with a real photograph had been showing a broken image**, and setting a portrait did not refresh the views drawing it. Both fixed; see the entry below for the first, and `MediaGallery` now reports changes upward so a profile avatar and a pedigree card stop waiting for a navigation.
- **The viewer says what a document is.** A facts column beside the image — date, place, kind of record, physical medium, description, note, who is identified — where before only the description appeared, in the footer. Empty fields show an em-dash rather than being omitted: hiding them made a scan with no date look identical to a viewer that could not record one, so the feature read as missing rather than as unfilled. Editing swaps the column for the existing edit panel rather than a second copy of the form, and is not gated on `read_only`: that flag governs restructuring a person's gallery, while recording when a scan was taken is describing the scan, and the moment a reader knows it is while looking at it. There is deliberately no source field — a media *is* a source document.
- **Right-click sets the portrait**, from the gallery and from a crop. It is the one edit a reader makes while *looking* at somebody's photographs, and it was previously reachable only from the edit modal — so on a profile page, where the gallery is read-only, it could not be done without leaving the page that prompted it. Offered only where a portrait means something: a couple's card shows its spouses', and an event has none.
- [ ] Verified by `cargo fmt`, `clippy --workspace --all-targets` (clean) and 628 tests. **Not yet exercised by hand in the running app.**

**Shipped 2026-08-18 — media say what they are, and exports carry them:**
- **A `.gdz` now contains its media.** It never could: `export_gedzip` took a `&str` and wrote `gedcom.ged` and nothing else, so there was no code path by which a photograph could reach the archive. A `.gdz` was a `.ged` in a costume, which is the one thing the format exists not to be. Files are now written under `media/{uuid}.{ext}` — keyed on the id rather than the file name, because two scans called `photo.jpg` in one tree is routine and an archive cannot hold both — and each `OBJE.FILE` points at the entry that ships with it. A medium whose bytes have gone missing from the store is logged and skipped rather than failing the export. Plain `.ged` is untouched and pinned by a test: there the `FILE` keeps the producer's own path, which is what makes a round trip lossless.
- **What a medium *is* is recorded twice, because it is two questions.** GEDCOM's `SOURCE_MEDIA_TYPE` (`OBJE.FILE.FORM.TYPE` in 5.5.1, `FORM.MEDI` in 7.0) enumerates the physical carrier — `PHOTO`, `MANUSCRIPT`, `TOMBSTONE`, `FICHE`, `FILM` — and OxidGene imports and exports it. But that vocabulary describes the carrier, not the record: a census return, a marriage contract and a conscription register are all `MANUSCRIPT` to it, and to a genealogist they are three different things — the distinction Geneanet's own media types draw. So `document_category` holds the richer answer, is nullable because an uploaded photograph needs no classification, and knows the medium it implies, so classifying a scan as a census return still exports `MANUSCRIPT` rather than `OTHER`. Where the user answered both questions, both answers are kept.
- **The viewer is one you can actually read a document in.** Zoom runs from 25 % to 800 % — the reason to magnify a parish register is one word of secretary hand in a corner, which 200 % does not reach — and the stage scrolls from the top-left rather than centring once the image outgrows it. The pager stopped drawing a button per page: a 144-page dossier produced a strip longer than the image above it, so it now keeps both ends and a window around where the reader is, eliding the rest — except where a gap would hide exactly one page, since an ellipsis costs the same width as the number and takes away a destination. A document downloads **whole, as a ZIP**, its entries numbered `001_`, `002_` so unzipping restores the reading order rather than the page names' alphabetical one. Downloads are named so the file opens: a Geneanet deposit is *titled* ("Mariage de Pierre"), not named, and saving that verbatim produced a file the operating system could not open — the MIME type now supplies the extension where the name carries none. Tiles grew from 112 px to 150 px, still inside the 400 px thumbnail the server already generates.
- **Every card with a real photograph was showing a broken image.** The portrait map was built from `file_path`, which is the *producer's* path — the `OBJE.FILE` a GEDCOM carried, or the address a Geneanet deposit was served under, kept verbatim so exports round-trip. It was never a URL this application can load. Hence the inverted symptom: people with **no** photograph fell through to the embedded silhouette and rendered correctly, while everyone with one got an `<img>` pointing at `geneanet.org` or `C:\Photos`. Portraits now resolve to our own thumbnail, or to a genuine `http(s)` URL for remote media we never fetched, or to nothing — so a record naming a file nobody uploaded draws the silhouette instead of asking the browser for a 404. A second bug fell out of the same code: the map was collected without reading `is_profile`, so whichever row the database returned last won and a person could star a photograph and still see a different one.

**Updated 2026-08-23 — `ged_io` 0.16.3 absorbs the temporary compatibility work:**
- French Republican dates now convert directly to their documented Gregorian day; OxidGene no longer measures and applies a local day shift.
- Record-level `OBJE` links retain their pointers during parsing, so GEDZIP and plain GEDCOM imports no longer scan source text to recover person and family media links.
- The writer now emits `OBJE.FILE.FORM.TYPE`; the export path sets `Format::source_media_type` directly and no longer rewrites serialized GEDCOM.

**Shipped 2026-08-18 — the Geneanet import runs end to end:**
- **Documents come in whole.** Geneanet attaches person links to *pages*, and someone who scans a dossier attaches its cover — so importing only linked pages imported a cover and discarded the document. On the reference account that was **235 of 623 views**, every one an interior page of one of eight dossiers, one running to 144 pages. A deposit with any linked page now becomes a `media.is_document` with every page beneath it, ordered by Geneanet's own page number, and the people its pages named are linked to the document. The order a page is *fetched* in never reaches the reader: pages are sorted before any is written, `append_page` indexes them in that order, and the gallery reads back by that index.
- **Media are recognised, not re-downloaded.** A single-page deposit states its byte length, so it is matched against the data archives exactly. A page of a document states nothing — its deposit downloads as a ZIP streamed with no `Content-Length`, and there is no per-page original URL — so a small `medium` rendition is fetched and **perceptually hashed** against the archives. Measured on a real 623-entry archive: **601 matched, 22 declined, 0 wrong — 96.5 % resolved without downloading**. A 64-bit hash was tried first and would have shipped broken (12 entries had an exact twin, 109 of 228 had one within distance 4); at 256 bits, none do. A clash is *detected* — byte-identical duplicates are interchangeable, anything else falls through to a download.
- **`wreq` was adopted and then removed.** A browser-impersonating transport passed Cloudflare, but only while its pinned Chrome profile stayed current — a treadmill with a silent failure mode — and it dragged BoringSSL, a `bindgen` toolchain and a vendored, patched `tungstenite` through the workspace to fix a linker clash it had caused. Every request now goes through the login window instead, which needs no emulation because it *is* a browser. **27 crates removed.**
- **The CLI is gone.** Half its commands needed direct HTTP, which no longer works at all; the rest were superseded by the window. With it went `client.rs`, `media.rs` and the console-paste script — and `reqwest` leaves `oxidgene-geneanet` entirely, so the **server** no longer links an HTTP client.
- **Sessions are saveable.** Step 3 is the only part that talks to Geneanet, and measuring the deposits costs one `HEAD` each — several hundred per run. What it collects now saves to a file and loads back, so the import can be re-run, or run on another machine, without asking Geneanet anything. The file *is* the collection JSON with the sizes added, so it feeds the existing reader unchanged.
- **The window stopped hiding itself.** Hiding broke cancellation outright — a hidden window can never emit a close request — so it shrinks to a status panel instead, carries no counters (the modal owns those), and runs in an **incognito context** so the months-long remember-me token dies with it. The modal refuses to be dismissed while an import is in flight, which previously threw away four settled steps on a stray click.
- [ ] Verified by `cargo fmt`, `clippy --workspace --all-targets` (clean) and 577 tests, plus two measurements against a real archive. **Steps 1–4 exercised against a live account; step 5 not yet run to completion.**

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

---

## EPIC A — Technical Foundation ✅

### Sprint A.1 — Project Scaffolding ✅

- [x] Initialize Cargo workspace with all crate stubs.
- [x] Configure workspace-level dependencies in root `Cargo.toml`.
- [x] Create `oxidgene-core` crate with all domain types and enums.
- [x] Set up `justfile` with basic commands (build, test, fmt, clippy).
- [x] Create `.gitignore`, `README.md`.
- [x] Set up GitHub Actions CI pipeline (build + test + clippy + fmt).

### Sprint A.2 — Database Schema & Migrations ✅

- [x] Define SeaORM entities for all 13 tables in `oxidgene-db`. → see [Data Model](data-model.md)
- [x] Write database migrations (create tables, indexes, foreign keys).
- [x] Implement migration runner (up/down).
- [x] Test migrations against both PostgreSQL and SQLite.

### Sprint A.3 — Repository Layer ✅

- [x] Implement repository traits in `oxidgene-db` for CRUD operations.
- [x] Implement soft-delete filtering.
- [x] Implement cursor-based pagination helpers. → see [API Contract](api.md) (pagination)
- [x] Unit tests for all repositories.

### Sprint A.4 — REST API Skeleton ✅

- [x] Set up Axum router in `oxidgene-api`. → see [API Contract](api.md) (REST)
- [x] Implement REST handlers for Trees (full CRUD).
- [x] Implement REST handlers for Persons (full CRUD + names).
- [x] Implement REST handlers for Families (full CRUD + spouses + children).
- [x] JSON serialization, error handling, validation.
- [x] Integration tests against a test database.

### Sprint A.5 — REST API (continued) ✅

- [x] Implement REST handlers for Events, Places, Sources, Citations.
- [x] Implement REST handlers for Media (upload/download), MediaLinks, Notes.
- [x] Implement ancestor/descendant endpoints (closure table; replaced by a recursive CTE in Aug 2026).
- [x] Complete integration test coverage.

### Sprint A.6 — GraphQL API ✅

- [x] Set up async-graphql schema in `oxidgene-api`. → see [API Contract](api.md) (GraphQL)
- [x] Implement all queries with connection types (cursor pagination).
- [x] Implement all mutations.
- [x] GraphQL playground / introspection endpoint.
- [x] Integration tests for GraphQL.

### Sprint A.7 — Web Server Binary ✅

- [x] Create `oxidgene-server` binary. → see [Architecture](architecture.md) (deployment)
- [x] Configuration loading (environment variables, config file).
- [x] Database connection pool setup (PostgreSQL).
- [x] Health check endpoint (`/healthz`).
- [x] Graceful shutdown.
- [x] Docker build for server + Docker Compose for local dev (server + PostgreSQL).

### Sprint A.8 — Desktop Binary Skeleton ✅

- [x] Create `oxidgene-desktop` binary. → see [Architecture](architecture.md) (desktop)
- [x] Embed Axum server on localhost with SQLite.
- [x] Open Dioxus WebView pointing to the local server.
- [x] Verify all API endpoints work with SQLite backend.

---

## EPIC B — GEDCOM Engine ✅

- [x] Implement `oxidgene-gedcom` crate wrapping `ged_io`.
- [x] GEDCOM → domain model import (persons, families, events, sources, media, places, notes).
- [x] Domain model → GEDCOM export.
- [x] Round-trip tests (import → export → import, verify equivalence).
- [x] Error and warning collection during import.
- [x] Streaming import for large files.
- [x] Wire up import/export REST and GraphQL endpoints. → see [API Contract](api.md) (GEDCOM)

### GeneWeb `.gw` import ✅ (Aug 2026)

> Rationale: GeneWeb is the genealogy software behind a large share of French online
> genealogies, and its `.gw` export carries more than the GEDCOM its own `gwb2ged` produces.
> The [`geneweb`](https://github.com/trois-six/rust-geneweb) crate converts `.gw` into the same
> `ged_io` model this EPIC already consumes, so importing the format costs a reader, not a
> second domain mapping.

- [x] Add the `geneweb` crate as a workspace dependency.
- [x] Split `import_gedcom` into a parse step and a reusable
  `import_gedcom_data(&GedcomData, tree_id)`, so both formats share one domain mapping.
- [x] Add `oxidgene_gedcom::geneweb::import_geneweb(bytes, origin_file, tree_id)` — lenient
  reading, `.gw` parse errors surfaced as import warnings rather than aborting the file.
- [x] Carry raw bytes end to end: `POST /trees/{id}/geneweb/import` takes
  `application/octet-stream` + `?filename=`, GraphQL `importGeneweb` takes base64. `.gw` is
  ISO-8859-1 unless a file opts into UTF-8 mid-stream, so decoding it upstream of the reader
  would mangle accented names.
- [x] Extract `persist_import_result` in the service layer so every import format shares one
  FK-ordered, transactional persistence path; rename the summary type to be format-neutral
  (`ImportGedcomResponse` → `ImportResponse`, `GqlImportGedcomResult` → `GqlImportResult`).
- [x] UI: one Import button, file dialog accepting `.ged` and `.gw`, dispatch by extension.
- [x] Tests: REST (incl. an ISO-8859-1 fidelity regression test), GraphQL, and unit coverage.
  Verified against a real 10K-person base (10,254 persons / 2,507 families / 23,201 events).

**Not covered** (the `geneweb` crate reads only): writing `.gw`, the binary `.gwb` database,
and GeneWeb-only concepts with no GEDCOM counterpart (per-person access rights, wizard notes,
wiki pages) — they survive conversion as `_GW…` tags, which the GEDCOM importer does not model.

---

## EPIC C — Tree Editing (Frontend) ✅

- [x] Set up `oxidgene-ui` crate with Dioxus. → see [Architecture](architecture.md) (frontend)
- [x] Implement frontend routing (tree list, tree detail, person detail).
- [x] Visual tree component (ancestor/descendant chart). → see [Tree View spec](ui-genealogy-tree.md)
- [x] Person detail sheet (names, events, sources, media, notes).
- [x] Inline editing of persons, families, events. → see [Person Edit Modal spec](ui-person-edit-modal.md)
- [x] Family creation and member linking UI.
- [x] GEDCOM import/export UI (file upload, download).
- [x] Frontend integration with REST/GraphQL API.

**Post-delivery (Aug 2026):**
- [x] Note bodies render as sanitized HTML (`ammonia` allowlist, applied on write in `oxidgene_db::html`) instead of escaped text, because the imported formats put markup in them.
- [x] Line breaks in note bodies canonicalized to `\n` on write and restored to `<br>` at display, so the same note reads identically whether it arrived as GEDCOM `CONT` lines, as GeneWeb `<br/>` + newline, or typed into the note textarea.

---

## EPIC D — UX, Languages, Performance ✅

- [x] Theme system (CSS-based, switchable at runtime). → see [Settings spec](ui-settings.md)
- [x] Implement at least 2 themes (default + one genealogy-platform-inspired theme). → see [Design Tokens](ui-design-tokens.md) §10
- [x] Internationalization (i18n) with runtime language switching. → `crates/oxidgene-ui/src/i18n/`
- [x] At least 2 languages (English + French). → `i18n/en.rs`, `i18n/fr.rs`
- [x] Client-side caching of API responses. → `ApiClient` in-memory cache, 30s TTL, invalidated on mutations.
- [x] Lazy loading of tree branches in the visualization. → Parallel JoinSet fetches for names & family members.
- [x] Performance optimization pass (bundle size, render performance). → Parallel fetches; cache avoids redundant round-trips.

**Post-delivery (Aug 2026):**
- [x] First-run defaults follow the OS: language picked from the ordered `navigator.languages` list (first translated entry wins), theme from `prefers-color-scheme`. English and light theme when detection fails. → `i18n::Language::from_preferences`, `layout::use_init_theme`

---

## EPIC E — Read Projections & Search ✅

> See [Read Projections specification](read-projections.md) for the full architecture.

### Sprint E.1 — Cache Foundation ✅

- [x] Create `oxidgene-cache` crate with `CacheStore` trait. *(removed in E.7 — see [Read Projections](read-projections.md))*
- [x] Implement cache type structs (`CachedPerson`, `CachedPedigree`, `CachedSearchIndex`, sub-types).
- [x] Implement `MemoryCacheStore` (DashMap-based, no persistence yet).
- [x] Implement `CacheBuilder` — build `CachedPerson` from DB data.
- [x] Implement `CacheService` with `rebuild_person`, `rebuild_tree_full`.
- [x] Unit tests for cache builder and service.

### Sprint E.2 — Person Cache & API Integration ✅

- [x] Add `CacheService` and `CacheStore` to `AppState`.
- [x] Implement `GET /cache/persons/{id}` and `GET /cache/persons?ids=...` REST endpoints. → see [API Contract](api.md) (Cache)
- [x] Implement `cachedPerson` and `cachedPersons` GraphQL queries. *(renamed `personProfile` / `personProfiles` in E.9)*
- [x] Hook all mutation handlers to trigger synchronous cache invalidation.
- [x] Update `person_detail.rs` to use cached endpoint.
- [x] Update `person_form.rs` and `union_form.rs` to use cached endpoint.

### Sprint E.3 — Pedigree Cache ✅

- [x] Implement pedigree cache builder from PersonAncestry + PersonCache (both since removed).
- [x] Implement `GET /cache/pedigree/{root_id}` and `PATCH .../expand` REST endpoints.
- [x] Implement `pedigree` query and `expandPedigree` mutation in GraphQL.
- [x] Implement LRU memory budget for pedigree caches (configurable per deployment).
- [x] Update `pedigree_chart.rs` to consume pedigree cache instead of snapshot.
- [x] Update `tree_detail.rs` page orchestration.

### Sprint E.4 — Search Index & GEDCOM Integration ✅

- [x] Implement `CachedSearchIndex` builder with accent-folding and normalization.
- [x] Implement `GET /cache/search?q=...` REST endpoint and `searchPersons` GraphQL query.
- [x] Hook GEDCOM import to trigger eager background cache build.
- [x] Update search components to use server-side search.
- [x] Remove `TreeSnapshot` endpoint and client-side `ResponseCache`.
- [x] Implement `POST /cache/rebuild` REST endpoint and `rebuildTreeCache` GraphQL mutation. *(renamed in E.9)*

### Sprint E.5 — Redis Backend & Desktop Persistence ✅

- [x] Implement `RedisCacheStore` (MessagePack serialization, `MGET` batch reads).
- [x] Add Redis container to Docker Compose for web deployment.
- [x] Implement disk persistence for `MemoryCacheStore` (bincode, serialize on exit, load on startup).
- [x] Auto-detect Redis (web) vs. memory (desktop) via configuration.
- [x] Cache staleness detection and recovery for desktop.

### Sprint E.6 — Desktop Cache Simplification (SQLite-native) ✅

> Rationale: the in-memory PersonCache and SearchIndex are redundant on desktop where SQLite is local.
> PedigreeCache stays (layout is parameter-dependent: root × depth × structure).

- [x] Replace `CachedSearchIndex` with a SQLite **FTS5 virtual table** (`person_search_fts`).
  - Add FTS5 migration (name tokens, birth year, death year; plain indexed table on PostgreSQL).
  - Populate on GEDCOM import and person/name mutations (bounded upserts via `PersonSearchRepo`).
  - Remove `GET /cache/search`. Handled by the normal search path: `GET /persons/search?q=...`.
- [x] Evaluate and remove `PersonCache` from `MemoryCacheStore` — removed; persons are built on
  demand with targeted SQLite queries (`caches_persons()` store flag; Redis keeps PersonCache).
  Disk persistence reduced to pedigrees only (cache schema v2).
- [x] Update the caching spec to document the SQLite-native path vs. Redis path.
- [x] Performance regression test: search and person-load times verified <= current with FTS5
  (`service_e6_test.rs`: person load < 100 ms asserted; measured ~1 ms at 2K persons).
- [x] Performance benchmarks on large GEDCOM-scale trees (`bench_large_tree_20k`, run with
  `cargo test -p oxidgene-cache -- --ignored`): 20K persons → person load ~9 ms, search ~10 ms,
  full rebuild ~0.7 s (release).

---

### Sprint E.7 — Refinement & Search Completion (✅ Jul 2026)

> Rationale: improve the UX to the definitive form. All items now completed.

**Completed:**
- [x] Reconsolidate DB migrations into initial migration — all schema in `m20250101_000001_initial.rs`; future changes add separate files (no squashing).
- [x] Search results grid view: one mini-pedigree card per result (self + parents + grandparents), 20 results/page.
- [x] Dictionary page (V1): read-only index of family names, sources, places, occupations with usage counts.
- [x] SOSA number search: numeric-only family-name queries resolve via `GET /persons/sosa/{number}`.
- [x] GEDCOM round-trip fidelity: EventType extended with 12 individual-attribute variants, ADOP as individual event, EventWitness join table, UTF-8 export.

**Deferred to Sprint F.1 (Media Management):**
- Media management (binary upload/download, thumbnails, multi-page docs, vignettes)

**Post-delivery (E.7 improvements):**
- [x] Sources smart drill-down: intelligent letter/prefix navigation (> 250 results → drill-down; <= 250 → display all), with server-side compression that auto-skips forced single-choice levels (see [ui-dictionary.md §8.10](ui-dictionary.md)) — a branch is only ever shown to the user when there is a genuine choice.
- [x] Multi-profession `OCCU` handling: import splits a Geneanet-style multi-profession `OCCU` value (`"Presales, Trainer"`) on `,` (each part trimmed) into one case-normalized `Occupation` event per profession; export gained an opt-in `merge_occupations` option to collapse them back into a single comma-separated `OCCU` tag for importers (Geneanet) that only support one profession field (see [API Contract](api.md) §3, [ui-settings.md](ui-settings.md) §18).
- [x] Multi-alias `SURN` handling: import previously trusted `SURN` over `NAME` for the primary surname, silently dropping the real primary name when a Geneanet-style multi-alias `SURN` value (`"LE NADEN,NADAM"`) was present. Fixed to prefer `NAME`'s surname as primary and split `SURN`'s extra parts (on `,`) into `AlsoKnownAs` `PersonName` rows; export gained a matching opt-in `merge_names` option to collapse non-primary names back into the primary `NAME`'s comma-separated `SURN` tag for importers (Geneanet) that only read the first `NAME` structure (see [API Contract](api.md) §3, [ui-settings.md](ui-settings.md) §18).
- [x] Reference content tooltips: new `oxidgene-api::reference` module + `GET /reference/{lang}/occupations|given-names?term=...` resolve free-text OCCU/GIVN values to short fiches (occupation sheets, given-name meanings), one gzip-compressed JSON file per language per data type, decompressed once into an in-memory table. Person profile page shows a hover tooltip over the occupation and the given name. Seeded with 5 occupations + 5 given names (fr/en); growing the data set is a follow-up content task (see [API Contract](api.md) §1 Reference Content).

**Future (lower priority):**
- Create a CLI tool for import/export
- Settings completion: manage locations, sources, occupations
- Statistics page (Post-MVP)
- Print layout (Post-MVP)

---

### Sprint E.8 — Dictionary V2: Genealogical Descent View (Planned)

Rationale: enhance the flat dictionary index with nested descent trees showing surname relationships.

- [ ] Database layer: group surname carriers into disjoint descent trees
- [ ] API: `GET /dictionary/family-names/{value}/tree` endpoint
- [ ] UI: recursive descent-tree component with generation indentation and SOSA badges when clicking on a last name in the dictionnary
- [ ] Resolve design questions: non-surname-carrying children handling, toggle vs. replacement

---

### Sprint E.9 — Denormalization Replaces Caching ✅ (Jul 2026)

> Rationale: the read models the cache held are a function of one person plus their immediate
> relatives, cheap to rebuild, and never acceptably stale — that is denormalization, not caching.
> E.6 proved the point by moving search into the database; E.9 finishes the job.
> See [Read Projections](read-projections.md) for the full architecture.

- [x] Add the `person_denorm` table (JSON payload, FK cascade on person/tree), `PersonDenormRepo`
  and migration `m20260728_000001_person_denorm`.
- [x] Move the projection types (`CachedPerson`, `CachedPedigree`, `SearchEntry`, …) from
  `oxidgene-cache::types` to `oxidgene_core::projection`, so `oxidgene-ui` no longer drags
  `oxidgene-db` / `tokio` / `dashmap` into the WASM build.
- [x] Replace `CacheService` with `ProfileService` (`oxidgene-api/src/profile/`), carrying the
  builder and the affected-set algorithm over unchanged.
- [x] Assemble pedigrees per request from ancestry traversal ⋈ `person_denorm` — pedigree cache,
  LRU budget and pedigree invalidation all removed.
- [x] **Delete the `oxidgene-cache` crate** (~4,100 lines, three storage backends) and the
  `redis`, `dashmap`, `rmp-serde`, `bincode` dependencies.
- [x] Remove the desktop disk-cache lifecycle (cache dir, staleness check, load-on-start,
  persist-on-shutdown handshake).
- [x] Rename the REST routes off `/cache/*` → `/profiles*` and `/pedigree/*`, **and the matching
  GraphQL fields and types**, so the two surfaces stay symmetric: `cachedPerson` → `personProfile`,
  `rebuildTreeCache` → `rebuildTreeProfiles`, `invalidateTreeCache` → `dropTreeProfiles`,
  `GqlCached*` → `GqlPersonProfile`/`GqlProfile*`/`GqlPedigree`, field `cachedAt` → `builtAt`.
  `expandPedigree` gained an optional `otherDepth`.
- [x] Port the integration tests to `crates/oxidgene-api/tests/profile_service_test.rs`, adding
  coverage for the guarantees a cache could not offer: projections survive a service restart,
  a relative's projection is never left stale after a rename, and a rolled-back mutation leaves
  no projection behind.
- [x] Make the refresh **atomic with the mutation**: widen all 119 repo methods from
  `&DatabaseConnection` to `&impl ConnectionTrait` (SeaORM implements it for `DatabaseTransaction`
  too), and open/commit one transaction across the write and the projection refresh in the 35
  REST + GraphQL mutation handlers. Whole-tree rebuilds stay outside — idempotent bulk work.

**Known follow-ups:**
- [ ] Refresh projections embedding a `Place` name when that place is renamed (pre-existing gap).

---

### Sprint E.10 — Surname Particle Fix & Bulk Repair ✅ (Aug 2026)

> Rationale: `split_surname_particle`'s flat particle list treated a bare leading article ("Le",
> "La") the same as a preposition ("de", "van"), so a whole class of names — Breton/Norman "Le …"
> surnames chief among them — got a spurious particle on every import. Fixing detection does not
> repair a tree already imported wrong, so the dictionary also gained a bulk edit.

- [x] Split `PARTICLES` into `HEAD_PARTICLES` (prepositions, may open a particle run) and
  `TAIL_PARTICLES` (bare articles, count only immediately after a head particle) in
  `oxidgene-core::types::surname`, with the same two-tier rule for elided forms (`d'`/`l'`).
  A leading article ("Le …", "La …") no longer splits; "de la Cruz" / "van der Berg" /
  "de l'Étang" still do.
- [x] `DictionaryRepo::set_family_name_particle`: re-cuts every `person_name` row matching a given
  surname (particle included) at a new particle, empty meaning "no particle". Rejects a particle
  not at the head of the surname (would inject a word the tree never had) and skips rows already
  cut that way (repeat calls are a no-op).
- [x] `PATCH /trees/{id}/dictionary/family-names/particle` (REST) and `setFamilyNameParticle`
  (GraphQL), both transactional and triggering a full projection rebuild when anything changed —
  a surname reaches every projection embedding a display name, so the affected set is unbounded.
- [x] Dictionary Family Names tab: pencil icon per row opens a modal showing the person count and
  a live preview (particle / root / filing letter) before applying.
- [x] Root-first display when filing by root: `d'Aubigné` under A now reads `Aubigné (d')` instead
  of leaving the particle stranded at the row's front.

---

### Sprint E.11 — Date Entry & Display ✅ (Aug 2026)

> Rationale: `DateQualifier::FromAge` shipped as a `<select>` entry with no behaviour behind it,
> and the widget wrote Gregorian month abbreviations whatever calendar was selected — so a
> Republican date left as `2 FEB 14` under a `@#DFRENCH R@` escape, which no reader can take back.
> Dates were also displayed as their bare stored value, dropping the qualifier that gives them
> their meaning.

- [x] "From an age" is a real entry mode: it swaps the day/month/year triplet for an age and the
  year it was observed in, and `DateParts::resolved` collapses the pair into the `About
  <year − age>` it stands for. Never persisted as `FromAge` — no schema, GEDCOM or GeneWeb form
  can record "aged 14 in 2026" — so a stored row reads back as the `About` date it always meant.
- [x] One `format_date` / `format_event_date` renders every date the reader sees (person profile
  vitals, unions and event list, both edit modals' event rows, the pedigree events panel), so a
  date reads « vers 2012 » rather than "2012" and reads the same way everywhere. The editor's
  literal preview goes through it too, so preview and page cannot disagree.
- [x] Per-calendar month vocabularies (`VEND…COMP`, `TSH…ELL`, thirteenth month included) for both
  the canonical stored value and the localized display, replacing a hardcoded Gregorian table.
  Fixes both directions: a Republican date is now written `2 BRUM 14`, and an imported one keeps
  its month instead of dropping it.
- [x] BCE years, stored the GEDCOM way (`15 MAR 44 BCE`, not `-44`) so exports stay readable, and
  parsed back from `BCE` / `BC` / `B.C.` / a leading minus. Year range 9999 BCE – 2999, excluding
  year 0.
- [x] Input protection: a keystroke guard turns away non-digits (a leading minus is allowed in
  year fields), paste and IME are caught by digit-stripping, and validation rejects dates that
  never existed — 30 February, a thirteenth Gregorian month, a backwards `Between` range, an age
  past 130. Leap rules follow the calendar, so 29 Feb 1900 is valid Julian and invalid Gregorian.
  Out-of-range entries are kept and explained inline rather than silently blanked.

- [x] `date_sort` is derived by the API, not sent by the client. Normalising a Julian, Hebrew or
  Republican date onto the Gregorian calendar needs `ged_io`, which a WASM frontend cannot reach,
  so the frontend had been reading the month number as if it were Gregorian: a Republican
  `2 BRUM 14` sorted in year 14, and a thirteenth month produced no key at all.
  `oxidgene_gedcom::date::sort_key` exposes the conversion the import path already used, and
  `service::event_date` wraps it for both write surfaces — including a patch, where whichever of
  calendar/value the request leaves alone is read back off the stored event, the two being
  meaningless apart. The field is gone from both request shapes.
- [x] French Republican dates are normalised to their documented Gregorian day by `ged_io`.
  Julian and Hebrew dates are checked against known dates too.

---

## EPIC F — Media Management (New, Sprints F.1–F.4)

Comprehensive media workflow: upload, storage, thumbnails, multi-page documents, image cropping (vignettes), event linking.

**First consumer:** the [Geneanet import](ui-geneanet-import.md) is blocked on this sprint — it arrives with hundreds of real files, multi-page PDFs and photos shared between several people, which makes it a better shakedown of the storage design than manual upload. Its steps 1–4 can be built before F.1; only the write step depends on it.

### Sprint F.1 — Media Storage & Serving 🔄

- [x] Media storage architecture — a `MediaStore` trait with one `FsStore` implementation, content-addressed as `{tree_id}/{aa}/{bb}/{sha256}.{ext}` under `OXIDGENE_MEDIA_ROOT` (default: the platform user-data directory, `~/.local/share/oxidgene/media` on Linux). Keys are scoped per tree so a purge is one directory removal, with no reference counting and no chance of pulling a file out from under another tree. Uploading the same scan twice writes one file and two rows — what a census page documenting eight siblings needs.
- [ ] **S3 backend for the server deployment.** The trait seam is in place and is the only thing a second implementation has to satisfy; the implementation itself, its credential/region/bucket configuration and its error mapping are not written. The web server runs on `FsStore` today, which needs a persistent volume.
- [x] `POST /trees/{id}/media/upload` (multipart; type decided by magic bytes, not by the declared MIME or the extension; 128 MiB ceiling; the body limit is lifted on that one route)
- [x] `GET .../media/{id}/file` and `.../thumbnail` (binary, `Content-Type`, RFC 6266 `Content-Disposition` that survives an accented name, SHA-256 as a strong `ETag`, `304` on `If-None-Match`)
- [x] Thumbnail generation on upload (longest edge 400 px, EXIF orientation applied, alpha preserved as PNG, decode-bomb limit; PDFs get none — see below)
- [x] Multi-page document parsing — page counts for PDF (via `lopdf`) and TIFF (IFD-chain walk, classic and BigTIFF)
- [x] Database schema — `media` gains `storage_key`, `sha256`, `thumbnail_key`, `width`, `height`, `page_count`; new `vignette` table with REST + GraphQL CRUD and a crop-on-read image endpoint
- [x] Tested on SQLite (69 unit + 23 media integration + 28 GraphQL tests)
- [ ] **PostgreSQL.** The migration test runs against a real server when `OXIDGENE_TEST_DATABASE_URL` is set, but no server was available in the sprint and there is no container harness in the repo, so the PostgreSQL path is still unexercised — as it has been since E.9.

**Deliberately out of scope, and why**

- **PDF thumbnails.** Rasterising a page needs pdfium or mupdf, a C dependency on a project that ships a desktop binary for three platforms. `thumbnail_key` is null for PDFs and the thumbnail endpoint answers `404`, so the UI branches on a status code rather than on a format list.
- **Audio and video.** Serving them usefully means `Range` requests and streaming, which belongs with EPIC H's chunked uploads.
- **GEDZIP bundling.** Was out of scope here — `export_gedzip` wrote the GEDCOM alone. Both directions shipped later: the export packs the media files (2026-08-18) and `.gdz` is now an import format too (2026-08-19).

**Two paths, on purpose.** `media.file_path` stays the GEDCOM `OBJE.FILE` value — the producer's own path, preserved verbatim so an export round-trips. `media.storage_key` is where OxidGene's copy lives, and is null for every GEDCOM-imported record until someone uploads the file; `POST .../media/upload` with a `media_id` is how that gap gets filled.

### Sprint F.2 — Media UI & Image Cropper 🔄

- [x] **MediaInput** (`components/media_input.rs`) — the upload cell that ends every gallery. Click for the platform file dialog, or drop files onto it; both land in the same loop. Uploads run **one at a time**: a user sending a folder of scans over a connection they do not control gets "3 of 12", which reads as progress, instead of twelve stalled requests finishing in an unpredictable order. One rejected file does not abandon the batch, and its message names the file — a folder containing a `.DS_Store` still delivers the other eleven.
- [x] **ImageCropper** (`components/image_cropper.rs`) — drag a rectangle on a scan, save it as a vignette. The whole component is about keeping **two coordinate systems apart**: the user drags in whatever pixels the image occupies on screen, a vignette is stored in the source image's own pixels, and the ratio comes from `media.width`/`media.height` (recorded at upload precisely so the frontend never decodes an image to find out how big it is) against the element's measured client rect. Crops already on the page are drawn while you draw the next one — without them the same entry gets cropped twice. Saving clears the draft but keeps the cropper open, since a register page is four crops in a row.
- [x] **MediaGallery** (`components/media_gallery.rs`) — thumbnail grid, ★ profile badge, hover controls, inline edit panel. One request per gallery, not one per tile: the `media-links?entity_type=…` endpoint returns the media alongside its link, because a tile cannot be drawn without the MIME type and whether a thumbnail exists. A missing `thumbnail_key` is the server saying it could not rasterise the file, so the tile draws a labelled icon rather than the broken image an `<img>` onto a 404 gives you. The edit panel opens **inline under the grid**, not as a second modal — the gallery already lives in one, and stacking leaves the user with two Cancel buttons.
- [x] **VignetteLinker** (`components/vignette_linker.rs`) — the crops on a media, each with the event it documents. Deliberately not part of the cropper: attribution is decided after the fact, looking at several crops at once, and forcing the choice while drawing turns "crop the page" into four interrupted tasks.
- [x] Integration with the **Person** and **Union** edit modals, and — read-only — with the **person profile page**. The profile shows the same grid with its controls withheld, so a reader who then clicks Edit finds the gallery they were just looking at; the section hides itself when the person has no files rather than leaving an empty frame on every profile in the tree.
- [x] Backend gaps F.1 left, filled here because the UI cannot have the features otherwise: `GET /media-links?entity_type=&entity_id=` (one entity's gallery, media included) and `PUT /media-links/{id}/profile` (the ★, which clears the person's others in the same statement so the tree never shows two). Both mirrored in GraphQL as `entityMedia` and `setProfileMediaLink`. Setting the flag rebuilds the person's projection, since the portrait is embedded in `person_denorm`.
- [ ] **Multi-page carousel.** A document's page count is known and shown on the tile, but there is no page-by-page viewer: `GET .../file` serves the whole PDF or TIFF, and rendering page 7 of one needs the rasteriser F.1 declined to take on. Cropping a document is refused for the same reason.
- [ ] **Date and place on a media.** The columns exist (`date_value`, `date_sort`, `place_id`) and [`ui-person-edit-modal.md` §10](ui-person-edit-modal.md) asks for them; the edit panel currently offers title and description only.
- [ ] Verified by compiling and by the API integration tests (32 in `media_test.rs`), **not by running the desktop app** — the gallery, the drag-to-crop interaction and the drop target have not been exercised by hand.

### Sprint F.3 — Event Linking, Media Fields & Multi-Page Documents 🔄

- [x] **Event evidence linking.** Every event editor gained an Evidence block (`MediaOwner::Event`), and the profile timeline shows each event's documents as small thumbnails. The timeline reads them from the **one** tree-wide media-links call the pedigree canvas already makes — that query now returns event links beside person links — so a forty-event life costs no extra request.
- [x] **Attaching a media to an event from the media's own side.** `GET /media-links?media_id=…` answers "what is this file attached to", and the media panel lists the tree's events as checkboxes. Mirrored as GraphQL `mediaLinks`.
- [x] **A media now carries what a fact carries.** `media` gained `date_qualifier`, `date_value2` and `calendar`, so the one date widget every event uses edits a photograph's date too — "around 1890", "between 1914 and 1918", "11 ventôse an IV" — and `date_sort` is derived server-side by the same code, so a photograph sorts against the events it sits between. `place_id` already existed and is now editable, through the **shared** place picker rather than a hand-rolled `<select>` (which reproduced exactly the bug that helper documents: a `value` on the element selects nothing when the options are built by a loop). `note` gained `media_id`, so "the left-hand column is water-damaged" has somewhere to live that is not the caption under the tile. **Deliberately no source field:** a media *is* a source document, and asking which source backs a scan of a parish register asks it to cite itself.
- [x] **Three kinds of media, told apart everywhere.** *Stored* (bytes in our store, thumbnail, croppable), *remote* (`file_path` is an http(s) URL — recorded, never fetched by us, so no thumbnail and no crop), *unheld* (a GEDCOM record naming a file nobody uploaded). The URL is editable for the last two and refused for the first, because there `file_path` is the value a GEDCOM export writes back and repointing it would make the export describe a file we are serving something else for. A remote MIME type is guessed from the URL's extension, which is the only evidence available without fetching.
- [x] **Viewing.** A viewer overlay embeds what the browser can play — image, video, audio — and offers a download for what it cannot, rather than an `<object>` onto a blank rectangle. Tiles show a per-kind icon when there is no thumbnail.
- [x] **Desktop file picker and Save-as.** The picker was already `rfd`'s native dialog. Added: a native **Save as…** in the viewer, desktop-only — the embedded WebView has no download UI of its own, so a `download` link does nothing there. It asks for the destination *before* fetching, since fetching megabytes and then discovering the user cancelled is work thrown away.
- [x] **Multi-page documents assembled from images.** F.1's `page_count` counts pages *inside* one file; a register is a folder of scans, which is a different thing. A document is now a `media` with `is_document`, and each page is a `media` with `parent_media_id` + `page_index`. A page has bytes, a thumbnail, dimensions and crops — every property `media` already models — so making it one means upload, storage, thumbnails, cropping and serving are all the code that already existed, rather than a second of each. The document carries the title, date, place, description and note; listings filter `parent_media_id IS NULL` so a nine-page act is one tile, not ten. The viewer pages through it with first/prev/next/last **and a numbered strip**, because "the entry is on page 27" is how a register is referenced and counting there with a Next button is absurd. `page_count` is recomputed from the pages that exist rather than incremented, and detaching a page closes the gap so page 3 of a 2-page document is never a number.
- [ ] **Vignette assignment as an event illustration.** A vignette can already say which event it documents (`vignette.event_id`, set from the linker), but the timeline shows the *whole* media as evidence, not the crop. Showing the crop where one exists is the remaining half.
- [ ] **PDF page rendering.** A PDF still opens as a whole file: paging *inside* one needs the rasteriser F.1 declined to take on as a C dependency. Multi-page now works for documents assembled from images, which is the case a scanner produces.
- [ ] Verified by compiling, by `clippy --all-targets --all-features` and by 41 API integration tests. **Not exercised by hand in the running app**: the pager, the drop target, the drag-to-crop and the native save dialog have not been used.

### Sprint F.3b — Geneanet Import Wizard 🔄

Specified in [ui-geneanet-import.md](ui-geneanet-import.md); the pipeline it
drives is [geneanet-media-import.md](geneanet-media-import.md).

- [x] **The pipeline moved out of the CLI into `crates/oxidgene-geneanet`.** `model`, `key`, `join`, `client`, `media` and the browser `script`s are now shared by the CLI and the API, so the headless and the interactive paths cannot drift apart on the join, the key folding or the size matching. `apps/oxidgene-cli/src/geneanet/` keeps the terminal driver and the `.gdz` writer; the app imports straight into a tree, so that container is a CLI affordance rather than the product's path.
- [x] **`archive.rs` — data archives read where they lie.** A ZIP's central directory records every entry's uncompressed length, which is exactly what the size matching needs, so several gigabytes of export cost a few kilobytes to index and nothing is extracted. Generalised over `LocalOriginals` so the CLI's unzipped `--local-media` directory and the wizard's still-zipped archives answer the same question. A size clash between *different* files resolves to nothing and the caller downloads — detected rather than silently guessed, which is the property that matters when a third of a real archive is same-scanner dossier pages.
- [x] **The import modal replaces the native file picker** on the tree card's `⋮` → Import. Two tabs: a file (`.ged`/`.gw`) and the Geneanet flow. The file tab also **fixes a real bug** — it read the picked file through `path()` + `tokio::fs::read`, which has no meaning in a browser, so the "shared web/desktop" import had in fact been desktop-only with nothing saying so.
- [x] **Five steps, exactly one expanded**, each settled one collapsing to a one-line receipt and the unreachable ones dimmed but visible.
- [x] **Step 3 opens a real `wry` login window** and issues the collection *inside it*: a probe on each page load reports whether the media API answers, then the ~19-request collection and a `HEAD` sizing pass run on the user's own session. This is the answer to the Cloudflare TLS fingerprinting that challenges the CLI — not a way around the check, but the thing the check is asking for. `oxidgene-ui` stays platform-free: it declares a `GeneanetCollector` trait that `oxidgene-desktop` implements and injects.
- [x] **Step 4's mismatch guard.** Under 10 % of keyed references finding a person blocks the import behind "are the export and the account the same tree?", with *Go back to step 1* primary and *Import anyway* secondary.
- [x] **Step 5 writes one `media` per photo and one `media_link` per person on it**, so a group photo is stored once and attached to everyone in it — what the export could not express and what `MediaLink`'s shape was always for. A photo that cannot be fetched is reported and skipped rather than aborting a run whose ten thousand people are already in.
- [x] **Web/desktop boundary made visible.** Steps 2, 3 and the photo half of 5 need a filesystem path and a same-origin window, neither of which a browser has. Both render an explanation naming the desktop app; the `.gw` still imports on web.
- [x] **en/fr parity test for the ~200 new strings.** A missing key does not fail to compile and does not fail to render — it renders as the key itself, mid-sentence, only for users of the other language. A second test pins that a `{placeholder}` in one language exists in the other.
- [ ] **Per-photo progress and cancellation in step 5.** The bar is indeterminate and the run cannot be interrupted; the person import is transactional but the photo pass that follows is not, so interrupting would leave a tree with some of its photos.
- [ ] Verified by `cargo fmt`, `clippy --workspace --all-targets` (clean) and the full test suite (572 passing). **Not exercised against a live Geneanet account**: the login window, the collection, the sizing pass and the download fallback have not been run end to end.

### Sprint F.4 — Performance & Polish

- [ ] Thumbnail caching
- [ ] Performance testing (large media libraries)
- [ ] Error handling (format validation, upload limits)
- [ ] Full test coverage

---

## EPIC G — Security & Deployment (formerly EPIC F)

- [ ] Authentication system (JWT or session-based).
- [ ] User registration and login.
- [ ] Per-tree access control (guest, read-only, editor).
- [ ] Contemporary individual masking for guests.
- [ ] Audit logging.
- [ ] Kubernetes manifests (deployment, service, ingress).
- [ ] FluxCD GitOps configuration.
- [ ] Liveness/readiness probes.
- [ ] Production PostgreSQL configuration.
- [ ] TLS termination + HTTP/2 for the web server.

---

## EPIC H — Asynchronous Pipeline (Post-MVP, formerly EPIC G)

- [ ] Platform-specific build and smoke test (Linux, macOS, Windows).
- [ ] Performance testing with 100K-person trees.
- [ ] Message queue integration (Redis/RabbitMQ/NATS).
- [ ] `document-queue` orchestration service.
- [ ] Chunked media uploads.
- [ ] Async GEDCOM processing for large files.
- [ ] Rust worker pool for background tasks.
- [ ] Notification system (processing status).
- [ ] Object storage (temporary and persistent).
