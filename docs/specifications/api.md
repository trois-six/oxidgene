---
type: "API Specification"
title: "API Contract"
description: "REST and GraphQL contract for OxidGene, including endpoints, pagination, and payload conventions."
tags: [oxidgene, specification, api, contract]
timestamp: 2026-06-17T00:00:00Z
---


# API Contract

> Part of the [OxidGene Specifications](index.md).
> See also: [Data Model](data-model.md) · [Architecture](architecture.md)

---

## 1. REST API

Base path: `/api/v1`
The API should eventually expose an OpenAPI description in YAML under the path: `/api/swagger.yaml` — **not implemented yet**.

### Trees

| Method | Path | Description |
|---|---|---|
| `GET` | `/trees` | List trees (cursor-paginated) |
| `POST` | `/trees` | Create a tree |
| `GET` | `/trees/{tree_id}` | Get a tree |
| `PUT` | `/trees/{tree_id}` | Update a tree (incl. `sosa_root_person_id`) |
| `DELETE` | `/trees/{tree_id}` | Soft-delete a tree |
| `POST` | `/trees/{tree_id}/duplicate` | Duplicate a tree (deep copy) |

Used by: [Homepage](ui-home.md) (tree list, create, duplicate, delete)

### Persons

| Method | Path | Description |
|---|---|---|
| `GET` | `/trees/{tree_id}/persons` | List persons (cursor-paginated, filterable) |
| `POST` | `/trees/{tree_id}/persons` | Create a person |
| `GET` | `/trees/{tree_id}/persons/search?q=...&limit=N&offset=N` | Server-side person search (paginated `SearchResult`, backed by `person_search_fts`; empty `q` = browse mode) |
| `GET` | `/trees/{tree_id}/persons/sosa/{number}` | Resolve a SOSA number to a person (relative to `Tree.sosa_root_person_id`) |
| `GET` | `/trees/{tree_id}/persons/{person_id}` | Get a person (with names, events, families) |
| `PUT` | `/trees/{tree_id}/persons/{person_id}` | Update a person |
| `DELETE` | `/trees/{tree_id}/persons/{person_id}` | Soft-delete a person |
| `GET` | `/trees/{tree_id}/persons/{person_id}/ancestors` | Get ancestors (depth param) |
| `GET` | `/trees/{tree_id}/persons/{person_id}/descendants` | Get descendants (depth param) |

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
| `DELETE` | `/trees/{tree_id}/sources/{source_id}` | Soft-delete a source |

### Citations

| Method | Path | Description |
|---|---|---|
| `GET` | `/trees/{tree_id}/citations` | List citations (filterable by person/event/family) |
| `POST` | `/trees/{tree_id}/citations` | Create a citation |
| `PUT` | `/trees/{tree_id}/citations/{citation_id}` | Update a citation |
| `DELETE` | `/trees/{tree_id}/citations/{citation_id}` | Delete a citation |

### Media

| Method | Path | Description |
|---|---|---|
| `GET` | `/trees/{tree_id}/media` | List media (cursor-paginated) |
| `POST` | `/trees/{tree_id}/media` | Create a media record (JSON metadata) |
| `GET` | `/trees/{tree_id}/media/{media_id}` | Get media metadata |
| `PUT` | `/trees/{tree_id}/media/{media_id}` | Update media metadata |
| `DELETE` | `/trees/{tree_id}/media/{media_id}` | Soft-delete media |

> **Planned (E.7 media management):** binary upload (`POST` multipart) and file download (`GET .../file`) endpoints are not implemented yet — today only metadata records exist; media binaries referenced by GEDZIP export must already be on disk at `file_path`.

Used by: [Person Edit Modal](ui-person-edit-modal.md) (media section)

### Media Links

| Method | Path | Description |
|---|---|---|
| `GET` | `/trees/{tree_id}/media-links` | List media links (filterable by target) |
| `POST` | `/trees/{tree_id}/media-links` | Create a media link |
| `DELETE` | `/trees/{tree_id}/media-links/{link_id}` | Delete a media link |

