---
okf_version: "0.1"
---

# Geneanet Media Import — recovering the person↔photo links

![OxidGene](../assets/OxidGene.png)

How to get a Geneanet tree **with its photos** into OxidGene, when neither
Geneanet export carries the link between the two.

This document is the **technical** half: the API, the join, the matching. The
user-facing flow it feeds is specified in
[Geneanet Import Wizard](ui-geneanet-import.md).

Related: [Architecture](architecture.md) · [Data Model](data-model.md) ·
[Geneanet Import Wizard](ui-geneanet-import.md) ·
[GEDCOM Import](ui-gedcom-import.md) · [General](general.md)

> **Destination: a tree, not a file.** An earlier draft of this pipeline
> emitted a `.gdz`. That was backwards — OxidGene can already *export* a tree to
> `.gdz`, so producing one only to re-import it is a detour. The photos are
> imported straight into a tree as `Media` + `MediaLink` rows, and anyone who
> wants a `.gdz` exports one afterwards.
>
> **Blocked on Sprint F.1.** `Media` is a metadata-only entity today — the
> GraphQL mutation still says *"no actual file upload in MVP"* — so there is
> nowhere to put image bytes until [F.1 Media Storage](roadmap.md) decides
> between filesystem and object storage. This import is F.1's natural first
> consumer: it arrives with 378 real files, multi-page PDFs, and photos shared
> by several people.

---

## 1. The problem

Geneanet offers two exports, and neither one carries the photos.

