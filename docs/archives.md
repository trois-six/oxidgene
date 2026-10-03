---
type: "Integration Specification"
title: "Archive Portals — Resolving a Cited Source to Its Image"
description: "Planned oxidgene-archives crate that resolves a cited source to the archive portal page showing its image: the per-country catalogue of national, regional, departmental, cantonal and municipal archives, one adapter per portal platform shared by every archive running it, citation parsing, the resolution contract, display in the portal or in OxidGene's own viewer over IIIF, attaching cited views as a remote multi-page document that can be cropped, caching, access etiquette, testing, delivery phases, and a survey of the platforms behind French departmental portals."
tags: [oxidgene, specification, archives, sources, integration]
generated: { by: claude-code/claude-opus-5-5, at: 2026-10-03T14:32:54Z }
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
  - id: arkotheque-departmental
    title: "Arkothèque — references, departmental archives"
    url: "https://www.arkotheque.fr/references/archives-departementales"
  - id: naoned-references
    title: "Naoned — Mnesys users"
    url: "https://naoned.fr/references/"
  - id: ligeo-references
    title: "Ligeo (Boscop) — references"
    url: "https://ligeo.fr/references/"
  - id: ad21-legal
    title: "Archives de la Côte-d'Or — legal notice naming the EidoPolis Prismia host"
    url: "https://archinoe.net/v2/site/AD21/Mentions_legales"
  - id: ad47-portal
    title: "Archives de Lot-et-Garonne — new digitized-archives portal"
    url: "https://archivesdepartementales.lotetgaronne.fr/actualites/decouvrez-notre-nouveau-portail-darchives-numerisees"
  - id: alsace-portal
    title: "Collectivité européenne d'Alsace — a new site for the Archives d'Alsace"
    url: "https://www.alsace.eu/actualites/un-nouveau-site-pour-archives-d-alsace"
  - id: ad37-credits
    title: "Archives d'Indre-et-Loire — credits"
    url: "https://archives.touraine.fr/page/credits"
  - id: panorama-2014
    title: "Petit panorama des interfaces des archives numérisées (2014)"
    url: "https://locomat.loria.fr/other/roegel2014panorama-archives.pdf"
  - id: anom-civil-status
    title: "Archives nationales d'outre-mer — civil status"
    url: "http://anom.archivesnationales.culture.gouv.fr/caomec2/"
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
| `jurisdiction` | Optional official codes of the area served, as a list: INSEE department or commune codes, Swiss canton abbreviations. A service serving several departments, such as Corsica's (2A and 2B), lists them all. |
| `citation_codes` | Uppercase codes a citation may start with, such as `["AD44"]`. Unique across the catalogue. |
| `website` | The archive's home page. |
| `collections` | The archive's searchable collections of registers, each with its own engine (below). Empty when no adapter exists yet. |
| `display` | `iiif` when OxidGene may show the archive's images in its own viewer and attach them as remote pages (§6.3, §6.4); `portal` when they are shown only in the portal's viewer. Default `portal`. |
| `attribution` | Credit the archive's reuse terms require, written in the archive's language with `{call_number}` and `{view}` placeholders, such as `Archives départementales d'Indre-et-Loire, {call_number}, vue {view}`. Required when `display` is `iiif`; never translated. |
| `terms` | Address of the archive's reuse terms. Required when `display` is `iiif`. |
| `citation` | Optional overrides of the citation grammar for this archive (§5.1). |
| `live_check` | `false` to exclude the archive from the scheduled live checks (§9.2). Default `true`. |

**Collections.** Many archives search their parish registers and their civil
status through different engines — two search pages, sometimes two
platforms, and the decennial tables often a third. One archive therefore has
one or more collections, each resolved on its own:

| Field | Rule |
|---|---|
| `id` | Slug unique within the archive: `parish-registers`, `civil-status`, `tables`. |
| `acts` | Act kinds the collection holds (§5.1): `B`, `M`, `S` for parish registers; `N`, `M`, `D` for civil status; table codes for tables. |
| `period` | Optional `[first year, last year]` the collection covers; either bound may be `null`. |
| `platform` | The adapter that searches it. |
| `portal` | The adapter's settings for this collection (§4.3, §4.4). |

