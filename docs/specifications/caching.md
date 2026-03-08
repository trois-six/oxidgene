# Server-Side Caching

> Part of the [OxidGene Specifications](README.md).
> See also: [Architecture](architecture.md) · [API Contract](api.md) · [Data Model](data-model.md) · [Roadmap](roadmap.md)

---

## 1. Overview & Motivation

### 1.1 Current Problems

The existing architecture has several performance bottlenecks that prevent instant page display, especially for large trees (up to 100,000 persons):

| Problem | Impact |
|---|---|
| **Monolithic `TreeSnapshot`** | A single endpoint returns *all* persons, names, events, places, spouses, and children for an entire tree. Does not scale past ~5,000 persons. |
| **N+1 on person detail** | The person profile page fetches the person, then names, then events, then families, then spouses *per family*, then children *per family* — 5–10 sequential requests. |
| **Client-side search** | The frontend loads the full snapshot and filters in the browser. Unusable for large trees. |
| **No shared server cache** | Every request hits the database. When multiple users work on the same tree (web deployment), they don't benefit from each other's fetches. |
| **30-second TTL HTTP cache** | The client-side `ResponseCache` is a blunt instrument — fixed TTL, no size limit, full invalidation per tree on any mutation. |

### 1.2 Goals

- **Instant page display** after initial load or GEDCOM import — every page transition should feel immediate.
- **Incremental updates** — editing one person should only recompute that person's cache (and a small set of related persons), not the entire tree.
- **Windowed tree display** — the pedigree chart should fetch only the visible subset of persons, not the full tree. Expanding/collapsing levels should fetch only the delta.
- **Server-side search** — search queries run on the server, returning paginated results, never sending the full person list to the client.
- **Shared cache** — on web deployments, all users editing the same tree benefit from the same cache (stored in Redis). On desktop, the cache is persisted to disk across app restarts.
- **Support up to 100,000 persons** per tree with no degradation.

---

## 2. The Three Caches

### 2.1 PersonCache — Per-Person Denormalized Profile

**Purpose:** Contains everything needed to display a person's card (in pedigree), profile page, or edit modal — in a single read. Eliminates all N+1 patterns.

**Key:** `(tree_id, person_id)`

```rust
struct CachedPerson {
    // Core identity
    person_id: Uuid,
    tree_id: Uuid,
    sex: Sex,

    // Names (denormalized from PersonName)
    primary_name: Option<CachedName>,       // The is_primary=true name
    other_names: Vec<CachedName>,           // All other names

    // Key life events (denormalized from Event + Place)
    birth: Option<CachedEvent>,
    death: Option<CachedEvent>,
    baptism: Option<CachedEvent>,
    burial: Option<CachedEvent>,
    occupation: Option<String>,             // Latest occupation event description
    other_events: Vec<CachedEvent>,

    // Family links (denormalized from FamilySpouse/FamilyChild)
    families_as_spouse: Vec<CachedFamilyLink>,  // Families where this person is a spouse
    family_as_child: Option<CachedChildLink>,   // Family where this person is a child

    // Attached media/sources/notes (counts + primary)
    primary_media: Option<CachedMediaRef>,  // Portrait / primary photo
    media_count: u32,
    citation_count: u32,
    note_count: u32,

    // Metadata
    updated_at: DateTime<Utc>,              // Person's last modification
    cached_at: DateTime<Utc>,               // When this cache entry was built
}

struct CachedName {
    name_id: Uuid,
    name_type: NameType,
    display_name: String,       // Pre-computed "Prefix Given Surname Suffix"
    given_names: Option<String>,
    surname: Option<String>,
}

struct CachedEvent {
    event_id: Uuid,
    event_type: EventType,
    date_value: Option<String>,     // Original GEDCOM date phrase
    date_sort: Option<NaiveDate>,   // Normalized for sorting
    place_name: Option<String>,     // Denormalized from Place.name
    place_id: Option<Uuid>,
    description: Option<String>,
}

struct CachedFamilyLink {
    family_id: Uuid,
    role: SpouseRole,
    spouse_id: Option<Uuid>,            // The other spouse (if any)
    spouse_display_name: Option<String>,
    spouse_sex: Option<Sex>,
    marriage: Option<CachedEvent>,      // Marriage event for this family
    children_ids: Vec<Uuid>,            // Children person IDs in this family
    children_count: u32,
}

struct CachedChildLink {
    family_id: Uuid,
    child_type: ChildType,
    father_id: Option<Uuid>,
    father_display_name: Option<String>,
    mother_id: Option<Uuid>,
    mother_display_name: Option<String>,
}

struct CachedMediaRef {
    media_id: Uuid,
    file_path: String,
    mime_type: String,
    title: Option<String>,
}
```

