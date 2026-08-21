---
type: "Read Projections Specification"
title: "Denormalized Read Projections"
description: "How OxidGene materializes denormalized read models in the database instead of caching them, including projection shapes, refresh, and pedigree assembly."
tags: [oxidgene, specification, projections, denormalization, performance]
timestamp: 2026-07-28T00:00:00Z
---


# Denormalized Read Projections

> Part of the [OxidGene Specifications](index.md).
> See also: [Architecture](architecture.md) · [API Contract](api.md) · [Data Model](data-model.md) · [Roadmap](roadmap.md)

---

## 1. Overview & Motivation

### 1.1 The problem this solves

Reading a person the naive way means five to ten sequential queries — the person, their names, their events, their families, then each family's spouses and children — and rendering a pedigree means doing that for every node. That is the N+1 problem, and it does not scale to the 100,000-person trees OxidGene targets.

The answer is to **pre-compute the read model**: assemble everything a person card, detail page or pedigree node needs into a single denormalized record, and read it back in one shot.

### 1.2 Why this is not a cache

Sprints E.1–E.6 built that pre-computation as a *cache* — an `oxidgene-cache` crate with a `CacheStore` trait, a Redis backend for web, a DashMap + disk backend for desktop, and pedigree LRU budgets. **Sprint E.9 deleted all of it**, because a cache was the wrong tool for this data:

| Question | Answer for these read models |
|---|---|
| Is it derived from a *bounded* set of rows? | Yes — a person plus their immediate relatives. |
| Can we compute exactly what changes on a write? | Yes — see the affected-set algorithm (§5.3), which predates the cache and survived it. |
| Is it expensive to recompute? | No — ~2 ms per person. |
| Does staleness ever produce a *correct* answer? | No. A stale name is simply a bug. |

Data that is a function of one entity plus its neighbours, cheap to rebuild, and never acceptably stale is **denormalization**, not caching. So the projection now lives in a table (`person_denorm`), written in the same request that mutates the underlying rows.

Pedigrees looked like the counter-example — a pedigree is keyed by `(root, ancestor_depth, descendant_depth)`, a *query window* rather than a fact about an entity, and materializing every window would be combinatorial. But the graph traversal is cheap on its own: a recursive CTE over the family links answers "who are the ancestors of X within N generations" in milliseconds. All the pedigree cache added on top was the *display payload* per node — which `person_denorm` now holds. So a pedigree is assembled on demand from a traversal joined against a projection batch read, and nothing about it is stored.

### 1.3 What this buys

- **No stale reads.** The projection is refreshed in the same request as the write.
- **No cold start.** Projections are durable; a restart reads them straight back.
- **One code path.** Desktop (SQLite) and web (PostgreSQL) behave identically; there is no backend to select, no Redis to deploy, no disk snapshot to flush at exit.
- **~4,100 fewer lines** and one fewer crate (`oxidgene-cache`, with its three storage backends).

### 1.4 Goals retained from the cache design

- **Instant page display** — every page transition should feel immediate.
- **Incremental updates** — editing one person recomputes that person and a small set of relatives, never the whole tree.
- **Windowed tree display** — the pedigree chart fetches only the visible subset.
- **Server-side search** — queries run in the database, returning paginated results.
- **Support up to 100,000 persons** per tree with no degradation.

---

## 2. The Person Projection — `person_denorm`

**Purpose:** everything needed to display a person's card (in the pedigree), profile page, or edit modal — in a single row read. Eliminates all N+1 patterns.

**Key:** `person_id` (primary key), with `tree_id` indexed for tree-wide reads.

### 2.1 Storage

| Column | Type | Notes |
|---|---|---|
| `person_id` | UUID | PK, FK → `person.id` `ON DELETE CASCADE` |
| `tree_id` | UUID | FK → `tree.id` `ON DELETE CASCADE`, indexed |
| `payload` | TEXT | The JSON-serialized `PersonProfile` (below) |
| `schema_version` | int | Which build's payload shape this row holds; see §2.1.1 |
| `updated_at` | timestamptz | When the projection was last rebuilt |

