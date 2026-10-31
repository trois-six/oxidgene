---
type: "API Specification"
title: "API Contract"
description: "REST and GraphQL contract for OxidGene, including endpoints, pagination, and payload conventions."
tags: [oxidgene, specification, api, contract]
generated: { by: human:maintainer, at: 2026-10-31T12:00:00Z }
---


# API Contract

> Part of the [OxidGene Specifications](index.md).
> See also: [Data Model](data-model.md) · [Cross-cutting Rules](cross-cutting.md) ·
> [Architecture](architecture.md)

---

## 1. Contract Conventions

### Surfaces and parity

OxidGene has one product API exposed through two transports. REST uses
`/api/v1`; GraphQL uses `/graphql`. Every product operation must have both a
REST mapping and a GraphQL mapping. A feature is complete only when both
surfaces provide the same capabilities, validation, authorization, domain
errors, update behavior, projection refresh, and integration-test coverage.

REST-only and GraphQL-only product operations are not part of the accepted
contract. A temporary implementation gap is a defect to close, not an API
exception to document. Changes to an operation update both mappings, their
tests, and this specification in the same change. The mapping is declared
operation by operation in the parity table of
`crates/oxidgene-api/tests/guards/api_contract.rs`: a guard fails when a
route or a root GraphQL field is missing from it, when it names one that no
longer exists, and when a route of the router has no row in §2 — or a row of
§2 no route.

The [Assistant Access (MCP)](mcp.md) server is an adapter over a
curated subset of these operations, not a third mirror. It never offers an
operation that REST and GraphQL lack, and it reuses their validation, errors,
and REST JSON representation.

Transport-specific representation differences are allowed where the protocol
requires them. Binary file uploads and downloads use streaming HTTP endpoints;
GraphQL does not duplicate those payloads as base64. GraphQL exposes the same
durable job status and can create export jobs, while clients use REST to upload
import sources and download export artifacts. Pagination envelopes follow each
transport's conventions.

Direct media reads (`/file`, `/download`, `/archive`, `/thumbnail`, and vignette `/image`)
remain HTTP representations because their cache validators, content types,
download disposition and conditional `ETag` semantics are HTTP behaviour rather
than product operations. GraphQL exposes the underlying metadata and checked
attachment URLs through `mediaDownload` and `mediaArchive`. Geneanet
session archives retain their existing base64 representation because they are
desktop session handoffs rather than genealogy import or export artifacts.

### Stability and versioning

- `/api/v1` is the current REST compatibility boundary.
- GraphQL evolves additively where possible.
- Projection schema versions are internal storage metadata, not API versions;
  see [Data Model §4](data-model.md).
- Removed names and endpoints are not retained as dead aliases unless a
  documented compatibility window requires them.
- Deprecation requires a replacement, migration note, tests during the
  compatibility window, and a planned removal milestone.

### Representation

- REST JSON uses `snake_case`; GraphQL uses `camelCase`.
- UUID v7 identifiers are serialized as opaque strings.
- Timestamps are RFC 3339 UTC strings.
- Enums use stable English technical values and are localized only by clients.
- Tree-scoped IDs from another tree return `not_found` rather than disclosing
  the existence of another tree's resource.
- A tree-scoped operation naming a tree that does not exist or has been
  deleted returns `not_found` on both surfaces, reads and writes alike: a
  deleted tree's rows remain until the background purge removes them, and
  nothing reads or writes them meanwhile. REST checks this once per request
  for the whole `/trees/{tree_id}/…` nest; GraphQL at the start of every
  resolver taking a `treeId`.
- A malformed identifier is a `validation_error`.
- Soft-deleted records are excluded by default.
- User and imported content is returned verbatim and never translated.
- REST and GraphQL responses are gzip-compressed alike for a client that
  accepts it. Pictures other than SVG, archives, PDFs, video, sound and raw
  `application/octet-stream` downloads are sent as they are: they are packed
  already or opaque, and compressing them again only costs time.

### Authentication and privacy