**The tree export** (`gw.geneanet.org/my-tree/operations/export`, `.ged` or
`.gw`, with "Liens web vers les photos principales des individus" and "Images de
la chronique familiale" ticked) emits **at most one medium per individual** —
the default portrait — as a URL:

```gedcom
0 @I136@ INDI
1 NAME Renée /SURNAME_A/
1 OBJE
2 FILE http://gw.geneanet.org/public/img/media/deposits/private/eb/fc/16196174/…/medium.jpg
```

That URL is a *downsized rendition*, and if the medium is marked private it
`403`s for anyone not logged in. Everything beyond the portrait is gone: the
other photos on the person's page, every group photo, every scanned document.

**The data export** (`www.geneanet.org/my-data/dashboard`) gives you a ZIP of
every original file — but named after the *upload*, with no deposit id and no
person attached. You end up with two halves that cannot be put back together:

| | Tree export | Data export |
|---|---|---|
| person ↔ medium link | one portrait each | none |
| file contents | rendition URL, often `403` | originals |

Measured on a real 10 254-person tree:

| | Tree export | Actually on the account |
|---|---|---|
| media | 219 `OBJE` / 218 `#image` | **378 deposits / 614 views** |
| person↔media links | 219 | **482**, across 234 persons |
| group photos | inexpressible | **62 views** linked to several persons |

The export loses roughly 55 % of the links and 100 % of the structure.

## 2. The key insight

Geneanet's media manager (`www.geneanet.org/media/manager`) is backed by a JSON
API that still holds the mapping, and it hands each link back keyed by the
**GeneWeb key** — `lastname|firstname|occurrence`:

```json
[{ "firstname": "Renée", "lastname": "SURNAME_A",
   "reference_extra_geneweb": { "ref": "surname_a|renee|" } }]
```

That is exactly what a `.gw` export encodes (`SURNAME_A Renée`,
`SURNAME_B Charles.1`), because GeneWeb has no surrogate id: a person *is*
that triple.

> **This is why the pipeline needs the `.gw` and not the `.ged`.**
> A GEDCOM's `@I136@` xrefs are opaque, assigned at export time and carrying no
> identity. The `.gw` is the only export that speaks the same language as the
> media API.

## 3. The three inputs

| Input | Where from | Why |
|-------|-----------|-----|
| `.gw` export | `gw.geneanet.org/my-tree/operations/export` | the tree, and the join key |
| a logged-in Geneanet session | the app's own login window, or a cookie for the CLI | media are private; every endpoint `403`s without it |
| data archive *(optional)* | `www.geneanet.org/my-data/dashboard`, **as downloaded** | the originals, so they need not be downloaded again |

**The archive is optional; the login is not.** This is the point users get
wrong, and the wizard says it plainly: the ZIPs contain no link to any person.
Which photo belongs to whom exists *only* on Geneanet, behind the session.
Without the archive the import still works — it downloads the originals
instead, which is slower but complete.

**Do not unzip the archive.** Geneanet splits it into several ZIPs when it is
large, and they are consumed in place: a ZIP's central directory records the
uncompressed size of every entry, which is exactly what the size matching in §5
needs. Reading a few kilobytes per archive is enough — no extraction, no
temporary space, and the split is handled by iterating over the files.

> **`.gw`, never `.ged`.** The `.gw` is the only export that carries the
> occurrence number, and the occurrence is part of the join key. See §2.

### 3.1 What the cookie actually needs

Measured by bisection against the live API. **Exactly one** of these
authenticates; nothing else in a browser cookie is read by this API:

| Cookie | Alone | What it is |
|--------|-------|------------|
| `gntsess5` | **200** | the session id — *use this one* |
| `REMEMBERME` | **200** | Symfony remember-me token; mints a fresh session on its own |
| `cf_clearance` | 403 | Cloudflare, authenticates nothing |
| nothing | 403 | — |

Everything else a browser sends — `__cf_bm`, `autolang`, `at_check`, `mbox`,
`mboxEdgeCluster`, `ismobile`, `tarteaucitron`, `geneweb_base`, the
`gntforum_phpbb_*` family, `dtjs` — is irrelevant.

```bash
export GENEANET_COOKIE='gntsess5=<value>'
```

> **Prefer `gntsess5` over `REMEMBERME`.** The remember-me token is valid for
> *months* and can mint new sessions on demand, so leaking it is far worse than
> leaking a session id, which dies when you log out. Pasting the whole browser
> cookie works, but hands around far more than the job needs.

The CLI refuses a cookie carrying neither name before making a single request,
rather than emitting hundreds of identical `403`s.

### 3.2 One header is mandatory

Every endpoint — including the image renditions on `gw.geneanet.org` — requires:

```http
X-Requested-With: XMLHttpRequest
```

Without it Geneanet answers `403` with an HTML page, *even with a valid
cookie*. The client sets it as a default header on every request.

## 4. The API

All three calls need the session cookie.

### `GET /media/api/deposits?page=N&per_page=100`

Every deposit the account holds. Total in the `x-gnt-media-total` header.

```json
{ "id": 16053569, "title": "Renée", "type": "portraits", "private": true,
  "date_create": "2019-04-26T…",
  "views": [{ "id": 16196174, "page": 1,
              "files": { "normal": "…/normal.jpg", "medium": "…/medium.jpg",
                         "screen": "…/screen.jpg", "thumbnail": "…/thumbnail.jpg" } }] }
```

A **deposit** is one upload; a **view** is one page of it. Links attach to
views, not deposits. There is no `original` rendition — only `/media/download`
serves that.

### `GET /media/api/references?page=N&per_page=100`

**The endpoint the whole pipeline exists for, and the one to use.** Every
person↔media link on the account, each carrying its **whole deposit inline**:

```json
{ "id": 15872352,
  "deposit": { "id": 11529525, "title": "…", "views": [ … ] },
  "firstname": "Georges Auguste", "lastname": "LE SURNAME",
  "reference_extra_geneweb": { "ref": "le surname|georges auguste|" } }
```

`per_page` is capped at 100 however much you ask for. On the reference account
that is **6 requests for all 517 links**, of which 482 carry a GeneWeb key.

Its one blind spot: for a multi-page deposit it lists *every* page, so it does
not say which one the link sits on.

### `GET /media/api/deposits/{depositId}/views/{viewId}/references`

The per-page version, and the fix for that blind spot. Used only for deposits
with several pages, probing pages until every link the bulk pass reported for
that deposit is accounted for — links cluster on page 1, so this costs about
one request per multi-page deposit.

**Measured end to end: 19 requests** (4 deposits + 6 references + 9 probes),
producing a manifest identical to the 618-request per-view walk it replaces —
378 deposits / 614 views / 379 linked views / 234 persons / 35 keyless
references. Request volume is what draws Cloudflare, so this is a correctness
property as much as a performance one.

### `GET /media/download/?deposits[]={id}`

The original, byte for byte — verified identical to the data-archive copy
(69 122 bytes for the sample portrait, matching `portrait.jpg` exactly).

- one deposit, one page → the raw file, with a `Content-Length`
- one deposit, several pages → a ZIP of its pages, **streamed without a
  `Content-Length`**
- several `deposits[]` → a ZIP, entries named after the original uploads

The manifest materialises this as an `original` field **on each deposit** —
derivable from the id, but written out so a consumer never has to know how to
build it. Deposit-level on purpose: it is one image when `views` holds a single
entry and a ZIP of every page when it holds several, and `views[].files` carries
only downsized renditions. Putting it under `files` alongside `normal`/`medium`
would hand a consumer a whole archive under a key that reads like a per-page
image; `views.len()` is what says which of the two will arrive.

The trailing slash is not decoration: `/media/download` without it answers `301`
to the same path *with* one, so omitting it doubles the request count of the
whole download phase.

## 5. Matching originals you already have

The data archive's filenames cannot be matched to deposits by name: they are
upload names, unrelated to the deposit title
(`"Grandparents in 1953"` → `grandparents.png`),
and a 144-page deposit's pages are named `00002.JPG`, `scan_002.jpg`…

So we match on **exact byte size**, which both sides can state without
transferring anything:

- **Geneanet's side** — `HEAD /media/download/?deposits[]={id}` returns
  `Content-Length` with no body.
- **The archive's side** — every ZIP records each entry's uncompressed size in
  its central directory. Reading that costs a few kilobytes per archive and
  requires no extraction, so the archives are indexed **in place, still
  zipped**, across however many files Geneanet split them into.

Then, per deposit:

1. `HEAD` → the original's exact length
2. archive entries of exactly that length are the candidates
3. **0 candidates** → download the original
4. **1 candidate** → use it
5. **several candidates** → use one only if their contents are identical (a
   duplicate upload); otherwise **download**, never guess

On the reference archive this gave **607 distinct sizes for 613 entries**, and
every one of the 6 collisions was the same file uploaded twice.

Two properties worth keeping when this is reimplemented: an entry is never
attached on a *probable* match, and a size clash is **detected** rather than
silently resolved.

### Why not one bulk download instead of one `HEAD` per deposit

Tempting — `/media/download/` accepts any number of `deposits[]`, and its ZIP
entries come out in request order, so a single request looks like it could
replace hundreds. Measured, it cannot:

- **`Range` is ignored.** `Range: bytes=-2000` returns `200` with the entire
  body, and the response carries neither `Accept-Ranges` nor `Content-Length` —
  the archive is assembled on the fly and streamed.
- **Local file headers carry no sizes.** The streaming bit (flag `0x08`) is set
  and both size fields are `0xFFFFFFFF`, so reading the stream as it arrives
  yields filenames and nothing else.
- **Sizes live only in the central directory, which is at the end.** Reaching it
  means having received everything before it.

So the comparison is 378 `HEAD`s transferring *no body* against one request
transferring ~780 MB. And the moment you have downloaded that archive you hold
every original already, which makes matching against a local copy pointless —
that path is just `fetch`, which exists.

Entry order does track request order, but no entry carries a deposit id, so any
mapping would be positional and would drift on multi-page deposits, which
contribute one entry per page.

(The manager UI also paginates at 20 deposits per page, so its "select all"
covers only the visible page — a manual bulk download would be ~19 archives,
not one.)

> **Why not a perceptual hash?**
> A pHash answers a harder question — matching *re-encoded* images — and answers
> it with a distance and a threshold. Here both sides are the same original, so
> byte equality is available and exact. It matters: ~240 of the 614 views are
> pages of administrative dossiers, same scanner, mostly white with text, where
> a pHash misattributes *silently*. A size clash, by contrast, is detected and
> falls back to downloading.

### Multi-page deposits

Their archive has no `Content-Length`, so they cannot be size-matched. On the
reference tree only **9 of 244** multi-page views are linked to anyone (almost
always page 1), so the default takes those from the per-page `normal` rendition
— one small download each — and says so in the run report.

`--multipage-originals` instead pulls each deposit's archive and extracts the
page by position (archive entries come out in page order). Costly — it fetches
244 pages to use 9 — but right when the pages are documents you need to read.

## 6. Folding the key

Geneanet folds names before writing a reference: lowercase, accents stripped,
and `_`, `-` and `'` all become spaces. The occurrence is left empty when zero.

| `.gw` | Geneanet ref |
|-------|--------------|
| `SURNAME_A Renée` | `surname_a\|renee\|` |
| `LE SURNAME Georges_Auguste` | `le surname\|georges auguste\|` |
| `Jean-Marie` | `jean marie` |
| `D'SURNAME_C` | `d surname c` |
| `SURNAME_B Charles.1` | `surname_b\|charles\|1` |

The hyphen and the apostrophe were each found by a failing join, not by
guessing — see `apps/oxidgene-cli/src/geneanet/key.rs`, where both have a
regression test naming the case.

Letters with a stroke (`ł`, `ø`, `đ`, `ß`, `æ`, `œ`, `þ`) have no canonical
decomposition, so NFD leaves them intact and the join would silently miss; they
are folded explicitly.

## 7. Attaching to the right person

`GwDatabase::persons[i]` becomes `GedcomData::individuals[i]` with xref
`@I{i+1}@` — a positional correspondence the conversion guarantees, and which
the builder asserts before writing anything.

A key matching **several** persons is never attached: it is reported as
ambiguous, because putting the photo on one of them would be a coin toss.

## 8. Usage

```bash
# 1. Collect the mapping. ~19 requests, no media. Safe and cheap.
oxidgene-cli geneanet-media manifest \
  --cookie "$GENEANET_COOKIE" \
  --out geneanet-media/manifest.json

# 2. See what will land where. Offline — no cookie, no network.
oxidgene-cli geneanet-media check \
  --gw samples/tree.gw \
  --manifest geneanet-media/manifest.json

# 3. Build the archive, reusing the originals you already downloaded.
oxidgene-cli geneanet-media gedzip \
  --cookie "$GENEANET_COOKIE" \
  --gw samples/tree.gw \
  --manifest geneanet-media/manifest.json \
  --local-media samples/media_images \
  --out geneanet-media/tree.gdz
```

`--cookie` also reads `GENEANET_COOKIE` from the environment, which is the
better habit: a session cookie carries `REMEMBERME` and grants full account
access, so it should not end up in shell history.

`geneanet-media fetch` downloads every deposit's original into a directory and
records the path in the manifest. Resumable, and independent of the `.gdz`
path.

### Throttling

`--delay-ms` (default 100) is the only knob, and requests go out one at a time.
There is deliberately no concurrency setting: collecting the mapping costs ~19
requests, so parallelism buys nothing and costs the one thing worth protecting
— traffic that looks like a person rather than a crawler.

The heavier phase is `gedzip`, which needs one `HEAD` per deposit to size-match
against the local archive (378 on the reference tree, ~40 s at the default
delay). That is irreducible: no endpoint reports a deposit's byte length in
bulk.

A `429` or `403` stops the run with an explanation rather than retrying into a
wall.

Whether scripted access fits Geneanet's terms of service is the operator's call:
the data is the account owner's own, fetched through the account's own session,
but that is not the same as a licence to automate.

### Cloudflare

geneanet.org sits behind Cloudflare, which can decide a non-browser client
deserves an interactive challenge. It answers `403` with `cf-mitigated:
challenge`, `server: cloudflare` and a "Just a moment…" HTML page.

**This looks exactly like an expired cookie and is not one.** The client
detects it and says so, because the obvious reaction — go fetch a fresh
cookie — fixes nothing. Observed directly: with the *same* cookie and the
*same* `User-Agent: oxidgene-cli/0.1.0`, `curl` gets `200` while the Rust
client is challenged. The cookie is irrelevant (full browser cookie and bare
`gntsess5` are challenged alike), and so is the user agent. What differs is the
TLS/HTTP2 fingerprint of the stack — rustls + hyper against curl's OpenSSL.

The challenge is adaptive: the same binary collected a full 378-deposit
manifest earlier the same day, then began to be challenged after a few hundred
requests from the same address.

**Defeating that fingerprinting is deliberately not implemented**, and should
not be. It is bot detection, and dressing the client up as something else to
get past it is evasion whoever owns the data. The supported responses are to
run gently (a higher `--delay-ms`), to run when the challenge has lapsed, or to accept that this path is closed and use the
per-fiche fallback in §10.

The metadata phase is now cheap enough to be largely out of reach of this — 19
requests — and the resulting manifest is reusable indefinitely, so it need only
succeed once.

## 9. Output

**A tree.** The `.gw` goes through the existing `import_geneweb` →
`persist_import_result` path, and the manifest then produces `Media` rows plus
one `MediaLink` per person the photo is attached to.

A photo shared by several people is **stored once** with several `MediaLink`
rows — precisely what the original export could not express, and what
`MediaLink`'s shape was already built for. `is_profile` carries Geneanet's
`is_default` flag, so the portrait stays the portrait.

Exporting that tree to `.gdz` afterwards is a separate, already-supported
operation. The CLI's `gedzip` subcommand predates this decision and remains
useful headless, but it is not the app's path.

Reference run, 10 254-person tree:

```
378 media attached to 234 persons (482 links)
235 views linked to nobody on Geneanet
 35 references without a GeneWeb key (persons outside the tree)
```

The 35 are irreducible: they name people who are not in this tree. Everything
else joins.

## 10. Limits

- **Unlinked media are dropped.** 235 views are attached to nobody on Geneanet;
  they are counted and reported, not imported. `geneanet-media fetch` retrieves
  them if wanted.
- **Multi-page pages are downsized by default.** See §5.
- **No incremental re-import.** Nothing here reconciles a second Geneanet
  export against a tree already imported. That is
  [Person Merge](ui-merge.md) territory.
- **Media storage does not exist yet.** See the note at the top: `Media` is
  metadata-only until Sprint F.1.
- **The API is undocumented.** `apps/oxidgene-cli/src/geneanet/model.rs` holds a
  test pinning the live wire shape; it is the first thing to fail if Geneanet
  reshapes the payloads.
- **Fallback if the deposits API disappears.** Each `?type=fiche` page embeds
  the same data as a `gntGeneweb.media` JSON blob in a `<script>` — but that is
  one fetch per person (10 195) instead of ~19 API calls.

## 11. Prior art, and why it does not help

Two Gramps addons look adjacent enough to be worth ruling out, from
[`grocanar/glopglop-addons-sources`](https://github.com/grocanar/glopglop-addons-sources).

### `ImportGenewebPlus` — a richer `.gw` importer for Gramps

Nothing to take. Tag coverage, counted across both parsers:

| | `.gw` tags handled |
|---|---|
| the `geneweb` crate we use | **110** |
| `ImportGenewebPlus` | 83 |
| present in the reference export | 50 |

A strict superset: no tag it handles that the crate does not, and no tag in a
real export left unhandled. Its treatment of images is, in full:

```python
elif field == '#image' and idx < len(fields):
    LOG.debug("Image: %s" % fields[idx])
    idx += 1
```

It logs the image and skips it. On the problem this document exists to solve,
it does not reach as far as the export already does.

### `GedcomforGeneanet` — a Geneanet-friendly GEDCOM *exporter*

The opposite direction: Gramps → Geneanet. Its media handling amounts to
"create a zip of the relevant media", which is the GEDZIP idea, and it says
nothing about reconstructing person↔media links — because in that direction
they were never lost.

### What this confirms

Nothing is lost by working from a `.gw` rather than a `.ged`. The `.gw` is the
*richer* input here: it is the only export carrying the occurrence number, and
the occurrence is part of the join key.

## 12. Why Geneanet's own GeneWeb API cannot replace this

[`geneweb-plugin-api`](https://github.com/geneanet/geneweb-plugin-api) is
Geneanet's own plugin, and it does expose images — `API_IMAGE_ALL` returns
every person's picture in one call, keyed by `reference_person` (`n`, `p`,
`oc`), which is the same GeneWeb key this pipeline joins on. Tempting, and a
structurally nicer source than parsing the export.

It cannot help, for a reason that is not going to change: it reads
`Image.get_portrait`, and GeneWeb's person record holds **a single `image`
string**. `API_UPDT_IMAGE` writes it the same way —
`{gen_person with image = Gwdb.insert_string base img}`. One person, one image.

That is the whole GeneWeb image model, and it is exactly the ceiling the GEDCOM
export hits. The extra photos on a person's page, the group photos shared by
several people, the scanned dossiers — none of it exists in the GeneWeb base.
It lives in Geneanet's own media layer, bolted alongside, which is why
`/media/api` is the only surface that knows about it.

The plugin's other seven endpoints (`API_SOSA`, `API_MAX_ANCESTORS`,
`API_LAST_MODIFIED_PERSONS`…) are about traversal and bookkeeping, not media.