### 2.2 PedigreeCache — Windowed Tree Display

**Purpose:** Contains the subset of persons visible in the pedigree chart, organized for instant rendering. When the user expands/collapses levels, only the delta is fetched from the server.

**Key:** `(tree_id, root_person_id)`

```rust
struct CachedPedigree {
    tree_id: Uuid,
    root_person_id: Uuid,

    // Persons keyed by ID, with pedigree-specific display metadata
    persons: HashMap<Uuid, PedigreeNode>,

    // Relationship edges for the chart renderer
    edges: Vec<PedigreeEdge>,

    // What depths have already been loaded
    ancestor_depth_loaded: u32,     // e.g. 5 = ancestors up to 5 generations loaded
    descendant_depth_loaded: u32,   // e.g. 3 = descendants up to 3 generations loaded

    cached_at: DateTime<Utc>,
}

struct PedigreeNode {
    person_id: Uuid,
    sex: Sex,
    display_name: String,               // Pre-computed primary name
    birth_year: Option<String>,         // "1842" or "~1842" — for card display
    birth_place: Option<String>,        // Short place name
    death_year: Option<String>,
    death_place: Option<String>,
    occupation: Option<String>,
    primary_media_path: Option<String>, // Portrait thumbnail path
    generation: i32,                    // Relative to root (0 = root, -1 = parent, +1 = child)
    sosa_number: Option<u64>,           // Sosa-Stradonitz number if on ancestor path
}

struct PedigreeEdge {
    parent_id: Uuid,
    child_id: Uuid,
    family_id: Uuid,
    edge_type: ChildType,               // Biological, Adopted, etc.
}
```

**Incremental operations:**

| Operation | Server behavior |
|---|---|
| User increases ancestor levels (e.g. 5→7) | Server queries PersonAncestry for depths 6–7, builds PedigreeNodes for new persons from PersonCache, appends to existing PedigreeCache. Returns only the new nodes and edges as a `PedigreeDelta`. |
| User decreases ancestor levels (e.g. 7→4) | Client-side only: hide nodes with `generation < -4`. Server cache is unchanged — the extra nodes remain for instant re-expansion. |
| User increases descendant levels | Same as ancestors but in the descendant direction. |
| User decreases descendant levels | Client-side only: hide nodes with `generation > N`. |
| User changes root person | Lookup or build a new PedigreeCache for the new root. If many persons overlap with the previous view, PersonCache already has them (instant node construction). |
| A person is edited | The PedigreeNode for that person is rebuilt from the updated PersonCache entry. All pedigrees containing that person are patched. |

### 2.3 SearchIndex — Per-Tree Search

**Purpose:** A pre-built index for instant person search within a tree. Avoids sending the full person list to the browser.

**Key:** `tree_id`

```rust
struct CachedSearchIndex {
    tree_id: Uuid,
    entries: Vec<SearchEntry>,      // Sorted by surname, given_names for efficient matching
    cached_at: DateTime<Utc>,
}

struct SearchEntry {
    person_id: Uuid,
    sex: Sex,
    // Searchable text fields (lowercased, accent-folded for matching)
    surname_normalized: String,
    given_names_normalized: String,
    maiden_name_normalized: Option<String>,
    // Display fields (original casing, for rendering results)
    display_name: String,
    // Key dates for result display
    birth_year: Option<String>,
    birth_place: Option<String>,
    death_year: Option<String>,
    // For sorting/filtering
    date_sort: Option<NaiveDate>,
}
```

The server performs the search (prefix match, trigram, or simple substring on normalized fields) and returns paginated results. The frontend never needs the full person list.

---

## 3. Storage Backends

### 3.1 `CacheStore` Trait