Resolution picks the collections whose `acts` contain the citation's act and
whose `period` contains its year, and tries them in catalogue order until one
finds a register; a citation without a year tries every collection holding
its act. A Republican-calendar or 1792–1793 register, which may sit in either
collection, is found by the order alone. Where one engine serves every
register, as on the Loire-Atlantique portal, the archive has a single
collection with every act kind and no period.

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
which reports 29 departmental archive services among its references[^arkotheque-references]
but names 28[^arkotheque-departmental],
and **Mnesys**, by Naoned[^naoned]. One adapter per product, configured per
archive in the catalogue, therefore covers many archives at once. Two more
products, Ligeo and Archinoë / Prismia Vision, bring three to four adapters
to about two thirds of the departments ([§11](#11-french-departmental-portals)).

### 4.2 The adapter contract

```rust
pub trait Platform: Send + Sync {
    /// The catalogue value of `platform` this adapter answers to.
    fn id(&self) -> &'static str;
    /// Rejects a collection's `portal` object it cannot use, at load time.
    fn validate(&self, portal: &serde_json::Value) -> Result<(), CatalogError>;
    /// The collection's filtered search page, built without any request.
    fn results_url(&self, collection: &Collection, citation: &CitationParts) -> Option<Url>;
    /// Resolves parsed citation parts to a target in this collection.
    async fn resolve(
        &self,
        archive: &Archive,
        collection: &Collection,
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
- Catalogue tests check unique ids and citation codes, that every
  collection's `platform` has an adapter, and that every adapter accepts its
  collections' `portal`.

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
renumbering its registers. For each collection of each archive, in order:

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
update. Fixing a drift updates the collection's `portal` settings, or the
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
3. Add the Ligeo adapter, then Archinoë / Prismia Vision, and catalogue the
   departmental archives running the four platforms in the order of §11.4,
   then municipal and Swiss cantonal archives.

## 11. French departmental portals

A survey of the 101 departments, dated 2026-10-03, guides which adapters to
build and which archives to catalogue. It is a starting point, not a
catalogue: an archive enters the catalogue only after its portal has been
opened and its platform confirmed from the portal itself (§11.4).

### 11.1 Platforms

| Platform | Vendor | Departments | Evidence |
|---|---|---|---|
| Arkothèque | 1 égal 2 (Marseille); some finding aids by Anaphore | 28 named: 03, 04, 08, 10, 15, 18, 23, 24, 28, 36, 38, 40, 43, 44, 45, 46, 49, 50, 54, 65, 71, 72, 75, 78, 83, 85, 87, 94 | Strong: the vendor's departmental references page[^arkotheque-departmental], which claims 29 services; another of its pages says 28 |
| Mnesys Expo | Naoned (Nantes) | 18 confirmed portals: 14, 19, 25, 26, 27, 37, 39, 51, 55, 58, 59, 68, 69, 73 (older interface), 80, 90, 91, 972. Mnesys customers whose portal runs another platform: 18, 34, 93 | Strong (`/search/form/<uuid>` entry points, Mnesys Expo logos, case studies)[^naoned-references] |
| Ligeo Diffusion | Boscop (Angers) | 29 confirmed portals: 01, 02, 05, 06, 07, 12, 13, 16, 29, 31, 33, 34, 41, 42, 48, 56, 57, 63, 67, 70, 74, 76, 79, 86, 88, 89, 92, 93, 95 | Strong (URL pattern of the civil-status page, or announcement)[^ligeo-references] |
| Archinoë / Prismia Vision | EidoPolis (Laval) | 17, 21, 47, 60, 62; a viewer for 49; former pages for 07 | URL pattern, legal notice (21)[^ad21-legal], announcement (47)[^ad47-portal] |
| Archives nationales d'outre-mer | National service | 973, 974, 976 | `caomec2` civil-status search[^anom-civil-status] |
| Unidentified | — | 2A/2B, 09, 11, 22, 30, 32, 35, 38, 52, 53, 61, 64, 66, 81, 82, 84, 971; 77 disputed | Several share an engine (§11.2) |

Vendor counts disagree with their own lists and are orders of magnitude.
Naoned's "22 departmental archives" counts users of Mnesys Archives, the
management software, not of the Mnesys Expo portal; Ligeo's references mix
its management and dissemination products. Several departments appear with
two vendors for that reason, one for internal management and one for the
public portal:

- Cher (18): Arkothèque and Naoned.
- Manche (50) and Creuse (23): Arkothèque and Ligeo.
- Sarthe (72): Arkothèque portal, Ligeo management.
- Vendée (85): Ligeo management; the former portal was hosted by Mazedia,
  the current one runs Arkothèque.
- Maine-et-Loire (49): an Arkothèque site, with a civil-status viewer on
  `archinoe.fr/v2/ad49`.
- Ardèche (07): civil status on Ligeo, while older consultation pages remain
  on `archinoe.net/site/AD07`.
- Hérault (34) and Seine-Saint-Denis (93): Mnesys customers whose
  civil-status portals run Ligeo.
- Seine-et-Marne (77): Ligeo or Archinoë, unresolved.
- Isère (38): listed by Arkothèque, while its online civil status runs the
  `/mdr/` engine also used in Ariège.

### 11.2 Identifying a platform

Each platform leaves a distinctive address pattern, which identifies the
portal in seconds and is the evidence required before cataloguing:

| Platform | Pattern |
|---|---|
| Arkothèque | `?arko_default_…--ficheFocus=`, `arko_default_…` references |
| Mnesys Expo | `/search/form/<uuid>`, `/page/…`; a credits page naming Naoned[^ad37-credits]. The older Mnesys interface routes by query: `/?id=recherche_guidee_…`, `?doc=accounts/mnesys_…` |
| Ligeo Diffusion | `/archive/recherche/<search>/n:<id>`, `/archive/resultats/<search>/n:<id>`, `/archive/fonds/<finding aid>`, the same under `/archives/`, and `/n/<page>/n:<id>` |
| Archinoë / Prismia Vision | `archinoe.net/v2/adXX/`, `archinoe.com/v2/adXX/`, `archinoe.fr`, `/v2/adXX/registre.html` on the archive's own domain, `/console/ir_ead_visu.php`, `/console/ir_seriel.php`, `<department>.archives.prismia.fr` |
| Archives nationales d'outre-mer | `anom.archivesnationales.culture.gouv.fr/caomec2/recherche.php?territoire=<territory>` |
| Unidentified | THOT, `/Internet_THOT/FrmSommaireFrame.asp` or `/thot_internet/FrmSommaireFrame.asp` (Corsica, Ille-et-Vilaine); `/mdr/index.php/rechercheTheme/…` (Ariège, Isère, Pyrénées-Orientales); `/archives/classification-scheme` on an `earchives.` or `recherche-archives.` host (Gard, Tarn, Vaucluse), and `/archives/search/default/…` on the Guadeloupe `earchives.` host, probably the same engine; `/document/<finding aid>` on a `recherche.` host (Haute-Marne, Tarn-et-Garonne); `/archives-en-ligne/etat-civil-search-form.html` (Mayenne, Pyrénées-Atlantiques); `/EC/ecx/commune.aspx` (Côtes-d'Armor); `/archives_numerisees/portail/etats_civils/…` (Gers) |

### 11.3 Recent migrations and special cases

Portals change often, which is why every catalogued archive has a live check
(§9.1). Recent or announced changes:

- Lot-et-Garonne (47): Prismia Vision portal online since 2025-03-11, more
  than 2.4 million files[^ad47-portal].
- Haut-Rhin (68): Mnesys Expo since 2023-09-18; its credits page names
  Naoned.
- Bas-Rhin (67): `archives.bas-rhin.fr` replaced by `archives67.alsace.eu`,
  on Ligeo Diffusion, announced for November 2025; the two Alsace sites are
  to converge between 2027 and 2028[^alsace-portal].
- Lot (46): a new Arkothèque version in 2026. Haute-Marne (52): a new portal
  at the end of November 2025. Bouches-du-Rhône (13): a new site in May 2026,
  `www.archives13.fr`, still on Ligeo. Aisne (02) and Eure (27):
  redesigned. Var (83): a 2026 redesign is announced but unverified.
- Former Archinoë customers (53, 62, 79, 82, 86)[^panorama-2014] have moved
  in several directions: Pas-de-Calais is still on Archinoë, Deux-Sèvres and
  Vienne share a Ligeo portal, and Mayenne and Tarn-et-Garonne run engines
  shared with other departments but not yet identified.

Cases the catalogue model must express:

- **Corsica.** One service, the Archives de la Collectivité de Corse, serves
  both 2A and 2B: one catalogue entry carrying both citation codes, its
  `jurisdiction` listing both department codes.
- **Alsace.** One service, the Archives d'Alsace, with two technically
  distinct portals for 67 and 68 on different platforms: two catalogue
  entries, one per portal, under the same `name`.
- **Rhône.** One service shared with the Métropole de Lyon, while the city of
  Lyon publishes its own civil status: a municipal entry for Lyon beside the
  departmental one.
- **Bordeaux and Marseille.** Bordeaux's civil status is published by the
  Archives Bordeaux Métropole, and Marseille's is excluded from the new
  Bouches-du-Rhône portal: municipal entries again.
- **Deux-Sèvres and Vienne.** One shared portal,
  `archives-deux-sevres-vienne.fr`, serves both departments: one catalogue
  entry carrying both citation codes, as for Corsica.
- **Paris.** The department and the city are one; civil status before 1860
  is a reconstitution, published as a separate collection from the civil
  status from 1860 onwards: two collections (§3.1).
- **Overseas.** Civil status for Guyane (973), La Réunion (974) and Mayotte
  (976, 1844–1906) is consulted through the Archives nationales
  d'outre-mer[^anom-civil-status] rather than through the local service: one
  national-level entry whose search takes the territory as a parameter
  (`territoire=GUYANE`, `REUNION`, `MAYOTTE`), carrying the territories'
  citation codes.

### 11.4 Cataloguing order

1. Fetch each departmental site's home page and look for `arko`, `mnesys`,
   `ligeo`, `archinoe` and `prismia` (§11.2). This settles most unconfirmed
   rows and the disputed one (77).
2. Never rely on a vendor's reference list alone: it is self-declared, mixes
   management and dissemination, and lags. Cross-check it with the portal's
   footer or credits page.
3. Before cataloguing an archive, inventory every collection of registers
   its portal publishes — parish registers, civil status, decennial tables,
   reconstituted or duplicate series — from the portal's own navigation, not
   from the single entry point of §11.5, and give each searched through its
   own engine or form a collection with its acts and period. Each collection
   gets its live check (§9.1).
4. Catalogue the Arkothèque and Mnesys Expo archives first, then Ligeo, then
   Archinoë / Prismia Vision; then investigate the departments with no
   identified platform: 2A/2B, 09, 11, 22, 30, 32, 35, 38, 52, 53, 61, 64,
   66, 81, 82, 84, 971, and the Archives nationales d'outre-mer for 973, 974
   and 976. Engines shared by several of them (§11.2) come first.

### 11.5 Survey by department

The entry point is the civil-status or parish-register search page seen on
the department's own site or supplied with this specification. Each row
gives one entry point, usually the civil-status one, and lists a second only
where it was supplied (Paris, Tarn-et-Garonne). The table is therefore not an
inventory of collections: many departments publish parish registers, civil
status, decennial tables or reconstituted series through distinct engines or
search pages that are not listed here, and each needs its own collection in
the catalogue entry (§3.1). Platforms marked *probable* rest on a vendor list
only, and *unconfirmed* means no evidence was found.

| No. | Department | Site | Civil-status entry point | Portal platform | Evidence |
|---|---|---|---|---|---|
| 01 | Ain | `www.archives.ain.fr` | `www.archives.ain.fr/archive/recherche/etatcivil/n:88` | Ligeo | Ligeo references; URL pattern |
| 02 | Aisne | `archives.aisne.fr` | `archives.aisne.fr/archive/recherche/etatcivil/n:11` (FranceConnect for recent civil status) | Ligeo | Ligeo references; URL pattern; redesigned portal |
| 03 | Allier | `archives.allier.fr` | `archives.allier.fr/rechercher/archives-numerisees/genealogie-histoire-des-familles/etat-civil-en-ligne` | Arkothèque | Arkothèque references; portal |
| 04 | Alpes-de-Haute-Provence | `www.archives04.fr` | `www.archives04.fr/rechercher/archives-en-ligne/etat-civil/actes-etat-civil` | Arkothèque (since 2008) | Arkothèque references; portal |
| 05 | Hautes-Alpes | `archives.hautes-alpes.fr` | `archives.hautes-alpes.fr/archive/fonds/FRAD005_2E` | Ligeo | Ligeo references; URL pattern |
| 06 | Alpes-Maritimes | `archives06.fr` | `archives06.fr/archive/resultats/etatcivil2/n:101?type=etatcivil2` | Ligeo | URL pattern |
| 07 | Ardèche | `archives.ardeche.fr` | `archives.ardeche.fr/archive/recherche/etatcivil/n:96` | Ligeo (older pages on Archinoë) | Ligeo references; URL pattern |
| 08 | Ardennes | `archives.cd08.fr` | `archives.cd08.fr/archives-numerisees/sources-genealogiques/registres-paroissiaux-et-detat-civil` | Arkothèque | Arkothèque references; portal |
| 09 | Ariège | `mdr-archives.ariege.fr` | `mdr-archives.ariege.fr/mdr/index.php/rechercheTheme/requeteConstructor/1/1/R/0/0` | Unidentified (`/mdr/index.php/rechercheTheme/…` engine) | Portal |
| 10 | Aube | `www.archives-aube.fr` | `www.archives-aube.fr/actualites-1/les-actualites-anterieures-a-2010/validation-pages-1/etat-civil-des-communes-de-laube` | Arkothèque | Arkothèque references; portal |
| 11 | Aude | `archivesdepartementales.aude.fr` | `archivesdepartementales.aude.fr/letat-civil` | Unidentified | New portal reported by the press; portal |
| 12 | Aveyron | `archives.aveyron.fr` | `archives.aveyron.fr/archive/recherche/etatcivil/n:122` | Ligeo | Ligeo references; URL pattern |
| 13 | Bouches-du-Rhône | `www.archives13.fr` (new site, May 2026) | `www.archives13.fr/archive/recherche/etatcivil/n:64` | Ligeo | Ligeo references; URL pattern |
| 14 | Calvados | `archives.calvados.fr` | `archives.calvados.fr/search/form/ecf01748-923d-463a-8d80-bd4142582bcd` | Mnesys Expo | Mnesys Expo logo, Naoned case study; URL pattern |
| 15 | Cantal | `www.archives.cantal.fr` | `www.archives.cantal.fr/vos-archives/etat-civil/recherche-dans-letat-civil` | Arkothèque | Arkothèque references; portal |
| 16 | Charente | `lasource.archives.lacharente.fr` | `lasource.archives.lacharente.fr/archive/resultats/etatcivil/n:115?type=etatcivil` | Ligeo | Ligeo references; URL pattern |
| 17 | Charente-Maritime | — | `archinoe.com/v2/ad17/registre.html` | Archinoë | URL pattern |
| 18 | Cher | `www.archives18.fr` | `www.archives18.fr/archives-numerisees/registres-paroissiaux-et-etat-civil` | Arkothèque (portal); Naoned customer | Both vendors' references; portal |
| 19 | Corrèze | `www.archives.correze.fr` | `www.archives.correze.fr/search/form/3b1ba8cc-6c08-47cd-a90e-f9b231fdc30f` | Mnesys Expo | Mnesys Expo logo; URL pattern |
| 2A / 2B | Corse (Archives de la Collectivité de Corse) | `archives.isula.corsica` | `archives.isula.corsica/Internet_THOT/FrmSommaireFrame.asp` | Unidentified (THOT engine) | Single site since December 2020; portal |
| 21 | Côte-d'Or | `archives.cotedor.fr` | `archives.cotedor.fr/console/ir_ead_visu.php?eadid=FRAD021_000000912&ir=26564`; formerly `archinoe.fr/v2/site/AD21/Rechercher/Recherche_thematique/Genealogie` | Archinoë / Prismia | Legal notice: hosted by EidoPolis Prismia; URL pattern |
| 22 | Côtes-d'Armor | `archives.cotesdarmor.fr` | `sallevirtuelle.cotesdarmor.fr/EC/ecx/commune.aspx` | Unidentified (ASP.NET "salle virtuelle") | Portal |
| 23 | Creuse | `archives.creuse.fr` | `archives.creuse.fr/rechercher/archives-numerisees/registres-paroissiaux-et-de-letat-civil` | Arkothèque | Arkothèque references (also listed by Ligeo); portal |
| 24 | Dordogne | `archives.dordogne.fr` | `archives.dordogne.fr/archives-numerisees/genealogie/registres-paroissiaux-et-detat-civil` | Arkothèque | Arkothèque references; portal |
| 25 | Doubs | `portail-archives.doubs.fr` | `portail-archives.doubs.fr/search/form/4d44dde5-4523-4384-a2da-c1169870f1b2` | Mnesys Expo | Logo, Naoned case study; URL pattern |
| 26 | Drôme | `archives.ladrome.fr` | `archives.ladrome.fr/search/form/f6e7c1a1-9bda-40bc-a68b-13ed003eb0e5` | Mnesys Expo (since February 2020) | Naoned case study; URL pattern |
| 27 | Eure | `archives.eure.fr` | `archives.eure.fr/search/form/a3b9883f-0939-449a-bcbf-9de4c2d49b89` | Mnesys Expo | Naoned customer list; URL pattern |
| 28 | Eure-et-Loir | `archives28.fr` | `archives28.fr/archives-et-inventaires-en-ligne/histoire-des-individus-des-populations-et-genealogie/les-registres-paroissiaux-et-detat-civil` | Arkothèque | Arkothèque references; 2024 visual redesign; portal |
| 29 | Finistère | `archives.finistere.fr` | `recherche.archives.finistere.fr/archive/resultats/etatcivil/n:138?type=etatcivil` | Ligeo | Ligeo references; URL pattern |
| 30 | Gard | `archives.gard.fr` | `earchives.gard.fr/archives/classification-scheme` | Unidentified (`/archives/classification-scheme`) | Portal |
| 31 | Haute-Garonne | `archives.haute-garonne.fr` | `archives.haute-garonne.fr/archive/recherche/etatcivil/n:97` | Ligeo | Ligeo references; URL pattern |
| 32 | Gers | `www.archives32.fr` | `www.archives32.fr/archives_numerisees/portail/etats_civils/ec/recherche/` | Unidentified | Portal |
| 33 | Gironde | `archives.gironde.fr` | `archives.gironde.fr/archive/recherche/etatcivil/n:629` | Ligeo | Ligeo references; URL pattern; Bordeaux published by the Archives Bordeaux Métropole |
| 34 | Hérault | `archives-pierresvives.herault.fr` | `archives-pierresvives.herault.fr/archive/recherche/etatcivil/n:23` | Ligeo (portal); Mnesys customer | URL pattern; both vendors' lists |
| 35 | Ille-et-Vilaine | `archives.ille-et-vilaine.fr` | `archives-en-ligne.ille-et-vilaine.fr/thot_internet/FrmSommaireFrame.asp` | Unidentified (THOT engine, as in Corsica) | Portal |
| 36 | Indre | `www.archives36.fr` | `www.archives36.fr/fonds-numerises/etat-civil` | Arkothèque | Arkothèque references; portal |
| 37 | Indre-et-Loire | `archives.touraine.fr` | `archives.touraine.fr/search/form/e9414896-40cc-4ec3-936c-8acdfdb11770` | Mnesys Expo | Credits page; Naoned case study; observed (§4.4) |
| 38 | Isère | `archivesenligne.archives-isere.fr` | `archivesenligne.archives-isere.fr/mdr/index.php/rechercheTheme/` | Unidentified (`/mdr/` engine, as in Ariège); listed by Arkothèque | Arkothèque references; portal |
| 39 | Jura | `archives39.fr` | `archives39.fr/search/form/1eb1f0a3-b7ba-4c8a-bdae-395b322800e4` | Mnesys Expo | Naoned customer list; URL pattern |
| 40 | Landes | `archives.landes.fr` | `archives.landes.fr/faire-une-recherche/archives-numerisees/etat-civil` | Arkothèque (since 2012) | Arkothèque references; URL pattern |
| 41 | Loir-et-Cher | `www.archives41.fr` | `www.archives41.fr/archives/recherche/etatcivil` | Ligeo Diffusion within the Culture 41 portal | Ligeo references; URL pattern; new site in 2025 |
| 42 | Loire | `archives.loire.fr` | `archives.loire.fr/archive/recherche/etatcivil/n:92` | Ligeo (formerly Archinoë) | Press article; Ligeo references; URL pattern |
| 43 | Haute-Loire | `www.archives43.fr` | `www.archives43.fr/archives-en-ligne/familles-et-individus-en-haute-loire/etat-civil-de-la-haute-loire` | Arkothèque | Arkothèque references; URL pattern |
| 44 | Loire-Atlantique | — | `archives-numerisees.loire-atlantique.fr/chercher/etat-civil-et-registres-paroissiaux` | Arkothèque | Arkothèque references; observed (§4.3) |
| 45 | Loiret | `www.archives-loiret.fr` | `www.archives-loiret.fr/faire-vos-recherches/archives-numerisees/etat-civil` | Arkothèque | Arkothèque references; URL pattern |
| 46 | Lot | `archives.lot.fr` | `archives.lot.fr/recherche-en-ligne/archives-numerisees/registres-paroissiaux-et-detat-civil` | Arkothèque (new version in 2026; finding aids by Anaphore) | Press article; URL pattern |
| 47 | Lot-et-Garonne | `archivesdepartementales.lotetgaronne.fr` | `lotetgaronne.archives.prismia.fr/Recherche/Etat%20civil` | Prismia Vision (since 2025-03-11) | Department announcement; URL pattern |
| 48 | Lozère | `archives.lozere.fr` | `archives.lozere.fr/archive/recherche/etatcivil/n:88` | Ligeo | Ligeo references; URL pattern |
| 49 | Maine-et-Loire | `recherche-archives.maine-et-loire.fr` | `recherche-archives.maine-et-loire.fr/rechercher-et-consulter/archives-consultables-en-ligne/etat-civil-et-registres-paroissiaux` | Arkothèque (an Archinoë viewer exists) | Arkothèque references; URL pattern; `archinoe.fr/v2/ad49` |
| 50 | Manche | `www.archives-manche.fr` | `www.archives-manche.fr/recherche/registres-paroissiaux-et-detat-civil` | Arkothèque | Arkothèque references; URL pattern |
| 51 | Marne | `archives.marne.fr` | `archives.marne.fr/search/form/6977c5eb-072c-470c-8dfa-6d6488a2d71e` | Mnesys Expo | Mnesys Expo logo; URL pattern |
| 52 | Haute-Marne | `recherche.archives.haute-marne.fr` (new address since November 2025) | `recherche.archives.haute-marne.fr/document/FRAD052_00000001E` | Unidentified | Press article; portal |
| 53 | Mayenne | `archives.lamayenne.fr` | `archives.lamayenne.fr/archives-en-ligne/etat-civil-search-form.html` | Unidentified (Archinoë in 2014) | 2014 panorama; portal |
| 54 | Meurthe-et-Moselle | `archivesenligne.meurthe-et-moselle.fr` | `archivesenligne.meurthe-et-moselle.fr/archives-en-ligne/registres-paroissiaux-et-detat-civil` | Arkothèque | Arkothèque references; URL pattern |
| 55 | Meuse | `archives.meuse.fr` | `archives.meuse.fr/search/form/32239fba-c3ac-416c-b0f7-889cfa87214a` | Mnesys Expo | Logo, Naoned case study; URL pattern |
| 56 | Morbihan | `patrimoines-archives.morbihan.fr` | `rechercher.patrimoines-archives.morbihan.fr/archive/recherche/etatcivil/n:6` | Ligeo | Ligeo references; URL pattern |
| 57 | Moselle | `www.archives57.com` | `www.archives57.com/archives/fonds/FRAD057_605804` | Ligeo | Ligeo references; URL pattern |
| 58 | Nièvre | `archives.nievre.fr` | `archives.nievre.fr/search/form/9430efb3-399f-4de3-a3e7-004e232d8601` | Mnesys Expo | Naoned customer list; URL pattern |
| 59 | Nord | `archivesdepartementales.lenord.fr` | `archivesdepartementales.lenord.fr/search/form/dc4e871d-0b62-41fb-9921-5ded573781b8` | Mnesys Expo | Naoned case study; URL pattern |
| 60 | Oise | `archives.oise.fr` | `ressources.archives.oise.fr/v2/ad60/registre.html` | Archinoë | URL pattern |
| 61 | Orne | `archives.orne.fr` | `archives.orne.fr/etat-civil` | Unidentified | Portal |
| 62 | Pas-de-Calais | `www.archivespasdecalais.fr` | `archivesenligne.pasdecalais.fr/console/ir_seriel.php?id=56&p=formulaire_etat_civil` | Archinoë | 2014 panorama; URL pattern |
| 63 | Puy-de-Dôme | `www.archivesdepartementales.puy-de-dome.fr` | `www.archivesdepartementales.puy-de-dome.fr/archive/recherche/etatcivil/n:13` | Ligeo Diffusion (since 2001) | Ligeo references; archive's own account |
| 64 | Pyrénées-Atlantiques | `earchives.le64.fr` | `earchives.le64.fr/archives-en-ligne/etat-civil-search-form.html` | Unidentified (same engine as Mayenne) | Portal |
| 65 | Hautes-Pyrénées | `archivesenligne65.fr` | `archivesenligne65.fr/archives/acces-thematique/naitre-vivre-et-mourir/les-registres-detat-civil` | Arkothèque | Arkothèque references; URL pattern |
| 66 | Pyrénées-Orientales | `archives.cd66.fr` | `archives.cd66.fr/mdr/index.php/rechercheTheme/requeteConstructor/1/1/R/0/0` | Unidentified (`/mdr/` engine, as in Ariège) | Portal |
| 67 | Bas-Rhin (Archives d'Alsace) | `archives.alsace.eu` | `archives67.alsace.eu/archive/resultats/etatcivil/n:128?type=etatcivil` | Ligeo Diffusion (November 2025) | Collectivité européenne d'Alsace announcement; URL pattern |
| 68 | Haut-Rhin (Archives d'Alsace) | `archives.alsace.eu` | `archives68.alsace.eu/search/form/f4ed0a71-fe36-42d4-81bd-780da14c2125` | Mnesys Expo (since 2023-09-18) | Naoned case study; credits page; URL pattern |
| 69 | Rhône and Métropole de Lyon | `archives.rhone.fr` | `archives.rhone.fr/search/form/dde23a8f-cc71-4300-9805-bda67eec1ac0` | Mnesys Expo | Naoned case study; URL pattern |
| 70 | Haute-Saône | `archives.haute-saone.fr` | `archives.haute-saone.fr/archive/recherche/etatcivil2/n:119` | Ligeo Diffusion | Ligeo references; URL pattern |
| 71 | Saône-et-Loire | `www.archives71.fr` | `www.archives71.fr/consulter/en-ligne/familles-et-individus/etat-civil` | Arkothèque | Arkothèque references; URL pattern |
| 72 | Sarthe | `archives.sarthe.fr` | `archives.sarthe.fr/archives-en-ligne/registres-paroissiaux-etat-civil` | Arkothèque | Arkothèque references; observed (§4.3) |
| 73 | Savoie | `recherche-archives.savoie.fr` | `recherche-archives.savoie.fr/?id=recherche_guidee_etat_civil_web` | Mnesys (older interface) | Mnesys Expo logo; URL pattern |
| 74 | Haute-Savoie | `archives.hautesavoie.fr` | `archives.hautesavoie.fr/archive/recherche/etatcivil/n:139` | Ligeo | Ligeo references; URL pattern |
| 75 | Paris | `archives.paris.fr` | `archives.paris.fr/archives-numerisees/etat-civil-de-paris/etat-civil-reconstitue-xvie-1859` (reconstituted, 16th century–1859) and `archives.paris.fr/archives-numerisees/etat-civil-de-paris/etat-civil-a-partir-de-1860` | Arkothèque | Arkothèque references |
| 76 | Seine-Maritime | `www.archivesdepartementales76.net` | `www.archivesdepartementales76.net/archive/resultats/etatcivil/n:113?type=etatcivil` | Ligeo | Ligeo references; URL pattern |
| 77 | Seine-et-Marne | `archives.seine-et-marne.fr` | `archives.seine-et-marne.fr/fr/etat-civil` | Ligeo or Archinoë (disputed) | Portal |
| 78 | Yvelines | `archives.yvelines.fr` | `archives.yvelines.fr/rechercher/archives-en-ligne/registres-paroissiaux-et-detat-civil/registres-paroissiaux-et-detat-civil` | Arkothèque | Arkothèque references; URL pattern |
| 79 | Deux-Sèvres | `archives-deux-sevres-vienne.fr` (shared with Vienne) | `archives-deux-sevres-vienne.fr/archive/resultats/etatcivil/n:100?type=etatcivil` | Ligeo (Archinoë in 2014) | URL pattern |
| 80 | Somme | `archives.somme.fr` | `archives.somme.fr/search/form/cebd4a00-25b1-4b1c-a31e-8ea66d58efa2` | Mnesys Expo | Naoned customer list; URL pattern |
| 81 | Tarn | `archives.tarn.fr` | `recherche-archives.tarn.fr/archives/classification-scheme` | Unidentified (same engine as Gard and Vaucluse) | Portal |
| 82 | Tarn-et-Garonne | `recherche.archives82.fr` | `recherche.archives82.fr/document/FRAD082_IR_01051` and `recherche.archives82.fr/document/FRAD082_IR_00196`, two finding aids | Unidentified (same engine as Haute-Marne; Archinoë in 2014) | Portal |
| 83 | Var | `archives.var.fr` | `archives.var.fr/rechercher-dans-les-archives-numerisees-et-les-inventaires-5/registres-paroissiaux-et-de-letat-civil` | Arkothèque | Arkothèque references; URL pattern |
| 84 | Vaucluse | `earchives.vaucluse.fr` | `earchives.vaucluse.fr/archives/classification-scheme` | Unidentified (same engine as Gard and Tarn) | Portal |
| 85 | Vendée | `etatcivil-archives.vendee.fr` | `etatcivil-archives.vendee.fr/consulter/etat-civil-et-recensements/etat-civil` | Arkothèque (Ligeo for management) | Both vendors' references; URL pattern |
| 86 | Vienne | `archives-deux-sevres-vienne.fr` (shared with Deux-Sèvres) | `archives-deux-sevres-vienne.fr/archive/resultats/etatcivil/n:100?type=etatcivil` | Ligeo (Archinoë in 2014) | Shared portal; URL pattern |
| 87 | Haute-Vienne | `archives.haute-vienne.fr` | `archives.haute-vienne.fr/rechercher/archives-en-ligne/etat-civil` | Arkothèque | Arkothèque references; URL pattern |
| 88 | Vosges | `recherche-archives.vosges.fr` | `recherche-archives.vosges.fr/archive/recherche/etatcivil/n:2` | Ligeo | Ligeo references; URL pattern |
| 89 | Yonne | `archives.yonne.fr` | `archives.yonne.fr/archive/recherche/etatcivil/n:157` | Ligeo | Ligeo references; URL pattern |
| 90 | Territoire de Belfort | `archives.territoiredebelfort.fr` | `archives.territoiredebelfort.fr/search/form/ce8c3b7f-77f9-493b-81dd-d09d11bdc431` | Mnesys Expo | Naoned customer list; URL pattern |
| 91 | Essonne | `archives.essonne.fr` | `archives.essonne.fr/search/form/f282515f-9106-4c17-a633-95f49fe7da66` | Mnesys Expo | Mnesys Expo logo; URL pattern |
| 92 | Hauts-de-Seine | `archives.hauts-de-seine.fr` | `archives.hauts-de-seine.fr/n/archives-en-ligne/n:89` | Ligeo | Ligeo references; URL pattern |
| 93 | Seine-Saint-Denis | `archives.seinesaintdenis.fr` | `archives.seinesaintdenis.fr/archive/resultats/etatcivil/n:217?type=etatcivil` | Ligeo (portal); Mnesys customer | URL pattern; both vendors' lists |
| 94 | Val-de-Marne | `archives.valdemarne.fr` | `archives.valdemarne.fr/recherches/archives-en-ligne/etat-civil` | Arkothèque | Arkothèque references; URL pattern |
| 95 | Val-d'Oise | `archives.valdoise.fr` | `archives.valdoise.fr/archive/recherche/EtatCivilNumerise/n:419` | Ligeo | Ligeo references; URL pattern |
| 971 | Guadeloupe | `www.archivesguadeloupe.fr` | `earchives.archivesguadeloupe.fr/archives/search/default/*:*` | Unidentified (probably the Gard engine); listed by Ligeo | Ligeo references; portal |
| 972 | Martinique (Archives territoriales) | `www.patrimoines-martinique.org` | `www.patrimoines-martinique.org/search/form/8ea80f22-2f9c-456b-94a0-adbd50d31e1c` | Mnesys Expo | Naoned case study; URL pattern |
| 973 | Guyane (Archives territoriales) | `ctguyane.fr` | `anom.archivesnationales.culture.gouv.fr/caomec2/recherche.php?territoire=GUYANE` | Archives nationales d'outre-mer | Collectivité territoriale research guide; portal |
| 974 | La Réunion | `departement974.fr` (directory) | `anom.archivesnationales.culture.gouv.fr/caomec2/recherche.php?territoire=REUNION` | Archives nationales d'outre-mer | Portal |
| 976 | Mayotte | — | `anom.archivesnationales.culture.gouv.fr/caomec2/recherche.php?territoire=MAYOTTE` (1844–1906) | Archives nationales d'outre-mer | Portal |

Corsica has two department codes and one service, so the table has 100 rows
for 101 departments. *(directory)* marks a domain quoted by a third-party
directory and not checked on the site itself.

[^arkotheque]: Arkothèque, publishing software for archive services.
[^arkotheque-references]: Arkothèque references, departmental archive count.
[^naoned]: Naoned, Mnesys.
[^ad44-portal]: Loire-Atlantique archive portal, search and viewer requests observed 2026-10-03.
[^ad72-portal]: Sarthe archive portal, search, viewer and anti-bot challenge observed 2026-10-03.
[^ad37-portal]: Indre-et-Loire archive portal, search form, results, viewer and manifest observed 2026-10-03.
[^iiif-image]: IIIF Image API, image sources listed by the viewer endpoint.
[^iiif-presentation]: IIIF Presentation API 3.0, the register manifest.
[^arkotheque-departmental]: Arkothèque departmental references, 28 services named.
[^naoned-references]: Naoned customer map, mixing Mnesys Archives and Mnesys Expo users.
[^ligeo-references]: Ligeo references, mixing management and dissemination.
[^ad21-legal]: Côte-d'Or archive legal notice, hosting by EidoPolis Prismia.
[^ad47-portal]: Lot-et-Garonne announcement of its Prismia Vision portal, 2025-03-11.
[^alsace-portal]: Collectivité européenne d'Alsace, the Bas-Rhin portal on Ligeo Diffusion.
[^ad37-credits]: Indre-et-Loire credits page naming Naoned.
[^panorama-2014]: 2014 panorama of digitized-archive interfaces, former Archinoë customers.
[^anom-civil-status]: Archives nationales d'outre-mer, overseas civil status.
