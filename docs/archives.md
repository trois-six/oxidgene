---
type: "Integration Specification"
title: "Archive Portals — Resolving a Cited Source to Its Image"
description: "Planned oxidgene-archives crate that resolves a cited source to the archive portal page showing its image: the per-country catalogue of national, regional, departmental, cantonal and municipal archives, one adapter per portal platform shared by every archive running it, citation parsing, the resolution contract, display in the portal or in OxidGene's own viewer over IIIF, attaching cited views as a remote multi-page document that can be cropped, caching, access etiquette, testing, and delivery phases."
tags: [oxidgene, specification, archives, sources, integration]
generated: { by: claude-code/claude-opus-5-5, at: 2026-10-03T13:06:11Z }
sources:
  - id: arkotheque
    title: "Arkothèque, publishing software for archive services (1 égal 2)"
    url: "https://www.arkotheque.fr/"
  - id: arkotheque-references
    title: "Arkothèque — map of references"
    url: "https://www.arkotheque.fr/references/cartographie"
  - id: naoned
    title: "Naoned — Mnesys archive software"
    url: "https://www.naoned.fr/"
  - id: ad44-portal
    title: "Archives départementales de Loire-Atlantique — registres paroissiaux et d'état civil"
    url: "https://archives-numerisees.loire-atlantique.fr/chercher/etat-civil-et-registres-paroissiaux"
  - id: ad72-portal
    title: "Archives départementales de la Sarthe — registres paroissiaux et d'état civil"
    url: "https://archives.sarthe.fr/archives-en-ligne/registres-paroissiaux-etat-civil"
  - id: ad37-portal
    title: "Archives d'Indre-et-Loire — état civil, recherche ciblée"
    url: "https://archives.touraine.fr/search/form/e9414896-40cc-4ec3-936c-8acdfdb11770"
  - id: iiif-image
    title: "IIIF Image API 2.1"
    url: "https://iiif.io/api/image/2.1/"
  - id: iiif-presentation
    title: "IIIF Presentation API 3.0"
    url: "https://iiif.io/api/presentation/3.0/"
---

# Archive Portals — Resolving a Cited Source to Its Image

