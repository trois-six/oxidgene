---
type: "Integration Specification"
title: "Archive Portals — Resolving a Cited Source to Its Image"
description: "The oxidgene-archives crate, which resolves a cited source to the archive portal page showing its image: the per-country catalogue of national, regional, departmental, cantonal and municipal archives, one adapter per portal platform shared by every archive running it, citation recognition in any convention — the normalized form, the words of the source and citation read with vocabularies kept as data per language, repository records, the cited event and portal addresses — for acts, tables and other series (censuses, military registers, conscription lists, succession tables), the "Find in the archives" dialog completing a partial citation, the resolution contract, display in the portal's own viewer for every archive, IIIF used behind the scenes to attach cited views as a remote multi-page document that can be cropped, caching, access etiquette, testing, delivery phases, and a survey of the platforms behind French departmental portals."
tags: [oxidgene, specification, archives, sources, integration]
generated: { by: claude-code/claude-opus-5-5, at: 2026-10-08T15:00:00Z }
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
  - id: ligeo-portals
    title: "Ligeo Diffusion departmental portals (search, results, viewer, manifest, terms)"
    url: "https://www.archives.ain.fr/archive/recherche/etatcivil/n:88"
  - id: thot-portals
    title: "THOT portals of the Ille-et-Vilaine and Corsica archives"
    url: "https://archives-en-ligne.ille-et-vilaine.fr/thot_internet/FrmSommaireFrame.asp"
  - id: pleade-portals
    title: "Pleade portals of the Mayenne and Pyrénées-Atlantiques archives"
    url: "https://archives.lamayenne.fr/archives-en-ligne/etat-civil-search-form.html"
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
  - id: bach-portals
    title: "Bach (Anaphore) departmental portals (classification schemes, finding aids, viewer, robots.txt)"
    url: "https://earchives.vaucluse.fr/archives/classification-scheme"
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
portal. OxidGene uses it to open sources in the portal's own viewer, for
every archive alike. Where the archive publishes its images over IIIF and its
terms allow it, OxidGene can attach the cited act as a document whose pages
are the archive's image addresses (§6.3, §6.4) — implemented, but not offered
to the reader for now (§6.3). OxidGene never copies, stores or redistributes
the image bytes themselves.

The catalogue and the citation recognizer live in this crate, which the interface
uses on both clients. The register is resolved through the portal's request
interface in Rust; the desktop's archive window only carries those requests,
since some portals demand a browser, and displays the result (§4.2, §6;
[Person Profile](ui-person-profile.md#opening-a-cited-register)).

## 2. Scope

**In scope**

- A catalogue of archive services, organized by country and by level.
- One adapter per portal platform, shared by every archive that runs it.
- Recognizing citations written in any convention, from the source, the
  citation, its repositories and the cited event, with the words of each
  language and the conventions of each country as data (§5.1); French is
  the one shipped.
- Resolution of a parsed citation to an [`ArchiveTarget`](#52-result): the
  portal page that shows the register, opened at the cited view when the
  platform allows it.
- The desktop archive window and the web tab that display a target.
- Attaching one or several views, at the reader's explicit request, as a
  document of remote pages, then cropping it with the existing region tool;
  implemented, its offer disabled for now (§6.3).

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
`assets/archives/<country>/`, discovered at build time by the crate's build
script and embedded as text
([Architecture §7.1](architecture.md#71-embedded-data)). Adding an archive
that runs an already supported platform is a data change with no code.

| Field | Rule |
|---|---|
| `id` | Lowercase slug starting with the lowercase country code: `fr-ad44`, `fr-am-nantes`, `ch-ae-vd`. Unique. |
| `country` | ISO 3166-1 alpha-2 code; matches the directory. |
| `level` | `national`, `regional`, `departmental`, `cantonal`, `municipal`, or `other`. |
| `name` | The archive's own name, shown verbatim; never translated. |
| `jurisdiction` | Optional official codes of the area served, as a list: INSEE department or commune codes, Swiss canton abbreviations. A service serving several departments, such as Corsica's (2A and 2B), lists them all. Citations name the archive by them (`AD 72`, `Exampleville (72)`), and an official code of a place starting with one names it too (§5.1). |
| `areas` | Optional names of the area served, today's and former ones, as citations and place names write them: `["Loire-Atlantique", "Loire-Inférieure"]`. Citations name the archive by them (`Archives départementales de la Loire-Inférieure`), and a place among whose parts one stands is in the area (§5.1). |
| `aliases` | Optional other names and abbreviations the archive goes by, beside `name`, such as `ADLA`. |
| `citation_codes` | Uppercase codes a citation may start with, such as `["AD44"]`. Unique across the catalogue. |
| `website` | The archive's home page. |
| `collections` | The archive's searchable collections of registers, each with its own engine (below). Empty when no adapter exists yet. |
| `display` | `iiif` when OxidGene may use the archive's images over IIIF to attach cited views as remote pages (§6.3, §6.4) — an offer disabled for now, the value kept as the record of the archive's reuse terms —; `portal` otherwise. Either way the archive opens on its portal. Default `portal`. |
| `attribution` | Credit the archive's reuse terms require, written in the archive's language with `{call_number}` and `{view}` placeholders, such as `Archives départementales d'Indre-et-Loire, {call_number}, vue {view}`. Required when `display` is `iiif`; never translated. |
| `terms` | Address of the archive's reuse terms. Required when `display` is `iiif`. |
| `citation` | Optional overrides of the citation grammar for this archive (§5.1): `no_parish`, the parish values meaning none (default `["(aucun)"]`); `view_words`, the words introducing the views (default `["vue"]`); `series`, phrases naming a series added to the built-in vocabulary, by series code (`{"RP": ["dénombrement des habitants"]}`, `{"RI": ["C"]}` where a letter of the archive's own convention names its cemeteries' registers; default none); `districts`, the cities cited by their numbered districts, each `{"city", "count"}` (`[{"city": "Paris", "count": 20}]`, default none); `localities`, other names citations give a locality, by the name the archive's portal knows it by (`{"Ivry": ["Ivry-sur-Seine"]}`, default none); and `call_number_periods`, where the archive's call numbers write their register's period, as templates of text around a `{first}` year and an optional `{last}` one, four digits each (`["_RJ{first}{last}_"]` for `XXX_RJ19171918_04`, default none). A field left out keeps its default. |
| `live_check` | `false` to exclude the archive from the scheduled live checks (§9.2). Default `true`. |

**Collections.** Many archives search their parish registers and their civil
status through different engines — two search pages, sometimes two
platforms, and the decennial tables often a third — and publish other series
beside them: population censuses, military registers, conscription lists,
the registration offices' tables of successions, the cemeteries' burial
registers. One archive therefore has
one or more collections, each resolved on its own:

| Field | Rule |
|---|---|
| `id` | Slug unique within the archive: `parish-registers`, `civil-status`, `tables`, `censuses`, `military-registers`. |
| `acts` | The document kinds the collection holds, by code (§5.1): act codes `B`, `M`, `S` for parish registers and `N`, `M`, `D` for civil status; table codes such as `TD` for tables; series codes for the other series (below). A collection holds a combined act (`BMS`) when it holds each of its kinds, and publications of banns (`P`) where it holds marriages, with which registers file them. |
| `period` | Optional `[first year, last year]` the collection covers; either bound may be `null`. |
| `platform` | The adapter that searches it. |
| `portal` | The adapter's settings for this collection (§4.3, §4.4, §4.5). |
| `insecure_http` | `true` for a portal that answers over plain `http` only: refused for an archive the exceptions below do not name, and on a portal whose origins are all `https`. Default `false`. |

The series codes, each a kind of document a collection may hold:

| Code | Series | French archive series | Searched by |
|---|---|---|---|
| `RP` | Population censuses, the nominative lists | M (`6 M`) | Commune and year |
| `RM` | Military registers (*registres matricules*) | R (`1 R`) | Recruitment bureau, class year, matricule number |
| `CM` | Conscription lists: conscripts, the contingent, the drawing of lots, the mobile national guard | R | Canton or bureau, class year |
| `TSA` | Tables of successions and absences | Q, registration (`3 Q`) | Registration office, period |
| `RI` | Daily burial registers of cemeteries (*registres journaliers d'inhumation*), each burial under its entry number (*n° d'ordre*) | Municipal, the cemeteries' service | Cemetery, year, entry number |

A series register has no parish. The decennial tables and the parish
tables keep their table codes (`TD`, `TB`, `TM`, `TS`, `TN`). An adapter
that cannot search a series yet refuses, when the catalogue loads, a
collection holding one (§4.2), so a series is offered as a link only where a
search can follow it.

Resolution picks the collections whose `acts` contain the citation's act and
whose `period` contains its year, and tries them in catalogue order until one
finds a register; a citation without a year tries every collection holding
its act, and so does a citation whose year no collection's period contains,
so that a wrong or approximate year still reaches a register. A
Republican-calendar or 1792–1793 register, which may sit in either
collection, is found by the order alone. Where one engine serves every
register, as on the Loire-Atlantique portal, the archive has a single
collection with every act kind and no period.

**Plain `http`.** Every portal origin is `https`, which each adapter's
settings require, but for the archives this list names, with why; the
catalogue refuses `insecure_http` for any other (`PLAIN_HTTP_ARCHIVES` in
`catalog.rs` holds the same list):

| Archive | Why plain `http` |
|---|---|
| `fr-anom`, Archives nationales d'outre-mer | Its `https` port offers TLS 1.0 alone, with a self-signed certificate of another host that expired in 2019 (2026-10-06), which no client accepts. The portal is reached only for public registers' listings, and a request names nothing but a territory, a commune and a year (§4.15). |

A collection marked `insecure_http` gives its endpoint the mark (§4.2), and
the transports reach plain `http` for that endpoint alone.

`display: "iiif"` is set only for an archive whose terms allow reuse with
attribution and whose images load across origins; the decision is recorded
with the archive, not inferred, and kept, with the live checks reading its
images, while the offer to attach is disabled (§6.3). Every collection of an `iiif` archive
answers any client (`transport: "any"`), since the backend resolves its
views for attaching on the web (§6.3); the catalogue refuses one with a `browser`
collection. An archive whose viewer requires accepting a
licence or passing an anti-bot challenge before showing an image stays
`portal`: the Sarthe, Calvados and Marne archives are `portal`, as are the
Hérault and Val-d'Oise archives, whose Ligeo viewer asks the reader to accept
a reuse licence (§4.5); the Indre-et-Loire archives are `iiif` under their
reuse terms, and the Loire-Atlantique archives are a candidate once their
terms are confirmed.

The hierarchy of the user's request — country, then level, then archive — is
the catalogue's navigation, not the crate's module tree: archives are data,
adapters are code, and an adapter serves archives of any level and country.

### 3.2 Crate layout

```text
crates/oxidgene-archives/
  build.rs          Embeds assets/archives/<country>/*.json and
                    assets/citations/<language>.json
  src/
    lib.rs          ArchiveRegistry, Resolver, public types
    catalog.rs      Loading and validating the embedded catalogue
    citation.rs     The normalized form, read into CitationParts
    recognize/      Recognizing a citation in any convention (§5.1): lex.rs
                    cuts texts into segments and tokens, scan.rs reads their
                    words, mod.rs weighs the signals; tests.rs is the corpus
    vocabulary.rs   Loading the citation vocabularies
    platform/
      mod.rs        The Platform trait and the adapter registry
      query.rs      Percent-encoded query strings
      markup.rs     Attribute and text scans of portal markup, folding
      locality.rs   The forms in which a portal may write a cited locality, and its other names (§5.1)
      select.rs     Choosing the cited register among search results (§4.3)
      iiif.rs       Reading an image service and building a view's image
      view.rs       The View target of a chosen register (§5.2, §7)
      arkotheque/   Arkothèque (1 égal 2) (§4.3); each adapter's live.rs is its live probe (§9.1)
      archinoe/     Archinoë (EidoPolis): `registre`, `seriel` and `ead` searches (§4.6)
      archives32/   The Gers portal's modules (§4.13)
      bach/         Bach (Anaphore): classification scheme, finding-aid trees, viewer (§4.11)
      caomec2/      The Archives nationales d'outre-mer's civil-status search (§4.15)
      gaia/         GAIA 9, the `/mdr/` search wizard (§4.8)
      ligeo/        Ligeo Diffusion (Boscop): settings, results, locality cells (§4.5)
      mnesys/       Mnesys Expo (Naoned) (§4.4)
      mnesys_inao/  The older Mnesys interface: guided searches, notices (§4.14)
      pleade/       Pleade (AJLSM): `form` and `tree` searches (§4.10)
      prismia/      Prismia Vision (EidoPolis) (§4.7)
      thot/         THOT, an ASP portal of search modules (§4.9)
      visualys/     Visualys, the Côtes-d'Armor "salle virtuelle" (§4.12)
    transport.rs    The request contract, the PortalFetch and PortalTransport
                    traits, and the native implementation
    live/           The live checks (feature `live`, §9.1): steps 1 to 3,
                    reports, and the bridge transport to a browser page
    bin/archives-live-bridge.rs
                    The live checks of browser-only collections (feature
                    `live`)
  tests/live.rs     The live checks over the native transport, `#[ignore]`d
  fixtures/<platform>/
                    Anonymized portal answers for the adapter tests (§9)
```

### 3.3 Dependencies

`oxidgene-archives` depends on `oxidgene-core` only, for the Republican
calendar, plus `serde`, `serde_json` and, behind a feature, `reqwest` from the
workspace. It has no
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

`oxidgene-api` enables the `native` feature, which provides the `reqwest`
transport, for the backend endpoint (§5.3); the desktop links it through the
API but resolves through its archive window, a second transport (§4.2). On
Linux the desktop also uses, directly, crates already in its tree: `webkit2gtk`
(through wry) for the window's TLS-error signal and per-host certificate
exception, and `rustls`, `rustls-native-certs` and `reqwest` (through
`reqwest`) to complete and verify a certificate chain (§6.1); they add no
crate to the lock file.
`oxidgene-ui` never enables it. Without a transport, the crate parses,
matches, and builds offline targets (§5.2) but cannot resolve a view.
The `live` feature, which no application enables, provides the live checks
of §9.1 with their bridge binary, and the Clippy matrix checks it on its own
([Development §2.8](development.md#28-guards)); their test over the native
transport takes `tokio`, the runtime `reqwest` needs, as a development
dependency.

Addresses are plain `String`s: the crate builds and compares them as text
and adds no URL library.

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
to about two thirds of the departments ([§11](#11-french-departmental-portals)),
and two engines each serve two archives: THOT (Ille-et-Vilaine, Corsica) and
Pleade (Mayenne, Pyrénées-Atlantiques). Some archives run software no other
departmental archive is known to run — the Côtes-d'Armor "salle virtuelle",
the Gers portal, the Savoie archives' older Mnesys interface —: each has an
adapter of its own, since a portal without one stays unreachable from a
citation.

### 4.2 The adapter contract

```rust
/// Boxed so that adapters and transports are object-safe.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait Platform: Send + Sync {
    /// The catalogue value of `platform` this adapter answers to.
    fn id(&self) -> &'static str;
    /// Rejects, at load time, a collection whose `portal` settings it cannot
    /// use or whose acts the settings do not search.
    fn validate(&self, collection: &Collection) -> Result<(), CatalogError>;
    /// The portal's origin, the further origins its pages call, the page a
    /// browser transport loads first, and whether the portal needs a browser.
    fn endpoint(&self, collection: &Collection) -> Option<PortalEndpoint>;
    /// The collection's filtered search page, built without any request.
    fn results_url(&self, collection: &Collection, citation: &CitationParts) -> Option<String>;
    /// The reuse licence the portal shows before every page of the
    /// collection, which OxidGene never accepts (§6.1); none by default.
    fn licence(&self, collection: &Collection) -> Option<Licence>;
    /// Resolves parsed citation parts to a target in this collection.
    fn resolve<'a>(
        &'a self,
        archive: &'a Archive,
        collection: &'a Collection,
        citation: &'a CitationParts,
        fetch: &'a dyn PortalFetch,
    ) -> BoxFuture<'a, Result<ArchiveTarget, ResolveError>>;
}

pub struct PortalEndpoint {
    pub origin: String,
    pub other_origins: Vec<String>,
    pub start: String,
    pub access: Access, // `any`, `browser` or `page`: the settings' `transport`
    /// The collection's `insecure_http` (§3.1): plain `http` origins allowed.
    pub insecure_http: bool,
}

pub struct PortalRequest {
    pub method: Method, // `Get` or `Post`
    /// A path and query on the portal's origin, or an absolute address on
    /// one of the endpoint's other origins.
    pub url: String,
    /// `Accept`, `Content-Type` and `ApiKey` only.
    pub headers: Vec<(String, String)>,
    /// A form-encoded or JSON body, on a `POST` only.
    pub body: Option<String>,
}

/// Requests on one portal within one resolution, returning the body.
/// Cookies a response sets are sent back by the following requests.
pub trait PortalFetch: Send + Sync {
    fn request<'a>(&'a self, request: &'a PortalRequest)
        -> BoxFuture<'a, Result<String, FetchError>>;
    /// A `GET` of a path and query on the portal's origin.
    fn get<'a>(&'a self, path_and_query: &'a str) -> BoxFuture<'a, Result<String, FetchError>>;
    /// Loads a path and query on the portal's origin as the browser page
    /// itself, and returns its document as rendered once an element matches
    /// `ready`, a CSS selector. A browser transport's only; any other
    /// refuses it.
    fn page<'a>(&'a self, path_and_query: &'a str, ready: &'a str)
        -> BoxFuture<'a, Result<String, FetchError>>;
}

/// Opens fetchers bound to an endpoint's origins.
pub trait PortalTransport: Send + Sync {
    /// Whether requests run in a browser page, which passes a challenge.
    fn is_browser(&self) -> bool;
    fn connect<'a>(
        &'a self,
        endpoint: &'a PortalEndpoint,
    ) -> BoxFuture<'a, Result<Box<dyn PortalFetch + 'a>, FetchError>>;
}
```

The futures are boxed because a trait's `async fn` cannot be called through
`dyn`, and are `Send` on every target: the resolver runs in the server's
handlers and on the desktop's runtime, while the web build never resolves
and implements no transport. `get` is provided over `request`, and `page`
answers `NotAllowed` unless the transport loads pages.

Some portals need more than a `GET`: a search form that reads only a
form-encoded `POST` and a session cookie set by a first request, or a JSON
API on a second origin that admits the portal's origin only and wants a
public key in a header. An adapter declares such an origin in its settings,
and `endpoint` lists it in `other_origins`; a request addressed anywhere
else, carrying any other header, or with a body on a `GET` is refused before
it is sent. Every transport — the native one, the desktop window and the
live checks' bridge — refuses an endpoint whose origins or start page are
not `https` before anything leaves (`transport::check_scheme`), unless the
endpoint carries the catalogue's `insecure_http` mark (§3.1), which admits
plain `http` for that endpoint only. `FetchError` tells a timeout, a network failure (a closed window
included), an error status, an oversized body, a request or redirect leaving
the endpoint's origins, and a refused header, body or kind of request apart.

An adapter issues only the requests it needs to find one register: no list
download beyond the search it performs, no image request. A portal that
lists the cited locality under another name costs at most one more search,
and only when the cited name finds nothing (§5.1, *Other names of the
locality*).

`validate` also refuses a collection holding a series (§3.1) that the
adapter cannot search, naming the series code: Arkothèque, Mnesys, Ligeo,
GAIA, THOT, Pleade's `tree` mode, Bach, Visualys's `search` mode, the Gers
portal and the older Mnesys interface search series collections (§4.3,
§4.4, §4.5, §4.8, §4.9, §4.10, §4.11, §4.12, §4.13, §4.14); Archinoë,
Prismia Vision, Pleade's `form` mode, Visualys's `localities` mode and
CAOMEC2 (§4.15), whose observed searches serve acts and tables only, refuse
them until a portal's series search has been observed.

**Two transports.** Some portals answer any HTTP client; others sit behind a
JavaScript anti-bot challenge that only a browser passes (the Sarthe portal
does[^ad72-portal]). The adapter logic is therefore independent of how a
request travels:

| Transport | Where | Used for |
|---|---|---|
| `native` | `oxidgene-api`, `reqwest` | Archives whose catalogue `transport` is `any`, on web and desktop. |
| `window` | The desktop archive window | Every archive on desktop, and the only one for `transport: "browser"` and `"page"`. |

The `native` transport sends the identifying `User-Agent` (§8), follows at
most five redirects itself so that each hop's address is checked against
the endpoint's origins — a relative `Location` resolved in the current
address's directory, as a browser resolves it (THOT, §4.9) —, and keeps
the cookies responses set, by origin, in a
jar that lives as long as the fetcher: one resolution. The jar keeps names
and values only — within one resolution on a few origins of one portal, the
domain, path and expiry attributes have nothing to separate — and forgets a
cookie set empty or with `Max-Age=0`. It opens a fresh connection for each
request: the Pas-de-Calais portal answers `HTTP/1.0` with `Connection:
Keep-Alive`, then drops a reused connection in the middle of its next
answer, and a resolution's handful of requests gains little from reuse.

The `window` transport ([§6.1](#61-desktop)) first loads the endpoint's
`start` page, a page of the portal's origin, and waits until the window
shows that page rather than an anti-bot check. The adapter chooses the
lightest page that passes the portal's checks — Arkothèque's `/robots.txt`
(§4.3) —, since the requests only need a page of the origin to carry them,
and a search page fetching its own lists on load delays them. It then runs each
request as that page's `fetch`, with the portal's cookies
(`credentials: "same-origin"`), so it passes the challenge, needs no CORS on the
portal's origin, and reaches a declared API origin whose CORS admits the
portal. A request to that other origin carries no cookie: a browser refuses
a credentialed answer that admits any origin (`Access-Control-Allow-Origin:
*`), as the Bach viewers' image lists do (§4.11). The body returns to Rust through the window's IPC channel, which
accepts messages from the archive's `origin` only and matches each answer
to the request it was issued for; an answer whose final address left the
endpoint's origins is refused. The window's own `User-Agent` is the
WebView's. The resolution logic itself never runs in injected script.

**Page loads.** A portal may refuse even the requests a page's script
sends, its own `fetch` included, while it serves its pages and lets their
own scripts render them. Such a collection is `transport: "page"`
(`Access::Page`): only a browser reaches it, and only by loading its pages.
The adapter asks for a page rather than a request, `PortalFetch::page(path,
ready)` — for a search, the search page with the filters in its address,
the page a reader lands on —, and parses the markup it gets back in Rust,
as it parses an answer. The window loads that page as it loads the start
page, waiting out or leaving to the reader any anti-bot check on the way
(§6.1), then runs a script that waits, within the request bound, until an
element matching the `ready` selector is in the document — what the
portal's own scripts render once they have loaded their data — and posts
the document as rendered back over IPC under the request's ticket, checked
as a request's answer. The script reads the page only: it requests,
fills and clicks nothing. Such a portal is never sent a request: the window
refuses one before it is sent (`FetchError::NotAllowed`), since a refused
request may cost the session the pages that follow. The `native` transport
loads no page, so such a collection resolves in the desktop window, and
elsewhere to its filtered search page without a request, as a `browser`
one does. The live checks' bridge loads pages the same way (§9.1).

**Anti-bot pages.** One list of signatures,
`platform/challenges.json`, tells a portal's page from an anti-bot page, for
every path a page or an answer takes: the adapters' answer checks
(`markup::anti_bot`, behind `FetchError::Challenged` and
`markup::unreadable`), the desktop window's page classification, and the
live checks' Playwright side, which reads the same file. Each signature names
its vendor, whether it is a **challenge** — a check a browser passes — or a
**block** — a refusal nobody passes from that browser —, the lower-case
fragments the markup must all hold, and those that rule it out; the first
match wins, so a vendor's block is listed before its challenge. A list of
widget fragments (Turnstile, hCaptcha, reCAPTCHA) marks a check that asks
the reader to answer. A new vendor is one entry.

| Vendor | Challenge | Block |
|---|---|---|
| Cloudflare | `_cf_chl_opt`, or the title `Just a moment...` | `cf-error-details`, or `Sorry, you have been blocked` |
| Anubis | `anubis`, or `Making sure you` | a `/.within.website/` page with an error code and no `anubis_challenge` |
| F5 (TSPD, ASM) | `bobcmn`, or a `/TSPD/` script with `enable JavaScript` | `Request Rejected` |
| Bot-mitigation redirect (Arkothèque) | `bot_mitigation`, or `window.location.href='/redirect_` | — |
| Altcha | `altcha-widget` | — |
| Other WAFs | — | `Access Denied` |

A signature describes the check's own page, not a page behind it: F5 and
Cloudflare add their scripts (`/TSPD/…`, `/cdn-cgi/challenge-platform/…`)
to every page a browser reaches once it has passed, so those addresses alone
do not mark a check.

### 4.3 Arkothèque

Observed on 27 departmental portals (§11.5). Each collection is served by a
search engine (`moteur`) with a stable unique reference per collection, per
filter and per record; every portal exposes the same request interface and
routes, with its own references, filters and result columns. An engine may
serve several collections — its census lists and its military registers, or
a city's parishes listed apart (the Aube's Troyes) — and one collection's
document kinds may sit in several engines, each then its own collection. The
`portal` settings are:

| Setting | Content |
|---|---|
| `origin` | Portal origin. |
| `transport` | `any`; `browser` when a challenge blocks other clients; or `page` when the portal refuses even the requests a page's script sends, which is then searched by its pages (below). Default `any`. |
| `search_path` | Path of the collection's search page, where records and views open. |
| `engine` | The engine's unique reference, such as `arko_default_…`. |
| `content_ids` | The search component's numeric content identifiers. |
| `display_mode` | The list display mode reference. |
| `fields` | The engine's filters the search sends, each optional: `locality` (the commune, or for a series the recruitment or registration bureau, or a canton), `act`, and `period` (the year, or a military class). Each is a filter reference, or an object `{"ref", "mode", "keyed", "end", "span"}`: `mode`, the request's `[extras][mode]`, is `popup` for the locality, `select` for the act and `slider` (`<year>\|<year>`) for the period by default, `input` or `autocomplete` for a typed year (`<year>`); `keyed` means the filter takes its listed values with their record keys only; `end` names the second input of a period searched by its first and last year; `span` makes a slider search the cited period (`1917\|1918` for a register of `1917-1918`) rather than its first year, for an engine dating each part of a register by its own dates, which lists the parts after the first year only so. |
| `acts` | Map from document code to the act filter value or values, as the portal writes them: `Baptèmes[[arko_fiche_…]]` for a list value with its record key, without which a list matches nothing, or a plain text value (`Registre matricule`, `Baptêmes / Naissances`). Several values (the decennial tables of each kind, the typologies of a series) are searched together, any of them matching. Two codes may share a value (`Baptêmes / Naissances` for `B` and `N`). Empty for an engine without an act filter. Where the engine has an act filter, every kind the collection holds needs a value; a combined act (`BMS`) without its own entry is searched by its first kind, publications of banns as marriages. |
| `locality_style` | How the portal writes a locality (§3.2 `platform/locality.rs`): `plain` (`Le Mans`, default), `article_suffix` (`Mans (Le)`), `article_comma` (`Mans, Le`), `article_dash` (`MANS - LE`), `qualified` (`Le Mans (Sarthe, France)`: searched by the name alone, and a hamlet's `Hamlet (Commune, Department, France)` is never its commune), `district` (`05` for a city's fifth district: `Paris 5e`), or `enclosing` (`Saint-Exemple (EXAMPLEVILLE)`, a city's parish listed apart, or `Office (City, Department, France)`, read as the city, the part before the parentheses being the parish). |
| `cells` | Where the result rows show what selection reads, each optional: `locality`, `parish`, `act`, `period`, `numbers` and `call_number`. A cell is a `data-champ` name, `#<n>` for the n-th column of a row that shows the part without a name, `#title` for the record's title (`resultats.results[].intitule`), or `#none`. Without a `locality` cell, every row the engine returned is kept (a census listed by canton). The `act` cell joins every span of its name (one per kind). `numbers` is one cell (`1 à 1586`) or two, whose last numbers are the first and last (`mat_debut`, `mat_fin`; `… (acte n° 4024)`, `… (acte n° 4081)`). `call_number` defaults to `#title`. An engine without an act filter whose collection holds several kinds needs the `act` cell. |

Resolution:

1. Where a filter is `keyed` and the citation has its part, `GET` of the
   engine's bare answer (`/_recherche-api/moteur?refUnique=<engine>&<engine>--contenuIds[]=…`,
   the request the search page sends on load), whose aggregations list each
   filter's values, the buckets under the field or under `<field>_terms`:
   the locality's value is the one whose name, read in the portal's style,
   folds to the cited locality, or failing any the one value naming it
   under another name (§5.1, *Other names of the locality*) — none means
   the portal lists no such locality, and the answer is `Results` with no
   match —, the period's the
   one naming the cited year alone (`Classe 1911`, `1936`) — none leaves the
   period filter out.
2. `GET /_recherche-api/moteur?refUnique=<engine>` with the filters: the
   locality in the portal's style, by name (the engines accept a plain name
   as a text match), or its key; the act filter value or values, with
   `[op]=OR` for several; and the year as the period mode writes it, in both
   inputs for an `end`, the cited period's first and last years for a
   `span`. Each filter carries its `[op]` and its
   `[extras][mode]`, and the query ends with `from`, `resultSize=100` (the
   engines accept 25, 50 or 100), the content identifiers and the display
   mode, all prefixed with `<engine>--` and percent-encoded. A series cited
   without a locality is searched without the locality filter. `results_url`
   is the search page with the same filters (keyed ones left out, since
   their keys need a request) and `resultSize=25`, the portal's default,
   which renders faster for the reader than 100.
3. The answer's `resultats.results` gives each register's record reference
   (`refUnique`) and title (`intitule`), `resultats.total` the rows of every
   page, and `resultats.html` renders the same registers, in the same order,
   as rows (`tr.resultat_container`) whose cells carry the parts the
   `cells` settings name, and whose viewer button carries the viewer address
   (`data-visionneuse-url`,
   `/_recherche-api/visionneuse-infos/<engine>/<record>/<field>/image/<id>`)
   beside the image count (`span.nombre_images`, `(46 images)`, absent on
   some portals). An answer whose rows do not match its results is a changed
   shape. A title read as the call number loses what follows the call number
   on some portals: the row's locality or the start of it
   (`9 E 99 Exampleville`), or its period (`9 E 99 1798 - 1800`); a title
   that is the locality alone is no call number.
4. Each row's locality is read in the portal's style; a row naming several
   localities (several cells of the name: a register of several communes)
   is the cited one's when one of them is. Where the search cannot single
   out the cited act — the engine has no act filter, or the act's values are
   another code's too —, the act cell is read as codes from its French words
   (`Baptêmes, Mariages, Sépultures` is `BMS`, `1850 (naissances)` `N`,
   `Tables décennales des…` `TD`).
5. Selection (§3.2 `select.rs`): rows are first kept by their locality,
   folded (case and accents ignored), equal to the cited locality read in
   the portal's style — the locality filter is a text match, `Bourg (Le)`
   also returns `Saint-Exemple-lès-le-Bourg` —; a series cited without a
   locality, or a collection without a `locality` cell, keeps every row.
   A row bearing a name the place dictionary knows for the cited locality
   is the locality's (§5.1, *Other names of the locality*). When no row
   bears either, a commune renamed since the citation is kept: rows whose
   locality is the cited one without its qualifier (`Exampleville-sous-Bois`
   become `Exampleville`) or extends it, its shortened form or a known name
   at a word of its name (`Exampleville` become `Exampleville-en-Plaine`,
   `Exampleville-sur-Mer` become `Exampleville-Billancourt`), each agreeing
   with every part of the citation it shows — the image count excepted for
   a row carrying a cited call number, renumbered since (§7) — and carrying
   a cited call number or the cited image count, since such a name may as
   well be another commune's (`Exampleville-la-Forêt`, an `Exampleville`
   elsewhere). A row of a known name needs no such evidence.
   A text-matched search (steps 1–2) whose rows hold no register of the
   cited locality and name it nowhere is sent once more, for the first page
   only, with the other name §5.1 gives; `results_url` of what follows is
   that search's.
   One register is then selected by the citation parts it has, in order:
   call number, act kind, parish, period, the cited act or matricule number
   within the numbers the row spans (`n° 1 à 1586`), and image count equal
   to the cited view count; selection stops at the first criterion that
   leaves exactly one row, and a criterion that would leave none is skipped.
   A call number joining a digitization's to the original's (`9NUM/8E99`)
   matches a row showing either alone, and a parish matches folded, a
   section of a city's registers whichever way it is written (`3e
   section`, `Section 3`). The call number keeps the rows carrying the most of the cited ones (a
   microfilm's shared by several registers, then the register's own,
   §5.1). The period first keeps the rows covering the cited year; then,
   for a cited period other than one whole year — the register's own rather
   than the act's year —, the rows holding all of it, the row spanning
   exactly it, and the rows overlapping it, to the day where both the
   citation and the row write dates (`05/03/1871-28/03/1871` among
   registers of half a month). Rows still tied go to the one whose image
   count is nearest the cited one, within a tenth of it (a portal's
   register gaining a few images since the citation). A cited call number
   that no row carries ends the selection with the results, unless exactly
   one row covers the cited year with the cited image count; a lone row
   showing both another call number and another image count than the cited
   ones is no match either (a citation of an older digitisation).
   Several rows carrying the cited call number are parts of one register,
   each dated by where it starts (the Paris cemeteries' daily registers, in
   parts of some 600 entries, `04/01/1918 (n° d'ordre 601)` to
   `15/01/1918 (n° d'ordre 1201)`): among them the cited number is tried
   before the period, which would keep the part a register's first year
   dates rather than the one holding the entry.
   Act cells written as codes must hold every cited kind;
   words the search already filtered are left to the engine. A period cell
   may hold several segments (`1598-1613 , 1656-1667`,
   `NMD 1857-1859, N 1853-1872`, `NM an II`, `1793/1802`, `1621...1687`),
   and covers the year when one does; the engine's own period filter is an
   overlap test.