The payload is **JSON rather than typed columns** on purpose: it embeds nested collections (other names, events, family links) that would otherwise need their own denormalized tables, and it lets the projection shape evolve without a migration per displayed field. Nothing queries *inside* the payload — lookups are by `person_id` or `tree_id`, and text search goes through `person_search_fts` (§4).

Owned by `PersonDenormRepo` in `oxidgene-db`: `get`, `get_many`, `list_tree`, `upsert` (bounded per-mutation refresh), `replace_tree` (full rebuild), `delete_person`, `delete_tree`, `count_current`, `count_tree`.

#### 2.1.1 Payload versioning

Letting the shape evolve without a migration has a cost, and it has to be paid explicitly. Every field added to `PersonProfile` carries `#[serde(default)]` so the rows already stored keep deserializing — which means an old payload comes back **looking complete**. Nothing can tell "this person genuinely has no date qualifier" from "this row predates qualifiers", and a projection change is therefore invisible on every existing install until somebody happens to re-import.

That is not hypothetical: it is exactly what the date-qualifier work shipped, and the only cure was knowing to re-import.

So `oxidgene_core::projection::PROJECTION_SCHEMA_VERSION` stamps every write, and reads compare it:

- `get`, `get_many` and `count_current` **filter on it**, so a row from an older build reads as *absent*. The callers that already rebuild a missing projection rebuild a stale one too — no second code path, and no way to forget one.
- `ensure_materialized` asks `count_current`, not `count_tree`: a tree whose rows an older build wrote is as unusable as one nobody has built, so it is rebuilt on first read (§5.5).
- `list_tree` deliberately does **not** filter. It answers "who is in this tree", and silently dropping stale rows would return a short list; its one caller checks `count_current` first, so nothing stale survives to reach it.
- `upsert`'s `ON CONFLICT` updates `schema_version` along with the payload. Leaving the old version behind would rebuild the row forever, once per read.

The column, not a field inside the payload: the version is metadata *about* the row and has to be queryable in one indexable comparison, identically on SQLite and PostgreSQL. Inside the JSON it would need each backend's own JSON functions, and counting stale rows would mean decoding every payload in the tree on a question asked by every read path.

**Raise the constant whenever a change alters what a payload means** — adding a field is the usual case and precisely the one that needs it. A bump costs one lazy rebuild per tree on first read; not bumping costs a silent wrong answer. When in doubt, bump.

### 2.2 Shape

Defined in `oxidgene_core::projection` — in `oxidgene-core` rather than the backend, so the Dioxus frontend can deserialize it without pulling in the database layer.

