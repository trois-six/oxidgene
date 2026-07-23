---
type: "Data Model Specification"
title: "Data Model"
description: "Canonical domain entities, enums, and relationship model used by OxidGene services and UI."
tags: [oxidgene, specification, data-model, domain]
timestamp: 2026-07-16T00:00:00Z
---


# Data Model

> Part of the [OxidGene Specifications](index.md).
> See also: [Architecture](architecture.md) · [API Contract](api.md)

Source of truth in code: `crates/oxidgene-core/src/types/` (domain structs), `crates/oxidgene-core/src/enums.rs` (enums), `crates/oxidgene-db/src/entities/` (SeaORM entities), `crates/oxidgene-db/src/migration/m20250101_000001_initial.rs` (single consolidated migration).

---

## 1. Entities

### Tree

| Column | Type | Notes |
|---|---|---|
| `id` | UUID v7 | PK |
| `name` | String | Required |
| `description` | String? | Optional |
| `sosa_root_person_id` | UUID v7? | FK → Person — SOSA 1 root for Sosa-Stradonitz numbering, set in [Settings](ui-settings.md) §7 |
| `created_at` | DateTime | Auto |
| `updated_at` | DateTime | Auto |
| `deleted_at` | DateTime? | Soft delete |

Displayed in: [Homepage](ui-home.md) (tree cards) · [Settings](ui-settings.md) (tree & roots section)

### Person

| Column | Type | Notes |
|---|---|---|
| `id` | UUID v7 | PK |
| `tree_id` | UUID v7 | FK → Tree |
| `sex` | Sex | Enum |
| `privacy` | Privacy | Enum — per-person privacy override (default `Default`) |
| `created_at` | DateTime | Auto |
| `updated_at` | DateTime | Auto |
| `deleted_at` | DateTime? | Soft delete |

Displayed in: [Tree View](ui-genealogy-tree.md) (person cards) · [Person Edit Modal](ui-person-edit-modal.md) (edit form)

### PersonName

| Column | Type | Notes |
|---|---|---|
| `id` | UUID v7 | PK |
| `person_id` | UUID v7 | FK → Person |
| `name_type` | NameType | Enum |
| `given_names` | String? | |
| `surname` | String? | |
| `prefix` | String? | |
| `suffix` | String? | |
| `nickname` | String? | |
| `is_primary` | bool | Default true |
| `created_at` | DateTime | Auto |
| `updated_at` | DateTime | Auto |

### Family

| Column | Type | Notes |
|---|---|---|
| `id` | UUID v7 | PK |
| `tree_id` | UUID v7 | FK → Tree |
| `created_at` | DateTime | Auto |
| `updated_at` | DateTime | Auto |
| `deleted_at` | DateTime? | Soft delete |

Displayed in: [Tree View](ui-genealogy-tree.md) (connectors) · [Person Edit Modal](ui-person-edit-modal.md) (couple edit)

### FamilySpouse

| Column | Type | Notes |
|---|---|---|
| `id` | UUID v7 | PK |
| `family_id` | UUID v7 | FK → Family |
| `person_id` | UUID v7 | FK → Person |
| `role` | SpouseRole | Enum |
| `sort_order` | i32 | For ordering |

### FamilyChild

| Column | Type | Notes |
|---|---|---|
| `id` | UUID v7 | PK |
| `family_id` | UUID v7 | FK → Family |
| `person_id` | UUID v7 | FK → Person |
| `child_type` | ChildType | Enum |
| `sort_order` | i32 | For ordering |

### Event