### Notes

| Method | Path | Description |
|---|---|---|
| `GET` | `/trees/{tree_id}/notes` | List notes (filterable by target) |
| `POST` | `/trees/{tree_id}/notes` | Create a note |
| `GET` | `/trees/{tree_id}/notes/{note_id}` | Get a note |
| `PUT` | `/trees/{tree_id}/notes/{note_id}` | Update a note |
| `DELETE` | `/trees/{tree_id}/notes/{note_id}` | Soft-delete a note |

### Snapshot

| Method | Path | Description |
|---|---|---|
| `GET` | `/trees/{tree_id}/snapshot` | Full tree snapshot (persons, names, events, places, spouses, children) in one response |

> Legacy endpoint predating the read projections. Still used by the person profile page to enrich events (witness/family context). Candidate for removal once the person projection payload covers those needs — see [Read Projections](read-projections.md).

### Dictionary

Aggregations backing the [Dictionary](ui-dictionary.md) page. Value endpoints return distinct values + usage counts; usage endpoints return the persons behind one value, resolved server-side into `PersonUsageEntry` (id, name parts, birth/death years) in one bulk query.

| Method | Path | Description |
|---|---|---|
| `GET` | `/trees/{tree_id}/dictionary/family-names` | Distinct surnames + person counts |
| `GET` | `/trees/{tree_id}/dictionary/family-names/usage?value=...` | Persons carrying a surname |
| `GET` | `/trees/{tree_id}/dictionary/occupations` | Distinct occupation labels + counts |
| `GET` | `/trees/{tree_id}/dictionary/occupations/usage?value=...` | Persons with an occupation |
| `GET` | `/trees/{tree_id}/dictionary/sources` | Sources + citation counts |
| `GET` | `/trees/{tree_id}/dictionary/sources/{source_id}/usage` | Persons citing a source |
| `GET` | `/trees/{tree_id}/dictionary/places` | Places + reference counts (events + media) |
| `GET` | `/trees/{tree_id}/dictionary/places/{place_id}/usage` | Persons referencing a place |

### Import / export

GEDCOM is read and written; GeneWeb `.gw` is read only — OxidGene imports the
format, it does not produce it.

| Method | Path | Description |
|---|---|---|
| `POST` | `/trees/{tree_id}/gedcom/import` | Import a GEDCOM file — JSON body `{ "gedcom": "…" }`, 10 MiB body limit |
| `POST` | `/trees/{tree_id}/geneweb/import?filename=name.gw` | Import a GeneWeb `.gw` file. Body is the **raw file bytes** (`application/octet-stream`), not JSON: `.gw` is ISO-8859-1 unless the file opts into UTF-8 with an `encoding:` directive, and the switch can happen mid-file, so only the reader can decode it. `filename` (default `import.gw`) is recorded on every family and quoted in warnings. 10 MiB body limit |
| `GET` | `/trees/{tree_id}/gedcom/export?format=gedcom\|gedzip&merge_occupations=bool&merge_names=bool` | Export tree as GEDCOM text (default) or GEDZIP archive (`application/zip`, includes media files). `merge_occupations` (default `false`) collapses each person's multiple `OCCU` tags back into one, comma-separated. `merge_names` (default `false`) collapses each person's non-primary names into the primary name's `SURN` tag, comma-separated. Both are for importers (e.g. Geneanet) that only support a single profession field / read the first `NAME` structure |

Both import endpoints return the same `ImportResponse` shape and trigger a full
projection rebuild of the tree.

Used by: [Homepage](ui-home.md) (card menu import) · [Settings](ui-settings.md) (export section)

### Profiles & Pedigree