```rust
struct PersonProfile {
    // Core identity
    person_id: Uuid,
    tree_id: Uuid,
    sex: Sex,

    // Names (denormalized from PersonName)
    primary_name: Option<ProfileName>,       // The is_primary=true name
    other_names: Vec<ProfileName>,           // All other names

    // Key life events (denormalized from Event + Place)
    birth: Option<ProfileEvent>,
    death: Option<ProfileEvent>,
    baptism: Option<ProfileEvent>,
    burial: Option<ProfileEvent>,
    occupation: Option<String>,             // Latest occupation event description
    other_events: Vec<ProfileEvent>,

    // Family links (denormalized from FamilySpouse/FamilyChild)
    families_as_spouse: Vec<ProfileFamilyLink>,  // Families where this person is a spouse
    family_as_child: Option<ProfileChildLink>,   // Family where this person is a child

    // Attached media/sources/notes (counts + primary)
    // The portrait the person chose — `person.portrait_media_id`, or the crop
    // in `portrait_vignette_id`. `ProfileMediaRef` carries `vignette_id` so a
    // card asks for the cropped image rather than the whole group photograph.
    // Before EPIC F this took whichever media had the lowest `sort_order` and
    // ignored the stored choice, so a starred photograph and the one a
    // pedigree card drew could disagree.
    primary_media: Option<ProfileMediaRef>,
    media_count: u32,
    citation_count: u32,
    note_count: u32,

    // Metadata
    updated_at: DateTime<Utc>,              // Person's last modification
    built_at: DateTime<Utc>,               // When this projection was built
}

struct ProfileName {
    name_id: Uuid,
    name_type: NameType,
    display_name: String,       // Pre-computed "Prefix Given Surname Suffix"
    given_names: Option<String>,
    surname: Option<String>,
}

struct ProfileEvent {
    event_id: Uuid,
    event_type: EventType,
    date_value: Option<String>,     // Original GEDCOM date phrase
    date_sort: Option<NaiveDate>,   // Normalized for sorting
    place_name: Option<String>,     // Denormalized from Place.name
    place_id: Option<Uuid>,
    description: Option<String>,
}

struct ProfileFamilyLink {
    family_id: Uuid,
    role: SpouseRole,
    spouse_id: Option<Uuid>,            // The other spouse (if any)
    spouse_display_name: Option<String>,
    spouse_sex: Option<Sex>,
    marriage: Option<ProfileEvent>,      // Marriage event for this family
    events: Vec<ProfileEvent>,           // All family events
    children_ids: Vec<Uuid>,            // Children person IDs, birth-order sorted
    children_count: u32,
}

struct ProfileChildLink {
    family_id: Uuid,
    child_type: ChildType,
    father_id: Option<Uuid>,
    father_display_name: Option<String>,
    mother_id: Option<Uuid>,
    mother_display_name: Option<String>,
}

struct ProfileMediaRef {
    media_id: Uuid,
    file_path: String,
    mime_type: String,
    title: Option<String>,
}
```

The **cross-references** are what make this non-trivial: `spouse_display_name`, `father_display_name` and `mother_display_name` embed *other people's* names. Renaming one person therefore invalidates their relatives' projections too — see §5.

---

## 3. Pedigree Assembly — computed, never stored

**Purpose:** the subset of persons visible in the pedigree chart, organized for instant rendering.

Assembled fresh on every request, in two indexed reads plus in-memory assembly:

1. **`AncestryRepo`** (recursive CTE over `family_child` ⋈ `family_spouse`) → the ancestor and descendant IDs within the requested depths, with their generation numbers.
2. **`person_denorm`** → the display payload for those IDs, in one batched read. Any person without a projection yet is built on the spot and persisted.
3. Assemble nodes, parent→child edges, family units (including childless couples, which produce no edge) and family events from those payloads.

```rust
struct Pedigree {
    tree_id: Uuid,
    root_person_id: Uuid,
    persons: HashMap<Uuid, PedigreeNode>,
    edges: Vec<PedigreeEdge>,
    family_events: HashMap<Uuid, Vec<ProfileEvent>>,
    families: HashMap<Uuid, PedigreeFamily>,   // Spouse + child membership per family
    ancestor_depth_loaded: u32,
    descendant_depth_loaded: u32,
    built_at: DateTime<Utc>,
}

struct PedigreeNode {
    person_id: Uuid,
    sex: Sex,
    display_name: String,
    given_names: Option<String>,
    surname: Option<String>,
    birth_year: Option<String>,         // "1842" — for card display
    birth_place: Option<String>,
    death_year: Option<String>,
    death_place: Option<String>,
    occupation: Option<String>,
    primary_media_path: Option<String>, // Portrait thumbnail path
    generation: i32,                    // Relative to root (0 = root, -1 = parent, +1 = child)
    sosa_number: Option<u64>,           // Set for the root; the UI derives the rest from layout
}

struct PedigreeEdge {
    parent_id: Uuid,
    child_id: Uuid,
    family_id: Uuid,
    edge_type: ChildType,
}
```