A trait abstracts over the two storage backends, allowing the same `CacheService` to work on both web and desktop:

```rust
#[async_trait]
trait CacheStore: Send + Sync {
    // --- PersonCache ---
    async fn get_person(&self, tree_id: Uuid, person_id: Uuid) -> Option<CachedPerson>;
    async fn set_person(&self, entry: &CachedPerson);
    async fn set_persons_batch(&self, entries: &[CachedPerson]);
    async fn delete_person(&self, tree_id: Uuid, person_id: Uuid);
    async fn get_persons_batch(&self, tree_id: Uuid, person_ids: &[Uuid]) -> Vec<CachedPerson>;

    // --- PedigreeCache ---
    async fn get_pedigree(&self, tree_id: Uuid, root_id: Uuid) -> Option<CachedPedigree>;
    async fn set_pedigree(&self, entry: &CachedPedigree);
    async fn delete_pedigree(&self, tree_id: Uuid, root_id: Uuid);
    async fn delete_all_pedigrees(&self, tree_id: Uuid);

    // --- SearchIndex ---
    async fn get_search_index(&self, tree_id: Uuid) -> Option<CachedSearchIndex>;
    async fn set_search_index(&self, entry: &CachedSearchIndex);
    async fn delete_search_index(&self, tree_id: Uuid);

    // --- Bulk ---
    async fn invalidate_tree(&self, tree_id: Uuid);
}
```

### 3.2 Redis Backend (Web Deployment)

Used when the application runs as a web server behind Redis.

| Aspect | Detail |
|---|---|
| **Key patterns** | `pc:{tree_id}:{person_id}` (PersonCache), `ped:{tree_id}:{root_id}` (PedigreeCache), `si:{tree_id}` (SearchIndex) |
| **Serialization** | MessagePack (compact, fast, schema-less) |
| **Batch reads** | `MGET` for `get_persons_batch` |
| **Pedigree nodes** | Stored as a Redis Hash (`HSET`/`HGET`) so individual nodes can be updated without rewriting the full pedigree |
| **TTL** | None by default (explicit invalidation on mutations). Optional 24-hour safety TTL as a fallback. |
| **Bulk invalidation** | `SCAN` + `DEL` with prefix patterns (e.g. `pc:{tree_id}:*`, `ped:{tree_id}:*`, `si:{tree_id}`) |

Redis is added as a container in the Docker Compose stack for web deployment. See [Architecture](architecture.md) §8.1.

### 3.3 In-Memory + Disk Backend (Desktop)

Used when the application runs as a desktop app (single user, embedded SQLite).