Pre-built, denormalized read models for instant page rendering. Person profiles are materialized in the `person_denorm` table; pedigrees are assembled per request by walking the family links and joining the reached persons against those profiles. See [Read Projections](read-projections.md) for the full architecture.

| Method | Path | Description |
|---|---|---|
| `GET` | `/trees/{tree_id}/profiles/{person_id}` | Get a single person projection (full denormalized profile) |
| `GET` | `/trees/{tree_id}/profiles` | Get every person projection of a tree |
| `POST` | `/trees/{tree_id}/profiles/rebuild` | Force a full projection rebuild for a tree |
| `POST` | `/trees/{tree_id}/profiles/rebuild/{person_id}` | Rebuild a single person's projection |
| `POST` | `/trees/{tree_id}/profiles/drop` | Drop a tree's projections (rebuilt lazily on next read) |
| `GET` | `/trees/{tree_id}/pedigree/{root_person_id}?ancestor_depth=N&descendant_depth=N` | Assemble a windowed pedigree for a root person |
| `PATCH` | `/trees/{tree_id}/pedigree/{root_person_id}/expand?direction=ancestors\|descendants&from_depth=N&to_depth=N&other_depth=N` | Expand pedigree depth (returns only new nodes/edges). `other_depth` is the depth already loaded in the opposite direction (default `0`) |

> **Sprint E.9 renamed these routes** off `/cache/*`, which named an implementation that no longer exists: `/cache/persons*` → `/profiles*`, `/cache/pedigree/*` → `/pedigree/*`, `/cache/invalidate` → `/profiles/drop`. GraphQL field and type names are unchanged.

**Search (Sprint E.6):** person search moved to the normal search path — `GET /trees/{tree_id}/persons/search?q=query&limit=20&offset=0` (paginated `SearchResult`, backed by the `person_search_fts` DB table; empty or missing `q` = browse mode, sorted by name). The former `GET /cache/search` endpoint and the legacy `surname`/`given_names`/`sex` field filters were removed.

Used by: [Tree View](ui-genealogy-tree.md) (pedigree chart) · [Person Profile](ui-person-profile.md) (person detail) · [Search Results](ui-search-results.md) (search)

**Note:** All mutation endpoints (create/update/delete) include a synchronous projection refresh after the DB write. The response waits for it, guaranteeing consistency on subsequent reads. See [Read Projections](read-projections.md) §5.

### Reference Content

Read-only lookup of static reference content (occupation sheets, given-name meanings) shown as a hover tooltip on the person profile page. Not tied to a tree — `term` is the raw free-text GEDCOM value (occupation label or given name); matching is case/accent/punctuation-insensitive with alias support (e.g. gendered variants), and given-name lookups fall back to the first token of a compound name (e.g. "Marie-Claire" → "Marie"). Source content lives in `oxidgene-api/src/reference/data/*.json` (one file per language per data type), gzip-compressed at build time and decompressed once into an in-memory table (see `oxidgene-api::reference`).

| Method | Path | Description |
|---|---|---|
| `GET` | `/reference/{lang}/occupations?term=...` | Occupation fiche (label, summary, text) for `lang` (`fr`/`en`); 404 if none |
| `GET` | `/reference/{lang}/given-names?term=...` | Given-name fiche (label, origin, meaning, text, feast day) for `lang`; 404 if none |

Note the path prefix: these sit at `/api/v1/reference/...`, not under `/api/v1/trees/{tree_id}/...`. Used by: [Person Profile](ui-person-profile.md) (`ReferenceHover`/`ReferenceBubble` tooltip over the occupation and given name). Content set is seeded with 5 occupations + 5 given names (fr/en) as of Sprint E.7 — growing the data set is a separate content task, not a code change.

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

## 2. GraphQL API

Endpoint: `/graphql` (POST for queries/mutations, WebSocket for subscriptions).

### Queries