Beyond the traversal window, the assembler also pulls in **spouses** of window members (so couples render), **one parent** of any family whose parents all fall outside the window (to recover the full sibling list in birth order), and **minimal info for family members outside the window** (so the event panel and the "+" hidden-relations indicator are accurate). None of these recurse.

### 3.1 Incremental operations

| Operation | Behavior |
|---|---|
| User increases ancestor levels (e.g. 5→7) | `PATCH .../expand` assembles the window at both depths and returns the difference as a `PedigreeDelta`, so the client merges instead of re-rendering. Equally valid: just re-`GET` at the new depth. |
| User decreases levels | Client-side only: hide nodes outside the range. Zero network requests. |
| User changes root person | A new `GET` — the projections for overlapping persons are already materialized, so it is a traversal plus a batch read. |
| A person is edited | Nothing to patch. The next pedigree request reads the refreshed projection. |

Because the server holds no per-client pedigree state, `expand` must be told the depth already loaded in the *opposite* direction (`other_depth`, default `0`) for the returned `*_depth_loaded` values to match what the caller holds.

> The expand endpoint currently has no caller in this repository — `pedigree_chart.rs` re-fetches instead. It is kept because it is part of the published GraphQL and REST contract.

---

## 4. Search — `person_search_fts`

**Purpose:** instant person search within a tree, without sending the full person list to the browser.

Since Sprint E.6 the search index lives in the database and survives restarts with the data:

| Backend | Implementation | Matching |
|---|---|---|
| **SQLite (desktop)** | FTS5 virtual table — indexed columns: `surname`, `given_names`, `maiden_name`, `birth_year`, `death_year`; display fields stored `UNINDEXED` | `MATCH` with per-word **prefix** queries (`"jean"* "dup"*`), all words must match |
| **PostgreSQL (web)** | Plain table with the same columns + `tree_id` index | Per-word `LIKE '%word%'` (substring), all words must match |

All searchable columns are pre-normalized in Rust (`oxidgene_core::search::normalize_for_search`: lowercase + accent folding) before insert, and queries are normalized the same way — so both backends match identically regardless of collation or missing DB extensions. A search like `"dupönt 1850"` matches a person with surname `DUPONT` born in 1850.

The API returns the `SearchEntry` wire shape (defined in `oxidgene_core::projection`):

```rust
struct SearchEntry {
    person_id: Uuid,
    sex: Sex,
    // Searchable text fields (lowercased, accent-folded for matching)
    surname_normalized: String,
    given_names_normalized: String,
    maiden_name_normalized: Option<String>,
    // Display fields (original casing, for rendering results)
    surname: String,
    given_names: String,
    display_name: String,
    // Key dates for result display
    birth_year: Option<String>,
    birth_place: Option<String>,
    death_year: Option<String>,
    // For sorting/filtering
    date_sort: Option<NaiveDate>,
}
```

`PersonSearchRepo` (in `oxidgene-db`) owns all reads and writes: `replace_tree`, `upsert`, `delete_person`, `delete_tree`, `search` (paginated, returns entries + total count).

An empty query is **browse mode**: all persons of the tree, sorted by surname then given names, paginated.

**Search semantics note:** FTS5 matches on **token prefixes** (`"mart"` matches `MARTIN`, but `"artin"` does not); the PostgreSQL fallback keeps substring semantics. Prefix matching is the standard genealogy search UX and enables index-backed lookups.

---

## 5. Refresh

### 5.1 Principle

**Mutations state exactly which persons are affected, and only those projections are rewritten** — in the **same transaction as the mutation itself**. The refresh reads the *post-mutation* state to build the projections, so it has to see the write; committing them together is what makes a projection impossible to observe out of step with its data.

```rust
let txn = begin_tx(&state.db).await?;
let person = PersonRepo::update(&txn, person_id, ...).await?;      // the write
let affected = invalidation::affected_persons(&txn, person_id).await?;
state.profiles.invalidate_for_mutation(&txn, tree_id, &affected).await?;  // the refresh
commit_tx(txn).await?;                                              // both, or neither
```