| Column | Type | Notes |
|---|---|---|
| `id` | UUID v7 | PK |
| `tree_id` | UUID v7 | FK → Tree |
| `event_type` | EventType | Enum |
| `date_value` | String? | GEDCOM date phrase (free text, e.g. "ABT 1842") |
| `date_sort` | Date? | Normalized date for sorting |
| `date_qualifier` | DateQualifier | Enum — precision/shape of the date (default `Exact`) |
| `date_value2` | String? | Second date, used by the `Or` and `Between` qualifiers |
| `calendar` | Calendar | Enum — calendar system the date was recorded in (default `Gregorian`) |
| `cause` | String? | Cause of event (GEDCOM `CAUS`), e.g. cause of death |
| `place_id` | UUID v7? | FK → Place |
| `person_id` | UUID v7? | FK → Person (individual event) — never set together with `family_id` |
| `family_id` | UUID v7? | FK → Family (family event) — never set together with `person_id` |
| `description` | String? | Free text; also holds occupation title for `Occupation` events |
| `created_at` | DateTime | Auto |
| `updated_at` | DateTime | Auto |
| `deleted_at` | DateTime? | Soft delete |

`Event::year()` / `oxidgene_core::types::year_from_date` provide the shared display-year logic (prefer `date_sort`, fall back to the first 4-digit token of `date_value`) used by pedigree cards, the person narrative, dictionary usage lists, and search results.

Displayed in: [Tree View](ui-genealogy-tree.md) (events sidebar) · [Person Edit Modal](ui-person-edit-modal.md) (event blocks)

### EventWitness

Join table mirroring GEDCOM's `ASSO`/`RELA` associations — a witness, godparent, or other role-holder linked to an event as a real `Person` in the tree.

| Column | Type | Notes |
|---|---|---|
| `id` | UUID v7 | PK |
| `event_id` | UUID v7 | FK → Event |
| `person_id` | UUID v7 | FK → Person |
| `relation` | String? | Free text (e.g. "Godmother", "Witness") |
| `sort_order` | i32 | For ordering |

Exposed via `GET/POST /events/{id}/witnesses` (REST) and `addEventWitness`/`removeEventWitness` (GraphQL). Round-trips through GEDCOM import/export as a top-level `ASSO` on the INDI record (see [API Contract](api.md) §3).

### Place

| Column | Type | Notes |
|---|---|---|
| `id` | UUID v7 | PK |
| `tree_id` | UUID v7 | FK → Tree |
| `name` | String | Required — single free-text string (e.g. "Beaune, 21200, Côte-d'Or, Bourgogne-Franche-Comté, France") |
| `latitude` | f64? | Filled when selected from offline database or geocoding |
| `longitude` | f64? | Filled when selected from offline database or geocoding |
| `created_at` | DateTime | Auto |
| `updated_at` | DateTime | Auto |

The `name` is a single string. The recommended format is comma-separated from most specific to least specific (see [PlaceInput](ui-shared-components.md) §5), but any text is valid.

### Source

| Column | Type | Notes |
|---|---|---|
| `id` | UUID v7 | PK |
| `tree_id` | UUID v7 | FK → Tree |
| `title` | String | Required |
| `author` | String? | |
| `publisher` | String? | |
| `abbreviation` | String? | |
| `repository_name` | String? | |
| `created_at` | DateTime | Auto |
| `updated_at` | DateTime | Auto |
| `deleted_at` | DateTime? | Soft delete |

### Citation

| Column | Type | Notes |
|---|---|---|
| `id` | UUID v7 | PK |
| `source_id` | UUID v7 | FK → Source |
| `person_id` | UUID v7? | FK → Person |
| `event_id` | UUID v7? | FK → Event |
| `family_id` | UUID v7? | FK → Family |
| `page` | String? | Where in the source |
| `confidence` | Confidence | Enum |
| `text` | String? | Extracted text |
| `created_at` | DateTime | Auto |
| `updated_at` | DateTime | Auto |

### Media

| Column | Type | Notes |
|---|---|---|
| `id` | UUID v7 | PK |
| `tree_id` | UUID v7 | FK → Tree |
| `file_name` | String | Original filename |
| `mime_type` | String | MIME type |
| `file_path` | String | Storage path |
| `file_size` | i64 | Bytes |
| `title` | String? | |
| `description` | String? | |
| `date_value` | String? | Date of the media (GEDCOM date phrase, same format as Event) |
| `date_sort` | Date? | Normalized date for sorting |
| `place_id` | UUID v7? | FK → Place — where the media was created/taken |
| `created_at` | DateTime | Auto |
| `updated_at` | DateTime | Auto |
| `deleted_at` | DateTime? | Soft delete |