6. A cited call number on none of the rows read, while the engine matched
   more, is looked for on the next page (`from`), up to three pages of 100
   rows: a populated locality without a period filter (the Sarthe's
   `Le Mans`, 186 marriage registers) lists the cited register anywhere.
7. `GET` of the selected register's viewer address. `medias[0].sources`
   lists its images in order: their count, and per image its path
   (`/_recherche-images/show/<record number>/image/<file>/<position>`, the
   image's IIIF base on the portal's origin, which some portals write as an
   absolute address on their host with or without `www.`) and `ARKLink`, its
   persistent address. A register digitised in parts lists its images from
   several files, each image's path naming its own file and its position
   within it (Maine-et-Loire: 109, 1 and 248 images; the Yvelines'
   censuses: one file per image), while the row's viewer address names the
   first file only. Nothing else of the answer is read:
   `infosImage["@id"]`, `imageLienComplet` and the file names name internal
   hosts and paths.
8. The target is the record page opened on the view:
   `<search_path>?detail=<record>#<viewer address>/<position>`, the viewer
   address naming the image's file (`…/image/<file>`) and `<position>` the
   image's zero-based position within that file, both read from the end of
   the image's path; the portal's viewer opens on that image. A position
   beyond the named file's images opens the register on its first image, so
   the register's own index is never written after the row's address: on a
   register of several files, a view past the first file would open on
   view 1. An image path of another shape, and a portal read by its pages
   (below), which lists no images, give the row's viewer address and the
   view's zero-based index in the register. Each cited view gets its own
   address and ARK. A register listed without a
   viewer gives `Results` with one match; a view beyond the register's
   images gives `View` with no views, which the resolver takes for another
   register (§7).

**A portal read by its pages** (`transport: "page"`, §4.2) is sent no
request: each search of step 2 loads the search page with the same filters
in its address — the address `results_url` also builds, with 100 rows —
and waits until its scripts have rendered the results table
(`[data-component="resultats"] table.tableau_resultat_facettes`), or the
notice that nothing matched (`div.alerte`, `Aucun résultat`), which shows
no table and no count. The rendered rows are the answer's `resultats.html` rows,
read through the same `cells`; the count is in the heading's
`aria-label` (`1 234 résultats`); the page shows no record titles, so each
record is named by the address of its row's viewer button, and every part
selection reads is a cell: such a collection names its `call_number` cell,
uses no `#title`, and has no `keyed` filter, whose keys only the engine's
answer lists. Step 7 is not sent: the row's image count stands for the
viewer's image list, a row without one leaving the viewer to bound the
view, and the views have no ARK; such an archive is `display: "portal"`.
Its views are addressed by the row's viewer address and their index in the
register (step 8): on a register of several files, a view past the first
file opens on the register's first image.

A browser transport's start page (§4.2) is the portal's `/robots.txt`: the
lightest page of the origin, which passes the portals' bot-mitigation check
and fetches nothing on load. The search page would request the engine's
whole unfiltered list (the Sarthe portal's 9 754 rows), which the portal
answers before the adapter's own search in the same session: started there,
a `Le Mans` search waited 22 to 30 seconds; started on `/robots.txt`, the
page loads in 1 to 3 seconds and the search takes the engine's own time,
0.4 seconds cached and 3 to 25 seconds uncached for 100 rows of the
portal's most populated locality.

For a `display: "iiif"` archive the adapter also reads each cited image's
`info.json` (`<image path>/info.json`) for its pixel size, and builds the
picture and thumbnail addresses on the image path on the portal's origin,
never on the service's `@id`. The picture is bounded to 2048 pixels where
the service scales freely (level 2) and the full image otherwise; the
thumbnail is the smallest size the service lists that is at least 150 pixels
wide. The service sends no CORS headers, so the size is read by the resolver
rather than by the client, which then loads images through plain `<img>`
elements. The images are served with `Cache-Control: public, max-age=864000`.

The adapter's tests replay anonymized answers shaped like the catalogued
portals' (`crates/oxidgene-archives/fixtures/arkotheque/`, written by the
`generate.py` beside them, which copies no recorded value): the shapes
above, a populated locality's second page, keyed values, a census of
several lists a year told apart by call number and image count, and the
parts of a cemetery's daily registers told apart by entry number.

### 4.4 Mnesys

Observed on the Mnesys Expo (Naoned) portals of seventeen departmental
archives (§11.5)[^ad37-portal]. A portal has one search form per collection,
each with its own UUID, input names and filters: its parish registers and
civil status, often its decennial tables, and one form per series —
censuses, military registers and their tables, conscription lists, tables of
successions and absences. Every form is a plain server-rendered `GET` of
`/search/results` and every answer has the same rows, so one adapter
configured per collection serves them all. Some forms are reached only
through the editorial pages' rich text (`"target":"/search/form/<uuid>"`),
not through links. Records and images are addressed by ARK, and the portal's
viewer shows its reuse conditions first, which the reader accepts in the
window. No challenge or cookie is involved, except one portal (Drôme) whose
server sends its certificate without the intermediate: its collections are
`transport: "browser"`, which a browser and the desktop window complete
(§6.1). The `portal` settings are:

| Setting | Content |
|---|---|
| `origin` | Portal origin. |
| `transport` | `any` (default), or `browser` for a portal a plain client cannot reach. |
| `form` | The search form's UUID. |
| `fields` | The form's input names, in full, each where the form has it: `locality`, `act`, `year`, and the pair `period_begin` and `period_end`. Each portal prefixes them differently (`0-controlledAccessGeographicName[]`, `4-date`). A select is named with its `[]`, without which the portal answers an error, and a plain input without. A form may lack any of them: a military register form of a single bureau has no locality, most census and series forms no act, some civil-status forms neither act nor year. |
| `locality_label` | The patterns of a locality's value, each with one `{locality}` (`{locality} (Marne, France)`, `Bureau de {locality}`, `{locality}, commune`); required with a locality input. The portal needs the exact label, and a bare name returns no row. All patterns are sent in one request, since the filter ORs its values and ignores those it does not know: a list that labels its former communes or parishes by a fixed qualifier gets one pattern each (`{locality} (ancienne commune) (Eure, France)`), at no request more. |
| `locality_style` | `plain` (default) or `article_suffix`, which moves a leading `Le`, `La`, `Les` or `L'` behind the name (`Bourg (Le)`), as for Arkothèque (§4.3). |
| `locality_lookup` | `true` for a list whose labels the citation cannot spell: capitals without accents (`SAINT-EXEMPLE`), dated former communes (`Exampleville (ancienne commune av. 1790, Somme, France)`), underscores, a period in the label. The labels sent are then those of the form's own list that a pattern makes name the cited locality, case, accents and punctuation ignored; a `*` after `{locality}` stands for any text (`{locality} (ancienne commune*, Somme, France)`). Default `false`. |
| `year_label` | The year input's value, with `{year}`, where the form lists classes by label (`Classe {year}.`); the bare year otherwise. |
| `acts` | Map from document code to the portal's labels: the act filter's values where the form has one, which every kind the collection holds then needs, a series included; and in any case the words that tell a row's acts and tables. A collection holding tables and registers needs its tables' labels even without an act filter. A combined act (`BMS`) is searched by the labels of its first kind. |
| `call_number` | `row` (default), the `Cote` cell; `context`, the cell or else the context entry before the title; `none` for rows that show none, so that a cited call number cannot select a row and is kept as the register's own. |
| `image_source` | `visualizer` or `manifest`: where the cited images are read (step 5). |

Two collections may share a form, each through its own act input: one form
of the Jura archives filters registers by act and decennial tables by
document type, one of the Territoire de Belfort archives parish registers by
register type and civil status by act.

Resolution:

1. With `locality_lookup`, `GET /search/form/<form>`: the locality select's
   `data-options` lists its labels, and the labels naming the cited locality
   are sent; failing any, those naming it under another name (§5.1, *Other
   names of the locality*), whose rows then show that name, so that
   selection takes their register on the citation's evidence. A locality
   the list does not name either way has no register in the collection:
   `Results` with no match, without a search. Otherwise the
   patterns are filled in with the locality in the portal's style. A series
   cited without a locality, or a form without a locality input, sends none.
2. `GET /search/results?formUuid=<form>&mode=list&sort=date_asc` with the
   locality labels, the act labels and the year, each in its input, and
   `resultsPerPage=80` (the portal serves 20, 40 or 80). The year goes to the
   year input, as the bare year or through `year_label`, and to both period
   inputs, which return the registers whose period contains it; a census or
   series form thus returns the register whose period contains the cited
   year (`1891-1926` for 1901). `mode=list` is always sent, because some
   forms default to a table or a mosaic. `results_url` is the same search
   without `resultsPerPage`, with the labels the patterns spell. A former
   commune's own label may embed a date and is found only by a lookup.
3. The page shows its total (`<span class="result">N résultats</span>`) or the
   empty marker (`div.no-result`); an anti-bot page is a challenge and
   anything else, or rows that do not match the total, a changed shape. Each
   result is a `li.element-list` giving the register's title, its `Date`, its
   call numbers (`6NUM8/999/050 (Cote)`, `9 Mi 9999 (Cote/Cotes extrêmes)`,
   several in one cell, `444, 1RP1029`, of which a bare number is internal),
   its image count (`315 medias`), the ARK of its first image
   (`/ark:/<naan>/<name>/<image>`) and a context list: the collection
   (`Contexte : Registres paroissiaux numérisés`), then the locality, bureau,
   parish, act or period in an order that differs per form, and the title. A
   row listed without images (`Manque`, a register hosted elsewhere) links
   its record only and cannot be opened. When the total exceeds the rows of
   the page, a twin of the selected row may be unseen: the answer is the
   `Results`.