A dropped `DatabaseTransaction` rolls back, so any `?` in a handler undoes the mutation *and* the refresh as a unit. There is no window in which a committed write has an unrefreshed projection.

This works because every repo takes `&impl ConnectionTrait` rather than `&DatabaseConnection`, and SeaORM implements that trait for both `DatabaseConnection` and `DatabaseTransaction`. `ProfileService` follows the same split: read methods use the service's own connection, write methods take the caller's transaction.

**Cost.** Inside a transaction, SeaORM funnels every query through one `Arc<futures_util::lock::Mutex<InnerConnection>>`, so the `tokio::try_join!` batches in `fetch_person_data` serialize — ~12 sequential round-trips instead of 3 parallel batches. Free on local SQLite; roughly +9 RTTs per mutation on a remote PostgreSQL. Correctness is worth more than that on a write path.

**Exception: whole-tree rebuilds.** `rebuild_tree_full` (GEDCOM import, `POST /profiles/rebuild`, tree duplication) runs **outside** a transaction. It is an idempotent bulk operation over every person in the tree, and wrapping 100K rows would hold a very long-lived write lock for no benefit — a partial rebuild is repaired by running it again. The GEDCOM import itself is already atomic in its own transaction.

### 5.2 Mutation → refresh map

| Mutation | `person_denorm` | `person_search_fts` |
|---|---|---|
| **Edit person** (sex change) | Rewrite the affected set | Upsert affected rows |
| **Edit person name** | Rewrite `person_id` + everyone embedding its display name (spouses, children, parents) | Upsert affected rows |
| **Add/edit/delete event** | Rewrite `person_id` (or both spouses if a family event) | Upsert affected rows |
| **Add/delete family spouse** | Rewrite both spouses + all children in the family | Upsert affected rows |
| **Add/delete family child** | Rewrite child + both parents | Upsert affected rows |
| **Delete person** | `DELETE` the row, rewrite affected relatives | `DELETE` the row, upsert affected relatives |
| **Create person** | Build the new row (no family links yet) | Insert row |
| **GEDCOM import** | `replace_tree` (full rebuild) | `replace_tree` (full rebuild) |
| **Delete tree** | `delete_tree` | `delete_tree` |

Pedigrees have no row in this table: they are assembled per request, so there is nothing to invalidate.

### 5.3 Affected-set algorithm

This is the piece that survived the removal of the cache unchanged — "who is affected by this change" is a domain question, not a caching one. It lives in `oxidgene-api/src/profile/invalidation.rs`.

```rust
fn affected_persons(db, person_id) -> Vec<Uuid> {
    let mut affected = vec![person_id];

    // Spouses and children in all families where this person is a spouse
    for family in families_as_spouse(person_id) {
        for spouse in family.spouses where spouse.person_id != person_id {
            affected.push(spouse.person_id);
            // Their ProfileFamilyLink embeds our display name
        }
        for child in family.children {
            affected.push(child.person_id);
            // Their ProfileChildLink embeds our display name as parent
        }
    }

    // Parents in the family where this person is a child
    if let Some(family) = family_as_child(person_id) {
        for spouse in family.spouses {
            affected.push(spouse.person_id);
            // Their ProfileFamilyLink.children_ids includes us
        }
    }

    affected.dedup();
    affected
}
```

This set is **bounded** — typically 2–10 persons. Rebuilding one `PersonProfile` from DB data takes <2 ms. Above `FULL_FETCH_THRESHOLD` (50 persons), `ProfileService` switches from targeted per-person queries to a single whole-tree fetch, so bulk paths don't degrade into N narrow reads.

### 5.4 Latency budget

```
Mutation latency breakdown (typical):
  DB write:                    ~2-10ms
  Compute affected set:        ~1ms   (query family memberships)
  Rebuild projections:         ~2-5ms (2-10 targeted builds)
  Upsert person_denorm:        ~1ms
  Upsert person_search_fts:    ~1ms
  ─────────────────────────────
  Total overhead:              ~5-15ms (imperceptible to user)
```

