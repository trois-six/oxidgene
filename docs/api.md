---
type: "API Specification"
title: "API Contract"
description: "REST and GraphQL contract for OxidGene, including endpoints, pagination, and payload conventions."
tags: [oxidgene, specification, api, contract]
generated: { by: claude-code/claude-opus-5-5, at: 2026-10-02T01:23:54Z }
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
tests, and this specification in the same change.

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

The REST API exposes its OpenAPI 3.1 description at
`GET /api/v1/openapi.json`. The build script generates the document from the
Axum router AST on every API build, including nested and merged routers, so its
paths, HTTP methods, operation identifiers, and path parameters track the
compiled REST surface. The document also defines the shared error envelope.

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

[App Settings §8](ui-app-settings.md) shows the current build's URLs, the
desktop token, and `curl` examples for the surfaces that build serves.

## 2. REST API

Base path: `/api/v1`.

### Trees

| Method | Path | Description |
|---|---|---|
| `GET` | `/trees` | List trees (cursor-paginated). List nodes add transient `import_in_progress: bool` and `import_job_id: UUID?` fields from the job queue (GraphQL: `importInProgress` and `importJobId` on every `Tree`); these are not persisted tree fields and disappear once the job completes or fails |
| `POST` | `/trees` | Create a tree; a blank `name` is a `validation_error` |
| `GET` | `/trees/{tree_id}` | Get a tree |
| `PUT` | `/trees/{tree_id}` | Update a tree (incl. `sosa_root_person_id` and `self_person_id`, which must name persons of the tree — another tree's person is `not_found`); a blank `name` is a `validation_error` |
| `DELETE` | `/trees/{tree_id}` | Soft-delete a tree |
| `POST` | `/trees/{tree_id}/duplicate` | Duplicate a tree (deep copy) |

Used by: [Homepage](ui-home.md) (tree list, create, duplicate, delete)

### Persons

| Method | Path | Description |
|---|---|---|
| `GET` | `/trees/{tree_id}/persons?search=` | List persons (cursor-paginated); `search` keeps those with a given name, surname or nickname containing it (GraphQL `persons(search:)`) |
| `POST` | `/trees/{tree_id}/persons` | Create a person |
| `GET` | `/trees/{tree_id}/persons/search` | Server-side person search with structured filters, sorting, and offset pagination (see below) |
| `GET` | `/trees/{tree_id}/persons/recently-modified?limit=` | The persons modified most recently, newest first, as `SearchEntry` rows (see below) |
| `GET` | `/trees/{tree_id}/persons/sosa/{number}` | Resolve a SOSA number to a person (relative to `Tree.sosa_root_person_id`) |
| `GET` | `/trees/{tree_id}/persons/{person_id}` | Get a person (with names, events, families) |
| `GET` | `/trees/{tree_id}/persons/{person_id}/detail-bundle` | Load the bounded read model for the person profile |
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
all records in the tree.

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

An event's `age` (GEDCOM `AGE`) is accepted in GEDCOM's syntax, read
leniently (`1y6m`, `34 Y`, a bare `34`), and returned in canonical form
(`1y 6m`, `34y`); a value that is not an age, or any age on a family event
(whose spouses' ages are recorded per spouse), is a `validation_error`. `age`
and `agency` follow the update convention: omitted keeps, `null` clears — on
both surfaces. A source's `agency` (`SOUR.DATA.AGNC`) likewise.

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
| `GET` | `/trees/{tree_id}/places` | List places (cursor-paginated, searchable) |
| `POST` | `/trees/{tree_id}/places` | Create a place; a blank `name` is a `validation_error` |
| `GET` | `/trees/{tree_id}/places/{place_id}` | Get a place |
| `PUT` | `/trees/{tree_id}/places/{place_id}` | Update a place; a blank `name` is a `validation_error` |
| `DELETE` | `/trees/{tree_id}/places/{place_id}` | Delete a place |

### Sources

| Method | Path | Description |
|---|---|---|
| `GET` | `/trees/{tree_id}/sources` | List sources (cursor-paginated) |
| `POST` | `/trees/{tree_id}/sources` | Create a source; a blank `title` is a `validation_error` |
| `GET` | `/trees/{tree_id}/sources/{source_id}` | Get a source |
| `PUT` | `/trees/{tree_id}/sources/{source_id}` | Update a source; a blank `title` is a `validation_error` |
| `DELETE` | `/trees/{tree_id}/sources/{source_id}` | Soft-delete a source. With `?only_if_unused=true` the source is kept if any citation, note, media link or repository link still points at it — `204` deleted, `200` kept |

### Repositories

| Method | Path | Description |
|---|---|---|
| `GET` | `/trees/{tree_id}/repositories` | List repositories (cursor-paginated) |
| `POST` | `/trees/{tree_id}/repositories` | Create a repository; a blank `name` is a `validation_error`; `address`, `phone`, `email`, `website` optional |
| `GET` | `/trees/{tree_id}/repositories/{repository_id}` | Get a repository |
| `PUT` | `/trees/{tree_id}/repositories/{repository_id}` | Update a repository: omitted keeps, `null` clears; a blank `name` is a `validation_error` |
| `DELETE` | `/trees/{tree_id}/repositories/{repository_id}` | Soft-delete a repository. With `?only_if_unused=true` it is kept while a live source is held there or a note is about it — `204` deleted, `200` kept |
| `GET` | `/trees/{tree_id}/repositories/{repository_id}/sources` | The live sources it holds: each link with its `source` |
| `GET` | `/trees/{tree_id}/sources/{source_id}/repositories` | A source's links to the live repositories holding it, in order |
| `POST` | `/trees/{tree_id}/sources/{source_id}/repositories` | Link a source to a repository of the tree: `repository_id`, optional `call_number`, `media_type` (`SourceMediaType`), `sort_order` (after the others when omitted). One link per call number |
| `PUT` | `/trees/{tree_id}/sources/{source_id}/repositories/{link_id}` | Update a link (`repository_id`, `call_number`, `media_type`, `sort_order`); a link of another source is `not_found` |
| `DELETE` | `/trees/{tree_id}/sources/{source_id}/repositories/{link_id}` | Remove a link; a link of another source is `not_found` |

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

A citation's `confidence` is optional: `null` (or omitted at creation) means
the evidence is not assessed. An update leaves it alone when the field is
omitted and clears it on `null`; GraphQL's `confidence` is nullable on
`Citation`, `CreateCitationInput` and `UpdateCitationInput` with the same
meaning.

### Media

| Method | Path | Description |
|---|---|---|
| `GET` | `/trees/{tree_id}/media` | List the tree's documents (cursor-paginated), narrowed by the media library filters, each with its `usage_count` — see **Media library** below |
| `GET` | `/trees/{tree_id}/media/facets` | The values the media library filters can take: `{tags: [{tag, count}], kinds: [{kind, count}], categories: [{category, count}]}` |
| `POST` | `/trees/{tree_id}/media` | Add a page that names a file without holding it — an archive's URL, or a path a GEDCOM mentioned. `document_id` is required: bytes and addresses live on pages, and a page belongs to a document. The GraphQL twin is `uploadMedia` |
| `POST` | `/trees/{tree_id}/media/upload` | Upload a file. `multipart/form-data`: `file` (required), `title`, `description`, `media_id`, `document_id`. `201` for a new record, `200` when `media_id` attaches bytes to an existing one; `document_id` appends the file as the next page of a multi-page document |
| `POST` | `/trees/{tree_id}/media/document` | Create an empty multi-page document (`{title?}`). Pages are added by uploading with `document_id` |
| `POST` | `/trees/{tree_id}/image-data` | Resolve held **image sources** (see below) to inline `data:` URLs, in request order, at most 1,024 per request. A slot is `null` when there is nothing of ours to inline — a remote source, or a picture we no longer hold. For clients with no origin of their own to serve pictures from; a client that has one never calls this |
| `POST` | `/trees/{tree_id}/gallery-bundle` | Load gallery data for `{media_ids, vignette_ids}`: thumbnail sources, linked event ids, up to four document-page previews, and cropped vignette sources. Every source is an **image source** (see below), never image bytes. A page preview is the page's own thumbnail, or — for a page held only as a remote image URL — that address, which the client draws directly. A remote page that is not an image contributes no preview. A vignette source is the crop we cut from our own copy; over a remote page there is nothing to cut, so the whole picture's address is sent with a `crop` object — `{x, y, width, height, source_width, source_height}` — and the client cuts it. `crop` is absent whenever the source already is the region, and for a page whose pixel size nobody has recorded |
| `GET` | `/trees/{tree_id}/media/{media_id}/pages` | A document's pages, in order |
| `PUT` | `/trees/{tree_id}/media/{media_id}/pages` | Set the page order (`{page_ids: [...]}`). Must name exactly this document's pages, once each — a partial list is refused rather than guessed at |
| `DELETE` | `/trees/{tree_id}/media/{media_id}/pages/{page_id}` | Permanently delete the page, its relationships and unshared stored bytes. Remaining pages close the gap; removing the last page leaves an empty document |
| `GET` | `/trees/{tree_id}/media/{media_id}` | Get media metadata |
| `GET` | `/trees/{tree_id}/media/{media_id}/file` | The stored bytes, served **inline** for previews. `Content-Type` from the file, strong `ETag` (its SHA-256), `Cache-Control: private, max-age=3600`, `304` on a matching `If-None-Match`. `404` if the record has no bytes |
| `GET` | `/trees/{tree_id}/media/{media_id}/download` | Stream one stored original of any supported media type with `Content-Disposition: attachment`, its MIME type, `Cache-Control: private, no-store`, and `X-Content-Type-Options: nosniff`. Always transfers the file rather than returning a conditional `304`. `404` for absent, deleted, foreign-tree or byte-less records; remote URLs are never fetched or redirected to |
| `GET` | `/trees/{tree_id}/media/{media_id}/thumbnail` | Generated thumbnail (longest edge 400 px). `404` when the format cannot be rasterised — PDFs — so a gallery can fall back to an icon on the status alone |
| `GET` | `/trees/{tree_id}/media/{media_id}/archive` | Every live page of a document, in one ZIP attachment (`application/zip`, `private, no-store`, `nosniff`). Entries are prefixed `001_`, `002_` in current reading order; padding grows beyond 999 pages so lexical order remains correct. A page held only as an `http(s)` URL contributes a `.url` Internet Shortcut (`[InternetShortcut]` + `URL=`, CRLF, control characters stripped) instead of bytes, so the archive has one entry per page and the address survives the round trip. Empty documents and any other page without stored bytes return `404`; pages are never silently skipped |
| `PUT` | `/trees/{tree_id}/media/{media_id}` | Update media metadata. `width`/`height` are sent together or not at all, and only for a page whose bytes we do not hold — the browser that displayed a remote picture is the only witness to its size, and for our own copy the size is read from the bytes. Recording them re-checks the regions already drawn on that page |
| `POST` | `/trees/{tree_id}/media/{media_id}/tags` | Add one tag (`{tag}`), idempotently by case-insensitive value |
| `DELETE` | `/trees/{tree_id}/media/{media_id}/tags/{tag}` | Remove one tag, matched like the add (case- and accent-insensitive), without replacing the other tags. `{tag}` is percent-encoded, `/` included |
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
| `tag` | carrying this tag, whatever its case, accents or punctuation (matched on the stored normalized key, the tag folded as in [Cross-cutting Rules §3.6](cross-cutting.md)). Repeatable: `tag=a&tag=b` keeps the documents carrying every tag given |
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

`GET .../media/facets` lists every tag carried by a live document, with its
document count, displayed in the spelling most of those documents carry (ties
to the first in code-point order) and sorted alphabetically ignoring case and
accents; every file kind and category held, with theirs, in their declaration
order. With `tag` (repeatable), the tags are listed and counted among the
documents carrying every tag given, so a tag cloud offers only the tags that
still narrow a selection; the kinds and categories always count the whole
library, and no other listing filter applies.

GraphQL mirrors both: `mediaList(treeId, first, after, filter:
MediaListFilterInput)` takes the same filters (`tags: [String!]`, `kind`, `category`,
`name`, `linkedName`, `eventFrom`, `eventTo`, `addedFrom`, `addedTo`) and puts
the count on each edge as `usageCount`; a backwards range is a GraphQL error.
`mediaFacets(treeId, tags: [String!])` returns `{tags {tag count} kinds {kind
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

**Three kinds of media.** *Stored* — `storage_key` is set, we serve the bytes, there is a thumbnail and crops can be drawn. *Remote* — `file_path` is an `http(s)` URL: recorded, never fetched by us, so no thumbnail and no crop, and the browser goes to the origin directly. *Unheld* — a record naming a file nobody uploaded, which is where every GEDCOM import starts. `PUT .../media/{id}` may edit `file_path` for the last two and **refuses it for a stored one**: there `file_path` is the value a GEDCOM export writes back, and repointing it would make the export describe a file we are serving something else for. It refuses a `mime_type` for a stored one too: that type was read from the bytes at upload and is the one they are served as. A remote `mime_type` is guessed from the URL's extension when not given — the only evidence available without fetching, and it decides whether a viewer embeds the file or offers it as a download. A URL with no extension leaves it `application/octet-stream`, which is not read as "not a picture": such a page is still sent as a gallery preview and still drawn in the viewer, because the browser fetching it is the only reader that can identify the bytes.

**A media carries what a fact carries.** `PUT .../media/{id}` takes `title`, `description`, `date_value`, `date_value2`, `date_qualifier`, `calendar`, `place_id`, `source_media_type` and `document_category`, so "a photograph taken around 1890 at Nantes" is written the way a birth around 1890 is. Tags are independent rows, added and removed through their two dedicated endpoints; this prevents one editor's save from overwriting another editor's tag. Labels are trimmed and case-insensitively unique. Multi-page document tags live on the document row, never its pages. GraphQL exposes the same operations as `addMediaTag` and `removeMediaTag`; `GqlMedia.tags` remains the ordered list for reads. `date_sort` is **not** accepted: the server derives it from `calendar` + `date_value`, exactly as for an event. Notes about a document go on `note.media_id` (`POST /notes` with `media_id`, `GET /notes?media_id=`). There is deliberately **no source field** — a media *is* a source document.

**Two fields for what looks like one question.** `source_media_type` is GEDCOM's `SOURCE_MEDIA_TYPE`, exactly — `photo`, `manuscript`, `tombstone`, `fiche`, `film`, `map`, `newspaper`, `book`, `card`, `magazine`, `audio`, `video`, `electronic`, `other` — and is what an export writes and other genealogy software reads. `document_category` is the distinction GEDCOM cannot draw: a census return, a marriage contract and a conscription register are all `manuscript` to it. Sending a category without a medium also sets the medium that category implies, so a census return exports as `MANUSCRIPT` rather than `OTHER`; sending both keeps both. `document_category` accepts an explicit `null` to unclassify. See [Data Model](data-model.md) (Media).

**Documents and pages.** Every document is a byte-less shell, including an
ordinary photograph's single-page document. Its pages are media rows carrying
`parent_media_id` and `page_index`; each page must belong to a live document in
the same tree, never another page. A page's `page_count` counts images inside
its file (for example, a multi-page TIFF); a document's count is recomputed from
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
| `GET` | `/trees/{tree_id}/vignettes/{vignette_id}/image` | The cropped region as its own JPEG, derived on read and scaled down to fit the 400-pixel thumbnail box: a crop is only ever drawn as a thumbnail. A request decodes at most 128 MiB of pixels: a larger JPEG is decoded at ½, ¼ or ⅛ scale and the vignette comes back that much smaller, and any other image that large is cut from its stored thumbnail (`400` without one). It carries an `ETag` derived from the source file's digest and the rectangle, and answers `If-None-Match` with `304`. `400` for a PDF — rasterising one needs a rendering engine OxidGene does not ship |

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
`"default"` resolves to for everything in that tree — and `entry_suggestions`
(a boolean, `updateTree`'s `entrySuggestions` in GraphQL), which turns the
entry fields' suggestions off or on for the tree, and the GEDCOM submitter:
`submitter_name`, `submitter_email` and `submitter_address` (GraphQL
`submitterName`, `submitterEmail`, `submitterAddress`) — omitted keeps, `null`
or a blank value clears, on both surfaces. **Privacy** is accepted on
all three of `PUT .../persons/{id}`,
`PUT .../families/{id}` and `PUT .../media/{id}` as `"default" | "public" |
"private"`. The family route's body is optional — it long predates this field as
a bare "touch `updated_at`" — so a request with no body still succeeds. Nothing
reads these values yet; see [Data Model](data-model.md) (Privacy).

### Portraits

| Method | Path | Description |
|---|---|---|
| `PUT` | `/trees/{tree_id}/persons/{person_id}/portrait` | Choose what represents a person: `{media_id}`, `{vignette_id}`, or `{}` to clear it. Both ids together is a `400` — a portrait is a media or a crop, never both |
| `GET` | `/trees/{tree_id}/portraits` | Every person's portrait in the tree, as `{person_id, media_id?, vignette_id?, file_path, has_thumbnail}` |
| `POST` | `/trees/{tree_id}/portrait-images` | Load display-ready portraits for `{person_ids: [...]}` in one bounded operation, returning `{person_id, source}` rows |

Replaces `PUT /media-links/{link_id}/profile`, and `MediaLink` no longer carries
`is_profile`. The portrait is a property of the *person* — see
[Data Model](data-model.md) (Person) for why — so setting one is a single write
and needs no clearing pass over the person's other links.

`media_id` accepts a document, which is what a gallery tile is and therefore
what a client sends. What the read operations return is the page that holds the
file: `GET /portraits`, `POST /portrait-images` and `PersonProfile.primary_media`
all resolve a stored document to its first drawable page — see
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

### Notes

| Method | Path | Description |
|---|---|---|
| `GET` | `/trees/{tree_id}/notes` | List notes in a cursor connection, filterable by `person_id`, `event_id`, `family_id`, `source_id`, and `media_id` |
| `POST` | `/trees/{tree_id}/notes` | Create a note |
| `GET` | `/trees/{tree_id}/notes/{note_id}` | Get a note |
| `PUT` | `/trees/{tree_id}/notes/{note_id}` | Update a note |
| `DELETE` | `/trees/{tree_id}/notes/{note_id}` | Soft-delete a note |

A note is created with some text: a blank `text` is a validation error on
both surfaces (`400` in REST, `VALIDATION_ERROR` from `createNote`).

### Dictionary

Aggregations backing the [Dictionary](ui-dictionary.md) page. Value endpoints return distinct values + usage counts; usage endpoints return the persons behind one value, resolved server-side into `PersonUsageEntry` (id, name parts, birth/death years) in one bulk query.

Each year is paired with a `birth_qualifier` / `death_qualifier` so a list can hedge the same way a pedigree card does (`ca 1849`, `< 1917`). The qualifier sits **beside** the year rather than being folded into it: `birth_year` stays an integer the client can sort on, and a `"ca 1849"` in that field would break both that and the search grid.

| Method | Path | Description |
|---|---|---|
| `GET` | `/trees/{tree_id}/dictionary/family-names` | Distinct surnames + person counts. One entry per full surname (particle + root) as spelled, case included; `primary_count` is how many of the `count` persons carry it as their primary name |
| `GET` | `/trees/{tree_id}/dictionary/family-names/usage?value=...` | Persons carrying a surname. `value` is matched exactly against the full surname as listed, particle included, however its rows are cut between particle and root |
| `PATCH` | `/trees/{tree_id}/dictionary/family-names/particle` | Bulk-edit — body `{ "value": "...", "particle": "..." }` re-cuts every `PersonName` carrying surname `value` at `particle` (empty = no particle). `particle` must already be at the head of `value`; rows already cut that way are skipped. Triggers a full projection rebuild when anything changed |
| `PATCH` | `/trees/{tree_id}/dictionary/family-names/rename` | Bulk rename — see below |
| `GET` | `/trees/{tree_id}/dictionary/occupations` | Distinct occupation labels + counts |
| `GET` | `/trees/{tree_id}/dictionary/occupations/usage?value=...` | Persons with an occupation |
| `GET` | `/trees/{tree_id}/dictionary/sources` | Sources + citation counts |
| `GET` | `/trees/{tree_id}/dictionary/sources/{source_id}/usage` | Persons citing a source |
| `GET` | `/trees/{tree_id}/dictionary/places` | Places + reference counts (events + media) |
| `GET` | `/trees/{tree_id}/dictionary/places/{place_id}/usage` | Persons referencing a place: those whose own events take place there, the spouses of the couples whose events do, and the persons a media filed there (or one of its pages) is linked to — directly, through one of their events or couples — or shows in a crop. Deleted persons are left out |

The family-names entry is `{ value, sort_key, count, primary_count }`;
`primary_count` is absent from the other dictionaries.

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

### Value suggestions

What the free-text fields of the entry forms suggest while the user types
(see [Common UI §4.4](ui-common.md)).

| Method | Path | Description |
|---|---|---|
| `GET` | `/trees/{tree_id}/suggestions/{field}?q=...&lang=...&limit=...&surname=...&given_names=...` | Values for `field`: `family-names`, `given-names`, `occupations` or `sources` (titles). `lang` is an interface language code; `limit` defaults to 10, 1–50 accepted. `surname` and `given_names` scope a name field (below). An unknown field or language, an out-of-range limit and a scope on `occupations` or `sources` are 400 |

The text of `q` is normalized like the reference sheets' terms (case,
accents and punctuation ignored) and must start a word of the value; a blank
`q` returns `[]`. The tree's values come first — those starting with `q`,
then the most used — and, for occupations and given names, the terms a
reference sheet answers to fill the rest of the list, in any language's
spelling, `lang`'s own first, never repeating a value the tree already holds.
Given names are single words: `"Jean Marie"` holds `"Jean"` and `"Marie"`.
Two sources with the same title are offered once, their citations summed.
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
`OCCUPATIONS` or `SOURCES`.

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
| `GET` | `/trees/{tree_id}/history/{record_type}/{record_id}?first=&after=` | A record's versions, **latest first**: its live state (`current: true`), then its stored ones. `record_type` is `person`, `place`, `source` or `tree` (whose `record_id` is the tree's own ID). A record that does not exist and has no stored state has none |
| `GET` | `/trees/{tree_id}/history/{record_type}/{record_id}/{version}` | One `RecordVersion`, the current one included; a number past it is `404` |
| `POST` | `/trees/{tree_id}/history/{record_type}/{record_id}/revert` | Body `{ "version": 3 }`. Puts the record back as that version had it — restoring the state a deletion replaced undeletes it — and returns the restore's own `AuditEntry` (`action: "revert"`, `details.version`). A version whose `deleted` is `true` is refused with `400`: restore the one before it instead |

An `AuditEntry` is `{ id, tree_id, occurred_at, category, action, entity,
entity_id, subject, subject_id, label, details, version_count }`. `action` is
`create`, `update`, `delete`, `merge`, `import`, `export` or `revert`;
`version_count` is the number of states the write stored; `entity` names the
kind of row written (`person_name`, `event`,
`media_tag`, …) and `subject` the record the write is about (`person`,
`family`, `place`, `source`, `media`, `tree`). `label` is the subject's display
name when the write happened. `details` carries only what applies: `format`,
`file_name` and `count` for imports and exports, `event_type` for event writes,
`version` for a restore, `other_label` for the person a merge absorbed,
`new_label` for the name a family-name rename gave (the entry's `label`
keeping the old one).

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

Used by: [Person History](ui-person-history.md) · [Settings §11](ui-settings.md#11-section-history) (audit log)

### Statistics

Aggregates backing the [Statistics](ui-statistics.md) page, computed on each
request from the person projections and place usages; nothing is stored.

| Method | Path | Description |
|---|---|---|
| `GET` | `/trees/{tree_id}/statistics?approximate=false&lang=en` | The tree's statistics, time series filed by year; `approximate=true` lets ages and averages use dates about, calculated or estimated; `lang` (an interface language, `en` by default) names the places' countries, regions and subdivisions. 400 for another language, 404 for an unknown tree |

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
| `GET` | `/trees/{tree_id}/anomalies` | The tree's anomalies by rule; 404 for an unknown tree |
| `GET` | `/trees/{tree_id}/duplicates` | The pairs of records that may be one person, best first; 404 for an unknown tree |
| `GET` | `/trees/{tree_id}/unlocated-places` | The used places the statistics cannot locate, most used first; 404 for an unknown tree |
| `GET` | `/trees/{tree_id}/ancestry-completeness?generations=8` | Generation by generation from the tree's SOSA root, the ancestors found and missing and their key facts; `generations` counts the root's, 8 by default, from 1 to 15 (400 otherwise); 404 for an unknown tree |

**Anomalies** returns `{persons, rules}`: the number of persons checked, and
one entry per rule that found something, in the order of the catalogue of
[Tools §3.3](ui-tools.md): `{rule, category, severity, count, items}`,
`category` being `dates`, `filiation`, `unions`, `witnesses` or
`data_quality` (shown as *records*), `severity` `error` or `warning`, and
`items` at most 500 of the `count` found. An item is `{persons, family_id,
value, event_type, text}`: the persons concerned as `{person_id, name}`, the
subject first; the union concerned; the figure measured, in whole years for
ages and gaps in years and in days for `siblings_too_close` and
`born_long_after_father_death` (the number of unions for
`repeated_union`); the event concerned, an `EventType` in snake_case; and
the recorded text for `unreadable_date` (the date as typed) and
`godparent_sex` (the relation).

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
domain), embedded and Brotli-compressed like the place dictionary.

### Import / export

GEDCOM and GEDZIP are read and written; GeneWeb `.gw` is read only — OxidGene
imports the format, it does not produce it.

| Method | Path | Description |
|---|---|---|
| `POST` | `/trees/{tree_id}/import-jobs?format=gedcom\|gedzip\|geneweb&filename=name.gw` | Stream a raw genealogy file to durable job storage and create an asynchronous import. Returns `202 { "job_id": UUID }` after the source is stored and the job is committed. `filename` is optional GeneWeb provenance metadata only. The 1 GiB limit is enforced while streaming. Workers copy the source to disposable scratch space; media extracted from GEDZIP are persisted through `MediaStore` |
| `GET` | `/trees/{tree_id}/import-jobs/{job_id}` | Poll `{ phase, done, total, result?, geneanet_result?, error? }`. File-import phases are `starting`, `parsing`, `media`, `database`, `projections`, `completed`, and `failed`; Geneanet additionally uses `people` and `matching`. A completed status retains either the standard `ImportResponse` or the Geneanet receipt; failure exposes a stable error code rather than internal details |
| `POST` | `/trees/{tree_id}/export-jobs?merge_occupations=bool&merge_names=bool` | Create a durable asynchronous GEDZIP export. Returns `202 { "job_id": UUID }`; archive creation and media reads run in the worker |
| `GET` | `/trees/{tree_id}/export-jobs/{job_id}` | Poll `{ phase, done, total, download_url?, expires_at?, warnings, error? }`. `download_url` and `expires_at` (RFC 3339, an hour after completion) appear together, only while the completed artifact can be downloaded. A job ended more than a day ago is gone (`404`) |
| `GET` | `/trees/{tree_id}/export-jobs/{job_id}/download` | Stream the completed GEDZIP artifact as `application/zip` with `Content-Disposition: attachment`; returns an error while the job is incomplete. The artifact can be downloaded any number of times for an hour after completion, then answers `404` and is deleted by the workers' next maintenance pass |
| `GET` | `/trees/{tree_id}/gedcom/export?format=gedcom\|gedzip&merge_occupations=bool&merge_names=bool` | Export tree as GEDCOM text (default) or GEDZIP archive (`application/zip`, includes media files). `merge_occupations` (default `false`) collapses each person's multiple `OCCU` tags back into one, comma-separated. `merge_names` (default `false`) collapses each person's non-primary names into the primary name's `SURN` tag, comma-separated. Both are for importers (e.g. Geneanet) that only support a single profession field / read the first `NAME` structure |

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
names. Matching folds separators and case, so a producer's `.\Media\Photo.JPG`
still finds `media/photo.jpg`. The `gedcom.ged` entry has a 1 GiB limit and
each decompressed medium the ordinary 128 MiB per-file limit; media are read
sequentially. A GeneWeb `.gw` is uploaded as raw bytes because it is
ISO-8859-1 unless the file opts into UTF-8 with an `encoding:` directive, and
the switch can happen mid-file, so only the reader can decode it; `filename`
(default `import.gw`) is recorded on every family and quoted in warnings.

Used by: [Homepage](ui-home.md) (card menu import) · [Settings](ui-settings.md) (export section)

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
| `POST` | `/trees/{tree_id}/geneanet/import` | **Step 5.** Copy every local input to durable job storage and queue the tree-and-media import. `fetched` maps source URLs to temporary filesystem paths; it never carries media bytes. Returns `202 { "job_id": UUID }` only after staging and job creation succeed. The UI then polls the common import-job status; its completed `geneanet_result` is the full Geneanet receipt |

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
| `GET` | `/trees/{tree_id}/profiles` | Get every person projection of a tree |
| `POST` | `/trees/{tree_id}/profiles/rebuild` | Force a full projection rebuild for a tree |
| `POST` | `/trees/{tree_id}/profiles/rebuild/{person_id}` | Rebuild a single person's projection |
| `DELETE` | `/trees/{tree_id}/profiles` | Drop a tree's projections (rebuilt lazily on next read) |
| `GET` | `/trees/{tree_id}/pedigree/{root_person_id}?ancestor_depth=N&descendant_depth=N` | Assemble a windowed pedigree for a root person |
| `POST` | `/trees/{tree_id}/pedigrees` | Assemble several pedigrees at once for `{root_person_ids, ancestor_depth, descendant_depth}`. Request order is preserved; a root that cannot be assembled is omitted rather than failing the batch. At most 64 roots per request |
| `GET` | `/trees/{tree_id}/pedigree/{root_person_id}/expand?direction=ancestors\|descendants&from_depth=N&to_depth=N&other_depth=N` | Expand pedigree depth (returns only new nodes/edges). `other_depth` is the depth already loaded in the opposite direction (default `0`) |

Every pedigree depth — `ancestor_depth`, `descendant_depth`, `from_depth`,
`to_depth`, `other_depth`, and their GraphQL counterparts — lies between 0 and
**10** generations, the range the pedigree view offers; anything else, a
negative GraphQL `Int` included, is a `validation_error`. REST, GraphQL and
the assistant tools ([MCP](mcp.md)) enforce the same limit, and the batched
`pedigrees` checks the tree's projections once for the whole batch.

The profile and pedigree vocabulary is identical across REST and GraphQL. No
legacy `/cache/*` routes or `cached*` GraphQL aliases are part of the contract.

**A pedigree node carries whole events, not extracted years.** `PedigreeNode` and `PedigreeFamilyMember` expose `birth` / `death` as `ProfileEvent`s. They used to hold a `birth_year` string plus a `birth_place` string, and everything that did not fit those two — the day and month, the far end of an `Or`/`Between` range, the calendar, the place's id — was gone before any client saw it: a birth on 2 Nov 1788 arrived as `"1788"`, and a death recorded as "between 11 Nov 1691 and 20 Aug 1693" as a qualifier promising a second date the payload could not carry. `ProfileEvent` therefore also carries `date_qualifier`, `date_value2` and `calendar`, which is what lets a client render « entre 11 nov. 1691 et 20 août 1693 » rather than « entre 1691 ».

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
| Media and ordering | `has_media`, `sort` (`relevance`, `name_asc`, `name_desc`, `birth_asc`, `birth_desc`) |

All supplied filters are combined with AND. Name and free-text matching is
case- and accent-insensitive, including the `Relations` filters. An empty or
missing `q` is valid: structured filters can be used alone, and no filters at
all select browse mode. The response's `total_count` is computed before `limit`
and `offset` are applied.

`q` is matched as a **prefix**, per word, on both backends: every word must
match the start of one of the indexed fields, in any order and in any field.
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
displaying a bare year.

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
(Jehan, Joannes) are aliases of one of them. Source content lives in
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

Sending the bytes inline instead — which is what these payloads used to do —
inflated them by a third in base64, put a whole album through a single JSON
parse before anything could be drawn, and denied the rendering engine every
optimisation it has for images: no caching between renders or pages, no decode
off the main thread, and no skipping a picture that never scrolls into view.

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

All list endpoints accept:
- `first` (i32): number of items to return (default 25, max 100).
- `after` (String): cursor for forward pagination.

Responses use a connection envelope:

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

---

## 3. GraphQL API

Endpoint: `/graphql` using POST with a JSON body (`{"query": …, "variables": …}`)
for queries and mutations; `GET /graphql` serves GraphiQL where enabled. No subscription
contract is currently exposed.

### Queries

```graphql
type Query {
  # Trees
  trees(first: Int, after: String): TreeConnection!
  tree(id: ID!): Tree

  # Persons
  persons(treeId: ID!, first: Int, after: String, search: String): PersonConnection!
  person(treeId: ID!, id: ID!): Person
  personDetailBundle(treeId: ID!, personId: ID!): PersonDetailBundle!
  relationLabels(treeId: ID!, personIds: [ID!]!, familyIds: [ID!]!): RelationLabels!
  personBySosa(treeId: ID!, number: Int!): Person
  personHomonyms(treeId: ID!, personId: ID!): [SearchEntry!]!
  recentlyModifiedPersons(treeId: ID!, limit: Int = 5): [SearchEntry!]!
  ancestors(treeId: ID!, personId: ID!, maxDepth: Int): [PersonWithDepth!]!
  descendants(treeId: ID!, personId: ID!, maxDepth: Int): [PersonWithDepth!]!
  kinship(treeId: ID!, personId: ID!, otherPersonId: ID!): Kinship!
  portraits(treeId: ID!): [Portrait!]!
  portraitImages(treeId: ID!, personIds: [ID!]!): [PortraitImage!]!

  # Dictionary and static reference content
  dictionaryFamilyNames(treeId: ID!): [DictionaryEntry!]!
  dictionaryOccupations(treeId: ID!): [DictionaryEntry!]!
  dictionarySources(treeId: ID!, prefix: String): [SourceDictionaryEntry!]!
  dictionarySourceDrill(treeId: ID!, prefix: String): SourceDictionaryDrill!
  dictionaryPlaces(treeId: ID!): [PlaceDictionaryEntry!]!
  treeStatistics(treeId: ID!, approximate: Boolean, language: String): TreeStatistics!
  treeGrowth(treeId: ID!): TreeGrowth!
  ancestryCompleteness(treeId: ID!, generations: Int): AncestryCompleteness!
  treeAnomalies(treeId: ID!): TreeAnomalies!
  unlocatedPlaces(treeId: ID!): [PlaceUsage!]!
  potentialDuplicates(treeId: ID!): PotentialDuplicates!   # { count, pairs { score reasons first second } }
  basemap: [BasemapCountry!]!
  familyNameUsage(treeId: ID!, value: String!): [PersonUsageEntry!]!
  occupationUsage(treeId: ID!, value: String!): [PersonUsageEntry!]!
  sourceUsage(treeId: ID!, sourceId: ID!): [PersonUsageEntry!]!
  placeUsage(treeId: ID!, placeId: ID!): [PersonUsageEntry!]!
  occupationReference(language: String!, term: String!): OccupationReference
  occupationReferences(language: String!, terms: [String!]!): [OccupationReferenceMatch!]!
  givenNameReference(language: String!, term: String!): GivenNameReference
  givenNameReferences(language: String!, terms: [String!]!): [GivenNameReferenceMatch!]!
  placeSuggestions(language: String!, query: String!, limit: Int): [PlaceSuggestion!]!
  valueSuggestions(treeId: ID!, field: SuggestionField!, language: String!, query: String!, limit: Int): [ValueSuggestion!]!

  # Geneanet import wizard (the archive path operation is desktop-only)
  inspectGeneweb(gwBase64: String!, fileName: String!): GeneanetInspection!
  indexGeneanetArchives(paths: [String!]!): GeneanetArchiveIndex!
  geneanetPreview(input: GeneanetPreviewInput!): GeneanetPreview!
  geneanetPlan(input: GeneanetPreviewInput!): [GeneanetNeededMedia!]!

  # Families
  families(treeId: ID!, first: Int, after: String): FamilyConnection!
  family(treeId: ID!, id: ID!): Family

  # Events
  events(treeId: ID!, first: Int, after: String, eventType: EventType, personId: ID, familyId: ID): EventConnection!
  event(treeId: ID!, id: ID!): Event

  # Places
  places(treeId: ID!, first: Int, after: String, search: String): PlaceConnection!
  place(treeId: ID!, id: ID!): Place

  # Sources
  sources(treeId: ID!, first: Int, after: String): SourceConnection!
  source(treeId: ID!, id: ID!): Source

  # Citations
  citations(
    treeId: ID!
    personId: ID
    eventId: ID
    familyId: ID
    sourceId: ID
    first: Int
    after: String
  ): CitationConnection!

  # Notes
  notes(
    treeId: ID!
    personId: ID
    eventId: ID
    familyId: ID
    sourceId: ID
    mediaId: ID
    first: Int
    after: String
  ): NoteConnection!
  note(treeId: ID!, id: ID!): Note

  # Media
  mediaList(treeId: ID!, first: Int, after: String): MediaConnection!
  media(treeId: ID!, id: ID!): Media
  mediaDownload(treeId: ID!, id: ID!): GqlMediaDownload!    # { url }, original attachment
  mediaArchive(treeId: ID!, id: ID!): GqlMediaDownload!     # { url }, complete document ZIP
  imageData(treeId: ID!, sources: [ImageSourceInput!]!): [String]!
  galleryBundle(treeId: ID!, mediaIds: [ID!]!, vignetteIds: [ID!]!): GalleryBundle!

  # Media galleries
  entityMedia(treeId: ID!, entityType: String!, entityId: ID!): [MediaWithLink!]!
  treeMediaLinks(treeId: ID!): [TreeMediaLink!]!              # all person/event links in a tree
  mediaLinks(treeId: ID!, mediaId: ID!): [MediaLink!]!       # what one file is attached to
  mediaPages(treeId: ID!, mediaId: ID!): [Media!]!           # a document's pages, in order

  # Vignettes
  mediaVignettes(treeId: ID!, mediaId: ID!): [Vignette!]!
  vignettes(treeId: ID!, personId: ID, eventId: ID): [Vignette!]!   # exactly one filter
  vignette(treeId: ID!, id: ID!): Vignette

  # Text GEDCOM compatibility export and durable job status
  # Records the export in the tree's audit log, as REST does.
  exportGedcom(treeId: ID!, mergeOccupations: Boolean, mergeNames: Boolean): ExportGedcomResult!
  importJobStatus(treeId: ID!, jobId: ID!): ImportJobStatus!
  exportJobStatus(treeId: ID!, jobId: ID!): ExportJobStatus!

  # Read projections (see Data Model section 4) — mirrors the REST routes
  personProfile(treeId: ID!, personId: ID!): GqlPersonProfile!
  personProfiles(treeId: ID!): [GqlPersonProfile!]!
  pedigree(treeId: ID!, rootPersonId: ID!, ancestorDepth: Int!, descendantDepth: Int!): GqlPedigree!
  pedigrees(treeId: ID!, rootPersonIds: [ID!]!, ancestorDepth: Int!, descendantDepth: Int!): [PedigreeEntry!]!
  # A read, like REST's `GET …/expand`: only what an expansion adds.
  expandPedigree(treeId: ID!, rootPersonId: ID!, direction: PedigreeDirection!, fromDepth: Int!, toDepth: Int!, otherDepth: Int = 0): GqlPedigreeDelta!
  searchPersons(
    treeId: ID!
    query: String!
    limit: Int
    offset: Int
    sex: Sex
    surname: String
    givenNames: String
    occupation: String
    spouseSurname: String
    spouseGivenNames: String
    fatherSurname: String
    fatherGivenNames: String
    motherSurname: String
    motherGivenNames: String
    birthFrom: Int
    birthTo: Int
    deathFrom: Int
    deathTo: Int
    place: String
    eventType: EventType
    eventFrom: Int
    eventTo: Int
    hasMedia: Boolean = false
    sort: PersonSearchSort
  ): GqlSearchResult!

  # History — mirrors the REST audit and history routes
  auditEntries(treeId: ID!, first: Int, after: String, category: GqlAuditCategory, subjectId: ID): GqlAuditEntryConnection!
  auditEntry(treeId: ID!, id: ID!): GqlAuditEntry!
  auditEntryChanges(treeId: ID!, entryId: ID!, first: Int, after: String): GqlVersionChangeConnection!
  recordVersions(treeId: ID!, recordType: GqlRecordType!, recordId: ID!, first: Int, after: String): GqlRecordVersionConnection!
  recordVersion(treeId: ID!, recordType: GqlRecordType!, recordId: ID!, version: Int!): GqlRecordVersion!
}
```

### Mutations

```graphql
type Mutation {
  # Trees
  createTree(input: CreateTreeInput!): Tree!
  duplicateTree(treeId: ID!, name: String!): Tree!
  updateTree(id: ID!, input: UpdateTreeInput!): Tree!
  deleteTree(id: ID!): Boolean!

  # Persons
  createPerson(treeId: ID!, input: CreatePersonInput!): Person!
  updatePerson(treeId: ID!, id: ID!, input: UpdatePersonInput!): Person!
  deletePerson(treeId: ID!, id: ID!): Boolean!
  markPersonsDistinct(treeId: ID!, personId: ID!, otherPersonIds: [ID!]!): Boolean!
  mergePersons(treeId: ID!, personId: ID!, duplicateId: ID!, choices: MergeChoicesInput): Person!

  # Person Names
  addPersonName(treeId: ID!, personId: ID!, input: PersonNameInput!): PersonName!
  updatePersonName(treeId: ID!, personId: ID!, nameId: ID!, input: PersonNameInput!): PersonName!
  deletePersonName(treeId: ID!, personId: ID!, nameId: ID!): Boolean!

  # Dictionary — bulk family-name edits (mirror the REST PATCH routes)
  setFamilyNameParticle(treeId: ID!, input: SetFamilyNameParticleInput!): GqlFamilyNameParticleUpdate!
  renameFamilyName(treeId: ID!, input: RenameFamilyNameInput!): GqlFamilyNameRename!
    # input { value, newValue, particle }; result { value, newValue, surnamePrefix,
    # surname, namesUpdated, personsUpdated, merged }

  # Families
  createFamily(treeId: ID!, input: CreateFamilyInput!): Family!
  updateFamily(treeId: ID!, id: ID!, input: UpdateFamilyInput!): Family!
  deleteFamily(treeId: ID!, id: ID!): Boolean!
  addSpouse(treeId: ID!, familyId: ID!, input: AddSpouseInput!): FamilySpouse!
  removeSpouse(treeId: ID!, familyId: ID!, spouseId: ID!): Boolean!
  addChild(treeId: ID!, familyId: ID!, input: AddChildInput!): FamilyChild!
  removeChild(treeId: ID!, familyId: ID!, childId: ID!): Boolean!

  # Events
  createEvent(treeId: ID!, input: CreateEventInput!): Event!
  updateEvent(treeId: ID!, id: ID!, input: UpdateEventInput!): Event!
  deleteEvent(treeId: ID!, id: ID!): Boolean!
  addEventWitness(treeId: ID!, eventId: ID!, input: AddEventWitnessInput!): EventWitness!
  # With eventId, a witness of another event is NOT_FOUND.
  removeEventWitness(treeId: ID!, id: ID!, eventId: ID): Boolean!

  # Places
  createPlace(treeId: ID!, input: CreatePlaceInput!): Place!
  updatePlace(treeId: ID!, id: ID!, input: UpdatePlaceInput!): Place!
  deletePlace(treeId: ID!, id: ID!): Boolean!

  # Sources
  createSource(treeId: ID!, input: CreateSourceInput!): Source!
  updateSource(treeId: ID!, id: ID!, input: UpdateSourceInput!): Source!
  deleteSource(treeId: ID!, id: ID!, onlyIfUnused: Boolean! = false): Boolean!

  # Citations
  createCitation(treeId: ID!, input: CreateCitationInput!): Citation!
  updateCitation(treeId: ID!, id: ID!, input: UpdateCitationInput!): Citation!
  deleteCitation(treeId: ID!, id: ID!): Boolean!

  # Media
  uploadMedia(treeId: ID!, input: UploadMediaInput!): Media!          # metadata only
  uploadMediaFile(treeId: ID!, input: UploadMediaFileInput!): Media!  # bytes, base64
  updateMedia(treeId: ID!, id: ID!, input: UpdateMediaInput!): Media!
  # Permanently deletes media. With onlyIfUnreferencedElsewhere, allowedLinkId
  # is required and the result is false when another reference retains it.
  deleteMedia(treeId: ID!, id: ID!, onlyIfUnreferencedElsewhere: Boolean! = false, allowedLinkId: ID): Boolean!
  createMediaLink(treeId: ID!, input: CreateMediaLinkInput!): MediaLink!
  setPersonPortrait(treeId: ID!, personId: ID!, mediaId: ID, vignetteId: ID): Person!

  # Multi-page documents
  createMediaDocument(treeId: ID!, title: String): Media!
  appendMediaPage(documentId: ID!, mediaId: ID!): Media!
  reorderMediaPages(documentId: ID!, pageIds: [ID!]!): [Media!]!
  deleteMediaPage(treeId: ID!, documentId: ID!, pageId: ID!): Boolean!
  deleteMediaLink(treeId: ID!, id: ID!): Boolean!

  # Vignettes
  createVignette(input: CreateVignetteInput!): Vignette!
  updateVignette(id: ID!, input: UpdateVignetteInput!): Vignette!
  deleteVignette(id: ID!): Boolean!

  # Notes
  createNote(treeId: ID!, input: CreateNoteInput!): Note!
  updateNote(treeId: ID!, id: ID!, input: UpdateNoteInput!): Note!
  deleteNote(treeId: ID!, id: ID!): Boolean!

  # Durable GEDZIP exports contain no binary GraphQL payload. Ordinary file
  # import jobs are created by the streaming REST upload endpoint.
  startExportJob(treeId: ID!, mergeOccupations: Boolean, mergeNames: Boolean): BackgroundJobStarted!

  # Geneanet session archives use base64. Import inputs name files on the
  # shared desktop filesystem; the mutation stages them into durable storage.
  encodeGeneanetSession(input: GeneanetSessionEncodeInput!): GeneanetSessionArchive!
  decodeGeneanetSession(archiveBase64: String!): GeneanetSession!
  releaseGeneanetSessionMedia(paths: [String!]!): Boolean!
  importGeneanet(treeId: ID!, input: GeneanetImportInput!): BackgroundJobStarted!

  # History
  revertRecord(treeId: ID!, recordType: GqlRecordType!, recordId: ID!, version: Int!): GqlAuditEntry!

  # Read projections (see Data Model section 4) — mirrors the REST routes
  rebuildTreeProfiles(treeId: ID!): GqlProfileRebuildResult!
  rebuildPersonProfile(treeId: ID!, personId: ID!): GqlProfileRebuildResult!
  dropTreeProfiles(treeId: ID!): Boolean!
}
```

`GeneanetPreviewInput` carries the same `gwBase64`, collection, deposit-size,
archive-path and `mediaFidelity` data as REST's preview and plan bodies
(`mediaFidelity` is `GeneanetMediaFidelity`: `RENDITIONS`, the default, or
`ORIGINALS`). `GeneanetImportInput` adds the source-URL-to-local-path map. These paths are the staging handoff:
GraphQL does not carry the corresponding bytes, and the mutation copies every
input to job-owned durable storage before returning its job id.
`indexGeneanetArchives`, `releaseGeneanetSessionMedia` and paths returned from `decodeGeneanetSession` are
desktop-only because they refer to the local filesystem. The runtime capability
that protects the REST data plane also protects these GraphQL fields. A caller
polls `importJobStatus`; `result` is set for GEDCOM/GEDZIP/GeneWeb jobs and
`geneanetResult` is set for a completed Geneanet job.

### Key Types

The nested lists of `Person`, `Family` and `Event` (names, families, events,
citations, media, spouses, children, witnesses) are complete — never cut to a
first page — and each is read with one query, whatever the size of the tree.
`Tree.personCount` and `familyCount` are counts, not a page of rows.

```graphql
type Tree {
  id: ID!
  name: String!
  description: String
  personCount: Int!
  familyCount: Int!
  # An import queued or running into the tree, read from the job queue.
  importInProgress: Boolean!
  importJobId: ID
  createdAt: DateTime!
  updatedAt: DateTime!
}

type Person {
  id: ID!
  sex: Sex!
  names: [PersonName!]!
  primaryName: PersonName
  families: [Family!]!
  events: [Event!]!
  citations: [Citation!]!
  media: [Media!]!
  notes: [Note!]!
  createdAt: DateTime!
  updatedAt: DateTime!
}

type PersonWithDepth {
  person: Person!
  depth: Int!
}

type Family {
  id: ID!
  spouses: [FamilySpouseDetail!]!
  children: [FamilyChildDetail!]!
  events: [Event!]!
  createdAt: DateTime!
  updatedAt: DateTime!
}

type FamilySpouseDetail {
  id: ID!
  person: Person!
  role: SpouseRole!
  sortOrder: Int!
}

type FamilyChildDetail {
  id: ID!
  person: Person!
  childType: ChildType!
  sortOrder: Int!
}

type Event {
  id: ID!
  eventType: EventType!
  dateValue: String
  dateSort: Date
  dateQualifier: DateQualifier!
  dateValue2: String
  calendar: Calendar!
  place: Place
  person: Person
  family: Family
  description: String
  cause: String            # GEDCOM CAUS tag (e.g. cause of death)
  age: String              # GEDCOM AGE, canonical form (34y, < 1y 6m, CHILD)
  agency: String           # GEDCOM AGNC, the authority responsible for the record
  spouseAges: [SpouseAge!]! # family event: { personId, age } per spouse (HUSB.AGE / WIFE.AGE)
  witnesses: [EventWitness!]!
  citations: [Citation!]!
  media: [Media!]!
  notes: [Note!]!
  createdAt: DateTime!
  updatedAt: DateTime!
}

type EventWitness {
  id: ID!
  eventId: ID!
  personId: ID!
  relation: String         # free text, e.g. "Godmother"
  sortOrder: Int!
}

# Returned by a completed import job, whatever the source format.
type ImportResult {
  personsCount: Int!
  familiesCount: Int!
  eventsCount: Int!
  sourcesCount: Int!
  mediaCount: Int!
  placesCount: Int!
  notesCount: Int!
  warnings: [String!]!
}

type BackgroundJobStarted {
  jobId: ID!
}

type ImportJobStatus {
  phase: String!
  done: Int!
  total: Int!
  result: ImportResult
  geneanetResult: GeneanetImportResult
  error: String
}

type GeneanetImportResult {
  personsCount: Int!
  familiesCount: Int!
  eventsCount: Int!
  sourcesCount: Int!
  placesCount: Int!
  notesCount: Int!
  mediaCount: Int!
  linksCount: Int!
  portraitsCount: Int!
  isolatedCount: Int!
  isolatedPeople: [GeneanetIsolatedPerson!]!
  vignettesCount: Int!
  skipped: [String!]!
  warnings: [String!]!
}

# A person created for an identification outside the tree, in creation order.
# REST: `isolated_people: [{ person_id, surname, given_names }]`.
type GeneanetIsolatedPerson {
  personId: ID!
  surname: String!
  givenNames: String!
}

type ExportJobStatus {
  phase: String!
  done: Int!
  total: Int!
  downloadUrl: String
  expiresAt: DateTime
  warnings: [String!]!
  error: String
}

# Connection types (Relay-style pagination)
type TreeConnection {
  edges: [TreeEdge!]!
  pageInfo: PageInfo!
  totalCount: Int!
}

type TreeEdge {
  cursor: String!
  node: Tree!
}

type PageInfo {
  hasNextPage: Boolean!
  endCursor: String
}

# The same edge/pageInfo/totalCount shape is exposed by Person, Family, Event,
# Place, Source, Citation, Note, and Media connection types, and by
# GqlAuditEntry, GqlRecordVersion and GqlVersionChange.

# --- History types (see Data Model section 5) ---

type GqlAuditEntry {
  id: ID!
  treeId: ID!
  occurredAt: DateTime!
  category: GqlAuditCategory!   # DATA, SETTINGS, MEDIA, IMPORT, EXPORT, HISTORY
  action: GqlAuditAction!       # CREATE, UPDATE, DELETE, MERGE, IMPORT, EXPORT, REVERT
  entity: GqlAuditEntity!       # PERSON, PERSON_NAME, EVENT, MEDIA_TAG, …
  entityId: ID
  subject: GqlAuditSubject      # TREE, PERSON, FAMILY, PLACE, SOURCE, MEDIA
  subjectId: ID
  label: String
  details: GqlAuditDetails!     # format, fileName, count, eventType, version, otherLabel, newLabel
  versionCount: Int!
}

type GqlRecordVersion {
  id: ID!
  treeId: ID!
  recordType: GqlRecordType!    # PERSON, PLACE, SOURCE, TREE
  recordId: ID!
  version: Int!
  current: Boolean!             # the live record
  deleted: Boolean!
  entry: GqlAuditEntry          # the write that produced the state
  snapshot: GqlRecordSnapshot   # recordType plus exactly one of person, place, source, tree; null when deleted
  labels: [GqlRecordLabel!]!    # { id, label }
}

type GqlVersionChange {
  version: GqlRecordVersion!
  previous: GqlRecordVersion!
}

# GqlPersonSnapshot, GqlPlaceSnapshot, GqlSourceSnapshot and GqlTreeSnapshot
# carry the same fields as the REST snapshot, camelCased.

# --- Read projection types (see Data Model section 4) ---

type GqlPersonProfile {
  personId: ID!
  treeId: ID!
  sex: Sex!
  primaryName: GqlProfileName
  otherNames: [GqlProfileName!]!
  birth: GqlProfileEvent
  death: GqlProfileEvent
  baptism: GqlProfileEvent
  burial: GqlProfileEvent
  occupation: String
  otherEvents: [GqlProfileEvent!]!
  familiesAsSpouse: [GqlProfileFamilyLink!]!
  familyAsChild: GqlProfileChildLink
  primaryMedia: GqlProfileMediaRef
  mediaCount: Int!
  citationCount: Int!
  noteCount: Int!
  updatedAt: DateTime!
  builtAt: DateTime!
}

type GqlProfileName {
  nameId: ID!
  nameType: NameType!
  displayName: String!
  givenNames: String
  surname: String
}

type GqlProfileEvent {
  eventId: ID!
  eventType: EventType!
  dateValue: String
  dateSort: Date
  dateQualifier: DateQualifier!
  dateValue2: String
  calendar: Calendar!
  placeName: String
  placeId: ID
  description: String
}

type GqlProfileFamilyLink {
  familyId: ID!
  role: SpouseRole!
  spouseId: ID
  spouseDisplayName: String
  spouseSurname: String
  spouseGivenNames: String
  spouseSex: Sex
  marriage: GqlProfileEvent
  childrenIds: [ID!]!
  childrenCount: Int!
}

type GqlProfileChildLink {
  familyId: ID!
  childType: ChildType!
  fatherId: ID
  fatherDisplayName: String
  fatherSurname: String
  fatherGivenNames: String
  motherId: ID
  motherDisplayName: String
  motherSurname: String
  motherGivenNames: String
}

type GqlProfileMediaRef {
  mediaId: ID!
  filePath: String!
  mimeType: String!
  title: String
}

type GqlPedigree {
  treeId: ID!
  rootPersonId: ID!
  persons: [PedigreeNode!]!
  edges: [PedigreeEdge!]!
  ancestorDepthLoaded: Int!
  descendantDepthLoaded: Int!
  builtAt: DateTime!
}

type PedigreeNode {
  personId: ID!
  sex: Sex!
  displayName: String!
  givenNames: String
  surname: String
  # Whole events, not a year and a place name pulled out of them — see below.
  # Fall back to baptism / burial when the birth / death carries no date.
  birth: GqlProfileEvent
  death: GqlProfileEvent
  occupation: String
  primaryMediaPath: String
  generation: Int!
  sosaNumber: Int
}

type PedigreeEdge {
  parentId: ID!
  childId: ID!
  familyId: ID!
  edgeType: ChildType!
}

type PedigreeDelta {
  newNodes: [PedigreeNode!]!
  newEdges: [PedigreeEdge!]!
  ancestorDepthLoaded: Int!
  descendantDepthLoaded: Int!
}

type SearchResult {
  entries: [SearchEntry!]!
  totalCount: Int!
}

type SearchEntry {
  personId: ID!
  sex: Sex!
  displayName: String!
  surname: String!
  givenNames: String!
  birthYear: String
  birthQualifier: DateQualifier!
  birthPlace: String
  deathYear: String
  deathQualifier: DateQualifier!
  spouseNames: [String!]!
  fatherName: String
  motherName: String
  childrenCount: Int!
}

enum PedigreeDirection {
  ANCESTORS
  DESCENDANTS
}
```

---

## 4. GEDCOM Compatibility Reference

The API handles GEDCOM import/export via the `ged_io` crate (0.16+ — see [Architecture](architecture.md) §1). See [Data Model](data-model.md) for the full enum-to-GEDCOM-tag mapping.

### Round-trip fidelity

| Data | Import | Export | Notes |
|------|--------|--------|-------|
| Persons (INDI) | Full | Full | All names (multiple `NAME` records), sex, events |
| Families (FAM) | Full | Full | Spouses, children, events, `FAMS`/`FAMC` back-links. GEDCOM 5.5.1 has two spouse slots, `HUSB` and `WIFE`: a husband and a wife take their own, a partner (or a second husband or wife) the one their sex points to, else the free one, and import reads them back as husband and wife. A third spouse cannot be written and is an export warning |
| Events with native tags | Lossless | Lossless | See EventType enum for tag list |
| Individual attributes | Lossless | Lossless | `CAST`, `DSCR`, `EDUC`, `IDNO`, `NATI`, `NCHI`, `NMR`, `PROP`, `RELI`, `SSN`, `TITL`, `FACT` each map to a dedicated EventType. A `TITL` keeps its text as the event's description, its `DATE` (a `FROM … TO …` period as a range), its `PLAC` and its `NOTE` |
| Occupation (`OCCU`) | Split | One tag per profession, or merged | A value with multiple professions (e.g. Geneanet's `"Presales, Trainer"`) is split on `,` (each part trimmed) into one `Occupation` event per profession, with its first letter uppercased (rest left as written). Export writes one `OCCU` tag per event unless `merge_occupations=true`, which collapses them back into a single comma-separated tag for importers that only support one profession field |
| Name aliases (`SURN`) | Split | One `NAME` per alias, or merged | The primary `PersonName` takes its surname from the `NAME` line, not `SURN` — Geneanet packs every surname alias it knows into a single `SURN` sub-tag (e.g. `"LE NADEN,NADAM"`) instead of matching `NAME`. That value is split on `,` (each part trimmed, primary excluded) into one `AlsoKnownAs` `PersonName` per alias. Export writes one `NAME`/`SURN` structure per name unless `merge_names=true`, which collapses non-primary names back into the primary name's comma-separated `SURN` tag for importers that only read the first `NAME` structure |
| Adoption (`ADOP`) | Full | Full | Individual-level event. The nested `FAMC` is not read: the child's own `FAMC` with `PEDI adopted` makes them an adopted child of the adoptive family |
| App-specific event types | N/A | As `EVEN` + `TYPE` | Confirmation, Military service, Civil union, etc. |
| Associations (`ASSO`/`RELA`) | Full | Full | Imported as `EventWitness` rows; exported as top-level `ASSO` on the INDI record (GEDCOM 5.5.1 nesting — Gramps rejects event-nested `ASSO`). Both Gramps encodings captured and deduplicated on import; a level-1 `ASSO` to an individual goes to the owner's baptism, else birth, else first event, after the witnesses nested in it |
| Sources (SOUR) | Full | Full | Title, author, publisher (`PUBL`, which `ged_io` does not write and the export adds itself), abbreviation; free-text `SOUR` citations preserved |
| Citations (with QUAY) | Full | Lossless | Page, the text quoted from the source (`DATA.TEXT`, over several lines too), confidence level. `QUAY` 0, 1, 2 and 3 are `VeryLow`, `Low`, `Medium` and `High`; a citation without `QUAY` is not assessed (no confidence) and is written without one, so no assessment is invented. `VeryHigh` has no `QUAY` of its own and is written as `3` |
| Media (OBJE) | Metadata; GEDZIP also restores held bytes | Metadata in `.ged`; metadata plus stored bytes in `.gdz` | File path, MIME type, title, description and physical medium use standard `FILE`, `FORM`, `TITL`, `NOTE`, and `FORM.TYPE` structures. Person, family, event and individual-attribute links use standard `OBJE` references — a scan documenting an `OCCU` or a `TITL` travels with its tag. A value split into several professions gives each of them the scan, and merging them back writes it once. A plain `.ged` never carries file bytes; a GEDZIP embeds every stored file and rewrites its `FILE` to the archive entry. Remote and unheld media retain only their original `FILE` reference. |
| Extended media metadata | OxidGene extension | OxidGene extension | `_OXIDGENE_MEDIA` is a versioned value beneath the owning `OBJE`. It preserves the original file name, structured date and calendar, document category, privacy, tags, media place with coordinates, record timestamps, and notes attached specifically to the media. Each exported page of a multi-page document owns a separate `OBJE` and extension value, so different page transcripts round-trip with their page rather than collapsing into one document note. It also mirrors standard title, description, and physical-medium values for an exact OxidGene round trip; other readers may ignore it. |
| Vignette identifications | OxidGene extension | OxidGene extension | Each person identification and its pixel rectangle round-trip beneath the owning `OBJE` as `_OXIDGENE_VIGNETTE`; software that does not know the extension ignores it. The same data survives both plain GEDCOM and GEDZIP export/import. |
| Dates (`DATE`) | Full | Full | Qualifier, range and calendar each have their own column. A calendar escape is read before each date (`ABT @#DJULIAN@ 1700`, `BET @#DJULIAN@ 1700 AND @#DJULIAN@ 1710`) or, as `ged_io` and earlier exports wrote it, before the whole value; it is written the GEDCOM 5.5.1 way, after the qualifier and before each bound |
| Places (PLAC) | Full | Full | Name + lat/lon coordinates |
| Notes (NOTE) | Full | Full | Inline and referenced notes, as many per person, event or attribute as the file holds — `ged_io` keeps one, so the import joins them before parsing and splits them again, and the export writes each one where `ged_io` writes the first. A note, text, cause, page, or source title, author, publication or abbreviation too long for one GEDCOM line continues on `CONC` lines, never split beside a space (readers trim a `CONC` value); on import, the spaces opening a `CONC` value are kept, so files whose writer split beside a space read back word for word. A `NOTE @N1@` pointer to a 5.5.1 note record, or a 7.0 `SNOTE`, imports the text of the record it points at; a pointer to a record the file does not hold is left out with a warning naming its line |
| Cause (CAUS) | Full | Full | On any event |
| Age at event (`AGE`) | Full | Full | On an individual event or attribute (each profession a split `OCCU` becomes keeps it), stored in canonical form (`34y`, `< 1y 6m`, `CHILD`). A value `ged_io` cannot read would make it reject the whole file, so it is repaired before parsing: one OxidGene reads as an age (`1y6m`, `child`) is rewritten in GEDCOM's form, and anything else — free text such as `2 AGE majeur`, an empty `AGE` — is left out and reported as one warning naming its line (never its value). Written without the GEDCOM 7.0 `PHRASE` `ged_io` would emit |
| Repositories (`REPO`) | Full | Full | Name, address (several lines), first phone, email and website, notes. `ged_io` writes only the name, so the export adds the rest |
| Source repository citations (`SOUR.REPO`) | Full | Full | Each becomes a link with its call number (`CALN`) and medium (`MEDI`, written in 5.5.1's lower case; a medium without a call number goes under an empty `CALN`). `ged_io` writes neither, so the export writes the whole structure. A citation without a pointer names a repository by its text — the line's, else its notes' — created once per distinct text; a pointer to a record the file does not hold is a warning. Several `CALN` under one citation collapse to the last (`ged_io`); a citation's own `NOTE` is not kept |
| Spouse ages (`HUSB.AGE`, `WIFE.AGE`) | Full | Full | On a family event, the age of the family's `HUSB` and `WIFE`; written back under the slot each spouse is exported in, a spouse without a slot being a warning |
| Agency (`AGNC`) | Full | Full | On an individual event or attribute, and a source's `DATA.AGNC`, which `ged_io` does not write and the export adds itself |
| Child pedigree (PEDI) | Full | Full | Biological, Adopted, Foster |
| Nicknames (`NICK`) | Full | Full | On the name that carries it. A non-primary `aka` name that only restates the primary name (or names nobody) to carry a `NICK` imports as a `Byname` holding the nickname alone, as the person form records one; a byname exports as such an `aka` name |
| Restriction (`RESN`) | Person and family privacy | Person and family privacy | `confidential` or `privacy` on an `INDI` or `FAM`, in any case and among several comma-separated values, makes the record `Private`; `locked`, another value or no `RESN` leaves it `Default`. A `Private` person or family is exported with `RESN confidential` (`ged_io` writes no record-level `RESN`, so the export adds it); `Public` has no `RESN` value and comes back as `Default` |
| Submitter (`SUBM`) | The record `HEAD.SUBM` points at fills the tree's submitter settings that are empty — never one already set; `Not Provided` is no name. Other `SUBM` records are a warning with their count | One record, pointed at by `HEAD.SUBM` | GEDCOM 5.5.1 requires a submitter. The record's `NAME` is the tree's submitter name, else the display name of its "Who am I?" person (`self_person_id`), else `Not Provided` (Gramps' wording). `EMAIL` and the multi-line `ADDR` are written only when set (`ged_io` writes neither, so the export adds them). A duplicated tree carries the settings over |
| Header charset | Declared `CHAR` decoded | `CHAR UTF-8` | A `.ged` is read in the character set it declares — a byte-order mark first, then `HEAD.CHAR`: UTF-8, UTF-16, ANSEL, ISO-8859-1 or -15, and `ANSI` as Windows-1252 — so files from GEDCOM 5 software keep their accents. Export declares UTF-8 explicitly |
| GEDCOM version | 5.5.1 + 7.0 | 5.5.1 only | ged_io auto-detects on import |

### GeneWeb `.gw`

A `.gw` is read by the `geneweb` crate, which converts it to the same `ged_io`
model a `.ged` is read into; the mapping above then applies unchanged. What the
conversion produces, and so what a `.gw` imports as:

| GeneWeb | Imported as |
|---|---|
| Witness of a `pevt` or `fevt` event | A witness of that event only, whether the file defines the witness elsewhere, inline, or nowhere else |
| Witness on a `fam` line | A witness of the union's marriage, created for it when the line gives no other detail, and joined to a `fevt` marriage that replaces the line's |
| Death reason (`k`, `m`, `e`, `s`, died young, presumed dead) | The death event's cause — `killed`, `murdered`, `executed`, `disappeared`, `died young` or `presumed dead`, as the `geneweb` crate words it — also when a `pevt` death replaces the line's, and exported as `CAUS`. The crate carries it as a `_GWDEATH <reason>` line in the death's note; any GEDCOM holding that line imports the same way, the line leaving the note and joining a cause the file states |
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

- Submitter records other than the one `HEAD.SUBM` points at, and the `SUBM`
  pointers of individual and family records (`INDI.SUBM`, `FAM.SUBM`), which
  `ged_io` does not parse
- A note on a source's repository citation (`SOUR.REPO.NOTE`) that has a
  pointer: the link keeps its call number and medium only
- Religion of a single event (`RELI` under an event; `RELI` as an individual
  attribute is imported, see above)
- Custom/vendor tags (`_CUSTOM`), OxidGene's own `_OXIDGENE_*` media
  extensions excepted

Skipping them emits no warning, except one counting the other submitter
records: the import warnings name only what the import repaired, could not
resolve, or left of the file's own description of itself.