Authentication and authorization are not implemented in the current MVP.
Privacy values are stored but not enforced, so clients must not claim that
private records are hidden. Future authorization uses the same domain checks
for REST and GraphQL. Until that work is complete, the API is local or private
infrastructure only and must not be published directly to an untrusted network.
Server defaults, container publication, CORS, and UI media rendering follow
[Cross-cutting Rules §7.1](cross-cutting.md#71-backend-exposure-before-authentication).
The standalone server's CORS policy lets the browser keep a preflight answer
for two hours, so the `POST` reads (pedigree batches, a page's pictures) are
not preceded by a preflight on every page.

Direct HTTP media representations remain API-client transports, not browser
navigation destinations. Frontends fetch their bytes through the typed client
and render a local `data:`/`blob:` resource; they never emit `/api/v1`,
`/graphql`, or an API origin in user-visible links, image sources, form actions,
redirects, or new-window targets. The one exception is the API section of
[App Settings](ui-app-settings.md), whose purpose is to show the endpoints: it
links the OpenAPI document and, in the web build, the GraphiQL page, which
carry no tree data.

### Errors and consistency

Error envelopes, stable codes, safe messages, request IDs, logging, and
anonymization follow [Cross-cutting Rules §4–5](cross-cutting.md). Neither
surface returns stack traces, SQL, filesystem paths, credentials, or genealogy
in error details. A REST request refused before any handler runs — an
identifier in the path that is not a UUID, a body that is not valid JSON or
lacks a required field, a missing JSON content type, a body over the route's
limit, a route that does not exist — receives the same envelope:
`400 validation_error`, `415 unsupported_media_type`, `413 payload_too_large`
or `404 not_found`. The standalone server also answers `503 timeout` when a
request outlives its time limit (five minutes, an hour for an upload); file
uploads on both surfaces wait for one of two intake slots first
([Cross-cutting Rules §7.1](cross-cutting.md#71-backend-exposure-before-authentication)).

Mutations refresh affected projections in the same database transaction as the
normalized write. A successful response guarantees read-after-write
consistency for profiles, pedigrees, and search. Import operations may report
partial media warnings only where their endpoint contract says so.

### Machine-readable schema

The REST API exposes its OpenAPI 3.1 description:

| Method | Path | Description |
|---|---|---|
| `GET` | `/openapi.json` | The OpenAPI 3.1 document of the REST surface; open, even where the rest of the API takes a credential |

The build script generates the document from the
Axum router AST on every API build, including nested and merged routers, so its
paths, HTTP methods, operation identifiers, and path parameters track the
compiled REST surface. Each handler's extractors are read from the sources too:
path parameters are typed from its `Path<…>` (a UUID, an integer, or the values
of an enum such as `{record_type}` or `{field}`; `{lang}` lists the interface
languages), query parameters come from the fields of its `Query<…>` type, and
the request body from its `Json<…>` type, described once under
`components.schemas` with the serde names, defaults and flattening applied, or
as `multipart/form-data` or raw bytes. Response bodies are not described: this
specification is their reference. The document also defines the shared error
envelope.

GraphQL uses its executable schema and standard introspection instead of a
separate OpenAPI description. The `graphql` feature is enabled in the
standalone server only; the desktop's embedded server and the background
worker do not compile it, so the desktop serves REST only. GraphiQL is served
at `GET /graphql` only where the deployment enables it (`OXIDGENE_GRAPHIQL`,
off by default): the page loads its scripts from a public CDN. Elsewhere
`GET /graphql` is an unknown route (`404 not_found`).

### Connecting a client

| Build | Base URL | Credential |
|---|---|---|
| Standalone server | The deployment's API origin (`OXIDGENE_HOST`/`OXIDGENE_PORT`, loopback `127.0.0.1:8080` by default) | None. A request whose `Origin` is present and not the frontend's cannot write (`403 forbidden`); `curl` and scripts send none. The `Host` must be a loopback name, the frontend origin's host or one of `OXIDGENE_ALLOWED_HOSTS` (`403 forbidden`). |
| Desktop (REST only) | `http://127.0.0.1:<port>`, a port the operating system picks at each launch; loopback only | `Authorization: Bearer <token>`, a token generated at each launch, on every request except `/api/v1/openapi.json`; otherwise `401 unauthenticated`. The `Host` must be a loopback name (`403 forbidden`). |

[App Settings §10](ui-app-settings.md#10-section-api) shows the current build's URLs, the
desktop token, and `curl` examples for the surfaces that build serves.

## 2. REST API

Base path: `/api/v1`.

### Trees

| Method | Path | Description |
|---|---|---|
| `GET` | `/trees` | List trees (cursor-paginated). List nodes add transient `import_in_progress: bool`, `import_job_id: UUID?` and `import_cancelling: bool` fields from the job queue (GraphQL: `importInProgress`, `importJobId` and `importCancelling` on every `Tree`); these are not persisted tree fields and disappear once the job completes, fails or is cancelled. `import_cancelling` is true while the import was asked to stop and is undoing what it wrote |
| `POST` | `/trees` | Create a tree; a blank `name` is a `validation_error` |
| `GET` | `/trees/{tree_id}` | Get a tree |
| `PUT` | `/trees/{tree_id}` | Update a tree (incl. `sosa_root_person_id` and `self_person_id`, which must name persons of the tree — another tree's person is `not_found`); a blank `name` is a `validation_error` |
| `DELETE` | `/trees/{tree_id}` | Soft-delete a tree |
| `POST` | `/trees/{tree_id}/duplicate` | Duplicate a tree (deep copy) |
| `GET` | `/trees/recent-persons?tree_ids=ID,ID&limit=N` | The persons modified most recently in each of up to 100 trees (a page of the tree list), more being a `validation_error`, `[{tree_id, persons}]` in the order asked, `persons` being `SearchEntry` rows as for one tree's `recently-modified`; a missing or deleted tree is left out, an id that is not a UUID is a `validation_error`. GraphQL: `recentPersonsOfTrees(treeIds, limit)` |

Used by: [Homepage](ui-home.md) (tree list, create, duplicate, delete, each
card's recent persons in one request)

### Persons

| Method | Path | Description |
|---|---|---|
| `GET` | `/trees/{tree_id}/persons?search=` | List persons (cursor-paginated); `search` keeps those with a given name, surname or nickname containing it, ignoring case (GraphQL `persons(search:)`) |
| `POST` | `/trees/{tree_id}/persons` | Create a person |
| `GET` | `/trees/{tree_id}/persons/search` | Server-side person search with structured filters, sorting, and offset pagination (see below) |
| `GET` | `/trees/{tree_id}/persons/recently-modified?limit=` | The persons modified most recently, newest first, as `SearchEntry` rows (see below) |
| `GET` | `/trees/{tree_id}/persons/sosa/{number}` | Resolve a SOSA number to a person (relative to `Tree.sosa_root_person_id`) |
| `GET` | `/trees/{tree_id}/persons/{person_id}` | Get a person (with names, events, families) |
| `GET` | `/trees/{tree_id}/persons/{person_id}/detail-bundle` | Load the bounded read model for the person profile. Beside the cited `sources`, `source_holdings` (`{source_id, name, call_number, website}`, one per repository link) and `source_links` (`{source_id, url}`, the addresses of the media linked to each source, a linked document's pages included) are what the profile recognizes archive citations from ([Archive Portals §5.1](archives.md#51-citation-recognition)). `event_vignettes` are the regions cut as evidence for one of `events` whose page and document are live, each a vignette with `document_id`, the document grouping its page (`null` for a page of its own); their pictures are in `gallery.vignettes`. GraphQL: `personDetailBundle(treeId, personId)`, with `sourceHoldings`, `sourceLinks` and `eventVignettes { vignette documentId }` |
| `PUT` | `/trees/{tree_id}/persons/{person_id}` | Update a person |
| `DELETE` | `/trees/{tree_id}/persons/{person_id}` | Soft-delete a person |
| `GET` | `/trees/{tree_id}/persons/{person_id}/homonyms` | List the other persons bearing the same name, as `SearchEntry` rows (see below) |
| `POST` | `/trees/{tree_id}/persons/{person_id}/distinct` | Record that the person differs from `{person_ids}`; `204` |
| `POST` | `/trees/{tree_id}/persons/{person_id}/merge` | Merge `{duplicate_id, choices?}` into the path's person, which is kept; returns the kept `Person` |
| `GET` | `/trees/{tree_id}/persons/{person_id}/ancestors?max_depth=` | Get ancestors as `{ person_id, depth }`, each at its shortest distance; `max_depth` is 1–64 generations, 64 by default, `validation_error` otherwise |
| `GET` | `/trees/{tree_id}/persons/{person_id}/descendants?max_depth=` | Get descendants, likewise |
| `GET` | `/trees/{tree_id}/persons/{person_id}/kinship/{other_person_id}` | Every way found to go from the person to the other: blood relationships, or the shortest paths through unions (see below) |
| `POST` | `/trees/{tree_id}/relation-labels` | Load names and spouse links for bounded `person_ids` and `family_ids` sets |

The detail bundle contains the person, their direct parental and conjugal
families, and parents' other unions needed to represent half-siblings. It
includes names and timeline events for that neighborhood, only the places
referenced by those events, citations attached to the requested person or an
included event, and only the sources referenced by those citations. It also
contains media attached directly to the person or their conjugal families,
vignettes identifying the person, event media links, and one display-ready
gallery bundle for those bounded sets. It never expands these collections to
all records in the tree. It carries the person's own `portrait` (an image
source and its crop, as a pedigree node's) so the page resolves it with the
gallery's pictures in one request, and `sosa_ancestor_ids`: those of its
persons who are the tree's SOSA root or one of its ancestors, for the marks of
the family narrative, so the page never loads the whole ancestry; they and
`sosa_number` come from the stored SOSA ancestry
([Data Model §4.8](data-model.md)).

Each profile media tile carries `family_id` (`familyId` in GraphQL): the
conjugal family the media reaches the profile through, or `null` when it is
attached to the person directly. A media attached both ways appears once, as
the person's own. The [Couple Profile](ui-couple-profile.md) relies on it to
show a spouse's own media in their column and the couple's media once, across
both.

**Homonyms.** Two persons are homonyms when their primary surnames and their
primary given names are equal once folded (lowercase, accents removed) — the
normalized columns of the search projection. A person missing either half has
none. The list is ordered by birth date, then display name, holds at most 100
rows, and leaves out every person already confirmed distinct. Confirmations
are symmetric pairs, recording one twice is a no-op, and a request naming the
person themselves, no one, more than 100 persons, or a person of another tree
is rejected (`validation_error`, `not_found`).

**Recently modified.** A person is modified when a write about them — the
subject of its entry in the [audit log](data-model.md#5-change-history) — is
recorded: a change to their record, names, events, notes, citations or unions,
a merge into them, a restore. Imports and exports are about the tree and are
left out, and so are persons deleted since. Each person appears once, at their
latest such write, newest first. `limit` defaults to 5 and is capped at 50. A
tree that does not exist is `not_found`. GraphQL:
`recentlyModifiedPersons(treeId, limit)`.

**Merge.** `POST …/{person_id}/merge` keeps `person_id` and soft-deletes
`duplicate_id` after moving everything the duplicate carried onto the kept
person, following the rules of [Data Model §1 Person merge](data-model.md#person-merge).
It is refused with `validation_error` when both IDs are the same person, when
they are spouses of the same family, or when one is an ancestor of the other.
The projections of both persons' relatives are rebuilt in the same transaction,
and the duplicate's projection and search row are removed.

`choices` (optional; every field defaults to empty or `false`) carries what
the comparison of the [merge wizard](ui-merge.md) chose:

| Field | Effect |
|---|---|
| `left_out_events` | Own events (`person_id`) of either person left out of the merged record, soft-deleted: the duplicate's events not taken, or the kept person's birth when the duplicate's is the one chosen |
| `left_out_media_links` | The duplicate's direct media links not taken, removed; the media stay in the library |
| `surname_from_duplicate` | The merged primary name takes the duplicate's surname, particle included |
| `given_names_from_duplicate` | The merged primary name takes the duplicate's given names |
| `sex_from_duplicate` | The merged record takes the duplicate's sex |

Left-out items are dropped in the same transaction, before the rest moves,
and the audit entry names the events left out. An event that is neither
person's own, or a media link that does not link the duplicate directly, is
refused with `validation_error`, so a merge never drops someone else's record.
A primary name composed from the duplicate's pieces becomes the kept person's
primary name, the former one staying as a secondary name; a name the person
already bears is promoted rather than written twice. GraphQL's `mergePersons`
takes the same fields as a `MergeChoicesInput` (`leftOutEvents`,
`leftOutMediaLinks`, `surnameFromDuplicate`, `givenNamesFromDuplicate`,
`sexFromDuplicate`).

**Kinship.** `GET …/{person_id}/kinship/{other_person_id}` returns a
`Kinship`: `from_person_id`, `to_person_id`, `paths`, `truncated`, and
`persons` — a `SearchEntry` for every person the paths name, the two ends
first. Each path is a list of `segments`, and each segment climbs to its
`ancestor_ids` and comes back down: `from_line` and `to_line` list the persons
from the generation just below the ancestors down to the segment's first and
last person, and an empty line means that end is the ancestor itself.
`ancestor_ids` holds one person, or both spouses of the `family_id` both lines
descend from; `half` marks two lines descending from one ancestor through
different unions; `union_family_id` names the union joining a segment's first
person to the previous segment's last (`null` on the first).

Blood relationships are listed when there is at least one, as a single
segment each: pairs of lines from a common ancestor that share nobody but that
ancestor, closest first, so pedigree implex yields one path per distinct pair.
Only when the persons share no ancestor are the shortest paths through unions
listed, fewest unions first. At most 32 paths are returned, with `truncated`
set when more exist. Deleted persons and families link nobody. Both persons
must belong to the tree (`not_found`) and differ (`validation_error`).

Relation-label requests accept at most 1,024 combined person and family IDs.
Clients split larger logical sets into consecutive requests. Results are
strictly scoped to active people and families in the requested tree.

Used by: [Tree View](ui-genealogy-tree.md) (pedigree chart) · [Person Edit Modal](ui-person-edit-modal.md) (edit/delete) · [Kinship](ui-kinship.md) · [Homepage](ui-home.md) (recently modified persons)

### Person Names

| Method | Path | Description |
|---|---|---|
| `GET` | `/trees/{tree_id}/persons/{person_id}/names` | List names |
| `POST` | `/trees/{tree_id}/persons/{person_id}/names` | Add a name |
| `PUT` | `/trees/{tree_id}/persons/{person_id}/names/{name_id}` | Update a name; a name of another person is `not_found` |
| `DELETE` | `/trees/{tree_id}/persons/{person_id}/names/{name_id}` | Delete a name; a name of another person is `not_found` |

### Families

| Method | Path | Description |
|---|---|---|
| `GET` | `/trees/{tree_id}/families` | List families (cursor-paginated) |
| `POST` | `/trees/{tree_id}/families` | Create a family |
| `GET` | `/trees/{tree_id}/families/{family_id}` | Get a family record; its spouses and children come from the member endpoints below |
| `PUT` | `/trees/{tree_id}/families/{family_id}` | Update a family (`{ "privacy": … }`). The body is optional — an empty one only touches `updated_at` — but a body that is there must be a valid update, `validation_error` otherwise |
| `DELETE` | `/trees/{tree_id}/families/{family_id}` | Soft-delete a family |
| `GET` | `/trees/{tree_id}/families/{family_id}/detail-bundle` | Everything the couple profile draws, in one answer: `family`, `spouses`, `persons` (each spouse's person detail bundle, in the order of `spouses`), `notes` (the family's, then each spouse's, at most 100 per owner), `media` (the family's own media tiles) and their `gallery`. A family of another tree is `not_found`. GraphQL: `coupleDetailBundle(treeId, familyId)` |

Used by: [Tree View](ui-genealogy-tree.md) (connectors) · [Person Edit Modal](ui-person-edit-modal.md) (couple edit)

### Family Members

| Method | Path | Description |
|---|---|---|
| `GET` | `/trees/{tree_id}/families/{family_id}/spouses` | List spouses |
| `POST` | `/trees/{tree_id}/families/{family_id}/spouses` | Add a spouse |
| `DELETE` | `/trees/{tree_id}/families/{family_id}/spouses/{spouse_id}` | Remove a spouse link; a link of another family is `not_found` |
| `GET` | `/trees/{tree_id}/families/{family_id}/children` | List children |
| `POST` | `/trees/{tree_id}/families/{family_id}/children` | Add a child |
| `DELETE` | `/trees/{tree_id}/families/{family_id}/children/{child_id}` | Remove a child link; a link of another family is `not_found` |

### Events

| Method | Path | Description |
|---|---|---|
| `GET` | `/trees/{tree_id}/events` | List events (cursor-paginated, filterable by type/person/family); a person or family of another tree is `not_found` |
| `POST` | `/trees/{tree_id}/events` | Create an event |
| `GET` | `/trees/{tree_id}/events/{event_id}` | Get an event |
| `PUT` | `/trees/{tree_id}/events/{event_id}` | Update an event; a `place_id` of another tree is `not_found` |
| `DELETE` | `/trees/{tree_id}/events/{event_id}` | Soft-delete an event |
| `GET` | `/trees/{tree_id}/events/{event_id}/witnesses` | List event witnesses (GEDCOM `ASSO`) |
| `POST` | `/trees/{tree_id}/events/{event_id}/witnesses` | Add a witness (person + optional relation text) |
| `DELETE` | `/trees/{tree_id}/events/{event_id}/witnesses/{witness_id}` | Remove a witness; a witness of another event is `not_found` (GraphQL: the optional `eventId`) |

Every event read carries `tags`, the labels put on the event as `[{id, label,
color}]` sorted by folded label (GraphQL `Event.tags { id label color }`). Tags
are put on and taken off through the tag endpoints of **Tags**, not through the
event's `PUT`, so one editor's save never overwrites another's tags.

A pedigree node carries `tags` the same way, the labels put on its person
(GraphQL `GqlPedigreeNode.tags { id label color }`), and its birth and death
events carry their own tags. The events panel of the tree shows them.

An event's `age` (GEDCOM `AGE`) is accepted in GEDCOM's syntax, read
leniently (`1y6m`, `34 Y`, a bare `34`), and returned in canonical form
(`1y 6m`, `34y`); a value that is not an age, or any age on a family event
(whose spouses' ages are recorded per spouse), is a `validation_error`. `age`,
`agency`, `classification` and `related_family_id` follow the update
convention: omitted keeps, `null` clears — on both surfaces. A source's
`agency` (`SOUR.DATA.AGNC`) likewise.

An individual event's `related_family_id` names a family of the tree that the
event does not belong to (the adoptive family, the family a child is sealed
to). A family event, or an event with no individual owner, that carries one is
a `validation_error`, as is a family that is not in the tree (`not_found`).

A family event carries `spouse_ages`, a list of `{ person_id, age }` (GraphQL
`spouseAges { personId age }`, input `SpouseAgeInput`): the age its record
gives for each spouse (GEDCOM `HUSB.AGE` / `WIFE.AGE`), canonical as `age`.
Every event read fills it. A create may give it; an update replaces the whole
list when it is given (`[]` clears it) and keeps it when omitted. An entry
with a blank age is left out. A person who is not a spouse of the event's
family, a person named twice, a value that is not an age, or any spouse age
on an individual event is a `validation_error`.

Used by: [Tree View](ui-genealogy-tree.md) (events sidebar) · [Person Edit Modal](ui-person-edit-modal.md) (event blocks)

### Places

| Method | Path | Description |
|---|---|---|
| `GET` | `/trees/{tree_id}/places?search=&name=&ids=` | List places (cursor-paginated). `search` keeps the names containing it, ignoring case; `name` the places named exactly so, trimmed and ignoring case (what an entry form looks a typed place up by); `ids` the places of a comma-separated list of ids (what a form names its record's places by), a malformed id being a `validation_error`. GraphQL: `places(search, name, ids)` |
| `POST` | `/trees/{tree_id}/places` | Create a place; a blank `name` is a `validation_error` |
| `GET` | `/trees/{tree_id}/places/{place_id}` | Get a place |
| `PUT` | `/trees/{tree_id}/places/{place_id}` | Update a place; a blank `name` is a `validation_error` |
| `POST` | `/trees/{tree_id}/places/{place_id}/merge` | Fold the place into another — see below. GraphQL: `mergePlace(treeId, id, input)` |
| `DELETE` | `/trees/{tree_id}/places/{place_id}` | Delete a place |

Every place read carries `tags`, the labels put on the place as `[{id, label,
color}]` sorted by folded label (GraphQL `Place.tags { id label color }`), also
on the place rows of `GET /trees/{tree_id}/dictionary/places` and the GraphQL
`dictionaryPlaces`. Tags are put on and taken off through the tag endpoints of
**Tags**, not through the place's `PUT`.

### Sources

| Method | Path | Description |
|---|---|---|
| `GET` | `/trees/{tree_id}/sources?title=` | List sources (cursor-paginated). `title` keeps the sources titled exactly so, trimmed and ignoring case — what an entry form looks a typed source up by. GraphQL: `sources(title)` |
| `POST` | `/trees/{tree_id}/sources` | Create a source; a blank `title` is a `validation_error` |
| `GET` | `/trees/{tree_id}/sources/{source_id}` | Get a source |
| `PUT` | `/trees/{tree_id}/sources/{source_id}` | Update a source; a blank `title` is a `validation_error` |
| `DELETE` | `/trees/{tree_id}/sources/{source_id}` | Soft-delete a source. With `?only_if_unused=true` the source is kept if any citation, note, media link or repository link still points at it — `204` deleted, `200` kept |
| `POST` | `/trees/{tree_id}/sources/{source_id}/archive-target` | Where a cited source opens on its archive's portal: a JSON body, `{}` or `{"citation_id": …, "view": …, "parts": …}`, `citation_id` naming a citation of the source whose page, text and event complete what the source and its repositories say ([Archive Portals §5.1](archives.md#51-citation-recognition)), and whose person — the person it is attached to, or its individual event's principal — gives the surname an index of persons searched by name may be sent, read from the tree and never from the body ([Archive Portals §5.3](archives.md#53-api)), `view` one view of the cited register to resolve instead of the cited ones, and `parts`, `{"locality", "act", "year", "view"}` all optional, what the reader completed in the "Find in the archives" dialog, `act` a document kind's code. Answers the `ArchiveTarget` ([Archive Portals §5.2](archives.md#52-result)). GraphQL: `Source.archiveTarget(citationId, view, parts: ArchivePartsInput)` |

Every source read carries `tags`, the labels put on the source as `[{id, label,
color}]` sorted by folded label (GraphQL `Source.tags { id label color }`),
put on and taken off through the tag endpoints of **Tags**.

`archive-target` reads nothing it writes, but may query the archive's portal,
once per call: it is a `POST` so that nothing caches or prefetches it. The
answer is `{"kind": "view", "url", "views", "view_count", "call_number",
"attribution", "renumbering"}` — each view `{"view", "url", "ark", "image"}`,
`image` being `{"picture", "thumbnail", "width", "height"}` or `null`, and
`renumbering`, present only when the register counts another number of
images than the citation, `{"cited_count", "shifted_by"}`: the cited count,
and how far the views opened are from the cited numbers ([Archive Portals
§7](archives.md#7-errors-and-fallbacks)) — or `{"kind":
"results", "url", "matches"}`, `matches` being `null` when the address was
built without querying the portal, as for an archive whose portal only a
browser reaches. GraphQL answers one `GqlArchiveTarget` object whose `kind`
is `VIEW` or `RESULTS` and whose fields of the other kind are `null`, and
answers the field only for a source read on its own, never for the items of
a list (`VALIDATION_ERROR`). `view` is how the document form attaching cited
views adds the previous or next view of the register, one request per click
([Archive Portals §6.4](archives.md#64-attaching-and-cropping)), an offer the
interface keeps disabled for now
([Archive Portals §6.3](archives.md#63-iiif-behind-the-scenes)): it resolves that
view alone, with the side the citation gives it, and a view below 1 or
beyond the cited view count is a `400 validation_error` (`VALIDATION_ERROR`).
`parts` win over everything the records say; an unknown document kind, one
none of the recognized archive's collections holds, a blank or overlong
locality, a year outside 1000–2100 or a view below 1 is a
`400 validation_error` too. A portal address of the archive found in the
records answers a `view` with that `url` and no views, without querying the
portal; a citation whose archive is recognized but whose act or locality is
not, without `parts`, answers `results` with no `matches`: the archive's
filtered search page, or its website without a document kind.
A source or citation that is absent, deleted or
of another tree, or a citation of another source, is `404 not_found`. The
other failures carry their own codes on both surfaces, in upper case in
GraphQL, with no `request_id`:

| Status | Code | Meaning |
|---|---|---|
| 422 | `not_an_archive_citation` | Nothing in the source, the citation, its repositories or its event names a register of an archive. |
| 422 | `no_adapter` | The archive is not catalogued, or none of its collections holds the cited document kind. |
| 502 | `unexpected_response` | The portal answered, but not as its adapter expects. |
| 502 | `challenged` | The portal answered with an anti-bot challenge instead of its page. |
| 502 | `unreachable` | The portal could not be reached, or answered with a server error. |
| 504 | `timeout` | A request to the portal got no answer within 30 seconds, or its gateway answered `504`. |

### Repositories

| Method | Path | Description |
|---|---|---|
| `GET` | `/trees/{tree_id}/repositories` | List repositories (cursor-paginated) |
| `POST` | `/trees/{tree_id}/repositories` | Create a repository; a blank `name` is a `validation_error`; `address`, `phone`, `email`, `website` optional |
| `GET` | `/trees/{tree_id}/repositories/{repository_id}` | Get a repository |
| `PUT` | `/trees/{tree_id}/repositories/{repository_id}` | Update a repository: omitted keeps, `null` clears; a blank `name` is a `validation_error` |
| `POST` | `/trees/{tree_id}/repositories/{repository_id}/merge` | Fold the repository into another — see below. GraphQL: `mergeRepository(treeId, id, input)` |
| `DELETE` | `/trees/{tree_id}/repositories/{repository_id}` | Soft-delete a repository. With `?only_if_unused=true` it is kept while a live source is held there or a note is about it — `204` deleted, `200` kept |
| `GET` | `/trees/{tree_id}/repositories/{repository_id}/sources` | The live sources it holds: each link with its `source` |
| `GET` | `/trees/{tree_id}/sources/{source_id}/repositories` | A source's links to the live repositories holding it, in order |
| `POST` | `/trees/{tree_id}/sources/{source_id}/repositories` | Link a source to a repository of the tree: `repository_id`, optional `call_number`, `media_type` (`SourceMediaType`), `sort_order` (after the others when omitted). One link per call number |
| `PUT` | `/trees/{tree_id}/sources/{source_id}/repositories/{link_id}` | Update a link (`repository_id`, `call_number`, `media_type`, `sort_order`); a link of another source is `not_found` |
| `DELETE` | `/trees/{tree_id}/sources/{source_id}/repositories/{link_id}` | Remove a link; a link of another source is `not_found` |

Every repository read carries `tags`, the labels put on the repository as
`[{id, label, color}]` sorted by folded label (GraphQL
`Repository.tags { id label color }`), put on and taken off through the tag
endpoints of **Tags**.

GraphQL: `repositories`, `repository`, `Source.repositories`
(`SourceRepository { callNumber mediaType sortOrder repository source }`),
`Repository.sources`, `createRepository`, `updateRepository`,
`deleteRepository(onlyIfUnused)`, `addSourceRepository`,
`updateSourceRepository`, `removeSourceRepository` — same validation and
errors. Notes take a `repository_id` (`repositoryId`) like the other owners,
and list by it. Every write records a change: a repository is a versioned
record, a link versions its source.

### Citations

| Method | Path | Description |
|---|---|---|
| `GET` | `/trees/{tree_id}/citations` | List citations in a cursor connection, filterable by `person_id`, `event_id`, `family_id`, and `source_id`; a filter naming a record of another tree is `not_found` |
| `POST` | `/trees/{tree_id}/citations` | Create a citation |
| `PUT` | `/trees/{tree_id}/citations/{citation_id}` | Update a citation — including `source_id`, which repoints it at another source in place |
| `DELETE` | `/trees/{tree_id}/citations/{citation_id}` | Delete a citation |

Every citation read carries `tags`, the labels put on the citation as `[{id,
label, color}]` sorted by folded label (GraphQL `Citation.tags { id label color }`),
put on and taken off through the tag endpoints of **Tags**. They are the
citation's own, not its event's or its source's.

A citation's `confidence` is optional: `null` (or omitted at creation) means
the evidence is not assessed. An update leaves it alone when the field is
omitted and clears it on `null`; GraphQL's `confidence` is nullable on
`Citation`, `CreateCitationInput` and `UpdateCitationInput` with the same
meaning.

### Media

| Method | Path | Description |
|---|---|---|
| `GET` | `/trees/{tree_id}/media` | List the tree's documents (cursor-paginated, or from an `offset`), narrowed by the media library filters, each with its `usage_count` — see **Media library** below |
| `GET` | `/trees/{tree_id}/media/facets` | The values the media library filters can take: `{tags: [{id, label, color, count}], kinds: [{kind, count}], categories: [{category, count}]}` |
| `POST` | `/trees/{tree_id}/media` | Add a page that names a file without holding it — an archive's URL, or a path a GEDCOM mentioned. `document_id` is required: bytes and addresses live on pages, and a page belongs to a document. A page held as an `http(s)` URL may also carry `thumbnail_url`, the `http(s)` address of a small picture of it its server serves, and its pixel size, `width` and `height` together. The GraphQL twin is `uploadMedia` |
| `POST` | `/trees/{tree_id}/media/upload` | Upload a file. `multipart/form-data`: `file` (required), `title`, `description`, `media_id`, `document_id`. `201` for a new record, `200` when `media_id` attaches bytes to an existing one; `document_id` appends the file as the next page of a multi-page document |
| `POST` | `/trees/{tree_id}/media/document` | Create an empty multi-page document (`{title?}`). Pages are added by uploading with `document_id` |
| `POST` | `/trees/{tree_id}/image-data` | Resolve held **image sources** (see below) to inline `data:` URLs, in request order, at most 1,024 per request. A slot is `null` when there is nothing of ours to inline — a remote source, or a picture we no longer hold. For clients with no origin of their own to serve pictures from; a client that has one never calls this |
| `POST` | `/trees/{tree_id}/gallery-bundle` | Load gallery data for `{media_ids, vignette_ids}`: thumbnail sources, linked event ids, up to four document-page previews, and cropped vignette sources. Every source is an **image source** (see below), never image bytes. A page preview is the page's own thumbnail, or — for a page held only as a remote image URL — its `thumbnail_url` when it has one and that address otherwise, which the client draws directly. A remote page that is not an image contributes no preview. A PDF page's thumbnail previews its document like any page's. `portrayable` says whether the medium may be chosen as a portrait as far as its type goes: `false` for a PDF page and for a document all of whose pages are PDFs, which a portrait refuses (see [Portraits](#portraits)), so a client draws such a tile without offering it. `pdf_page` (`{media_id, file_size}`) names, for a document none of whose pages has a preview, its first page that is a PDF we hold without a thumbnail, for a client to draw itself; it is null otherwise. A vignette source is the crop we cut from our own copy; over a remote page there is nothing to cut, so the whole picture's address is sent with a `crop` object — `{x, y, width, height, source_width, source_height}` — and the client cuts it. `crop` is absent whenever the source already is the region, and for a page whose pixel size nobody has recorded |
| `GET` | `/trees/{tree_id}/media/{media_id}/pages` | A document's pages, in order |
| `PUT` | `/trees/{tree_id}/media/{media_id}/pages` | Set the page order (`{page_ids: [...]}`). Must name exactly this document's pages, once each — a partial list is refused rather than guessed at |
| `DELETE` | `/trees/{tree_id}/media/{media_id}/pages/{page_id}` | Permanently delete the page, its relationships and unshared stored bytes. Remaining pages close the gap; removing the last page leaves an empty document |
| `GET` | `/trees/{tree_id}/media/{media_id}` | Get media metadata |
| `GET` | `/trees/{tree_id}/media/{media_id}/file` | The stored bytes, served **inline** for previews. `Content-Type` from the file, strong `ETag` (its SHA-256), `Cache-Control: private, max-age=3600`, `304` on a matching `If-None-Match`. `404` if the record has no bytes |
| `GET` | `/trees/{tree_id}/media/{media_id}/download` | Stream one stored original of any supported media type with `Content-Disposition: attachment`, its MIME type, `Cache-Control: private, no-store`, and `X-Content-Type-Options: nosniff`. Always transfers the file rather than returning a conditional `304`. `404` for absent, deleted, foreign-tree or byte-less records; remote URLs are never fetched or redirected to |
| `GET` | `/trees/{tree_id}/media/{media_id}/thumbnail` | Generated thumbnail (longest edge 400 px): a PNG when a pixel of the source is transparent, a JPEG otherwise — an alpha channel opaque everywhere, as photographs saved as PNG often carry, counts as none. For a PDF it is the scan its first page paints (see **Upload rules**). `404` when none was made — a PDF whose first page paints no image we decode, or a file whose decoding failed — so a gallery can fall back to an icon on the status alone |
| `GET` | `/trees/{tree_id}/media/{media_id}/archive` | Every live page of a document, in one ZIP attachment (`application/zip`, `private, no-store`, `nosniff`). Entries are prefixed `001_`, `002_` in current reading order; padding grows beyond 999 pages so lexical order remains correct. A page held only as an `http(s)` URL contributes a `.url` Internet Shortcut (`[InternetShortcut]` + `URL=`, CRLF, control characters stripped) instead of bytes, so the archive has one entry per page and the address survives the round trip. Empty documents and any other page without stored bytes return `404`; pages are never silently skipped |
| `PUT` | `/trees/{tree_id}/media/{media_id}` | Update media metadata. `width`/`height` are sent together or not at all, and only for a page whose bytes we do not hold — the browser that displayed a remote picture is the only witness to its size, and for our own copy the size is read from the bytes. Recording them re-checks the regions already drawn on that page. `thumbnail_url` sets a remote page's thumbnail address (`null` clears it); repointing `file_path` without one clears it |
| `GET` | `/trees/{tree_id}/media/{media_id}/deletion-status?allowed_link_id={link_id}` | Whether the gallery link is the sole external reference (`{can_delete: bool}`); used to ask for confirmation only when deletion is certain |
| `DELETE` | `/trees/{tree_id}/media/{media_id}` | Permanently delete the media, its related rows and unshared stored objects. With `?only_if_unreferenced_elsewhere=true&allowed_link_id={link_id}`, keep it when any reference other than that gallery link remains (`204` deleted, `200` retained) |

GraphQL mirrors the status endpoint with `canDeleteMedia(treeId:, id:, allowedLinkId:)`, returning the same eligibility boolean before `deleteMedia` is called.

Every media write — a page, an upload, a metadata update, a tag, a page
order, a deletion — runs in one transaction with its audit entry, so a
failed audit record undoes the write. A write that changes what a person's
card draws rewrites that person's projection in the same transaction: the
persons linked to the document or any of its pages, and those whose chosen
portrait is one of them or a crop of one. Deleting a media or a page, adding
or reordering pages and updating the metadata all do; a stored file leaves
the media store only once the deletion is committed. A `place_id` of another
tree is `not_found`; a blank `file_name` on a new page is a
`validation_error`.

**Media library.** `GET .../media` lists documents only — a page is reached
through its document — in creation order, the cursor being the last id seen.
Its optional filters combine with AND, and `total_count` is counted under them:

| Parameter | Keeps the documents… |
|---|---|
| `tag` | carrying this tag, named by its id (see **Tags**). Repeatable: `tag=a&tag=b` keeps the documents carrying every tag given |
| `kind` | with at least one live page of this file kind: `image`, `pdf`, `video`, `audio` or `other`, read from the page's MIME type |
| `category` | filed under this `document_category` |
| `name` | whose title or file name, or one of whose pages' file names, contains the text, ignoring case and accents |
| `linked_name` | connected to a person whose primary given names and surname (in either order) or maiden name contain the text, ignoring case and accents. Connected means a media link, on the document or one of its pages, to the person, to a family they are a spouse in, or to an event of theirs or of such a family — or a crop identifying them |
| `event_from`, `event_to` | linked, by a media link or a crop, to a live event whose `date_sort` falls within these years, inclusive. An undated event matches no range |
| `added_from`, `added_to` | created on these UTC days, inclusive (`YYYY-MM-DD`) |

Blank text is no filter. A range that ends before it starts is a `400`; an
unknown `kind` or `category` is a `400` too. Each node is the document with
`usage_count` beside its fields: the distinct persons, families, events and
sources it is attached to by media links on it or its pages — crops and
portraits are not counted. The counts for a page are one grouped query.

Besides `first` and `after`, the list takes `offset`, the number of documents
to skip, so that a numbered pager jumps to any page without walking the
cursors before it; the envelope and `total_count` are the same. An offset past
the end is an empty page, and giving `offset` with `after` is a `400`.

`GET .../media/facets` lists every tag carried by a live document, as
`{id, label, color, count}` with its document count, sorted by folded label; every file kind and category held, with theirs, in their declaration
order. With `tag` (repeatable), the tags are listed and counted among the
documents carrying every tag given, so a tag cloud offers only the tags that
still narrow a selection; the kinds and categories always count the whole
library, and no other listing filter applies.

GraphQL mirrors both: `mediaList(treeId, first, after, offset, filter:
MediaListFilterInput)` takes the same filters (`tags: [ID!]`, `kind`, `category`,
`name`, `linkedName`, `eventFrom`, `eventTo`, `addedFrom`, `addedTo`) and puts
the count on each edge as `usageCount`; a backwards range, or an `offset` with
`after`, is a GraphQL error.
`mediaFacets(treeId, tags: [ID!])` returns `{tags {id label color count} kinds {kind
count} categories {category count}}`.

**Download capability.** `mediaDownload(treeId: ID!, id: ID!): GqlMediaDownload!`
and `mediaArchive(treeId: ID!, id: ID!): GqlMediaDownload!` return `{ url }`, a
same-origin `/api/v1/trees/{tree_id}/media/{id}/download` or `/archive` URL.
Both queries apply the same live-tree, live-parent, deletion and stored-key checks as REST and
open each required object without collecting its body. Missing records and
unheld or remote pages produce GraphQL errors, not usable URLs. Storage failures
(including a referenced object lost from the store) produce REST `500` or
GraphQL errors. Invalid UUIDs produce REST `400` or GraphQL errors. The URL is
not a snapshot or authorization token: HTTP repeats the checks, and later
storage failures can still abort the transfer. Binary bytes are never encoded
in GraphQL. Single-page documents use their page id for the original download;
`/archive` also works for a document with one stored page. Existing `/file`,
`/thumbnail`, page-list, and vignette-image endpoints remain independent.

**Archive packaging and filenames.** ZIP entries use `Stored`, not Deflate:
JPEG, PNG, PDF and other already-compressed originals do not benefit from
another compression pass. Files are read sequentially as store streams into an
anonymous temporary ZIP on a blocking worker, never collected as an album in
memory. At most two archives are packaged or streamed concurrently per process. ZIP64
supports large entries; ZIP metadata still scales with the page count and
temporary disk use scales with the archive size. The complete ZIP is finalized
before HTTP success headers are sent, then streamed to the client. A missing or
unreadable page fails the whole request instead of returning a partial archive;
temporary files are removed on error or when the response stream is dropped.
Attachment filenames use ASCII fallback and RFC 6266 UTF-8 encoding. Both
attachment and ZIP entry names discard path components and neutralize control
characters and platform-reserved punctuation; numbered entry names avoid
collisions even when pages have identical filenames. Page rows and storage-key
prefixes must belong to the requested tree. Remote media remain client-side
links, never server-side fetch targets.

The gallery bundle accepts at most 1,024 media and vignette ids combined. It
resolves each database collection in one query and reads blobs with bounded
concurrency. Clients split larger galleries into as many bundles as necessary.
The per-media thumbnail, page-list, and vignette-image endpoints remain for a
viewer or editor that opens one selected asset; the initial grid does not call
them once per tile.

**Upload rules.** The type is decided by the file's magic bytes, not by the declared MIME type or the extension: JPEG, PNG, GIF, BMP, TIFF, WebP, ICO and PDF are accepted, everything else is a `400`. Maximum 128 MiB — comfortably above what the services we exchange with take (Geneanet caps a media file at 50 MB and accepts only JPEG, PNG, GIF and PDF), because a 1200 dpi register spread or a few-hundred-page dossier clears 64 MiB unremarkably. Larger still is EPIC H's chunked-upload problem. Uploading a file the tree already holds re-uses the stored bytes and still creates a second record, which is what a census page shared by eight siblings needs.

**PDF uploads.** A PDF has no `width` or `height`: it is a document, not a picture. Its `page_count` comes from its page tree, `1` when it cannot be opened — a password-protected one included. OxidGene ships no PDF renderer, so its thumbnail is the scan its first page is made of: the largest image, by declared area, that page 1's content paints, turned as the page's `/Rotate` turns it. A JPEG is thumbnailed as stored; a CCITT fax, JBIG2 or JPEG 2000 image is decoded first. A page with no image (text or vector), an image compressed any other way (Flate, LZW, a chain of filters), an encrypted file, and an image whose declared size would decode to more than 1 GiB — checked before anything is decoded — get no thumbnail; the upload still succeeds and the warning logged names the case, never the content. Viewing a PDF is the client's work.

**Three kinds of media.** *Stored* — `storage_key` is set, we serve the bytes, there is a thumbnail (for a PDF, when its first page paints a scan) and, on a raster image, crops can be drawn. *Remote* — `file_path` is an `http(s)` URL: recorded, never fetched by us, so no thumbnail of ours and no crop of ours, and the browser goes to the origin directly; `thumbnail_url` may name the small picture the origin serves, which tiles draw. A thumbnail address is accepted only as an `http(s)` URL and only for a page whose `file_path` is one — anywhere else it is a `validation_error`. *Unheld* — a record naming a file nobody uploaded, which is where every GEDCOM import starts. `PUT .../media/{id}` may edit `file_path` for the last two and **refuses it for a stored one**: there `file_path` is the value a GEDCOM export writes back, and repointing it would make the export describe a file we are serving something else for. It refuses a `mime_type` for a stored one too: that type was read from the bytes at upload and is the one they are served as. A remote `mime_type` is guessed from the URL's extension when not given — the only evidence available without fetching, and it decides whether a viewer embeds the file or offers it as a download. A URL with no extension leaves it `application/octet-stream`, which is not read as "not a picture": such a page is still sent as a gallery preview and still drawn in the viewer, because the browser fetching it is the only reader that can identify the bytes.

**A media carries what a fact carries.** `PUT .../media/{id}` takes `title`, `description`, `date_value`, `date_value2`, `date_qualifier`, `calendar`, `place_id`, `source_media_type` and `document_category`, so "a photograph taken around 1890 at Nantes" is written the way a birth around 1890 is. Tags are independent links, put on and taken off through the tag endpoints of **Tags**; this prevents one editor's save from overwriting another editor's tag. Labels are trimmed and folded to one tag per tree. Multi-page document tags live on the document, never its pages. `GqlMedia.tags` carries the document's tags, sorted by label. `date_sort` is **not** accepted: the server derives it from `calendar` + `date_value`, exactly as for an event. Notes about a document go on `note.media_id` (`POST /notes` with `media_id`, `GET /notes?media_id=`). There is deliberately **no source field** — a media *is* a source document.

**Two fields for what looks like one question.** `source_media_type` is GEDCOM's `SOURCE_MEDIA_TYPE`, exactly — `photo`, `manuscript`, `tombstone`, `fiche`, `film`, `map`, `newspaper`, `book`, `card`, `magazine`, `audio`, `video`, `electronic`, `other` — and is what an export writes and other genealogy software reads. `document_category` is the distinction GEDCOM cannot draw: a census return, a marriage contract and a conscription register are all `manuscript` to it. Sending a category without a medium also sets the medium that category implies, so a census return exports as `MANUSCRIPT` rather than `OTHER`; sending both keeps both. `document_category` accepts an explicit `null` to unclassify. See [Data Model](data-model.md) (Media).

**Documents and pages.** Every document is a byte-less shell, including an
ordinary photograph's single-page document. Its pages are media rows carrying
`parent_media_id` and `page_index`; each page must belong to a live document in
the same tree, never another page. A page's `page_count` counts the pages inside
its file (a multi-page TIFF's images, a PDF's pages); a document's count is recomputed from
its live page rows, including zero. Every write that adds a page refreshes that
count, whichever surface it arrives through and whether the page carries bytes
or only an address. Listings filter `parent_media_id IS NULL`.
File attachment targets pages only. Neither a document's file fields nor a
stored page's path or sniffed MIME type can be changed through metadata updates.
Replacing page bytes must preserve the validity of existing crop coordinates.
Page reordering requires a complete, duplicate-free permutation. Definitive
page deletion, through either deletion endpoint, closes the ordering gap and
refreshes the document count.

**Storage.** Media bytes are content-addressed under `{tree_id}/{aa}/{bb}/{sha256}.{ext}`. `OXIDGENE_MEDIA_BACKEND=filesystem` is the default and stores them below `OXIDGENE_MEDIA_ROOT` (the platform user-data directory, `~/.local/share/oxidgene/media` on Linux, by default). `OXIDGENE_MEDIA_BACKEND=s3` stores the same keys in the bucket configured by `OXIDGENE_S3_BUCKET`, `OXIDGENE_S3_REGION`, optional `OXIDGENE_S3_ENDPOINT`, `OXIDGENE_S3_ACCESS_KEY_ID`, and `OXIDGENE_S3_SECRET_ACCESS_KEY`. HTTP endpoints are accepted only when explicitly configured, for development S3-compatible services such as RustFS; one beyond loopback logs an `s3_plain_http` warning at startup, since media and signed requests then travel unencrypted. Keys are scoped per tree so a purge can delete one tree by prefix without affecting another.

Used by: [Person Edit Modal](ui-person-edit-modal.md) (media section)

### Vignettes

A vignette is a rectangle on a stored media file — one parish-register page carries entries for several unrelated families, and each is a crop rather than a copy. Coordinates are in the source image's own pixels, so re-scanning at a higher resolution does not orphan them.

| Method | Path | Description |
|---|---|---|
| `GET` | `/trees/{tree_id}/media/{media_id}/vignettes` | Vignettes on a media file, oldest first |
| `POST` | `/trees/{tree_id}/media/{media_id}/vignettes` | Create one. Body: `x`, `y`, `width`, `height` (required), `person_id`, `event_id` |
| `GET` | `/trees/{tree_id}/vignettes?person_id=…` / `?event_id=…` | Vignettes attributed to a person, or standing as evidence for an event. Exactly one filter is required |
| `GET` | `/trees/{tree_id}/vignettes/{vignette_id}` | Get one |
| `PUT` | `/trees/{tree_id}/vignettes/{vignette_id}` | Move or re-attribute. The four rectangle fields travel together — all or none |
| `DELETE` | `/trees/{tree_id}/vignettes/{vignette_id}` | Delete it. Hard delete; the media is untouched |
| `GET` | `/trees/{tree_id}/vignettes/{vignette_id}/image` | The cropped region as its own JPEG, derived on read and scaled down to fit the 400-pixel thumbnail box: a crop is only ever drawn as a thumbnail. A request decodes at most 128 MiB of pixels: a larger JPEG is decoded at ½, ¼ or ⅛ scale and the vignette comes back that much smaller, and any other image that large is cut from its stored thumbnail (`400` without one). It carries an `ETag` derived from the source file's digest and the rectangle, and answers `If-None-Match` with `304`. `400` for a PDF, even one with a thumbnail — crops are raster-only, and a PDF's thumbnail shows one image of its first page, not the page a region was drawn on |

Creation and updates require a page, never a document shell (`400`), and a live
parent document. Person/event attributions must be live records in the page's
tree; missing, deleted or foreign-tree references are rejected as not found.
The rectangle must have a nonnegative origin, positive dimensions and
non-overflowing extents, and fit each known page dimension (`400`). Unknown
dimensions on imported pages and PDFs do not impose invented bounds. REST and
GraphQL use the same domain and repository validation, including attribution-only
updates. Each write runs in one transaction with its audit entry; deleting a
vignette a person uses as their portrait clears it and rewrites that person's
projection in the same transaction.

`PUT /trees/{id}` accepts `default_privacy` (`"public" | "private"`) — what
`"default"` resolves to for everything in that tree — and the GEDCOM submitter:
`submitter_name`, `submitter_email` and `submitter_address` (GraphQL
`submitterName`, `submitterEmail`, `submitterAddress`) — omitted keeps, `null`
or a blank value clears, on both surfaces. It also accepts
`anomaly_thresholds` (GraphQL `anomalyThresholds`, input
`AnomalyThresholdsInput`), the thresholds of the anomaly rules
([Tools §3.3](ui-tools.md)) set all at once: an object of whole numbers
named as the tree returns them (`max_lifespan_years`,
`old_centenarian_years`, `centenarian_before_year`, `min_parent_age_years`,
`max_father_age_years`, `max_mother_age_years`, `max_posthumous_birth_days`,
`twin_days`, `min_sibling_gap_days`, `max_sibling_gap_years`,
`min_union_age_years`, `max_union_age_years`, `max_spouse_gap_years`,
`recorded_age_tolerance_years`, `approximate_slack_years`,
`max_age_alive_years`, `max_same_day_distance_km`), a threshold left out
taking its default and an unknown name refused. A value out of its range
([Settings §8b](ui-settings.md)), an `old_centenarian_years` above
`max_lifespan_years` or a `twin_days` not below `min_sibling_gap_days` is a
`400 validation_error` (GraphQL `VALIDATION_ERROR`) and changes nothing.
Every tree carries its `anomaly_thresholds`, the defaults until set.
**Privacy** is accepted on
all three of `PUT .../persons/{id}`,
`PUT .../families/{id}` and `PUT .../media/{id}` as `"default" | "public" |
"private"`. The family route's body is optional — it long predates this field as
a bare "touch `updated_at`" — so a request with no body still succeeds. Nothing
reads these values yet; see [Data Model](data-model.md) (Privacy).

### Portraits

| Method | Path | Description |
|---|---|---|
| `PUT` | `/trees/{tree_id}/persons/{person_id}/portrait` | Choose what represents a person: `{media_id}`, `{vignette_id}`, or `{}` to clear it. Both ids together is a `400` — a portrait is a media or a crop, never both. A PDF is a `400` too, on both surfaces: a PDF page, or a document all of whose pages are PDFs. A PDF is never a portrait, even with a thumbnail |
| `GET` | `/trees/{tree_id}/portraits` | Every person's portrait in the tree, as `{person_id, media_id?, vignette_id?, file_path, has_thumbnail}` |
| `POST` | `/trees/{tree_id}/portrait-images` | Load display-ready portraits for `{person_ids: [...]}` in one bounded operation, returning `{person_id, source}` rows |

The portrait is a property of the *person*, not of a media link — see
[Data Model](data-model.md) (Person) for why — so setting one is a single write
and needs no clearing pass over the person's other links.

`media_id` accepts a document, which is what a gallery tile is and therefore
what a client sends. What the read operations return is the page that holds the
file: `GET /portraits`, `POST /portrait-images` and `PersonProfile.primary_media`
all resolve a stored document to its first drawable page that is not a PDF — see
[Data Model](data-model.md) (Person). A portrait may be a page held only as a
remote URL.

`GET /portraits` exposes the complete portrait assignments as an inventory
operation. First-party display surfaces do not use it: a pedigree, result page,
or picker generally needs portraits for only a bounded set of people.

**Drawing portraits.** Screens submit the person ids they need to
`POST /portrait-images` rather than loading the portrait inventory or
downloading one image per person. The operation accepts at most 1,024 ids,
resolves them in one database query, and returns an **image source** per
person rather than an image.
Remote portraits retain their `http(s)` source; unavailable portraits are
omitted. A portrait that is a region of a remote picture comes back as that
picture's address plus the same `crop` object the gallery bundle uses, because a
region of a file we do not hold cannot be cut here — the client cuts it. Clients split larger sets into as many requests of at most 1,024 ids
as necessary. The individual thumbnail and vignette image endpoints remain for
single-image media workflows. `file_path` is never itself assumed to be a URL:
it is the producer's own path, kept verbatim so an export round-trips.

### Media Links

| Method | Path | Description |
|---|---|---|
| `GET` | `/trees/{tree_id}/media-links` | Unfiltered: every person↔media link in the tree, flat — what the pedigree canvas reads to find each card's photo |
| `GET` | `/trees/{tree_id}/media-links?entity_type=person\|family\|event\|source&entity_id={id}` | One entity's gallery. Each row is the link (`link_id`, `sort_order`) with the **media flattened in**, so a grid of twenty scans is one request rather than twenty-one — a tile cannot be drawn without the MIME type and whether a thumbnail exists |
| `POST` | `/trees/{tree_id}/media-links` | Attach a media to an entity |
| `DELETE` | `/trees/{tree_id}/media-links/{link_id}` | Detach. The media itself is untouched — the file may document three other people |

For a multi-page document, posting the parent media id attaches the whole
document; posting a child page media id attaches that page only. No separate
page field is accepted. Person-profile galleries include direct person links,
links to the person's conjugal families, and person-attributed vignettes;
duplicate attachment tiles for the same media are collapsed client-side.

Creating or deleting a link runs in one transaction with its audit entry, and
a link to a person rewrites that person's projection in it — their first
linked picture is what their card draws when no portrait is chosen — on both
surfaces.

### Tags

A tag belongs to one tree and is shared by every kind of record; see
[Data Model](data-model.md#tag) for the model. `kind` is `person`, `family`, `event`, `place`,
`source`, `citation`, `repository`, `media` or `note`. A page of a document is
tagged on the document.

| Method | Path | Description |
|---|---|---|
| `GET` | `/trees/{tree_id}/tags` | The whole vocabulary, sorted by folded label, each tag with `{id, label, color, counts: {persons, families, events, places, sources, citations, repositories, media, notes}}`; answered whole |
| `POST` | `/trees/{tree_id}/tags` | Create a tag (`{label, color?}`), or return the one its label folds to (`200` instead of `201`) |
| `PUT` | `/trees/{tree_id}/tags/{tag_id}` | Rename or recolour (`{label?, color?}`). A label folding to another tag's is `409` |
| `POST` | `/trees/{tree_id}/tags/{tag_id}/merge` | Move every link to `{into}`, dropping those it already has, then delete the tag. Returns `into` |
| `DELETE` | `/trees/{tree_id}/tags/{tag_id}` | Delete the tag and its links. `204` |
| `POST` | `/trees/{tree_id}/tags/{tag_id}/bulk` | Add the tag to, or remove it from, every person of `target` (`{action: "add" \| "remove", target}`), in one transaction and one audit entry. Returns `{count}`, the persons changed |
| `POST` | `/trees/{tree_id}/tags/target` | `{target}` in, `{total, tags: [{tag_id, count}]}` out: the persons of the target and, per tag they carry, how many. Writes nothing. The same bounds and errors as the bulk change |
| `GET` | `/trees/{tree_id}/tags/{tag_id}/records?kind=…&first=…&after=…` | The live records of that kind carrying the tag, cursor-paginated, each as `{id, kind, label, person_id, family_id, person, event, citation, persons}`. `person` is a person's name parts and lifespan (`{person_id, given_names, surname, birth_year, birth_qualifier, death_year, death_qualifier}`, as the dictionary's usage lists read them) and `null` for any other kind. `event` is an event's own details, without its sources (`{event_type, date_value, date_qualifier, date_value2, calendar, place_name, description}`, the place by its name, the dates as the event records them) and `null` for any other kind. `citation` is a citation's source title and page (`{source_title, page}`, the title `null` when the source is gone) and `null` for any other kind. `persons` lists the persons a record shows, each in the `person` shape: for an event, citation or note, the person it belongs to, or the two spouses of the couple it belongs to; for a couple, its spouses in their order; `[]` when nothing owns the record (a note of a source, a repository or a document, say), and `null` for a person, a source, a place, a repository or a document. A citation's `label` is its source's title; a note's `label` is the start of its text, its first non-empty line cut to 100 characters with an ellipsis; an event's `label` is its type as the wire spells it, and the summary above carries the rest. A `label` is empty when the record has no text to show (a citation whose source is gone, a note of blank text): the UI names such a record by its kind, with its own translated text, and never shows the id. The sentences that join the parts of a line (a dash, a dot, a citation's title and page) are the UI's, not the API's; the one exception is a couple's `label`, its spouses' names joined by a middle dot, which the UI does not show. Every list is read in a fixed number of grouped queries, whatever the page holds (GraphQL: `taggedRecords`, the same fields in camel case, with `GqlTaggedEvent`) |
| `PUT` | `/trees/{tree_id}/tags/{tag_id}/records/{kind}/{record_id}` | Put the tag on a record. Idempotent: `204` |
| `DELETE` | `/trees/{tree_id}/tags/{tag_id}/records/{kind}/{record_id}` | Take the tag off a record. Idempotent: `204` |

GraphQL: `tags(treeId)`, `taggedRecords(treeId, tagId, kind, first, after)`,
`createTag`, `updateTag`, `mergeTag`, `deleteTag`, `tagRecord`, `untagRecord`,
`bulkTag(treeId, tagId, action, target)` and `tagTarget(treeId, target)`, which
take the same `target` as the REST routes. `searchPersons` takes `tags: [ID!]`.
The GraphQL `action` of `bulkTag` is the enum `ADD` or `REMOVE` (`GqlBulkTagAction`),
where REST takes the strings `"add"` or `"remove"`. `bulkTag` returns
`{count}` (GraphQL `count`), and `tagTarget` returns `total` and `tags`, a list
of `{tagId, count}` (`GqlTagTargetCounts`), where REST returns `{total, tags: [{tag_id, count}]}`.

**Bulk targets.** `target` names exactly one of:

- `persons`: a list of person ids, at most 1 000. Each must be a live person of
  the tree; one of another tree, or a deleted one, is `404 not_found`;
- `ancestors_of`, `descendants_of` or `ancestors_and_descendants_of`: a person
  id, which must be a live person of the tree; one of another tree, or a deleted
  one, is `404 not_found`, as for a listed person. The walk takes `depth` (1 to
  30 generations; all of them, up to 30, when absent), `include_self` (default
  `false`) and, for descendants and for both sides, `include_spouses` (default
  `false`) naming the spouses of the descendants. The walk follows live families
  and live persons only: a soft-deleted family links no one, and a soft-deleted
  person is neither named nor walked through;
  With `ancestors_and_descendants_of` the two sides are one set of persons: a
  person named on both sides is counted once;
- `search`: `{q, …PersonSearchFilters}`, every person the search returns, not
  one page of it;
- `tag`: a tag id, which must be a tag of the tree; one of another tree is
  `404 not_found`. Every live person carrying the tag.

A target that names none of these or several, a depth outside 1–30, a listing
of more than 1 000 persons, or more than 10 000 persons in all is
`400 validation_error`, with the generic message of its code on both surfaces
([Cross-cutting Rules §4.4](cross-cutting.md)): the response does not name the bound it
passed. `bulkTag` writes its links in one transaction, refreshes the projections
of the persons it changed in that transaction, and writes one audit entry in the
`record_tag` entity with `details.count` ([Data Model](data-model.md), Audit log).
It returns `{count}`, the persons it added or removed the tag from. `tagTarget`
and `POST /tags/target` write nothing.

### Notes

| Method | Path | Description |
|---|---|---|
| `GET` | `/trees/{tree_id}/notes` | List notes in a cursor connection, filterable by `person_id`, `event_id`, `family_id`, `source_id`, and `media_id` |
| `POST` | `/trees/{tree_id}/notes` | Create a note |
| `GET` | `/trees/{tree_id}/notes/{note_id}` | Get a note |
| `PUT` | `/trees/{tree_id}/notes/{note_id}` | Update a note |
| `DELETE` | `/trees/{tree_id}/notes/{note_id}` | Soft-delete a note |

Every note read carries `tags`, the labels put on the note as `[{id, label,
color}]` sorted by folded label (GraphQL `Note.tags { id label color }`), put on
and taken off through the tag endpoints of **Tags**. The tagged-records listing
gives a note the owner it is attached to, a person's or a family's; a note of a
source, repository or media has none there.

A note is created with some text: a blank `text` is a validation error on
both surfaces (`400` in REST, `VALIDATION_ERROR` from `createNote`).

### Dictionary

Aggregations backing the [Dictionary](ui-dictionary.md) page. Value endpoints return distinct values + usage counts; usage endpoints return the persons behind one value, resolved server-side into `PersonUsageEntry` (id, name parts, birth/death years) in one bulk query.

Each year is paired with a `birth_qualifier` / `death_qualifier` so a list can hedge the same way a pedigree card does (`ca 1849`, `< 1917`). The qualifier sits **beside** the year rather than being folded into it: `birth_year` stays an integer the client can sort on, and a `"ca 1849"` in that field would break both that and the search grid.

| Method | Path | Description |
|---|---|---|
| `GET` | `/trees/{tree_id}/dictionary/family-names` | Distinct surnames + person counts. One entry per full surname (particle + root) as spelled, case included; `primary_count` is how many of the `count` persons carry it as their primary name |
| `GET` | `/trees/{tree_id}/dictionary/family-names/usage?value=...` | Persons carrying a surname. `value` is matched exactly against the full surname as listed, particle included, however its rows are cut between particle and root |
| `GET` | `/trees/{tree_id}/dictionary/family-names/descent?value=...&offset=...&person_id=...` | The descent of a family name: its bearers grouped into branches, a page of 50 branches from `offset` (default 0), or with `person_id` the folded descent of that bearer — see below. GraphQL: `familyNameDescent(treeId, value, offset, personId)` |
| `PATCH` | `/trees/{tree_id}/dictionary/family-names/particle` | Bulk-edit — body `{ "value": "...", "particle": "..." }` re-cuts every `PersonName` carrying surname `value` at `particle` (empty = no particle). `particle` must already be at the head of `value`; rows already cut that way are skipped. Triggers a full projection rebuild when anything changed |
| `PATCH` | `/trees/{tree_id}/dictionary/family-names/rename` | Bulk rename — see below |
| `GET` | `/trees/{tree_id}/dictionary/occupations` | Distinct occupation labels + counts |
| `GET` | `/trees/{tree_id}/dictionary/occupations/usage?value=...` | Persons with an occupation |
| `PATCH` | `/trees/{tree_id}/dictionary/occupations/rename` | Bulk rename — see below |
| `GET` | `/trees/{tree_id}/dictionary/sources?prefix=...&exact=...` | Sources + citation counts; `prefix` keeps the titles starting with it, ignoring case (absent or empty: every source). With `exact=true` the titles equal to `prefix` instead, ignoring case, which needs a non-empty `prefix` (a validation error otherwise). GraphQL: `dictionarySources(prefix, exact)` |
| `GET` | `/trees/{tree_id}/dictionary/sources/groups?prefix=` | The Sources tab's next drill-down level: `{prefix, total, groups}`, single-choice levels skipped so `prefix` may be longer than asked. Once no group is left (`total` within the drill threshold), the level's sources come with it in `sources`, so the tab needs no second request. GraphQL: `dictionarySourceDrill` |
| `GET` | `/trees/{tree_id}/dictionary/sources/{source_id}/usage` | Persons a source documents: those cited directly, the person of a cited individual event, and the spouses of a cited family or family event. Deleted persons, events and families are left out |
| `GET` | `/trees/{tree_id}/dictionary/places` | Places + reference counts (events + media) |
| `GET` | `/trees/{tree_id}/dictionary/places/{place_id}/usage` | Persons referencing a place: those whose own events take place there, the spouses of the couples whose events do, and the persons a media filed there (or one of its pages) is linked to — directly, through one of their events or couples — or shows in a crop. Deleted persons are left out |

The family-names entry is `{ value, sort_key, count, primary_count }`;
`primary_count` is absent from the other dictionaries.

`GET /trees/{tree_id}/dictionary/family-names/descent` answers the descent of
the family name `value`, matched like the usage list's, under the grouping
rules of [Dictionary §7.2](ui-dictionary.md#72-descent-of-a-family-name): a
bearer carries the name on a name other than a married name, a root is a
bearer none of whose parents bears it, and each bearer is shown in full once.
The answer is `{ value, bearer_count, branch_count, offset, branches,
by_marriage }`:

- `branches` holds at most `DESCENT_BRANCH_PAGE` (50) branches from `offset`,
  each `{ number, root_has_parents, person_count, lines }` — its rank from 1
  among all `branch_count` branches, whether the root has a parent in the
  tree none of whom bears the name, the bearers it shows in full.
- `lines` is the branch written out in reading order, flat rather than
  nested so both surfaces carry it the same way: each line is `{ kind,
  depth, person, marriage, spouse, count, branch, unfold_person_id }`.
  `kind` is `person` (a bearer, with their first union's `marriage` and
  `spouse`), `union` (a later union of the bearer above, repeated with its
  own marriage and spouse; that union's children follow), `others` (`count`
  children of the union above who do not bear the name), `reference` (a
  bearer shown in full in the branch `branch`) or `more` (`count` bearers of
  the descent of the bearer above, folded: asking again with
  `person_id=unfold_person_id` answers one branch made of that bearer's
  descent, `depth` 0 at that bearer).
- A person is `{ person_id, sex, given_names, surname, birth, death,
  deceased, sosa_ancestor }`, `birth` and `death` each `{ year, qualifier,
  year2 }` or `null` — the birth falling back to the baptism and the death to
  the burial — `deceased` whether a death or burial is recorded at all, and
  `sosa_ancestor` whether the person is the tree's SOSA root or one of its
  ancestors. `marriage` is a year of the same shape.
- An answer unfolds its branches generation by generation, all together,
  until `DESCENT_LINE_BUDGET` (400) lines; what is left is folded behind
  `more` lines. An answer for `person_id` always unfolds that bearer's own
  unions.
- `by_marriage` lists the carriers of the name by a married name alone whom
  no union line shows as a spouse, in the first page only (`offset` 0, no
  `person_id`).

A blank `value` is refused with `400` (`VALIDATION_ERROR`), and a `person_id`
that is not a bearer of the name with `404` (`NOT_FOUND`). The rows are read
in nine grouped queries — the carriers' names, their families as spouse and
as child, those families' spouses and children, the persons' sexes, names and
dates, the unions' marriages — plus the SOSA lookup, however many bearers the
name has; the forest is built in memory.

`PATCH /trees/{tree_id}/dictionary/family-names/rename` gives every person
whose **primary** name carries surname `value` the surname `new_value`. Body
`{ "value": "X", "new_value": "Y", "particle": "de" }`:

- `value` is matched exactly against the full surname as listed, particle and
  case included. Alias, married and other secondary names carrying it are left
  unchanged, so `value` may remain listed for them.
- `new_value` is stored as sent; clients that want capitals, as the UI's name
  fields do, send capitals.
- `particle` is optional. Absent, the renamed rows take the cut `new_value`
  already has in the tree, or the detected one when it is not listed yet;
  `""` means no particle. When given it must be at the head of `new_value`,
  and it is then also applied to the rows already carrying `new_value`, so an
  entry never holds two cuts.
- A blank `value` or `new_value`, or a `particle` that is not at the head of
  `new_value`, is refused with `400`. Renaming a name to itself changes nothing.

The response is `{ value, new_value, surname_prefix, surname, names_updated,
persons_updated, merged }`: the cut stored, the rows and distinct persons
rewritten, and whether `new_value` was already listed so that the renamed rows
merged into it. The renamed persons' projections and search rows, and those of
their spouses, children and parents, are refreshed in the same transaction.
When anything changed, one audit entry is recorded (`entity: "family_name"`,
`action: "update"`, `label` the old name, `details.count` the persons renamed,
`details.new_label` the new name) and each renamed person gets a version; a
person is restored individually through `POST …/history/person/{id}/revert`.

`PATCH /trees/{tree_id}/dictionary/occupations/rename` gives every occupation
event labelled `value` the label `new_value` (GraphQL `renameOccupation(treeId,
input: { value, newValue })`). Body `{ "value": "X", "new_value": "Y" }`:
`value` is matched against the label as listed, trimmed; `new_value` is stored
trimmed, and joins the occupation it already names. A blank `value` or
`new_value` is refused with `400`; renaming a label to itself changes nothing.
The response is `{ value, new_value, events_updated, persons_updated, merged }`.
The holders' projections are refreshed in the same transaction. When anything
changed, one audit entry is recorded (`entity: "occupation"`, `action:
"update"`, `label` the old label, `details.count` the persons, `details.new_label`
the new label) and each holder gets a version.

**Merging a place or a repository.** `POST …/places/{place_id}/merge` folds the
place into another of the tree: body `{ "into": "<id>", "name": "…",
"latitude": 47.2, "longitude": -1.5 }`, the fields the kept place `into` takes
— chosen by the client from either place, a coordinate omitted or `null` being
none. Every event and media at the place moves to `into`, its tags join
`into`'s, and the place is deleted. The persons the two places show on are
refreshed in the same transaction. `POST …/repositories/{repository_id}/merge`
does the same for a repository: body `{ "into": "<id>", "name": "…", "address":
"…", "phone": "…", "email": "…", "website": "…" }`; the sources it holds become
held at `into` (a link `into` already has — same source, call number and medium
— is not doubled), its notes and tags move there, what its GEDCOM file kept
follows it, and it is soft-deleted. Both answer the kept record. A blank
`name` or a record merged into itself is a `validation_error`, an `into` of
another tree `not_found`. One audit entry is recorded (`action: "merge"`,
`entity` `place` or `repository`, `details.other_label` the name of the record
absorbed), with a version of both records, of the persons a place merge
refreshes and of the sources whose links a repository merge moves.

### Value suggestions

What the free-text fields of the entry forms suggest while the user types
(see [Common UI §4.4](ui-common.md)).

| Method | Path | Description |
|---|---|---|
| `GET` | `/trees/{tree_id}/suggestions/{field}?q=...&lang=...&limit=...&surname=...&given_names=...` | Values for `field`: `family-names`, `given-names`, `occupations`, `sources` (titles) or `places` (the tree's place names). `lang` is an interface language code; `limit` defaults to 10, 1–50 accepted. `surname` and `given_names` scope a name field (below). An unknown field or language, an out-of-range limit and a scope on `occupations`, `sources` or `places` are 400 |

The text of `q` is normalized like the reference sheets' terms (case,
accents and punctuation ignored) and must start a word of the value; a blank
`q` returns `[]`. The tree's values come first — those starting with `q`,
then the most used — and, for occupations and given names, the terms a
reference sheet answers to fill the rest of the list, in any language's
spelling, `lang`'s own first, never repeating a value the tree already holds.
Given names are single words: `"Jean Marie"` holds `"Jean"` and `"Marie"`.
Two sources with the same title, or two places with the same name, are
offered once, their uses summed.
Each suggestion is:

```json
{ "value": "Laboureur", "count": 3, "reference": true }
```

`count` is the number of persons carrying the value (citations for a
source), 0 for a term only a sheet knows; `reference` says whether a sheet
answers to the value itself, not merely to a word inside it.

**Scope.** A search form's name field suggests what the persons its other
field finds carry: `surname` and `given_names`, when not blank, keep only the
persons the person search's filters of the same names find (a substring of
the primary name, case and accents ignored). The values are then read from
those persons' primary names, `count` counts them — what the search would find
once the value is picked — and no reference term fills the list.

GraphQL's `valueSuggestions` takes the same arguments (`surname`,
`givenNames`), its `SuggestionField` being `FAMILY_NAMES`, `GIVEN_NAMES`,
`OCCUPATIONS`, `SOURCES` or `PLACES`.

### Audit log and versions

Every write to a tree leaves an audit entry, and every write that changes a
person, a place, a source or the tree's settings also stores the state it
replaced as a numbered version; the live record is the current version, never
stored. See [Data Model §5](data-model.md#5-change-history) for what is
recorded, when a state is stored and what a version holds. An export is recorded too
(`category: export`, the format in `details`), whichever surface produced
it: `GET /gedcom/export` and GraphQL `exportGedcom` alike, a GEDZIP job when
its archive is complete.

| Method | Path | Description |
|---|---|---|
| `GET` | `/trees/{tree_id}/audit?first=&after=&category=&subject_id=` | The tree's audit log, **newest first**, as a `Connection<AuditEntry>`. `category` is one of `data`, `settings`, `media`, `import`, `export`, `history`; `subject_id` keeps only the writes about one record |
| `GET` | `/trees/{tree_id}/audit/{entry_id}` | One `AuditEntry` |
| `GET` | `/trees/{tree_id}/audit/{entry_id}/changes?first=&after=` | The states the write replaced, in the order it stored them, each as `{ previous, version }` — `previous` the state replaced, `version` the one that follows it: the state the write produced, or a later one when the record changed since |
| `GET` | `/trees/{tree_id}/history/{record_type}/{record_id}?first=&after=` | A record's versions, **latest first**: its live state (`current: true`), then its stored ones. `record_type` is `person`, `place`, `source`, `repository` or `tree` (whose `record_id` is the tree's own ID). A record that does not exist and has no stored state has none |
| `GET` | `/trees/{tree_id}/history/{record_type}/{record_id}/{version}` | One `RecordVersion`, the current one included; a number past it is `404` |
| `POST` | `/trees/{tree_id}/history/{record_type}/{record_id}/revert` | Body `{ "version": 3 }`. Puts the record back as that version had it — restoring the state a deletion replaced undeletes it — and returns the restore's own `AuditEntry` (`action: "revert"`, `details.version`). A version whose `deleted` is `true` is refused with `400`: restore the one before it instead |

An `AuditEntry` is `{ id, tree_id, occurred_at, category, action, entity,
entity_id, subject, subject_id, label, details, version_count }`. `action` is
`create`, `update`, `delete`, `merge`, `import`, `export` or `revert`;
`version_count` is the number of states the write stored; `entity` names the
kind of row written (`person_name`, `event`,
`record_tag`, …) and `subject` the record the write is about (`person`,
`family`, `place`, `source`, `media`, `tree`). `label` is the subject's display
name when the write happened. `details` carries only what applies: `format`,
`file_name` and `count` for imports and exports, `scope` for an export limited
to a target (the target as JSON text, see [Export scope](#export-scope)), `count` for a tag's merge
(links moved) and delete (links removed), `event_type` for event writes,
`version` for a restore, `other_label` for the person a merge absorbed or, on a
tag merge, the label of the tag kept, `new_label` for the name a family-name
rename gave (the entry's `label` keeping the old one). A tag merge is a `delete`
of the absorbed tag.

A `RecordVersion` is `{ id, tree_id, record_type, record_id, version,
current, deleted, entry, snapshot, labels }`. `id` is the stored state's, or
the record's own for the current version. `current` marks the live record and
`deleted` a state in which the record was deleted. `entry` is the `AuditEntry`
of the write that produced the state — for version 1, the write that created
the record or the import that brought it — and `null` when none was recorded.
`snapshot` is tagged by `type` and holds the record's state, `null` for a
deleted state; `labels` is a list of `{ id, label }` naming, as they read when
the state was stored (now, for the current version), the places, sources,
persons and families the snapshot refers to by ID.

Used by: [Person History](ui-person-history.md) · [Settings §9](ui-settings.md#9-section-history) (audit log)

### Statistics

Aggregates backing the [Statistics](ui-statistics.md) page, computed on each
request from the person projections and place usages; nothing is stored.

| Method | Path | Description |
|---|---|---|
| `GET` | `/trees/{tree_id}/statistics?approximate=false&lang=en` | The tree's statistics, time series filed by year; `approximate=true` lets ages and averages use dates about, calculated or estimated; `lang` (an interface language, `en` by default) names the places' countries, regions and subdivisions; `tag` (a tag id of the tree) limits the figures about persons to the persons carrying it, the sources and the growth staying whole. 400 for another language, 404 for an unknown tree or tag |

With `tag`, the figures about persons count only the persons carrying the tag
(GraphQL `treeStatistics(treeId, approximate, language, tag)`). A union counts when one of its spouses carries
the tag, with its names, ages and lifespans counted from those spouses alone;
its end (a divorce, an annulment, or the death of its spouses) reads the
dates of every spouse, the untagged ones included, and no untagged spouse is
listed. The places are those the tagged persons' events use, each event
counted once; media are not counted. The sources and the growth
(`GET /trees/{tree_id}/statistics/growth`) stay whole. No person carrying the tag gives
the empty statistics of a tree without persons.

The response carries:

- **Counts**: `persons`, `men`, `women`, `unknown_sex`, `unions`, `places`,
  `sources`, `first_year` and `last_year` of the dated events,
  `dated_births`, `dated_deaths`, `without_parents`, `without_children`,
  `without_union`, and the distinct `surnames` and `given_names`.
- **Summaries** `{count, mean, median, std_dev, min, max}`, each figure
  `null` without a value: `lifespan` and `first_union_age` (`{all, men,
  women}`), `generation_interval` and `family_size`.
- **Rankings** `{label, count}`: the top ten `top_surnames`,
  `top_given_names_men`, `top_given_names_women`, `top_occupations`, and
  every `event_types` entry (the label an `EventType` in snake_case), most
  first; `children_histogram`, the unions by number of children.
- **Time series** filed by year, oldest first, only the years with a value
  appearing (the client groups them into periods of any width over any
  range of years, [Statistics §7](ui-statistics.md)): averages as
  `{year, sum, count}` (`age_at_death` by year of death and
  `life_expectancy` by year of birth, both by sex; `parents_age` at the
  first, last and every child; `age_at_first_union` by sex;
  `union_duration`, `children_per_union`, `birth_spacing`,
  `first_last_child_gap`, `spouse_age_gap`), and counts as
  `{year, counts}`: `events_by_year` (births, baptisms, unions, deaths,
  burials), `births_by_sex` (men, women), `mortality` (births, deaths
  before one, deaths before five, by year of birth), `births_by_month` and
  `unions_by_month` (twelve), `unions_by_weekday` (seven, Monday first).
- The age `pyramid` in five-year bands, and the `records`
  `{kind, persons, value, value2, date}`: `value` in days for an age or a
  duration and a number for a count ([Statistics §8](ui-statistics.md)).
- The notable lists (`recent_births`, `recent_deaths`, `recent_unions`,
  `oldest_possibly_alive`, `longest_lives`, `largest_families`, up to 100
  each, dates as recorded with their qualifier and calendar).
- The places: `located_places` for the heat map, `top_places` (ten, located
  or not), `unlocated_places`, the distinct `countries`, `regions` and
  `subdivisions` of the used places, and `births_by_country`,
  `births_by_region` and `births_by_subdivision` (top ten each). A place
  without coordinates is located through the
  [place dictionary](place-dictionary.md) by its label at each request, a
  bare homonym being read in the countries the tree uses most
  ([Statistics §4.1](ui-statistics.md)).

The rules each figure follows are in [Statistics §7](ui-statistics.md).

| Method | Path | Description |
|---|---|---|
| `GET` | `/trees/{tree_id}/statistics/growth` | How many persons the tree held over the days it was worked on, for the Growth tab ([Statistics §10](ui-statistics.md)). 404 for an unknown tree |

The response is `{ days, imports }`:

- `days`: `[{ date, added, removed }]`, each UTC day (`YYYY-MM-DD`) the
  number of persons changed, oldest first. `added` counts the persons
  created, imported or restored that day, `removed` those deleted or merged
  into another; the running total of `added - removed` is the number of
  persons at the end of each day, and its last value the tree's persons now.
  Computed from the persons' creation and deletion times, deleted persons
  included, and from the person versions of a restore, which clears a
  deletion time.
- `imports`: `[{ occurred_at, format, file_name, persons }]`, the tree's
  import entries of the audit log, oldest first (`format: duplicate` for a
  duplication), with the persons each brought.

### Tools

Read models backing the [Tools](ui-tools.md) page, computed on each request
from the person projections; nothing is stored.

| Method | Path | Description |
|---|---|---|
| `GET` | `/trees/{tree_id}/anomalies?tag=<tag_id>` | The tree's anomalies by rule; with `tag`, only those naming a person who carries it (a union names its spouses) or a place the events of such a person use (an anomaly naming neither is never kept), every person still examined so that a rule about a couple or a witness sees its whole family, and `persons` counting the whole tree; GraphQL `treeAnomalies(treeId, tag)`; 404 for an unknown tree or tag |
| `GET` | `/trees/{tree_id}/duplicates` | The pairs of records that may be one person, best first; 404 for an unknown tree |
| `GET` | `/trees/{tree_id}/unlocated-places` | The used places the statistics cannot locate, most used first; 404 for an unknown tree |
| `GET` | `/trees/{tree_id}/ancestry-completeness?generations=8` | Generation by generation from the tree's SOSA root, the ancestors found and missing and their key facts; `generations` counts the root's, 8 by default, from 1 to 15 (400 otherwise); 404 for an unknown tree |

**Anomalies** returns `{persons, thresholds, rules}`: the number of persons
checked, the tree's `anomaly_thresholds` the rules applied (which the page's
titles and sentences quote), and
one entry per rule that found something, in the order of the catalogue of
[Tools §3.3](ui-tools.md): `{rule, category, severity, count, items}`,
`category` being `dates`, `filiation`, `unions`, `witnesses`,
`data_quality` (shown as *records*) or `places`, `severity` `error` or
`warning`, and `items` at most 500 of the `count` found. An item is
`{persons, family_id, places, value, event_type, other_event_type, text}`:
the persons concerned as `{person_id, name}`, the subject first (none for
`place_spellings`); the union concerned; the places concerned as
`{place_id, name}` (for `place_spellings` and `distant_places_same_day`);
the figure measured, in whole years for ages and gaps in years, in days for
`siblings_too_close` and `born_long_after_father_death`, in kilometres for
`distant_places_same_day`, and a count for `repeated_union` (unions),
`unique_event_twice` (events of the type) and `child_of_several_families`
(families); the event concerned, an `EventType` in snake_case, and the event
it is compared with for `qualifier_contradiction` and
`distant_places_same_day`; and the recorded text for `unreadable_date` (the
date as typed), `godparent_sex` (the relation), `given_name_sex` (the first
given name) and `same_name_living_siblings` (the given names). GraphQL names
them `familyId`, `eventType` and `otherEventType`, a place `placeId`.

**Potential duplicates** returns `{count, pairs}`: every pair found and at
most 500 of them, best first, each `{score, reasons, first, second,
first_dates, second_dates}` — the
score out of 100, the reasons (`same_name`, `similar_name`,
`same_birth_date`, `same_birth_year`, `close_birth`, `same_birth_place`,
`same_death_year`, `same_parents`, `same_father`, `same_mother`,
`same_spouse`), both records as `SearchEntry` rows, and each record's full
birth and death dates as `{birth, death}`, each a recorded date `{value,
value2, qualifier, calendar, sort}` or `null` (birth falling back to the
baptism, death to the burial), which the rows reduce to a year. GraphQL
names them `firstDates` and `secondDates`. Pairs confirmed
distinct are left out. A pair is settled with the existing `distinct` and
`merge` operations; the rules are those of [Tools §6](ui-tools.md).

**Unlocated places** returns `[{place_id, name, count, latitude,
longitude}]`, the coordinates always `null`: the places the statistics count
as `unlocated_places`, by the same rule.

**Ancestry completeness** returns `{root, generations}`: `root` is the SOSA
root (`{person_id, name}`) or `null` when the tree has none, and then
`generations` is empty. Each generation carries `generation` (1 for the
root), `expected` (2^(generation − 1)), `found`, `with_birth`, `with_death`,
`with_union`, `living`, `implied_missing` (the ancestors missing because
their child is missing too, not listed) and `entries`: one per found
ancestor and per missing parent of a found ancestor, `{sosa, person}`,
`person` being `null` for a missing ancestor or `{person_id, name, sex,
birth, death, has_birth, has_death, has_union, living}`, the dates as
recorded with their qualifier and calendar. The rules are those of
[Tools §3](ui-tools.md).

`GET /reference/basemap` serves the country outlines the heat map is drawn
over, with the places it names: `[{iso, name, rings, cities}]`, each ring a
flat list of longitude and latitude pairs in tenths of a degree, and each
city `{name, names: [{lang, name}], lon, lat, zoom, population}`: its names
in the interface languages where they differ, its position in tenths of a
degree, the web map zoom it is named from in tenths, and its population in
thousands, ordered by zoom then population. From Natural Earth (public
domain), embedded and Brotli-compressed like the place dictionary. Only a
release changes it, but its URL carries no version, so it is served with
`Cache-Control: public, no-cache` and a strong `ETag` (a digest of the
embedded data): browsers keep it and revalidate it on each use, and an
unchanged map answers `If-None-Match` with a bodiless `304`.

### Import / export

GEDCOM and GEDZIP are read and written; GeneWeb `.gw` is read only — OxidGene
imports the format, it does not produce it. A `.gw` has no room for tags and
carries none, on import or export: an import creates no tag, and no export
writes one to a `.gw`.

| Method | Path | Description |
|---|---|---|
| `POST` | `/trees/{tree_id}/import-jobs?format=gedcom\|gedzip\|geneweb&filename=name.gw` | Stream a raw genealogy file to durable job storage and create an asynchronous import. Returns `202 { "job_id": UUID }` after the source is stored and the job is committed. `filename` is optional GeneWeb provenance metadata only. The 1 GiB limit is enforced while streaming. Workers copy the source to disposable scratch space; media extracted from GEDZIP are persisted through `MediaStore` |
| `GET` | `/trees/{tree_id}/import-jobs/{job_id}?since=version&wait=seconds` | Poll `{ phase, version, done, total, result?, geneanet_result?, error? }`; `since` and `wait` make it a [long poll](#waiting-for-a-job). File-import phases are `starting`, `parsing`, `media`, `database`, `projections`, `completed`, and `failed`; Geneanet additionally uses `people` and `matching`. A job asked to stop reads `cancelling` while it undoes its work, then `cancelled` (see the cancel row below). A completed status retains the import receipt, `result` for a file import: `{ persons_count, families_count, events_count, sources_count, places_count, notes_count, images_count, documents_count, document_pages_count, warnings }`, or `geneanet_result` for a Geneanet import, the same fields and its own. Media are counted alike for every import, over the records it stored: a record of one page is an image, any other a document, whose pages `document_pages_count` totals. GraphQL: `importJobStatus`, with the same fields in camel case; failure exposes a stable error code rather than internal details |
| `POST` | `/trees/{tree_id}/import-jobs/{job_id}/cancel` | Cancel an import and undo it. A queued job ends `cancelled` at once, unrun. A running job stops at its next unit of work — a batch of rows, a medium — and rolls back everything it wrote: its persons phase and media phase are one database transaction, projections and history entry included, and the media files it stored are deleted ([Architecture §6.3](architecture.md#63-job-cancellation)); its phase reads `cancelling` until then, `cancelled` after. Cancelling a job that has ended changes nothing. Answers `200` with the job's status, as `GET /import-jobs/{job_id}`, waiting for nothing the running job holds. GraphQL: `cancelImportJob(treeId, jobId)` |
| `POST` | `/trees/{tree_id}/export-jobs?include_notes_and_sources=bool&include_media=bool&scope=json` | Create a durable asynchronous GEDZIP export. Returns `202 { "job_id": UUID }`; archive creation and media reads run in the worker. The options are those `GET /gedcom/export` below reads for a GEDZIP; the job keeps them until its worker packs the archive, with its `scope` ([Export scope](#export-scope)), validated when the job is created. A GEDZIP takes no destination: it is standard GEDCOM 7.0 (see [GEDZIP](#gedzip)) |
| `GET` | `/trees/{tree_id}/export-jobs/downloadable` | The tree's most recently completed export whose archive can still be downloaded: `{ job_id, format, download_url, expires_at, size_bytes?, include_notes_and_sources, include_media }`, or `null` (`200`) when there is none — none completed, all past their hour, or the archive gone from storage. `size_bytes` is absent for an archive recorded without it. Lets a page that did not start the export offer it again. GraphQL: `downloadableExport(treeId)`, nullable |
| `GET` | `/trees/{tree_id}/export-jobs/{job_id}?since=version&wait=seconds` | Poll `{ phase, version, done, total, download_url?, expires_at?, size_bytes?, warnings, error? }`; `since` and `wait` make it a [long poll](#waiting-for-a-job). `download_url` and `expires_at` (RFC 3339, an hour after completion) appear together, only while the completed artifact can be downloaded. `size_bytes` is the completed archive's size. The phase ends `completed`, `failed` or `cancelled`, reading `cancelling` while a cancelled export stops. A job ended more than a day ago is gone (`404`) |
| `POST` | `/trees/{tree_id}/export-jobs/{job_id}/cancel` | Cancel an export: a queued job ends `cancelled` at once, a running one stops at its next medium, produces no archive and deletes any it stored; nothing in the tree changes. Cancelling a job that has ended changes nothing. Answers `200` with the job's status, as `GET /export-jobs/{job_id}`. GraphQL: `cancelExportJob(treeId, jobId)` |
| `GET` | `/trees/{tree_id}/export-jobs/{job_id}/download` | Stream the completed GEDZIP artifact as `application/zip` with `Content-Disposition: attachment`; returns an error while the job is incomplete. The artifact can be downloaded any number of times for an hour after completion, then answers `404` and is deleted by the workers' next maintenance pass |
| `GET` | `/trees/{tree_id}/gedcom/export?format=gedcom\|gedzip&destination=id&include_notes_and_sources=bool&include_media=bool&scope=json` | Export tree as GEDCOM text (default) or GEDZIP archive (`application/zip`: a GEDCOM 7.0 `gedcom.ged` and the media files, see [GEDZIP](#gedzip)). The text is written for `destination`, an [export destination](#export-destinations)'s id (default `generic-551`, standard GEDCOM 5.5.1); an unknown id is `400 validation_error`. It answers `{ gedcom, warnings, report }`, `report` being what writing for the destination lost: `{ destination, losses: [{ kind, record?, detail }], limits: [{ record?, detail }] }`. A GEDZIP takes no destination (`400 validation_error` with one). `include_notes_and_sources` (default `true`), when `false`, leaves out every note, every source with its citations, and every repository. `include_media` (default `true`) is read for a GEDZIP only: when `false`, the archive holds `gedcom.ged` alone, without any `OBJE` record, media link or vignette; a plain GEDCOM always writes its media's references. Either way no pointer is left dangling. `scope` (default: the whole tree) limits the export to a target, see [Export scope](#export-scope). GraphQL: `exportGedcom(treeId, destination, includeNotesAndSources, scope)`, plain GEDCOM only |
| `GET` | `/gedcom/export-destinations` | The [export destinations](#export-destinations), standard versions first, then the programs by name: `[{ id, label, version, standard, to_be_verified }]`. GraphQL: `exportDestinations` |

### Export scope

`scope` on `GET /trees/{tree_id}/gedcom/export`, on `POST /trees/{tree_id}/export-jobs`
(as a query parameter, the target as URL-encoded JSON) and as the `scope`
argument of GraphQL `exportGedcom` and `startExportJob` (a `TagTargetInput`,
the same target as the bulk tagging, see [Tags](#tags)) names the persons an
export keeps. Absent, the export is the whole tree, as before; a scope holding
no person is `400 validation_error`, and an unknown tag is `404 not_found`. An
export is not bounded to 10 000 persons.

The export keeps the chosen persons; every family one of them belongs to as a
spouse or a child, with the spouse and child links of the chosen persons only;
the events of those persons and families; the citations on them and the sources
they cite; the repositories of those sources; the media linked to a kept
person, family, event or source, with the pages of those documents and their
parents; the places of the events and media kept; the notes, vignettes and
kept GEDCOM of the records kept, except the kept GEDCOM of a record OxidGene
has no entity for, which may point at records left out. Nothing written points
at a record left out, and tags travel with the records kept. A GEDZIP packs
only the media kept. The export's audit entry names the scope in `details.scope`, over GraphQL too (`auditEntries { edges { node { details { scope } } } }`).

A scope that leaves out the tree's "Who am I?" person keeps that person's
names, so the submitter's name falls back to them as in a whole-tree export;
no record is written for that person. The kept GEDCOM of a record OxidGene
has no entity for is left out of a scoped export, while the submitter's is
kept.


A tree holds at most one queued or running import or export job at a time.
Starting another while one is active answers `409 conflict` (GraphQL
`CONFLICT`); the client waits for the running job instead.

A genealogy file is imported only through an import job: no request parses a
file or writes its records while the client waits. The UI uses durable jobs for
every file import and every GEDZIP export, regardless of size. It polls the job
after the single initiating action and automatically starts the download when
an export artifact is ready. Raw streaming, browser upload progress and
artifact downloads are HTTP transport concerns and therefore use REST.

A GEDZIP import stores every medium whose `FILE` names an entry in the archive,
thumbnails it and writes it as a held medium; one naming an entry the archive
lacks stays an unheld record and says so in `warnings`, as does a file no `OBJE`
names. A medium an OxidGene GEDZIP keeps as an extension, having no file in
the archive by design, is restored as an unheld record without a warning (see
[GEDZIP](#gedzip)). Matching folds separators, percent-encoding and case, so a
producer's `.\Media\Photo.JPG` still finds `media/photo.jpg`, and a GEDCOM 7.0
`media/my%20photo.jpg` finds `media/my photo.jpg`. A medium already
compressed (JPEG, PNG, PDF, …) is stored in an exported archive as it is,
any other deflated. The `gedcom.ged` entry has a 1 GiB limit and
each decompressed medium the ordinary 128 MiB per-file limit; media are read
sequentially. A GeneWeb `.gw` is uploaded as raw bytes because it is
ISO-8859-1 unless the file opts into UTF-8 with an `encoding:` directive, and
the switch can happen mid-file, so only the reader can decode it; `filename`
(default `import.gw`) is recorded on every family and quoted in warnings.

Used by: [Homepage](ui-home.md) (card menu import) · [Settings](ui-settings.md) (export section)

### Waiting for a job

A job status read waits for the job to change rather than the client
polling it on a timer. Every status carries `version`, an opaque string
that any visible change of the job — its phase or its counts — changes.
With `since`, the `version` last read, and `wait`, a number of seconds
(at most 25), `GET /import-jobs/{job_id}` and `GET /export-jobs/{job_id}`
answer as soon as the job's state differs from `since` or the job has
ended, and otherwise once `wait` has passed, with the unchanged state.
Without `since` or with no `wait`, they answer at once. GraphQL's
`importJobStatus` and `exportJobStatus` take the same `since` and `wait`
arguments and return the same `version`. The server waits on notifications,
never by reading the job over and over ([Architecture §6.4](architecture.md#64-waiting-for-a-job)),
and bounds how many reads wait at once: past the bound a read answers at
once, as one without `wait` does, so a client that gets its `since` back
early waits a moment before it asks again.

### Geneanet import

Backs the [Import](ui-import.md) Geneanet flow. The first three are
**not tree-scoped**: they run before the user has committed to importing
anything, which is the point — the wizard's whole design is that you find out
whether the two halves belong together before a row is written.

There is no endpoint for the wizard's step 3. Signing in and collecting the
person↔photo mapping happens inside the desktop app's login window, because
that is the only place a Geneanet session exists; what reaches the server is
its output, carried by the steps that follow.

Geneanet media enter through a desktop-only filesystem data plane. The login
WebView writes gathered bytes to a temporary directory shared with the
embedded backend. REST and GraphQL carry collection metadata, archive paths,
and a `source URL -> local path` map, but never the media bytes themselves. The
request handler copies the `.gw`, archives, and gathered media into durable
job-owned `MediaStore` keys before committing the job and returning. The worker
therefore never depends on the WebView's temporary files.

This data plane requires an explicit runtime capability. It is disabled by
default, including in the standalone server and the public GraphQL schema
constructor, and enabled only when the desktop application builds its embedded
backend state. REST and GraphQL reject archive indexing, preview, planning,
session encoding/decoding, and import before any local path is accessed when
the capability is absent. Raw `.gw` inspection remains available because its
bytes are carried in the request and it performs no filesystem handoff.

| Method | Path | Description |
|---|---|---|
| `POST` | `/geneweb/inspect?filename=name.gw` | **Step 1.** Parse a `.gw` and report `person_count`, `family_count` and `skipped_blocks`, writing nothing. Body is the raw file bytes, for the same encoding reason as the import above |
| `POST` | `/geneanet/archives` | **Step 2.** JSON `{ "paths": [...] }`. Index each data archive's ZIP central directory **in place** — nothing is extracted and no bytes are uploaded. Returns per-archive `file_count`/`image_count`, and a per-archive `error` for one that could not be read, so the others still stand. Desktop only: it takes filesystem paths, which is sound because there the server is in-process |
| `POST` | `/geneanet/session/encode` | Turn a collected session into the file the wizard saves. Returns **`application/zip`** — `session.json` plus the gathered media as files. Saved during step 3 it carries the collection and deposit sizes; saved after step 4 it carries the media too, and importing it then needs no Geneanet connection at all |
| `POST` | `/geneanet/session/decode` | Read a ZIP session or raw browser JSON collection, detected by content. Media references must resolve to archive entries; inline base64 media and missing entries are rejected. Refuses anything that is not a collection |
| `POST` | `/geneanet/session/release` | JSON `{ "paths": [...] }`. Delete the media a decoded session staged that the wizard no longer needs — closed, or reset without importing. A path the backend did not stage is ignored. Returns `204` |
| `POST` | `/geneanet/preview` | **Step 4.** Join the collected mapping onto the `.gw` and report what an import *would* do. No writes, no network. Sets `mismatch` when under 10 % of keyed references find a person, which the wizard blocks on |
| `POST` | `/geneanet/plan` | **Step 4.** List the media the server cannot produce on its own, for the login window to fetch. Same body as the preview. Under `media_fidelity: "renditions"` that is one `normal` rendition per page of every attached deposit; under `"originals"` it is each single-page deposit's download that no archive length accounts for, plus a rendition per document page to recognise it by |
| `POST` | `/trees/{tree_id}/geneanet/import` | **Step 5.** Copy every local input to durable job storage and queue the tree-and-media import. `fetched` maps source URLs to temporary filesystem paths; it never carries media bytes. Returns `202 { "job_id": UUID }` only after staging and job creation succeed. The UI then polls the common import-job status; its completed `geneanet_result` is the full Geneanet receipt: the import receipt's counts (`persons_count`, `families_count`, `events_count`, `sources_count`, `places_count`, `notes_count`, `images_count`, `documents_count`, `document_pages_count`, see `GET /import-jobs/{job_id}`), then the `links_count`, `portraits_count`, `isolated_count` and `vignettes_count` written, `isolated_people` (`[{ person_id, surname, given_names }]`, the persons created for identifications outside the tree, in creation order), and the `skipped` photos and `warnings`, one line each |

The preview and import bodies carry the `.gw` **base64-encoded** (`gw_base64`)
because they bundle it with other fields and JSON cannot hold raw bytes — the
two endpoints that send nothing else take it as a raw body instead. They also
carry `deposit_sizes`, archive paths, and the `fetched` URL-to-local-path map.

The preview, plan and import bodies carry `media_fidelity`, which decides which
bytes are kept per medium: `"renditions"` (the default when the field is
omitted) stores Geneanet's largest per-page copy and **ignores
`deposit_sizes` and `archive_paths` entirely** — no byte-length match, no
perceptual index, and no archive staged into job storage; `"originals"` stores
the uploaded files, resolving them from the archives where a length or a
content match lands and downloading the rest. Staged jobs require an explicit
fidelity value.
Media bytes are never included in these request bodies.
Metadata-only wizard requests have a **32 MiB body limit**. Session decoding
is excluded: saved sessions contain media, and have no fixed total-size cap.
The desktop streams the file to REST; the backend spools the upload to a private
temporary file and extracts media sequentially to private staging files, without
buffering the album or converting its media to base64. A session load takes one
of the two file-intake slots every upload shares, so at most two loads, uploads
or stagings run at once. Failed extraction discards its staged files.
Temporary storage must accommodate the upload and extracted media.
Staged media are the account's own photos, so they do not outlive their use:
queuing the import deletes them once copied to job storage, the wizard
releases them when it closes or reloads a session, and any left are deleted a
day after staging — or, after a restart, by the desktop backend's start-up
sweep of the application's temporary files older than a day.

GraphQL uses the same extractor and concurrency gate. Its existing base64 input
is decoded directly to a temporary file, but the GraphQL JSON/string itself is
still buffered. The desktop therefore uses streamed REST for session loading.
The GraphQL HTTP body cap is disabled only in the local-file-enabled desktop
backend; the standalone server retains its default cap and rejects session
operations. The import body has a **1 GiB limit**: it carries the same `.gw`
and collection as the preview plus the fetched-media map, which a large
account fills well past the wizard's 32 MiB.

Used by: [Import](ui-import.md) (From Geneanet tab)

### Profiles & Pedigree

Person profiles are materialized in `person_denorm`; pedigrees are assembled
per request by walking family links and joining reached people against those
profiles. See [Data Model §4](data-model.md).

| Method | Path | Description |
|---|---|---|
| `GET` | `/trees/{tree_id}/profiles/{person_id}` | Get a single person projection (full denormalized profile) |
| `GET` | `/trees/{tree_id}/profiles?first=N&after=CURSOR` | One page of the tree's person projections (full denormalized profiles), by person id, as a connection |
| `POST` | `/trees/{tree_id}/profiles/rebuild` | Force a full projection rebuild for a tree |
| `POST` | `/trees/{tree_id}/profiles/rebuild/{person_id}` | Rebuild a single person's projection |
| `DELETE` | `/trees/{tree_id}/profiles` | Drop a tree's projections (rebuilt lazily on next read) |
| `GET` | `/trees/{tree_id}/pedigree/{root_person_id}?ancestor_depth=N&descendant_depth=N` | Assemble a windowed pedigree for a root person |
| `GET` | `/trees/{tree_id}/pedigree?ancestor_depth=N&descendant_depth=N` | The same around the tree's default root: its SOSA root while that is a live person of the tree, else its first person; a tree without anyone is `not_found`. GraphQL: `pedigree` without `rootPersonId` |
| `POST` | `/trees/{tree_id}/pedigrees` | Assemble several pedigrees at once for `{root_person_ids, ancestor_depth, descendant_depth}`. Request order is preserved, a root given twice answered twice; a root that is not a live person of the tree is omitted rather than failing the batch. At most 64 roots per request |
| `GET` | `/trees/{tree_id}/pedigree/{root_person_id}/expand?direction=ancestors\|descendants&from_depth=N&to_depth=N&other_depth=N` | Expand pedigree depth (returns only new nodes/edges). `other_depth` is the depth already loaded in the opposite direction (default `0`) |

Every pedigree depth — `ancestor_depth`, `descendant_depth`, `from_depth`,
`to_depth`, `other_depth`, and their GraphQL counterparts — lies between 0 and
**10** generations, the range the pedigree view offers; anything else, a
negative GraphQL `Int` included, is a `validation_error`. REST, GraphQL and
the assistant tools ([MCP](mcp.md)) enforce the same limit. The batched
`pedigrees` checks the tree's projections once for the whole batch and
assembles its pedigrees together: one walk up and one down from all the
roots, then one read of projections, portraits and SOSA marks per step for
all the windows, so a batch costs the statements of one pedigree whatever
its number of roots, and each root gets the pedigree it gets alone.

The profile and pedigree vocabulary is identical across REST and GraphQL.

**A pedigree node carries its portrait and its SOSA mark.** `portrait` is
the node's portrait source and crop, absent when the person has none, so a
chart goes straight to the pictures; `sosa_ancestor` (`sosaAncestor`) says
whether the person is the tree's SOSA root or one of its ancestors, whatever
the window, so a chart marks them without loading the whole ancestry. Both are
read once for the whole window (once for all the windows of a batch); neither
is part of the stored projection, and the mark comes from the stored SOSA
ancestry ([Data Model §4.8](data-model.md)).

**A pedigree node carries whole events, not extracted years.** `PedigreeNode` and `PedigreeFamilyMember` expose `birth` / `death` as `ProfileEvent`s rather than a year and a place name, so nothing a client may draw is lost before it sees it: the day and month, the far end of an `Or`/`Between` range, the calendar, the place's id. `ProfileEvent` carries `date_qualifier`, `date_value2` and `calendar` beside the date, which is what lets a client render « entre 11 nov. 1691 et 20 août 1693 » rather than « entre 1691 ».

`birth` falls back to the **baptism** and `death` to the **burial**, and the fallback triggers on a missing *date*, not a missing event — a parish tree is full of empty birth stubs created to hang a source on, and one of those would otherwise mask a perfectly good "vers 1620" on the baptism. Each event keeps its own precision; there is deliberately no single "approximate" flag spanning both ends of a life. See [Tree View](ui-genealogy-tree.md) for how a client draws these.

**Projection payloads are versioned.** A row written by an older build is
treated as absent and rebuilt on first read, so missing fields cannot appear as
genuinely empty data. The internal version is not exposed. See
[Data Model §4.1](data-model.md).

Person search uses `GET /trees/{tree_id}/persons/search` and returns a paginated
`SearchResult` backed by `person_search_fts`. Supported query parameters are:

| Category | Parameters |
|---|---|
| Text and paging | `q`, `limit` (default 25, maximum 100), `offset` |
| Individual | `sex`, `surname`, `given_names`, `occupation`, `birth_from`, `birth_to`, `death_from`, `death_to` |
| Relations | `spouse_surname`, `spouse_given_names`, `father_surname`, `father_given_names`, `mother_surname`, `mother_given_names` |
| Events | `place`, `event_type`, `event_from`, `event_to` |
| Media, tags and ordering | `has_media`, `tag` (repeatable: a tag id, every one carried), `sort` (`relevance`, `name_asc`, `name_desc`, `birth_asc`, `birth_desc`) |

All supplied filters are combined with AND. Name and free-text matching is
case- and accent-insensitive, including the `Relations` filters. An empty or
missing `q` is valid: structured filters can be used alone, and no filters at
all select browse mode. The response's `total_count` is computed before `limit`
and `offset` are applied.

`q` is matched as a **prefix**, per word, on both backends: every word must
match the start of a word of one of the indexed fields (`sophie` finds
`Anne-Sophie`), in any order and in any field.
The named filters (`surname`, `given_names`, and the `Relations` group) match a
substring instead, so a filter finds a name recorded with a particle or a
compound where a prefix would not.

`sort=relevance` ranks a hit whose surname starts with the search term above one
whose given names do, and both above a match found further in; ties fall back to
ascending name order. The term is the first word of `q`, or the `surname` /
`given_names` filter when `q` is empty, so relevance is meaningful for a search
made only of structured filters. With nothing to rank by it is exactly
`name_asc`.

Each `SearchEntry` names the person's close relatives — every spouse, the
father and mother, and the number of children — so a result can be rendered
without a follow-up request. Birth and death years fall back to the baptism and
the burial when the primary event carries no date, and each year is accompanied
by its own qualifier; a client must render the two together rather than
displaying a bare year. A row whose person has a portrait carries it in
`portrait` — `{source, crop?}`, an [image source](#image-sources) and, for a
picture the client cuts itself, the region — on every list of rows the API
answers: search, homonyms, recently modified persons and kinship.

Used by: [Tree View](ui-genealogy-tree.md) (pedigree chart) · [Person Profile](ui-person-profile.md) (person detail) · [Search Results](ui-search-results.md) (search)

All mutation endpoints refresh affected projections in their write transaction.
See [Data Model §4.4](data-model.md).

### Reference Content

Read-only lookup of static reference content: occupation sheets and given-name
meanings shown on the person profile, and the [place dictionary](place-dictionary.md)
that place fields suggest from. It is not tied to a tree: `term` is the
raw free-text GEDCOM value. Matching ignores case, accents, and punctuation,
supports aliases such as gendered variants, and falls back to the first token
of a compound given name. A term matches in any language, whatever `lang`:
a Polish register's `Kmieć` returns the French sheet under `/reference/fr/`.
Keys match first, then the aliases of `lang`'s own file, then those of the
other languages in the order above, so a term two languages use for
different entries goes to `lang`'s. English holds every entry; a sheet not
yet written in `lang` is returned in English. An occupation sheet's label
names the term as records write it, followed, when `lang` does not use that
word itself, by a gloss in `lang` in parentheses: `Kmieć (paysan tenancier)`,
`Laboureur (ploughman-farmer)`. A given-name sheet covers one
name as written in one language, masculine and feminine apart: Jean,
Jeanne, Johann and Giovanni are four sheets, each with its own saint and
feast day, while spellings, diminutives and Latin forms met in records
(Jehan, Joannes) are aliases of one of them. Each given-name sheet also
says who bears the name, `male`, `female` or `both` (the name or one of its
aliases given to either sex somewhere), the same in every language's file:
the anomalies read it ([Tools §3.2](ui-tools.md)), the lookups do not return
it. Source content lives in
`assets/reference/*.json`, one file per language and data type,
is Brotli-compressed at build time ([Architecture §7.1](architecture.md)), and is decompressed and indexed once in memory —
warmed at server and desktop startup so no request pays for it.

| Method | Path | Description |
|---|---|---|
| `GET` | `/reference/{lang}/occupations?term=...` | Occupation fiche (label, summary, text) for `lang` (`fr`, `en`, `de`, `es`, `it`, `nl`, `pl`, `pt`); 404 if none |
| `POST` | `/reference/{lang}/occupations/bundle` | Ordered, deduplicated matches for `{terms: string[]}`; unknown terms are omitted |
| `GET` | `/reference/{lang}/given-names?term=...` | Given-name fiche (label, origin, meaning, text, feast day) for `lang`; 404 if none |
| `POST` | `/reference/{lang}/given-names/bundle` | Ordered, deduplicated matches for `{terms: string[]}`; unknown terms are omitted |
| `GET` | `/reference/{lang}/places?q=...&limit=...` | Place suggestions from the place dictionary, best first; `limit` defaults to 10, 1–50 accepted, 400 otherwise |
| `GET` | `/reference/basemap` | Country outlines and named places of the statistics heat map (see [Statistics](#statistics)) |

Errors use the shared envelope: an unsupported `lang`, a batch over the
limit or a `limit` out of range is `400 validation_error`, a term without a
sheet `404 not_found`. GraphQL reports the first two as `VALIDATION_ERROR`
and answers a term without a sheet with `null`.

These routes sit at `/api/v1/reference/...`, not under a tree. Used by:
[Person Profile](ui-person-profile.md) and the place fields
([Common UI §4.4](ui-common.md)).

**Place suggestions.** The text of `q` before its first comma is matched
against place names, ignoring case, accents and punctuation: the same name
first, then names starting with it, then names with a later word starting
with it. Each part after a comma must start a word of the code, subdivision,
region or country, so `q=saint, finist` narrows to one département. Among
equal matches, a place filed under today's subdivision comes first, then
current places before former ones, then shorter names. Rows with the same
label and end date — a British locality and the civil parish of that name —
are offered once, the first of them. A blank name returns `[]`. Each
suggestion is:

```json
{
  "label": "Name, 12345, Subdivision, Region, France",
  "name": "Name", "code": "12345",
  "subdivision": "Subdivision", "region": "Region", "country": "France",
  "kind": "commune", "valid_from": null, "valid_until": null,
  "successor": null, "latitude": 48.1, "longitude": -1.5, "current": true
}
```

`label` is what a place field stores. `kind` is `commune`,
`municipal_arrondissement`, `former_name`, `former_commune`, `settlement` or
`parish`; `successor` is the official code of the municipality holding a
former one's territory today. `lang` is any interface language (`fr`, `en`,
`de`, `es`, `it`, `nl`, `pl`, `pt`): the countries and British nations are
named in it, while subdivisions and regions keep their local names. The
dictionary is decompressed and indexed on the first
search, off the request workers. A physical batch accepts at most 128
terms. Clients split larger logical operations into consecutive batches and
merge every response; they never truncate terms at the limit.

### Image sources

Payloads that list pictures — the gallery bundle, portrait images — carry a
picture's **address**, never its bytes. A source is one of:

| `kind` | Payload | Meaning |
|---|---|---|
| `remote` | `url` | An address outside our control. The client fetches it directly; we never proxy somebody else's file |
| `thumbnail` | `media_id` | The thumbnail we generated, served by `GET /trees/{tree_id}/media/{media_id}/thumbnail` |
| `crop` | `vignette_id` | The region we cut, served by `GET /trees/{tree_id}/vignettes/{vignette_id}/image` |

GraphQL exposes the same three shapes as an `ImageSource` object whose `kind`
selects which of `url`, `mediaId` and `vignetteId` is set, and takes the same
shape as `ImageSourceInput`.

A held source names the resource rather than a URL because until authentication
ships no backend address may appear in UI markup — see
[Cross-cutting §7.1](cross-cutting.md). Turning it into something drawable is
the client's business, and the two platforms differ: the desktop shell serves
the picture from its own origin, and the web client resolves a whole screen's
sources through `POST /image-data` in one request and hands them to the markup
as `data:` URLs. Resolving them one at a time would be a request per portrait on
a pedigree.

The payloads do not carry the bytes inline: base64 would inflate them by a
third, put a whole album through a single JSON parse before anything could be
drawn, and deny the rendering engine every optimisation it has for images —
caching between renders or pages, decoding off the main thread, and skipping a
picture that never scrolls into view.

### Update semantics — omitted vs `null`

On every update endpoint (`PUT`/`PATCH`) and every GraphQL `Update*Input`, a
nullable field distinguishes three cases:

| Sent | Meaning |
|---|---|
| field omitted | leave the stored value unchanged |
| `"field": null` | **clear** the stored value |
| `"field": "value"` | set it |

This holds identically on both surfaces. REST gets it from the `double_option`
deserializer in `rest/dto.rs` (plain serde maps a JSON `null` to `None` for any
`Option`, which would make "clear" indistinguishable from "omitted"); GraphQL
gets it from `MaybeUndefined<T>` on the input field plus `mutation::patch`.
Non-nullable fields (a tree's `name`, a source's `title`) stay plain optionals:
omitting them leaves them alone, and `null` is rejected.

### Pagination

The collection endpoints — trees, persons, families, events, places, sources,
repositories, citations, notes, media, audit entries and their changes, record
versions — page by cursor. They accept:
- `first` (i32): number of items to return (default 25, clamped to 1–100).
- `after` (String): cursor for forward pagination.

The media library list also takes `offset` in place of `after`, so that its
numbered pager can jump to a page (see **Media library** under Media).

Their responses use a connection envelope:

```json
{
  "edges": [
    { "cursor": "...", "node": { ... } }
  ],
  "page_info": {
    "has_next_page": true,
    "end_cursor": "..."
  },
  "total_count": 142
}
```

Every other list is one of these documented exceptions:

- **Person search** (`/persons/search`, GraphQL `searchPersons`) pages a ranked
  result by `limit` and `offset` and answers `{ entries, total_count }`.
- **Ancestors and descendants** return every person within `max_depth`
  generations, and pedigrees every person within their depths: the depth is
  the bound.
- **Lists under one record** — a person's names and homonyms, a family's
  spouses and children, an event's witnesses, a source's repositories and a
  repository's sources, a medium's pages, vignettes and links, an entity's
  media — are returned whole.
- **The descent of a family name** (`/dictionary/family-names/descent`,
  `familyNameDescent`) pages its branches by `offset`, 50 at a time, and
  folds what passes its 400-line budget behind lines that ask for one
  bearer's descent ([Dictionary](#dictionary)).
- **Limited lists** — the persons recently modified, value and place
  suggestions, the reference bundles — return at most their documented limit.
- **Whole lists** — answered whole, on both surfaces, because their client
  draws or filters the whole set and each row is a handful of short fields;
  none of them carries a profile, an event list or a picture:

| List | Why it is whole |
|---|---|
| The dictionary's family names, occupations and places (`/dictionary/family-names`, `/occupations`, `/places`) | One row per distinct value with its counts, drawn as an alphabetical index the page filters as the reader types |
| The dictionary's sources at a drill-down prefix (`/dictionary/sources?prefix=`) | The level is split into groups past `SOURCE_DRILL_THRESHOLD` sources, so a level stays short |
| Usage lists (`/dictionary/…/usage`) | The persons behind one value, shown as one list under its row |
| Tree-wide media links (`/media-links` without a filter, `treeMediaLinks`) | One short row per attachment, for an export or an external client's own index |
| `/portraits` | One short row per person with a portrait |
| `/gedcom/export-destinations` | The programs an export can be written for, which `oxidged` compiles in: a fixed list the export section draws as one select |
| `/trees/{tree_id}/tags` (`tags(treeId)`) | The vocabulary, a handful of short rows the picker and the Tags tab filter as the reader types |

Batch reads (`/pedigrees`, `/portrait-images`, `/image-data`,
`/gallery-bundle`, `/relation-labels`, the reference bundles) take a list of
ids or terms and are bounded by their documented per-request maximum instead.

---

## 3. GraphQL API

Endpoint: `/graphql` using POST with a JSON body (`{"query": …, "variables": …}`)
for queries and mutations; `GET /graphql` serves GraphiQL where enabled. No subscription
contract is currently exposed.

### Schema

The executable schema is the contract's GraphQL side; this specification does
not copy it. Its source is `crates/oxidgene-api/src/graphql/`: the root
queries in `query.rs`, the mutations in `mutation.rs`, the output types in
`types.rs` and `history.rs`, and the input types in `inputs.rs`. Its SDL is
committed as [schema.graphql](schema.graphql), which `just graphql-schema`
regenerates: a guard fails while the two differ, so every schema change is
reviewed as a diff of that file. A client reads the schema through standard
introspection on `/graphql`, or browses it in GraphiQL where the deployment
enables it. Every query is limited to a depth of 16 and a
complexity of 1,000.

The schema mirrors §2 operation by operation, following these rules:

- **Names.** A REST resource or action becomes a camelCase field: a list
  query for a collection (`persons`, `families`), a singular query for one
  record (`person`, `family`), and a `create…`/`update…`/`delete…` mutation
  per write (`createPerson`); other operations take the name of the action
  (`mergePersons`, `renameFamilyName`, `startExportJob`). Arguments and fields
  are the REST ones in camelCase, and enum values are the REST values in upper
  case (`BIRTH`, `ABOUT`).
- **Arguments.** Every tree-scoped field takes `treeId`, the other
  identifiers of its REST path — the record it acts on as `id` — and its query
  parameters. A REST body becomes one `input` argument of a `…Input` type.
- **Results.** A REST list with the cursor envelope of the
  [Pagination](#pagination) section is a connection (`edges`, `pageInfo`,
  `totalCount`), paginated with the same `first` and `after`; a bare REST
  array is a list. A REST `204` is a `Boolean!`.
- **Nested records.** `Person`, `Family` and `Event` resolve their nested lists
  (names, families, events, citations, media, notes, spouses, children,
  witnesses) completely — never cut to a first page. Every nested field —
  those lists, an event's place, person and family, a source's citations
  and repositories, a tree's counts — is read in batches: the fields of a
  whole page, or of a whole level of the document, are answered by one query
  per relation, so a request costs the same number of statements for ten
  records as for a hundred. `Tree.personCount` and
  `Tree.familyCount` are counts, and `Tree.importInProgress` and
  `Tree.importJobId` and `Tree.importCancelling` are the transient job fields
  the REST tree list adds.
- **Absence.** A singular query answers `null` for a record that is absent,
  deleted or of another tree, where REST answers `404`; a tree that does not
  exist is `NOT_FOUND` on every field.
- **Updates.** An `Update…Input` field distinguishes omitted from `null` as
  described in [Update semantics](#update-semantics--omitted-vs-null).
- **Errors.** Errors use the codes of
  [Cross-cutting Rules §4.3](cross-cutting.md#43-graphql-mapping).

The rows of §2 name their GraphQL twin where these rules do not make it
obvious; introspection is authoritative. The transport differences the
[Surfaces and parity](#surfaces-and-parity) section allows are the only
operations without a twin: file uploads and direct media reads stay on REST.
GraphQL reaches the same media through `mediaDownload` and `mediaArchive`
(the URL to fetch), polls the import jobs the REST upload creates with
`importJobStatus` — waiting for a change as REST's `since` and `wait` do —
cancels them with `cancelImportJob`, and starts GEDZIP
exports with `startExportJob`, which takes the export options of
`POST /export-jobs`, so no destination.
`exportGedcom` writes plain GEDCOM only, so it takes neither `format` nor
`includeMedia`.

### Geneanet operations

`GeneanetPreviewInput` carries the same `gwBase64`, collection, deposit-size,
archive-path and `mediaFidelity` data as REST's preview and plan bodies
(`mediaFidelity` is `GeneanetMediaFidelity`: `RENDITIONS`, the default, or
`ORIGINALS`). `GeneanetImportInput` adds the source-URL-to-local-path map.
These paths are the staging handoff: GraphQL does not carry the corresponding
bytes, and the mutation copies every input to job-owned durable storage before
returning its job id. `indexGeneanetArchives`, `releaseGeneanetSessionMedia`
and paths returned from `decodeGeneanetSession` are desktop-only because they
refer to the local filesystem. The runtime capability that protects the REST
data plane also protects these GraphQL fields. A caller polls
`importJobStatus`; `result` is set for GEDCOM/GEDZIP/GeneWeb jobs and
`geneanetResult` is set for a completed Geneanet job.

---

## 4. GEDCOM Compatibility Reference

The API handles GEDCOM import/export via the `oxidged` crate (see [Architecture](architecture.md) §1), which reads any file as it is and writes conformant GEDCOM. See [Data Model §2](data-model.md#2-enums) for the rules mapping the enums onto GEDCOM.

### Round-trip fidelity

| Data | Import | Export | Notes |
|------|--------|--------|-------|
| Persons (INDI) | Full | Full | All names (multiple `NAME` records), sex, events |
| Families (FAM) | Full | Full | Spouses, children, events, `NCHI` and `FACT` (written as the family's own, in 5.5.1 and 7.0 alike, each with its `TYPE` beside its value, as an individual's attributes are), `FAMS`/`FAMC` back-links. The back-links are written in the order of the families' export numbers (`F1`, `F2`, …), and a family's `CHIL` in its children's `sort_order` then export order, so a re-import reads them back in the same order. GEDCOM 5.5.1 has two spouse slots, `HUSB` and `WIFE`: a husband and a wife take their own, a partner (or a second husband or wife) the one their sex points to, else the free one, and import reads them back as husband and wife. A third spouse cannot be written and is an export warning |
| Events with native tags | Lossless | Lossless | Every type GEDCOM 5.5.1 and 7.0 give a tag is written under it — `CONF`, `FCOM`, `BARM` and `BASM` included, each written under the tag it was read from (a `BASM` reads as a bat mitzvah whatever the sex of the person) — and read from it; [Data Model](data-model.md) §2 states the rules |
| Individual attributes | Lossless | Lossless | `CAST`, `DSCR`, `EDUC`, `IDNO`, `NATI`, `NCHI`, `NMR`, `PROP`, `RELI`, `SSN`, `TITL`, `FACT` each map to a dedicated EventType. A `TITL` keeps its text as the event's description, its `DATE` (a `FROM … TO …` period as a range), its `PLAC` and its `NOTE`. An attribute's `TYPE` beside its value (`FACT Sample fact` / `TYPE Hobby`) is kept as the event's classification and written back beside the value |
| Occupation (`OCCU`) | Split | One tag per profession, or merged for the destination | A value with multiple professions (e.g. Geneanet's `"Presales, Trainer"`) is split on `,` (each part trimmed) into one `Occupation` event per profession, with its first letter uppercased (rest left as written). Export writes one `OCCU` tag per event; a destination that reads a single profession field (Geneanet, GeneWeb) collapses them back into one comma-separated tag and reports it (see [Export destinations](#export-destinations)) |
| Name aliases (`SURN`) | Split | One `NAME` per alias, or packed for the destination | The primary `PersonName` takes its surname from the `NAME` line, not `SURN` — Geneanet packs every surname alias it knows into a single `SURN` sub-tag (e.g. `"LE NADEN,NADAM"`) instead of matching `NAME`. That value is split on `,` (each part trimmed, primary excluded) into one `AlsoKnownAs` `PersonName` per alias. Export writes one `NAME`/`SURN` structure per name; a destination that reads the first `NAME` only (Geneanet, GeneWeb) packs the other names' surnames into the primary name's comma-separated `SURN` tag and reports it |
| Adoption (`ADOP`) | Full | Full | Individual-level event. Its nested `FAMC` is the event's `related_family_id` (the adoptive family), written back as `FAMC` beneath `ADOP`; a `FAMC` naming a family the file does not hold is a warning, and the event names none. The child's own `FAMC` with `PEDI adopted` still makes them an adopted child of that family |
| Event types without a tag | `EVEN` + `TYPE`; a `MILI` as a military service | As `EVEN` + `TYPE` | Military service, civil union, separation (`SEP`, which GeneWeb writes, and the `MILI` extension are in no GEDCOM version), GeneWeb's vocabulary and `Other`. The `TYPE` is the type's label, or the description when it reads back as the same type (`PACS`); any other description is the `EVEN` line value. On import, the `TYPE` gives the type and the line value joins it in the description. Every type keeps its type, date, place and description through a 5.5.1, 7.0 or GEDZIP round trip |
| LDS ordinances (`BAPL`, `CONL`, `ENDL`, `INIL`, `SLGC`, `SLGS`) | Full, place included | As ordinance structures: `BAPL`, `CONL`, `ENDL`, `INIL` and `SLGC` under the individual, `SLGS` under the family. An event holding a description, media, an age, an agency or a cause, which an ordinance has no room for, is written as `EVEN` + `TYPE`, a child sealing's family beneath it as `_FAMC`, since GEDCOM gives an `EVEN` no `FAMC` (`oxidged` reads it back). An ordinance holding nothing of its own is written as its bare tag (`1 CONL`) in 5.5.1, and left out of 7.x, which permits no structure without a value or a substructure (GEDCOM 7.0, section 1.2) | Imported as `LdsBaptism`, `LdsConfirmation`, `Endowment`, `LdsInitiatory` (7.x `INIL`), `SealingChild` and `SealingSpouse` with their date, place, notes and citations, and no description: the temple and the status are kept GEDCOM beside the event ([Kept GEDCOM](#kept-gedcom)), and a child sealing's `FAMC` is its `related_family_id`, the family the child is sealed to, written back beneath `SLGC`. A generic `EVEN` typed `BAPL`, `INIL` and the like imports as its type. GEDCOM 5.5.1 has no `INIL` and requires a child sealing's `FAMC`, so `oxidged` writes those under the extension tags `_INIL` and `_SLGC`, and reads them back as the ordinances they stand for |
| Event tags no version defines (`MILI`, `SEP`, …), events out of their record | As events | As `EVEN` + `TYPE` | `oxidged` keeps such a tag whole, untyped, under the person or family; one that has substructures is read as a generic event: `MILI` as a military service, `SEP` (GeneWeb's, read even bare) as a separation, any other as an `Other` event described by its tag and value. Its date, place, notes, citations and media are the event's. An event of the other kind of record (a `MARR` on a person, a `PROP` on a family) is read as it is, and written as the generic event its tag names (`1 EVEN Farm lease` / `2 TYPE PROP`). Custom `_` tags and other standard tags kept aside (a second `SEX`) are kept GEDCOM ([Kept GEDCOM](#kept-gedcom)) |
| Associations (`ASSO`/`RELA`) | Full | Full | Imported as `EventWitness` rows, the relation being `RELA`, else the 7.0 `ROLE`'s phrase, else the role. Where a witness is written depends on the version of the file. In GEDCOM 5.5.1 (a `.ged` for a 5.5.1 destination), a witness is a top-level `ASSO` on the INDI record, which 5.5.1 permits only there (Gramps rejects event-nested `ASSO`), with a `RELA`, which 5.5.1 requires (`Unknown` for a witness without a relation); a family-event witness points at the family, Gramps' convention, which 5.5.1 does not permit, so the writer keeps it as `_ASSO`, which the import reads. In GEDCOM 7.x (a GEDZIP's `gedcom.ged`, and a `.ged` for a 7.x destination), each witness of an individual's or a family's event is an `ASSO` beneath that event, pointing at the witness, with its relation as the 7.0 `ROLE` and its `PHRASE` (`ROLE WITN` / `PHRASE Witness`, `ROLE GODP` / `PHRASE Godparent`, `ROLE OTHER` for a relation 7.0 has no role for), the writer making them from the relation. A witness of an ordinance (`BAPL`, `CONL`, `ENDL`, `INIL`, `SLGC`, `SLGS`), which GEDCOM 7 gives no `ASSO`, is written as in 5.5.1 (an individual's as a top-level `ASSO`, a family's sealing as `_ASSO` beneath the witness). The import reads an `ASSO` or `_ASSO` beneath an event and a top-level one, both encodings of one witness. A person is one witness of an event per relation: associations naming the same person with the same relation (compared trimmed and without case) are one witness, and with different relations they are several, so a person who is both godparent and witness of one baptism is two witnesses, written as two `ASSO` (7.0 `ROLE GODP` and `ROLE WITN`, or two 5.5.1 `ASSO` with their own `RELA`) and read back as the same two. A level-1 `ASSO` to an individual goes to the owner's baptism, else birth, else first event, after the witnesses nested in it |
| Sources (SOUR) | Full | Full | Title, author, publisher (`PUBL`), abbreviation; free-text `SOUR` citations preserved |
| Citations (with QUAY) | Full | Lossless | Page, the text quoted from the source (`DATA.TEXT`, over several lines too; on import the first `TEXT` is the citation's text, whether it sits under `DATA` or directly under a described source's `SOUR`, and further `TEXT` lines are kept GEDCOM), confidence level. `QUAY` 0, 1, 2 and 3 are `VeryLow`, `Low`, `Medium` and `High`; a citation without `QUAY` is not assessed (no confidence) and is written without one, so no assessment is invented. `VeryHigh` has no `QUAY` of its own and is written as `3` |
| Media (OBJE) | Metadata; GEDZIP also restores held bytes | Metadata in `.ged`; metadata plus stored bytes in `.gdz` | File path, format, title, description and physical medium use standard `FILE`, `FORM` (the MIME type, written in 5.5.1's vocabulary in a `.ged`: `jpg` for `image/jpeg`; as itself in a GEDZIP's 7.0), `FILE.TITL` (a record-level `TITL`, as GEDCOM 5.5 and earlier exports wrote it, is read too), `NOTE`, and `FORM.TYPE` (7.0's `FORM.MEDI`, which a GEDZIP writes, is read too) structures. Person, family, event and individual-attribute links use standard `OBJE` references — a scan documenting an `OCCU` or a `TITL` travels with its tag. A value split into several professions gives each of them the scan. A plain `.ged` never carries file bytes, and its remote and unheld media retain their original `FILE` reference. A GEDZIP embeds every stored file and rewrites its `FILE` to the archive entry, unless it is exported without its media (`include_media=false`), which leaves every `OBJE` out; a remote medium keeps its web address, and an unheld one, which only a file outside the archive could name, is kept as the OxidGene extension `_OXIDGENE_OBJE`, its path and links with it, which an OxidGene import restores (see [GEDZIP](#gedzip)) |
| Extended media metadata | OxidGene extension | OxidGene extension | `_OXIDGENE_MEDIA` is a versioned value beneath the owning `OBJE`. It preserves the original file name, structured date and calendar, document category, privacy, media place with coordinates, record timestamps, and notes attached specifically to the media, each with its tags. Each exported page of a multi-page document owns a separate `OBJE` and extension value, so different page transcripts round-trip with their page rather than collapsing into one document note. Document tags travel as `_OXIDGENE_TAG` lines beneath its first `OBJE`, not in this value; a document with no page has none to carry them, so its tags are not exported and the export warns. It also mirrors standard title, description, and physical-medium values for an exact OxidGene round trip; other readers may ignore it. |
| Identifications (vignettes) | Full | Full | A crop is GEDCOM's `CROP` (`TOP`, `LEFT`, `HEIGHT`, `WIDTH`, written `_CROP` in 5.5.1, which has none) beneath the `OBJE` link to its page: beneath the person's record for a crop naming a person, beneath the event for a crop naming an event. An event's crop names its person with an `_OXIDGENE_PERSON` pointer beneath the link, and a crop that is both the person's portrait and an event's is written both ways. A person's first link is their portrait, so a crop portrait is written first. A crop naming neither a person nor an event stays `_OXIDGENE_VIGNETTE` `x y width height` beneath the `OBJE` record of its page. On import an absent `TOP` or `LEFT` is 0, and an absent `WIDTH` or `HEIGHT` runs to the edge of the picture, which needs the picture's recorded size. A crop that names no rectangle OxidGene can hold (a document of several pages, or an edge with no size known) stays a whole-picture link, with a warning, and its `CROP` is kept GEDCOM. A crop beneath a person and the same crop beneath one of that person's events, naming them, are one identification, which carries the event. Software that knows no crops ignores them. |
| Dates (`DATE`) | Full | Full | Qualifier, range and calendar each have their own column, split by `oxidged`'s date grammar, which reads both versions' syntax in any case. A calendar escape is read before each date (`ABT @#DJULIAN@ 1700`, `BET @#DJULIAN@ 1700 AND @#DJULIAN@ 1710`) or, as some programs and earlier exports wrote it, before the whole value, and a 7.0 calendar keyword (`ABT JULIAN 1700`) too; it is written the GEDCOM 5.5.1 way, after the qualifier and before each bound, in a `.ged`, and as 7.0's keyword in a GEDZIP. A 7.0 date with a `PHRASE` reads as 5.5.1 writes it: an interpreted date (`INT 1850 (phrase)`) after a single date, a date phrase (`(phrase)`) alone, and a dual year (`30 JAN 1648/49`) or a date before the common era (`44 B.C.`) as 5.5.1 words it, so each round-trips through a GEDZIP. A date is stored in GEDCOM's canonical words (`7 SEP 1830`), its unread wording as written; a date phrase, an interpreted date, a calendar OxidGene has no value for, and a 7.0 date known only by its `PHRASE` keep their whole text |
| Places (PLAC) | Full | Full | Name + lat/lon coordinates |
| Notes (NOTE) | Full | Full | Inline and referenced notes, as many per person, event or attribute as the file holds, each written back. In a `.ged`, any text too long for one GEDCOM line, the OxidGene extensions' included, continues on `CONC` lines, never split beside a space or before a combining mark (readers trim a `CONC` value), so that no line, level and tag included, exceeds GEDCOM 5.5.1's 255 characters. A GEDZIP's GEDCOM 7.0 has neither the limit nor `CONC`. A line break of any kind (`CR LF`, `CR`, `LF`) is a `CONT` line. On import, a value continued on `CONC` or `CONT` lines is read whole wherever it stands, and the spaces opening a `CONC` value are kept, so files whose writer split beside a space read back word for word. A `NOTE @N1@` pointer to a 5.5.1 note record, or a 7.0 `SNOTE`, imports the text of the record it points at; a pointer to a record the file does not hold is left out with a warning naming it. Each note read from one record keeps that record's group (`Note.sharedGroup` in GraphQL, `shared_group` in REST, read-only). An export writes the notes of one group that say the same thing as one shared record again — `NOTE` records in 5.5.1, `SNOTE` in 7.0 — and each owner points at it; notes of different text in one group become one record per text, and a note of its own stays inline |
| Cause (CAUS) | Full | Full | On any event |
| Age at event (`AGE`) | Full | Full | On an individual event or attribute (each profession a split `OCCU` becomes keeps it), stored in canonical form (`34y`, `< 1y 6m`, `CHILD`). A value outside GEDCOM's age grammar costs that age, not the file: one OxidGene reads as an age (`1y6m`, `child`) is stored in GEDCOM's form, and anything else — free text such as `2 AGE majeur` — is left out and reported as one warning naming its record (never its value); an empty `AGE` says nothing and is ignored. Written in GEDCOM 5.5.1's form in a `.ged`, in 7.0's in a GEDZIP, whose keyword ages are durations with the keyword as their `PHRASE` (`< 8y` / `PHRASE CHILD`); the import reads such an age back as its keyword |
| Repositories (`REPO`) | Full | Full | Name, address (several lines), first phone, email and website, notes |
| Source repository citations (`SOUR.REPO`) | Full | Full | Each becomes a link with its call number (`CALN`) and medium (`MEDI`, written in 5.5.1's lower case; a medium without a call number goes under an empty `CALN`). A citation without a pointer names a repository by its text — the line's, else its notes' — created once per distinct text; a pointer to a record the file does not hold is a warning. Several `CALN` under one citation are one link each, every one with its own `MEDI`; a citation's own `NOTE` is kept GEDCOM. A medium GEDCOM has no value for (`OTHER`) is not written as `MEDI`; its `MEDI` and `PHRASE` are kept GEDCOM beneath the call number |
| Spouse ages (`HUSB.AGE`, `WIFE.AGE`) | Full | Full | On a family event, the age of the family's `HUSB` and `WIFE`; written back under the slot each spouse is exported in, a spouse without a slot being a warning |
| Agency (`AGNC`) | Full | Full | On an individual event or attribute, and a source's `DATA.AGNC` |
| Child pedigree (PEDI) | Full | Full | Biological, Adopted, Foster. A `FAMC` or `FAMS` that the family does not list back (written on the individual's record only) still links the individual to the family, with a warning: a child becomes a `FamilyChild` (its `PEDI` kept), a spouse a `FamilySpouse` with the role of their sex |
| Nicknames (`NICK`) | Full | Full | On the name that carries it. A non-primary `aka` name that only restates the primary name (or names nobody) to carry a `NICK` imports as a `Byname` holding the nickname alone, as the person form records one; a byname exports as such an `aka` name |
| Restriction (`RESN`) | Person and family privacy | Person and family privacy | `confidential` or `privacy` on an `INDI` or `FAM`, in any case and among several comma-separated values, makes the record `Private`; `locked`, another value or no `RESN` leaves it `Default`. A `Private` person or family is exported with `RESN confidential`; `Public` has no `RESN` value and comes back as `Default` |
| Submitter (`SUBM`) | The record `HEAD.SUBM` points at fills the tree's submitter settings that are empty — never one already set; `Not Provided` is no name. The other `SUBM` records are kept, as records of the tree ([Kept GEDCOM](#kept-gedcom)) | One record, pointed at by `HEAD.SUBM` | GEDCOM 5.5.1 requires a submitter. The record's `NAME` is the tree's submitter name, else the display name of its "Who am I?" person (`self_person_id`), else `Not Provided` (Gramps' wording). `EMAIL` and the multi-line `ADDR` are written only when set. A duplicated tree carries the settings over |
| Header charset | Declared `CHAR` decoded | `CHAR UTF-8`; UTF-8 with a byte-order mark in a GEDZIP | A `.ged` is decoded by what its bytes hold first — a byte-order mark, a UTF-16 pattern, valid UTF-8 — then by the character set `HEAD.CHAR` declares: ANSEL, ASCII, ISO-8859-1 or -15, IBM PC, Macintosh, and `ANSI` as Windows-1252, which also stands in for a declaration the bytes contradict (8-bit bytes under `CHAR ASCII`) — so files from GEDCOM 5 software keep their accents and decoding never fails. A `.ged` export is UTF-8 and declares it, a byte-order mark first only for a destination that wants one (PAF, Ancestral Quest); GEDCOM 7.0, always UTF-8, has no `CHAR`, and a GEDZIP's starts with the byte-order mark it recommends |
| GEDCOM version | 5.5.1 + 7.0 + 7.1 | 5.5.1 in a `.ged` (7.0 for a destination that reads it), 7.0 in a GEDZIP | oxidged auto-detects on import; the export is conformant, `oxidged`'s writer repairing as extension structures (`_TAG`) what the version does not permit. See [Export destinations](#export-destinations) and [GEDZIP](#gedzip) |

### Export destinations

A plain GEDCOM is written for the program that will import it, its
*destination*: one of the export profiles `oxidged` compiles in, which `GET
/gedcom/export-destinations` (GraphQL `exportDestinations`) lists — the
authoritative list, in the order a client shows it:

- **Standard** destinations (`standard: true`): `generic-551`, GEDCOM 5.5.1,
  the default, and `generic-70`, GEDCOM 7.0 as a `.ged`. They reshape
  nothing: what OxidGene builds is written as the version says.
- **Programs**, by name (`label`, the program's own name): Geneanet,
  Ancestry, Heredis, Gramps, …. Each profile chooses the version (5.5.1 for
  most, 7.0 for Family Historian and MacFamilyTree), the line layout and a
  byte-order mark, and folds what OxidGene holds into the standard
  structures the program reads: several occupations into one for Geneanet,
  the other names' surnames into the primary name's `SURN`, a custom fact as
  an `EVEN`, an address into the place, a witness as the program records
  one. The file stays conformant GEDCOM.
- `to_be_verified` marks the programs whose profile follows published and
  observed behaviour, much of it dated, and is to be re-verified against
  their recent versions (`oxidged`'s tier 1); `version` is the version
  written.

**Tags do not travel to Geneanet or GeneWeb.** Those two profiles have no
room for OxidGene's tag extensions, so the export does not write them as
tags: every `_OXIDGENE_TAG`, `_OXIDGENE_PLACE` and note tag is taken out of
its record, and its text is kept as a `NOTE` where the record takes one,
otherwise it is left out. Each one is reported in the export's `losses`, as
`as_text` or `omitted` respectively. The other destinations, the standard
ones included, write the tags as tags.

The OxidGene export builds its records as for any `.ged` (GEDCOM 5.5.1),
then `oxidged`'s `Exporter` writes them with the destination's profile.
Every fold that loses something — a type, a structure, a link made text —
and every limit of the program the file exceeds (a size, a number of
persons) is returned in the export's `report`: its `losses`, each with its
`kind` (`merged`, `moved`, `relabelled`, `added`, `as_text`, `omitted`,
`reordered`, `recoded`, or `other` for a kind a later `oxidged` adds), the
cross-reference of the record it was in (`@I12@`) and `oxidged`'s
description, in English; and its `limits`. The folds that lose nothing (a
value escaped, records reordered) and the writer's conformance repairs are
not reported. A standard destination reports nothing. The credentials a
dataset may hold are left out, as `oxidged` does by default; OxidGene holds
none. The header names OxidGene as the file's producer (`HEAD.SOUR
OXIDGENE`, with its version and its name), and `oxidged`, which wrote the
file, beneath it (`HEAD.SOUR._OXIDGED`, declared in `HEAD.SCHMA` in 7.0).

The destination is a request option: nothing stores it, and the audit entry
of the export records its format only.

### GEDZIP

A GEDZIP (`.gdz`) is a ZIP archive holding a `gedcom.ged` and the media
files it names. The format is part of FamilySearch GEDCOM 7 and holds
GEDCOM 7.x, so OxidGene writes the archive's `gedcom.ged` as GEDCOM 7.0, the
latest released version and the one GEDZIP readers read, while a plain
`.ged` stays GEDCOM 5.5.1, the version every genealogy program and site
reads. Both are one export: the records are built once, as for a `.ged`, and
`oxidged`'s writer converts them to 7.0 as the FamilySearch migration guide
says — note records become `SNOTE`, a `RELA` a `ROLE` with its `PHRASE`,
file formats media types (`image/jpeg`), dates and ages 7.0's grammar —
then checks and repairs them for 7.0 as its 7.0.18 release states it, so the
text conforms to the specification. The records an export builds already
keep that release's pointer rules: a source holds no media, a medium no
citation and every note is written in place, so no cycle of source,
multimedia and shared note records can form. It is UTF-8 and starts with a
byte-order mark.

- **Files.** A medium OxidGene holds is packed under `media/{id}.{ext}` and
  its `FILE` names that entry: a relative path with `/` separators, no `..`
  and nothing to percent-encode. A remote medium keeps its web address
  (`http`, `https` or `ftp`). GEDZIP permits no other `FILE` — a local
  `file:` URL is forbidden in one, and a relative path names an archive
  entry — so a medium whose bytes OxidGene does not hold, which only its
  producer's path names, cannot be an `OBJE` there. It is kept as an
  [`_OXIDGENE_OBJE`](#_oxidgene_obje) extension record instead, with its
  path, title, notes and OxidGene extensions (its identifications
  included), and each link to it as an `_OXIDGENE_OBJE` pointer: the text
  stays conformant, other programs ignore the extension, and an OxidGene
  import restores the medium, unheld, with its links, as the `.ged` of the
  same tree does.
- **Extensions.** The header declares OxidGene's extension tags in
  `HEAD.SCHMA`, each with the URI of its documentation below, which makes
  them documented extension tags.
- **No destination.** A GEDZIP is the complete, standard GEDCOM 7.0 export:
  no export profile of `oxidged` writes a GEDZIP, so it takes no
  [destination](#export-destinations), which shapes a plain GEDCOM.
- **Nothing to say.** An attribute with no value and nothing beneath it (a
  GeneWeb `#occu` alone) has no GEDCOM 7.0 form, which requires its value,
  and is not written.

An import reads either version, so a GEDZIP OxidGene wrote restores what a
`.ged` of the same tree restores, its packed media with their bytes besides,
and does not look in the archive for the files of the media it kept as
extensions;
archives earlier versions wrote, whose `gedcom.ged` is 5.5.1, import as
before.

### OxidGene extensions

OxidGene writes three extension tags beneath the `OBJE` record of each page
it exports, a GEDZIP two more for the media it keeps, `_OXIDGENE_PERSON`
beneath the link of an event's crop, and `_OXIDGENE_TAG` beneath the records
that carry tags. Other software may ignore them; an OxidGene import reads
them back.

#### `_OXIDGENE_MEDIA`

A versioned JSON value: what a page says about itself — its original file
name, its timestamps and its own notes, each note with its tags. See *Extended media metadata* above.

#### `_OXIDGENE_DOC`

A versioned JSON value: the document a page belongs to, by a token that is
stable within the file, the page's index and the document's page count;
page one carries what the document says about itself. It is what
reassembles a multi-page document on import.

#### `_OXIDGENE_VIGNETTE`

A crop of the page that names neither a person nor an event: its pixel
rectangle `x y width height`. A crop naming a person or an event is a `CROP`
beneath its link instead (see *Identifications (vignettes)* above).

#### `_OXIDGENE_PERSON`

Beneath the `OBJE` link of a crop that names an event, the person the crop
identifies, as a pointer to the individual's record (`_OXIDGENE_PERSON @I1@`).
It is absent when the crop names no person. A crop beneath a person's own
record needs none, the record being the person.

#### `_OXIDGENE_OBJE`

In a GEDZIP only, a medium whose file OxidGene does not hold (see
[GEDZIP](#gedzip)). As a record (`0 @M1@ _OXIDGENE_OBJE`), it stands for the
`OBJE` record a `.ged` writes: its substructures are that record's — `FILE`
with the producer's path exactly as stored, `FORM` with its media type and
`MEDI`, `TITL`, `NOTE` — and the extensions above. As a substructure of a
person, a family or an event, its pointer payload is a link to such a
record, written where the `OBJE` link stands in a `.ged`, with the `CROP` and
the `_OXIDGENE_PERSON` the link it stands for holds.

#### `_OXIDGENE_POS`

Beneath an `_OXIDGENE_OBJE`, its position, from 0: a record's among the
`OBJE` records, counting the kept ones; a link's among the `OBJE` links of
the structure holding it. A reader keeps extensions apart from the standard
structures, and the order says which picture is a person's portrait, so the
import puts each one back at its position.

#### `_OXIDGENE_PLACE`

A record of its own, for a tagged place that no written event names: a place
used by no event, by a document alone, or only by events of a person or family
the export does not write (a soft-deleted one, say), has no `PLAC` to carry its
tags. It is
`0 @P1@ _OXIDGENE_PLACE`, holding a `PLAC` with the place's name, its `MAP`
coordinates when it has them, and the `_OXIDGENE_TAG` lines of the place beneath
the `PLAC`. An import reads it as a place of that name, which joins a place of
the same name already read, and gives the place those tags and coordinates.
Other software may ignore it.

**Place identity on import.** One name gives one place, whichever record names
it: an event's `PLAC`, an `_OXIDGENE_PLACE` record, or the place of a document
(`_OXIDGENE_DOC` metadata). The name is compared trimmed, so names that differ
only in leading or trailing blanks are one place. Each place takes the union of
the tags its records carry, and the first non-empty latitude and longitude read
for it; a later record does not replace them. The same rule applies to a `.ged`
and to a GEDZIP.

#### `_OXIDGENE_TAG`

One tag beneath each `INDI` and `FAM` record that carries tags, beneath the
first `OBJE` of a document that carries them, beneath each `SOUR` record that
carries tags, beneath each citation's `SOUR` pointer that carries them (the
structure under the event, person or family that cites the source), beneath
each event or attribute structure that carries them, an LDS ordinance included,
beneath each `REPO` record that carries tags, beneath each `PLAC` structure of a
place that carries tags (every `PLAC` of the place, since GEDCOM repeats the
place in each event), and beneath each inline `NOTE` structure that carries
tags, or each shared note record (`NOTE` in 5.5.1, `SNOTE` in 7.0) once, with
the union of the tags of the notes it stands for. A pointer to a shared note
carries no tag line. Its value is a versioned JSON value,
`{"v":1,"label":…,"color":…}`, the label as entered and the colour as its index
in the tree's palette (0 to 11). Labels that fold alike are one tag on import,
the first spelling and colour read winning; the line is read into the tag
vocabulary and is not kept as untyped GEDCOM, so a re-export writes each tag
once. On import a place takes the union of the tags of its `PLAC` structures,
and each row of a shared note takes the tags of its record. A tag line beneath
any other structure is kept as it was read. The tags of the media transcripts
(`_OXIDGENE_MEDIA` notes) travel in their note entry instead, as a `tags` array
of the same values, absent when the note carries none. A line of another version
than `1`, wherever it stands (a transcript's `tags` included), is skipped with a
warning. A place no written event names (see `_OXIDGENE_PLACE`) is written as an
`_OXIDGENE_PLACE` record instead.

### Kept GEDCOM

OxidGene models the genealogy, not every structure GEDCOM can hold. What a
file holds that the model has no place for — identifiers (`REFN`, `RIN`,
`UID`, `EXID`, `AFN`, `RFN`), change and creation dates (`CHAN`, `CREA`),
name variants (`FONE`, `ROMN`, `TRAN`), an event's address and contacts, a
place's format, variants and notes, a citation's `EVEN`/`ROLE` and
`DATA.DATE`, a source's `TEXT` and `DATA.EVEN`, `SDATE`, `NO`, `RESN` on an
event, the records OxidGene has no entity for (`SUBN`, the submitters other
than the header's, extension records), and every extension tag — is kept as
it was read and written back on export, so that an import followed by an
export loses nothing but the header, which every export writes anew.

On oxidged's every-structure conformance files this is checked by
`crates/oxidgene-gedcom/tests/conformance_round_trip.rs`; the losses it still
accepts are listed, with their counts, in
`crates/oxidgene-gedcom/tests/fixtures/conformance/known_gaps.tsv`.

**What is kept.** At import, OxidGene writes back what it understood — the
records it built, exported for the file's own version (`generic-551` for a
5.5.1 file, `generic-70` for a 7.x one) — and compares each record of the
file with the record exported for the same entity. The comparison walks the
substructures:

- A substructure is *understood* when the exported record holds a
  substructure with the same tag at the same place, paired first by the
  record its pointer names (resolved to the entity's id on both sides),
  then, for the ones left, in order. Its value is not compared: OxidGene
  normalises values (a date in GEDCOM's words, a name's pieces), and a
  value it maps wrongly is a defect of the mapping, fixed there.
- Paired substructures are compared in turn; what an understood structure
  holds that its partner does not is kept beneath it.
- A substructure with no partner is kept whole, its substructures with it.
- An association (`ASSO`, `_ASSO`) with no partner is understood only when
  the exported record associates the same entity, at any depth of it, by a
  pointer of its own (`ASSO` or `_ASSO`): the export then writes the witness
  itself. An association the export writes nowhere for its entity is kept
  whole, with what it holds, since the import could not make it a witness (an
  individual's `ASSO` on a person without events, a 7.0 `ASSO` on a `FAM`, an
  association to a record that is not an individual). The exported record's
  associations are collected once per record, so the rule holds wherever the
  export writes the witness. A family's witnesses count as associated with the
  family too: a 5.5.1 export writes a family-event witness beneath the witness's
  own record, so an `ASSO` beneath the family's event naming that witness is
  understood. An association paired in order with another one (the pass above)
  keeps what it holds beneath it, as any structure does.
- The structures the import reads as something else are understood whatever
  the export holds, and never kept: the `TYPE` beneath an association, which
  names the kind of record its pointer names; the event tags the import reads
  as generic events (`MILI`, `SEP`, an undefined tag with substructures, an
  event of the other kind of record); an `AGE`, which is stored or left out
  with a warning, an empty one saying nothing; the `_GWDEATH` a GeneWeb death
  reason is read from; a repository citation (`REPO`) holding several call
  numbers (`CALN`) is compared as one citation per call number, since the export
  writes one `REPO` per call number, each with what it holds beneath its call
  number (a medium and its phrase); an inline `OBJE`, which the import reads
  as a medium of its own, written as an `OBJE` pointer with nothing beneath the
  link; and a `TEXT` beneath a citation (a pointerless `SOUR` beneath a record,
  not a source record). GEDCOM 5.5.1 gives a citation no `TEXT` of its own, so
  the import reads it as the citation's text and the export writes it beneath
  the citation's `DATA`: the `TEXT` is understood when the exported partner
  holds a `DATA` with a `TEXT`, and kept whole otherwise. The citation then
  carries its text once, with no `_TEXT` beside it.
- Not understood, so compared like any structure: the LDS ordinances (`BAPL`,
  `CONL`, `INIL`, `ENDL`, `SLGC`, `SLGS`), each written under its own tag, so
  an unpaired one is kept whole and a paired one keeps what it holds beneath
  it (its `TEMP` and `STAT`); and
  the `RESN` of a person, a family or an event, which OxidGene's privacy does
  not model: a `RESN` the export writes (`confidential`) pairs with the
  file's, and any other is kept whole.
- A `@VOID@` pointer (GEDCOM 7.0: `1 CHIL @VOID@`, a pointer to nothing)
  names no entity. It is kept as it is, with what it holds, and merged back
  as `@VOID@`. It never pairs in the first pass, which pairs by entity; in
  the second it pairs in order with an exported `@VOID@` of the same tag.
- The header is not compared.
- A record other than the header and the trailer with no xref (`0 _CUSTOMREC`,
  an extension record) has no entity to compare: it is kept whole.
- A record with an xref whose entity the export writes nothing for (a shared
  `NOTE` that no structure points at, which OxidGene imports as no note)
  is kept whole, not compared.

What is kept is stored per entity — a person (`INDI`), a family (`FAM`), a
source (`SOUR`), a repository (`REPO`), a medium (`OBJE`), the tree's
submitter (`HEAD.SUBM`'s `SUBM`) — as the kept substructures, each with
the path that leads to it: the *anchors*, the understood structures above
it, each named by its tag, the entity its pointer names, and its position
among the exported substructures with that tag (for a pointer, among those
naming the same entity, so that two citations of one repository are told
apart). A record OxidGene has no
entity for, a record without an xref, and a record whose entity the export
writes nothing for are kept whole, for the tree, as a record of their own
(a row whose entity id is the record's entity, or a fresh one for a record
with no entity). A record keeps its tag, which is what the export's choices
act on: a kept note or source is left out with the notes and sources. A
pointer is stored as the entity it names, never as the file's identifier,
which an export does not reuse; a pointer to a record the file does not hold
is dropped, and `@VOID@` is stored as it is. The tree's
submitter is kept as a row even when nothing beneath it is, so that a pointer
to it kept elsewhere in the file finds the submitter on export.

**Export.** Every export, `.ged` and GEDCOM inside a GEDZIP alike, puts
what is kept back before it is written: beneath each record, each kept
substructure is placed under its anchors — a pointer anchor found by the
entity it names and its position among the substructures naming that entity,
another by its position among the substructures with its tag — and the
records kept for the tree are added. A substructure kept whole takes the
place of a bare one of its tag the export writes beneath the same anchor
(`1 BAPL` with neither value nor substructure), and otherwise follows the
rest: an ordinance read from GEDCOM 7.x with a status OxidGene has no value
for is kept whole, since its event, holding nothing, is a structure 7.x
leaves out, and in a 5.5.1 export the kept ordinance stands for that bare
event instead of doubling it. A kept record that had an
xref is given an identifier of its own, `@K1@`, `@K2@`, …; one that had none
is written without an xref. A kept substructure whose anchor is no longer exported (an
event deleted since) or whose pointer names an entity no longer in the tree
is left out, and the export's warnings count what was left out. What an
export's choices leave out goes with it, without a warning: a kept row of a
note, a source, a repository or a medium the export does not write, and each
kept node tagged `NOTE`, `SNOTE`, `SOUR` or `REPO` when the notes and sources
are left out, or `OBJE` when the media are. The records then go through
`oxidged`'s writer like the rest, so a structure the destination version does
not permit — a 7.0 `EXID` exported as 5.5.1, a note beneath a `REFN` — is
converted or kept as an extension structure, and the file stays conformant.

What is kept follows its entity: a duplicated tree carries it; a merge
keeps the merged record's beneath the record kept; a purged tree leaves
none. Editing an entity in OxidGene leaves what is kept beneath it.

### GeneWeb `.gw`

A `.gw` is read by the `geneweb` crate, which converts it to the same `oxidged`
model a `.ged` is read into; the mapping above then applies unchanged. A `.gw`
holds no GEDCOM structures of its own, so nothing is kept from it: the GEDCOM
the conversion produces is the model's, not the file's. What the
conversion produces, and so what a `.gw` imports as:

| GeneWeb | Imported as |
|---|---|
| Witness of a `pevt` or `fevt` event | A witness of that event only, whether the file defines the witness elsewhere, inline, or nowhere else |
| Witness on a `fam` line | A witness of the union's marriage, created for it when the line gives no other detail, and joined to a `fevt` marriage that replaces the line's |
| Death reason (`k`, `m`, `e`, `s`, died young, presumed dead) | The death event's cause — `killed`, `murdered`, `executed`, `disappeared`, `died young` or `presumed dead`, as the `geneweb` crate words it — also when a `pevt` death replaces the line's, and exported as `CAUS`. The crate carries it as a `_GWDEATH <reason>` extension beneath the death; any GEDCOM holding one imports the same way, joining a cause the file states |
| Title `[name:title:domain:start:end:nth]` | A `NobilityTitle` described `title, domain, nth`, dated `FROM start TO end`, with the name it is held under as its note; the domain is no place |
| Adoptive and foster parents (`rel`) | A family of their own with the child as `Adopted` or `Foster`; an adoption also gives the child an `Adoption` event |
| Godparents (`rel`) | Witnesses of the child's baptism, else birth, else first event, as `GODF` and `GODM` |
| Other `rel` relations | Witnesses of the child's baptism, else birth, else first event, labelled with the relation |
| Every `#nick` | The first on the primary name, each further one a `Byname` |
| `#apriv`, `#semipub` | A `Private` person |
| Text date `0(…)` | The parenthesised text as the date value, with no sort date |

Reading is lenient: a malformed block is skipped whole, up to its end marker or
the next block for a `fam`, and reported as one warning naming its line.

### Not currently imported

Nothing else a GEDCOM file holds is dropped: what the model has no place for
is [kept](#kept-gedcom) and written back on export — among it the submitter
records other than the one `HEAD.SUBM` points at, the `SUBM` pointers of
individual and family records (`INDI.SUBM`, `FAM.SUBM`), a note on a
repository citation, the religion of a single event and custom tags. What is
not read is what the export replaces: the header, which every export writes
anew, an `AGE` that is not an age (left out with a warning naming its record)
and an empty `AGE`, which says nothing.

The import warnings name only what the import repaired, could not resolve, or
left out.