### 5.5 Lazy materialization

A tree whose projections have never been built — an existing database on first run after the `person_denorm` migration — is materialized on its first read (`ensure_materialized`), the same trick `person_search_fts` uses. No manual backfill step is needed.

The same path covers a tree whose projections are **stale** rather than missing: `ensure_materialized` counts rows at the current `schema_version` (§2.1.1), so a build that raised it rebuilds each tree once, on first read. This is why the version migration backfills nothing — rebuilding inside a migration would re-derive every projection in the database up front, needing the whole builder there, to redo work the first read does lazily anyway.

---

## 6. API Endpoints

### 6.1 REST

Base path: `/api/v1`

| Method | Path | Description |
|---|---|---|
| `GET` | `/trees/{tree_id}/profiles/{person_id}` | Get a single person projection (full denormalized profile) |
| `GET` | `/trees/{tree_id}/profiles` | Get every person projection of a tree |
| `POST` | `/trees/{tree_id}/profiles/rebuild` | Force a full rebuild for a tree (admin/debug) |
| `POST` | `/trees/{tree_id}/profiles/rebuild/{person_id}` | Rebuild one person's projection |
| `POST` | `/trees/{tree_id}/profiles/drop` | Drop a tree's projections (rebuilt lazily on next read) |
| `GET` | `/trees/{tree_id}/pedigree/{root_person_id}?ancestor_depth=N&descendant_depth=N` | Assemble a windowed pedigree for a root person |
| `PATCH` | `/trees/{tree_id}/pedigree/{root_person_id}/expand?direction=ancestors\|descendants&from_depth=N&to_depth=N&other_depth=N` | Expand pedigree depth (returns only the new nodes and edges as a `PedigreeDelta`) |
| `GET` | `/trees/{tree_id}/persons/search?q=query&limit=20&offset=0` | Server-side person search (paginated, backed by `person_search_fts`; empty `q` = browse mode) |

> **Sprint E.9 renamed these routes.** `/cache/persons*` → `/profiles*`, `/cache/pedigree/*` → `/pedigree/*`, `/cache/invalidate` → `/profiles/drop`. The old paths named an implementation that no longer exists. GraphQL was renamed in step — see §6.2.

Used by: [Tree View](ui-genealogy-tree.md) (pedigree chart) · [Person Profile](ui-person-profile.md) (person detail) · [Search Results](ui-search-results.md) (search)

**Note:** all mutation endpoints (create/update/delete person, name, event, family, family_member, …) are unchanged but include a synchronous projection refresh after the DB write. See §5.

### 6.2 GraphQL

GraphQL mirrors REST exactly — same vocabulary, same operations. Sprint E.9 renamed the fields and types off `cache` in step with the routes.

```graphql
type Query {
  personProfile(treeId: ID!, personId: ID!): GqlPersonProfile!
  personProfiles(treeId: ID!): [GqlPersonProfile!]!
  pedigree(treeId: ID!, rootPersonId: ID!, ancestorDepth: Int!, descendantDepth: Int!): GqlPedigree!
  searchPersons(treeId: ID!, query: String!, limit: Int, offset: Int): GqlSearchResult!
}

type Mutation {
  expandPedigree(
    treeId: ID!, rootPersonId: ID!,
    direction: PedigreeDirection!,
    fromDepth: Int!, toDepth: Int!,
    otherDepth: Int = 0        # depth already loaded in the opposite direction
  ): GqlPedigreeDelta!
  rebuildTreeProfiles(treeId: ID!): GqlProfileRebuildResult!
  rebuildPersonProfile(treeId: ID!, personId: ID!): GqlProfileRebuildResult!
  dropTreeProfiles(treeId: ID!): Boolean!
}
```