Displayed in: [Person Edit Modal](ui-person-edit-modal.md) (media section)

### MediaLink

| Column | Type | Notes |
|---|---|---|
| `id` | UUID v7 | PK |
| `media_id` | UUID v7 | FK → Media |
| `person_id` | UUID v7? | FK → Person |
| `event_id` | UUID v7? | FK → Event |
| `source_id` | UUID v7? | FK → Source |
| `family_id` | UUID v7? | FK → Family |
| `sort_order` | i32 | For ordering |

### Note

| Column | Type | Notes |
|---|---|---|
| `id` | UUID v7 | PK |
| `tree_id` | UUID v7 | FK → Tree |
| `text` | String | Required |
| `person_id` | UUID v7? | FK → Person |
| `event_id` | UUID v7? | FK → Event |
| `family_id` | UUID v7? | FK → Family |
| `source_id` | UUID v7? | FK → Source |
| `created_at` | DateTime | Auto |
| `updated_at` | DateTime | Auto |
| `deleted_at` | DateTime? | Soft delete |

### PersonAncestry (Closure Table)

| Column | Type | Notes |
|---|---|---|
| `id` | UUID v7 | PK |
| `tree_id` | UUID v7 | FK → Tree |
| `ancestor_id` | UUID v7 | FK → Person |
| `descendant_id` | UUID v7 | FK → Person |
| `depth` | i32 | Generation distance (0 = self) |

Used by: ancestor/descendant [API endpoints](api.md) · SOSA badge computation ([Person Profile](ui-person-profile.md), [Dictionary](ui-dictionary.md) §12)

### person_search_fts (Search Table — Sprint E.6)

DB-native person search index; not a domain entity (no UUID PK, maintained by `PersonSearchRepo`). SQLite FTS5 virtual table on desktop, plain indexed table on PostgreSQL. Columns: normalized `surname`, `given_names`, `maiden_name`, `birth_year`, `death_year` plus unindexed display fields. See [Caching](caching.md) §2.3.

---

## 2. Enums

Defined in `crates/oxidgene-core/src/enums.rs`; DB string representations in `crates/oxidgene-db/src/entities/sea_enums.rs`.

