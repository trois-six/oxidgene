---
okf_version: "0.1"
---

# Geneanet Media Import — recovering the person↔photo links

![OxidGene](../assets/OxidGene.png)

How to get a Geneanet tree **with its photos** into OxidGene, when neither
Geneanet export carries the link between the two.

This document is the **technical** half: the API, the join, the matching. The
user-facing flow it feeds is specified in
[Import](ui-import.md).

Related: [Architecture](architecture.md) · [Data Model](data-model.md) ·
[Import](ui-import.md) ·
[Import](ui-import.md) · [General](general.md) ·
[Geneanet Upload API](geneanet-upload-api.md) (the *other* Geneanet API —
the upload app's — reverse-engineered 2026-08-16; Cloudflare/client findings
live there too)

> **Destination: a tree, not a file.** An earlier draft of this pipeline
> emitted a `.gdz`. That was backwards — OxidGene can already *export* a tree to
> `.gdz`, so producing one only to re-import it is a detour. The photos are
> imported straight into a tree as `Media` + `MediaLink` rows, and anyone who
> wants a `.gdz` exports one afterwards.
>
> Files are stored on the filesystem, content-addressed and scoped per tree,
> behind `MediaStore`, and reach the server through
> `POST /trees/{id}/media/upload`. A shared photo is stored once and linked many
> times, while multi-page documents preserve their page structure.

---

## 1. The problem

Geneanet offers two exports, and neither one carries the photos.