4. Each row is read for what it is. In a collection of one series every row
   is of that series; in one of tables only, a table; in one of registers
   only, a register. Where the collection holds both, a row whose title,
   collection or context holds a word only its tables' labels hold is a
   table, kept only for a cited table, and the other rows only for a cited
   register. A register's acts are the single-kind acts whose label words its
   text mentions, or else the act codes its title writes in capitals (`BMS
   1739-1750`, `N (1849-1852)`, `EXAMPLEVILLE / BMS-NMD [1700-1800]`), which
   selection tests as an act written in codes (§4.3, step 3). The numbers a
   register spans come from its title or context after a range word
   (`matricules 500 à 1000`, `n°1-499`). A row is at the cited locality when
   a context entry or the title is the locality, as written or as one of the
   patterns label it, the label possibly shortened as a former commune's is
   — followed by its current commune
   (`Exampleville (ancienne commune) (Exemple, France)/Sampleton...`) or cut
   short past the locality's name (`Exampleville (ancienne commune)
   (Exemple,...`) —; when the labels sent came from the form's list; or,
   for a series, when the collection, a context entry or the title names it
   among other words (`Bureau de l'Enregistrement d'Exampleville`). On a form
   without a locality input, the rows naming the cited bureau are kept, and
   every row when none does. The parish is the context entry that equals the
   cited parish or ends with it (`Paroisse Saint-Exemple`), and the call
   number the cell's that matches the cited one, or its first. One register
   is then selected as for Arkothèque (§4.3, step 3), the call number
   criterion left out where `call_number` is `none`.
5. A row whose count spans several lots (`2 lots 892 medias`, images and a
   document), or that shows none, has its image count read from the
   viewer's state for its first image, `GET
   /visualizer/api?arkName=<name>&uuid=<image>`: `counts.media` counts the
   images of that image's lot. The cited images then come in at most one
   request per run of neighbouring views. With `visualizer`,
   `GET /visualizer/api?arkName=<name>&start=<i>&end=<j>&group=0` (indices
   zero-based, windows of at most ten views) answers each image's
   identifier; it is the portal viewer's own undocumented endpoint, and
   answers an array for a window starting at the first image and an object
   keyed by index otherwise. With `manifest`,
   `GET /iiif/ark:/<naan>/<name>/manifest.json`, the register's IIIF
   Presentation 3 manifest[^iiif-presentation], lists every image in order
   with its pixel size, in one request but of 400 kB and more for a register
   of 300 images; each canvas id ends `/<image id>/canvas/<n>`. Both are
   read for the image identifier (and the size) only. `visualizer` is
   preferred: it avoids listing the whole register, and the pixel size, which
   only a `display: "iiif"` archive needs, comes from the image's own
   `info.json` for the cited images alone. `manifest` stays for a portal
   whose viewer endpoint is missing.
6. The target of a view is the image's own ARK,
   `/ark:/<naan>/<name>/<image id>`, which opens the portal's viewer on it
   (the viewer's number is the zero-based index plus one). Unlike Arkothèque,
   this is a persistent identifier: it is also returned as `ark`. A register
   listed without images gives `Results` with one match; a view beyond the
   register's image count gives `View` with no views, which the resolver
   takes for another register (§7).

For a `display: "iiif"` archive the adapter also builds each cited view's
image. The portal's IIIF service is level 0 and serves the full size only: a
request for any other size fails, and the full image weighs about 1 MB. The
picture is therefore the full image,
`/iiif/ark:/<naan>/<name>/<image id>/full/max/0/default.jpg`, and the
thumbnail is the portal's own, `/images/<image id>_thumbnail.jpg` (a few
kilobytes). The size is the manifest canvas's, or the `info.json`'s of
`/iiif/ark:/<naan>/<name>/<image id>` (Image API 3, whose answer the manifest
labels loosely as an Image API 1 service: only the size is read). The portal
sends CORS headers on the manifest, `info.json` and IIIF image, but not on
the thumbnail and `/visualizer/api`, which plain `<img>` elements and the
resolver need no CORS for.

Indre-et-Loire is the only `display: "iiif"` Mnesys archive: its reuse terms
allow free reuse with attribution, « Archives départementales
d'Indre-et-Loire, cote », and the attribution template is that credit with
the view. The terms also ask for the date of the information or its last
update, which the template cannot hold: the reader adds it in the document
description. The other portals serve no IIIF service (`location.iiif` is
null in the viewer's answers, the manifest answers 404), or one under terms
that require a licence (Doubs), or show a viewer whose reuse conditions
forbid public redistribution: they stay `portal`, whatever their site's
reuse page says. Savoie runs an older Mnesys interface, which has its own
adapter (§4.14).

**Not covered.** Name indexes whose rows are persons, not registers — the
Calvados military-register search (one row per conscript) and the Somme
`feuillets matricules` —, and series that a portal publishes without a form
of their own and reaches only through its global search
(`/search/results?q=…` with facets): the succession tables of Corrèze,
Calvados and Drôme, the Indre-et-Loire censuses. A free-text query over every
record matches too loosely to single out a register, so they are left
uncatalogued.

The adapter's tests replay anonymized answers shaped like the portals'
(`crates/oxidgene-archives/fixtures/mnesys/`, written by the `generate.py`
beside them, which copies no recorded value): registers, tables and
combined registers, letter-coded acts, a lookup of upper-case labels,
military registers chosen by bureau, class and matricule range on forms with
and without a locality input, rows in several lots and without an image
count, rows without images, several call numbers in a cell and a call number
in the context.

### 4.5 Ligeo

Observed on the departmental portals listed in §11.5[^ligeo-portals]. Each
collection is one search ("recherche") of the portal, named in the path and
placed in its menu by a node number, or the search within one finding aid;
the form is a plain `GET` and the answer is server-rendered HTML, with no
session, key or token. Input names, act encodings, result layouts and the
labels of what they show differ from portal to portal and from search to
search, so the `portal` settings describe them. A portal usually publishes
several searches — parish registers, civil status, decennial tables,
censuses, military registers, tables of successions — and some split one
search into collections by an input of their own (the kind of register, a
department on a portal two archives share). The `portal` settings are:

| Setting | Content |
|---|---|
| `origin` | Portal origin. |
| `transport` | `any` (default), or `browser` when only a browser reaches the portal: an F5 challenge, a web application firewall, or a server whose certificate chain lacks its intermediate (§4.2). |
| `prefix` | `/archive` (default), or `/archives` on some portals. |
| `search` | The search's name in the path, `etatcivil`, `paroissiaux`, `EtatCivilNumerise`; also sent as `type`. |
| `node` | The menu node, `n:<node>` in the path. |
| `page` | The search page's name in the menu where it differs from the search's (`listerecensement` for `recensement`); default `search`. |
| `fonds` | A finding aid whose page holds the form: the search runs within it, `<prefix>/fonds/<fonds>/<search>/n:<node>`. |
| `layout` | The results' layout, a path segment before the node, where the default layout is not the one the settings read: `Tableau`, `tableau`, `tabulaire`, `etatcivil_tableau` (named by the portal's own layout tabs; a wrong name answers no row). |
| `fields` | Names of the form inputs: the optional `locality` (a commune, or the recruitment or registration bureau of a series; none for a series searched by year alone), `act`, `year_from` and `year_to` (both or neither), or `year`, a single year's input (an exact year, a select of census years, checkboxes of years `RECH_date[]`); `number`, the input of an index of persons that takes the cited matricule; `call_number`, the input taking the cited call number (`RECH_cote`), on a portal whose search by locality alone answers too slowly for its largest communes; and `year_margin`, years searched on either side of the cited one by `year_from` and `year_to` (default 0). |
| `params` | Inputs sent with every search, with their values: `{"RECH_departement": "Exampledept"}`, `{"RECH_typeregistre": "registre paroissial"}`, `{"RECH_images": "1"}` (digitised registers only). |
| `acts` | Map from document code to the act filter, omitted where the form has none. A string is the value of the `act` input, which a name ending in `[]` makes a checkbox list: a combined act repeats it once per kind (`RECH_acte[]=B&RECH_acte[]=M`), publications of banns as marriages, and a value two kinds share (`naissance|baptême`, `NB`, `Baptême ou naissance`) is sent once. Values are the form's own labels or globs (`bapt*`). An object gives inputs of its own, with their values: `{"RECH_doc": "EC", "RECH_acte2": "*aissanc*"}`; a combined act without its own entry is then searched by its first kind. Every kind the collection holds needs one, a series included, unless the form has no act filter. |
| `columns` | What each part of a row is read from: `locality`, `title`, `acts`, `parish`, `period`, `call_number` and `numbers` (the numbers a register spans, `1 à 1586`, or a person's matricule), each the header text of a table column or the label of a notice's item, read as written (case, accents and punctuation ignored); a list names several, whose texts are read together (a document type and an act, `["Type de document", "Type d'acte"]`) or which the results show in turn. A notice's heading parts are read by their class, `cote`, `unittitle` and `date`, and the whole heading as `title`. A row's locality is its `locality` cell, or else its `title`'s head; rows showing neither belong to the locality the search filtered. |

Resolution:

1. `GET <prefix>/resultats/<search>[/<layout>]/n:<node>?<locality input>=<locality>&<act filter>&<params>&<years>&<number input>=<number>&<call-number input>=<call number>&type=<search>` — or `<prefix>/fonds/<fonds>/<search>/n:<node>?…` within a finding aid — the address a form submission reaches through a redirect from `<prefix>/recherche/…`. Values are the readable names and labels; no key is needed. `results_url` is this address. The locality is a **substring** match on most searches (`Exampleville` also returns `Exampleville-lès-Bois`). When the answer holds no register of the cited locality and no row names it — a commune the portal lists under another name, `Exampleville` for a cited `Exampleville-sous-Bois` — the search is sent once more with the other name §5.1 gives (*Other names of the locality*): a name the place dictionary knows for it, else the cited name without its qualifier, whose substring match lists every name extending it; that search's address is then `results_url` of what follows. The years are an interval test, both inputs the cited year (widened by `year_margin` on a portal whose index dates a register `mars - décembre 1672` by other years), or the single `year` input; a citation without a year omits them. Only an index of persons with a `number` input receives the cited matricule, which finds the person's row, and only a collection with a `call_number` input the cited call number, as written: the Gironde's search of `Bordeaux` for a year's deaths lists the registers of every section of the city and outlasts the portal's own gateway (`504 Gateway Time-out`), while the same search with the call number answers its one register in a second. A search by call number that answers no row — a call number the portal writes otherwise, or the original's where it shows its copy's — is sent once more without it. Each of these runs at most once. The portal otherwise learns the locality, the act, the collection's own inputs and the year, nothing else of the citation. Portals that answer with a redirect to `…/tableau/<finding aid>/n:<node>` are followed on their origin.
2. The answer is a Ligeo page when it holds `div#arc_liste_update`, or `div#arc_fonds_notice` within a finding aid. Any other body is an anti-bot challenge when it bears the signature of Anubis or of the F5 pages (`ResolveError::Challenged`, §5.2: not drift), and a changed shape otherwise. The count is in `p.nb_reponses > span` or `span.arc_nbr_reponses`. The results are a table whose header row (`tr.entete`) names the columns of its rows (`tr.pair`, `tr.impair`), a list of notices (`tr.arc_pair`, `tr.arc_impair` of `div#linear_liste`, or `li.arc_notice` in a finding aid) whose items carry their labels (`<strong class="arc_libelle_strong">Commune : </strong>…`), or nothing (no match). A column the settings name that a table lacks is a changed shape; a notice shows only the items it has. More answers than rows read (pages of 5 to 50 rows) gives `Results` with the count rather than a guess.
3. Each row gives:
   - its **places**, every one its locality cell names: a name with the thesaurus qualifier of the places it lies within (`Hameau (Exampleville, Exampledept ; lieu-dit)`, an article written back before the name, `Bourg (Le)`), several places in one cell (`Exampleville (…), Autreville (…)`, or one after another), a place within a commune after it (`Exampleville / Saint-Exemple (paroisse)`, `Exampleville — Ancienne`, `Exampleville -- Rue …`, a city's section `Exampleville -- Section 3`, `EXAMPLEVILLE Hameau`, `Exampleville, paroisse Saint-Exemple`), without bracketed notes (`[aujourd'hui : …]`), the last step of a finding aid's path (`Registres > Exampleville`), an office or district standing for its seat (`Bureau de Exampleville`, `subdivision de …`, `Canton de …`); on a title, the head up to ` : `, `, `, `. `, `.- `, ` - `, ` n°` or a word starting with a digit, after a leading call number (`9 M 99 - Exampleville - 1901`), and its parish (`Exampleville : Saint-Exemple, paroisse de …`, `paroisse de Saint-Exemple.`);
   - its **acts**, read as the codes selection compares from the acts cells, else the title, else the viewer link's label: the series the text names in the citation vocabulary (§5.1; `RM` for `Registre matricule`), `TD` for decennial tables, the kinds the text names (`B, M, S`, `Naissances.`, `Bapteme / Sepulture`, `publications_de_mariages`); letter codes only in an acts cell or a label, never in a title, where an initial is not an act;
   - its **period** as displayed, whose years `covers` reads (§4.3): ranges written with a dash, a slash, `à` or spaces (`1683/1750`, `1833 à 1852`, `1841 1860`), full dates around them (`13/11/1697 - 06/11/1707`, `26 juillet 1849-20 février 1850`), several in a composite cell (`Baptêmes (1512-1569, 1597-1673) ; …`), Republican years;
   - its **call number**, the cell up to the heading's next part (`9 NUM /1 - `), or the title's (before its first ` - `, or the shaped words after its first sentence, `… Saint-Exemple. 1 GG 8, registre …`), or the viewer link's label when shaped like one (`3 vues - 9 Mi 99`);
   - its **numbers**, from the `numbers` cell (`1 à 500`, `N° 1-500`, `Matricules, 1-500`, a person's `984`), or else from the title or the link's label after `n°`, `nos`, `numéros` or `matricule(s)` (`Volume 1 n° 1 à 500.`, `(matricule 984)`);
   - its **image count** and viewer, from the first viewer link, `/ark:/<naan>/<id>/<tag>/<group>` followed by named segments (`/layout:table|linear`, `/idsearch:…`), whose `title` reads `120 vues  dont 104 indexées - <label> (ouvre la visionneuse)`. A row without one lists a register not digitised.
4. One register is selected as for Arkothèque (§4.3, step 3), among the rows with a viewer link, each read as the citation names it: a row one of whose places is the cited locality, or lies within it, has the cited locality, the place within it being its parish (the cited `Exampleville` finds `Hameau (Exampleville, …)` with the parish `Hameau`), failing which a place naming it under another name is the row's locality, for selection to weigh (§4.3, step 5), and a row naming the cited parish among its places or parishes has it. Then act kind, parish, period, number and image count. A collection whose rows show no locality — a series searched by year alone, a select of bureaux — keeps every row whatever the cited locality. A military register is thus found by its bureau and class and the volume whose matricules hold the cited one, a person's row by the matricule itself. The **call number only breaks a tie**: the first selection ignores it, and it is applied only when that leaves several rows or none (a renamed commune's register, kept on its cited call number), choosing one only if exactly one carries it. The portals disagree on what it is: Ain shows an internal reference, not a call number; some show one shared by the registers of every locality of the same kind and year, or by a commune's volumes; a citation of a military register often gives the original's while the portal shows its microfilm. A cited call number no row carries therefore does not discard them.
5. The target is `<origin>/ark:/<naan>/<id>/<tag>/<group>/<view>`, the view one-based. `<tag>/<group>` is kept from the row's viewer link: `daogrp/0` normally, `daoloc/0` on some parish registers and a person's row, `dao/0` on others. The portal answers with a redirect to the same path and `?id=<canvas ark>`; its Monocle viewer opens on that view, and a reload keeps it. A view beyond the register's images gives `View` with no views, which the resolver takes for another register (§7); a person's row of an index, which carries the cited call number, opens on the person's own view whatever view of the register the citation counts.
6. For a `display: "iiif"` archive, `GET /ark:/<naan>/<id>/manifest` (IIIF Presentation 2, `Access-Control-Allow-Origin: *`, not cacheable, 0.25 to 1 MB) gives the image count and, per canvas, the image service and the view's own persistent address (`…/img:<image name>`, returned as `ark`). The canvases' declared sizes are not their images': the live checks found canvases of 1392 × 1212 over images of 2704 × 1780 (Ain), and other proportions on every portal. Each cited view's size is therefore its service's `info.json` (`<service>/info.json`, Image API 3 context, served as `text/html`), one request per view; the manifest's other fields (renderings, thumbnails, file paths) name server paths and are not read. The services are Image API 2 level 1 under the portal's `/iiif/` path, rebuilt on the portal's origin whatever host the manifest declares. Level 1 sizes by width or height, never by a bounding box and never above the image's own size (`404`): the picture is `full/2048,/0/default.jpg` (`,2048` for a portrait image) when the long side exceeds 2048 pixels and `full` otherwise, the thumbnail `full/150,/0/default.jpg`. Images carry `Access-Control-Allow-Origin: *`. A `display: "portal"` archive reads the count from the row and fetches no manifest.

The viewer shows the current view in `.monocle-PageNav input[role="spinbutton"]` (`aria-valuenow`) and the total in `.monocle-PageNav-total`, which the live check reads; the Hautes-Alpes portal runs Binocle instead, whose gallery counter shows them (`.bn-gallery-counter-current`, ` sur 29`); the portals observed show no licence step. A live check finds a locality to probe from what backs the locality input, read from the page, not set: a thesaurus named in the page script (`new VT_Control("ArchivesRECHCommune",{…"str":"…"…})`), whose autocomplete `POST <prefix>/xhr/gettheslist/<thesaurus>/0/<search>/<input>_Index` lists the localities starting with three letters; a typed facet (`new VT_Control(…,{…"url":"/arcfacette.php?…&id=<input>&autoc=1"…})`), whose answer to that address and the letters, or no letters for a short list such as bureaux, lists them as `button.facette-select-<input>`, those qualified by the department a shared portal's `params` filter first; the input's own options or checkboxes, those written as names first (a form's bureaux may include `liste nominative`); the branches of a finding aid's tree (`title="Détail de la branche <locality>"`). A locality typed in a plain input nothing backs is probed with the three letters themselves, which the portal matches as text, and a search by year alone with no locality; a sparse series (Protestant registers) holding nothing of the first locality a shared thesaurus lists, or a portal two archives share answering an error for a locality of the other department, is probed for every locality. An index of persons is probed with the matricule 1, which each class has. The registers listed are read oldest first, whatever the portal's order, and only those of the act searched: a search by act also lists that act's decennial tables.

**Reuse licence over the viewer.** The Monocle viewer asks the reader to accept the reuse terms (a dialog titled « Réutilisation des informations publiques », « Accepter » and « Refuser » buttons) on the portals that enable its clickwrap in the page's own script: `Monocle.iiif({"config": {…, "monocle-clickwrap": {"active": true}, …}})`. The messages of the dialog stand in the same configuration on every portal; a portal that does not enable the step either omits the `monocle-clickwrap` key or sets `"active": false`, and the manifest's reuse text (`ligeoReUseProfil.content`) does not tell the two apart (the Val-d'Oise manifest's is empty). An archive whose viewer enables it is `portal` whatever its terms allow (§3.1). The browser check of an `iiif` archive reads the viewer page's configuration and fails with the licence named when the key is active, so a portal that enables the step later is reported by the weekly check (§9.1); it costs no request beyond the viewer's own.

**Etiquette.** These portals' `robots.txt` set `Crawl-delay: 5` (one asks 3) and disallow `/archive/resultats/*?*`, `/archive/recherche/*?*` and `/archive/*/view:*`, as the Loire-Atlantique one does; a few disallow every query address (`/archive/*?`). Resolutions are user-initiated only, one per click (§8), and the live checks run at the weekly rate (§9.2), spaced by the `Crawl-delay`. Several portals run Anubis, a proof-of-work challenge shown to browser `User-Agent`s only: the identifying `User-Agent` of the `native` transport is not challenged, while a headless browser is refused (`Access Denied`, or a page whose assets lie under `/.within.website/`), on some portals at the viewer too, so a live check's opening ends `challenged` there. Others run F5 TSPD or a firewall that gives a plain client a challenge page or `Request Rejected` (hence `transport: "browser"`).

**Reuse.** An archive is `display: "iiif"` where its reuse terms, often an Etalab open licence, ask only for the attribution to the archive with the call number (and the date of the last update), and its manifests and images answer across origins; its `terms` are the deliberation or the reuse page, or the manifest's own reuse text (`ligeoReUseProfil`). It stays `portal` where the portal enables the viewer's clickwrap, a reuse licence the reader accepts over the viewer before any image (below), where reuse needs a licence requested from the archive, a paid licence for some uses or a third party's authorisation (registers digitised by FamilySearch), where the terms could not be read or forbid reuse beyond private consultation, where a collection is reachable by a browser only, or where the manifest has no IIIF image service. Waiting periods keep recent acts out of the portals (births under 100 years, marriages 75, deaths 25).

**Not covered.** Searches that answer with a finding-aid tree (`/resultats/<search>/fonds2|hierarchique/…`) rather than a list of registers; indexes of persons without a matricule input, whose searches by bureau and class list thousands of rows; searches whose answer for one locality or class exceeds a page of results with no input to narrow it (the volumes of a bureau's tables listed without years, the feuillets of a class); and the name indexes (by surname). Their collections are not catalogued.

The adapter's tests replay anonymized answers shaped like the portals' (`crates/oxidgene-archives/fixtures/ligeo/`, written by the `generate.py` beside them, which copies no recorded value and keeps no server path): tables, lists of notices, qualified and composite locality cells, an index of persons, a search within a finding aid, and a military-register table built in the same markup.

### 4.6 Archinoë

Observed on the Charente-Maritime, Oise, Pas-de-Calais and Côte-d'Or portals
(2026-10-05). One viewer serves every portal, `visualiseur/<page>.html?id=<id>&vue=<n>`
with `n` one-based, showing `n/total` in `#visu_pagination` and one
`div_image_<n>` per view; three search modules stand in front of it, so one
adapter serves them with a `search` setting. The portals publish no persistent
address for a view and no IIIF service, so every archive is `display: "portal"`;
the viewer's "permanent link" tool is never used, because each call creates an
ARK on the portal. The `portal` settings are:

| Setting | Content |
|---|---|
| `origin` | Portal origin. |
| `transport` | `browser` for `archinoe.com` and `ressources.archives.oise.fr`, which answer a client that does not look like a browser with a redirect to another site; `any` for the others. |
| `search` | `registre`, `seriel` or `ead`. |
| `base` | Path of the search module: `/v2/ad17` (`registre`), `/console` (`seriel`, `ead`). |
| `viewer` | The viewer's path and query with one `{id}`: `/v2/ad17/visualiseur/registre.html?id={id}`. The view is appended as `&vue=<n>`. |
| `acts` | Map from act code to the portal's value: the act identifier (`registre`), the checkbox name (`seriel`), or the title of the commune's act node (`ead`). Every act the collection holds needs one; a combined act (`BMS`) without its own entry is searched by its first kind. |
| `fields` | `registre`: the `locality`, `act` and `year` input names. `seriel`: `locality`, `year_from`, `year_to` and `cote`. Absent for `ead`. |
| `licence` | `registre` only: `click` when the search page stands behind a licence page whose link the reader clicks. |
| `id`, `form`, `locality_label` | `seriel` only: the form module's number and key, and the pattern of a locality as the portal writes it, `{locality} (Pas-de-Calais, France)`. |
| `ir`, `eadid` | `ead` only: the finding aid's number and identifier. |

A member of another search, or a missing one, is refused when the catalogue
loads.

Resolution, `registre` (Charente-Maritime, Oise):

1. `GET <base>/registre.html` returns the form, whose locality `<select>` maps
   each commune's name to the portal's identifier: this is the whole lookup, one
   request. A page without the select is a changed portal (`drift`), except with
   `licence: "click"`, where it is the licence page: the adapter never accepts a
   licence for the reader, and returns the search page for the reader to open
   (`Results` without a match count). A commune that no option names, by its
   name as cited or with its leading article moved behind it, folded, gives
   `Results` with no match.
2. `GET <base>/registre_liste.html` with the locality identifier, the act
   identifier, the year (omitted when the citation has none) and `ajax=true`.
   Collection and register type are not sent, so every collection is searched.
3. The fragment lists the matching registers as `a.Row` rows, `N résultat(s)`
   in `div.total`, and "Pas de résultat" for none. The header names each
   column, which differ between portals: call number (`Cote`), commune,
   acts, period (`Période`, `Dates extrêmes`) and, on the Oise portal, the
   parish. The Charente-Maritime portal writes the parish in its observations
   column (`Paroisse Saint-Exemple`). A row has **no image count**, and one for a
   register not yet digitized has no viewer address and is skipped. A search for
   births or deaths also returns the tables of the same years, listed with
   `Tables décennales` as their acts: they are dropped unless tables are cited.
4. One register is selected as in §4.3 step 3. When several remain and the
   citation gives its image count, the viewer page of each remaining register,
   at most three, is read for its `div_image_<n>` count: the one register whose
   count equals the cited one is chosen, and its count becomes the target's
   `view_count`. Otherwise `Results` with the number left.
5. The target is the viewer address with `&vue=<n>`. The count is known only
   from step 4, so a view is not checked against it.

Resolution, `seriel` (Pas-de-Calais):

1. `POST <base>/ir_seriel_action.php?f=0&cle=<form>&id=<id>`, form-encoded,
   needs no session: the locality as the portal writes it
   (`Exampleville (Pas-de-Calais, France)`, from `locality_label`, which saves the
   request for the portal's autocomplete), the year in both bounds, the act's
   checkbox set to `on` and, when the citation has a call number, the call number
   (the portal matches words and prefixes). A search that finds nothing with a
   call number is sent again without it.
2. The fragment says `N résultats trouvés`, `Un résultat trouvé` or `Aucun
   résultat trouvé`, and lists three registers per page, each with a notice
   table (call number, place, period) and the parish as the content's heading.
   A page is `POST …&r=0&page=<n>`, zero-based, with the same body. Pages are
   read until the citation decides or the results are exhausted, at most five.
3. Selection and target as `registre` steps 4 and 5.

Resolution, `ead` (Côte-d'Or), which has no register search form:

1. `GET <base>/ir_ead_visu.php?eadid=<eadid>&ir=<ir>` lists every commune as
   `javascript:showEntry(<id>)`. The page is ISO-8859-1, which a transport
   decodes as UTF-8 and turns each accented letter into U+FFFD: a commune is
   matched by its letters, a U+FFFD standing for any one letter, and with the
   article moved behind the name (`Étang-Exemple (L')`). This page is the
   only list of the communes' nodes — the aid's tree fragments start below
   it, and its text search answers nodes of another numbering —, some
   300 KB that the Côte-d'Or portal builds in about a second, or at busy
   times in 74 to 150 seconds, past a request's bound. The adapter therefore
   keeps each aid's commune index, a few tens of kilobytes of nodes and
   names, for the session (§8), and the following resolutions start at
   step 2. A commune node that answers no act node — the portal published
   the aid anew, with other nodes — drops the index, which the same
   resolution reads again.
2. `GET <base>/ir_ead_visu_action.php?ir=<ir>&id=<commune>&toc=1` lists the
   commune's act nodes (`Actes (BMS puis NMD)`, `Tables décennales`); the one
   whose title is the act's value is read the same way, giving its collections
   (`Collection communale`, `Collection départementale`). A node with no
   children is itself the notice.
3. `GET …&id=<collection>` (no `toc`) is the notice: one `item_<id>` block per
   register with its call number, period, `107 images numériques` and the link
   `lienImage(<id>)` that opens the viewer. A block without a link has no images
   and is skipped. Collections are read in order until the citation decides.
4. Selection as in §4.3 step 3, with the block's image count, which is not
   rechecked in the viewer. The target is the viewer address with `&vue=<n>`; a
   cited view beyond the count gives `View` with no views (§7).

An anti-bot challenge answers in place of a page (the Pas-de-Calais portal serves
an F5 challenge to browsers, whose script loads from `/TSPD/`): a body that
contains that marker is `ResolveError::Challenged`, not `unexpected_response`.
Any other redirect to another site on a `transport: "any"` collection fails the
request as leaving the endpoint's origins, which is drift. The live check's
steps 1 and 2 (§9.1) use the module's locality list (the select, the label, the
commune nodes) and step 4 reads `#visu_pagination`.

### 4.7 Prismia Vision

Observed on the Lot-et-Garonne portal (2026-10-05): a single-page application
over a JSON API on another origin, which admits the portal's origin only and
wants the portal's public key in an `ApiKey` header. Results are manifest stubs
of IIIF Presentation 3, but the images need the key too, so the archive is
`display: "portal"`. The `portal` settings are:

| Setting | Content |
|---|---|
| `origin`, `transport` | The portal and its access; `any` on the observed portal. |
| `api` | The API's address, `https://<host>/api`, whose origin the endpoint declares as another origin of the portal. |
| `search_path` | The portal's search page, for `results_url`. |
| `paths` | The instrument's `searchBarPathOrId`: the collections searched. |
| `filters` | The keys of the `locality` and `act` filters. |
| `acts` | Map from act code to the filter's value: `Baptêmes ou Naissances`, `Mariages`, `Sépultures ou Décès`, `Tables décennales`. |

**The key is not a setting.** The portal's `/runtimeConfig.js` publishes it
(`apiKey: '…'`) and the application reads it at every load, so the adapter reads
it the same way at resolution time: one small request that spares the catalogue a
credential to keep current and a silent failure of every resolution on the day
the archive rotates it. The value must be letters, digits and `-_.`, and is sent
only in the `ApiKey` header to the declared API origin.

Resolution:

1. `GET /runtimeConfig.js` on the portal's origin.
2. `POST <api>/presentation/v1/facet/getFacetValues` with the collection paths,
   the locality filter's key, and `text`, the start of the cited name (without
   its article, up to the first space or apostrophe, since the portal writes both
   `d'Exemple` and `d’Exemple`). The answer's `searchAggsMetaTag` lists the communes
   with that prefix; the one whose label, folded, is the cited locality or the
   cited locality with its article moved behind it (`Mas-d’Exemple (Le)`) gives
   the filter value. None gives `Results` with no match.
3. `POST <api>/presentation/v1/Query` with the collection paths, the two
   `tagSelectedFilters` (locality, act), the year as `periodeDeb`/`periodeFin`
   with `periodeRange: "between"` (omitted when the citation has none),
   `target: ["document"]` and `size: 50`. Each result is a register's stub:
   `id` (the manifest address, `<api>/iiif/presentation/v3/<id>/manifest`),
   `prismCoteId` (the **call number**), `prismNbMedias` (the **image count**),
   `prismNavDateValue` (the years held, `1673-1681, 1686, 1692-1723`, empty on
   some stubs, then the years of `prismNavDate`) and the parish among
   `listIndexationAgg`. A manifest address outside the API's is a changed shape.
4. One register is selected as in §4.3 step 3, with no further request. More
   results than the page holds give `Results` with the total.
5. The portal's viewer opens at `/viewer/<manifest address, percent-encoded>/<n>`:
   the canvas number is the view number, one-based (canvas 5 of a 216-canvas
   register shows `5` of `216`); without it the viewer opens on the first view.
   The register's own manifest, with its canvases and image services behind the
   key, is never requested.

### 4.8 GAIA

Observed on the Ariège, Aude, Orne, Pyrénées-Orientales and Seine-et-Marne
portals (2026-10-05 and 2026-10-06), which run GAIA 9 (9.4.8 to 9.5.7), a PHP
search engine under `/mdr/index.php/rechercheTheme/…` (`/mdr_aude/…` in
Aude); Orne and Aude embed it from their own site in an `<iframe>` on another
host, which the catalogue names. Each "thematic search" of the portal is a
wizard whose choices the PHP session keeps: a locality from an alphabetical
list, the kind of register, sometimes a further list, the years, then the
search, whose answer lists the registers twenty at a time, sorted by date.
The pages are ISO-8859-1 (Aude's database labels are UTF-8 inside them),
answer a plain client with a cookie jar, and have no anti-bot check. The
viewer draws a register's images on a canvas: it has no address per view
and always opens on the first image, and serves no IIIF and no persistent
identifier, so every archive is `display: "portal"`. The `portal` settings
are:

| Setting | Content |
|---|---|
| `origin`, `transport` | The engine's host and its access; `any` on every observed portal. |
| `base` | The engine's path, `/mdr` by default. |
| `theme` | The thematic search, `requeteConstructor/<theme>/…`: the civil status (`1`, `14` in Seine-et-Marne), the censuses, the military registers, the succession tables. |
| `localities` | `false` when the search starts with the kinds of registers rather than a list of localities (military registers by class); default `true`. |
| `letters` | `false` when the locality list is one page rather than one per initial; default `true`. |
| `locality_style` | `plain` (`La Ville-Exemple`) or `article_suffix` (`Bourg (Le)`): which initial lists a locality with a leading article. |
| `prefix` | Words the list writes before every locality: `Bureau de `. |
| `types` | Map from document kind code to the labels to choose, in order, after the locality: `["naissances"]`, `["Registres d'état-civil", "Décès"]` for a two-level choice. A combined act (`BMS`) takes its first kind's, banns the marriages'. Left out when the search offers no kind: the acts are then read from the titles. Labels compare by letters, case and accents aside. |

A portal whose parish registers, civil status and tables share one kind
list (`Registres paroissiaux`, `Registres d'état civil`) has one collection
each, with their periods, so that a marriage goes to the right list.

Resolution:

1. `GET <base>/index.php/rechercheTheme/requeteConstructor/<theme>/1/R/<letter>/0`
   (`…/R/0/0` without `letters`): the locality list, as `a.color_liens`
   links `…/<theme>/<step>/A/<id>/<label>`. A label is read as a name — the
   `prefix` removed, cut at a comma, a full stop or a bracketed note, a
   parenthesised qualifier other than an article dropped (`Exampleville
   (après 1793)`, `Exampleville (Département)`) — and a detail, what follows
   a comma (`, paroisse Saint-Exemple`, `, Eglise protestante`). The entries
   whose name is the cited locality, as cited or with its article behind it,
   are kept; among them those whose detail names the cited parish, or else
   the locality's own entry. None gives `Results` with no match, several
   `Results` with their count. Without `localities`, the list of the search's
   first step is read instead.
2. `GET …/<step>/A/<id>/x` for the chosen entry, then for each label of the
   act's `types` the link of that label on the answer (the label segment of
   the address is cosmetic; the adapter rebuilds every address from the
   numbers it reads). A missing label gives `Results` with no match.
3. While the answer is a further list (a census's years, a registration
   office's areas, the acts of a two-level choice) rather than the year
   form, the link whose label covers the cited year is followed, or the
   step's "search all" form `…/F/0/0` is posted; a list with neither is
   searched as it stands, which finds nothing (a conscription list's class
   must be cited). At most three such steps. A choice of one year followed
   (a class, a census year) dates the registers listed under it, since a
   class's lists are drawn the year after it.
4. With a year and a year form (`…/<step>/A/0/0`), `POST` it,
   `typeDate=simple&dateDeb=&dateFin=&dateSimple=<year>`: the server keeps
   the registers whose period overlaps the year, Republican years included.
5. `POST` the search, the page's `…/<step>/T/0/0`, with `forcepost=essai`.
   The answer counts the registers (`N réponses`, `Aucune réponse`) and
   lists twenty, each a `div.reponsemot`: its title, its call number and
   period cells (either may be empty: then the title gives the period, as in
   `Exampleville 1836`), and, for a register with images, the viewer's
   `docnumViewer/calculHierarchieDocNum/<unit>/<hierarchy>/` link. A register
   without it is skipped. Further pages are `GET
   <base>/index.php/rechercheTheme/paginer/<offset>`: with a year, the pages
   are searched by halves for the last one starting by that year, at most
   five more requests; without a year, only a cited call number has the
   next five pages read. A search the year form does not filter (the Orne
   and Seine-et-Marne censuses, the Seine-et-Marne military registers) is
   the one that pages.
6. One register is selected as in §4.3 step 3. When the locality's own
   entry was chosen and none of its registers holds the cited act and covers
   the cited year, its other entries are searched in turn from step 2, at
   most two (more give `Results` with their count): Orne lists a commune's
   parish registers under its parishes' entries, its own entry holding the
   civil status; one register found among them is the target. The kind's
   list does not always filter the search (Orne), which is why the acts are
   read from the titles. The locality is the chosen
   entry, or, without `localities`, a title's start (`Exampleville,
   matricules n° 1-496.`), any when no title names the cited one; the acts
   are read from the title, as codes (`BMS`, `NMD`, `N + T`) or words, a
   title naming a table being `TD`; the matricule numbers from it too. A
   transport that decodes the pages as UTF-8 leaves a U+FFFD for each
   accented letter: labels compare with it standing for any letter, and the
   act words and the `°` of `n°` are read through it. No image count is
   known, so the cited view count does not select. More registers than the
   pages read gives `Results` with the search's count.
7. The target is the viewer, `<base>/index.php/docnumViewer/calculHierarchieDocNum/<unit>/<hierarchy>/900/1400`
   (the last two segments are the window size the viewer lays itself out
   for); it needs no cookie. It opens on the first image whatever the cited
   view, so the target is `View` with no views, no view count and the
   register's call number, and the window brings the viewer to the cited
   view with its page box — the number typed, then the box left, which
   jqPagination applies (Enter only leaves the box), the box then reading
   `Page <n> de <total>` again —, or names the view to go to when it cannot
   (§6.1). Its page embeds one object per view in its `docs`
   array, which only the live check reads.

`results_url` is the list of step 1, which a reader can open. Requests: 4
to 7 without pages or other entries; the viewer is never requested.

### 4.9 THOT

Observed on the Ille-et-Vilaine and Corsica portals (2026-10-05 and
2026-10-06)[^thot-portals], which run THOT, an ASP portal of framesets
("THOT Internet"; the vendor is not named). Each family of documents is a
**module** (`MOD`) of the documentary search — the registers of acts, the
censuses, the military registers, the tables of successions — whose form of
numbered criteria (`txt_CIN_CH<k>`) posts back to itself: a locality chosen
from the form's list, a type of document, years. The portal keeps the
module and the search in its ASP session, which it opens only for a client
that passes its cookie check. Each result row is a register with its call
number and a link to a Zoomify viewer fed by a per-register slide file
(`slides_<n>.xml`, one `<SLIDE>` per view); the Ille-et-Vilaine portal gives
each view an ARK there, whose resolver opens the register at that view with
no session, and the Corsica portal gives none. Neither publishes a IIIF
service: every archive is `display: "portal"`. The `portal` settings are:

| Setting | Content |
|---|---|
| `origin` | Portal origin. |
| `transport` | `any`, or `browser` behind an anti-bot challenge (Corsica: Cloudflare refuses every client but a browser). |
| `base` | The portal's path: `/thot_internet`, `/Internet_THOT`. |
| `module` | The module's number, `MOD`: `10` for the registers of Ille-et-Vilaine, `4` for those of Corsica. |
| `criteria` | The criteria's numbers: `locality`, the list of localities (or registration offices); `act`, the type of document, omitted where the module has none; `year`, `"dex"` where the form takes a first and a last year (`txt_CIN_DEX_D`, `txt_CIN_DEX_F`), or the number of an interval criterion (`txt_CIN_CH<k>` = `<first>\|<last>`, typed in `intervalleDate1_<k>` and `intervalleDate2_<k>`); omitted where the year criterion does not find a register by a year it holds — the tables of successions, whose interval criterion matches a volume's extreme dates only when both are the year asked —, the years then told apart by the rows' periods. |
| `acts` | Map from document code to the type criterion's value, a list option (`baptemes`, `REGISTRES MATRICULES`) or a checkbox (`BAPTEMES`, Corsica), printable ASCII since the portal reads its forms as windows-1252. Every kind the collection holds needs one where the module has a type criterion; a combined act (`BMS`) without its own entry is searched by its first kind, publications of banns as marriages. |
| `views` | `ark` where the slide file gives each view's ARK, `register` where the portal has no address per view. |

The pages are windows-1252, which both transports read as UTF-8: every
accented letter arrives as U+FFFD. The adapter reads only what is ASCII —
the form's labels and values, call numbers, periods, the viewer's
arguments — and reads the act words with U+FFFD as `e`, the accented letter
of every French act word.

Resolution:

1. `GET <base>/FrmAccueilDroite.asp` opens the session (the `ASPSESSIONID…`
   cookie); its script names the cookie check,
   `<base>/FrmAccueilDroite.asp?checkCookie=<timestamp>`, which the adapter
   requests next. A session already open — the window keeps it between
   lookups — answers instead the page sending the browser on to the summary
   (`FrmSommaireFrame.asp`), and the check is skipped. A check answering
   that the browser refuses cookies, or a page later answering `Votre
   session a expiré`, is a changed shape.
2. `GET <base>/Recherche/FrmRechFrame.asp?MOD=<module>` selects the module
   in the session; `GET <base>/Recherche/FrmRechHaut.asp?MOD=<module>`,
   then `GET <base>/Recherche/FrmRechDOCCritere.asp?MOD=<module>`, the form
   (17 to 78 kB). The form must still list the localities, offer the
   document kind's value and have the year inputs; otherwise drift.
3. The localities' labels are the form's list, matched as the cited locality
   is written (case, accents and punctuation aside), or failing any by the
   name they give it: `BOURG-EXEMPLE (LE)` and `LA-BAUSSAINE` (the article
   written either way), `EXAMPLEVILLE (EXEMPLE, FRANCE)` (Corsica), a
   registration office `EXAMPLEVILLE (BUREAU DE L'ENREGISTREMENT)`, a part of
   a city `EXAMPLEVILLE (NORD-EST)` — beside a plain `EXAMPLEVILLE`, which
   is preferred. A hamlet keeps the commune its label names, `HAMEAU
   (EXAMPLEVILLE, EXEMPLE, FRANCE ; HAMEAU)`. Failing any, the labels naming
   it under another name (§5.1, *Other names of the locality*). No label
   gives `Results` with no match; more than three, `Results` with their
   number.
4. For each label, `POST <base>/Recherche/FrmRechDOCCritere.asp`,
   form-encoded: the form's hidden inputs, `txt_CIN_IDX<locality>=1` and
   `txt_CIN_CH<locality>=<label>`, the type (`txt_CIN_IDX<act>=1`,
   `txt_CIN_CH<act>=<value>`, and `cbx_txt_CIN_CH<act>=<value>` where it is
   a checkbox), the years where the settings name their criterion (both
   bounds the cited year, empty without one),
   `b_ExecForm=1`, `txt_IDX_OCC=0`. The portal answers `302` with a
   **relative** `Location`, `FrmRechListeHaut.asp?RechDoc=1`, which the
   transports resolve in the posted page's directory, as a browser does.
5. The list says `Aucune fiche…`, `Une seule fiche…` or `N fiches
   correspondent…` (`div.resultatrech`); a body without it is a challenge
   when it bears one's signature, a changed shape otherwise. Its headings
   name its columns, which differ between modules and portals: `Cote(s)`,
   `Commune` or `Bureau`, `Intitulé`, `Type d'acte` or `Type de document`,
   `Date` or `Dates`. Rows (`tr.l1`, `tr.l2`, 40 a page) give the call
   number, the locality (or else the label searched), the period, the acts —
   the act cell, or else the title (Corsica writes `Registre des actes de
   naissances, mariages…`), any table as `TD` — and, from the title, the
   matricules a military register's volume spans (`numéros matricules
   1-500`). A row's viewer link, `openLot(<idfic>, <idlot>, <ref>,
   <application>, '', <flag>)`, is skipped when restricted (flag `1`), not
   digitized (`cfecFichier`) or without a lot; rows carry no image count.
   Further pages, `GET …/FrmRechListeHaut.asp?RechDoc=1&page=<n>`, are read
   until the citation decides, five at most.
6. One register is selected as for Arkothèque (§4.3, step 3). The
   Ille-et-Vilaine portal lists two copies of a commune's year, the
   municipal one (`COMMUNE`) and the court clerk's (`GREFFE`): only the
   call number tells them apart, and without it the answer is `Results`.
7. `views: "ark"`: `GET <base>/FrmLotDocFrame.asp?idlot=…&idfic=…&ref=…&appliCindoc=<application>&resX=1400&resY=900&init=1&visionneuseHTML5=0`,
   the viewer page, names the slide file (`zSlidePath=<base>/download/thot/<n>/slides_<k>.xml`);
   its `GET` lists the views, whose number is the register's count, with
   `URLARK` (`<origin><base>/ark:/<naan>`) and each view's `LIENARK`
   (`<name>/<register>/<view>`). The target of view `n` is the resolver
   `<origin><base>/gestionARK.asp?a=<naan>%2F<name>%2F<register>%2F<n>`,
   which needs no session, the ARK `<URLARK>/<LIENARK>` returned as `ark`;
   a view beyond the count gives `View` with no views (§7). A
   slide file without the portal's ARKs is drift.
8. `views: "register"`: the target is the viewer page of step 7 itself, a
   `View` with no views and no count, which opens the register on its first
   view in the session that searched it, the window's (§6.1); the window
   then brings the open viewer to the cited view by changing its view
   input, or names the view to go to when it cannot. The viewer page is not
   requested: **every opening of it writes a slide file on the server**, as
   a reader's does, so the window's own opening is the only one; moving
   within the open viewer opens nothing more.

`results_url` and the target of `Results` are the portal's home page,
`<base>/FrmAccueilFrame.asp`, a frameset a reader searches from: the search
lives in the session. The window's start page is the portal's stylesheet,
`<base>/css/general.css`, a page of the origin that the portal's anti-bot
measure guards like the others and that navigates nowhere; the home
frameset's frames send the top page on to the summary about a second after
it loads, which would lose the requests running in it.

Requests: Ille-et-Vilaine 9 (steps 1 to 4, the redirect followed, and step
7), Corsica 7, in the window. The live check's step 1 opens the session and
reads the form (step 2), the localities being the labels' names with a
hamlet or a placeholder (`AUTRES DEPARTEMENTS (…)`) passed over; step 2
searches as above; the chosen register of an `ark` collection is counted
from its slide file, one more opening, and a `register` collection's is not
counted: it is cited at its first view and step 3 expects a `View` without
views (§9.1). The viewer shows the current view in `input#imageNum` and the
count in `h3#imageNumMax` (` / 13`), with its image tiles aborted; step 4
then brings a `register` collection's viewer to the middle of the views it
counts, changing `input#imageNum` as the window does.

**Collections.** Ille-et-Vilaine (`MOD` 10, 16, 17, 15): the registers of
acts (parish registers, civil status and their tables in one search), the
censuses (by commune, an interval of years), the military registers (by
place, where any commune finds its recruitment subdivision's volumes of a
class, and the subdivisions themselves, `EXAMPLEVILLE (SUBDIVISION
MILITAIRE)`), and the tables of successions and absences (by registration
office). Corsica (`MOD` 4 and 11): the registers of acts and the censuses
(by commune or hamlet, years). Not covered: Corsica's military registers
(`MOD` 12), an index of soldiers searched by name whose search by class
alone exceeds the portal's maximum (`Le nombre de résultats dépasse le
maximum autorisé`); Ille-et-Vilaine's `bans` and `catholicite` types, which
no act code names; and the modules that are not registers of persons
(notaries, mortgages, maritime registration, prison registers, maps,
press). The Rennes civil status is published by the city, not on this
portal.

**Etiquette.** Neither portal publishes a `robots.txt` (an IIS `404`); the
analytics beacons the pages send are the browser's, never the adapter's.

### 4.10 Pleade

Observed on the Mayenne and Pyrénées-Atlantiques portals (2026-10-05 and
2026-10-06)[^pleade-portals], which embed AJLSM's Pleade, an EAD publishing
software, in a Drupal site. Registers are components of EAD finding aids;
each opens in a Mirador viewer at its ARK, view `n` at
`<path>/ark:/<naan>/<name>/f<n>` (one-based, the canvas order), with or
without a `?context=` naming its component. The manifests are IIIF
Presentation 2 with Image API 2 level 1 services, but neither portal's terms
are attribution-only (Mayenne reserves reproduction to personal use; the
Pyrénées-Atlantiques manifests declare a click-through licence) and
Mayenne's CORS names another host: both archives are `display: "portal"`.
A collection finds its register in one of two ways, its `mode`. The
`portal` settings are:

| Setting | Content |
|---|---|
| `origin`, `transport` | The portal and its access; `any` on both portals, which have no anti-bot measure. |
| `path` | The Pleade mount, `/archives-en-ligne`. |
| `mode` | `form` (Mayenne's registers) or `tree` (Pyrénées-Atlantiques' registers, Mayenne's military registers). |
| `form`, `results` | `form` mode: the form page and the results fragment the page itself requests, under `path`: `etat-civil-search-form.html`, `functions/ead/custom-results.ajax-html`. |
| `criteria` | `form` mode: the numbers of the form's inputs, `query<n>` for the `locality` and the `kind` of document, `du<n>` (and `db<n>`, `de<n>`) for the `year`. |
| `kinds` | `form` mode: the kind criterion's values for `registers` and `tables`: `Registres d'actes`, `Tables`. |
| `aid` | `tree` mode: the finding aid's identifier. |
| `locality_depth`, `locality_label` | `tree` mode: the depth of the aid's table of contents at which the localities' nodes stand (its first level being 1), and how they are titled, with one `{locality}`: `{locality}` at depth 2, `Bureau de recrutement de {locality}` at depth 5. |

A `form` collection holds acts and tables; a series is searched in `tree`
mode.

Resolution, `form`:

1. `GET <path>/<form>`: the form (`form.pl-form-advanced-srch`, 123 kB),
   whose hidden inputs the search sends as the page does, whose locality
   select (`query<locality>`, 364 thesaurus labels) and kinds select must
   be there. The labels naming the cited locality are those written as
   cited or naming it (§4.9, step 3): `Exampleville (Exemple, France)`, a
   former commune `Exampleville (Exemple, France ; jusqu'à 1919)
   [aujourd'hui : …]` — a commune and its former namesake both, three at
   most —, failing any those naming it under another name (§5.1, *Other
   names of the locality*). The search matches a label exactly:
   `Exampleville` alone finds nothing.
2. For each label, `GET <path>/<results>?<hidden inputs>&query<locality>=<label>&query<kind>=<kind>&du<year>=<year>&db<year>=&de<year>=`,
   the kind being the registers' or the tables' value, the year empty
   without one. The year is an interval test on each register's period
   (`1850` finds `1843-1852`); put in `de<year>` alone it is ignored. The
   fragment says `<n> résultats` (`span.nbresults`) and `Page 1 de N`, or
   `Aucun résultat` (`h4.pl-results-zero`); neither is a changed shape.
   Rows (`tr.odd`, `tr.even`, 20 a page) give the viewer's ARK
   (`td.img-tab a`), the locality (`td.commune`), the period (`td.date`),
   the kind and acts, one paragraph each (`td.type p`: `Registres d'actes`,
   `Baptêmes`, `Mariages`; `Tables`, `Tables décennales des naissances`), the
   call number (`td.cote`). Further pages add `&p=<n>`, read until the
   citation decides, five at most; a register two labels list is one.
3. One register is selected as for Arkothèque (§4.3, step 3).
4. `Results` opens the form's own results page, `<action>?<the same query>`.

Resolution, `tree`:

1. `GET <path>/functions/ead/get-toc-fragment/<aid>/<aid>.ajax-html`: the
   aid's table of contents, one fragment per node asked for, each listing
   the node's children and grandchildren (`ul#treeRoot`, nested `li` with
   the title in `a[name=link]`), marked `image_illustrated` (the node is a
   register) or `anc_illustrated` (some descendants are); a child list left
   empty is read from the child's own fragment,
   `…/get-toc-fragment/<aid>/<node>.ajax-html`. The Pyrénées-Atlantiques
   aid's own fragment lists its 556 communes (152 kB).
2. The walk keeps, at the localities' depth, the node titled with the cited
   locality (the title less the label's text, read as a citation writes it:
   `Hauts-Exemples (Les)` is `Les Hauts-Exemples`); elsewhere it prunes a
   node whose title names another kind of document — in the citation
   vocabulary's series words (`Registres matricules du recrutement`, `RM`;
   the mobile national guard's lists, `CM`), a table (`Tables décennales
   (TD)`), the kinds (`Baptêmes, mariages, sépultures`, `Naissances`) — or,
   a title of years alone (`1793-1806`, `Classe 1900`), other years than
   the cited one. It reads the fragments of the nodes it keeps, six at
   most; a node left unread is counted among the results. Registers deeper
   than the localities are the candidates, with their branch's kind and
   years, the matricules their title spans and the call number after its
   bullet (`Matricules 503-1004 • R 9002`).
3. One register is selected as for Arkothèque (§4.3, step 3). The
   Pyrénées-Atlantiques registers have no call number; each commune's
   civil registers come in two copies, the departmental and the communal
   collection, which only the image count tells apart.
4. Each register left, three at most, is read from its component,
   `GET <path>/ead-fragment.xsp?c=<component>`: its viewer link
   (`a.pl-pgd-medias-thumbnail`, its ARK), its call number
   (`td.pl-tbl-unitid`) and dates (`td.pl-tbl-unitdate`), which date a
   whole collection the table of contents titles only `Collection
   départementale`. A register without a viewer link is drift. Then
   selection again.
5. `Results` opens the aid, `<path>/ead.html?id=<aid>`.

Both modes, when several registers remain, three at most, and the citation
gives its image count: `GET <path>/iiif/ark:/<naan>/<name>/manifest.json`
for each (85 to 280 kB), whose `sequences[0].canvases` count its views; the
one register of the cited count is chosen, with that count. Otherwise
`Results` with the number left. The target is
`<origin><path>/ark:/<naan>/<name>/f<view>`, the ARK of the view as `ark`;
a register is counted only where its manifest was read, so a cited view is
otherwise left to the viewer. `results_url` is the form page or the aid,
built without a request; the window's start page is the origin's
`/robots.txt`.

Requests: Mayenne's registers 2 (the form, one search), 3 to 6 with a
former namesake or the manifests; Pyrénées-Atlantiques 4 to 8 (the aid, the
commune, its copies' kinds and years, the components, the manifests);
Mayenne's military registers 4 (the aid, its collection, the class with its
offices, the volume's component).

**Collections.** Mayenne: the registers of acts (parish registers, civil
status and tables, one form over the digitized series), and the military
registers (`FRAD053_2NUM109_RM`, by class, recruitment office and volume,
1865–1940). Pyrénées-Atlantiques: the registers of acts (`FRAD064003_IR0002`,
by commune, kind, copy, act and period). Not covered: Mayenne's lists of the
mobile national guard's contingent (classes 1865–1871), whose classes name
no office; the Pyrénées-Atlantiques "Registres militaires" page, which
answered a server error, and its other finding aids (the M series has no
digitized component); neither portal publishes censuses or tables of
successions.

**Etiquette.** Both `robots.txt` disallow the results pages
(`/archives-en-ligne/custom-results.html`, `/archives-en-ligne/*search*`,
`/*query`), which only a reader's click requests (§8); the tables of
contents, fragments, ARKs and manifests are not disallowed. The
Pyrénées-Atlantiques one asks `Crawl-delay: 10`, which the live checks
honour. Both name AI crawlers they refuse; OxidGene is not one, and acts
only on a reader's request.

### 4.11 Bach

Observed on the Gard, Haute-Marne, Tarn, Tarn-et-Garonne, Vaucluse and
Guadeloupe portals (2026-10-05 and 2026-10-06), all run by Anaphore's Bach
(`Développé par Anaphore` in every footer) on one hosting address. Bach
publishes EAD finding aids, server-rendered, beside a full-text search that
`robots.txt` keeps robots from and that the adapter never uses: a register
is a component of a finding aid, and has no search of its own. The portal's
classification scheme (`/archives/classification-scheme`) lists the finding
aids — one per commune on four portals, one for every commune on the
others — and a finding aid's page (`/document/<aid>`) shows its whole tree,
82 KB for a commune to 4.4 MB for a department's censuses. A register's show
page (`/archives/show/<aid>_<node>`) links to the viewer, a separate
application on the portal's origin (`/viewer/`) or on its own, whose JSON
image list (`/api/info/series/<folder>`) names the files, CORS `*`. The
viewer opens on a file named in `img`; it is not IIIF, its images refuse
other origins (`Cross-Origin-Resource-Policy: same-origin`), and none of the
archives' terms is attribution-only: every archive is `display: "portal"`.
The `portal` settings are:

| Setting | Content |
|---|---|
| `origin`, `transport` | The portal and its access: `browser` for the Gard portal, whose Anubis check guards the show pages and the viewer (the classification scheme and the finding aids answer the identifying agent, not a browser's). |
| `viewer` | The viewer's root: its own origin (`https://viewer.example.org`), declared as the endpoint's other origin, or a path on the portal's (`https://archives.example.org/viewer`). |
| `views` | Where the image names come from: `api` (default), the viewer's list; `range`, the first and last names a link carries (`s`, `e`), counted between them — for the Gard portal, whose viewer host is behind its own check, out of reach of a request from the portal's page. |
| `inventory` | `{"kind": "classification", "prefix", "locality", "title_prefixes"}`: the classification scheme's entries (`li.standalone`) linking to `/document/<prefix>…`, the locality being the entry's `title` (`span.cdc_unittitle`) or its `link` text, after one of the `title_prefixes` an entry must start with (`Registres paroissiaux et d'état civil :`), case, accents and punctuation aside. `{"kind": "document", "document", "level"}`: one finding aid whose nodes at `level` (1 for its top nodes) are the localities; without a level, the aid is searched whole, the cited locality aside (a series of one office). |

Resolution:

1. For a `classification` inventory, `GET /archives/classification-scheme`
   (80 to 340 KB): the entries whose locality is the cited one, as cited,
   with its article behind it or without it. None or several give `Results`
   on that page with their count. A page without the collection's entries is
   drift; an anti-bot page, `Challenged`.
2. `GET /document/<aid>`: the tree (`div.css-treeview`), each node an
   `a.display_doc` link to its show page with its title
   (`span.unit_title_lb`), dates (`span.date`) and call number
   (`span.unitid`), its depth the nesting of the lists, a register being a
   leaf. The aid's own title (`#cdcTitle h2`) is the root of every register.
   For a `document` inventory with a level, the registers are those under
   the nodes of that level naming the cited locality: the place itself
   (`AVAL-EXEMPLE`, `Mas-d'Exemple (Le)`) or an office of it (`Bureau de
   recrutement d'Exampleville`, `Consistoire de Saint-Exemple`), a
   parenthesis after it aside.
3. Each register is read from its title and its ancestors': its period from
   the first of its own dates, its own title, then its ancestors' that holds
   a year (`mars 1564-avril 1566`, `1793-an X`); its parish from the nearest
   title naming one (`Paroisse Saint-Exemple`); its numbers from its title
   (`N° 501-1000`); and what it holds, never from letters. A table is a
   register under a node naming one, or whose title only names one
   (`Tables décennales`, `TD`; `Répertoires annuels`, `TA`); a register's
   acts are the kinds the nearest title names in words, item by item
   (`Naissances, mariages, décès, tables décennales` holds both acts and
   tables), publications of banns apart from the marriages; failing that,
   the kinds its nearest collection implies (`Registres paroissiaux`: `BMS`;
   `État civil`: `NMD`). The candidates are the registers of the cited year
   (one showing no year kept) that hold the cited document, those naming it
   first, then those filing it with another kind (banns with marriages), then
   those implying it, then those saying nothing; for a series, those under a
   node naming it first and an index (`Répertoire alphabétique`) last. One is
   selected as in §4.3 step 3, the call number only breaking a tie, since the
   Vaucluse communes' aids show none and others share one between several
   registers; a selected register showing another call number than the cited
   one is not it, and gives `Results`, which leaves the next collection to
   try (the Haute-Marne registers hold decennial tables too).
4. `GET /archives/show/<aid>_<node>`: its viewer links,
   `figure#relative_documents a[href^="<viewer>/series/"]`,
   `<viewer>/series/<folder>` with the link's own query (`s`, `e`,
   `levelDescription`). None, a register without images, or several give
   `Results` on the register's place in its aid (`/document/<aid>#<node>`).
   A link to another viewer is drift.
5. With `views: "api"`, `GET <viewer>/api/info/series/<folder>` with the
   link's query (a folder of several registers answers 404 without its
   range): `count` images, `data[i].name` the file of view `i + 1`. An empty
   list gives `Results`.
6. The target is the link with `img=<name of view n>` (`&` after its query,
   `?` without), which the viewer opens on that view, numbered within the
   link's range; never `s=<view n>`, which renumbers the views. A view
   beyond the count gives `View` with no views, on the link. With `views:
   "range"`, a link naming no range leaves the count unknown and the view to
   the reader.

`results_url` is the classification scheme, or the collection's finding
aid. The endpoint's start page is `/robots.txt`, or for a `browser`
collection the classification scheme, a page the anti-bot check guards, so
that the window passes it before the requests. Requests: 3 or 4 (2 or 3 for
a `document` inventory); `robots.txt` asks `Crawl-delay: 10`, which the
live checks follow.

### 4.12 Visualys

Observed on the Côtes-d'Armor archives' "salle virtuelle"
(`sallevirtuelle.cotesdarmor.fr`, 2026-10-05 and 2026-10-06), an ASP.NET
WebForms application whose windows are named `VisualysInfo` and
`VisualysConsult`; no other portal is known to run it. One origin serves a
site per family of documents, `/<family>/<site>`: `/EC/ecx` for the parish
and civil registers, `/RM/rmx` for the military registers, others for maps,
notaries' repertories and the registration offices' *contrôle des actes*.
A visitor enters a site by its home page's "Entrer",
`connexion.aspx?ref=demo&res=<width>x<height>`, which opens an ASP.NET
session and shows the archive's reuse licence (`licence.aspx`); accepting
it (`btnAccepter`) leads to the site's search page (`commune.aspx`). The
portal does not enforce the licence — every page answers a session that
never accepted it, and the viewer (`consult.aspx?image=<id>`) answers
without a session — but the reader must pass it: OxidGene never accepts it
on the reader's behalf. The adapter therefore searches as a visitor whose
licence is pending, finds the cited register and, on the register's sheets
of thumbnails, the address of the cited view; its target — the view, or the
locality's list of lots — stands behind the licence, which the adapter
declares (`Platform::licence`: the site's entry, `licence.aspx`, the site's
root). A client opens the entry first, and the desktop window goes on to
the target once the reader has accepted the licence themselves (§6.1). The
images are plain JPEG files, with no IIIF service and no call number on the
registers' lots: the archive is `display: "portal"`. The `portal` settings
are:

| Setting | Content |
|---|---|
| `origin`, `transport` | The portal and its access; `any`, the portal having no anti-bot measure. |
| `base` | The site's path: `/EC/ecx`, `/RM/rmx`. |
| `mode` | `localities` (the registers) or `search` (the military registers). |
| `locality_style` | `localities` mode: how the alphabetical list writes a locality, `article_suffix` (`Bourg (Le)`). |
| `acts` | `search` mode: the kind-of-register select's value of each series (`{"RM": "Registre matricule"}`). |

A `localities` collection holds acts and tables only.

Resolution, `localities`:

1. `GET <base>/connexion.aspx?ref=demo&res=1920x1080`: the visitor's
   session, the redirect to the licence page followed. Its acceptance is
   never posted.
2. `GET <base>/commune.aspx?lettre=<initial>`, the initial of the cited
   locality as the list writes it (`B` for `Bourg (Le)`): rows of a
   locality (`javascript:lot('<id>')`), with the parish or body a row is
   for when a town's are listed apart (`Saint-Exemple`, `Hospice`, `Etat
   civil protestant`) and the extreme dates. The rows naming the locality
   are kept: the cited parish's when it has one, otherwise the locality's
   own (listed without a parish), otherwise all of them. None, or more than
   three, gives `Results` with that number.
3. For each row, `GET <base>/plage.aspx?id=<id>`: the locality's lots in two
   blocks, parish registers and civil status, each listed only while the
   session holds it open (its icon `tree_moins`, closed `tree_plus`). A
   block holding the cited act — baptisms and burials in the parish block,
   births, deaths and tables in the civil one, a marriage cited alone by its
   year (before 1793 in the first, from 1793 in the second, both without a
   year) — that is closed is opened by its toggle, `GET
   …&r=0` or `…&r=1`, which redirects to the page; one already open (the
   window's session from an earlier lookup) is not toggled, which would
   close it. Each lot gives its first and last years, its acts as codes
   (`BMS`, `N`, `TD`) and its image count.
4. One lot is selected as for Arkothèque (§4.3, step 3), the row's parish
   as its parish. Several lots, or none, give `Results` on the row's list of
   lots (`plage.aspx?id=<row>`) when one row was read, at the site's entry
   otherwise.
5. For each cited view within the lot, `GET <base>/planche.aspx?id=<lot>&page=<sheet>&width=1400&height=900`,
   the lot's sheet of thumbnails holding it — 24 a sheet at that size, sheet
   `⌈n / 24⌉` —: the thumbnail numbered with the view (`td.MiniNum`, `36.`)
   opens its image (`javascript:ouvrir('<image>')`), whose viewer is
   `consult.aspx?image=<image>`. One request per sheet; a sheet that does not
   number the view leaves every view unaddressed, and a page without
   numbered thumbnails is drift. The target is `View` with the cited views,
   the lot's image count and no call number, on the first view; without a
   view, or one beyond the lot, `View` with no views on the row's list of
   lots, where the reader opens the lot and goes to the view (§6.1).

Resolution, `search`:

1. The visitor's session, as above.
2. `GET <base>/commune.aspx?lettre=*`: the search form (`frmRecherche`),
   whose state fields (`__VIEWSTATE`, `__EVENTVALIDATION`) the search
   posts back, with its selects: years (`lstAnnee1`), offices
   (`lstBureau`) and kinds of register (`lstRegistre`), which must offer
   the settings' value.
3. `POST` the form's action with its state, `lstAnnee1=<class>` when the
   cited year is listed, `lstBureau=<office>` when the cited locality is an
   office, the settings' kind and `btnFind`. The portal requires a year or
   an office: a citation with neither gives `Results` built without a
   search. The answer lists the volumes, office, class, call number and
   lot, without image counts.
4. One volume is selected as in §4.3 step 3, the office as its locality —
   any office's when the cited locality names none —, the call number
   telling a class's volumes apart. Its cited views are found on its sheets
   as in step 5 above, the volume's size unknown; the target is `View` with
   them and the volume's call number, or with no views on the volume's
   sheets (`planche.aspx?id=<lot>`). Several volumes, or none, give `Results`
   at the site's entry.

`results_url` is the site's entry; the window's start page is the sites'
shared stylesheet (`/<family>/slv.css`), which opens no session. Requests:
the registers 3 to 6 (entry, list, lots, up to two toggles, each followed by
its page), the military registers 3, and one per sheet holding a cited
view. The lots the adapter's toggles opened stay open in the session, which
the window shares with its requests: the list of lots the window goes on to
shows them.

**Collections.** The registers (`/EC/ecx`: parish registers, civil status
to 1922 and decennial tables, one alphabetical list) and the military
registers (`/RM/rmx`, classes 1867–1921, by class, office and kind). Not
covered: the census lists and the tables of successions, published on the
archive's finding-aid portal (`recherche.archives.cotesdarmor.fr`), another
engine, which did not answer during the survey; the *contrôle des actes*
(`/SA/sax`, 1693–1791, by registration office), which no series code names;
and the military registers' alphabetical tables (the kind `Table`), which
are not registers.

**Etiquette.** The portal publishes no `robots.txt` (an IIS `404`).

### 4.13 Gers portal

Observed on the Gers archives' portal of digitized archives
(`www.archives32.fr/archives_numerisees/portail/`, 2026-10-05 and
2026-10-06), a PHP application of its own (Bootstrap and DataTables). Each
family of documents is a module with a search form,
`<module>/recherche/`, and a viewer, `<module>/visu/`: the civil status
(`etats_civils/ec`), the parish registers (`etats_civils/rp`), the decennial
tables (`etats_civils/td`), the censuses (`recensement_population`) and the
tables of successions and absences (`successions_absences`). Every page
sits behind the bot-mitigation redirect of §4.2 (`/redirect_<token>/…`),
which only a browser passes: every collection is `transport: "browser"`.
The civil status was digitized with FamilySearch, whose written
authorisation its massive reuse requires for ten years, and the viewer
serves plain images: the archive is `display: "portal"`. The `portal`
settings are:

| Setting | Content |
|---|---|
| `origin`, `transport` | The portal and its access, `browser`. |
| `path` | The module's path: `/archives_numerisees/portail/etats_civils/ec`. |
| `fields` | The names of the form's locality lists: `locality` (`lieu`, `LIEU`; the communes, or the registration offices of the tables of successions) and, where the form has one, `former` (`ancienne`, the former communes). |
| `acts` | The checkboxes searching each document kind (`{"N": ["chk_naissance"], "P": ["chk_pub_mariage"]}`, a table's every box); a combined act searches each kind's, banns the marriages' where the form has no box of their own. Left out for a module without kinds (censuses, tables of successions). |
| `views` | `contiguous` where a register's views have consecutive identifiers (civil status, parish registers, tables), `listed` where only the viewer's list orders them (censuses, tables of successions). |

Resolution:

1. `POST <path>/recherche/`, form-encoded: the locality list's field set to
   the cited locality as written, the former communes' to `all`, empty
   years — the portal's year filter returns registers outside the years
   asked, so the periods decide —, the act's checkboxes, `valider`. The
   answer repeats the form with its lists, whose values are the labels
   (some with a trailing space the search needs). When the list does not
   hold the cited locality as written, the search is sent again with the
   label naming it, case, accents and punctuation aside — a commune's, or
   failing one a former commune's (`lieu=all&ancienne=<label>`), or failing
   both the one label of either list naming it under another name (§5.1,
   *Other names of the locality*) —; no label gives `Results` with no
   match. An answer without the locality list
   is a challenge when it bears one's signature, a changed shape otherwise.
2. The answer's table (`table.tableau_td`) lists every register at once,
   its columns named by their headings (`Commune` or `Bureau`, `Ancienne
   commune`, `Paroisse`, `Annexe`, `Période` or `Année`, `Cote`, `Contenu`,
   `Vues`; commented-out headings ignored): the locality (the former
   commune's when one was searched), the parish or else the annex, the
   period, the call number, the acts from the content's first line
   (`Naissances / Mariages / Décès`, before notes that may name other acts),
   the image count (`393 vues`) and the viewer's query
   (`../visu/?id=<register>&fichier=<first view>&lieu=…&annee=…`, `td=`,
   `rec=` or `sa=` by module).
3. One register is selected as for Arkothèque (§4.3, step 3): the
   decennial tables list two copies of each decade, the court clerk's and
   the prefecture's, told apart only by the call number or the image count.
4. The target is the viewer at each cited view: `fichier=<first + n − 1>`
   for `contiguous` views, built without a request; for `listed` views, `GET
   <path>/visu/?<query>`, whose `select#fichier` lists the views'
   identifiers in order, gives view `n`'s and the count. A view beyond the
   count gives `View` with no views (§7); a citation naming no
   view sends no viewer request.

`results_url` and the target of `Results` are the module's search form,
since its search is a `POST`; the window's start page is the site's
`/robots.txt`, behind the same redirect. Requests: 1 search, 2 when the
list writes the locality otherwise, plus 1 viewer page for a `listed`
module's cited view.

**Collections.** Parish registers (to 1792), civil status (1792–1934),
decennial tables (1792–1932), censuses (1836–1946) and tables of
successions and absences (by registration office). Not covered: the
military registers, which the portal searches by the soldier's name only
(`matricules_militaires/rm/`), their alphabetical tables by class
(`…/trm/`), which are not registers, and a third military search whose page
answers "not found".

**Etiquette.** The site's `robots.txt` disallows only `/wp-admin/`.

### 4.14 Older Mnesys interface

Observed on the Savoie archives' search (`recherche-archives.savoie.fr`,
2026-10-05 and 2026-10-06), which runs the interface Naoned's Mnesys had
before Mnesys Expo ("iNAO, powered by Naoned"): nothing of the Expo
protocol (§4.4) applies. Each guided search (`/?id=recherche_guidee_…`)
is a `GET` form whose fields are named after the finding aids' elements: a
locality matched by word against the place index (`form_search_geogname`,
so that `Exampleville` also finds `Exampleville-le-Vieux`), the dates
(`form_search_unitdate`, the exact year in `form_search_unitdate3` as the
page's script copies it), the digitized filter (`form_search_dao=oui`), and
selects whose generated names (`form_search_v2_field_<id>`) are matched,
through a companion field (`form_req_v2_field_<id>={:unittitle}__VAL_`),
against the nodes' titles: the kind of act, the military class. Its hits
are nodes of EAD finding aids — registers, and the headings over them —
twenty to a page, the further pages kept in the session. A register's
notice links its images on a viewer host of its own
(`archives-numeriques.savoie.fr`), at the register's ARK, whose viewer
opens view `n` with `?vue=<n>` and keeps it through its redirect. That host
turns away clients other than browsers and, until it is fixed, serves its
certificate without its issuer (§6.1): the adapter never requests it. Its
footer requires an authorisation for any Internet diffusion, and it serves
no IIIF: the archive is `display: "portal"`. The `portal` settings are:

| Setting | Content |
|---|---|
| `origin`, `transport` | The search's host and its access, `any`. |
| `form` | The guided search: `recherche_guidee_etat_civil_web`, `…_recensement_population_web`, `…_registres_matricules_web`. |
| `locality` | The place field, `geogname`; left out where the search has none (the military registers, whose recruitment office is one). |
| `act`, `acts` | The kind-of-document select (`v2_field_<id>`) and its value for each document kind (`Baptêmes OU naissances`, `Tables`); a combined act by its first kind's. Left out for a search without kinds (censuses). |
| `year` | A title-matched select carrying the year, `field` and `value` with one `{year}` (`"Classe {year}"`, quotes included); the date fields otherwise. |
| `viewer` | The viewer host's origin, where the registers' ARKs open. |

Resolution:

1. `GET /?<the form's fields>&display_thesaurus=autocomplete&action=search&id=<form>`:
   the cited locality as written, the year (the date fields, or the
   `year` select's value), digitized documents only, the kind's value. The
   answer counts its hits (`span.nb_reponses`: `20 réponses`, `Aucune
   réponse`) and lists them (`div.list li`): the period (`div.date`), the
   call number (`div.cote strong`), the title and the notice's link
   (`div.title a`), the breadcrumb of the node's ancestors (`div.ariane`),
   older copies of the cells left in comments ignored. An answer without a
   count is the form again, which shows no hit, or a challenge or a
   changed shape.
2. A node without a call number is a heading over registers that the search
   lists themselves, and is left out. A node's place is the breadcrumb entry
   or the title's end after a dash (`Registre paroissial : mariages. -
   Exampleville.`) naming the cited locality, each read up to its first
   full stop (`Exampleville. 1876-1936 (6M 1-12)`); failing one, an entry
   holding the cited name in a longer one, which the word search matched
   too (`Exampleville-le-Vieux.`, a list `- Sampleton, Exampleville,…`), is
   another place; failing both — a title cut short, a collection's heading —
   the node is the place index's answer for the cited locality. Its parish is the
   breadcrumb's next entry when that names no kind; its act the kinds its
   title and breadcrumb name, a table (`Table chrono-alphabétique des
   baptêmes…`, `Tables décennales…`) being `TD`, the portal's own filter
   for a series; the matricules a military register's title spans
   (`volume 2, n° 503 à 1003`).
3. One register is selected as for Arkothèque (§4.3, step 3); while none is
   singled out, further pages, `GET /?id=<form>&doc=&page=<n>&page_ref=`
   in the session, are read, five pages at most. A military register is
   searched without a locality, and found by its class and matricules.
4. `GET` the chosen node's notice (`/?id=<form>_detail&doc=…&page_ref=<node>`):
   its link to the digitized document (`a.media_link`, `- Voir : <call
   number>`) must be an ARK of the `viewer` host. The target of view `n` is
   `<ARK>?vue=<n>`, built without any request to that host, `View` with
   every cited view and no view count, since only the viewer host knows the
   register's size.

`results_url` and the target of `Results` are the search's own address,
built without a request; the window's start page is the search host's
`/robots.txt`. Requests: 2 (the search and the notice), one more per
further page.

**Collections.** The registers (parish registers, civil status and their
tables, 1501–1932, in the communal, diocesan and court-clerk series), the
censuses (1561–1946 online) and the military registers (classes 1867–1921).
Not covered: the military registers' alphabetical repertories, which are not
registers, and the land registers and notarial searches.

**Etiquette.** `robots.txt` disallows `/?*action=search` and the other
query actions: the search runs only on a reader's click, one per lookup
(§8), as for Ligeo. The viewer host sends a client naming OxidGene to
another site, the live check's browser included: its step 4 ends
`challenged`, unverified (§9.1), while steps 1 to 3 run on the search host.

### 4.15 Archives nationales d'outre-mer (CAOMEC2)

Observed on the civil-status search of the Archives nationales d'outre-mer
(`anom.archivesnationales.culture.gouv.fr/caomec2/`, 2026-10-05 and
2026-10-06), an XHTML application in ISO-8859-1 beside an OpenSeadragon
viewer (`/osdanom/`) of Deep Zoom images, which serves the registers the
colonies sent to the *Dépôt des papiers publics des colonies* — for
Guyane, La Réunion and Mayotte the civil status OxidGene reaches there
rather than through a local service (§11.3). It answers over plain `http`
only, the catalogue's one named exception (§3.1), with no cookie, no
anti-bot measure and no `robots.txt` (a `404`). One search serves every
territory, its value a parameter (`territoire=GUYANE`, `REUNION`,
`MAYOTTE`): one collection per territory. A row is a register of one
commune's year, of one kind of act or of all of them (`Tous actes`); it
has no call number and no image count, and its viewer
(`osd.php?territoire=&commune=&annee=&typeacte=`) has no address per view:
it ignores every parameter naming one. The images are not IIIF: the
archive is `display: "portal"`. The `portal` settings are:

| Setting | Content |
|---|---|
| `origin`, `transport` | The portal and its access, `any`. Its `http` origin needs the collection's `insecure_http` (§3.1). |
| `territory` | The search's value of the collection's territory, in capitals. |
| `code` | The archive's citation code naming this territory alone (`ANOM973`), if any: a citation with another territory's code does not search this collection. |

A collection holds acts only (`B`, `M`, `S`, `N`, `D`): the search's tables
and its other kinds (manumissions, recognitions, judgements, stillbirths)
are not cited as registers.

Resolution:

1. A citation whose code names another territory than the collection's is
   not searched: the collection answers its offline `Results`, which a
   search of another collection outranks (§5.2).
2. `GET /caomec2/recherche.php?territoire=<T>`: the territory's form, whose
   commune list (`select[name=commune]`) writes communes in capitals without
   accents, a hospital or a penitentiary beside its commune
   (`EXAMPLEVILLE (HOPITAL)`). The cited locality is the label equal to it,
   case, accents and punctuation aside, its article in front or behind
   (§4.3); none gives `Results` with no match on the form.
3. `GET /caomec2/resultats.php?territoire=<T>&commune=<label>&annee=<year>`
   with every other field of the form empty, every kind at once: the count
   (`strong#results-nb`; `p.nores` for none) and the rows of the first page,
   twenty at most, each with its commune, year and kind (`td.commune`,
   `td.annee`, `td.acte`). A page without the count is drift.
4. Each row holding a cited act is a candidate: `Tous actes` holds every
   one, `Naissance` births and baptisms, `Mariage` marriages, `Décès`
   deaths and burials — read letter by letter, since the transports decode
   the page as UTF-8 and its accents are lost —, any other kind none. One is
   selected as for Arkothèque (§4.3, step 3), the year its period, and a row
   of another kind is never the cited register.
5. The target is `View` with no views, no view count and no call number,
   on the row's viewer (its address rebuilt from the row, the commune
   percent-encoded): the register's first view, which the window brings to
   the cited view with the viewer's page input and the Enter key, or names
   the view to go to when it cannot (§6.1). None, or several, give `Results` on the search's page.

`results_url` is the search's page for the commune as the portal writes
it (`SAINT-ETIENNE-D'EXEMPLE`), whose search ignores case, or the
territory's form without a locality; the window's start page is the
territory's form. Requests: 2.

**Collections.** Guyane (from 1717), La Réunion and Mayotte (1844–1906),
births, marriages and deaths, the parish registers' baptisms and burials
among them. Not covered: the decennial and annual tables, whose rows the
survey did not date, and the territories outside the three departments,
which the search serves too.

**Etiquette.** The live check reads the chosen register's viewer once to
count its views (§9.1); the resolution never requests it.

## 5. Contract

### 5.1 Citation recognition

Genealogists cite registers in many ways: the archive first and the act last,
separated by commas (`AD72, état civil de Exampleville, naissances 1872, cote
4E 1234, vue 45/200, acte n° 312`); a bare call number and view (`AD72, 4E
1234, v. 45, n° 312`); the normalized form below; or, in a GEDCOM-shaped
tree, the register as the source, the archive as its repository with the call
number on the link, the act and view in the citation's page, and the act's
kind, year and place on the cited event. `ArchiveRegistry::recognize` reads
all of them with one engine that knows no language: the words it reads are
data per language, and a country's conventions data per country (below).
French is the one shipped.

**What is read.** Per citation, `CitationEvidence` gathers:

| Input | Read for |
|---|---|
| Source title, abbreviation, author, publisher, agency | Every part, in words |
| Citation page | Every part in words, its views, number and folio first |
| Citation text | Web addresses only: a transcription's names are the act's people |
| Repositories holding the source | The archive, from the name or the website; the link's call number |
| Media linked to the source held as addresses, the pages of a linked document included | Web addresses |
| The cited event, when the citation is attached to one | Its kind, year, place and agency |

**Order of reliability.** Each part is taken from the most reliable signal
that gives it, and keeps that signal (`Recognition::signals`: `reader`,
`address`, `normalized`, `record`, `words`, `event`), which tests and
diagnostics read:

1. what the reader supplied in the "Find in the archives" dialog (§6.5);
2. a portal address of a catalogued archive: an address on an origin of the
   archive's website or of one of its collections' endpoints, beyond the home
   page, is the target as it is, opened without any lookup;
3. the normalized form, a strict first pass (below);
4. the records' dedicated fields: a repository's name or website naming the
   archive, and the call number of the link of a repository naming it (or of
   the only repository);
5. the words of the source and the citation;
6. the cited event, which only completes what the records leave out: its
   kind gives the act, its year the year, its place the locality. The event
   says what happened and the records which register holds it, so a birth
   cited from `baptêmes` stays a baptism, a register's period wins over the
   act's year, and a combined register (`NMD`) keeps its kinds — the event
   only narrows it to the one it is among them (a death cited from an `NMD`
   register is searched as a death, never as the register's first kind). The
   event never designates the archive: it happened where the record was not
   necessarily written — a death in one department may be cited from the
   birth register of another.

**The archive.** It must be designated: by a portal address, by the
normalized form's code, by a repository holding the source (its name or its
website), or in the words by a catalogue citation code written over one to
three tokens (`AD44`, `AD 44`, `A.D. 44`), by its `name` or an alias, or by
a kind of archive of the vocabulary followed by an area the catalogue gives
it or one of its `jurisdiction` codes (`Archives départementales de la
Sarthe`, `Arch. dép. 72`, `Archives de la Loire-Inférieure`, `AM
Exampleville` for a municipal archive). A designation wins absolutely: a
code or a kind of archive followed by an area or a code the catalogue does
not list (`AD98`, `Archives départementales de l'Imaginaire`), in the text
or a repository's name, leaves the citation without a link (`no_adapter`) —
no other archive ever stands in for it. Nothing else designates an archive:
neither an area written alone nor the event's place.

**The words.** A text is cut into segments at commas, semicolons, vertical
bars, line breaks, parentheses and spaced dashes, and each segment into
tokens: an elision is split from its word (`d'Exampleville`), an abbreviation
from the number glued to it (`n°312`, `v.45`). Tokens are compared folded —
case, accents and punctuation set aside, a degree sign after a letter read as
the `o` it abbreviates (`n°`, `f°`, `v°`), a word of three letters or more
also matching its plural in `s` or `x` — against the phrases of every
vocabulary, the longest first. A keyword decides what follows it, whatever
the order of the segments:

| Keyword | Reads |
|---|---|
| A kind of archive | The archive, by the area or code after it |
| An act, a combined register's code in capitals (`BMS`, `NMD`, `N 1877`), a table or a series | The document kind; a year or period after it in the segment is the register's; a preposition after it introduces the locality (`état civil de Exampleville`) |
| A register word (`registres paroissiaux`, `état civil`) | That the text cites a register, parish or civil, which reading the event's kind uses |
| A view word (`vue`, `v.`, `image`) | The views: `45`, `45/200`, `45 sur 200`, `5d-6g/13`, the sides as the vocabulary writes them |
| A number word (`acte`, `n°`, `matricule`, `ordre`, `n° d'ordre`) | The act, matricule or entry number, unless the word before names another number (`ménage n° 56`) |
| A folio or page word (`f°`, `fol.`, `p.`) | The folio, with its recto or verso: kept for the reader, never a view |
| A call-number word (`cote`) | The call number, up to the next keyword |
| A parish word | The parish |
| A section word (`section`) | With the ordinal before it or the number after it (`3e section`, `1re section`, `section 3`), the parish: the section of a city's registers |
| An approximate-period word (`env.`, `vers`, `ca`, `circa`) | Nothing of its own: the period after it is read as if it were not there (`env. 1792-1952` is 1792 to 1952); with no period after it, the word may be a place's name (`Vers`) |
| A bureau word | A military series' locality, its recruitment bureau |

A segment shaped like a call number (as the normalized form below defines it) that starts
with nothing else known is the call number (`4E 1234`, `GG 45`, `1 Mi 456`).
The first archive named by a catalogued citation code is not overridden by
what follows: once `AD85` has named the archive, a later segment starting with
the abbreviation of another kind of archive — alone or glued to digits, as the
communal deposit `AC262` or `AM12` — is a call number, not a second
designation, and so is no uncatalogued archive. Only a later code of the
same letters (`AD98` after `AD85`) designates another archive, and a catalogued
code always does. `RP` is the census series' code, never a register word:
written as an act it reads the census, while `registres paroissiaux` written
out stays the parish registers.
Years read as periods: `1745-1760`, `1745 – 1760`, `1745 à 1760`, a
Republican year `an XII`, a tilde before them marking them approximate
(`~1850`) set aside; a full date (`12/03/1752`, `1er mars 1752`) is the
act's, used only when nothing else gives a year. A proper name — capitalized
words with the particles between them, no keyword — is a locality: one a
preposition introduces after a document word first, then one written as a
segment of its own; only when the words state none of these, the event's
place; then a name within another segment.
The archive's own areas and name are never its locality, but a municipal
archive serves its commune, which is its locality when nothing else names one.
Of the document kinds the words name, the first the archive holds is taken
(`inhumations` beside `cimetière de Exampleville` names the cemetery's
register where no parish register is catalogued), or else the first.

**Districts and other names.** A city its archive cites by numbered
districts (`citation.districts`, §3.1) is a locality with its district
number, however written: the city and the number in Arabic or Roman
numerals, with or without an ordinal suffix of the vocabularies (`Paris
11e`, `Paris 11ème`, `Paris 11è`, `Paris XIe`, `Paris XI`, `Paris 1er`,
`Paris Ier`), with or without a district word (`Paris 20e arrondissement`),
in parentheses (`Paris (11e)`) or joined by a hyphen (`Paris-11`), or the
number and a district word, then possibly the city (`11e arrondissement`,
`naissances du 11e arrondissement de Paris`). A bare Arabic number after the
city is a district only as a whole segment, so `Paris 12 mars 1917` stays a
date, and a number beyond the city's count is none. Recognition writes the
district alike, in the archive's language — `Paris 11e`, `Paris 1er` —,
which the adapter's locality style turns into the portal's value (§4.3
`district`: `11`); a number without its city is the first listed city's. A
locality written as one of the archive's other names (`citation.localities`)
becomes the name its portal knows it by.
A citation without a year takes the period its call number writes, by the
archive's `call_number_periods` (`1904` for `XXX_RJ19041904_01`,
`1917-1918` for `XXX_RJ19171918_04`, whose year is 1917).

**Vocabularies.** One JSON document per language under `assets/citations/`,
embedded at build time like the catalogue; adding a language is a data
change, and a test reads a second, test-only language. Every field holds
phrases, written naturally and folded when loaded:

| Field | Content |
|---|---|
| `language`, `countries` | The language, and the countries whose archives follow these conventions |
| `civil_registration_from` | The year civil registration began in those countries (1793 in France): a birth or death cited without a register word reads as a baptism or burial before it |
| `archives` | Kinds of archive by level, and `any` |
| `acts`, `registers`, `tables`, `series` | Act kinds by letter, register words (`any`, `parish`, `civil`), tables and series by code |
| `views`, `view_counts`, `sides` | View words, the word before a view count, the side suffixes |
| `numbers`, `other_numbers`, `folios`, `pages`, `recto`, `verso`, `call_numbers`, `parishes`, `sections`, `approximate`, `bureaus` | The keywords above |
| `places`, `particles` | Prepositions introducing a locality, and particles within place names |
| `districts`, `ordinals` | Words naming a city's district (`arrondissement`), and the suffixes of an ordinal number, `first` after 1 (`er`) and `other` after the others (`e`, `ème`); the first of each is written (`Paris 1er`, `Paris 11e`) |
| `months`, `ranges`, `republican_years` | Month names, the words joining a period, the word before a Republican year (French conventions only) |
| `unsupported` | Phrases naming a kind of document no collection holds (`minutes notariales`, `matrice cadastrale`, `hypothèques`, `écrou`): a citation naming one is no link (below) |
| `ignored` | Words that are no locality (`s.d.`, `France`) |

The first phrase of an act, table or series and the first view word are
those OxidGene writes when the reader keeps completed parts (§6.5).

**The place dictionary.** When several localities are offered — a parish and
a commune in the words, a hamlet before its commune in the event's place when
the words state none — the backend and the desktop consult the [place
dictionary](place-dictionary.md): the first candidate the dictionary knows
within one of the archive's areas is the locality, and a name the words gave
before it is the parish (`AD72, Saint-Exemple, Exampleville, BMS 1745`). It
never sets the event's place against a locality the words state. The dictionary
is read once for the names asked, unless a place search already holds its
index; it is never asked about a single candidate. One recognition
decompresses the file at most once for all its lookups — these areas and the
other names below — and drops it with the recognition: nothing is kept. The
web interface, which does not embed it, offers the link from the other
signals, so it is never what decides whether a citation is a link.

**Other names of the locality.** A commune renamed since the citation was
written may be listed by the portal under its current name, or a register
under a former one: `Exampleville-sous-Bois` cited, `Exampleville` listed,
the portal's locality filter finding nothing for the cited name. Three
sources give the other names a portal may use, none of which the citation
spells:

- **The place dictionary.** Once the locality is known, the backend and the
  desktop ask the [place dictionary](place-dictionary.md) for the other
  names of the commune so named within one of the archive's areas
  (`PlaceLookup::other_names`): the names that bore its official code — its
  current name, its former names, a name under the code of a département
  since split —, and for a commune merged into another, that one's current
  name; never another commune merged into the same one, nor a parish or a
  hamlet. They ride with the parts to resolve
  (`CitationParts::alternate_localities`, current names first), so that
  `oxidgene-archives` never depends on the dictionary. The rows of the
  archive's areas alone are read from the file the recognition decompressed,
  unless a place search already holds the index, and nothing is kept. It covers renames since 1943
  (INSEE history); older ones (`-sous-Bois` dropped in 1937) come from the
  names alone. The web interface has none, which only makes those names
  unknown.
- **The cited name shortened.** The cited name without the qualifier closing
  it: from its first `sur`, `sous`, `en`, `lès`, `lez`, `près`, `de`, `du`,
  `des`, `d'`, `la`, `le`, `l'`, `au` or `aux` after its first word, a
  leading article aside, with a word after it — `Exampleville` for
  `Exampleville-sous-Bois`, `La Ville` for `La Ville-en-Plaine`.
- **Names extending one of these.** A name extending the cited one, its
  shortened form or a known name at a word of its own name — a hyphen or a
  space, then a word or a parenthesised qualifier (`Exampleville-en-Plaine`,
  `Exampleville-Billancourt`, `Exampleville (Department, France)`) —, never
  a part of the place after a dash, a comma or a slash (`EXAMPLEVILLE -
  Section A`).

A name the dictionary knows is the cited locality: its rows are selected as
the cited name's. A shortened or extending name may as well be another
commune's (`Exampleville-la-Forêt`, an `Exampleville` elsewhere): its rows
are kept only when none bears the cited or a known name, each agreeing with
the citation and carrying a cited call number or the cited image count
(§4.3, step 5). A portal's list of localities, where an adapter reads one
anyway (Arkothèque's keyed values, Mnesys's `locality_lookup`, THOT,
Pleade, the Gers portal), is matched with them when no label names the
cited locality, the most likely kind winning: a known name, then the
shortened one, then names extending. A portal matching the locality as text
(Ligeo, Arkothèque) whose answer holds no register of the cited locality and
names it nowhere is searched once more, with the first known name the
cited one's text does not hold, else the shortened name, whose text match
finds every name extending it: one more request at most, and none when the
cited name finds its locality.

**What becomes a link.** A citation is recognized once a catalogued archive
with an adapter is designated; one designated by a repository alone must also
read as a register citation: a document kind, a register word, a view, a
number, a folio or a call number in the words, or an event of a kind the
archive's registers record. A document kind
the words or the normalized form name that no collection holds is
`no_adapter`, as is an uncatalogued archive; one only the event gives is
dropped instead. So is a document kind the words name that OxidGene does not
know at all: a phrase of the vocabulary's `unsupported` list, or a generic
register word followed by a preposition and a lowercase word that is no
keyword (`registres d'écrou des condamnés`, `registre d'entrées de
l'hôpital`) — a locality (capitalized), a known act, table or series
(`registre des naissances`), a period, a call number or a view qualifies
nothing unknown, and neither do free fields without a register word
(`Canton est`, `collection communale`). Such a citation is `no_adapter`
whatever else the words or the cited event say (the event's kind never
stands in for it); only the reader's kind or the normalized form's overrides
it. Nothing naming an archive is `not_an_archive_citation`.
The result may still miss the act or the locality (`Recognition::missing`),
provided it names no document kind at all: the link is offered all the same,
and opens the "Find in the archives"
dialog (§6.5); asked without the reader's parts, the backend answers the
archive's filtered search page, built without a request
(`ArchiveTarget::Results` with no match count), or its website without a
document kind. A series needs no locality.

#### The normalized form

The normalized form is read first, strictly, from the source title followed
by the citation's page as further fields (`cited_text`):

```text
<code> - <locality> - <parish> - <act> - <period> - <free…> - vue <n>[d|g]/<count>
```

The parish field may be left out when the act code is followed by its
period: `<code> - <locality> - <act> - <period> - <free…>`, such as
`AD75 - Paris 11e - N - 1917 - 11N 999 - acte 1894 - vue 11d/31`.

A series of records other than acts (§3.1) is named in words where the
parish and the act would stand, and its fields vary more, as genealogists
write them:

```text
<code> - [<locality>] - [<period>] - <series in words> - [<period>] - <free…> - [vue] <n>[d|g]/<count>
```

such as `AD99 - Exampleville - Recensement - 1866 - Canton est - 7 M 999 - vue 204d/242`,
`AD99 - Exampleville - Registres matricules - 1898 - 1 R 9999 - 348 - 579/833` or
`AD99 - Registres matricules des classes 1859 à 1940 - 1871 - 1 R 9999 - vue 181/196`,
or a cemetery's daily burial register, by its cemetery, the call number
writing its period and the burial's entry number (`ordre`):
`AD75 - Exampleville - C - XXX_RJ19041904_01 - ordre 945 - vue 18/31`, where
`C` names the series only because the Paris archives' `citation.series` says
so, or `AD75 - Cimetière parisien de Exampleville - 1904 - XXX_RJ19041904_01 - n° d'ordre 945 - vue 18/31`.

`CitationParts::parse` returns `CitationParts` with every field optional
except the code, the locality and the act; the locality is empty only for a
series cited without one. A title with neither an act code nor a series is
not a citation, and a title with an act code is read as an act whatever its
other fields say. `ArchiveRegistry::parse` reads a title with the grammar
overrides of the archive its code names (§3.1).

| Part | Read from |
|---|---|
| `code` | First field: capitals and digits, matched against `citation_codes`. |
| `locality` | Fields up to the parish field, or up to the act when the parish is left out; may itself contain ` - `. For a series, the fields before the series, without a period field or a `no_parish` value that ends them; possibly none, when the series field may name its place after a preposition, its name capitalized (`Cimetière parisien de Exampleville`, `Cimetière d'Exampleville`). A military series' locality is its recruitment bureau; a cemetery register's, its cemetery. |
| `parish` | The field before the act, unless it is a `no_parish` value (`(aucun)`) or the act is the third field. A series has none. |
| `act` | The document kind. An act code: `N`, `B`, `M`, `D`, `S`, `P` (publications of banns, filed and searched with the marriages), and their combinations such as `BMS`, `NMD` or `NPMD`, each letter once; `T` followed by one to four capitals is a table code (`TB`, `TM`, `TS`, `TN`, `TD`), kept as written; a series code `RP`, `RM`, `CM`, `TSA` or `RI` (§3.1), which `TSA` is rather than a table code. It is searched from the fourth field on, and an act code followed by a period wins over an earlier one that is not; failing a dated one there, a third field that is an act code followed by a period is the act of a citation without its parish. Without an act code, the first field after the code that names a series: by its code, or in words — folded (case, accents and punctuation ignored), the field contains every word of one of the series' phrases, in any order, a word also matching its plural in `s` or `x`. The built-in French phrases are `RP`, `recensement`, `liste nominative`, `dénombrement` (`RP`); `registre matricule`, `matricule militaire` (`RM`); `conscrit`, `conscription`, `contingent`, `tirage sort`, `garde nationale mobile` (`CM`); `table succession`, `succession absence` (`TSA`); `registre journalier`, `registre inhumation`, `cimetière` (`RI`); a field naming two series is the first one's in the order `TSA`, `RM`, `CM`, `RP`, `RI`. An archive's `citation.series` adds phrases. |
| `year` | The first year of the period field: `1877`, `1702-1703`, a full date (`05/03/1871-28/03/1871`, `26 juillet 1849-1er février 1850`), or a Republican year `an XII` (Roman or Arabic numerals, an I to an XIV, as in `an XI-an XII` or `an XI-XII`) converted to the Gregorian year of its 1 Vendémiaire; a note in parentheses after the period is left aside (`1931 (A-H, collection communale)`), and so is a word marking it approximate before it (`env 1792-1952`, `env.`, `environ`, `vers`, `circa`, `ca`, `c.`, `~1850`, the French vocabulary's `approximate` words), the period then being its years alone. For a series, the period field is the one after the series, or else the one before it, or else a period in parentheses within a free field (`Bureau de Exampleville n° 1 à 1586 (1870)`); years within the series' own name (`des classes 1859 à 1940`) are not its period. A military series' year is its class. Without any of these, the period the call number writes by the archive's `call_number_periods` (§3.1), its first year the year. |
| `period` | The period field as written, kept to match portals that list registers by period text, its days and months included. A field that reads as no period leaves both empty and stays a free field. |
| `call_number` | The free fields shaped like a call number — letters, digits and ` /._-`, at least one digit and one capital, no lowercase word of three letters or more (`1 Mi 456` is one, `acte 26` is not), except a deposit of capitals of one to three letters, one lowercase word of four letters or more and digits (`E dépôt 12`) —, several of them (a register's microfilm's and its original's, `5MI999BIS - 4E 9927`) kept together, joined by the field separator, each compared on its own. A call number is compared without spaces or case, or by its runs of letters and of numbers, numbers without leading zeros: `3E73/14` matches `3 E 73 / 14`, `4E212/45` matches `4 E 212 45`, and `9 R 1` matches `9R0001`. One ending with a range of numbers (`5 Mi 9_374-376`, the microfilms of several years) matches one it holds or that holds it. A portal's text joining several call numbers (`4E 9927 / 5Mi 999 BIS [9999999/2]`: the original's, the microfilm's, an internal reference) matches each of them; a slash before a bare number (`9 E 250 / 1`) belongs to the call number. A cited call number joining two with a slash, each with a letter and a digit — a digitization's and the original register's, `9NUM/8E99` — also matches a portal's text that is either alone (`9 NUM`, `8 E 99`). |
| `number` | The first free field that is an act, matricule or entry number: `acte 31`, `matricule 1268`, `n° 12`, `ordre 945`, `n° d'ordre 945`, or a bare number of up to seven digits (`348`). Selection compares it with the numbers a register spans (§4.3, step 3); it is sent to a portal only by an index of persons searched by matricule, to find the person's row (§4.5). |
| `views`, `view_count` | The last field, introduced by a `view_words` word: `vue <n>[d|g]/<count>`, a range `vue <n>[d|g]-<m>[d|g]/<count>` of at most ten views for an act spanning several, such as `vue 5d-6g/13`, or a view without its count; or written bare, `579/833`, right after a number field. Each view keeps its side (`d` right, `g` left); in a range the sides apply to its ends. A malformed range, or a view beyond the cited count, leaves both empty. |

A series cited without a locality, such as a department's military
registers, is searched with the locality filter empty, and selection keeps
every row whatever its locality (§4.3, step 3).

The document kind also gives the kind of record attaching a view proposes
(§6.4), `CitationParts::category`.

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
        /// Present when the register counts another number of images than
        /// the citation (§7).
        renumbering: Option<Renumbering>,
    },
    /// Several or no registers matched: the portal's filtered results.
    Results { url: Url, matches: Option<usize> },
}

pub struct Renumbering {
    /// The register's image count as cited.
    pub cited_count: u16,
    /// How far the views opened are from the cited numbers, `0` when kept.
    pub shifted_by: u16,
}

pub struct ArchiveView {
    /// One-based view number in the register: as cited, or as estimated
    /// when the register's numbering differs.
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

Addresses are `String`s, and the target serializes with a `kind` of `view` or
`results`. `views` holds every cited view, in order, so an act spanning views
5 and 6 resolves to both. `picture` is the address an attached page holds — a size bounded to
the screen where the service allows it, the full image otherwise — and
`thumbnail` the smallest address the archive serves. `attribution` is the
catalogue template filled with the call number and views. `renumbering` is
left out of the serialized target when absent.

The target carries no `terms` and no `display`: both clients embed the
catalogue and read them from the entry of the citation's archive. So it is with a portal's reuse licence: a target may stand behind one,
which the adapter declares (§4.12), and both clients ask the embedded
catalogue (`ArchiveRegistry::licence`) whether one stands before the
target's address, to open its entry first (§6.1, §6.2).

`Results` built by `results_url` without any request has no match count; it
is what a client gets when no transport can reach the portal.

`ResolveError` distinguishes an archive without an adapter (`no_adapter`: not
catalogued, or no collection holds the act), a portal that did not answer as
expected (`unexpected_response`, with a description of what differed and no
response content), an anti-bot challenge answering in place of the page
(`challenged`, which an adapter reports apart from a changed shape because the
portal did not change), a timeout (`timeout`, also a gateway's `504`: the
portal behind it did not answer in time), and a portal that could not be
reached or answered with another server error (`unreachable`). Each code is stable
and the interface translates it as `archive_viewer.<code>`.

The `Resolver` tries the candidate collections in order (§3.1) and returns
the first `View`. When none finds one, it returns the first `Results` of a
search that ran; failing that, the first error; failing that, the offline
`Results` of a collection whose `transport` is `browser` and that the
transport in use cannot reach.

### 5.3 API

The backend resolves archives whose transport is `any` over the `native`
transport; for a `browser` archive it returns the offline `Results` target
without any request:

- REST: `POST /api/v1/trees/{tree_id}/sources/{source_id}/archive-target`
  with a JSON body naming an optional `citation_id`, returning the
  `ArchiveTarget`.
- GraphQL: `Source.archiveTarget(citationId: ID)`, the same target as one
  object whose `kind` is `VIEW` or `RESULTS`.

The body may name the parts a reader completed in the "Find in the archives"
dialog (§6.5), `parts`: `{"locality": …, "act": …, "year": …, "view": …}`,
every field optional, `act` a document kind's code (`N`, `BMS`, `TD`, `RP`);
in GraphQL `archiveTarget(citationId, parts: {locality, act, year, view})`.
They win over every other signal (§5.1). An unknown code, a document kind
none of the recognized archive's collections holds, a blank locality or one
over 200 characters, a year outside 1000–2100 or a view below 1 is a
`400 validation_error`.

The body may also name a `view` (`archiveTarget(citationId, view)` in
GraphQL): the one view of the cited register to resolve instead of the cited
views, which the document form attaching cited views asks for when the
reader adds the previous or the next view of the register (§6.4). It keeps
the side the citation gives that view, if it cites it; a view below 1 or beyond the cited view count is a
`400 validation_error`. A view beyond the register's own images resolves
as any cited view does (§7).

Both surfaces share one service, validation and error mapping, and are tested
symmetrically ([API Contract](api.md#sources)). The request sends the portal
only the locality, period, act and call number; never the person's name, the
citation text, or the act or matricule number.

**What is read.** The backend recognizes the citation from the stored
records (§5.1): the source, the named citation's page and text, the
repositories holding the source with their call numbers and websites, the
addresses of the media linked to the source, and, when the citation is
attached to an event, the event with its place. Without a `citation_id`, the
source and its repositories alone are read. It consults the place dictionary
when several localities are offered, on the blocking pool. A portal address
of the archive found in the records is answered as a `View` with that
address and no views, without a request; a citation still missing its act or
its locality, with no reader's parts, as the archive's filtered search page
built without a request, or its website without a document kind. The
interface reads a citation the same way from the person's page bundle, which
carries each cited source's repositories and media addresses
([API Contract](api.md#sources)), when deciding to offer the link.

**Process-wide resolver.** The backend keeps one `Resolver` for the process,
shared by both surfaces, so its session cache (§8) answers a citation
already resolved without a request; the native client is built on the first
resolution, so a process that never resolves holds none. The transport is
injected into the application state, which lets the API tests serve the
recorded fixtures (§9) instead of a portal.

**Errors.** A source or citation that is absent, deleted or of another tree,
or a citation of another source, is `not_found`. A text that is no
archive citation — nothing in the records names a register of an archive
(§5.1) — is `422 not_an_archive_citation`; a citation of an archive the
catalogue does not list, or of an act none of its collections holds, is
`422 no_adapter`. A resolution failure keeps its `ResolveError`
code: `502 unexpected_response`, `502 unreachable`, `504 timeout`, and the
same codes in upper case in GraphQL's `extensions`. They are the portal's
failures, not the server's: no correlation ID, and the backend logs the
code and the archive's identifier only, never the citation. GraphQL answers
the field for a source read on its own, never for the items of a list
(`VALIDATION_ERROR`), so that no query resolves sources in bulk (§8).

## 6. Display

### 6.1 Desktop

Every archive opens on its portal, alike: the archive window is a top-level
WebView, because portals forbid framing (`frame-ancestors 'self'` on the
Loire-Atlantique portal), beside the application's window, which stays where
the reader was. A click on a cited source starts the resolution on the desktop's Dioxus runtime, with one
`Resolver` shared by every window, whose cache (§8) spares a second request
for a citation already opened in the session. The desktop resolves every
archive with the `window` transport (§4.2): for each collection it tries, the
window loads the endpoint's start page — the lightest page of the portal's
origin that passes its checks, Arkothèque's `/robots.txt` — and the
adapter's requests run in it. The window then loads the target. Loading the portal before the target
matters on a portal with a challenge: on a cold session the Sarthe
challenge's redirect drops the address's fragment, which carries the view,
while the challenge cookie the first load leaves keeps it.

The window fills no field and clicks no control but a cookie banner's
refusal, a listed information notice's acknowledgement and the
page-number control of a viewer without an address per view (all below);
where the portal has an address per view, the page's own scripts open the
viewer at the view. While it resolves, the window covers the portal's
pages with OxidGene's progress overlay: an opaque full-window cover hiding
the portal's page, in the application's theme with its one spinner
([UI Common §4.17](ui-common.md#417-spinner)), out of reach of the portal's
styles (a shadow root), showing the archive's name, the citation and the
step, as a polite status:
connecting to the portal while the start page loads
(`archive_viewer.step_connecting`), searching for the register during each
of the adapter's requests (`archive_viewer.step_searching`), and opening the
cited view, or the archive's page, while the landing loads
(`archive_viewer.step_opening_view`, `archive_viewer.step_opening`). From 3
seconds on, the step's elapsed time shows beside it
(`archive_viewer.step_elapsed`), updated every second. The page script says
when each document starts, so that the overlay covers it at once. The
overlay gives way whenever the reader has to act on the page — an anti-bot
check they were asked to answer, until the portal's page shows; a cookie
banner left to them, until it is gone — and is gone once the landing's page
has shown: the register at the cited view — over a viewer the window brings
to that view, once it is there or the window gave up —, or, when the lookup
failed, the portal's page the failure lands on, with its banner. Its Cancel button (`common.cancel`) stops the resolution — its
pending requests are dropped — and the window loads the collection's
filtered search page (`results_url`, the archive's `website` when there is
none), without a banner. Once the target has loaded, a banner over the
portal page, in the interface language (`archive_viewer.*`), says that no
register or several registers match the citation, over the filtered results;
or, over a register whose portal has no address per view — a `View` with no
views although the cited views lie within the register, or its size is
unknown —, which view to go to (`archive_viewer.go_to_view`, with the first
cited view), when the window could not bring the viewer there itself
(below); or, over a register counting another number of images than the
citation (§7), that the numbering changed, with both counts and the view
opened (`archive_viewer.renumbered`). A load's banner shows over every page that follows it — a
check's redirect, a portal page navigating on — until the reader closes it
or the window loads another page. A server's error page in place of the
portal's — a gateway's `504 Gateway Time-out` where the portal took too
long, a `503` — shows, once no progress overlay covers it, why the portal
failed in place of the load's banner: that it did not answer in time
(`archive_viewer.page_timeout`, for a `504`) or answered with a server error
(`archive_viewer.page_error`), with a button loading the page again
(`archive_viewer.reload`). The landing of a lookup that timed out is the
same search the portal could not answer, which may well time out again. When the resolution fails, the window
lands with the banner of the failure's code (`archive_viewer.<code>`): on
the collection's filtered search page (`results_url`, the offline `Results`
of §5.2) for an anti-bot check or block (`challenged`), where the reader may
pass the check, and for a portal that did not answer in time (`timeout`) or
could not be reached (`unreachable`), where the page may still load for the
reader; on the archive's `website` for any other failure, or when the
collection has no search page. A resolution still running after 8 minutes —
every wait within it is bounded already, this bounds their sum, leaving room
for a reader answering a check twice — lands as a `timeout`. The failure is
logged with its code and the archive's identifier only. A reader who closes
the window during the resolution stops it.

The banner never competes with what the portal asks of the reader: over a
portal page, it stays hidden while one of the portal's dialogs is on screen
— a modal dialog (`dialog[open]`, `[aria-modal="true"]`: the Val-d'Oise
Monocle viewer's reuse licence), a cookie banner of the managers the window
recognizes (below), or the reuse licence of the viewer the target opens in
(`licence` in `viewers.json`: Arkothèque's « licence clic ») —, looked at
every half second, and shows once the reader has answered; on a licence
page the reader is asked to accept (below), only dialogs hold the request.
The request to answer an anti-bot check and a server's error page's reason
show at once. The banner lives in a shadow root whose host's styles are
reset (`all: initial`), so that no style sheet of the portal changes it,
and a banner without text is never shown. A banner with a button
(attaching, reloading, opening in the browser) is compact.

**Reuse licences.** A target behind a portal's reuse licence (the
Côtes-d'Armor "salle virtuelle", §4.12) is never opened directly: OxidGene
never accepts a licence for the reader. The window lands on the licence's
entry, with a banner asking the reader to accept the licence on that page
(`archive_viewer.licence`), and watches the pages that follow — each
page's address, as its IPC message carries it: once a page behind the
licence shows after the licence page did — the reader accepted it, and the
portal moved on to its search page —, the window loads the target itself,
the cited view or the list of lots, with the target's own banner (the view
to go to where the view has no address). A page shown before the licence
page, the page being left included, never counts; nothing on the licence
page is clicked or filled. The live check's step 4 passes the licence the
same way before opening the target (§9.1).

What a window's page posts — what the page is, the answer of a request, the
reader's click on a banner button — wakes the application's event loop at
once, so that the banner and the resolution never wait for the reader to
move the mouse over a window.

**Viewers without an address per view.** A portal whose viewer opens every
register on its first view (GAIA, §4.8; THOT's `views: "register"`, §4.9;
CAOMEC2, §4.15) is driven the way a reader would drive it: once the landing's page is the
portal's, a script of the window (`go_to.js`) types the first cited view in
the viewer's own page-number control and submits it, under the progress
overlay, which says that the view opens. How is data, per platform, in
`oxidgene-archives`' `platform/viewers.json` — the file the live checks
read the viewers with (§9.1) —, a viewer's selectors being lists where a
platform's portals run different viewers: the element showing the view
number (`view`) and the count (`view_count`), and `go_to`, the control to
type in (`input`, `view` by default) and how the number is submitted
(`submit`): a `change` event, that event followed by the Enter key, a
`blur` event — a control applying the number once left —, or a click of a
`button`. The collection is the candidate whose portal serves
the target's origin. The script gives the viewer 30 seconds to show its view
number and count, writes the number as a reader types it (its value, an
`input` event) and submits it, dispatching the events rather than focusing
the control — a window in the background focuses nothing —, and gives the
viewer 5 seconds to show the view, three times at most; the view counts as shown
when the `view` element shows it and the count is unchanged — where one
element shows both (`Page 178 de 396`), both numbers must show, so that the
number typed is not taken for the viewer's answer. It never types a view
beyond the count the viewer shows. When the viewer shows the view, nothing
more is said; otherwise — or when the page is replaced twice while the
script runs — the banner names the view to go to, as above. The script
requests nothing itself; the viewer loads the view's image as for a
reader's click, and a viewer opening that writes on the server (THOT) is
not repeated. A viewer whose platform has no `go_to` keeps the banner.
The outcome is logged at the debug level, without the view.

**Anti-bot checks.** A script of the window classifies each main-frame page
once its markup is parsed (`DOMContentLoaded`) — not once its images and
scripts have loaded, which a portal's own heavy requests may delay —, with
the signatures of §4.2: a **challenge** or a
**block** when its markup bears one, **interactive** when a challenge also
shows a widget, a server's **error** page when the navigation's timing gives
a `5xx` status (on the WebViews that give it) or else when its title or
first heading starts with a `5xx` status and its reason phrase (`504 Gateway
Time-out`, `503 Service Unavailable`, `Error 503 Backend fetch failed`; a
portal's heading `500 ans d'archives` is none), and otherwise the **portal**
once it renders something —
text, or a frameset with a frame, whose body has no text of its own (THOT).
A page that shows nothing yet, or a challenge, is looked at again every half
second, since a widget or a portal's content may come later. Waiting for
the start page:

- the portal's page lets the requests run;
- a challenge is given 5 seconds to clear itself, as Anubis's proof of work,
  F5's script or a bot-mitigation redirect do, with the progress overlay
  over it; one still on screen after that, or one showing a widget, is the
  reader's: the banner asks them to answer the check in this window
  (`archive_viewer.challenge`), on every page of the check until the
  portal's page shows, and the reader has 3 minutes, after which the
  resolution fails `challenged`;
- a block fails `challenged` at once;
- a server's error page fails at once with its status: `timeout` for a
  gateway's `504`, `unreachable` for another;
- a page that shows nothing for 30 seconds fails `timeout`.

When an anti-bot check answers one of the adapter's requests — an answer
bearing a challenge's signature, whatever its status — the window loads the
start page again, where the reader faces the check as above, and once the
portal's page shows, sends that same request once more: once per search of a
collection (§8). A block answering a request fails `challenged`. The window
never answers a check itself: it waits, and the reader answers.

**Cookie consent.** OxidGene refuses a portal's cookie consent on the
reader's behalf and never accepts it. A script of the window, run in each
main-frame document, recognizes the consent managers of the shared list
beside it (`consent.json`: the Arkothèque banner, tarteaucitron, Axeptio,
Didomi, OneTrust, CookieConsent and Osano, Klaro, Complianz, Orejime — each
with its banner, refuse and accept controls), watches the document for 15 seconds,
and clicks the first visible refuse control of a banner on screen, once: the
manager's own, or else, within the banner, a control whose whole text is a
refuse phrase. Nothing whose text, title, label or value is an accept phrase
is ever clicked. A banner offering only an accept control, still there after
the refusal, or still on screen after the 15 seconds, is left to the reader,
and the progress overlay gives way to it.
An information notice that asks no consent is acknowledged instead: the
same list names such notices with their banner and the one control that
dismisses them (`notices`: the Mnesys Expo portals' note that they set only
technical cookies exempt from consent, `#rgpd-infos`, dismissed by its
"J'ai compris"). Acknowledging it grants nothing, since the portal asks for
no consent and sets the same cookies either way, so the window clicks that
control once while it watches the document; a notice is listed only after
its text has been read, never recognized by its wording, and one that
offers a control whose whole text is a refuse phrase is a consent request
and is not clicked. The window's persistent profile keeps the choice. The
live checks' browser runs the same script.

**Certificates on Linux.** WebKitGTK does not fetch an intermediate
certificate a server omits, where Chromium, Firefox and the macOS and
Windows WebViews do, so a portal serving its certificate without its issuer
opens everywhere but in the Linux window (Savoie's image host did). On the
load's TLS failure, when the page is on an origin the window was sent to
(the collection's or the target's), over `https`, and an unknown issuer is
the only error, the window fetches the one issuer the certificate names in
its Authority Information Access extension (`caIssuers`, DER or PEM, 10
seconds, 64 KiB, no retry), verifies the completed chain — that certificate,
that issuer, a root the system trusts (`rustls-native-certs`) — for the
page's host name and the current time with rustls's verifier, and only then
allows that exact certificate for that host in the archive windows' web
context, for the application session, and loads the page again.
Verification is never disabled: any other failure, or a chain that does
not verify, stays refused, the window says that the portal's certificate
could not be verified (`archive_viewer.certificate`) with a button opening
the page in the system browser (`archive_viewer.open_in_browser`), and a
resolution waiting on that page fails `unreachable`.

**Attaching from the window.** Disabled for now (§6.3): the banner never
offers it. With the offer on, over the views of an archive whose images
OxidGene may use (`display: "iiif"`, §6.3) — a `View` target whose every
view carries its image —, the window's banner, once the portal's dialogs
are answered (above), says that OxidGene can keep the cited view (`archive_viewer.attach_hint`, or the landing's own message)
with an **Attach as a document** button (`archive_viewer.attach`). Its
click sends the target the window shows, over the window's IPC channel, to
the page of the application that opened it, which brings its window forward
and opens the document form prefilled (§6.4); nothing is written before the
reader saves it there. A page of the application closed meanwhile receives
nothing.

The window uses a persistent web profile of its own, separate from the
application's, under the state directory (`archives-webview/`,
[Architecture §8.3](architecture.md#83-local-files)). Archive portals hold no
credential, and keeping their cookies spares the reader a reuse licence and an
anti-bot challenge at every opening. Every page the window loads itself — the
start page, the landing — is asked of the server with `Cache-Control:
no-cache`, never taken from WebKit's HTTP cache: a portal behind a bot
mitigation (Sarthe) marks its pages cacheable for a day while the cookie
letting a page's own requests through lasts for the session, which the profile
does not keep across restarts, so a page taken from the cache got no cookie
and its own requests met the mitigation's check, its loading indicators
turning for good. A page still fresh is answered `304 Not Modified`.

Each step of a window's lookup is logged at the debug level — the start page
and every request, with its outcome and duration, never an address or the
citation — and the resolution's end with its duration, so that a failure can
be told from a terminal.

### 6.2 Web

Every archive opens on its portal here too: the web client resolves through
the backend endpoint (§5.3), naming the citation the reader clicked, and opens the target in a new browser tab that
can neither reach the application (`noopener`) nor tell the portal where the
reader came from (`noreferrer`). For a `browser` archive the tab opens on
the filtered search results, from which the reader opens the register. The
source becomes a link on both clients; only the window type and the
precision differ.

A browser opens a tab only during the reader's click, and the address
arrives after a request that may wait on the portal for seconds, beyond the
few seconds a browser grants. So the click opens a blank tab at once, showing
`archive_viewer.searching`, cuts it from the application, and sends it to
the address when it arrives, through a link of the tab's own document marked
`noopener noreferrer`. When the browser refuses even that tab, the address
is opened in a new tab once known, which a strict popup blocker may refuse
too. A reader who closes the blank tab meanwhile is not sent another.

The tab cannot carry a banner over the portal's page, so what the window's
banner says (§6.1) appears in the application, beside the source: no
register, or several, over the filtered results; and on a failure, by its
code (`archive_viewer.<code>`, `archive_viewer.failed` for any other), while
the tab opens the same landing as the window (§6.1): the collection's
filtered search page for `challenged`, where the reader passes the check in
their own browser, `timeout` and `unreachable`, and the archive's `website`
otherwise. The view to go to on a portal without an address per
view is said the same way. A target behind a reuse licence opens the
licence's entry in the tab, which cannot go on past it for the reader: the
notice beside the source says that the licence comes first and the register
is to be looked up on the portal (`archive_viewer.licence_tab`), or gives
the target's own notice, the view to go to.

### 6.3 IIIF behind the scenes

OxidGene shows every archive's images in the archive's own viewer, on its
portal (§6.1, §6.2). For a `display: "iiif"` archive it also knows each
cited view's image — its `picture` and `thumbnail` addresses and its pixel
size, read over IIIF by the adapter (§4) — and uses it to attach the views
as a document (§6.4), with the attribution the archive's terms require.

**The offer is disabled.** Attaching is implemented but offered nowhere:
viewing the source an event cites is not where a reader collects documents —
the act is already cited, and the portal already shows it. The offer is meant
for a future free browsing and search of the archives within OxidGene
([Roadmap](roadmap.md#3b-active-archive-viewer)), where a reader who finds a
document may want to attach it to a person of the tree. One switch holds it
off, `ATTACH_OFFERED` in `oxidgene-ui`'s `archive_viewer`, which every entry
point reads through `offers_attach`: the web's button beside the source, the
interface's request giving the desktop's archive window somewhere to send its
target, and that window's banner, which checks the switch again before
offering anything. Turning it on restores all three. Everything else stays
built and tested: `display: "iiif"` in the catalogue and its live checks,
the adapters' IIIF reads, the backend endpoint on both surfaces with its
`view` (§5.3), the document form's register paging (§6.4), and the window's
banner action; the interface and desktop tests exercise the enabled path
explicitly, and check that nothing is offered with the switch off (§9).

With the offer on, nothing is displayed in OxidGene before the reader
attaches: the desktop offers to attach from the archive window's banner
(§6.1), the web from an **Attach as a document** button beside the cited
source, whose click asks
the backend for the target (§5.3) — one lookup, answered from the session
cache when the source was just opened — and, when its views carry their
images, opens the document form. Any other answer — several or no
registers, a view without an image, a failure — is said beside the source
as the landing's notice (§6.2), or `archive_viewer.no_image`.

### 6.4 Attaching and cropping

With the offer on (§6.3), **Attach as a document** (§6.1) opens the canonical `DocumentForm`
([UI Common](ui-common.md#adding-a-document)) prefilled, and nothing is
written before the reader saves it:

- one remote page per cited view, in order, holding the view's `picture`
  address, its pixel size, and its thumbnail address
  ([Data Model](data-model.md#media), `thumbnail_url`); the reader removes
  views, or adds the previous or next view of the register, each resolved on
  its click — one request naming that `view` (§5.3) —, so a two-page act
  becomes one two-page document
  and a single page one single-page document;
- the title from the call number (the archive's name without one) and views,
  and the description from the attribution of the views; both follow the
  views the reader adds or removes until the reader writes them;
- the kind of record from the document kind (`CitationParts::category`) and
  the medium `manuscript`: `parish_record` for baptisms and burials,
  `civil_record` for births and deaths, and for marriages and banns alone
  `parish_record` before 1793 and `civil_record` from then (none without a
  year); a table is of the acts its code names after the `T` (`TB` a parish
  record, `TD` a civil record), and a civil record when they are no act
  letters (`TA`); a census (`RP`) is `census`, a military register (`RM`)
  or conscription list (`CM`) `military_archive`, and succession tables
  (`TSA`) `notarial_archive` — the data model has no kind for registration
  records, and estates are the nearest, beside deeds, wills and inventories
  ([Data Model](data-model.md#media)) —, and a cemetery's burial register
  (`RI`) `civil_record`, the municipal record of a death nearest to it;
- the event the citation documents, which the document is attached to, and
  the cited source as a link of the document, so the archive address can be
  resolved again from the citation if the portal moves its images.

The saved document is an ordinary media record: it appears among the
documents of its event, on the profiles and couple pages that show the
event, and every viewer action applies to it. Its tiles draw each page's
thumbnail address, never the full view.

Keeping only the act out of a double page or a crowded view is done with the
existing region tool of the gallery ([Data Model](data-model.md#vignette)): a
region drawn on a remote page is stored as coordinates over the page, in the
pixel size the archive stated, and cut by the client, so it needs none of the
bytes; a region can serve as a portrait or be attributed to a person like any
other.

### 6.5 Find in the archives

A citation whose archive is recognized but whose act or locality is not
(§5.1) is a link all the same, provided it names no document kind: one the
catalogue's collections do not hold, or one OxidGene does not know (a prison
register, notarial minutes), is no link at all rather than a dialog that
could never find it. Its click opens a small dialog, the shared
modal of [UI Common](ui-common.md), titled **Find in the archives**
(`archive_viewer.find_title`) and prefilled with what was recognized: the
archive, shown and fixed; the locality; the kind of register, chosen among
the document kinds the archive's collections hold, named in the interface
language (`archive_viewer.kind.<code>`); the year; and the view. **Search**
completes the citation with the reader's parts, which win over every other
signal, and opens the register exactly as a complete citation's click does —
in the archive window or a new tab — the reader's parts
riding with every resolution of it, the neighbouring views included
(`parts`, §5.3). A kind or locality still missing keeps the dialog open with
`archive_viewer.find_missing`.

Nothing is saved while the reader only searches. A checkbox, unticked by
default and offered for a stored citation, **Add these details to the
citation** (`archive_viewer.find_keep`), writes the parts the reader
supplied at the end of the citation's page, in the archive's language with
the first words of its vocabulary — `Exampleville, naissance 1872, vue 45` —
through the ordinary citation update, which records its history; the
citation then opens directly, read back by the words (§5.1), and the page
reloads. The source, shared by other citations, is never changed.

## 7. Errors and fallbacks

| Situation | Result |
|---|---|
| Records naming no archive, an uncatalogued archive, or a document kind no collection holds | Plain text, no link; the endpoint answers `not_an_archive_citation` or `no_adapter` (§5.3). |
| Archive recognized, act or locality missing | A link opening the "Find in the archives" dialog (§6.5); the endpoint, without the reader's parts, answers the filtered search page built without a request, or the archive's website without a document kind. |
| Portal address of a catalogued archive in the records | The address, opened as it is: in the archive window (desktop) or a new tab (web), without a lookup. |
| No register, or several | `Results`: the filtered search page, with a banner (desktop) or a notice beside the source (web). |
| Commune listed by the portal under another name than the cited one (renamed, merged) | The register under that name, chosen as §5.1 (*Other names of the locality*) says: a name the place dictionary knows as the cited locality's, a shortened or extending name only with a cited call number or image count. A portal matching names as text is searched once more for that name; when nothing decides, `Results` on that search. |
| View beyond the register's image count | `Results` with no match, the collection's filtered search page with the banner of no register: the register chosen is taken for another one than the cited (a citation of an older digitisation, its call number and view count those of a microfilm since replaced by the originals, split otherwise), which opening it on its first image would hide. A register carrying the cited call number — a person's row of an index of matricules, one view of the register the citation counts — is the cited one: `View` with no views, its first image. |
| Register counting another number of images than the citation (digitised or bound again since: a register cited for 1832–1851 with 184 images, now bound with the years from 1818 in 301) | Its views, with `renumbering` and the banner `archive_viewer.renumbered` (desktop) or the same notice beside the source (web): the numbering changed, from the cited count to the register's, and the view opened, which may not be the cited page. Where the register's period ends with the cited one but starts earlier and it counts more images, the earlier years come first: each cited view is moved by the difference of the counts (view 38 of 184 opens 155 of 301). Otherwise — the cited period the register's start, the whole of it, or unknown, or the register counting fewer images — the cited numbers are kept. A register opened on its first image (no address per view, a view beyond it) has no `renumbering`. |
| `iiif` archive resolves to anything but views with images | The archive opens on the landing like any other. With the offer to attach on (§6.3), the window offers nothing to attach, and the web's **Attach as a document** says the landing's message, or `archive_viewer.no_image`, beside the source. |
| Target behind a portal's reuse licence | The licence's entry, with `archive_viewer.licence` (desktop): once the reader has accepted it, the window opens the target; the tab opens the entry with `archive_viewer.licence_tab` (§6.1, §6.2). |
| View cited within a register whose portal has no address per view | `View` with no views: the register's first image, with `archive_viewer.go_to_view` naming the cited view, as a banner (desktop) or a notice (web). |
| Anti-bot check on the start page (desktop) | Waited out for 5 seconds, then the reader's to answer in the window within 3 minutes (§6.1). |
| Anti-bot check answering a request (desktop) | The start page again for the reader, then the same request once more (§6.1, §8). |
| Anti-bot block, a check left unanswered, or a challenge answering the backend | `challenged`: its message, as a banner (desktop) or a notice (web); the window or the tab opens the collection's filtered search page (`results_url`), the archive's `website` when there is none. |
| Portal certificate served without its issuer (Linux desktop) | Completed from its `caIssuers` address and verified against the system's roots; otherwise `archive_viewer.certificate` with a button opening the page in the system browser (§6.1). |
| Portal timed out or could not be reached; a desktop lookup running past 8 minutes | `timeout` or `unreachable`: its message, as a banner (desktop) or a notice beside the source (web); the window or the tab opens the collection's filtered search page (`results_url`), the archive's `website` when there is none. |
| A server's error page in the window (desktop): the start page or the landing answered by a gateway's `504` or another `5xx` | On the start page, `timeout` (`504`) or `unreachable` at once; on the landing, a banner saying the portal did not answer in time or failed, with a button loading the page again (§6.1). |
| Portal changed shape | The failure's message, as a banner (desktop) or a notice beside the source (web); the window or the tab opens the archive's `website`. |

## 8. Access etiquette

Archive portals are public services whose terms OxidGene follows:

- One resolution per user click: no crawling, bulk resolution, background
  refresh or prefetching. Some portals, Loire-Atlantique and the Ligeo ones included, publish a
  `robots.txt` that disallows automated agents, and some protect themselves
  with an anti-bot challenge; OxidGene acts only on a user's explicit request,
  as a browser does, and never works around a challenge outside the window
  the reader sees. The window answers no check itself: it waits for a check
  that clears itself, and asks the reader to answer one that does not
  (§6.1).
- The window's only clicks on a portal page are a cookie banner's refusal,
  on the reader's behalf, and the acknowledgement of a listed information
  notice that asks no consent (§6.1): it never accepts a consent, never
  answers a check, and fills nothing.
- Requests of the `native` transport identify OxidGene in their
  `User-Agent`, `OxidGene/<version> (+https://github.com/trois-six/oxidgene)`;
  the `window` transport's are the portal page's own. Both time out
  after 30 seconds — a reader waits for the one lookup they asked for, and
  some engines take 15 to 25 seconds to search their most populated
  locality —, read at most 8 MiB, and are never retried
  automatically. The one exception is reader-driven: a request of the
  window that an anti-bot check answers is sent once more after the
  window has shown the start page again and the reader has passed the
  check there — at most once per search of a collection, never for a
  block, an error status, a timeout or a network failure. They stay on the endpoint's origins (§4.2): a path is
  refused unless it is absolute on the portal's origin, an absolute address
  unless its origin is declared, and a redirect elsewhere fails the request.
- Requests travel over `https`. The one portal reached over plain `http`,
  the catalogue's named exception (§3.1), is sent nothing but a territory,
  a commune and a year, and answers public registers' listings; nothing a
  reader keeps passes through it.
- Resolved targets are cached in memory for the session, by the resolver:
  each `View`, and each `Results` of a search that ran, keyed by the citation
  parts, up to 256 entries before the cache starts afresh. Errors and offline
  targets are not kept, so the next click asks the portal again. The one
  portal list kept is the commune index of an Archinoë finding aid, whose
  page is slow to build and lists nothing else (§4.6), also for the session. The only portal data
  written to the database is what the reader attaches (§6.4): image addresses,
  sizes and the attribution, never image bytes.
- Adding a view to a document being attached is a click too: one resolution of that one view, kept by the
  session cache like any other. No neighbouring view is asked for ahead of
  the reader.
- Image bytes are cached only by the browser's or WebView's HTTP cache,
  under the archive's own cache headers. The server never fetches, proxies
  or stores them, and a cropped region is cut by the client from the
  archive's image.
- Images are shown in OxidGene only for a `display: "iiif"` archive, always
  with the archive's attribution and a link to its terms. An archive whose
  reuse terms require accepting a licence keeps that step in its own pages
  and is displayed by its portal.

## 9. Testing

- Unit tests parse anonymized citations of every catalogued grammar, and of
  every series shape of §5.1: the series in words or by code, the period
  before or after it or in parentheses, no locality, a matricule or a bare
  number, a bare view.
- A table-driven corpus of fictitious citations runs the recognizer (§5.1)
  per convention, every part compared: the classic description, its short
  form, series in words, the normalized form (read exactly as the strict
  grammar reads it), structured records with the cited event, portal
  addresses, refusals, the reader's parts and their writing back, the place
  dictionary through a fake, the embedded catalogue's archives named as
  genealogists write them, and a second language added as a test-only
  vocabulary. Each part's signal is checked.
- Adapter tests replay recorded portal responses, anonymized and committed as
  fixtures, covering one match, several matches, no match, a changed
  response shape, and the platform's own cases: for Arkothèque, a locality
  matched as text, a period of several segments, a view beyond the
  register's images, an act filter value two kinds share, a qualified
  locality and its hamlets, several values of one kind searched together,
  matricules in two cells, a call number before the locality or the period
  in a title, a census of several lists a year, a cited call number on the
  second page of a populated locality, a keyed value read from the engine's
  lists, the IIIF images of a `display: "iiif"` archive, and a portal read
  by its pages (rendered rows, the no-match notice, an anti-bot page, the
  settings it refuses); for Ligeo, a combined act, a call number that only breaks a tie, a title-only table, an anti-bot challenge reported apart from drift, the images sized by their services' `info.json` rather than the manifest's canvases, a military register chosen by bureau, class and matricule range, a list of notices, qualified and composite locality cells, a register listed without a viewer link, an index of persons searched by matricule, a search within a finding aid, and the layout, page, single-year and margin settings; for Mnesys, acts written as letter codes, a lookup of the form's locality list, a form without a locality input, rows in several lots or without images, and several call numbers in one cell; for THOT, the session and cookie check, a label written with its article behind or its department, the two copies of a register told apart by call number, a table set aside for an act, a restricted register, a second page, a census searched by an interval of years, a military volume chosen by matricule, Corsica's checkboxes and register-level target opened without the viewer, a refused or expired session and a slide file without ARKs; for Pleade, a form search with its hidden inputs, a commune and its former namesake, a second page, tables, the registers told apart by their manifests' counts, a finding aid walked to the cited copy, a collection dated by its component, a military volume chosen by office, class and matricule, and a component without a viewer link; for Bach, entries named by their title or their link after a common title, a locality written in capitals or with its article behind it, an office naming its place, periods from dates, titles and ancestors (months, Republican years), tables told from the acts they index, an aid whose title says what its registers hold, publications of banns apart from marriages, a call number that only breaks a tie and one that rules a register out, a military register chosen by bureau, class and number before its index, a register without one viewer link, image names from the viewer's list or a link's range, an anti-bot page reported apart from drift, and a viewer link off the settings' viewer. For Visualys, the cited view found on the lot's sheet of thumbnails, a birth cited by its register's period, act number and view without a parish field, the licence standing before the views and lists of lots but not the entry, a military volume's view, a sheet not numbering the view and a page that is no sheet. For CAOMEC2, the commune as the form lists it (an apostrophe, an article), the kinds of a year told apart by their labels with their accents lost to the decoding, a baptism filed as a birth, a combined register none of a year's holds, a territory's code leaving the other territories unsearched, and the endpoint reached over plain `http` as the catalogue's exception.
- Transport tests check the declared origins, the header allow-list, plain
  `http` refused but for an endpoint marked `insecure_http` (native, bridge
  and desktop window alike), the
  native cookie jar, a portal read by its pages sent no request and a
  transport that loads no page refusing one; the desktop's, that a window's answers reach only their
  own request and only from the archive's origin, the waiting for the
  portal's page — a check that clears itself, one left to the reader with
  its deadline, a widget, a block, a blank page —, a request answered by
  a check sent once more, once only, and a page loaded then read back,
  or refused by a block before it is read.
- Anti-bot tests classify each vendor's challenge and block from the shared
  signatures, and a portal page carrying a vendor's scripts as the portal;
  the window's page script, run on Node.js (`just ui-js`), tells a
  challenge with text and a frameset apart from the portal.
- The window's consent script, run on Node.js over a minimal DOM (`just
  ui-js`), refuses each recognized manager's banner with its refuse control,
  never clicks an accept control, leaves accept-only banners to the reader
  and ignores pages without a banner; it acknowledges a listed information
  notice once while it watches, and not one offering a refusal; Rust tests
  read its messages, none of which says a consent was accepted. The
  progress overlay, run the same way, shows each step, the elapsed time from
  3 seconds, gives way and goes when told, and its Cancel posts the window's
  message; Rust tests cover when the
  overlay gives way and is gone, the cancellation of a resolution, and the
  landing of a cancelled lookup.
- The Linux window's certificate completion reads the issuer address of a
  certificate and verifies a completed chain with a test authority made for
  the test alone, refusing another name, an untrusted root, a wrong issuer
  and an expired certificate.
- Interface tests check each landing: the filtered search page on
  `challenged`, the view to go to on a portal without an address per view,
  and a target behind a reuse licence landing on its entry and going on to
  it; the desktop's, that the window goes on only once a page behind the
  licence shows after the licence page.
- Catalogue tests check unique ids and citation codes, that each archive
  sits in its country's directory, that an `iiif` archive has its
  attribution and terms, that every collection's `platform` has an adapter,
  that every adapter accepts its collections' `portal`, and that an adapter
  that cannot search a series refuses a collection holding one.
- Resolver tests run a scripted adapter over a counting transport: collection
  order, the fallbacks, the cache, and offline targets for `browser`
  portals.
- Live check tests (§9.1) replay the fixtures through a scripted transport:
  the citation built from the portal, the requests it takes, the choice of
  a register, the verdicts and the outcomes of a drifted or unanswered
  portal; and the bridge transport's exchanges, including an answer to
  another request and a challenge.
- API tests resolve recorded answers on both surfaces, a neighbouring view
  included, a citation recognized from a repository record and its event,
  an incomplete citation before and after the reader's parts, refused parts
  and a portal address; interface tests read the attached register, its
  kinds of record and the views a target lets attach from fictitious
  targets, the document drafted from cited views, what a citation offers,
  and the dialog's kinds and completion; the desktop's, the archive window's
  banner action and its message. Both check the offer to attach against its
  switch: nothing offered with it off, an `iiif` archive's views offered
  with it on (§6.3). The browser test opens a register in a tab without an
  offer to attach, and completes a citation in the dialog; its scenario
  attaching an `iiif` archive's views is skipped until the offer is turned
  on.

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

1. **Search page.** The portal's `robots.txt` is read first: a
   `Crawl-delay` for every robot or for OxidGene spaces the check's
   following requests (Ligeo portals ask 5 seconds), up to 30 seconds. The
   collection's search page loads, and every reference of its `portal`
   settings is still declared by the portal. The locality searched next is
   the alphabetically first of those listed for at least 10 records, where
   the portal says (Arkothèque), so that a pseudo-locality grouping a few
   registers is passed over.
2. **Discovery.** The alphabetically first locality the portal's own
   locality filter lists, with the first document kind of the collection's
   `acts` — an act, a table or a series alike — and no year, returns at
   least one register whose row yields an image address
   and a year within the collection's period. The engines list their most
   populated locality first, whose search is the slowest — the largest
   Sarthe parish's registers take 15 to 25 seconds to search uncached — so
   the check takes an ordinary one, and reads 25 rows (Arkothèque). A
   locality listed for other kinds only has the kind searched anywhere
   instead. Of the registers, it keeps the first
   that its citation can single out, one with a call number first: none
   other of the list shares its call number and image count with a period
   covering its year; failing that, the first one it can cite. A portal
   whose rows show no call number (Calvados, Ain) has its registers cited
   without one; one whose rows do not count the images (Archinoë) has the
   chosen register's counted by its viewer page, and so does Arkothèque,
   whose rows may count images that only the reading room shows: a
   citation cites the views the viewer shows. A register of one image is
   checked only where no other is listed, since the viewer then shows no
   view number. A register without an address per view whose count would
   cost an opening of the viewer (THOT's `views: "register"`: each opening
   writes a file on the server) is not counted, and cited at its first
   view; one with an address per view whose size only a viewer host the
   check does not request knows (the older Mnesys interface, whose probe
   says so) is cited at its second view, uncounted, so that step 4 sees the
   address open another view than the first. For a series the locality
   is what its search filters by — the commune of a census, the recruitment
   bureau of a military register — and registers that show the numbers they
   span (the volumes of one class) are also told apart by them.
3. **Resolution.** A citation assembled from that register — its locality
   as a citation writes it (`Le Bourg` for the portal's `Bourg (Le)`), the
   document kind by its code (`RM` for a military register), the first year
   of its period, its call number when it has one, the first number it
   spans when it shows them (`n° 501`), and its middle view
   `⌈count / 2⌉ / count` — resolves through the `Resolver`
   to `View` with that call number, that view and, where the adapter counts
   the images, that count; for a `display: "iiif"` archive, with the view's
   image and an attribution without a placeholder left. A platform whose
   viewer has no address per view (GAIA, whose probe says so) resolves to
   `View` with that call number and no views, which opens on the first
   view: step 4 checks view 1 and the register's count, where it was
   counted, then the drive to the cited view (`go_to` of the report's
   opening). The same citation
   without its call number resolves to the same register or to `Results`,
   never to another register.
4. **Opening.** The target loads in a browser and the portal's viewer shows
   the cited view: the view number it displays equals the cited one, once
   its reuse licence, if any, is accepted, and its view count, where it
   shows one, equals the register's. A viewer without an address per view,
   on its first view, is then brought to the cited view by the desktop
   window's own script (§6.1, `go_to.js` with the platform's `go_to`), which
   must answer that the viewer shows it; a register cited at its first view
   (THOT's `register` collections, uncounted) is brought to the middle of
   the views the viewer counts, unless it counts fewer than two. A target
   behind a reuse licence (Visualys, §4.12; the report's `licence`) is opened
   as the desktop window opens it: the licence's entry first, whose licence
   the check accepts as a reader does, a page behind it showing next, then
   the target.
5. **Images**, for a `display: "iiif"` archive: the picture and the
   thumbnail load as images no larger than the resolved size, the picture
   with the resolved proportions; a thumbnail may be the portal's own,
   square one (Mnesys).

What each platform's probe reads, and where its viewer shows the view:

| Platform | Step 1: references and localities | Step 4: viewer |
|---|---|---|
| Arkothèque | The search page's `data-moteur` and `data-contenu`; the engine's bare answer, the one the page requests on load (`/_recherche-api/moteur?refUnique=<engine>&<engine>--contenuIds[]=…`): its `filtres` hold the locality, act and period filters, its `restits` the display mode, the act filter's values every `acts` value with its record key; the localities are its aggregation of the locality filter's field. A portal read by its pages: its unfiltered search page as rendered names the engine, the content and each filter (`aria-filtre-<filter>`), and its rows the localities; the search of the first act checks the act values, and the rows count the images. | `input[data-cy="input-position-image"]`, count `[data-cy="nb-total-images"]`, after `button[data-cy="accept-license"]` (Sarthe). |
| Mnesys | The form `/search/form/<form>`: each select is an `enhanced-select` whose `data-options` lists its labels; every input of `fields` exists, a select named with its `[]` and a plain input without; the act select holds every `acts` label; the localities are the locality select's labels that a `locality_label` pattern names, or, for a plain locality input, the context entries of a search without it. A form without a locality input is searched without one. A row whose image count spans several lots is counted by the viewer's state. | `.media-browse .pagination-form input`, count `.media-browse .page-count`, after `input.btn.primary[value="Accepter"]`; the viewer's image requests are aborted, since it shows its view without them. |
| Ligeo | The search form `arc_form_rech` (or the finding aid's page) holds every input of the settings, and every act value a choice list offers; the localities come from what backs the locality input: the thesaurus the page script names (`VT_Control`, `str`), whose autocomplete (`POST <prefix>/xhr/gettheslist/<thesaurus>/0/<search>/<input>_Index`) is asked for `Sai`, a typed facet (`arcfacette.php?…&autoc=1`), the input's options or checkboxes, or a finding aid's branches; labels naming a parish, a place or a former commune are left out. A plain text input nothing backs is probed with the letters themselves; a search by year alone with no locality. | Monocle: `.monocle-PageNav input[role="spinbutton"]`, count `.monocle-PageNav-total`; Binocle (Hautes-Alpes): `.bn-gallery-counter input.bn-gallery-counter-current`, count `.bn-gallery-counter` (` sur 29`). |
| Archinoë | `registre`: the locality select's labels, the act select holds every act identifier, the year input exists. `seriel`: the form names its inputs (quoted with apostrophes); the localities are the autocomplete's suggestions for `Sai` (`ir_seriel_data.php`) that follow `locality_label`. `ead`: the finding aid's root lists the communes, a leading article written behind the name. The results count no images: the chosen register's viewer page does (one `div_image_<n>` per view). | `#visu_pagination` (`n/total`). |
| Prismia Vision | The API key of `/runtimeConfig.js`; the facet endpoint lists the act filter's values, which hold every `acts` value, and the localities, written `Name (Article)`. | `button[aria-label="Numéro de la vue"]` (`n` and `total`). |
| THOT | The session and the module's form: the locality list, every `acts` value among the type criterion's options or checkboxes, the year inputs; the localities are the list's labels as citations name them, a hamlet or a placeholder passed over. The results count no images: an `ark` collection's chosen register is counted by its slide file; a `register` one's is not counted (above). | `input#imageNum`, count `h3#imageNumMax`, the tiles aborted; a `register` collection's viewer is then brought to a view by changing `input#imageNum`. |
| Bach | The classification scheme's entries of the collection, or the finding aid's nodes at its level, an office cited by its place (`Exampleville` for `Bureau de recrutement d'Exampleville`); a series of one office has none. The discovery reads the locality's aid; since aids list registers without images too, it opens the first registers a citation would name, call numbers first, at most three, until one links to its images, which the viewer's list or the link's range counts. | `#currentpage input[type="number"]`, count `#currentpage` (`/ 71`); the viewer's image requests are aborted. |
| GAIA | The search's first list (`…/R/0/0`): its first locality written in full, read as step 1 of §4.8 reads a label; without `localities`, the list holds every first label of `types`. The discovery runs the wizard without a year, a list left answered by its first choice (a search through every choice fails on the server for the Aude censuses); the chosen register's viewer page counts the views (one `docs` object each). | `#pagination input[type=text]` (`Page 1 de total`), on view 1 since the viewer has no address per view, then brought to the cited view by typing it in that box and leaving it (`Page n de total`); its image requests are aborted. |
| Visualys | The visitor's entry, then `localities`: the list of the initial `A`, its first locality listed without a parish; `search`: the form, offering the settings' kind, its first office. The search lists lots, counted for the registers; a military volume is not counted, and cited at its second view. | The licence (`#btnAccepter`) at the site's entry, accepted, then the viewer: `span#LabelImage` (`30 / 248`); its images aborted. |
| CAOMEC2 | The territory's form: its commune list, act type and year fields; its first commune that is no hospital. The search counts no images: the chosen register's viewer does (one `div#thn<i>` each). | `input#iddoc`, count the last thumbnail's number (`#imgstrip .thn:last-child .thn_id`), on view 1 since the viewer has no address per view; the images aborted. |
| Gers portal | The module's form: the locality list's and former communes' fields, the settings' checkboxes; its first locality. The results count the images. | `select#fichier option[selected]` (`k / total`), the image aborted. |
| Older Mnesys | The guided search's form: the settings' fields, kinds and title-matching companions; the place index is behind a script `robots.txt` disallows, so the locality is the first place a hit names in a search of the collection's first kind at the last year of its period, without a locality (none for the military registers). Registers are not counted (above). | `#inputvue_actuelle`, count `#total`, on the viewer host; the images aborted. |
| Pleade | `form`: the form's locality list (its current communes, as citations name them), kinds and year input. `tree`: the aid's table of contents down its first branch of the collection's first kind to the localities' depth; the first registers' components give their ARKs and dates. The chosen register's manifest counts its images. | Mirador 3, `.mirador-canvas-nav input[type="number"]`, count `.mirador-canvas-nav label` (`/ 269`); Mirador 2, `li.highlight .thumb-label`, count `.canvas-count`; the images aborted. |

**How it runs.** The adapter logic stays in Rust, whichever transport
carries the requests:

| Steps | Collections | Where | Transport |
|---|---|---|---|
| 1 to 3 | `transport: "any"` | `crates/oxidgene-archives/tests/live.rs`, `#[ignore]`d, features `native` and `live` | `native` (§4.2), identifying `User-Agent` |
| 1 to 3 | `transport: "browser"` | the `archives-live-bridge` binary (feature `live`), started by `e2e/archives/live.spec.ts` | a Chromium page, as the desktop window |
| 4 and 5 | every resolved one | `e2e/archives/live.spec.ts` | the same Chromium page |

The browser part is a Playwright project of its own,
`e2e/playwright.archives.config.ts`: one worker, no retry, no server of
ours, and its own test directory, so `just e2e` never runs it. Its
`User-Agent` is Chromium's own followed by
`OxidGene-live-check (+https://github.com/trois-six/oxidgene)`.

**The bridge.** For a browser-only collection, `live.spec.ts` starts
`archives-live-bridge <archive id>`, which runs the same steps with a
`PortalTransport` whose requests are JSON lines on its standard output,
each answered by one line on its standard input before the next is sent:
`connect` asks the page to load the endpoint's start page and wait until it
shows the portal rather than an anti-bot page (as below), `fetch` asks
it to run one request with the page's own `fetch` and
`credentials: "same-origin"`, as the window's script does, and `page` asks it
to load a page of a portal read by its pages, wait until it is the portal's
own page and `ready` matches, and answer with the document as rendered
(§4.2). The check reads such a portal's `robots.txt` as a page too. The answers are checked as the window's are
(`PageAnswer`): a final address on the endpoint's origins, a success
status, a bounded body. The binary ends with its collections' reports and
the indices of the collections the native test checks instead. The process
boundary is two standard streams: no port, no new dependency — the binary
drives its future on its own thread, since its requests block on the
streams — and the same `Resolver`, adapters and verdicts run over both
transports.

A portal that sends the browser to another site instead of its own page
refuses the identified agent as a challenge would: the Archinoë v2 hosts
(Charente-Maritime, Oise) redirect a `User-Agent` naming OxidGene to a
search engine, whether a plain client's or the browser's, and so they do a
standard browser `User-Agent` followed by the project's address alone
(checked 2026-10-06): what they refuse is any agent identifying itself.
Their checks end `challenged` at once, as expected, and stay unverified: §8
asks the checks to identify themselves, and a check does not hide what it
is to pass. A reader's own
browser and the desktop window send the browser's own `User-Agent` and are
not refused.

A challenge is told apart from a drift on every path, with the signatures
of §4.2. The bridge's `connect` classifies the page as the desktop window
does: it waits out a check that clears itself, and fails with
`FetchError::Challenged` on a block, or on a check still on screen when its
30 seconds end — a live check answers no check, and a check left for a
reader is not answered for them. An error status whose body is an anti-bot
page is `FetchError::Challenged` too, for the native transport, the bridge
and the desktop window alike (`PageAnswer`); and an answer a success status
let through is the adapter's or the probe's to tell, once it cannot read it
(`markup::unreadable`). Each becomes `ResolveError::Challenged`, and the
collection `challenged` rather than `drift` or `unreachable`. Step 4 names
the vendor and the kind of the page it met (`cloudflare block`, `anubis
challenge`) in its received shape. It also watches the viewer's own
requests: a viewer that shows another view than the cited one after an
anti-bot page answered one of them, in place of a manifest or a page list,
was refused, not drifted — the Alpes-Maritimes firewall blocks the
manifest of the live check's browser after its searches —, and the opening
ends `challenged`.

What a check of an archive behind an anti-bot measure is expected to end
on — the runners have datacentre addresses, which some vendors score
badly, and some deny rules match a headless browser; a check never runs
headful, nor hides what it is, to pass:

| The portal's measure | Expected outcome |
|---|---|
| A check that clears itself in a headless browser (a bot-mitigation redirect, a proof of work let through) | `ok`: waited out like the window does. |
| A check left for a reader (a widget, or one that stays) | `challenged`, unverified. |
| A block of the headless browser or of the runner's address (a Cloudflare block, such as the Landes portal's after the browser's first page; an Anubis deny rule), at any step, step 4 included | `challenged`, unverified; the steps before it keep their verdicts. |
| A challenge answering the native client | `challenged` for that collection; the desktop resolves it in the window. |
| A certificate served without its issuer | Not seen: Chromium completes the chain, as the desktop window on Linux does (§6.1). |

**Reports.** `just archives-live` writes under `target/archives-live/`
(`OXIDGENE_LIVE_REPORT_DIR`), or in a directory per archive within it when
it checks several, `native.json`, the native test's, and `report.json`, the
run's: per archive its outcome, and per collection the
transport, the outcome, the failing step with the expected and received
shapes, the number of requests sent to the portal (five to thirteen per
collection for steps 1 to 3, `robots.txt` included), the locality searched,
the citation built, and the opening of steps 4 and 5. Each archive ends in one of four outcomes, the worst of its
collections':

| Outcome | Meaning | Run result |
|---|---|---|
| `ok` | Every step passed. | Pass |
| `drift` | The portal answered, but not as the adapter or its settings expect; the failing step and the expected and received shapes are reported. | Fail |
| `unreachable` | Timeout, network error or a `5xx` answer. | Warning; an issue after two consecutive scheduled runs |
| `challenged` | An anti-bot challenge answered in place of the portal, to the headless browser or to the native client. | Warning, reported as unverified |

A check never solves or works around a challenge. Reports name the archive,
step, URL path and response shape, never response bodies beyond the fields
compared; failure artifacts (Playwright traces) hold portal pages only and
are kept for a short period.

**Adding an adapter's live check.** Every adapter arrives with its live
check, in three places; archives are then picked up from the catalogue:

1. `crates/oxidgene-archives/src/platform/<platform>/live.rs`, compiled
   under `#[cfg(any(test, feature = "live"))]`, implements `live::Probe` for
   the adapter: `search_page` loads the search page and whatever declares
   the references the settings use, fails with
   `Failure::drift(Step::SearchPage, …)` naming what is missing, and
   returns the alphabetically first locality the portal's locality filter
   lists, as a citation writes it; `registers` sends the search a citation
   of that locality and act would send, without a year, and maps each
   result to a `live::Register` (the locality as a citation writes it, the
   call number and the image count where the results show them, the
   displayed period, the image address). A platform whose results do not
   count the images also implements `images`, which counts those of the
   chosen register from the portal's own pages; one whose collection has no
   address per view answers `addresses_views` with `false` for it, and one
   whose portal cannot count a register's images answers `counts_images`
   with `false`.
2. `live::probe` in `crates/oxidgene-archives/src/live/mod.rs` lists it by
   platform id; the `every_adapter_has_a_probe` test fails until it does.
3. `crates/oxidgene-archives/src/platform/viewers.json`, which
   `e2e/archives/viewers.ts` and the desktop window read, describes the
   portal's viewer by platform id: the element showing the view number, and
   the licence button and the view-count element where the viewer has them —
   for a target behind a reuse licence, the licence page's button —; and,
   for a viewer without an address per view, its `go_to` (§6.1), which the
   `every_viewer_without_an_address_per_view_can_be_driven` test requires.

The probe's own tests replay the platform's anonymized fixtures, as the
Arkothèque probe's replay `ad44-search-page.html` and `ad44-engine.json`.

### 9.2 Scheduled run

A dedicated workflow, `.github/workflows/archives.yml`, runs the live checks
every Monday and on demand, with an optional archive id as input. It is not
part of the nightly workflow, does not gate releases, and is not a required
status check: a portal change is not a defect of a commit. A first job lists
the archives from the catalogue — those with a collection, whose entry does
not set `"live_check": false`, or the one named — and groups them by the
address their portal resolves to: several services hosted by one vendor
answer on one address and are rate-limited together (the six Bach portals
refused requests after 15 to 25 of them in a row), as are the Mnesys, Ligeo
and Arkothèque hostings. Each group becomes an independent matrix entry
without fail-fast, so one group's drift does not hide another's, and runs
`scripts/archives-live.sh <archive id…>`, the script behind `just
archives-live`, which checks its archives one after another with a pause
of two minutes before each archive sharing the previous one's address
(`OXIDGENE_LIVE_HOST_PAUSE`); `just archives-live` without an archive
spaces them the same way. The entry keeps its reports for 15 days — one
directory per archive when it checks several — and its Playwright traces
for 5.

A `drift`, or an `unreachable` whose previous scheduled run, read from that
run's report artifact, was `unreachable` too, opens an issue labelled
`archive-drift` for that archive, titled `Archive portal drift: <archive
id>`, or comments on the open one; an `ok` run closes it. The issue lists
the failing collections with their step and shapes, and names the catalogue
entry, the adapter and the viewer description to update. Fixing a drift
updates the collection's `portal` settings, or the adapter and its recorded
fixtures when the platform itself changed, so the offline tests learn the
new shape.

The checks follow §8: one archive at a time per job, archives of one
address one after another and paused apart, sequential requests, a
handful per collection and run, the identifying `User-Agent` with the
repository address, and no retry within a run. A portal whose `robots.txt`
disallows automated agents, Loire-Atlantique and the Ligeo portals included, is checked only at
this weekly rate; an archive that objects is marked `"live_check": false` in
its catalogue entry, which every live check, `just archives-live <archive
id>` included, then skips, and relies on the user-reported failures of §7.

## 10. Delivery phases

1. Create `oxidgene-archives`; move the catalogue and the citation parser
   into it with the extended grammar (§5.1); implement the Arkothèque adapter
   with both transports; catalogue Loire-Atlantique and Sarthe; make the
   desktop window resolve and load targets; remove the injected driver
   script; add the live checks of both archives, `just archives-live` and the
   weekly workflow (§9.1, §9.2), and list them in
   [Development §2.7](development.md#27-test-categories).
   Every later archive or adapter arrives with its live check. All of it is
   in place.
2. Add the Mnesys adapter with Indre-et-Loire; add the backend endpoint on
   both surfaces and open targets from the web client; add `display`, the
   IIIF view in the shared viewer, attaching views as a remote multi-page
   document, and `Media.thumbnail_url`. All of it is in place, the offer to
   attach disabled since (§6.3).
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
| Mnesys Expo | Naoned (Nantes) | 18 confirmed portals: 14, 19, 25, 26, 27, 37, 39, 51, 55, 58, 59, 68, 69, 73 (older interface, §4.14), 80, 90, 91, 972. Mnesys customers whose portal runs another platform: 18, 34, 93 | Strong (`/search/form/<uuid>` entry points, Mnesys Expo logos, case studies)[^naoned-references] |
| Ligeo Diffusion | Boscop (Angers) | 29 confirmed portals: 01, 02, 05, 06, 07, 12, 13, 16, 29, 31, 33, 34, 41, 42, 48, 56, 57, 63, 67, 70, 74, 76, 79, 86, 88, 89, 92, 93, 95 | Strong (URL pattern of the civil-status page, or announcement)[^ligeo-references] |
| Archinoë / Prismia Vision | EidoPolis (Laval) | 17, 21, 47, 60, 62; a viewer for 49; former pages for 07 | URL pattern, legal notice (21)[^ad21-legal], announcement (47)[^ad47-portal] |
| Archives nationales d'outre-mer (CAOMEC2) | National service | 973, 974, 976 | Observed (§4.15): the `caomec2` civil-status search[^anom-civil-status] |
| GAIA 9 | Unidentified vendor (the Ariège instance is hosted by Oxyd) | 09, 11, 61, 66, 77; a legacy site for 38 | Observed (§4.8): the pages are titled `GAIA 9 : moteur de recherche - <version>` |
| THOT | Vendor not named (`sicem`, `Cindoc` in the markup) | 2A/2B, 35 | Observed (§4.9)[^thot-portals] |
| Pleade | AJLSM | 53, 64 | Observed (§4.10)[^pleade-portals] |
| Bach | Anaphore | 30, 52, 81, 82, 84, 971 | Observed (§4.11): every footer reads `Développé par Anaphore`; one hosting address[^bach-portals] |
| Visualys ("salle virtuelle") | Vendor not named | 22 | Observed (§4.12) |
| Gers portal | The archive's own | 32 | Observed (§4.13) |

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
- Seine-et-Marne (77): GAIA, although the department's site links other
  departments' Ligeo, Archinoë and Mnesys portals.
- Isère (38): its online civil status runs Arkothèque; a legacy GAIA site
  (`/mdr/`) remains, whose certificate names another host.

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
| GAIA 9 | `/mdr/index.php/rechercheTheme/requeteConstructor/<theme>/1/R/0/0` (`/mdr_aude/…` in Aude), often in an `<iframe>` of the archive's site |
| THOT | `/Internet_THOT/FrmSommaireFrame.asp`, `/thot_internet/FrmSommaireFrame.asp`; `Recherche/FrmRechFrame.asp?MOD=<n>` |
| Pleade | `/archives-en-ligne/<form>-search-form.html`, `/archives-en-ligne/ead.html?id=<finding aid>`, `/pleade/theme/…` assets, `ark:/<naan>/<name>/f<n>` viewer addresses |
| Bach | `/archives/classification-scheme`, `/document/<finding aid>`, `/archives/show/<finding aid>_<node>`; a viewer under `/viewer/series/…` or on a `v-earchives.` or `viewer-recherche.` host |
| Visualys | `/<family>/<site>/commune.aspx`, `connexion.aspx?ref=demo`, `VisualysConsult` windows (Côtes-d'Armor) |
| Gers portal | `/archives_numerisees/portail/<module>/recherche/` and `…/visu/?…&fichier=<id>` |

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
  Vienne share a Ligeo portal, Mayenne runs Pleade (§4.10), as the
  Pyrénées-Atlantiques do, and Tarn-et-Garonne Bach (§4.11).

Cases the catalogue model must express:

- **Savoie.** The portal of the Archives de la Savoie runs the older Mnesys
  interface, not Mnesys Expo: its guided searches are `GET` forms (`F_search`)
  whose hits are finding-aid nodes, a register's images reached through its
  notice on a viewer host of its own. The Mnesys Expo adapter (§4.4) does not
  apply: it has its own (§4.14).
- **Côtes-d'Armor.** The portal shows a reuse licence it does not enforce
  server-side. OxidGene does not skip it: every target stands behind it, a
  client opens the site's entry first, and the desktop window goes on to the
  cited view or the list of lots once the reader has accepted it (§4.12,
  §6.1).

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
  is a reconstitution, published apart from the civil status from 1860
  onwards. The civil status from 1860 is searched by arrondissement, the
  locality in the `district` style (`05` for `Paris 5e`), and its registers
  are told apart by the act numbers they span; citations write the
  arrondissement in many ways (`Paris 11e`, `Paris XIe`, `Paris (11e)`,
  `11e arrondissement`), read by the archive's `citation.districts` (§5.1).
  The daily burial registers of the Parisian cemeteries, inside and outside
  the city (Pantin, Ivry, Bagneux, Thiais, Saint-Ouen…), are a collection of
  their own (`RI`): one engine, filtered by cemetery (a list of eighteen
  names, `Père Lachaise`, `Ivry`, `Saint-Ouen`) and by a year slider
  that tests overlap with each row's dates; each register (call number
  `<cemetery>_RJ<first year><last year>_<volume>`, the period read by the
  archive's `call_number_periods`) is listed in parts of some 600 entries
  and 31 images, dated and numbered by their first and last burials. Paris
  citations name the series `C` (`AD75 - Pantin - C - 1918 -
  PAN_RJ19181918_03 - ordre 981 - vue 20/31`), which the archive's
  `citation.series` adds, the cemetery as the locality (`Cimetière parisien
  de Pantin`, `Ivry-sur-Seine` read as the portal's `Ivry` by its
  `citation.localities`). The annual burial indexes (*répertoires
  annuels*), which lead to the entry, are not catalogued. The reconstituted acts and
  files have no locality, the decennial tables are split by surname range
  (no citation names the range), and the 1926–1936 censuses are searched by
  name or address on a page without an engine: none is catalogued. The
  archive is displayed by its portal: its reuse terms ask a credit with the
  download date, which the attribution template cannot hold, and exclude
  collections digitised with private partners without naming them.
- **Landes.** Behind Cloudflare, which refused every request of the
  engine that a script of an automated Chromium sent, the page's own
  `fetch` included, while the search page loaded with its filters in its
  address rendered the rows. Its registers are therefore searched by page
  loads (`transport: "page"`, §4.2, §4.3): the window loads the filtered
  search page as a reader would and reads the rows its scripts render, the
  view count from the row. Observed 2026-10-06: the desktop's WebKitGTK
  passes every page load, and even a scripted request, while an automated
  Chromium is blocked after its first HTML page, whatever it loads next —
  so the live check ends `challenged` at its discovery, unverified (§9.1).
  The military registers are not catalogued: their class filter finds
  nothing by a plain year, and without it a bureau lists some 300 registers
  over pages that would each be a full page load.
- **Arkothèque collections left out.** Name-indexed military registers (one record per soldier: 04, 24's indexed
  classes, 71) and inventory pages without images are not register
  collections. The Aube's military registers and succession tables have no
  search engine on their landing pages.
- **Overseas.** Civil status for Guyane (973), La Réunion (974) and Mayotte
  (976, 1844–1906) is consulted through the Archives nationales
  d'outre-mer[^anom-civil-status] rather than through the local service: one
  national-level entry whose search takes the territory as a parameter
  (`territoire=GUYANE`, `REUNION`, `MAYOTTE`), carrying the territories'
  citation codes: `ANOM`, and `ANOM973`, `ANOM974`, `ANOM976` naming one
  territory. Genealogists also name it by its former name, the *Centre des
  archives d'outre-mer* (`CAOM`), an alias. Its portal answers over plain
  `http` only — its `https` port offers TLS 1.0 alone, with a self-signed
  certificate for another host that expired in 2019 (2026-10-06) —: the
  archive is the catalogue's one named exception to `https` (§3.1). The
  territories' own services keep their codes (`AD974`): a citation of their
  copies is not one of the national copy.

### 11.4 Cataloguing order

1. Fetch each departmental site's home page and look for `arko`, `mnesys`,
   `ligeo`, `archinoe`, `prismia` and `/mdr/` (§11.2). This settles most
   unconfirmed rows.
2. Never rely on a vendor's reference list alone: it is self-declared, mixes
   management and dissemination, and lags. Cross-check it with the portal's
   footer or credits page.
3. Before cataloguing an archive, inventory every collection of registers
   its portal publishes — parish registers, civil status, decennial tables,
   reconstituted or duplicate series, and the other series of §3.1:
   military registers (*registres matricules*) and conscription lists,
   population censuses, tables of successions and absences — from the
   portal's own navigation, not from the single entry point of §11.5, and
   give each searched through its own engine or form a collection with its
   document kinds and period. A series whose platform's adapter cannot
   search it yet (§4.2) is noted, not catalogued. Each collection gets its
   live check (§9.1).
4. Catalogue the Arkothèque and Mnesys Expo archives first, then Ligeo, then
   Archinoë / Prismia Vision, then GAIA and Bach, then the engines of a
   single archive (Visualys, the Gers portal, the older Mnesys interface).
   The Archives nationales d'outre-mer (973, 974, 976) are catalogued over
   plain `http`, as a named exception (§3.1, §11.3).

### 11.5 Survey by department

The entry point is the civil-status or parish-register search page seen on
the department's own site or supplied with this specification. Each row
gives one entry point, usually the civil-status one, and lists a second only
where it was supplied (Paris, Tarn-et-Garonne); parish-register entry points are in §11.6. The table is therefore not an
inventory of collections: many departments publish parish registers, civil
status, decennial tables or reconstituted series through distinct engines or
search pages that are not listed here, and each needs its own collection in
the catalogue entry (§3.1). Platforms marked *probable* rest on a vendor list
only, and *unconfirmed* means no evidence was found.

| No. | Department | Site | Civil-status entry point | Portal platform | Evidence |
|---|---|---|---|---|---|
| 01 | Ain | `www.archives.ain.fr` | [`www.archives.ain.fr/archive/recherche/etatcivil/n:88`](https://www.archives.ain.fr/archive/recherche/etatcivil/n:88) | Ligeo | Ligeo references; URL pattern; observed (§4.5); catalogued, searched 2026-10-05: parish and civil registers, censuses, military registers, succession tables |
| 02 | Aisne | `archives.aisne.fr` | [`archives.aisne.fr/archive/recherche/etatcivil/n:11`](https://archives.aisne.fr/archive/recherche/etatcivil/n:11) (FranceConnect for recent civil status) | Ligeo | Ligeo references; URL pattern; redesigned portal; catalogued, searched 2026-10-05: parish and civil registers, censuses, military registers, succession tables |
| 03 | Allier | `archives.allier.fr` | [`archives.allier.fr/rechercher/archives-numerisees/genealogie-histoire-des-familles/etat-civil-en-ligne`](https://archives.allier.fr/rechercher/archives-numerisees/genealogie-histoire-des-familles/etat-civil-en-ligne) | Arkothèque | Observed and live-checked (§4.3): registers and tables, RP, RM, CM, TSA |
| 04 | Alpes-de-Haute-Provence | `www.archives04.fr` | [`www.archives04.fr/rechercher/archives-en-ligne/etat-civil/actes-etat-civil`](https://www.archives04.fr/rechercher/archives-en-ligne/etat-civil/actes-etat-civil) | Arkothèque (since 2008) | Observed and live-checked (§4.3): civil status, parish registers, TD, RP |
| 05 | Hautes-Alpes | `archives.hautes-alpes.fr` | [`archives.hautes-alpes.fr/archive/recherche/etatcivil/n:198`](https://archives.hautes-alpes.fr/archive/recherche/etatcivil/n:198) | Ligeo | Ligeo references; URL pattern; catalogued, searched and live-checked 2026-10-06: registers (notice lists), censuses, military registers (by class); its viewer is Binocle, behind Anubis, which denied a headless browser on 2026-10-05 (the opening then ends `challenged`) and let it through on 2026-10-06 |
| 06 | Alpes-Maritimes | `archives06.fr` | [`archives06.fr/archive/resultats/etatcivil2/n:101?type=etatcivil2`](https://archives06.fr/archive/resultats/etatcivil2/n:101?type=etatcivil2) | Ligeo | URL pattern; F5 challenge, observed (§4.5); reachable by a browser only, which the challenge let through when checked again; catalogued, searched 2026-10-06: registers, censuses; the military registers are a name index whose rows link no viewer, not catalogued. Its F5 firewall refuses the viewer's manifest to the live check's browser after its searches: the opening ends `challenged` |
| 07 | Ardèche | `archives.ardeche.fr` | [`archives.ardeche.fr/archive/recherche/etatcivil/n:96`](https://archives.ardeche.fr/archive/recherche/etatcivil/n:96) | Ligeo (older pages on Archinoë) | Ligeo references; URL pattern; observed (§4.5); catalogued, searched 2026-10-05: parish registers, civil status, censuses, succession tables |
| 08 | Ardennes | `archives.cd08.fr` | [`archives.cd08.fr/archives-numerisees/sources-genealogiques/registres-paroissiaux-et-detat-civil`](https://archives.cd08.fr/archives-numerisees/sources-genealogiques/registres-paroissiaux-et-detat-civil) | Arkothèque | Observed and live-checked (§4.3): registers, RP, RM, TSA |
| 09 | Ariège | `archives.ariege.fr` | [`mdr-archives.ariege.fr/mdr/index.php/rechercheTheme/requeteConstructor/1/1/R/0/0`](https://mdr-archives.ariege.fr/mdr/index.php/rechercheTheme/requeteConstructor/1/1/R/0/0) | GAIA 9 | Observed and live-checked (§4.8): parish and civil registers, TD; the engine's only search |
| 10 | Aube | `www.archives-aube.fr` | [`www.archives-aube.fr/actualites-1/les-actualites-anterieures-a-2010/validation-pages-1/etat-civil-des-communes-de-laube`](https://www.archives-aube.fr/actualites-1/les-actualites-anterieures-a-2010/validation-pages-1/etat-civil-des-communes-de-laube) | Arkothèque | Observed and live-checked (§4.3): registers, TD, Troyes parishes, RP |
| 11 | Aude | `archivesdepartementales.aude.fr` | [`archivesdepartementales.aude.fr/letat-civil`](https://archivesdepartementales.aude.fr/letat-civil), embedding [`mdr.aude.fr/mdr_aude/index.php/rechercheTheme/requeteConstructor/1/1/R/0/0`](https://mdr.aude.fr/mdr_aude/index.php/rechercheTheme/requeteConstructor/1/1/R/0/0) | GAIA 9 | Observed and live-checked (§4.8): parish registers, civil status, TD, RP, RM; the site sits behind an F5 check, the engine's host behind none; the cantonal decennial tables are not catalogued |
| 12 | Aveyron | `archives.aveyron.fr` | [`archives.aveyron.fr/archive/recherche/etatcivil/n:122`](https://archives.aveyron.fr/archive/recherche/etatcivil/n:122) | Ligeo | Ligeo references; URL pattern; catalogued, searched 2026-10-05: parish and civil registers, censuses, succession tables |
| 13 | Bouches-du-Rhône | `www.archives13.fr` (new site, May 2026) | [`www.archives13.fr/archive/recherche/etatcivil/n:64`](https://www.archives13.fr/archive/recherche/etatcivil/n:64) | Ligeo | Ligeo references; URL pattern; reachable by a browser only (incomplete certificate chain, firewall), in maintenance during the 2026-10-05 checks; checked again 2026-10-06: its search pages load in a browser, but the site announces incidents, answered one search with an error page after 30 seconds, and failed every request of the live check; retried once 2026-10-07 with its four collections drafted (registers, censuses, military registers, succession tables): the registers' search form answered `503`, the next search page an anti-bot check, the others no answer: not catalogued |
| 14 | Calvados | `archives.calvados.fr` | [`archives.calvados.fr/search/form/ecf01748-923d-463a-8d80-bd4142582bcd`](https://archives.calvados.fr/search/form/ecf01748-923d-463a-8d80-bd4142582bcd) | Mnesys Expo | Mnesys Expo logo, Naoned case study; catalogued (§4.4): registers, censuses; live-checked 2026-10-05 |
| 15 | Cantal | `www.archives.cantal.fr` | [`www.archives.cantal.fr/vos-archives/etat-civil/recherche-dans-letat-civil`](https://www.archives.cantal.fr/vos-archives/etat-civil/recherche-dans-letat-civil) | Arkothèque | Observed and live-checked (§4.3): registers and TD, RP, RM, TSA |
| 16 | Charente | `lasource.archives.lacharente.fr` | [`lasource.archives.lacharente.fr/archive/resultats/etatcivil/n:115?type=etatcivil`](https://lasource.archives.lacharente.fr/archive/resultats/etatcivil/n:115?type=etatcivil) | Ligeo | Ligeo references; URL pattern; catalogued, searched 2026-10-05: parish and civil registers, censuses, military registers, succession tables |
| 17 | Charente-Maritime | `archives.charente-maritime.fr` | [`archinoe.com/v2/ad17/registre.html`](https://archinoe.com/v2/ad17/registre.html) | Archinoë | URL pattern; catalogued, searched 2026-10-05; redirects any agent identifying itself, so its live check ends `challenged` (§9.1) |
| 18 | Cher | `www.archives18.fr` | [`www.archives18.fr/archives-numerisees/registres-paroissiaux-et-etat-civil`](https://www.archives18.fr/archives-numerisees/registres-paroissiaux-et-etat-civil) | Arkothèque (portal); Naoned customer | Observed and live-checked (§4.3): registers and TD, RP, RM, TSA |
| 19 | Corrèze | `www.archives.correze.fr` | [`www.archives.correze.fr/search/form/3b1ba8cc-6c08-47cd-a90e-f9b231fdc30f`](https://www.archives.correze.fr/search/form/3b1ba8cc-6c08-47cd-a90e-f9b231fdc30f) | Mnesys Expo | Mnesys Expo logo; URL pattern; catalogued (§4.4): registers, censuses, military registers; live-checked 2026-10-05 |
| 2A / 2B | Corse (Archives de la Collectivité de Corse) | `archives.isula.corsica` | [`archives.isula.corsica/Internet_THOT/FrmSommaireFrame.asp`](https://archives.isula.corsica/Internet_THOT/FrmSommaireFrame.asp) | THOT | Single site since December 2020; observed (§4.9); catalogued 2026-10-06, behind Cloudflare (browser only), registers opened at their first view: registers of acts and tables, censuses; the military registers are a name index, not catalogued |
| 21 | Côte-d'Or | `archives.cotedor.fr` | [`archives.cotedor.fr/console/ir_ead_visu.php?eadid=FRAD021_000000912&ir=26564`](https://archives.cotedor.fr/console/ir_ead_visu.php?eadid=FRAD021_000000912&ir=26564); formerly [`archinoe.fr/v2/site/AD21/Rechercher/Recherche_thematique/Genealogie`](https://archinoe.fr/v2/site/AD21/Rechercher/Recherche_thematique/Genealogie) | Archinoë / Prismia | Legal notice: hosted by EidoPolis Prismia; URL pattern; catalogued, browsed 2026-10-05; its commune index kept for the session (§4.6) |
| 22 | Côtes-d'Armor | `archives.cotesdarmor.fr` | [`sallevirtuelle.cotesdarmor.fr/EC/ecx/commune.aspx`](https://sallevirtuelle.cotesdarmor.fr/EC/ecx/commune.aspx) | Visualys (ASP.NET "salle virtuelle") | Observed (§4.12); catalogued, searched and live-checked 2026-10-06: registers, military registers; the target, the cited view or the list of lots, stands behind the reuse licence the reader accepts (§6.1) |
| 23 | Creuse | `archives.creuse.fr` | [`archives.creuse.fr/rechercher/archives-numerisees/registres-paroissiaux-et-de-letat-civil`](https://archives.creuse.fr/rechercher/archives-numerisees/registres-paroissiaux-et-de-letat-civil) | Arkothèque | Observed and live-checked (§4.3): registers and TD, RP, RM, TSA |
| 24 | Dordogne | `archives.dordogne.fr` | [`archives.dordogne.fr/archives-numerisees/genealogie/registres-paroissiaux-et-detat-civil`](https://archives.dordogne.fr/archives-numerisees/genealogie/registres-paroissiaux-et-detat-civil) | Arkothèque | Observed and live-checked (§4.3): registers, TD, RP, RM |
| 25 | Doubs | `portail-archives.doubs.fr` | [`portail-archives.doubs.fr/search/form/4d44dde5-4523-4384-a2da-c1169870f1b2`](https://portail-archives.doubs.fr/search/form/4d44dde5-4523-4384-a2da-c1169870f1b2) | Mnesys Expo | Logo, Naoned case study; URL pattern; catalogued (§4.4): registers, decennial tables, censuses, military registers, succession tables; live-checked 2026-10-05 |
| 26 | Drôme | `archives.ladrome.fr` | [`archives.ladrome.fr/search/form/f6e7c1a1-9bda-40bc-a68b-13ed003eb0e5`](https://archives.ladrome.fr/search/form/f6e7c1a1-9bda-40bc-a68b-13ed003eb0e5) | Mnesys Expo (since February 2020) | Naoned case study; URL pattern; catalogued (§4.4): registers, censuses, military registers; `browser` transport (certificate without its intermediate); live-checked 2026-10-05 |
| 27 | Eure | `archives.eure.fr` | [`archives.eure.fr/search/form/a3b9883f-0939-449a-bcbf-9de4c2d49b89`](https://archives.eure.fr/search/form/a3b9883f-0939-449a-bcbf-9de4c2d49b89) | Mnesys Expo | Naoned customer list; URL pattern; catalogued (§4.4): registers, censuses, military registers, succession tables; live-checked 2026-10-05 |
| 28 | Eure-et-Loir | `archives28.fr` | [`archives28.fr/archives-et-inventaires-en-ligne/histoire-des-individus-des-populations-et-genealogie/les-registres-paroissiaux-et-detat-civil`](https://archives28.fr/archives-et-inventaires-en-ligne/histoire-des-individus-des-populations-et-genealogie/les-registres-paroissiaux-et-detat-civil) | Arkothèque | Observed and live-checked (§4.3): registers and TD, RP, RM, TSA |
| 29 | Finistère | `archives.finistere.fr` | [`recherche.archives.finistere.fr/archive/resultats/etatcivil/n:138?type=etatcivil`](https://recherche.archives.finistere.fr/archive/resultats/etatcivil/n:138?type=etatcivil) | Ligeo | Ligeo references; URL pattern; catalogued, searched 2026-10-05: parish and civil registers, censuses, military registers |
| 30 | Gard | `archives.gard.fr` | [`earchives.gard.fr/archives/classification-scheme`](https://earchives.gard.fr/archives/classification-scheme) | Bach | Observed (§4.11): parish and civil registers, TD, one finding aid per commune; `browser` transport (Anubis on the show pages and the viewer host), image names from the viewer links' ranges; live-checked 2026-10-06 through the browser; the nominal search of military registers (`/matricules/search`, a person index) is not a register collection |
| 31 | Haute-Garonne | `archives.haute-garonne.fr` | [`archives.haute-garonne.fr/archive/recherche/etatcivil/n:97`](https://archives.haute-garonne.fr/archive/recherche/etatcivil/n:97) | Ligeo | Ligeo references; URL pattern; observed (§4.5); catalogued, searched 2026-10-05: parish and civil registers, censuses, military registers |
| 32 | Gers | `www.archives32.fr` | [`www.archives32.fr/archives_numerisees/portail/etats_civils/ec/recherche/`](https://www.archives32.fr/archives_numerisees/portail/etats_civils/ec/recherche/) | Gers portal | Observed (§4.13); catalogued, searched and live-checked 2026-10-06: parish registers, civil status, decennial tables, censuses, succession tables |
| 33 | Gironde | `archives.gironde.fr` | [`archives.gironde.fr/archive/recherche/etatcivil/n:629`](https://archives.gironde.fr/archive/recherche/etatcivil/n:629) | Ligeo | Ligeo references; URL pattern; Bordeaux's civil status also published by the Archives Bordeaux Métropole; catalogued, searched 2026-10-05: parish and civil registers, censuses, succession tables; Bordeaux's registers by section searched by call number (2026-10-07) |
| 34 | Hérault | `archives-pierresvives.herault.fr` | [`archives-pierresvives.herault.fr/archive/recherche/etatcivil/n:23`](https://archives-pierresvives.herault.fr/archive/recherche/etatcivil/n:23) | Ligeo (portal); Mnesys customer | URL pattern; both vendors' lists; catalogued, searched 2026-10-05: parish and civil registers, censuses, succession tables; viewer asks the reader to accept a reuse licence, so `portal` |
| 35 | Ille-et-Vilaine | `archives.ille-et-vilaine.fr` | [`archives-en-ligne.ille-et-vilaine.fr/thot_internet/FrmSommaireFrame.asp`](https://archives-en-ligne.ille-et-vilaine.fr/thot_internet/FrmSommaireFrame.asp) | THOT | Observed (§4.9); catalogued 2026-10-06, each view by its ARK: registers of acts and tables, censuses, military registers, succession tables |
| 36 | Indre | `www.archives36.fr` | [`www.archives36.fr/fonds-numerises/etat-civil`](https://www.archives36.fr/fonds-numerises/etat-civil) | Arkothèque | Observed and live-checked (§4.3): registers and tables, RP, RM, CM, TSA |
| 37 | Indre-et-Loire | `archives.touraine.fr` | [`archives.touraine.fr/search/form/e9414896-40cc-4ec3-936c-8acdfdb11770`](https://archives.touraine.fr/search/form/e9414896-40cc-4ec3-936c-8acdfdb11770) | Mnesys Expo | Credits page; Naoned case study; catalogued (§4.4): registers, military registers, succession tables; live-checked 2026-10-05 |
| 38 | Isère | `archivesenligne.archives-isere.fr` | [`archivesenligne.archives-isere.fr/mdr/index.php/rechercheTheme/`](https://archivesenligne.archives-isere.fr/mdr/index.php/rechercheTheme/) | Arkothèque | Observed and live-checked (§4.3): registers and TD, RP, RM |
| 39 | Jura | `archives39.fr` | [`archives39.fr/search/form/1eb1f0a3-b7ba-4c8a-bdae-395b322800e4`](https://archives39.fr/search/form/1eb1f0a3-b7ba-4c8a-bdae-395b322800e4) | Mnesys Expo | Naoned customer list; URL pattern; catalogued (§4.4): registers, decennial tables, censuses, military registers; live-checked 2026-10-05 |
| 40 | Landes | `archives.landes.fr` | [`archives.landes.fr/faire-une-recherche/archives-numerisees/etat-civil`](https://archives.landes.fr/faire-une-recherche/archives-numerisees/etat-civil) | Arkothèque (since 2012) | Observed: Cloudflare refuses an automated browser's requests to the engine, even the page's own `fetch`; catalogued 2026-10-06 with its registers searched by page loads (§4.3, §11.3); the live check ends `challenged`; military registers not catalogued (§11.3) |
| 41 | Loir-et-Cher | `www.archives41.fr` | [`www.archives41.fr/archives/recherche/etatcivil`](https://www.archives41.fr/archives/recherche/etatcivil) | Ligeo Diffusion within the Culture 41 portal | Ligeo references; URL pattern; new site in 2025; catalogued, searched 2026-10-05: parish and civil registers, censuses, military registers, succession tables |
| 42 | Loire | `archives.loire.fr` | [`archives.loire.fr/archive/recherche/etatcivil/n:92`](https://archives.loire.fr/archive/recherche/etatcivil/n:92) | Ligeo (formerly Archinoë) | Press article; Ligeo references; URL pattern; catalogued, searched 2026-10-05: parish and civil registers, censuses, succession tables |
| 43 | Haute-Loire | `www.archives43.fr` | [`www.archives43.fr/archives-en-ligne/familles-et-individus-en-haute-loire/etat-civil-de-la-haute-loire`](https://www.archives43.fr/archives-en-ligne/familles-et-individus-en-haute-loire/etat-civil-de-la-haute-loire) | Arkothèque | Observed and live-checked (§4.3): registers, TD, RP, RM, TSA |
| 44 | Loire-Atlantique | `archives.loire-atlantique.fr/44/accueil-archives/j_6` | [`archives-numerisees.loire-atlantique.fr/chercher/etat-civil-et-registres-paroissiaux`](https://archives-numerisees.loire-atlantique.fr/chercher/etat-civil-et-registres-paroissiaux) | Arkothèque | Observed and live-checked (§4.3): registers and TD, RP, RM, CM, TSA |
| 45 | Loiret | `www.archives-loiret.fr` | [`www.archives-loiret.fr/faire-vos-recherches/archives-numerisees/etat-civil`](https://www.archives-loiret.fr/faire-vos-recherches/archives-numerisees/etat-civil) | Arkothèque | Observed and live-checked (§4.3): registers and TD, RP, RM, TSA |
| 46 | Lot | `archives.lot.fr` | [`archives.lot.fr/recherche-en-ligne/archives-numerisees/registres-paroissiaux-et-detat-civil`](https://archives.lot.fr/recherche-en-ligne/archives-numerisees/registres-paroissiaux-et-detat-civil) | Arkothèque (new version in 2026; finding aids by Anaphore) | Observed and live-checked (§4.3): registers and TD, RP, RM, TSA |
| 47 | Lot-et-Garonne | `archivesdepartementales.lotetgaronne.fr` | [`lotetgaronne.archives.prismia.fr/Recherche/Etat%20civil`](https://lotetgaronne.archives.prismia.fr/Recherche/Etat%20civil) | Prismia Vision (since 2025-03-11) | Department announcement; URL pattern; catalogued, searched 2026-10-05 |
| 48 | Lozère | `archives.lozere.fr` | [`archives.lozere.fr/archive/recherche/etatcivil/n:88`](https://archives.lozere.fr/archive/recherche/etatcivil/n:88) | Ligeo | Ligeo references; URL pattern; catalogued, searched 2026-10-05: parish and civil registers, censuses, military registers |
| 49 | Maine-et-Loire | `recherche-archives.maine-et-loire.fr` | [`recherche-archives.maine-et-loire.fr/rechercher-et-consulter/archives-consultables-en-ligne/etat-civil-et-registres-paroissiaux`](https://recherche-archives.maine-et-loire.fr/rechercher-et-consulter/archives-consultables-en-ligne/etat-civil-et-registres-paroissiaux) | Arkothèque (an Archinoë viewer exists) | Observed and live-checked (§4.3): registers and TD to 1902, RP, RM, TSA |
| 50 | Manche | `www.archives-manche.fr` | [`www.archives-manche.fr/recherche/registres-paroissiaux-et-detat-civil`](https://www.archives-manche.fr/recherche/registres-paroissiaux-et-detat-civil) | Arkothèque | Observed and live-checked (§4.3): registers and TD, RM, TSA |
| 51 | Marne | `archives.marne.fr` | [`archives.marne.fr/search/form/6977c5eb-072c-470c-8dfa-6d6488a2d71e`](https://archives.marne.fr/search/form/6977c5eb-072c-470c-8dfa-6d6488a2d71e) | Mnesys Expo | Mnesys Expo logo; URL pattern; catalogued (§4.4): registers, censuses, military registers; live-checked 2026-10-05 |
| 52 | Haute-Marne | `recherche.archives.haute-marne.fr` (new address since November 2025) | [`recherche.archives.haute-marne.fr/document/FRAD052_00000001E`](https://recherche.archives.haute-marne.fr/document/FRAD052_00000001E) | Bach | Observed (§4.11): registers (`1 E`), decennial tables (`164 M`), censuses (`158 M`), one finding aid each; live-checked 2026-10-06; the soldiers' person index is not a register collection |
| 53 | Mayenne | `archives.lamayenne.fr` | [`archives.lamayenne.fr/archives-en-ligne/etat-civil-search-form.html`](https://archives.lamayenne.fr/archives-en-ligne/etat-civil-search-form.html) | Pleade (Archinoë in 2014) | Observed (§4.10); catalogued 2026-10-06: registers of acts and tables (`form`), military registers (`tree`); no census or succession tables online |
| 54 | Meurthe-et-Moselle | `archivesenligne.meurthe-et-moselle.fr` | [`archivesenligne.meurthe-et-moselle.fr/archives-en-ligne/registres-paroissiaux-et-detat-civil`](https://archivesenligne.meurthe-et-moselle.fr/archives-en-ligne/registres-paroissiaux-et-detat-civil) | Arkothèque | Observed and live-checked (§4.3): registers and TD, RP, RM, TSA |
| 55 | Meuse | `archives.meuse.fr` | [`archives.meuse.fr/search/form/32239fba-c3ac-416c-b0f7-889cfa87214a`](https://archives.meuse.fr/search/form/32239fba-c3ac-416c-b0f7-889cfa87214a) | Mnesys Expo | Logo, Naoned case study; URL pattern; catalogued (§4.4): registers, censuses, military registers; live-checked 2026-10-05 |
| 56 | Morbihan | `patrimoines-archives.morbihan.fr` | [`rechercher.patrimoines-archives.morbihan.fr/archive/recherche/etatcivil/n:6`](https://rechercher.patrimoines-archives.morbihan.fr/archive/recherche/etatcivil/n:6) | Ligeo | Ligeo references; URL pattern; catalogued, searched 2026-10-05: parish and civil registers, military registers |
| 57 | Moselle | `www.archives57.com` | [`www.archives57.com/archives/fonds/FRAD057_605804`](https://www.archives57.com/archives/fonds/FRAD057_605804) | Ligeo | Ligeo references; URL pattern; searched within its finding aid (§4.5); catalogued, searched 2026-10-05: parish and civil registers; decennial tables (finding aid `FRAD057_698861`) list one notice per commune with no digitised object, viewer link or manifest (2026-10-07): not catalogued |
| 58 | Nièvre | `archives.nievre.fr` | [`archives.nievre.fr/search/form/9430efb3-399f-4de3-a3e7-004e232d8601`](https://archives.nievre.fr/search/form/9430efb3-399f-4de3-a3e7-004e232d8601) | Mnesys Expo | Naoned customer list; URL pattern; catalogued (§4.4): registers, censuses, military registers, succession tables; live-checked 2026-10-05 |
| 59 | Nord | `archivesdepartementales.lenord.fr` | [`archivesdepartementales.lenord.fr/search/form/dc4e871d-0b62-41fb-9921-5ded573781b8`](https://archivesdepartementales.lenord.fr/search/form/dc4e871d-0b62-41fb-9921-5ded573781b8) | Mnesys Expo | Naoned case study; URL pattern; catalogued (§4.4): registers, decennial tables, censuses, military registers and their tables, succession tables; live-checked 2026-10-05 |
| 60 | Oise | `archives.oise.fr` | [`ressources.archives.oise.fr/v2/ad60/registre.html`](https://ressources.archives.oise.fr/v2/ad60/registre.html) | Archinoë | URL pattern; catalogued, searched 2026-10-05; redirects any agent identifying itself, so its live check ends `challenged` (§9.1) |
| 61 | Orne | `archives.orne.fr` | [`archives.orne.fr/etat-civil`](https://archives.orne.fr/etat-civil), embedding [`gaia.orne.fr/mdr/index.php/rechercheTheme/requeteConstructor/1/1/R/0/0`](https://gaia.orne.fr/mdr/index.php/rechercheTheme/requeteConstructor/1/1/R/0/0) | GAIA 9 | Observed and live-checked (§4.8): parish registers, civil status, TD, RP, RM, TSA |
| 62 | Pas-de-Calais | `www.archivespasdecalais.fr` | [`archivesenligne.pasdecalais.fr/console/ir_seriel.php?id=56&p=formulaire_etat_civil`](https://archivesenligne.pasdecalais.fr/console/ir_seriel.php?id=56&p=formulaire_etat_civil) | Archinoë | 2014 panorama; URL pattern; catalogued, searched 2026-10-05 |
| 63 | Puy-de-Dôme | `www.archivesdepartementales.puy-de-dome.fr` | [`www.archivesdepartementales.puy-de-dome.fr/archive/recherche/etatcivil/n:13`](https://www.archivesdepartementales.puy-de-dome.fr/archive/recherche/etatcivil/n:13) | Ligeo Diffusion (since 2001) | Ligeo references; archive's own account; catalogued, searched 2026-10-05: parish and civil registers, military registers |
| 64 | Pyrénées-Atlantiques | `earchives.le64.fr` | [`earchives.le64.fr/archives-en-ligne/ead.html?id=FRAD064003_IR0002&c=FRAD064003_IR0002_e0000030&qid=`](https://earchives.le64.fr/archives-en-ligne/ead.html?id=FRAD064003_IR0002&c=FRAD064003_IR0002_e0000030&qid=) | Pleade | Observed (§4.10); catalogued 2026-10-06: registers of acts and tables (`tree`); the military registers' page answered an error, no other digitized series |
| 65 | Hautes-Pyrénées | `archivesenligne65.fr` | [`archivesenligne65.fr/archives/acces-thematique/naitre-vivre-et-mourir/les-registres-detat-civil`](https://archivesenligne65.fr/archives/acces-thematique/naitre-vivre-et-mourir/les-registres-detat-civil) | Arkothèque | Observed and live-checked (§4.3): civil status, parish registers, TD, RP, RM, TSA |
| 66 | Pyrénées-Orientales | `archives.cd66.fr` | [`archives.cd66.fr/mdr/index.php/rechercheTheme/requeteConstructor/1/1/R/0/0`](https://archives.cd66.fr/mdr/index.php/rechercheTheme/requeteConstructor/1/1/R/0/0) | GAIA 9 | Observed and live-checked (§4.8): parish registers, civil status, TD, RP, RM, TSA |
| 67 | Bas-Rhin (Archives d'Alsace) | `archives.alsace.eu` | [`archives67.alsace.eu/archive/resultats/etatcivil/n:128?type=etatcivil`](https://archives67.alsace.eu/archive/resultats/etatcivil/n:128?type=etatcivil) | Ligeo Diffusion (November 2025) | Collectivité européenne d'Alsace announcement; URL pattern; catalogued, searched 2026-10-05: parish and civil registers, censuses, succession tables |
| 68 | Haut-Rhin (Archives d'Alsace) | `archives.alsace.eu` | [`archives68.alsace.eu/search/form/f4ed0a71-fe36-42d4-81bd-780da14c2125`](https://archives68.alsace.eu/search/form/f4ed0a71-fe36-42d4-81bd-780da14c2125) | Mnesys Expo (since 2023-09-18) | Naoned case study; credits page; URL pattern; catalogued (§4.4): registers, censuses, military registers; live-checked 2026-10-05 |
| 69 | Rhône and Métropole de Lyon | `archives.rhone.fr` | [`archives.rhone.fr/search/form/dde23a8f-cc71-4300-9805-bda67eec1ac0`](https://archives.rhone.fr/search/form/dde23a8f-cc71-4300-9805-bda67eec1ac0) | Mnesys Expo | Naoned case study; URL pattern; catalogued (§4.4): registers, censuses, military registers, succession tables; live-checked 2026-10-05 |
| 70 | Haute-Saône | `archives.haute-saone.fr` | [`archives.haute-saone.fr/archive/recherche/etatcivil2/n:119`](https://archives.haute-saone.fr/archive/recherche/etatcivil2/n:119) | Ligeo Diffusion | Ligeo references; URL pattern; catalogued, searched 2026-10-05: parish registers, civil status, censuses, succession tables |
| 71 | Saône-et-Loire | `www.archives71.fr` | [`www.archives71.fr/consulter/en-ligne/familles-et-individus/etat-civil`](https://www.archives71.fr/consulter/en-ligne/familles-et-individus/etat-civil) | Arkothèque | Observed and live-checked (§4.3): registers, TD, RP, TSA |
| 72 | Sarthe | `archives.sarthe.fr` | [`archives.sarthe.fr/archives-en-ligne/registres-paroissiaux-etat-civil`](https://archives.sarthe.fr/archives-en-ligne/registres-paroissiaux-etat-civil) | Arkothèque | Observed and live-checked (§4.3): registers and TD (two engines), RP, RM, CM, TSA |
| 73 | Savoie | `recherche-archives.savoie.fr` | [`recherche-archives.savoie.fr/?id=recherche_guidee_etat_civil_web`](https://recherche-archives.savoie.fr/?id=recherche_guidee_etat_civil_web) | Mnesys (older interface) | Mnesys Expo logo; URL pattern; observed (§4.14); catalogued, searched and live-checked 2026-10-06: registers, censuses, military registers (the viewer host refuses the check's identified browser: step 4 unverified) |
| 74 | Haute-Savoie | `archives.hautesavoie.fr` | [`archives.hautesavoie.fr/archive/recherche/etatcivil/n:139`](https://archives.hautesavoie.fr/archive/recherche/etatcivil/n:139) | Ligeo | Ligeo references; URL pattern; catalogued, searched 2026-10-05: parish and civil registers, censuses, military registers, succession tables |
| 75 | Paris | `archives.paris.fr` | [`archives.paris.fr/archives-numerisees/etat-civil-de-paris/etat-civil-reconstitue-xvie-1859`](https://archives.paris.fr/archives-numerisees/etat-civil-de-paris/etat-civil-reconstitue-xvie-1859) (reconstituted, 16th century–1859), [`archives.paris.fr/archives-numerisees/etat-civil-de-paris/etat-civil-a-partir-de-1860`](https://archives.paris.fr/archives-numerisees/etat-civil-de-paris/etat-civil-a-partir-de-1860) and [`archives.paris.fr/archives-numerisees/sources-genealogiques-complementaires/cimetieres-et-pompes-funebres/cimetieres/consulter-les-registres-journaliers-dinhumation`](https://archives.paris.fr/archives-numerisees/sources-genealogiques-complementaires/cimetieres-et-pompes-funebres/cimetieres/consulter-les-registres-journaliers-dinhumation) (cemeteries' daily burial registers) | Arkothèque | Observed and live-checked (§4.3): civil status from 1860, RI |
| 76 | Seine-Maritime | `www.archivesdepartementales76.net` | [`www.archivesdepartementales76.net/archive/resultats/etatcivil/n:113?type=etatcivil`](https://www.archivesdepartementales76.net/archive/resultats/etatcivil/n:113?type=etatcivil) | Ligeo | Ligeo references; URL pattern; catalogued, searched 2026-10-05: parish and civil registers, military registers, succession tables |
| 77 | Seine-et-Marne | `archives.seine-et-marne.fr` | [`archives.seine-et-marne.fr/fr/etat-civil`](https://archives.seine-et-marne.fr/fr/etat-civil), linking [`archives-en-ligne.seine-et-marne.fr/mdr/index.php/rechercheTheme/requeteConstructor/14/1/R/0/0`](https://archives-en-ligne.seine-et-marne.fr/mdr/index.php/rechercheTheme/requeteConstructor/14/1/R/0/0) | GAIA 9 | Observed and live-checked (§4.8): registers and TD, RP, RM, CM, TSA |
| 78 | Yvelines | `archives.yvelines.fr` | [`archives.yvelines.fr/rechercher/archives-en-ligne/registres-paroissiaux-et-detat-civil/registres-paroissiaux-et-detat-civil`](https://archives.yvelines.fr/rechercher/archives-en-ligne/registres-paroissiaux-et-detat-civil/registres-paroissiaux-et-detat-civil) | Arkothèque | Observed and live-checked (§4.3): registers and TD, RP, TSA |
| 79 | Deux-Sèvres | `archives-deux-sevres-vienne.fr` (shared with Vienne) | [`archives-deux-sevres-vienne.fr/archive/resultats/etatcivil/n:100?type=etatcivil`](https://archives-deux-sevres-vienne.fr/archive/resultats/etatcivil/n:100?type=etatcivil) | Ligeo (Archinoë in 2014) | URL pattern; catalogued, searched 2026-10-05: parish registers, civil status, censuses, succession tables |
| 80 | Somme | `archives.somme.fr` | [`archives.somme.fr/search/form/cebd4a00-25b1-4b1c-a31e-8ea66d58efa2`](https://archives.somme.fr/search/form/cebd4a00-25b1-4b1c-a31e-8ea66d58efa2) | Mnesys Expo | Naoned customer list; URL pattern; catalogued (§4.4): registers, censuses, military-register tables, conscription lists; live-checked 2026-10-05 |
| 81 | Tarn | `archives.tarn.fr` | [`recherche-archives.tarn.fr/archives/classification-scheme#tt2-39`](https://recherche-archives.tarn.fr/archives/classification-scheme#tt2-39) | Bach | Observed (§4.11): parish and civil registers, TD, one finding aid per commune; the reformed churches' registers; the viewer, out of service on 2026-10-05, answered again on 2026-10-06; live-checked 2026-10-06; the military registers (`1 R`) show no images, and the tables of successions are split by surname initial, which no citation names: not catalogued |
| 82 | Tarn-et-Garonne | `recherche.archives82.fr` | [`recherche.archives82.fr/document/FRAD082_IR_01051`](https://recherche.archives82.fr/document/FRAD082_IR_01051) and [`recherche.archives82.fr/document/FRAD082_IR_00196`](https://recherche.archives82.fr/document/FRAD082_IR_00196), two finding aids | Bach (Archinoë in 2014) | Observed (§4.11): parish and civil registers, TD, one finding aid per commune; the reformed churches' registers; censuses (`6 M`); military registers (`1 R`); live-checked 2026-10-06; Montauban's censuses, listed by street beside the whole list, are not catalogued |
| 83 | Var | `archives.var.fr` | [`archives.var.fr/rechercher-dans-les-archives-numerisees-et-les-inventaires-5/registres-paroissiaux-et-de-letat-civil`](https://archives.var.fr/rechercher-dans-les-archives-numerisees-et-les-inventaires-5/registres-paroissiaux-et-de-letat-civil) | Arkothèque | Observed and live-checked (§4.3): registers and tables, TD, RP, RM and CM, TSA |
| 84 | Vaucluse | `earchives.vaucluse.fr` | [`earchives.vaucluse.fr/archives/classification-scheme`](https://earchives.vaucluse.fr/archives/classification-scheme) | Bach | Observed (§4.11): parish and civil registers, publications of banns, TD, one finding aid per commune, without call numbers; live-checked 2026-10-06; the departmental collection (`IR0001730`) shows no images |
| 85 | Vendée | `etatcivil-archives.vendee.fr` | [`etatcivil-archives.vendee.fr/consulter/etat-civil-et-recensements/etat-civil`](https://etatcivil-archives.vendee.fr/consulter/etat-civil-et-recensements/etat-civil) | Arkothèque (Ligeo for management) | Observed and live-checked (§4.3): civil status and TD, parish registers, RP |
| 86 | Vienne | `archives-deux-sevres-vienne.fr` (shared with Deux-Sèvres) | [`archives-deux-sevres-vienne.fr/archive/resultats/etatcivil/n:100?type=etatcivil`](https://archives-deux-sevres-vienne.fr/archive/resultats/etatcivil/n:100?type=etatcivil) | Ligeo (Archinoë in 2014) | Shared portal; URL pattern; catalogued, searched 2026-10-05: parish registers, civil status, censuses, succession tables |
| 87 | Haute-Vienne | `archives.haute-vienne.fr` | [`archives.haute-vienne.fr/rechercher/archives-en-ligne/etat-civil`](https://archives.haute-vienne.fr/rechercher/archives-en-ligne/etat-civil) | Arkothèque | Observed and live-checked (§4.3): registers and TD, RP, RM, CM, TSA |
| 88 | Vosges | `recherche-archives.vosges.fr` | [`recherche-archives.vosges.fr/archive/recherche/etatcivil/n:2`](https://recherche-archives.vosges.fr/archive/recherche/etatcivil/n:2) | Ligeo | Ligeo references; URL pattern; catalogued, searched 2026-10-05: parish registers, civil status, censuses |
| 89 | Yonne | `archives.yonne.fr` | [`archives.yonne.fr/archive/recherche/etatcivil/n:157`](https://archives.yonne.fr/archive/recherche/etatcivil/n:157) | Ligeo | Ligeo references; URL pattern; catalogued, searched 2026-10-05: parish and civil registers |
| 90 | Territoire de Belfort | `archives.territoiredebelfort.fr` | [`archives.territoiredebelfort.fr/search/form/ce8c3b7f-77f9-493b-81dd-d09d11bdc431`](https://archives.territoiredebelfort.fr/search/form/ce8c3b7f-77f9-493b-81dd-d09d11bdc431) | Mnesys Expo | Naoned customer list; URL pattern; catalogued (§4.4): parish registers, civil status, censuses, military registers, succession tables; live-checked 2026-10-05 |
| 91 | Essonne | `archives.essonne.fr` | [`archives.essonne.fr/search/form/f282515f-9106-4c17-a633-95f49fe7da66`](https://archives.essonne.fr/search/form/f282515f-9106-4c17-a633-95f49fe7da66) | Mnesys Expo | Mnesys Expo logo; URL pattern; catalogued (§4.4): registers, succession tables; live-checked 2026-10-05 |
| 92 | Hauts-de-Seine | `archives.hauts-de-seine.fr` | [`archives.hauts-de-seine.fr/n/archives-en-ligne/n:89`](https://archives.hauts-de-seine.fr/n/archives-en-ligne/n:89) | Ligeo | Ligeo references; URL pattern; catalogued, searched 2026-10-05: parish registers, civil status, censuses, conscription lists, succession tables |
| 93 | Seine-Saint-Denis | `archives.seinesaintdenis.fr` | [`archives.seinesaintdenis.fr/archive/resultats/etatcivil/n:217?type=etatcivil`](https://archives.seinesaintdenis.fr/archive/resultats/etatcivil/n:217?type=etatcivil) | Ligeo (portal); Mnesys customer | URL pattern; both vendors' lists; catalogued, searched 2026-10-05: civil status, censuses, succession tables |
| 94 | Val-de-Marne | `archives.valdemarne.fr` | [`archives.valdemarne.fr/recherches/archives-en-ligne/etat-civil`](https://archives.valdemarne.fr/recherches/archives-en-ligne/etat-civil) | Arkothèque | Observed and live-checked (§4.3): registers, RP, TSA |
| 95 | Val-d'Oise | `archives.valdoise.fr` | [`archives.valdoise.fr/archive/recherche/EtatCivilNumerise/n:419`](https://archives.valdoise.fr/archive/recherche/EtatCivilNumerise/n:419) | Ligeo | Ligeo references; URL pattern; catalogued, searched 2026-10-05: parish registers, civil status, censuses; viewer asks the reader to accept a reuse licence, so `portal` |
| 971 | Guadeloupe | `www.archivesguadeloupe.fr` | [`earchives.archivesguadeloupe.fr/document/FRAD971_1E`](https://earchives.archivesguadeloupe.fr/document/FRAD971_1E) | Bach; listed by Ligeo | Observed (§4.11): civil status (`1 E`, the courts' copies) with tables and annual indexes, military registers (`1 R`); live-checked 2026-10-06; `4 F` and the registration series (`3 Q`) show no images |
| 972 | Martinique (Archives territoriales) | `www.patrimoines-martinique.org` | [`www.patrimoines-martinique.org/search/form/8ea80f22-2f9c-456b-94a0-adbd50d31e1c`](https://www.patrimoines-martinique.org/search/form/8ea80f22-2f9c-456b-94a0-adbd50d31e1c) | Mnesys Expo | Naoned case study; URL pattern; catalogued (§4.4): registers, military registers; live-checked 2026-10-05 |
| 973 | Guyane (Archives territoriales) | `ctguyane.fr` | [`anom.archivesnationales.culture.gouv.fr/caomec2/recherche.php?territoire=GUYANE`](http://anom.archivesnationales.culture.gouv.fr/caomec2/recherche.php?territoire=GUYANE) | Archives nationales d'outre-mer | Collectivité territoriale research guide; portal; observed (§4.15), catalogued and live-checked 2026-10-06: births, marriages and deaths of the national copy, over plain `http` (§3.1) |
| 974 | La Réunion | `departement974.fr` (directory) | [`anom.archivesnationales.culture.gouv.fr/caomec2/recherche.php?territoire=REUNION`](http://anom.archivesnationales.culture.gouv.fr/caomec2/recherche.php?territoire=REUNION) | Archives nationales d'outre-mer | Portal; observed (§4.15), catalogued and live-checked 2026-10-06: births, marriages and deaths of the national copy, over plain `http` (§3.1) |
| 976 | Mayotte | — | [`anom.archivesnationales.culture.gouv.fr/caomec2/recherche.php?territoire=MAYOTTE`](http://anom.archivesnationales.culture.gouv.fr/caomec2/recherche.php?territoire=MAYOTTE) (1844–1906) | Archives nationales d'outre-mer | Portal; observed (§4.15), catalogued and live-checked 2026-10-06: births, marriages and deaths of the national copy, over plain `http` (§3.1) |

Corsica has two department codes and one service, so the table has 100 rows
for 101 departments. *(directory)* marks a domain quoted by a third-party
directory and not checked on the site itself.

### 11.6 Parish-register entry points

Parish registers often have a search page of their own beside the civil
status, even on the same platform, which is why an archive has several
collections (§3.1). The ones supplied so far:

| No. | Department | Parish-register entry point | Portal platform | Relation to the civil-status page |
|---|---|---|---|---|
| 04 | Alpes-de-Haute-Provence | [`www.archives04.fr/rechercher/archives-en-ligne/etat-civil/actes-paroissiaux`](https://www.archives04.fr/rechercher/archives-en-ligne/etat-civil/actes-paroissiaux) | Arkothèque | Same portal; `actes-paroissiaux` beside `actes-etat-civil` |
| 07 | Ardèche | [`archives.ardeche.fr/archive/recherche/paroissiaux/n:164`](https://archives.ardeche.fr/archive/recherche/paroissiaux/n:164) | Ligeo | Same portal; search `paroissiaux` beside `etatcivil` |
| 32 | Gers | [`www.archives32.fr/archives_numerisees/portail/etats_civils/rp/recherche/`](https://www.archives32.fr/archives_numerisees/portail/etats_civils/rp/recherche/) | Gers portal | Same portal; `/rp/` beside `/ec/` (§4.13) |
| 65 | Hautes-Pyrénées | [`archivesenligne65.fr/archives/acces-thematique/naitre-vivre-et-mourir/les-registres-paroissiaux`](https://archivesenligne65.fr/archives/acces-thematique/naitre-vivre-et-mourir/les-registres-paroissiaux) | Arkothèque | Same portal; `les-registres-paroissiaux` beside `les-registres-detat-civil` |
| 92 | Hauts-de-Seine | [`archives.hauts-de-seine.fr/archive/resultats/registresparoissiaux/n:93?type=registresparoissiaux`](https://archives.hauts-de-seine.fr/archive/resultats/registresparoissiaux/n:93?type=registresparoissiaux) | Ligeo | Same portal; search `registresparoissiaux` |

[^arkotheque]: Arkothèque, publishing software for archive services.
[^arkotheque-references]: Arkothèque references, departmental archive count.
[^naoned]: Naoned, Mnesys.
[^ad44-portal]: Loire-Atlantique archive portal, search and viewer requests observed 2026-10-03.
[^ad72-portal]: Sarthe archive portal, search, viewer and anti-bot challenge observed 2026-10-03.
[^ad37-portal]: Indre-et-Loire archive portal, search form, results, viewer and manifest observed 2026-10-03.
[^thot-portals]: The THOT portals of Ille-et-Vilaine and Corsica: session, search forms of every module, results, viewer, slide files and ARK resolver observed 2026-10-05 and 2026-10-06.
[^pleade-portals]: The Pleade portals of Mayenne and Pyrénées-Atlantiques: search form, results fragment, finding aids' tables of contents and components, Mirador viewers and manifests observed 2026-10-05 and 2026-10-06.
[^ligeo-portals]: The Ligeo departmental portals of §11.5: search pages, results, viewer, manifest, image service and reuse terms observed 2026-10-05, with live checks of every catalogued collection.
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
[^bach-portals]: The Bach portals of §11.5: classification schemes, finding aids, show pages, the viewer's image lists and `robots.txt` observed 2026-10-05 and 2026-10-06.