```graphql
type Query {
  # Trees
  trees(first: Int, after: String): TreeConnection!
  tree(id: ID!): Tree

  # Persons
  persons(treeId: ID!, first: Int, after: String, search: String): PersonConnection!
  person(treeId: ID!, id: ID!): Person
  ancestors(treeId: ID!, personId: ID!, maxDepth: Int): [PersonWithDepth!]!
  descendants(treeId: ID!, personId: ID!, maxDepth: Int): [PersonWithDepth!]!

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

  # Media
  mediaList(treeId: ID!, first: Int, after: String): MediaConnection!
  media(treeId: ID!, id: ID!): Media

  # GEDCOM (export is a read — it lives on Query, not Mutation)
  exportGedcom(treeId: ID!, mergeOccupations: Boolean, mergeNames: Boolean): ExportGedcomResult!

  # Read projections (see Read Projections spec) — mirrors the REST routes
  personProfile(treeId: ID!, personId: ID!): GqlPersonProfile!
  personProfiles(treeId: ID!): [GqlPersonProfile!]!
  pedigree(treeId: ID!, rootPersonId: ID!, ancestorDepth: Int!, descendantDepth: Int!): GqlPedigree!
  searchPersons(treeId: ID!, query: String!, limit: Int, offset: Int): GqlSearchResult!
}
```

### Mutations

```graphql
type Mutation {
  # Trees
  createTree(input: CreateTreeInput!): Tree!
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
  deleteSource(treeId: ID!, id: ID!): Boolean!

  # Citations
  createCitation(treeId: ID!, input: CreateCitationInput!): Citation!
  updateCitation(treeId: ID!, id: ID!, input: UpdateCitationInput!): Citation!
  deleteCitation(treeId: ID!, id: ID!): Boolean!

  # Media
  uploadMedia(treeId: ID!, input: UploadMediaInput!): Media!
  updateMedia(treeId: ID!, id: ID!, input: UpdateMediaInput!): Media!
  deleteMedia(treeId: ID!, id: ID!): Boolean!
  createMediaLink(treeId: ID!, input: CreateMediaLinkInput!): MediaLink!
  deleteMediaLink(treeId: ID!, id: ID!): Boolean!

  # Notes
  createNote(treeId: ID!, input: CreateNoteInput!): Note!
  updateNote(treeId: ID!, id: ID!, input: UpdateNoteInput!): Note!
  deleteNote(treeId: ID!, id: ID!): Boolean!

  # Import (content passed inline — no Upload scalar)
  importGedcom(treeId: ID!, input: ImportGedcomInput!): ImportResult!
  # `.gw` bytes are base64-encoded: the format is ISO-8859-1 unless the file
  # opts into UTF-8, and a GraphQL String cannot carry non-UTF-8 bytes.
  importGeneweb(treeId: ID!, input: ImportGenewebInput!): ImportResult!

  # Read projections (see Read Projections spec) — mirrors the REST routes
  expandPedigree(treeId: ID!, rootPersonId: ID!, direction: PedigreeDirection!, fromDepth: Int!, toDepth: Int!, otherDepth: Int = 0): GqlPedigreeDelta!
  rebuildTreeProfiles(treeId: ID!): GqlProfileRebuildResult!
  rebuildPersonProfile(treeId: ID!, personId: ID!): GqlProfileRebuildResult!
  dropTreeProfiles(treeId: ID!): Boolean!
}
```

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

input ImportGedcomInput {
  gedcom: String!
}

input ImportGenewebInput {
  contentBase64: String!
  filename: String   # default "import.gw"
}

# Returned by every import mutation, whatever the source format.
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

# Similar connection types for Person, Family, Event, Place, Source, Media

# --- Cache types (see Caching spec for full details) ---

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
  birthYear: String
  birthPlace: String
  deathYear: String
  deathPlace: String
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

## 3. GEDCOM Compatibility Reference

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
| Media (OBJE) | Metadata only | Metadata only | File path, MIME type, title. GEDZIP export bundles the referenced files |
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