| Aspect | Detail |
|---|---|
| **Runtime** | `DashMap` (lock-free concurrent HashMap) for instant reads |
| **Persistence directory** | Platform cache directory via `dirs::cache_dir()` — resolves to `~/.cache/oxidgene/` (Linux), `~/Library/Caches/oxidgene/` (macOS), `C:\Users\<user>\AppData\Local\oxidgene\` (Windows) |
| **File layout** | `{tree_id}.persons.bin` (all CachedPerson), `{tree_id}.search.bin` (SearchIndex), `{tree_id}.pedigree.{root_id}.bin` (per-root PedigreeCache) |
| **Serialization** | `bincode` (fast, compact binary encoding) |
| **Lifecycle** | Load `.bin` files into DashMap on app startup. Reads/writes at runtime are in-memory. On graceful app exit, serialize DashMap contents back to `.bin` files. |
| **Crash recovery** | Stale or missing cache is detected via a `cache_version` counter stored alongside the cache. If it doesn't match the DB state, the cache is rebuilt lazily on first access. |

---

## 4. Cache Invalidation

### 4.1 Principle

**Mutations specify exactly which persons are affected, and only those cache entries are recomputed.** The cache update happens **synchronously** before the mutation response is returned, guaranteeing that the next read sees fresh data. The overhead is ~5–15ms per mutation — negligible for a write operation.

### 4.2 Mutation → Invalidation Map

| Mutation | PersonCache | PedigreeCache | SearchIndex |
|---|---|---|---|
| **Edit person** (sex change) | Rebuild `person_id` | Update node in all pedigrees containing it | Update entry |
| **Edit person name** | Rebuild `person_id` + all persons referencing its display name (spouses, children, parents) | Update node display_name | Update entry |
| **Add/edit/delete event** | Rebuild `person_id` (or both spouses if family event) | Update node if birth/death/occupation changed | Update entry if birth/death changed |
| **Add/delete family spouse** | Rebuild both spouses + all children in the family | Rebuild affected edges | No impact |
| **Add/delete family child** | Rebuild child + both parents | Rebuild affected edges; add PedigreeNode if within loaded depth | No impact |
| **Delete person** | Remove entry | Remove node from all pedigrees containing it | Remove from index |
| **Create person** | Build new entry | No impact (not linked to any family yet) | Add to index |
| **GEDCOM import** | Build all entries (eager, batched) | Build pedigree for `sosa_root_person_id` if set | Build full index |
| **Delete tree** | `invalidate_tree(tree_id)` | Drop all pedigrees | Drop index |

### 4.3 Affected Set Algorithm

When a person is modified, the server identifies the bounded "affected set" of persons whose cache must be rebuilt:

```rust
fn affected_persons(db, tree_id, person_id) -> Vec<Uuid> {
    let mut affected = vec![person_id];

    // Spouses and children in all families where this person is a spouse
    for family in families_as_spouse(person_id) {
        for spouse in family.spouses where spouse.person_id != person_id {
            affected.push(spouse.person_id);
            // Their CachedFamilyLink references our display name
        }
        for child in family.children {
            affected.push(child.person_id);
            // Their CachedChildLink references our display name as parent
        }
    }

    // Parents in the family where this person is a child
    if let Some(family) = family_as_child(person_id) {
        for spouse in family.spouses {
            affected.push(spouse.person_id);
            // Their CachedFamilyLink.children_ids includes us
        }
    }

    affected.dedup();
    affected
}
```

This set is **bounded** — typically 2–10 persons. Rebuilding each `CachedPerson` from DB data takes <2ms.

### 4.4 Latency Budget

```
Mutation latency breakdown (typical):
  DB write:              ~2-10ms
  Compute affected set:  ~1ms  (query family memberships)
  Rebuild PersonCache:   ~2-5ms (2-10 entries from DB)
  Patch PedigreeCaches:  ~1ms  (update nodes in existing caches)
  Update SearchIndex:    ~1ms  (update 1-10 entries)
  ─────────────────────────────
  Total overhead:        ~5-15ms (imperceptible to user)
```

---

## 5. API Endpoints

### 5.1 REST Endpoints

Base path: `/api/v1`

| Method | Path | Description |
|---|---|---|
| `GET` | `/trees/{tree_id}/cache/persons/{person_id}` | Get a single cached person (full denormalized profile) |
| `GET` | `/trees/{tree_id}/cache/persons?ids=uuid1,uuid2,...` | Batch get cached persons |
| `GET` | `/trees/{tree_id}/cache/pedigree/{root_person_id}?ancestor_depth=N&descendant_depth=N` | Get windowed pedigree for a root person |
| `PATCH` | `/trees/{tree_id}/cache/pedigree/{root_person_id}/expand?direction=ancestors|descendants&from_depth=N&to_depth=N` | Expand pedigree depth (returns only the new nodes and edges as a `PedigreeDelta`) |
| `GET` | `/trees/{tree_id}/cache/search?q=query&limit=20&offset=0` | Server-side person search (paginated) |
| `POST` | `/trees/{tree_id}/cache/rebuild` | Force full cache rebuild for a tree (admin/debug) |

Used by: [Tree View](ui-genealogy-tree.md) (pedigree chart) · [Person Profile](ui-person-profile.md) (person detail) · [Search Results](ui-search-results.md) (search)

**Note:** All existing mutation endpoints (create/update/delete person, name, event, family, family_member, etc.) are unchanged but now include a synchronous cache update step after the DB write. See §4.

### 5.2 GraphQL Queries & Mutations

```graphql
type Query {
  # Cache-backed queries
  cachedPerson(treeId: ID!, personId: ID!): CachedPerson!
  cachedPersons(treeId: ID!, personIds: [ID!]!): [CachedPerson!]!
  pedigree(treeId: ID!, rootPersonId: ID!, ancestorDepth: Int!, descendantDepth: Int!): CachedPedigree!
  searchPersons(treeId: ID!, query: String!, limit: Int, offset: Int): SearchResult!
}