```rust
enum Sex {
    Male,
    Female,
    Unknown,
}

enum NameType {
    Birth,
    Married,
    AlsoKnownAs,
    Maiden,
    Religious,
    Other,
}

enum SpouseRole {
    Husband,
    Wife,
    Partner,
}

enum ChildType {
    Biological,
    Adopted,
    Foster,
    Step,
    Unknown,
}

/// Per-person privacy override (see ui-person-edit-modal.md §7).
enum Privacy {
    Default,   // Follows the tree-level privacy settings
    Public,    // Always visible regardless of tree settings
    Private,   // Always hidden regardless of tree settings
}

/// Precision/shape of a date entry (see ui-person-edit-modal.md §5).
/// `Or` and `Between` use two date values; the rest use a single one.
enum DateQualifier {
    Exact,     // default
    About,     // GEDCOM ABT
    Perhaps,   // GEDCOM EST
    Before,    // GEDCOM BEF
    After,     // GEDCOM AFT
    Or,        // app-specific (two dates)
    Between,   // GEDCOM BET ... AND ...
    FromAge,   // app-specific
}

/// Calendar system used to record a date.
enum Calendar {
    Gregorian, // default
    Julian,
    Hebrew,
    FrenchRepublican,
}

// GEDCOM tag mapping shown per variant. Variants without a native tag
// export as EVEN + TYPE subrecord.
enum EventType {
    // Individual events
    Birth,               // BIRT
    Death,               // DEAT
    Baptism,             // BAPM
    Confirmation,        // (EVEN + TYPE)
    FirstCommunion,      // (EVEN + TYPE)
    BarBatMitzvah,       // (EVEN + TYPE)
    MilitaryService,     // (EVEN + TYPE)
    Burial,              // BURI
    Cremation,           // CREM
    Graduation,          // GRAD
    Immigration,         // IMMI
    Emigration,          // EMIG
    Naturalization,      // NATU
    Census,              // CENS
    Occupation,          // OCCU (description holds the title)
    Residence,           // RESI
    Retirement,          // RETI
    Will,                // WILL
    Probate,             // PROB
    Adoption,            // ADOP — individual-level, may reference the
                         //        adoptive family via a nested FAMC
    // Individual attributes (GEDCOM 5.5.1 "attribute" tags)
    CasteName,           // CAST
    PhysicalDescription, // DSCR
    Education,           // EDUC
    NationalId,          // IDNO
    NationalOrigin,      // NATI
    ChildrenCount,       // NCHI
    MarriagesCount,      // NMR
    Property,            // PROP
    Religion,            // RELI
    SocialSecurityNumber,// SSN
    NobilityTitle,       // TITL (as an individual attribute)
    Fact,                // FACT
    // Family events
    Marriage,            // MARR
    Divorce,             // DIV
    Annulment,           // ANUL
    Engagement,          // ENGA
    MarriageBann,        // MARB
    MarriageContract,    // MARC
    MarriageLicense,     // MARL
    MarriageSettlement,  // MARS
    CivilUnion,          // (EVEN family tag) — PACS / cohabitation
    Separation,          // SEP (GEDCOM 7.0)
    DivorceFiled,        // DIVF
    // Generic
    Other,               // EVEN + TYPE
}

// Maps to GEDCOM QUAY (Certainty Assessment)
enum Confidence {
    VeryLow,   // QUAY 0 (Unreliable)
    Low,       // QUAY 1 (Questionable)
    Medium,    // QUAY 2 (Secondary)
    High,      // QUAY 3 (Direct)
    VeryHigh,  // app-specific fifth level
}
```

`EventType::is_individual()` / `is_family()` partition the variants; `Adoption` is individual, never family.

---

## 3. Entity Relationship Diagram (Mermaid)

```mermaid
erDiagram
    Tree ||--o{ Person : contains
    Tree ||--o{ Family : contains
    Tree ||--o{ Event : contains
    Tree ||--o{ Place : contains
    Tree ||--o{ Source : contains
    Tree ||--o{ Media : contains
    Tree ||--o{ Note : contains
    Tree ||--o{ PersonAncestry : contains
    Tree }o--o| Person : "sosa_root_person_id"

    Person ||--o{ PersonName : "has names"
    Person ||--o{ FamilySpouse : "spouse in"
    Person ||--o{ FamilyChild : "child in"
    Person ||--o{ Event : "individual events"
    Person ||--o{ EventWitness : "witnesses"
    Person ||--o{ Citation : "cited by"
    Person ||--o{ MediaLink : "linked media"
    Person ||--o{ Note : "has notes"

    Family ||--o{ FamilySpouse : "has spouses"
    Family ||--o{ FamilyChild : "has children"
    Family ||--o{ Event : "family events"
    Family ||--o{ Citation : "cited by"
    Family ||--o{ MediaLink : "linked media"
    Family ||--o{ Note : "has notes"

    Event }o--o| Place : "occurred at"
    Media }o--o| Place : "taken at"
    Event ||--o{ EventWitness : "has witnesses"
    Event ||--o{ Citation : "cited by"
    Event ||--o{ MediaLink : "linked media"
    Event ||--o{ Note : "has notes"

    Source ||--o{ Citation : "has citations"
    Source ||--o{ MediaLink : "linked media"
    Source ||--o{ Note : "has notes"

    Media ||--o{ MediaLink : "linked to"

    PersonAncestry }o--|| Person : "ancestor"
    PersonAncestry }o--|| Person : "descendant"
```