| Was | Now | REST counterpart |
|---|---|---|
| `cachedPerson` | `personProfile` | `GET /profiles/{person_id}` |
| `cachedPersons` | `personProfiles` | `GET /profiles` |
| `rebuildTreeCache` | `rebuildTreeProfiles` | `POST /profiles/rebuild` |
| `rebuildPersonCache` | `rebuildPersonProfile` | `POST /profiles/rebuild/{person_id}` |
| `invalidateTreeCache` | `dropTreeProfiles` | `POST /profiles/drop` |
| `GqlCachedPerson`, `GqlCachedName`, … | `GqlPersonProfile`, `GqlProfileName`, … | — |
| `GqlCachedPedigree` | `GqlPedigree` | — |
| `GqlCacheRebuildResult` | `GqlProfileRebuildResult` | — |
| field `cachedAt` | field `builtAt` | — |

---

## 7. Code Layout

### 7.1 Modules

```
crates/oxidgene-core/
  src/projection.rs         // PersonProfile, Pedigree, SearchEntry & co.
                            // (in core so the frontend can deserialize them)

crates/oxidgene-db/
  src/entities/person_denorm.rs
  src/repo/person_denorm.rs         // PersonDenormRepo
  src/repo/person_search.rs         // PersonSearchRepo
  src/migration/m20260728_000001_person_denorm.rs

crates/oxidgene-api/
  src/profile/
    mod.rs                  // Re-exports ProfileService
    builder.rs              // Assembles projections from raw entities
    invalidation.rs         // affected_persons() — the affected-set algorithm
    service.rs              // ProfileService: reads, rebuilds, pedigree assembly
  src/rest/profile.rs       // REST handlers
  tests/profile_service_test.rs
```

### 7.2 Dependency graph

```
oxidgene-core (no internal deps)
    ↑
oxidgene-db (depends on: oxidgene-core)
    ↑
oxidgene-api (depends on: oxidgene-core, oxidgene-db, oxidgene-gedcom)
    ↑
oxidgene-server (depends on: oxidgene-api, oxidgene-db)
oxidgene-desktop (depends on: oxidgene-api, oxidgene-db, oxidgene-ui)

oxidgene-ui (depends on: oxidgene-core only)
```

`oxidgene-ui` used to depend on `oxidgene-cache`, which dragged `oxidgene-db`, `tokio` and `dashmap` into the WASM build purely to reach the projection types. Moving those types to `oxidgene-core` removed that edge.

### 7.3 AppState

```rust
struct AppState {
    pub db: DatabaseConnection,
    pub profiles: Arc<ProfileService>,
}
```

`ProfileService::new(db)` — no backend selection, no environment variables, no fallback path.

---

## 8. GEDCOM Import Integration

After a GEDCOM import (which can create thousands of persons at once), projections are built **eagerly** via `rebuild_tree_full`, which:

1. Fetches all persons + names + events + places + family members in parallel (`tokio::try_join!`).
2. Builds every `PersonProfile` in one batch.
3. `PersonDenormRepo::replace_tree` — writes them in chunks of 500.
4. `PersonSearchRepo::replace_tree` — rebuilds the search rows for the tree.

For 100K persons this takes a few seconds. Subsequent page interactions are instant.

---

## 9. Desktop

There is no cache directory, no disk snapshot, and nothing to flush at exit. The projections live in the same SQLite file as the data (`person_denorm`), written as part of each mutation. Closing the window shuts the embedded server down and exits.

This removed, from `apps/oxidgene-desktop/src/main.rs`: the cache-directory resolution, the pedigree memory budget, the staleness check, the load-from-disk path, the persist-on-shutdown thread handshake, and the `MemoryCacheStore` downcast.

---

## 10. Performance Targets