> Part of the [OxidGene Specifications](index.md).
> See also: [Person Profile](ui-person-profile.md#opening-a-cited-register) ·
> [Architecture](architecture.md) · [Data Model](data-model.md) (Source,
> Citation, Repository) · [Roadmap](roadmap.md#3b-active-archive-viewer)

---

## 1. Purpose

Most genealogical sources are pages of registers that public archives have
digitized and publish on their own portals. A citation that names the
register and the view should take the reader to that image in one click,
instead of leaving them to repeat the portal search by hand.

`oxidgene-archives` is the crate that knows the archives: which ones exist,
which portal software each runs, how to recognize a citation that belongs to
one, and how to turn that citation into the address of the cited image on the
portal. OxidGene uses it to display sources, either in the portal's own viewer
or, where the archive publishes its images over IIIF and its terms allow it,
in OxidGene's viewer. A reader who wants to keep the cited act attaches it as a
document whose pages are the archive's image addresses (§6.4). OxidGene never
copies, stores or redistributes the image bytes themselves.

This specification supersedes the split delivered first, where the citation
parser and the catalogue live in `oxidgene-ui::archive_viewer` and a driver
script per platform lives in the desktop binary
([Person Profile](ui-person-profile.md#opening-a-cited-register)). That split
drives each portal's own search form inside the archive window; this one
resolves the register through the portal's request interface in Rust, and
uses the window only to carry those requests where a portal demands a browser
and to display the result (§4.2, §6).

## 2. Scope

**In scope**

- A catalogue of archive services, organized by country and by level.
- One adapter per portal platform, shared by every archive that runs it.
- Parsing of normalized citations, configurable per archive.
- Resolution of a parsed citation to an [`ArchiveTarget`](#52-result): the
  portal page that shows the register, opened at the cited view when the
  platform allows it.
- The desktop archive window and the web fallback that display a target.
- Displaying a resolved view in OxidGene's shared media viewer over IIIF.
- Attaching one or several views, at the reader's explicit request, as a
  document of remote pages, then cropping it with the existing region tool.

**Out of scope**

- Downloading, caching or storing archive image bytes in OxidGene's media
  store or on the server.
- Attaching sources as media automatically: a source becomes a medium only
  when the reader asks for it.
- Transcription, indexing or full-text search of archive content.
- Crawling, bulk resolution, or prefetching of any kind (§8).
- Archives behind a login. A portal that requires an account is catalogued
  without an adapter until a later phase decides how to handle sessions.

## 3. Organization

### 3.1 Catalogue

The catalogue is data, not code: one JSON document per archive service under
`assets/archives/<country>/`, discovered at build time as the current
`assets/archives/*.json` documents are
([Architecture §7.1](architecture.md#71-embedded-data)). Adding an archive
that runs an already supported platform is a data change with no code.

| Field | Rule |
|---|---|
| `id` | Lowercase slug starting with the lowercase country code: `fr-ad44`, `fr-am-nantes`, `ch-ae-vd`. Unique. |
| `country` | ISO 3166-1 alpha-2 code; matches the directory. |
| `level` | `national`, `regional`, `departmental`, `cantonal`, `municipal`, or `other`. |
| `name` | The archive's own name, shown verbatim; never translated. |
| `jurisdiction` | Optional official code of the area served: INSEE department or commune code, Swiss canton abbreviation. |
| `citation_codes` | Uppercase codes a citation may start with, such as `["AD44"]`. Unique across the catalogue. |
| `website` | The archive's home page. |
| `platform` | The adapter that resolves its citations, or `null` when none exists yet. |
| `portal` | The adapter's settings for this archive (§4.3). Absent when `platform` is `null`. |
| `display` | `iiif` when OxidGene may show the archive's images in its own viewer and attach them as remote pages (§6.3, §6.4); `portal` when they are shown only in the portal's viewer. Default `portal`. |
| `attribution` | Credit the archive's reuse terms require, written in the archive's language with `{call_number}` and `{view}` placeholders, such as `Archives départementales d'Indre-et-Loire, {call_number}, vue {view}`. Required when `display` is `iiif`; never translated. |
| `terms` | Address of the archive's reuse terms. Required when `display` is `iiif`. |
| `citation` | Optional overrides of the citation grammar for this archive (§5.1). |
| `live_check` | `false` to exclude the archive from the scheduled live checks (§9.2). Default `true`. |

`display: "iiif"` is set only for an archive whose terms allow reuse with
attribution and whose images load across origins; the decision is recorded
with the archive, not inferred. An archive whose viewer requires accepting a
licence or passing an anti-bot challenge before showing an image stays
`portal`: the Sarthe archives are `portal`, the Indre-et-Loire and
Loire-Atlantique archives are candidates for `iiif` once their terms are
confirmed.

The hierarchy of the user's request — country, then level, then archive — is
the catalogue's navigation, not the crate's module tree: archives are data,
adapters are code, and an adapter serves archives of any level and country.

### 3.2 Crate layout

```text
crates/oxidgene-archives/
  src/
    lib.rs          ArchiveRegistry, Resolver, public types
    catalog.rs      Loading and validating the embedded catalogue
    citation.rs     Parsing citations into CitationParts
    platform/
      mod.rs        The Platform trait and the adapter registry
      arkotheque.rs Arkothèque (1 égal 2)
      mnesys.rs     Mnesys (Naoned)
    transport.rs    The PortalFetch trait and its native implementation
```

### 3.3 Dependencies

`oxidgene-archives` depends on `oxidgene-core` only, plus `serde`,
`serde_json` and, behind a feature, `reqwest` from the workspace. It has no
Dioxus and no SeaORM dependency, and compiles to `wasm32-unknown-unknown`
without the native transport, so `oxidgene-ui` may use its parser and
catalogue on the web build (§6.2). Responses are read with `serde_json` and a
few attribute scans of the portal's result markup; no HTML parser is added.

```text
oxidgene-archives  oxidgene-core
oxidgene-api       … , oxidgene-archives
oxidgene-ui        oxidgene-core, oxidgene-archives (catalogue and parser only)
oxidgene-desktop   … , oxidgene-archives
```

The `native` feature, enabled by `oxidgene-api`, provides the `reqwest`
transport. The desktop supplies a second transport through its archive window
(§4.2). Without either, the crate parses, matches, and builds offline targets
(§5.2) but cannot resolve a view.

## 4. Platforms

### 4.1 Why per platform

Departmental, regional and municipal archives rarely build their portals:
they license publishing software and configure it. Two products cover a large
share of the French departmental archives: **Arkothèque**, by 1 égal 2[^arkotheque],
which reports 29 departmental archive services among its references[^arkotheque-references],
and **Mnesys**, by Naoned[^naoned]. One adapter per product, configured per
archive in the catalogue, therefore covers many archives at once.

### 4.2 The adapter contract

```rust
pub trait Platform: Send + Sync {
    /// The catalogue value of `platform` this adapter answers to.
    fn id(&self) -> &'static str;
    /// Rejects a catalogue `portal` object it cannot use, at load time.
    fn validate(&self, portal: &serde_json::Value) -> Result<(), CatalogError>;
    /// The portal's filtered search page, built without any request.
    fn results_url(&self, archive: &Archive, citation: &CitationParts) -> Option<Url>;
    /// Resolves parsed citation parts to a target on this archive's portal.
    async fn resolve(
        &self,
        archive: &Archive,
        citation: &CitationParts,
        fetch: &dyn PortalFetch,
    ) -> Result<ArchiveTarget, ResolveError>;
}

/// One same-origin GET on the archive's portal, returning the body.
pub trait PortalFetch: Send + Sync {
    async fn get(&self, path_and_query: &str) -> Result<String, FetchError>;
}
```

An adapter issues only the requests it needs to find one register: no list
download beyond the search it performs, no image request.

**Two transports.** Some portals answer any HTTP client; others sit behind a
JavaScript anti-bot challenge that only a browser passes (the Sarthe portal
does[^ad72-portal]). The adapter logic is therefore independent of how a
request travels:

| Transport | Where | Used for |
|---|---|---|
| `native` | `oxidgene-api`, `reqwest` | Archives whose catalogue `transport` is `any`, on web and desktop. |
| `window` | The desktop archive window | Every archive on desktop, and the only one for `transport: "browser"`. |

The `window` transport runs each `get` as a same-origin `fetch` inside the
archive window once the portal page has loaded, and returns the body to Rust
through the window's IPC channel, which accepts messages from the archive's
`origin` only. Being the portal's own page, it passes the challenge and needs
no CORS. The resolution logic itself never runs in injected script.

### 4.3 Arkothèque

Observed on the Loire-Atlantique[^ad44-portal] and Sarthe[^ad72-portal]
portals. Each collection is served by a search engine (`moteur`) with a stable
unique reference per collection, per filter and per record; the two portals
expose the same request interface and routes, with their own references and
filters. The `portal` settings are:

| Setting | Content |
|---|---|
| `origin` | Portal origin. |
| `transport` | `any`, or `browser` when a challenge blocks other clients. |
| `search_path` | Path of the collection's search page. |
| `engine` | The engine's unique reference, such as `arko_default_…`. |
| `content_ids` | The collection's content identifiers. |
| `display_mode` | The list display mode reference. |
| `fields` | References of the `locality` filter and of the optional `period`, `act` and `parish` filters; a portal without a period filter (Sarthe) omits it. |
| `acts` | Optional map from act kind to the portal's category label, written as the portal writes it (`Baptèmes` on the Sarthe portal). |
| `locality_style` | How the portal writes a locality: `plain` (`Le Mans`), or `article_suffix` (`Mans (Le)`, Sarthe). |

Resolution:

1. `GET /_recherche-api/moteur` with `refUnique=<engine>`, the locality, and,
   when the portal has the filters, the period `<year>|<year>` and the act
   category. Each filter carries its `[op]=AND` and its `[extras][mode]`
   (`popup` for a list filter, `slider` for the period): without the mode the
   engine silently returns nothing. The locality is accepted by name, written
   in the portal's `locality_style`, without the portal's internal record key.
2. The response lists the matching registers, each titled by its **call
   number**, and an HTML rendering whose rows carry each register's viewer
   address (`/_recherche-api/visionneuse-infos/<engine>/<record>/<field>/image/<id>`)
   and its image count.
3. One register is selected among the rows by the citation parts it has,
   in order: call number, act kind, parish, the period written in the row
   (needed where the portal has no period filter), and image count equal to
   the cited view count. Selection stops at the first criterion that leaves
   exactly one row; criteria the citation lacks are skipped.
4. The target is the record page opened on the view:
   `<search_path>?detail=<record id>#visionneuse-manual|<viewer address>/<i>|0|<i>`,
   where `<i>` is the zero-based view index. Loading this address opens the
   portal's own viewer directly on that image, and a reload keeps it; this
   was checked on both portals (a view 42 of 187 on the Sarthe portal). A
   portal that shows a reuse licence first opens the view once the reader
   accepts it.

The viewer endpoint also lists, per image, an IIIF Image API source and a
persistent ARK address[^iiif-image]. The ARK is recorded in the target when
present, as the citation's durable link. For a `display: "iiif"` archive the
adapter also returns the image's IIIF service and reads its `info.json` for
the pixel size (§5.2): the service is level 2, so any size can be requested,
but it sends no CORS headers, so the size is read by the resolver rather than
by the client, which then loads images through plain `<img>` elements. The
images are served with `Cache-Control: public, max-age=864000`.

### 4.4 Mnesys

Observed on the Indre-et-Loire portal[^ad37-portal]. Its search forms are
server-rendered and answer a plain `GET`; records and images are addressed by
ARK. The `portal` settings are:

| Setting | Content |
|---|---|
| `origin` | Portal origin. |
| `transport` | `any` on the observed portal. |
| `form` | The search form's UUID. |
| `fields` | Names of the `locality`, `act`, `year`, and optional `parish` and `period_begin`/`period_end` inputs, such as `0-controlledAccessGeographicName[]`. |
| `locality_label` | The pattern of a locality value, such as `{locality} (Indre-et-Loire, France)`. |
| `acts` | Map from act kind to the portal's label (`Naissances`, `Baptêmes`, `Table décennale`…). |

Resolution:

1. `GET /search/results?formUuid=<form>&mode=list&sort=date_asc` with the
   locality label, the act label and the year. Values are the readable labels;
   no internal key is needed. A locality also matches the former localities
   merged into it, which the call number or the period then tells apart.
2. Each result row gives the register's title, period, **call number**
   (`6NUM8/003/050`), image count (`315 medias`), and its ARK
   (`/ark:/<naan>/<name>/<first image>`).
3. One register is selected as for Arkothèque (§4.3, step 3).
4. `GET /iiif/ark:/<naan>/<name>/manifest.json`, the register's IIIF
   Presentation 3 manifest[^iiif-presentation], lists every image in order;
   item `i` carries the image identifier in its id.
5. The target is that image's own ARK, `/ark:/<naan>/<name>/<image id>`, which
   opens the portal's viewer on it. Unlike Arkothèque, the view's address is a
   persistent identifier: it is also returned as `ark`.

The manifest also gives each image's pixel size and IIIF service, and the
portal sends CORS headers on the manifest, `info.json` and images. The service
is level 0 and lists only the full size (about 1 MB per view): a request for
any smaller size fails. The adapter therefore returns the portal's own
thumbnail, `/images/<image id>_thumbnail.jpg` (a few kilobytes), as the
view's thumbnail, and the full image as its picture.

The portal's viewer shows its reuse conditions first; the reader accepts them
in the window.

## 5. Contract

### 5.1 Citation parsing

The default grammar is the normalized form already delivered:

```text
<code> - <locality> - <parish> - <act> - <period> - <free…> - vue <n>[d|g]/<count>
```

`parse` returns `CitationParts` with every field optional except the code and
the locality:

| Part | Read from |
|---|---|
| `code` | First field, matched against `citation_codes`. |
| `locality` | Fields up to the act code; may itself contain ` - `. |
| `parish` | The field before the act, unless it is `(aucun)`. |
| `act` | The act code: `N`, `B`, `M`, `D`, `S`, and their combinations such as `BMS` or `NMD`; table codes such as `TB` are kept as tables. |
| `year` | The first year of the period field: `1877`, `1702-1703`, or a Republican year converted to its Gregorian start. |
| `period` | The period field as written, kept to match portals that list registers by period text. |
| `call_number` | A field shaped like a call number, compared without spaces or case: `3E73/14` matches `3 E 73 / 14`. |
| `views`, `side`, `view_count` | `vue <n>[d|g]/<count>`, or a range `vue <n>-<m>/<count>` for an act spanning several views, such as `vue 5d-6g/13`. |

The call number is **one criterion among several**, not a requirement: many
citations carry none, and resolution falls back to the act, the parish and
the view count (§4.3). A citation lacking a field that its archive's adapter
needs to find a single register resolves to the filtered search results
(`ArchiveTarget::Results`) rather than to an arbitrary register.

### 5.2 Result

```rust
pub enum ArchiveTarget {
    /// The cited views, or the register when the citation names no view.
    View {
        url: Url,
        views: Vec<ArchiveView>,
        view_count: Option<u16>,
        call_number: Option<String>,
        attribution: Option<String>,
    },
    /// Several or no registers matched: the portal's filtered results.
    Results { url: Url, matches: Option<usize> },
}

pub struct ArchiveView {
    /// One-based view number, as cited.
    pub view: u16,
    /// The portal page opened on this view.
    pub url: Url,
    /// The image's persistent address, when the portal publishes one.
    pub ark: Option<Url>,
    /// Present only for a `display: "iiif"` archive.
    pub image: Option<ArchiveImage>,
}

pub struct ArchiveImage {
    pub picture: Url,
    pub thumbnail: Url,
    pub width: u32,
    pub height: u32,
}
```

`views` holds every cited view, in order, so an act spanning views 5 and 6
resolves to both. `picture` is the address the viewer loads — a size bounded to
the screen where the service allows it, the full image otherwise — and
`thumbnail` the smallest address the archive serves. `attribution` is the
catalogue template filled with the call number and views.

`Results` built by `results_url` without any request has no match count; it
is what a client gets when no transport can reach the portal.

`ResolveError` distinguishes an archive without an adapter, a portal that did
not answer as expected, and a timeout; each has a stable code the interface
translates (`archive_viewer.*`).

### 5.3 API

The backend resolves archives whose transport is `any`; for a `browser`
archive it returns the offline `Results` target:

- REST: `POST /api/v1/trees/{tree_id}/sources/{source_id}/archive-target`
  with an optional `citation_id`, returning the `ArchiveTarget`.
- GraphQL: `Source.archiveTarget(citationId: ID)`.

Both surfaces share one service, validation and error mapping, and are tested
symmetrically ([API Contract](api.md)). The request sends the portal only the
locality, period, act and call number; never the person's name, the citation
text or the act number.

## 6. Display

### 6.1 Desktop

The archive window stays a top-level WebView, because portals forbid framing
(`frame-ancestors 'self'` on the Loire-Atlantique portal). The desktop
resolves every archive in it with the `window` transport: the window first
loads the archive's search page, the adapter's requests run in it (§4.2), and
the window then loads the target. It no longer fills fields or clicks
controls; the page's own scripts open the viewer at the view. The banner keeps
reporting resolution failures and the several-registers case.

The window uses a persistent web profile of its own, separate from the
application's, under the state directory (`archives-webview/`,
[Architecture §8.3](architecture.md#83-local-files)). Archive portals hold no
credential, and keeping their cookies spares the reader a reuse licence and an
anti-bot challenge at every opening.

### 6.2 Web

The web client resolves through the same backend endpoint and opens the
target in a new browser tab (`rel="noopener noreferrer"`). For a `browser`
archive the tab opens on the filtered search results, from which the reader
opens the register. The source becomes a link on both clients; only the
window type and the precision differ.

### 6.3 OxidGene's viewer

For a `display: "iiif"` archive, both clients open a resolved view in the
shared media viewer ([UI Common §4.5](ui-common.md#45-mediainput-mediagallery-and-documentform))
instead of the portal, as an unsaved document whose pages are the cited
views' `picture` addresses. The viewer pages through the cited views and
offers the previous and next views of the register, so a reader who finds
the act continues on the following image reaches it without leaving. The
attribution is shown under the image and links to the archive's `terms`, and
**Open on the archive's site** opens the view's portal page as §6.1 and §6.2
do. When the citation names a side (`d` or `g`), the viewer marks that half
of the double page.

Nothing is written while the reader only looks. Closing the viewer leaves no
record.

### 6.4 Attaching and cropping

**Attach as a document** in the viewer opens the canonical `DocumentForm`
([UI Common](ui-common.md#adding-a-document)) prefilled, and nothing is
written before the reader saves it:

- one remote page per cited view, in order, holding the view's `picture`
  address, its pixel size, and its thumbnail address; the reader removes
  views, or adds the previous or next view of the register, so a two-page act
  becomes one two-page document and a single page one single-page document;
- the title from the call number and views, and the description from the
  attribution, which the reader may edit;
- the kind of record from the act (`parish_record` or `civil_record`) and the
  medium `manuscript`;
- the event the citation documents, and the cited source as a link of the
  document, so the archive address can be resolved again from the citation if
  the portal moves its images.

The saved document is an ordinary media record: it appears in the galleries
of its persons, couple and event, and every viewer action applies to it.
Keeping only the act out of a double page or a crowded view is done with the
existing region tool ([Data Model](data-model.md#vignette)): a region drawn on
a remote page is stored as coordinates over the page and cut by the client,
so it needs none of the bytes; a region can serve as a portrait or be
attributed to a person like any other. When the citation names a side, the
region tool opens on that half of the view.

Attaching requires the remote page to record its thumbnail address, which the
data model does not yet have: phase 2 adds a nullable `thumbnail_url` to
`Media` for pages held only as a URL, used by gallery tiles instead of the
full picture ([Data Model](data-model.md#media)). Without it an Indre-et-Loire
tile would load a full view.

## 7. Errors and fallbacks

| Situation | Result |
|---|---|
| Unknown citation code, or no adapter | Plain text, no link. |
| No register, or several | `Results`: the filtered search page. |
| View beyond the register's image count | `View` with no views: the register's first image. |
| `iiif` image fails to load | The viewer shows the portal link in place of the picture. |
| Portal changed shape or timed out | Error banner; the window opens the archive's `website`. |

## 8. Access etiquette

Archive portals are public services whose terms OxidGene follows:

- One resolution per user click: no crawling, bulk resolution, background
  refresh or prefetching. Some portals, Loire-Atlantique included, publish a
  `robots.txt` that disallows automated agents, and some protect themselves
  with an anti-bot challenge; OxidGene acts only on a user's explicit request,
  as a browser does, and never works around a challenge outside the window
  the reader sees.
- Requests identify OxidGene in their `User-Agent`, time out after a short
  bound, and are never retried automatically.
- Resolved targets are cached in memory for the session. The only portal data
  written to the database is what the reader attaches (§6.4): image addresses,
  sizes and the attribution, never image bytes.
- Image bytes are cached only by the browser's or WebView's HTTP cache,
  under the archive's own cache headers. The server never fetches, proxies
  or stores them, and a cropped region is cut by the client from the
  archive's image.
- Images are shown in OxidGene only for a `display: "iiif"` archive, always
  with the archive's attribution and a link to its terms. An archive whose
  reuse terms require accepting a licence keeps that step in its own pages
  and is displayed by its portal.

## 9. Testing

- Unit tests parse anonymized citations of every catalogued grammar.
- Adapter tests replay recorded portal responses, anonymized and committed as
  fixtures, covering one match, several matches, no match, and a changed
  response shape.
- Catalogue tests check unique ids and citation codes, that every `platform`
  has an adapter, and that every adapter accepts its archives' `portal`.

`just check` never contacts a portal: the tests above run offline.

### 9.1 Live checks

Portals change without notice, most often when the archive installs a new
version of its vendor's software: renamed filter references, a different
result markup, a moved viewer route. Recorded fixtures cannot see that, so
every catalogued archive with an adapter has a live end-to-end check against
its real portal. It is opt-in, never run by `just check` or on a commit or
pull request, and runs on a schedule (§9.2).

The check builds its citation from the portal itself rather than from a
committed reference, so the repository holds no locality, call number or
view chosen from anyone's research, and the check survives an archive
renumbering its registers. For each archive, in order:

1. **Search page.** The `search_path` (or the search form) loads, and the
   engine, content, display-mode and filter references of the `portal`
   settings are still present in it.
2. **Discovery.** The first locality the portal's own locality filter lists,
   with the first act kind of `acts`, returns at least one register whose row
   yields a call number, an image count and a viewer address or ARK.
3. **Resolution.** A citation assembled from that register — its locality,
   act, year and call number, and a view in the middle of its image count —
   resolves to `View` with that call number and that view. The same citation
   without its call number resolves to the same register or to `Results`,
   never to another register.
4. **Opening.** The target loads in a browser and the portal's viewer shows
   the cited view: the view number displayed by the viewer equals the cited
   one after the reuse licence, if any, is accepted.
5. **Images**, for a `display: "iiif"` archive: the picture and the thumbnail
   answer with an image type, the pixel size matches the one resolved, and the
   attribution template fills without a placeholder left.

Steps 1 to 3 are Rust tests of `oxidgene-archives` marked `#[ignore]` over the
`native` transport. Steps 4 and 5, and steps 1 to 3 of a `transport:
"browser"` archive, run in a Playwright project of `e2e/` with its own
configuration, which loads the portal in Chromium and runs the adapter's
requests in the page, as the desktop window does. `just archives-live` runs
both for every archive, and `just archives-live <archive id>` for one.

Each archive ends in one of four outcomes:

| Outcome | Meaning | Run result |
|---|---|---|
| `ok` | Every step passed. | Pass |
| `drift` | The portal answered, but not as the adapter or its settings expect; the failing step and the expected and received shapes are reported. | Fail |
| `unreachable` | Timeout, network error or a `5xx` answer. | Warning; fail after two consecutive scheduled runs |
| `challenged` | An anti-bot challenge blocked the headless browser. | Warning, reported as unverified |

A check never solves or works around a challenge. Reports name the archive,
step, URL path and response shape, never response bodies beyond the fields
compared; failure artifacts (Playwright traces) hold portal pages only and
are kept for a short period.

### 9.2 Scheduled run

A dedicated workflow, `.github/workflows/archives.yml`, runs the live checks
every week and on demand, with an optional archive id as input. It is not
part of the nightly workflow, does not gate releases, and is not a required
status check: a portal change is not a defect of a commit. Archives run as
independent matrix entries without fail-fast, so one archive's drift does
not hide another's.

A `drift`, or an `unreachable` reaching its second run, opens an issue
labelled `archive-drift` for that archive, or comments on the open one; an
`ok` run closes it. The issue names the catalogue entry and the adapter to
update. Fixing a drift updates the archive's `portal` settings, or the
adapter and its recorded fixtures when the platform itself changed, so the
offline tests learn the new shape.

The checks follow §8: one archive at a time, sequential requests, a handful
per archive and run, the identifying `User-Agent` with the repository
address, and no retry within a run. A portal whose `robots.txt` disallows
automated agents, Loire-Atlantique included, is checked only at this weekly
rate; an archive that objects is marked `"live_check": false` in its
catalogue entry and relies on the user-reported failures of §7.

## 10. Delivery phases

1. Create `oxidgene-archives`; move the catalogue and the citation parser
   into it with the extended grammar (§5.1); implement the Arkothèque adapter
   with both transports; catalogue Loire-Atlantique and Sarthe; make the
   desktop window resolve and load targets; remove the injected driver
   script; add the live checks of both archives, `just archives-live` and the
   weekly workflow (§9.1, §9.2), and list them in
   [Development §2.7](development.md#27-test-categories).
   Every later archive or adapter arrives with its live check.
2. Add the Mnesys adapter with Indre-et-Loire; add the backend endpoint on
   both surfaces and open targets from the web client; add `display`, the
   IIIF view in the shared viewer, attaching views as a remote multi-page
   document, and `Media.thumbnail_url`.
3. Catalogue the other departmental archives running either platform, then
   municipal and Swiss cantonal archives.

[^arkotheque]: Arkothèque, publishing software for archive services.
[^arkotheque-references]: Arkothèque references, departmental archive count.
[^naoned]: Naoned, Mnesys.
[^ad44-portal]: Loire-Atlantique archive portal, search and viewer requests observed 2026-10-03.
[^ad72-portal]: Sarthe archive portal, search, viewer and anti-bot challenge observed 2026-10-03.
[^ad37-portal]: Indre-et-Loire archive portal, search form, results, viewer and manifest observed 2026-10-03.
[^iiif-image]: IIIF Image API, image sources listed by the viewer endpoint.
[^iiif-presentation]: IIIF Presentation API 3.0, the register manifest.