**The tree export** (`gw.geneanet.org/my-tree/operations/export`, `.ged` or
`.gw`, with "Liens web vers les photos principales des individus" and "Images de
la chronique familiale" ticked) emits **at most one medium per individual** —
the default portrait — as a URL:

```gedcom
0 @I136@ INDI
1 NAME GIVEN_A /SURNAME_A/
1 OBJE
2 FILE http://gw.geneanet.org/public/img/media/deposits/private/<path>/medium.jpg
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

Authorized validation confirmed that tree exports expose only a minority of
the available links, cannot express group-photo associations, and omit the
structure of multi-page deposits. Most tested views were private, which makes
anonymous download routes unsuitable.

## 2. The key insight

Geneanet's media manager (`www.geneanet.org/media/manager`) is backed by a JSON
API that still holds the mapping, and it hands each link back keyed by the
**GeneWeb key** — `lastname|firstname|occurrence`:

```json
[{ "firstname": "GIVEN_A", "lastname": "SURNAME_A",
   "reference_extra_geneweb": { "ref": "surname_a|given_a|" } }]
```

That is exactly what a `.gw` export encodes (`SURNAME_A GIVEN_A`,
`SURNAME_B GIVEN_B.1`), because GeneWeb has no surrogate id: a person *is*
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

### 3.3 Getting a session: password login is dead (verified 2026-08-16)

Do not design anything around automating the login form — both password paths
are closed:

- **The legacy endpoint** (`POST /connexion/verify.php?ctype=id`, the old
  Android-app login) answers `-1` even with **valid** credentials. Trap: it
  still *sets* a `gntsess5` cookie — an anonymous placeholder, random per
  attempt, that authenticates nothing (`/media/api/references` → app-level
  `403`, 34-byte body, same as no cookie). A `Set-Cookie` is not proof of
  authentication; validate against an authenticated endpoint.
- **The modern form** (`/connexion/login_check`, Symfony) enforces **reCAPTCHA
  v2 server-side on every attempt** (flash: *"We couldn't confirm that you are
  not a robot…"*). Not scriptable without a captcha solver.

What remains:

- **The desktop app's login window** — a real webview, a human solves whatever
  challenge appears. This is the only automatable login, and §6 of the wizard
  spec is built on it. The app pre-ticks and hides the form's `_remember_me`
  checkbox so the (now hidden) collection window's session survives `gntsess5`
  expiry; it still only *extracts* `gntsess5`.
- **Cookie import for the CLI** (yt-dlp `--cookies` pattern): the user copies
  `gntsess5` out of devtools once. `REMEMBERME` alone also authenticates
  (§3.1) and lives ~1 year — which is exactly why we ask for `gntsess5`
  instead.
- One programmatic login does survive, on the *other* API: the Geneanet Upload
  app's OAuth2 password grant on `api.geneanet.org` issues tokens with no
  captcha. Useless here — that surface has no originals, no bulk references
  and no `is_default` (§4b) — but worth knowing it exists.

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
{ "id": 0, "title": "<media title>", "type": "portraits", "private": true,
  "date_create": "<ISO 8601 timestamp>",
  "views": [{ "id": 0, "page": 1,
              "files": { "normal": "…/normal.jpg", "medium": "…/medium.jpg",
                         "screen": "…/screen.jpg", "thumbnail": "…/thumbnail.jpg" } }] }
```

A **deposit** is one upload; a **view** is one page of it. Links attach to
views, not deposits. There is no `original` rendition — only `/media/download`
serves that.

#### Media classification

The deposit's `type` is retained on import; it is not inferred from the image
format or the number of pages. OxidGene records the two questions separately:

| Geneanet `type` | `source_media_type` (GEDCOM) | `document_category` (OxidGene) |
|---|---|---|
| A GEDCOM medium such as `photo`, `film` or `video` | That exact GEDCOM medium | none |
| `portraits` | `photo` | `portrait` |
| `photo_groupe` | `photo` | `group_photo` |
| `État civil` / `acte_etat_civil` | `manuscript` | `civil_record` |
| `Registre paroissial`, `Archive notariée`, `Archive militaire`, `Recensement` | `manuscript` | the matching record category |
| `Blason` | `other` | `coat_of_arms` |
| `Tombe` | `tombstone` | `grave` |
| `autres`, absent, or an unrecognised value | `other` | `other` |

The category labels are translated by the UI's i18n catalogs after import. A
French value such as `Archive notariée` therefore appears as *Notarial record*
in an English interface, while its GEDCOM export remains `MANUSCRIPT`.

`private` is likewise retained rather than treated as the tree default:
`true` writes `Media.privacy = private`, and `false` writes `public`. It applies
to the parent and every page of a multi-page deposit, and the media viewer shows
the resulting visibility with the normal i18n label.

`date_create` is the creation timestamp of the Geneanet deposit. It writes
`Media.created_at` (converted from RFC 3339 to UTC) for a single medium, a
multi-page document and every page beneath it. A missing or malformed source
timestamp falls back to the import time, so it cannot stop an import. Media
created directly in OxidGene continue to use their local creation time. This is
separate from the historical `Media.date_value`, which describes when the image
was taken or the record was made.

### `GET /media/api/deposits/{depositId}`

After the bulk references are known, OxidGene requests this detail endpoint for
each distinct **linked** deposit, with at most four requests in flight. A failed
detail request leaves the media importable; it only omits the optional
enrichment. The response supplies the historical `date` and `location` that the
paginated payload does not.

`date` is not retained as a source string. It is converted to the same four
fields as every Event and Media date: `calendar`, `date_qualifier`,
`date_value`, `date_value2`, plus the server-derived Gregorian `date_sort`.
Geneanet's ISO-like partial dates are normalised first: `1924-00-00` becomes
the Gregorian GEDCOM year `1924`; `1946-09-00` becomes `SEP 1946`; and
`1946-09-03` becomes `3 SEP 1946`. The ordinary calendar-aware media editor can
then edit it without a Geneanet-specific path.

`location` is resolved against the tree's existing `Place` rows by trimmed,
case-insensitive name. A missing name creates one normal `Place`, and the
resulting id is written to `Media.place_id`. The document and every one of its
pages receive the same date and place metadata.

#### Source provenance for future multi-user servers

Geneanet exposes `username_sender`, the account that submitted a deposit. The
future provenance/audit model must preserve this source value alongside an
imported medium. It is **not** an OxidGene actor id: the Geneanet login,
`username`, and `username_sender` must never be matched to, or used to create,
an OxidGene user during import. Account attribution belongs to the later
multi-user server design, where it can retain both the external source identity
and the OxidGene actor that performed the import.

### `GET /media/api/references?page=N&per_page=100`

**The endpoint the whole pipeline exists for, and the one to use.** Every
person↔media link on the account, each carrying its **whole deposit inline**:

```json
{ "id": 0,
  "deposit": { "id": 0, "title": "<media title>", "views": [ … ] },
  "firstname": "GIVEN_A GIVEN_B", "lastname": "SURNAME_A",
  "reference_extra_geneweb": { "ref": "surname_a|given_a given_b|" } }
```

`per_page` is capped at 100. References without a GeneWeb key remain explicit
and unresolved.

Its one blind spot: for a multi-page deposit it lists *every* page, so it does
not say which one the link sits on.

### `GET /media/api/deposits/{depositId}/views/{viewId}/references`

The per-page version, and the fix for that blind spot. Used only for deposits
with several pages, probing pages until every link the bulk pass reported for
that deposit is accounted for — links cluster on page 1, so this costs about
one request per multi-page deposit.

Authorized validation produced a manifest identical to a complete per-view
walk while using paginated bulk requests plus probes only for multi-page
deposits. Lower request volume reduces Cloudflare challenges, so this is a
correctness property as much as a performance one.

### `GET /media/download/?deposits[]={id}`

The original, byte for byte, verified identical to its data-archive copy.

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

### `normal` is not the original — verified 2026-08-16

`views[].files.{normal,medium,screen,thumbnail}` are generated renditions,
re-encoded and downsized. Authorized original/rendition pairs established:

- **PDF pages are rewritten** from archive `.pdf` entries to CDN
  `normal.jpg` renditions. Tested image formats retained their extension. PDF
  pages therefore require rendering before perceptual comparison.
- `normal` > `medium` > `screen` > `thumbnail` is a four-size ladder; even a
  220-px-wide original gets a `normal.jpg` — "normal" means "the largest
  rendition", not "the file uploaded".
- No API object carries a byte size or content hash, and the hexadecimal CDN
  path component is not the original SHA-1. Size or hash matching is therefore
  impossible from API data alone; §5 uses a `HEAD` per deposit.

### Public vs private renditions

- **Public** CDN rendition URLs can be fetched **anonymously** — but only with
  a TLS-fingerprint-impersonating client (`curl_cffi`/`wreq`, see §8); plain
  curl gets Cloudflare's `403`.
- **Private** rendition URLs answer `404` *even past the Cloudflare layer* —
  Geneanet hides the asset itself; the session cookie is mandatory, and the
  `api.geneanet.org` OAuth bearer is not accepted on `gw.geneanet.org`.

### 4b. The other two API surfaces (and why they change nothing)

**`api.geneanet.org` — the Geneanet Upload app's API** (full spec:
[Geneanet Upload API](geneanet-upload-api.md)). A real third surface, with the
one remaining programmatic login (embedded OAuth2 client, `password` grant, no
captcha) — and nothing this pipeline needs:

| | `api.geneanet.org` (upload app) | `www.geneanet.org/media/api` (this doc) |
|---|---|---|
| Auth | OAuth2 `Bearer`, password grant works | `gntsess5`/`REMEMBERME` cookie |
| Deposits | `GET /media/deposits.json` — 10/page, `204` past end | 100/page |
| References | one request per view | bulk pagination with `is_default`, which is absent from the app-API object |
| Originals | none — 8 candidate routes `404` | `/media/download/`, byte-identical |
| Sizes / hashes | none anywhere | `HEAD` → `Content-Length` |
| Tree | **write-only** (upload; no export route) | — |

**`geneweb-plugin-api` on `gw.geneanet.org`** — Geneanet's own open-source
GeneWeb plugin. Invocation: `/<base>?m=<MODE>&data=<piqi payload>&input=json|pb|xml&output=json|pb|xml`
(`output` mandatory; proto2 schemas in the repo's `src/assets/*.proto`). Its
~50 modes include `API_ALL_PERSONS`/`API_ALL_FAMILIES` (paginated) — a full
tree-data export channel, should the `.gw` export endpoint ever break. It
still cannot help with media: one image string per person (§12), and
`gw.geneanet.org` sits behind the same Cloudflare fingerprinting as www.

## 5. Matching originals you already have

The data archive's filenames cannot be matched to deposits by name: they are
upload names, unrelated to the deposit title
(`<media title>` → `<original filename>`), and a large deposit's pages can use
unrelated sequential filenames.

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

Authorized validation produced mostly unique sizes. Every observed collision
was the same file uploaded more than once.

Two properties worth keeping when this is reimplemented: an entry is never
attached on a *probable* match, and a size clash is **detected** rather than
silently resolved.

### Site-free pre-matching by upload timestamp (verified 2026-08-16)

The archive can be pre-linked without a session, which shrinks the `HEAD` pass
to entries that still need verification. Authorized validation found a
bijection between archive entries and views:

- **Join key**: the ZIP entry's DOS datetime equals the deposit's
  `date_create` **to the minute** for most entries. Exceptions retain an
  original file modification time.
  The archive's global order tracks the deposit list in **reverse**, and a
  multi-page deposit's pages are **consecutive entries in page order**.
- **Method**: group entries and views by minute; equal-count groups pair by
  order; leftovers pair by elimination. Each pair gets an `exact` flag:
  singleton minutes and single-deposit groups are exact. Multi-deposit
  same-minute batches are order-only where a swap cannot be excluded locally;
  those entries are flagged and never silently trusted.
- **Rejected**: filename↔title similarity as a join — it correlates on this
  validation data but is not robust and would misattribute silently in the general
  case (same rule as above: detect clashes, never resolve them on probability).
- **Use**: exact pairs need no `HEAD` at all; only the order-only remainder
  does — or a pHash validation (below). On an account with no same-minute
  batches the whole matching phase becomes session-free.

> **pHash as a validator, not a matcher.** `imagehash.phash` of a downloaded
> `normal.jpg` vs its archive original gives Hamming distance **0** — it
> tolerates resize/re-encode/format change (render PDF pages first, e.g.
> `pdftoppm`). But it stays a *validation* of pairs proposed by something
> exact. Near-white administrative scans can collide perceptually, which is
> the same reason §5 rejects pHash as a primary matcher.

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

The comparison is one body-free `HEAD` per deposit against downloading the
entire archive. Once that archive is downloaded, it already contains every
original, which makes matching against a local copy pointless; that path is
the existing `fetch` workflow.

Entry order does track request order, but no entry carries a deposit id, so any
mapping would be positional and would drift on multi-page deposits, which
contribute one entry per page.

The manager UI also paginates, so its "select all" covers only the visible
page and a manual bulk download may produce several archives.

> **Why not a perceptual hash?**
> A pHash answers a harder question — matching *re-encoded* images — and answers
> it with a distance and a threshold. Here both sides are the same original, so
> byte equality is available and exact. Administrative pages from the same
> scanner can look perceptually alike, allowing pHash to misattribute silently.
> A size clash, by contrast, is detected and falls back to downloading.

### Multi-page deposits

Their archive has no `Content-Length`, so pages cannot be size-matched. The
default takes linked pages from their per-page `normal` rendition and reports
that the imported bytes are a rendition.

Fetching multi-page originals instead pulls each deposit archive and extracts
pages by position because archive entries retain page order. This is more
expensive but preserves readable document pages.

## 6. Folding the key

Geneanet folds names before writing a reference: lowercase, accents stripped,
and `_`, `-` and `'` all become spaces. The occurrence is left empty when zero.

| `.gw` | Geneanet ref |
|-------|--------------|
| `SURNAME_A Renée` | `surname_a\|renee\|` |
| `LE SURNAME Given_C_Given_D` | `le surname\|given_c given_d\|` |
| `<token-a>-<token-b>` | `<token a> <token b>` |
| `D'SURNAME_C` | `d surname c` |
| `SURNAME_B Charles.1` | `surname_b\|charles\|1` |

The hyphen and the apostrophe were each found by a failing join, not by
guessing — see `crates/oxidgene-geneanet/src/key.rs`, where both have a
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

## 7b. Where this lives

The pipeline is `crates/oxidgene-geneanet`, shared so the CLI and the app
cannot drift apart on the join, the key folding or the size matching:

| Module | What it holds |
|---|---|
| `model.rs` | The manifest and the live wire shapes, with a test pinning them |
| `key.rs` | Folding a `.gw` name into a Geneanet reference (§6) |
| `join.rs` | Attaching references to persons by position (§7) |
| `client.rs` | The HTTP client, for the headless path |
| `archive.rs` | Indexing data archives in place, by entry length (§5) |
| `media.rs` | Choosing between a local original and a download |
| `script.rs` | The scripts a *browser* runs — console (CLI) and IPC (app) |

`crates/oxidgene-api/src/service/geneanet.rs` keeps the write step, and
`apps/oxidgene-desktop/src/geneanet.rs` the login window. Nothing in this crate
speaks HTTP any more — see §8.

**The app takes the browser path, not the cookie path.** §8's `--cookie` flow
is the headless one. The desktop wizard opens a real login window and evaluates
`script.rs`'s IPC variants inside it, so the ~19 metadata requests and the
`HEAD` sizing pass are issued by a browser on the user's own session — which is
what §8's Cloudflare note says the honest answer is. The cookie is read out of
that window afterwards, and only for the downloads in §5, which have no browser
equivalent that would not push hundreds of megabytes through an IPC channel.

## 8. Running it

There is one way to run this, and it is the wizard: tree card `⋮` → Import →
**From Geneanet**. See [Import](ui-import.md) for the five
steps.

> **The CLI is gone (2026-08-18).** `oxidgene-cli geneanet-media` had six
> subcommands; three needed direct HTTP, which no longer works at all (below),
> and the other three — printing a console script, folding its output into a
> manifest, and reporting the join offline — were all superseded by the window.
> Removing it also took `client.rs` and `media.rs` out of this crate, and with
> them `reqwest`, so the server no longer links an HTTP client.

### Collect once, import many times

Step 3 is the only part that touches Geneanet, and the expensive half of it is
one `HEAD` per deposit — several hundred on a real account. So its output is
**saveable**: step 3 offers *Save this connection*, and *Load a saved
connection* takes it back. That means

- iterating on an import without asking Geneanet anything again,
- collecting on the machine with the browser and importing on the machine with
  the archives,
- and keeping the mapping, which is the one part §1 says no export can carry.

**One file, however far you got.** Saved during step 3 it carries the
collection and the deposit sizes, and a later import still opens the window at
step 4 to gather media. Saved after step 4 it carries the gathered media as
well, and importing it needs **no Geneanet connection at all** — which is what
makes a genuinely air-gapped import possible: collect on the machine that can
reach Geneanet, import on the one that holds the archives, or holds nothing.
The wizard reads what it was given and asks only for what is missing, so there
is one *Load* button and no format for the user to choose between.

**The container is a ZIP**, holding `session.json` and the media beside it as
the files they are. Base64 inside JSON was the obvious first shape and the
wrong one: it inflates binary by a third, and an account with no data archive
has every medium in there.

`session.json` *is* the collection JSON with the deposit sizes, the media names
and a format version added beside it.
[`BrowserCollection`](../../crates/oxidgene-geneanet/src/model.rs) ignores
fields it does not know, so unzipping a session gives you a file the manifest
builder reads unchanged — the mapping stays inspectable, which matters for the
one thing no export can carry.

A bare JSON file loads too: sessions saved before the container changed, and
the raw output of a browser console script. The two are told apart by content
rather than by extension, because a renamed file is still the file it was.

### Pace

Requests go out one at a time. Parallelism provides little benefit and makes
traffic more likely to trigger automated-client detection.

The heavier pass is one `HEAD` per deposit for size matching. It is irreducible
because no endpoint reports deposit byte lengths in bulk, which is why the
result is persisted.

Whether scripted access fits Geneanet's terms of service is the operator's
call: the data is the account owner's own, fetched through the account's own
session, but that is not the same as a licence to automate.

### Cloudflare

geneanet.org sits behind Cloudflare, which can decide a non-browser client
deserves an interactive challenge. It answers `403` with `cf-mitigated:
challenge`, `server: cloudflare` and a "Just a moment…" HTML page.

**This looks exactly like an expired cookie and is not one.** The client
detects it and says so, because the obvious reaction — go fetch a fresh
cookie — fixes nothing. Observed directly: with the *same* cookie and the
*same* user agent, `curl` gets `200` while a Rust client is challenged. The cookie is irrelevant (full browser cookie and bare
`gntsess5` are challenged alike), and so is the user agent. What differs is the
TLS/HTTP2 fingerprint of the stack — rustls + hyper against curl's OpenSSL.

The challenge is adaptive: the same binary completed a full manifest earlier
the same day, then began to be challenged after sustained requests from the
same address.

**What the client does about it (settled 2026-08-17).** This section has now
said three different things, so here is the measurement that ends it: **no
direct download succeeds**. Every request from an HTTP client is challenged,
whatever cookie it presents and whatever the stack.

A browser-impersonating transport (`wreq` + `wreq-util`, pinned to a
current-Chrome profile) was tried and **removed**. It worked — and it was the
wrong trade twice over. It only worked while the pinned profile stayed current
(a Chrome 131 profile was already being challenged), so it was a treadmill with
a silent failure mode; and it dragged a BoringSSL toolchain, a `bindgen` build
and a patched, vendored copy of `tungstenite` through the whole workspace to
resolve a linker clash it had caused.

**Every request now goes through the login window**, media as well as metadata.
That is not an optimisation and not a preference — it is the only place the
bytes can come from. A real browser engine, on the user's own session, against
their own data, is what the check is asking for rather than a way around it.
See [Import §9.5](ui-import.md).

This is why there is no headless path left at all, and why the CLI was
removed rather than kept as a fallback: a fallback that cannot fetch a single
byte is not one.

The metadata phase is cheap enough to be largely out of reach of this — 19
requests, issued inside the webview anyway — and the resulting manifest is
reusable indefinitely, so it need only succeed once.

## 9. Output

**A tree.** The `.gw` goes through the existing `import_geneweb` →
`persist_import_result` path, and the manifest then produces `Media` rows plus
one `MediaLink` per person the photo is attached to.

A photo shared by several people is **stored once** with several `MediaLink`
rows — precisely what the original export could not express, and what
`MediaLink`'s shape was already built for. `person.portrait_media_id` carries Geneanet's
`is_default` flag, so the portrait stays the portrait.

> **Correction (2026-08-18): there is no `is_default`.** Walking a real
> `/media/api/references` payload, a reference carries `event`, `face`,
> `firstname`, `id`, `lastname`, `reference_extra_geneweb` and `thumb` —
> nothing that marks a portrait. The portrait is knowable from the *other*
> side: a `.gw` records it as `#image <url>`, and that URL is one of the
> renditions the collection lists, so matching the path (minus its `?t=` cache
> buster) says exactly which view a person's portrait is.
>
> That match does double duty. The `#image` URL would otherwise be imported as
> a *remote* medium — a dead link, since it 403s for anyone not signed in —
> sitting beside the stored copy of the same photo. The import now drops the
> remote row and points the person's portrait at the stored one instead, so a portrait
> appears once and shows up as the person's avatar.

Exporting that tree to `.gdz` afterwards is a separate, already-supported
operation.

References without a GeneWeb key identify people outside the imported tree and
cannot be joined. They remain explicit unresolved references.

## 10. Limits

- **"Unlinked" commonly means "an interior page", not "a forgotten photo".**
  Authorized validation found unlinked views inside scanned dossiers whose
  cover page was linked. The wizard therefore imports the whole multi-page
  deposit when any view is linked, using `media.is_document`,
  `parent_media_id`, and `page_index`.
- **Multi-page pages are downsized by default.** See §5.
- **No API exposes original filenames, byte sizes, or content hashes** — not
  the website API, not the upload app's API (§4b). Sizes come from a `HEAD`
  per deposit; names exist only in the data archive. Matching is therefore
  positional/temporal (site-free) or byte-size (session) — never nominal.
- **Password login cannot be automated** (§3.3): reCAPTCHA on the modern form,
  the legacy endpoint dead. The login window and cookie import are the only
  session sources.
- **No incremental re-import.** Nothing here reconciles a second Geneanet
  export against a tree already imported. That is
  [Person Merge](ui-merge.md) territory.
- **The API is undocumented.** `crates/oxidgene-geneanet/src/model.rs` holds a
  test pinning the live wire shape; it is the first thing to fail if Geneanet
  reshapes the payloads.
- **Fallback if the deposits API disappears.** Each `?type=fiche` page embeds
  the same data as a `gntGeneweb.media` JSON blob in a `<script>` — but that is
  one fetch per person instead of a small number of paginated API calls.

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