| Operation | Before projections | Now |
|---|---|---|
| **Pedigree chart (initial load)** | 1 large snapshot request (scales with tree size — unusable at 100K) | 1 windowed request (~250 persons, constant regardless of tree size) |
| **Pedigree depth change (+2 levels)** | Re-fetch full snapshot | 1 request returning only new nodes (~50–100 nodes) |
| **Pedigree depth decrease** | Same as above | Client-side only — zero network requests |
| **Person detail page** | 5–10 sequential requests (N+1) | 1 request, 1 indexed row read |
| **Search (10K persons)** | Full snapshot + client-side filter (~5–10 s) | 1 server-side request (<50 ms) |
| **After editing a person** | Invalidate everything, re-fetch snapshot | Rewrite 2–10 rows (~5–15 ms), next read instant |
| **After GEDCOM import (100K persons)** | Build snapshot on next page load (may time out) | Eager build of a few seconds, then all pages instant |
| **After a restart** | Cold cache, rebuild on first access | Nothing to warm — projections are already there |

Measured at 20K persons (release, SQLite): person load ~9 ms, search ~10 ms, full rebuild ~0.7 s. Regression guards live in `crates/oxidgene-api/tests/profile_service_test.rs` and `crates/oxidgene-db/tests/person_search_test.rs` (`#[ignore]`d — run with `cargo test -p oxidgene-api -- --ignored`).

---

## 11. Implementation History

### Sprints E.1–E.5 — the cache era (superseded)

Built `oxidgene-cache`: the `CacheStore` trait, `PersonProfile`/`Pedigree`/`CachedSearchIndex` types, the builder and invalidation logic, `MemoryCacheStore` (DashMap), `RedisCacheStore` (MessagePack), desktop disk persistence, and pedigree LRU budgets. The REST/GraphQL surface and the frontend integration date from these sprints.

### Sprint E.6 — Search moves into the database

- Replaced the in-memory `CachedSearchIndex` with the DB-native `person_search_fts` table (§4).
- Moved search to the normal search path (`GET /trees/{tree_id}/persons/search?q=…`); the GraphQL query was renamed `cachedSearch` → `searchPersons`.
- Removed `PersonCache` from `MemoryCacheStore` (`caches_persons()` flag): on desktop, projections were built on demand with targeted SQLite queries. Redis kept the shared person cache.
- Reduced desktop disk persistence to pedigrees only.

In hindsight this sprint was the proof of concept for E.9: moving one of the three caches into the database worked, and nothing was lost.

### Sprint E.9 — Denormalization replaces caching

- Added the `person_denorm` table, `PersonDenormRepo`, and migration `m20260728_000001_person_denorm`.
- Moved the projection types from `oxidgene-cache::types` to `oxidgene_core::projection`, removing the frontend's dependency on the database layer.
- Replaced `CacheService` with `ProfileService` in `oxidgene-api/src/profile/`, carrying the builder and the affected-set algorithm over unchanged.
- Pedigrees are assembled per request by walking the family links and joining against `person_denorm`; the pedigree cache, its LRU budget and its invalidation are gone.
- **Deleted the `oxidgene-cache` crate** — all three storage backends, ~4,100 lines — plus the `redis`, `dashmap`, `rmp-serde` and `bincode` dependencies.
- Removed the desktop disk-cache lifecycle entirely (§9).
- Renamed the REST routes off `/cache/*` **and the matching GraphQL fields and types** (§6.1, §6.2), so the two surfaces stay symmetric. The projection structs lost their `Cached*` prefix (`PersonProfile`, `ProfileName`, `Pedigree`, …) and the `cachedAt` field became `builtAt`.
- Widened all 119 repository methods from `&DatabaseConnection` to `&impl ConnectionTrait`, so mutations and their projection refresh run on one transaction and commit together (§5.1). 35 REST and GraphQL handlers now open and commit a transaction.
- Ported the integration tests to `crates/oxidgene-api/tests/profile_service_test.rs`, adding coverage for the three guarantees a cache could not offer: projections survive a service restart, a relative's projection is never left stale after a rename, and a rolled-back mutation leaves no projection behind.

**Known follow-ups:**
- Renaming a `Place` does not refresh the projections that embed its name; a full rebuild is needed. (Pre-existing, unchanged by E.9.)
