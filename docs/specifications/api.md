---
type: "API Specification"
title: "API Contract"
description: "REST and GraphQL contract for OxidGene, including endpoints, pagination, and payload conventions."
tags: [oxidgene, specification, api, contract]
timestamp: 2026-06-17T00:00:00Z
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

Transport-specific representation differences are allowed where the protocol
requires them. Binary file uploads and downloads use streaming HTTP endpoints;
GraphQL does not duplicate those payloads as base64. GraphQL exposes the same
durable job status and can create export jobs, while clients use REST to upload
import sources and download export artifacts. Pagination envelopes follow each
transport's conventions.

Direct media reads (`/file`, `/archive`, `/thumbnail`, and vignette `/image`)
remain HTTP representations because their cache validators, content types,
download disposition and conditional `ETag` semantics are HTTP behaviour rather
than product operations. GraphQL exposes the underlying metadata. Geneanet
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
- Soft-deleted records are excluded by default.
- User and imported content is returned verbatim and never translated.

### Authentication and privacy

Authentication and authorization are not implemented in the current MVP.
Privacy values are stored but not enforced, so clients must not claim that
private records are hidden. Future authorization uses the same domain checks
for REST and GraphQL. Until that work is complete, the API is local or private
infrastructure only and must not be published directly to an untrusted network.
Server defaults, container publication, CORS, and UI media rendering follow
[Cross-cutting Rules §7.1](cross-cutting.md#71-backend-exposure-before-authentication).

Direct HTTP media representations remain API-client transports, not browser
navigation destinations. Frontends fetch their bytes through the typed client
and render a local `data:`/`blob:` resource; they never emit `/api/v1`,
`/graphql`, or an API origin in user-visible links, image sources, form actions,
redirects, or new-window targets.

### Errors and consistency

Error envelopes, stable codes, safe messages, request IDs, logging, and
anonymization follow [Cross-cutting Rules §4–5](cross-cutting.md). Neither
surface returns stack traces, SQL, filesystem paths, credentials, or genealogy
in error details.

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
separate OpenAPI description. GraphiQL remains available at `GET /graphql` when
the `graphql` feature is enabled.

## 2. REST API

Base path: `/api/v1`.

### Trees

| Method | Path | Description |
|---|---|---|
| `GET` | `/trees` | List trees (cursor-paginated). REST list nodes add transient `import_in_progress: bool` and `import_job_id: UUID?` fields from the active file-job registry; these are not persisted tree fields and disappear for completed/failed jobs |
| `POST` | `/trees` | Create a tree |
| `GET` | `/trees/{tree_id}` | Get a tree |
| `PUT` | `/trees/{tree_id}` | Update a tree (incl. `sosa_root_person_id` and `self_person_id`) |
| `DELETE` | `/trees/{tree_id}` | Soft-delete a tree |
| `POST` | `/trees/{tree_id}/duplicate` | Duplicate a tree (deep copy) |

Used by: [Homepage](ui-home.md) (tree list, create, duplicate, delete)

### Persons

| Method | Path | Description |
|---|---|---|
| `GET` | `/trees/{tree_id}/persons` | List persons (cursor-paginated, filterable) |
| `POST` | `/trees/{tree_id}/persons` | Create a person |
| `GET` | `/trees/{tree_id}/persons/search` | Server-side person search with structured filters, sorting, and offset pagination (see below) |
| `GET` | `/trees/{tree_id}/persons/sosa/{number}` | Resolve a SOSA number to a person (relative to `Tree.sosa_root_person_id`) |
| `GET` | `/trees/{tree_id}/persons/{person_id}` | Get a person (with names, events, families) |
| `GET` | `/trees/{tree_id}/persons/{person_id}/detail-bundle` | Load the bounded read model for the person profile |
| `PUT` | `/trees/{tree_id}/persons/{person_id}` | Update a person |
| `DELETE` | `/trees/{tree_id}/persons/{person_id}` | Soft-delete a person |
| `GET` | `/trees/{tree_id}/persons/{person_id}/ancestors` | Get ancestors (depth param) |
| `GET` | `/trees/{tree_id}/persons/{person_id}/descendants` | Get descendants (depth param) |
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

Relation-label requests accept at most 1,024 combined person and family IDs.
Clients split larger logical sets into consecutive requests. Results are
strictly scoped to active people and families in the requested tree.

Used by: [Tree View](ui-genealogy-tree.md) (pedigree chart) · [Person Edit Modal](ui-person-edit-modal.md) (edit/delete)

### Person Names

| Method | Path | Description |
|---|---|---|
| `GET` | `/trees/{tree_id}/persons/{person_id}/names` | List names |
| `POST` | `/trees/{tree_id}/persons/{person_id}/names` | Add a name |
| `PUT` | `/trees/{tree_id}/persons/{person_id}/names/{name_id}` | Update a name |
| `DELETE` | `/trees/{tree_id}/persons/{person_id}/names/{name_id}` | Delete a name |

### Families

| Method | Path | Description |
|---|---|---|
| `GET` | `/trees/{tree_id}/families` | List families (cursor-paginated) |
| `POST` | `/trees/{tree_id}/families` | Create a family |
| `GET` | `/trees/{tree_id}/families/{family_id}` | Get a family (with spouses, children, events) |
| `PUT` | `/trees/{tree_id}/families/{family_id}` | Update a family |
| `DELETE` | `/trees/{tree_id}/families/{family_id}` | Soft-delete a family |

Used by: [Tree View](ui-genealogy-tree.md) (connectors) · [Person Edit Modal](ui-person-edit-modal.md) (couple edit)

### Family Members

| Method | Path | Description |
|---|---|---|
| `GET` | `/trees/{tree_id}/families/{family_id}/spouses` | List spouses |
| `POST` | `/trees/{tree_id}/families/{family_id}/spouses` | Add a spouse |
| `DELETE` | `/trees/{tree_id}/families/{family_id}/spouses/{spouse_id}` | Remove a spouse |
| `GET` | `/trees/{tree_id}/families/{family_id}/children` | List children |
| `POST` | `/trees/{tree_id}/families/{family_id}/children` | Add a child |
| `DELETE` | `/trees/{tree_id}/families/{family_id}/children/{child_id}` | Remove a child |

### Events

| Method | Path | Description |
|---|---|---|
| `GET` | `/trees/{tree_id}/events` | List events (cursor-paginated, filterable by type/person/family) |
| `POST` | `/trees/{tree_id}/events` | Create an event |
| `GET` | `/trees/{tree_id}/events/{event_id}` | Get an event |
| `PUT` | `/trees/{tree_id}/events/{event_id}` | Update an event |
| `DELETE` | `/trees/{tree_id}/events/{event_id}` | Soft-delete an event |
| `GET` | `/trees/{tree_id}/events/{event_id}/witnesses` | List event witnesses (GEDCOM `ASSO`) |
| `POST` | `/trees/{tree_id}/events/{event_id}/witnesses` | Add a witness (person + optional relation text) |
| `DELETE` | `/trees/{tree_id}/events/{event_id}/witnesses/{witness_id}` | Remove a witness |

Used by: [Tree View](ui-genealogy-tree.md) (events sidebar) · [Person Edit Modal](ui-person-edit-modal.md) (event blocks)

### Places

| Method | Path | Description |
|---|---|---|
| `GET` | `/trees/{tree_id}/places` | List places (cursor-paginated, searchable) |
| `POST` | `/trees/{tree_id}/places` | Create a place |
| `GET` | `/trees/{tree_id}/places/{place_id}` | Get a place |
| `PUT` | `/trees/{tree_id}/places/{place_id}` | Update a place |
| `DELETE` | `/trees/{tree_id}/places/{place_id}` | Delete a place |

### Sources

| Method | Path | Description |
|---|---|---|
| `GET` | `/trees/{tree_id}/sources` | List sources (cursor-paginated) |
| `POST` | `/trees/{tree_id}/sources` | Create a source |
| `GET` | `/trees/{tree_id}/sources/{source_id}` | Get a source |
| `PUT` | `/trees/{tree_id}/sources/{source_id}` | Update a source |
| `DELETE` | `/trees/{tree_id}/sources/{source_id}` | Soft-delete a source. With `?only_if_unused=true` the source is kept if any citation, note or media link still points at it — `204` deleted, `200` kept |

### Citations

| Method | Path | Description |
|---|---|---|
| `GET` | `/trees/{tree_id}/citations` | List citations in a cursor connection, filterable by `person_id`, `event_id`, `family_id`, and `source_id` |
| `POST` | `/trees/{tree_id}/citations` | Create a citation |
| `PUT` | `/trees/{tree_id}/citations/{citation_id}` | Update a citation — including `source_id`, which repoints it at another source in place |
| `DELETE` | `/trees/{tree_id}/citations/{citation_id}` | Delete a citation |

### Media

| Method | Path | Description |
|---|---|---|
| `GET` | `/trees/{tree_id}/media` | List media (cursor-paginated) |
| `POST` | `/trees/{tree_id}/media` | Create a media record from JSON metadata — names a file without holding it |
| `POST` | `/trees/{tree_id}/media/upload` | Upload a file. `multipart/form-data`: `file` (required), `title`, `description`, `media_id`, `document_id`. `201` for a new record, `200` when `media_id` attaches bytes to an existing one; `document_id` appends the file as the next page of a multi-page document |
| `POST` | `/trees/{tree_id}/media/document` | Create an empty multi-page document (`{title?}`). Pages are added by uploading with `document_id` |
| `POST` | `/trees/{tree_id}/gallery-bundle` | Load gallery data for `{media_ids, vignette_ids}`: thumbnail sources, linked event ids, up to four document-page previews, and cropped vignette sources |
| `GET` | `/trees/{tree_id}/media/{media_id}/pages` | A document's pages, in order |
| `PUT` | `/trees/{tree_id}/media/{media_id}/pages` | Set the page order (`{page_ids: [...]}`). Must name exactly this document's pages, once each — a partial list is refused rather than guessed at |
| `DELETE` | `/trees/{tree_id}/media/{media_id}/pages/{page_id}` | Detach a page as an ordinary media. Its attachments, identifications, and portrait references are removed; its bytes and transcript remain; remaining pages close the gap |
| `GET` | `/trees/{tree_id}/media/{media_id}` | Get media metadata |
| `GET` | `/trees/{tree_id}/media/{media_id}/file` | The stored bytes. `Content-Type` from the file, strong `ETag` (its SHA-256), `Cache-Control: private, max-age=3600`, `304` on a matching `If-None-Match`. `404` if the record has no bytes |
| `GET` | `/trees/{tree_id}/media/{media_id}/thumbnail` | Generated thumbnail (longest edge 400 px). `404` when the format cannot be rasterised — PDFs — so a gallery can fall back to an icon on the status alone |
| `GET` | `/trees/{tree_id}/media/{media_id}/archive` | Every page of a document, in one ZIP. Entries are prefixed `001_`, `002_` so unzipping restores the reading order whatever the page file names sort as. Pages with no stored bytes are skipped; `404` when none of them has any |
| `PUT` | `/trees/{tree_id}/media/{media_id}` | Update media metadata |
| `POST` | `/trees/{tree_id}/media/{media_id}/tags` | Add one tag (`{tag}`), idempotently by case-insensitive value |
| `DELETE` | `/trees/{tree_id}/media/{media_id}/tags` | Remove one tag (`{tag}`) without replacing the other tags |
| `GET` | `/trees/{tree_id}/media/{media_id}/deletion-status?allowed_link_id={link_id}` | Whether the gallery link is the sole external reference (`{can_delete: bool}`); used to ask for confirmation only when deletion is certain |
| `DELETE` | `/trees/{tree_id}/media/{media_id}` | Permanently delete the media, its related rows and unshared stored objects. With `?only_if_unreferenced_elsewhere=true&allowed_link_id={link_id}`, keep it when any reference other than that gallery link remains (`204` deleted, `200` retained) |

GraphQL mirrors the status endpoint with `canDeleteMedia(treeId:, id:, allowedLinkId:)`, returning the same eligibility boolean before `deleteMedia` is called.

The gallery bundle accepts at most 1,024 media and vignette ids combined. It
resolves each database collection in one query and reads blobs with bounded
concurrency. Clients split larger galleries into as many bundles as necessary.
The per-media thumbnail, page-list, and vignette-image endpoints remain for a
viewer or editor that opens one selected asset; the initial grid does not call
them once per tile.

**Upload rules.** The type is decided by the file's magic bytes, not by the declared MIME type or the extension: JPEG, PNG, GIF, BMP, TIFF, WebP, ICO and PDF are accepted, everything else is a `400`. Maximum 128 MiB — comfortably above what the services we exchange with take (Geneanet caps a media file at 50 MB and accepts only JPEG, PNG, GIF and PDF), because a 1200 dpi register spread or a few-hundred-page dossier clears 64 MiB unremarkably. Larger still is EPIC H's chunked-upload problem. Uploading a file the tree already holds re-uses the stored bytes and still creates a second record, which is what a census page shared by eight siblings needs.

**Three kinds of media.** *Stored* — `storage_key` is set, we serve the bytes, there is a thumbnail and crops can be drawn. *Remote* — `file_path` is an `http(s)` URL: recorded, never fetched by us, so no thumbnail and no crop, and the browser goes to the origin directly. *Unheld* — a record naming a file nobody uploaded, which is where every GEDCOM import starts. `PUT .../media/{id}` may edit `file_path` for the last two and **refuses it for a stored one**: there `file_path` is the value a GEDCOM export writes back, and repointing it would make the export describe a file we are serving something else for. A remote `mime_type` is guessed from the URL's extension when not given — the only evidence available without fetching, and it decides whether a viewer embeds the file or offers it as a download.

**A media carries what a fact carries.** `PUT .../media/{id}` takes `title`, `description`, `date_value`, `date_value2`, `date_qualifier`, `calendar`, `place_id`, `source_media_type` and `document_category`, so "a photograph taken around 1890 at Nantes" is written the way a birth around 1890 is. Tags are independent rows, added and removed through their two dedicated endpoints; this prevents one editor's save from overwriting another editor's tag. Labels are trimmed and case-insensitively unique. Multi-page document tags live on the document row, never its pages. GraphQL exposes the same operations as `addMediaTag` and `removeMediaTag`; `GqlMedia.tags` remains the ordered list for reads. `date_sort` is **not** accepted: the server derives it from `calendar` + `date_value`, exactly as for an event. Notes about a document go on `note.media_id` (`POST /notes` with `media_id`, `GET /notes?media_id=`). There is deliberately **no source field** — a media *is* a source document.

**Two fields for what looks like one question.** `source_media_type` is GEDCOM's `SOURCE_MEDIA_TYPE`, exactly — `photo`, `manuscript`, `tombstone`, `fiche`, `film`, `map`, `newspaper`, `book`, `card`, `magazine`, `audio`, `video`, `electronic`, `other` — and is what an export writes and other genealogy software reads. `document_category` is the distinction GEDCOM cannot draw: a census return, a marriage contract and a conscription register are all `manuscript` to it. Sending a category without a medium also sets the medium that category implies, so a census return exports as `MANUSCRIPT` rather than `OTHER`; sending both keeps both. `document_category` accepts an explicit `null` to unclassify. See [Data Model](data-model.md) (Media).

**Multi-page documents.** F.1's `page_count` counts pages *inside* one file (a PDF, a TIFF). A register scanned to a folder of JPEGs is a different thing: a `media` with `is_document`, whose pages are `media` rows carrying `parent_media_id` + `page_index`. A page is a media in its own right — bytes, thumbnail, dimensions, crops — so upload, storage, thumbnailing and serving are the endpoints above, unchanged. Listings filter `parent_media_id IS NULL`, so a nine-page act is one entry rather than ten. The document carries the title, date, place, description and note; `page_count` is recomputed from the pages that exist.

**Storage.** Media bytes are content-addressed under `{tree_id}/{aa}/{bb}/{sha256}.{ext}`. `OXIDGENE_MEDIA_BACKEND=filesystem` is the default and stores them below `OXIDGENE_MEDIA_ROOT` (the platform user-data directory, `~/.local/share/oxidgene/media` on Linux, by default). `OXIDGENE_MEDIA_BACKEND=s3` stores the same keys in the bucket configured by `OXIDGENE_S3_BUCKET`, `OXIDGENE_S3_REGION`, optional `OXIDGENE_S3_ENDPOINT`, `OXIDGENE_S3_ACCESS_KEY_ID`, and `OXIDGENE_S3_SECRET_ACCESS_KEY`. HTTP endpoints are accepted only when explicitly configured, for local S3-compatible services such as RustFS. Keys are scoped per tree so a purge can delete one tree by prefix without affecting another.

Used by: [Person Edit Modal](ui-person-edit-modal.md) (media section)

### Vignettes

A vignette is a rectangle on a stored media file — one parish-register page carries entries for several unrelated families, and each is a crop rather than a copy. Coordinates are in the source image's own pixels, so re-scanning at a higher resolution does not orphan them.

| Method | Path | Description |
|---|---|---|
| `GET` | `/trees/{tree_id}/media/{media_id}/vignettes` | Vignettes on a media file, in page order |
| `POST` | `/trees/{tree_id}/media/{media_id}/vignettes` | Create one. Body: `x`, `y`, `width`, `height` (required), `page`, `person_id`, `event_id` |
| `GET` | `/trees/{tree_id}/vignettes?person_id=…` / `?event_id=…` | Vignettes attributed to a person, or standing as evidence for an event. Exactly one filter is required |
| `GET` | `/trees/{tree_id}/vignettes/{vignette_id}` | Get one |
| `PUT` | `/trees/{tree_id}/vignettes/{vignette_id}` | Move or re-attribute. The four rectangle fields travel together — all or none |
| `DELETE` | `/trees/{tree_id}/vignettes/{vignette_id}` | Delete it. Hard delete; the media is untouched |
| `GET` | `/trees/{tree_id}/vignettes/{vignette_id}/image` | The cropped region as its own JPEG, derived on read. `400` for a PDF — rasterising one needs a rendering engine OxidGene does not ship |

A rectangle that does not fit the media it crops is a `400` at write time, so a stored vignette always describes a region that exists.

`PUT /trees/{id}` accepts `default_privacy` (`"public" | "private"`) — what
`"default"` resolves to for everything in that tree. **Privacy** is accepted on
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

`GET /portraits` exposes the complete portrait assignments as an inventory
operation. First-party display surfaces do not use it: a pedigree, result page,
or picker generally needs portraits for only a bounded set of people.

**Drawing portraits.** Screens submit the person ids they need to
`POST /portrait-images` rather than loading the portrait inventory or
downloading one image per person. The operation accepts at most 1,024 ids,
resolves them in one database query, and returns locally held thumbnails or
cropped vignettes as data URLs.
Remote portraits retain their `http(s)` source; unavailable portraits are
omitted. Clients split larger sets into as many requests of at most 1,024 ids
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

### Notes

| Method | Path | Description |
|---|---|---|
| `GET` | `/trees/{tree_id}/notes` | List notes in a cursor connection, filterable by `person_id`, `event_id`, `family_id`, `source_id`, and `media_id` |
| `POST` | `/trees/{tree_id}/notes` | Create a note |
| `GET` | `/trees/{tree_id}/notes/{note_id}` | Get a note |
| `PUT` | `/trees/{tree_id}/notes/{note_id}` | Update a note |
| `DELETE` | `/trees/{tree_id}/notes/{note_id}` | Soft-delete a note |

### Snapshot

| Method | Path | Description |
|---|---|---|
| `GET` | `/trees/{tree_id}/snapshot` | Full tree snapshot (persons, names, events, places, spouses, children) in one response |

> Legacy endpoint still used by the person profile to enrich events with
> witness and family context. It must be removed, together with both REST and
> GraphQL coverage, once [Data Model §4](data-model.md) includes that context.

### Dictionary

Aggregations backing the [Dictionary](ui-dictionary.md) page. Value endpoints return distinct values + usage counts; usage endpoints return the persons behind one value, resolved server-side into `PersonUsageEntry` (id, name parts, birth/death years) in one bulk query.

Each year is paired with a `birth_qualifier` / `death_qualifier` so a list can hedge the same way a pedigree card does (`ca 1849`, `< 1917`). The qualifier sits **beside** the year rather than being folded into it: `birth_year` stays an integer the client can sort on, and a `"ca 1849"` in that field would break both that and the search grid.

| Method | Path | Description |
|---|---|---|
| `GET` | `/trees/{tree_id}/dictionary/family-names` | Distinct surnames + person counts |
| `GET` | `/trees/{tree_id}/dictionary/family-names/usage?value=...` | Persons carrying a surname |
| `PATCH` | `/trees/{tree_id}/dictionary/family-names/particle` | Bulk-edit — body `{ "value": "...", "particle": "..." }` re-cuts every `PersonName` carrying surname `value` at `particle` (empty = no particle). `particle` must already be at the head of `value`; rows already cut that way are skipped. Triggers a full projection rebuild when anything changed |
| `GET` | `/trees/{tree_id}/dictionary/occupations` | Distinct occupation labels + counts |
| `GET` | `/trees/{tree_id}/dictionary/occupations/usage?value=...` | Persons with an occupation |
| `GET` | `/trees/{tree_id}/dictionary/sources` | Sources + citation counts |
| `GET` | `/trees/{tree_id}/dictionary/sources/{source_id}/usage` | Persons citing a source |
| `GET` | `/trees/{tree_id}/dictionary/places` | Places + reference counts (events + media) |
| `GET` | `/trees/{tree_id}/dictionary/places/{place_id}/usage` | Persons referencing a place |

### Import / export

GEDCOM and GEDZIP are read and written; GeneWeb `.gw` is read only — OxidGene
imports the format, it does not produce it.

| Method | Path | Description |
|---|---|---|
| `POST` | `/trees/{tree_id}/import-jobs?format=gedcom\|gedzip\|geneweb&filename=name.gw` | Stream a raw genealogy file to durable job storage and create an asynchronous import. Returns `202 { "job_id": UUID }` after the source is stored and the job is committed. `filename` is optional GeneWeb provenance metadata only. The 1 GiB limit is enforced while streaming. Workers copy the source to disposable scratch space; media extracted from GEDZIP are persisted through `MediaStore` |
| `GET` | `/trees/{tree_id}/import-jobs/{job_id}` | Poll `{ phase, done, total, result?, geneanet_result?, error? }`. File-import phases are `starting`, `parsing`, `media`, `database`, `projections`, `completed`, and `failed`; Geneanet additionally uses `people` and `matching`. A completed status retains either the standard `ImportResponse` or the Geneanet receipt; failure exposes a stable error code rather than internal details |
| `POST` | `/trees/{tree_id}/export-jobs?merge_occupations=bool&merge_names=bool` | Create a durable asynchronous GEDZIP export. Returns `202 { "job_id": UUID }`; archive creation and media reads run in the worker |
| `GET` | `/trees/{tree_id}/export-jobs/{job_id}` | Poll `{ phase, done, total, download_url?, warnings, error? }`. `download_url` appears only after the artifact is complete |
| `GET` | `/trees/{tree_id}/export-jobs/{job_id}/download` | Stream the completed GEDZIP artifact as `application/zip` with `Content-Disposition: attachment`; returns an error while the job is incomplete |
| `POST` | `/trees/{tree_id}/gedcom/import` | Import a GEDCOM file — JSON body `{ "gedcom": "…" }`, 1 GiB body limit (the JSON escaping costs a further ~1.4× over the file itself) |
| `POST` | `/trees/{tree_id}/gedzip/import` | Import a GEDZIP archive (`.gdz`): the `gedcom.ged` it wraps **and** the media files it carries. Body is the **raw archive** (`application/zip`), not JSON — base64 in an envelope would inflate a photo album by a third. Every medium whose `FILE` names an entry in the archive is stored, thumbnailed and written as a held medium; one naming an entry the archive lacks stays an unheld record and says so in `warnings`, as does a file no `OBJE` names. Matching folds separators and case, so a producer's `.\Media\Photo.JPG` still finds `media/photo.jpg`. The archive and `gedcom.ged` entry each have a 1 GiB limit, and each decompressed medium has the ordinary 128 MiB per-file limit. Media are read sequentially and there is no additional cumulative album limit |
| `POST` | `/trees/{tree_id}/geneweb/import?filename=name.gw` | Import a GeneWeb `.gw` file. Body is the **raw file bytes** (`application/octet-stream`), not JSON: `.gw` is ISO-8859-1 unless the file opts into UTF-8 with an `encoding:` directive, and the switch can happen mid-file, so only the reader can decode it. `filename` (default `import.gw`) is recorded on every family and quoted in warnings. 1 GiB body limit |
| `GET` | `/trees/{tree_id}/gedcom/export?format=gedcom\|gedzip&merge_occupations=bool&merge_names=bool` | Export tree as GEDCOM text (default) or GEDZIP archive (`application/zip`, includes media files). `merge_occupations` (default `false`) collapses each person's multiple `OCCU` tags back into one, comma-separated. `merge_names` (default `false`) collapses each person's non-primary names into the primary name's `SURN` tag, comma-separated. Both are for importers (e.g. Geneanet) that only support a single profession field / read the first `NAME` structure |

The three synchronous format endpoints remain compatibility surfaces. The UI
uses durable jobs for every file import and every GEDZIP export, regardless of
size. It polls the job after the single initiating action and automatically
starts the download when an export artifact is ready. Raw streaming, browser
upload progress and artifact downloads are HTTP transport concerns and
therefore use REST.

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
| `POST` | `/geneanet/session/decode` | Read one back. Body is the file itself; a ZIP and a bare JSON collection are told apart by content, not extension. Refuses anything that is not a collection, so a wrong file is reported rather than producing an import that attaches nothing |
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
content match lands and downloading the rest. A Geneanet import job staged
before the field existed replays as `"originals"`, which is what it was.
Media bytes are never included in these request bodies.
The two wizard routes that sit outside the tree nest run under a **32 MiB body
limit**: a 10 000-person tree is around 8 MiB base64-encoded before the mapping
is added, and that number grows with tree size rather than with how many
photographs somebody owns. The tree-scoped routes here share the **1 GiB**
allowance of a plain import.

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
| `POST` | `/trees/{tree_id}/profiles/drop` | Drop a tree's projections (rebuilt lazily on next read) |
| `GET` | `/trees/{tree_id}/pedigree/{root_person_id}?ancestor_depth=N&descendant_depth=N` | Assemble a windowed pedigree for a root person |
| `PATCH` | `/trees/{tree_id}/pedigree/{root_person_id}/expand?direction=ancestors\|descendants&from_depth=N&to_depth=N&other_depth=N` | Expand pedigree depth (returns only new nodes/edges). `other_depth` is the depth already loaded in the opposite direction (default `0`) |

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
case- and accent-insensitive. An empty or missing `q` is valid: structured
filters can be used alone, and no filters at all select browse mode. The
response's `total_count` is computed before `limit` and `offset` are applied.

Used by: [Tree View](ui-genealogy-tree.md) (pedigree chart) · [Person Profile](ui-person-profile.md) (person detail) · [Search Results](ui-search-results.md) (search)

All mutation endpoints refresh affected projections in their write transaction.
See [Data Model §4.4](data-model.md).

### Reference Content

Read-only lookup of static reference content (occupation sheets and given-name
meanings) shown on the person profile. It is not tied to a tree: `term` is the
raw free-text GEDCOM value. Matching ignores case, accents, and punctuation,
supports aliases such as gendered variants, and falls back to the first token
of a compound given name. Source content lives in
`oxidgene-api/src/reference/data/*.json`, one file per language and data type,
is compressed at build time, and is loaded once in memory.

| Method | Path | Description |
|---|---|---|
| `GET` | `/reference/{lang}/occupations?term=...` | Occupation fiche (label, summary, text) for `lang` (`fr`/`en`); 404 if none |
| `GET` | `/reference/{lang}/given-names?term=...` | Given-name fiche (label, origin, meaning, text, feast day) for `lang`; 404 if none |
| `POST` | `/reference/{lang}/given-names/bundle` | Ordered, deduplicated matches for `{terms: string[]}`; unknown terms are omitted |

These routes sit at `/api/v1/reference/...`, not under a tree. Used by:
[Person Profile](ui-person-profile.md). A physical batch accepts at most 128
terms. Clients split larger logical operations into consecutive batches and
merge every response; they never truncate terms at the limit.

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

Endpoint: `/graphql` using POST for queries and mutations. No subscription
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
  ancestors(treeId: ID!, personId: ID!, maxDepth: Int): [PersonWithDepth!]!
  descendants(treeId: ID!, personId: ID!, maxDepth: Int): [PersonWithDepth!]!
  portraits(treeId: ID!): [Portrait!]!
  portraitImages(treeId: ID!, personIds: [ID!]!): [PortraitImage!]!

  # Dictionary and static reference content
  dictionaryFamilyNames(treeId: ID!): [DictionaryEntry!]!
  dictionaryOccupations(treeId: ID!): [DictionaryEntry!]!
  dictionarySources(treeId: ID!, prefix: String): [SourceDictionaryEntry!]!
  dictionarySourceDrill(treeId: ID!, prefix: String): SourceDictionaryDrill!
  dictionaryPlaces(treeId: ID!): [PlaceDictionaryEntry!]!
  familyNameUsage(treeId: ID!, value: String!): [PersonUsageEntry!]!
  occupationUsage(treeId: ID!, value: String!): [PersonUsageEntry!]!
  sourceUsage(sourceId: ID!): [PersonUsageEntry!]!
  placeUsage(placeId: ID!): [PersonUsageEntry!]!
  occupationReference(language: String!, term: String!): OccupationReference
  givenNameReference(language: String!, term: String!): GivenNameReference
  givenNameReferences(language: String!, terms: [String!]!): [GivenNameReferenceMatch!]!

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

  # Media
  mediaList(treeId: ID!, first: Int, after: String): MediaConnection!
  media(treeId: ID!, id: ID!): Media
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
  exportGedcom(treeId: ID!, mergeOccupations: Boolean, mergeNames: Boolean): ExportGedcomResult!
  importJobStatus(treeId: ID!, jobId: ID!): ImportJobStatus!
  exportJobStatus(treeId: ID!, jobId: ID!): ExportJobStatus!

  # Read projections (see Data Model section 4) — mirrors the REST routes
  personProfile(treeId: ID!, personId: ID!): GqlPersonProfile!
  personProfiles(treeId: ID!): [GqlPersonProfile!]!
  pedigree(treeId: ID!, rootPersonId: ID!, ancestorDepth: Int!, descendantDepth: Int!): GqlPedigree!
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
  treeSnapshot(treeId: ID!): TreeSnapshot!
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

  # Person Names
  addPersonName(treeId: ID!, personId: ID!, input: PersonNameInput!): PersonName!
  updatePersonName(treeId: ID!, personId: ID!, nameId: ID!, input: PersonNameInput!): PersonName!
  deletePersonName(treeId: ID!, personId: ID!, nameId: ID!): Boolean!

  # Dictionary — bulk surname-particle edit (mirrors the REST PATCH route)
  setFamilyNameParticle(treeId: ID!, input: SetFamilyNameParticleInput!): GqlFamilyNameParticleUpdate!

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
  removeEventWitness(treeId: ID!, id: ID!): Boolean!

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
  detachMediaPage(treeId: ID!, documentId: ID!, pageId: ID!): Media!
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
  importGeneanet(treeId: ID!, input: GeneanetImportInput!): BackgroundJobStarted!

  # Read projections (see Data Model section 4) — mirrors the REST routes
  expandPedigree(treeId: ID!, rootPersonId: ID!, direction: PedigreeDirection!, fromDepth: Int!, toDepth: Int!, otherDepth: Int = 0): GqlPedigreeDelta!
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
`indexGeneanetArchives` and paths returned from `decodeGeneanetSession` are
desktop-only because they refer to the local filesystem. The runtime capability
that protects the REST data plane also protects these GraphQL fields. A caller
polls `importJobStatus`; `result` is set for GEDCOM/GEDZIP/GeneWeb jobs and
`geneanetResult` is set for a completed Geneanet job.

### Key Types

```graphql
type Tree {
  id: ID!
  name: String!
  description: String
  personCount: Int!
  familyCount: Int!
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
  vignettesCount: Int!
  skipped: [String!]!
  warnings: [String!]!
}

type ExportJobStatus {
  phase: String!
  done: Int!
  total: Int!
  downloadUrl: String
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
# Place, Source, Citation, Note, and Media connection types.

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
  motherId: ID
  motherDisplayName: String
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
  birthPlace: String
  deathYear: String
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
| Families (FAM) | Full | Full | Spouses, children, events, `FAMS`/`FAMC` back-links |
| Events with native tags | Lossless | Lossless | See EventType enum for tag list |
| Individual attributes | Lossless | Lossless | `CAST`, `DSCR`, `EDUC`, `IDNO`, `NATI`, `NCHI`, `NMR`, `PROP`, `RELI`, `SSN`, `TITL`, `FACT` each map to a dedicated EventType |
| Occupation (`OCCU`) | Split | One tag per profession, or merged | A value with multiple professions (e.g. Geneanet's `"Presales, Trainer"`) is split on `,` (each part trimmed) into one `Occupation` event per profession, with its first letter uppercased (rest left as written). Export writes one `OCCU` tag per event unless `merge_occupations=true`, which collapses them back into a single comma-separated tag for importers that only support one profession field |
| Name aliases (`SURN`) | Split | One `NAME` per alias, or merged | The primary `PersonName` takes its surname from the `NAME` line, not `SURN` — Geneanet packs every surname alias it knows into a single `SURN` sub-tag (e.g. `"LE NADEN,NADAM"`) instead of matching `NAME`. That value is split on `,` (each part trimmed, primary excluded) into one `AlsoKnownAs` `PersonName` per alias. Export writes one `NAME`/`SURN` structure per name unless `merge_names=true`, which collapses non-primary names back into the primary name's comma-separated `SURN` tag for importers that only read the first `NAME` structure |
| Adoption (`ADOP`) | Full | Full | Individual-level event; adoptive family via nested `FAMC` |
| App-specific event types | N/A | As `EVEN` + `TYPE` | Confirmation, Military service, Civil union, etc. |
| Associations (`ASSO`/`RELA`) | Full | Full | Imported as `EventWitness` rows; exported as top-level `ASSO` on the INDI record (GEDCOM 5.5.1 nesting — Gramps rejects event-nested `ASSO`). Both Gramps encodings captured and deduplicated on import |
| Sources (SOUR) | Full | Full | Title, author, publisher, abbreviation; free-text `SOUR` citations preserved |
| Citations (with QUAY) | Full | Full | Page, text, confidence level |
| Media (OBJE) | Metadata; GEDZIP also restores held bytes | Metadata in `.ged`; metadata plus stored bytes in `.gdz` | File path, MIME type, title, description and physical medium use standard `FILE`, `FORM`, `TITL`, `NOTE`, and `FORM.TYPE` structures. Person, family and event links use standard `OBJE` references. A plain `.ged` never carries file bytes; a GEDZIP embeds every stored file and rewrites its `FILE` to the archive entry. Remote and unheld media retain only their original `FILE` reference. |
| Extended media metadata | OxidGene extension | OxidGene extension | `_OXIDGENE_MEDIA` is a versioned value beneath the owning `OBJE`. It preserves the original file name, structured date and calendar, document category, privacy, tags, media place with coordinates, record timestamps, and notes attached specifically to the media. Each exported page of a multi-page document owns a separate `OBJE` and extension value, so different page transcripts round-trip with their page rather than collapsing into one document note. It also mirrors standard title, description, and physical-medium values for an exact OxidGene round trip; other readers may ignore it. |
| Vignette identifications | OxidGene extension | OxidGene extension | Each person identification and its pixel rectangle round-trip beneath the owning `OBJE` as `_OXIDGENE_VIGNETTE`; software that does not know the extension ignores it. The same data survives both plain GEDCOM and GEDZIP export/import. |
| Places (PLAC) | Full | Full | Name + lat/lon coordinates |
| Notes (NOTE) | Full | Full | Inline and referenced notes |
| Cause (CAUS) | Full | Full | On any event |
| Child pedigree (PEDI) | Full | Full | Biological, Adopted, Foster |
| Header charset | — | `CHAR UTF-8` | Export declares UTF-8 explicitly |
| GEDCOM version | 5.5.1 + 7.0 | 5.5.1 only | ged_io auto-detects on import |

### Not currently imported (silently skipped)

- Repository records (`REPO`)
- Submitter records (`SUBM`)
- Age at event (`AGE`)
- Agency (`AGNC`)
- Custom/vendor tags (`_CUSTOM`)