type Mutation {
  # Cache management
  expandPedigree(treeId: ID!, rootPersonId: ID!, direction: PedigreeDirection!, fromDepth: Int!, toDepth: Int!): PedigreeDelta!
  rebuildTreeCache(treeId: ID!): Boolean!
}
```

### 5.3 GraphQL Cache Types

```graphql
type CachedPerson {
  personId: ID!
  treeId: ID!
  sex: Sex!
  primaryName: CachedName
  otherNames: [CachedName!]!
  birth: CachedEvent
  death: CachedEvent
  baptism: CachedEvent
  burial: CachedEvent
  occupation: String
  otherEvents: [CachedEvent!]!
  familiesAsSpouse: [CachedFamilyLink!]!
  familyAsChild: CachedChildLink
  primaryMedia: CachedMediaRef
  mediaCount: Int!
  citationCount: Int!
  noteCount: Int!
  updatedAt: DateTime!
  cachedAt: DateTime!
}

type CachedName {
  nameId: ID!
  nameType: NameType!
  displayName: String!
  givenNames: String
  surname: String
}

type CachedEvent {
  eventId: ID!
  eventType: EventType!
  dateValue: String
  dateSort: Date
  placeName: String
  placeId: ID
  description: String
}

type CachedFamilyLink {
  familyId: ID!
  role: SpouseRole!
  spouseId: ID
  spouseDisplayName: String
  spouseSex: Sex
  marriage: CachedEvent
  childrenIds: [ID!]!
  childrenCount: Int!
}

type CachedChildLink {
  familyId: ID!
  childType: ChildType!
  fatherId: ID
  fatherDisplayName: String
  motherId: ID
  motherDisplayName: String
}

type CachedMediaRef {
  mediaId: ID!
  filePath: String!
  mimeType: String!
  title: String
}

type CachedPedigree {
  treeId: ID!
  rootPersonId: ID!
  persons: [PedigreeNode!]!
  edges: [PedigreeEdge!]!
  ancestorDepthLoaded: Int!
  descendantDepthLoaded: Int!
  cachedAt: DateTime!
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

## 6. Frontend Changes

### 6.1 What Gets Removed

| Component | Reason |
|---|---|
| `ResponseCache` (30s TTL HTTP cache in `api.rs`) | Replaced by server-side cache — no more client-side response caching |
| `TreeSnapshot` endpoint + `get_tree_snapshot()` | Replaced by windowed `PedigreeCache` + per-person `PersonCache` |
| Client-side search filtering in `SearchResults`/`SearchPerson` | Replaced by server-side `GET /cache/search` endpoint |

### 6.2 What Gets Simplified

The `TreeCache` in `tree_cache.rs` becomes a thin **client-side reference holder** rather than a data cache:

```rust
struct TreeCache {
    tree: Signal<Option<Tree>>,
    // Instead of storing a full TreeSnapshot, store the pedigree response
    pedigree: Signal<Option<CachedPedigree>>,
    generation: Signal<u64>,        // Invalidation counter
}
```

Individual person lookups go through the API (which reads from server-side cache — effectively instant).

### 6.3 Page-by-Page Data Flow

| Page | Current approach | New approach |
|---|---|---|
| **Home** (tree list) | `list_trees` | Unchanged (lightweight, no cache needed) |
| **Pedigree chart** | Fetch full `TreeSnapshot`, build tree client-side | `GET /cache/pedigree/{root_id}?ancestor_depth=5&descendant_depth=3` — server returns pre-built windowed tree. On depth change: `PATCH .../expand` returns only delta nodes/edges. |
| **Person detail** | `get_person` + N+1 for names, events, families, spouses per family, children per family | `GET /cache/persons/{person_id}` — single request, everything pre-joined |
| **Person edit modal** | Same N+1 as person detail | Same `GET /cache/persons/{person_id}` for read; mutations hit existing REST endpoints (which auto-update cache) |
| **Union edit modal** | N+1 per person in family | `GET /cache/persons?ids=spouse1,spouse2,child1,...` — batch read |
| **Search** | Load full `TreeSnapshot`, filter/score/sort in browser | `GET /cache/search?q=...&limit=20` — server-side, paginated, instant |

---

## 7. New Crate: `oxidgene-cache`

### 7.1 Module Layout

```
crates/oxidgene-cache/
  Cargo.toml
  src/
    lib.rs              // Re-exports
    types.rs            // CachedPerson, CachedPedigree, SearchEntry, etc.
    store.rs            // CacheStore trait definition
    store/
      redis.rs          // Redis implementation (feature-gated)
      memory.rs         // DashMap + disk persistence implementation
    builder.rs          // Logic to build cache entries from DB data
    service.rs          // CacheService: orchestrates builds, invalidation, queries
    invalidation.rs     // affected_persons(), cascading rebuild logic
```

### 7.2 Position in Dependency Graph

```
oxidgene-core (no internal deps)
    ↑
oxidgene-db (depends on: oxidgene-core)
    ↑
oxidgene-cache (depends on: oxidgene-core, oxidgene-db)
    ↑
oxidgene-api (depends on: oxidgene-core, oxidgene-db, oxidgene-cache, oxidgene-gedcom)
    ↑
oxidgene-server (depends on: oxidgene-api, oxidgene-db)
oxidgene-desktop (depends on: oxidgene-api, oxidgene-db, oxidgene-ui)
```

### 7.3 AppState Changes

```rust
// Modified AppState (in oxidgene-api/src/rest/state.rs)
struct AppState {
    pub db: DatabaseConnection,
    pub cache: Arc<dyn CacheStore>,         // The storage backend
    pub cache_service: Arc<CacheService>,   // The orchestration layer
}
```

---

## 8. GEDCOM Import Integration

After a GEDCOM import (which can create thousands of persons at once), the cache is built **eagerly** in a background task:

```rust
async fn import_gedcom_handler(...) -> Result<Json<ImportResult>> {
    let result = gedcom_service.import(&db, tree_id, gedcom_data).await?;

    // Respond immediately with import stats
    let response = Json(result);

    // Spawn background cache build (non-blocking to the HTTP response)
    tokio::spawn(async move {
        cache_service.rebuild_tree_full(tree_id).await;
    });

    response
}
```

`rebuild_tree_full` does:
1. Fetch all persons + names + events + places + family_members in parallel (via `tokio::try_join!`).
2. Build all `CachedPerson` entries in batch (using `set_persons_batch`).
3. Build the `CachedSearchIndex` for the tree.
4. Build the `CachedPedigree` for the `sosa_root_person_id` (if set on the tree).

For 100K persons, this takes approximately 2–5 seconds. The frontend can display a "Building cache..." progress indicator and poll for completion. Subsequent page interactions are instant.

---

## 9. Desktop Persistence

### 9.1 File Layout

```
<cache_dir>/oxidgene/          # dirs::cache_dir() + "oxidgene/"
                               # Linux:   ~/.cache/oxidgene/
                               # macOS:   ~/Library/Caches/oxidgene/
                               # Windows: C:\Users\<user>\AppData\Local\oxidgene\
  {tree_id_1}.persons.bin         # All CachedPerson for tree 1
  {tree_id_1}.search.bin          # SearchIndex for tree 1
  {tree_id_1}.pedigree.{root_id}.bin   # PedigreeCache per root
  {tree_id_2}.persons.bin
  ...
```

### 9.2 Lifecycle

| Phase | Behavior |
|---|---|
| **App startup** | Load all `.bin` files from the cache directory into the in-memory `DashMap`. |
| **Runtime** | All reads and writes operate on the `DashMap` (sub-microsecond). |
| **Graceful shutdown** | Serialize all `DashMap` contents to `.bin` files using `bincode`. |
| **Crash recovery** | On next startup, if cache files are stale or missing, the cache is rebuilt lazily on first access. Staleness is detected via a `cache_version` counter stored in the cache files and compared to a version counter in the database. |

---

## 10. Pedigree Memory Budget

Each `PedigreeNode` is approximately 300 bytes. A typical pedigree with 7 ancestor levels + 3 descendant levels contains at most ~250 persons ≈ 75 KB.

Pedigree caches are managed with a **memory-budgeted LRU** per tree:

| Deployment | Budget per tree | Approx. pedigrees retained |
|---|---|---|
| Desktop | 64 MB (default, configurable) | ~850 distinct roots |
| Web (Redis) | 128 MB (default, configurable) | ~1700 distinct roots |

When the total memory for a tree's pedigree caches exceeds the budget, the least-recently-used pedigree is evicted. For typical usage (a single user navigating a tree), eviction never occurs.

The budget is configurable via:
- Environment variable: `OXIDGENE_PEDIGREE_CACHE_MB`
- Config file: `pedigree_cache_mb` field

---

## 11. Performance Targets

| Operation | Before (current) | After (with cache) |
|---|---|---|
| **Pedigree chart (initial load)** | 1 large snapshot request (scales with tree size — unusable at 100K) | 1 windowed request (~250 persons, constant regardless of tree size) |
| **Pedigree depth change (+2 levels)** | Re-fetch full snapshot or reprocess existing data | 1 `PATCH /expand` returning only new nodes (~50-100 nodes) |
| **Pedigree depth decrease** | Same as above | Client-side only (hide nodes) — zero network requests |
| **Person detail page** | 5–10 sequential requests (N+1) | 1 request (`GET /cache/persons/{id}`) |
| **Search (10K persons)** | Full snapshot + client-side filter (~5-10s on large trees) | 1 server-side request (<50ms) |
| **After editing a person** | Invalidate all cached responses, re-fetch snapshot | Server rebuilds 2–10 entries (~5-15ms), next read instant |
| **After GEDCOM import (100K persons)** | Build snapshot on next page load (may time out) | Background build ~2–5s, then all pages instant |

---

## 12. Implementation Sprints

### Sprint E.1 — Cache Foundation

- Create `oxidgene-cache` crate with `CacheStore` trait.
- Implement cache type structs (`CachedPerson`, `CachedPedigree`, `CachedSearchIndex`, sub-types).
- Implement `MemoryCacheStore` (DashMap-based, no persistence yet).
- Implement `CacheBuilder` — logic to construct a `CachedPerson` from DB data.
- Implement `CacheService` with `rebuild_person`, `rebuild_tree_full`.
- Unit tests for cache builder and service.

### Sprint E.2 — Person Cache & API Integration

- Add `CacheService` and `CacheStore` to `AppState`.
- Implement `GET /cache/persons/{id}` and `GET /cache/persons?ids=...` REST endpoints.
- Implement `cachedPerson` and `cachedPersons` GraphQL queries.
- Hook all mutation handlers to trigger synchronous cache invalidation.
- Update `person_detail.rs` to use the cached endpoint.
- Update `person_form.rs` and `union_form.rs` to use cached endpoint.

### Sprint E.3 — Pedigree Cache

- Implement pedigree cache builder from PersonAncestry + PersonCache.
- Implement `GET /cache/pedigree/{root_id}` and `PATCH .../expand` REST endpoints.
- Implement `pedigree` query and `expandPedigree` mutation in GraphQL.
- Implement LRU memory budget for pedigree caches (configurable per deployment).
- Update `pedigree_chart.rs` to consume pedigree cache instead of snapshot.
- Update `tree_detail.rs` page orchestration.

### Sprint E.4 — Search Index & GEDCOM Integration

- Implement `CachedSearchIndex` builder with accent-folding and normalization.
- Implement `GET /cache/search?q=...` REST endpoint and `searchPersons` GraphQL query.
- Hook GEDCOM import to trigger eager background cache build.
- Update search components to use server-side search.
- Remove `TreeSnapshot` endpoint and client-side `ResponseCache`.
- Implement `POST /cache/rebuild` REST endpoint and `rebuildTreeCache` GraphQL mutation.

### Sprint E.5 — Redis Backend & Desktop Persistence

- Implement `RedisCacheStore` (MessagePack serialization, `MGET` batch reads).
- Add Redis container to Docker Compose for web deployment.
- Implement disk persistence for `MemoryCacheStore` (bincode, serialize on exit, load on startup).
- Auto-detect Redis (web) vs. memory (desktop) via configuration.
- Performance testing with 100K-person trees.
- Cache staleness detection and recovery for desktop.
