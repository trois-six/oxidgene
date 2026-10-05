//! HTTP API client for communicating with the OxidGene backend.
//!
//! Provides a typed client wrapping [`reqwest::Client`] that maps to the
//! REST API defined in `oxidgene-api`.  All methods return domain types
//! from [`oxidgene_core`] directly, since those types already derive
//! `Serialize` / `Deserialize`.

mod history;

use base64::Engine as _;
#[cfg(feature = "telemetry-client")]
use opentelemetry::global;
#[cfg(feature = "telemetry-client")]
use opentelemetry::propagation::Injector;
use oxidgene_core::projection::{Pedigree, PersonProfile, SearchEntry, SearchResult};
use oxidgene_core::types::{
    Citation, Connection, DOCUMENT_MIME, Event, EventWitness, Family, FamilyChild, FamilySpouse,
    ImageCrop, ImageSource, Kinship, Media, Note, Person, PersonName, Place, QualifiedYear,
    Repository, Source, SourceRepository, SpouseAge, Tree, Vignette,
};
use oxidgene_core::{
    Calendar, ChildType, Confidence, DateQualifier, DocumentCategory, EventType, NameType, Privacy,
    Sex, SourceMediaType, SpouseRole, TreeDefaultPrivacy,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
#[cfg(feature = "telemetry-client")]
use tracing::Instrument as _;
#[cfg(feature = "telemetry-client")]
use tracing_opentelemetry::OpenTelemetrySpanExt as _;
use uuid::Uuid;

// ── PersonDetail — person + server-computed SOSA number ──────────────

/// Mirrors `PersonDetailResponse` from the API: all `Person` fields flat + SOSA.
#[derive(Debug, Clone, Deserialize)]
pub struct PersonDetail {
    pub id: Uuid,
    pub tree_id: Uuid,
    pub sex: Sex,
    pub privacy: Privacy,
    /// Which image represents this person: a whole media, or a region of one.
    /// At most one is ever set.
    #[serde(default)]
    pub portrait_media_id: Option<Uuid>,
    #[serde(default)]
    pub portrait_vignette_id: Option<Uuid>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    pub sosa_number: Option<u64>,
}

// ── Re-usable request / response DTOs (client-side mirrors) ─────────

/// Paginated response returned by list endpoints.
/// Re-uses the same shape as `oxidgene_core::types::Connection<T>`.
type PaginatedResponse<T> = Connection<T>;

/// The query parameters naming the records a list is restricted to: one per
/// owner given, none for an owner left out.
fn owner_filters<const N: usize>(
    owners: [(&'static str, Option<Uuid>); N],
) -> Vec<(&'static str, String)> {
    owners
        .into_iter()
        .filter_map(|(name, id)| id.map(|id| (name, id.to_string())))
        .collect()
}

#[derive(Debug, Clone, Copy, Default)]
pub enum PersonSearchSort {
    #[default]
    Relevance,
    NameAsc,
    NameDesc,
    BirthAsc,
    BirthDesc,
}

impl PersonSearchSort {
    fn as_str(self) -> &'static str {
        match self {
            Self::Relevance => "relevance",
            Self::NameAsc => "name_asc",
            Self::NameDesc => "name_desc",
            Self::BirthAsc => "birth_asc",
            Self::BirthDesc => "birth_desc",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct PersonSearchParams {
    pub query: String,
    pub limit: u32,
    pub offset: u32,
    pub sex: Option<Sex>,
    pub surname: Option<String>,
    pub given_names: Option<String>,
    pub occupation: Option<String>,
    pub spouse_surname: Option<String>,
    pub spouse_given_names: Option<String>,
    pub father_surname: Option<String>,
    pub father_given_names: Option<String>,
    pub mother_surname: Option<String>,
    pub mother_given_names: Option<String>,
    pub birth_from: Option<i32>,
    pub birth_to: Option<i32>,
    pub death_from: Option<i32>,
    pub death_to: Option<i32>,
    pub place: Option<String>,
    pub event_type: Option<EventType>,
    pub event_from: Option<i32>,
    pub event_to: Option<i32>,
    pub has_media: bool,
    pub sort: PersonSearchSort,
}

// ── Dictionary — distinct-value aggregations with usage counts ──────

/// A distinct free-text value (surname, occupation label) plus how many
/// persons carry it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct DictionaryEntry {
    pub value: String,
    /// Filing key when surname particles are ignored; see the sorting
    /// preference in `crate::prefs`.
    #[serde(default)]
    pub sort_key: String,
    pub count: i64,
    /// Family names only: how many of `count` carry the value as their
    /// primary name, i.e. how many a rename would reach.
    #[serde(default)]
    pub primary_count: Option<i64>,
}

/// A source paired with its citation count.
#[derive(Debug, Clone, Deserialize)]
pub struct SourceDictionaryEntry {
    #[serde(flatten)]
    pub source: Source,
    pub count: i64,
    /// The names of the repositories holding the source.
    #[serde(default)]
    pub repositories: Vec<String>,
}

/// A prefix group for the Sources tab's smart drill-down (see
/// ui-dictionary.md §8): `label` is the resolved prefix (see
/// `SourceDrillResponse`) extended by exactly one more character, paired
/// with how many sources fall under it.
#[derive(Debug, Clone, Deserialize)]
pub struct SourceGroupEntry {
    pub label: String,
    pub count: i64,
}

/// Response for the Sources tab's smart drill-down (ui-dictionary.md
/// §8.10): the backend auto-skips forced single-choice levels, so `prefix`
/// may be longer than the prefix that was requested. `groups` is empty
/// once `total` has dropped to <= the drill threshold, and the level's
/// sources then come with it.
#[derive(Debug, Clone, Deserialize)]
pub struct SourceDrillResponse {
    pub prefix: String,
    pub total: i64,
    pub groups: Vec<SourceGroupEntry>,
    #[serde(default)]
    pub sources: Option<Vec<SourceDictionaryEntry>>,
}

/// A place paired with its usage count (events + media referencing it).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct PlaceDictionaryEntry {
    #[serde(flatten)]
    pub place: Place,
    pub count: i64,
}

/// The persons a name suggestion counts, when not the whole tree: those a
/// search on this surname and these given names finds. Blank is no filter.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NameScope {
    pub surname: String,
    pub given_names: String,
}

/// The entry-form field a [`ValueSuggestion`] is for (`docs/api.md`, value
/// suggestions).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuggestionField {
    FamilyNames,
    /// One given name: the field completes the word being typed.
    GivenNames,
    Occupations,
    /// Source titles.
    Sources,
    /// The tree's place names.
    Places,
}

impl SuggestionField {
    fn path(self) -> &'static str {
        match self {
            Self::FamilyNames => "family-names",
            Self::GivenNames => "given-names",
            Self::Occupations => "occupations",
            Self::Sources => "sources",
            Self::Places => "places",
        }
    }
}

/// A value a field suggests: one the tree holds, or a term a reference sheet
/// answers to.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ValueSuggestion {
    pub value: String,
    /// Persons carrying it, or citations of a source; 0 for a sheet's term.
    pub count: i64,
    /// Whether a reference sheet answers to the value itself.
    pub reference: bool,
}

/// A place suggested from the place dictionary (`docs/place-dictionary.md`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct PlaceSuggestion {
    /// What a place field stores: name, code, subdivision, region, country.
    pub label: String,
    pub name: String,
    #[serde(default)]
    pub valid_until: Option<String>,
    #[serde(default)]
    pub latitude: Option<f64>,
    #[serde(default)]
    pub longitude: Option<f64>,
}

/// How many persons a tree held over the days it was worked on
/// (`docs/ui-statistics.md` §10): each day (UTC) the count changed, oldest
/// first, and the imports to mark.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TreeGrowth {
    pub days: Vec<GrowthDay>,
    pub imports: Vec<GrowthImport>,
}

/// One day's changes to the number of persons.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct GrowthDay {
    pub date: chrono::NaiveDate,
    /// Persons created, imported or restored that day.
    pub added: i64,
    /// Persons deleted or merged into another that day.
    pub removed: i64,
}

/// A completed import.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct GrowthImport {
    pub occurred_at: chrono::DateTime<chrono::Utc>,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub file_name: Option<String>,
    pub persons: i64,
}

/// A tree's statistics (`docs/ui-statistics.md`), as the backend computes
/// them. Time series are filed by year, oldest first; the page groups them
/// into periods.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TreeStatistics {
    pub persons: i64,
    pub men: i64,
    pub women: i64,
    pub unknown_sex: i64,
    pub unions: i64,
    pub places: i64,
    pub sources: i64,
    pub first_year: Option<i32>,
    pub last_year: Option<i32>,
    pub dated_births: i64,
    pub dated_deaths: i64,
    pub without_parents: i64,
    pub without_children: i64,
    pub without_union: i64,
    pub surnames: i64,
    pub given_names: i64,
    pub lifespan: StatSexSummary,
    pub first_union_age: StatSexSummary,
    pub generation_interval: StatSummary,
    pub family_size: StatSummary,
    pub top_surnames: Vec<StatCount>,
    pub top_given_names_men: Vec<StatCount>,
    pub top_given_names_women: Vec<StatCount>,
    pub top_occupations: Vec<StatCount>,
    /// Labels are `EventType`s in their snake_case form.
    pub event_types: Vec<StatCount>,
    pub children_histogram: Vec<i64>,
    /// Births, baptisms, unions, deaths and burials per year.
    pub events_by_year: Vec<StatYearCounts>,
    /// Births of men and of women per year.
    pub births_by_sex: Vec<StatYearCounts>,
    /// Per year of birth: births, deaths before one and before five.
    pub mortality: Vec<StatYearCounts>,
    pub age_at_death: StatSexSeries,
    pub life_expectancy: StatSexSeries,
    pub births_by_month: Vec<StatYearCounts>,
    pub parents_age: StatParentAges,
    pub age_at_first_union: StatSexSeries,
    pub unions_by_weekday: Vec<StatYearCounts>,
    pub unions_by_month: Vec<StatYearCounts>,
    pub union_duration: Vec<StatYearSum>,
    pub children_per_union: Vec<StatYearSum>,
    pub birth_spacing: Vec<StatYearSum>,
    pub first_last_child_gap: Vec<StatYearSum>,
    pub spouse_age_gap: Vec<StatYearSum>,
    pub pyramid: Vec<StatPyramidBand>,
    pub records: Vec<StatRecord>,
    pub recent_births: Vec<StatPerson>,
    pub recent_deaths: Vec<StatPerson>,
    pub recent_unions: Vec<StatUnion>,
    pub oldest_possibly_alive: Vec<StatPerson>,
    pub longest_lives: Vec<StatPerson>,
    pub largest_families: Vec<StatFamily>,
    pub located_places: Vec<StatPlace>,
    pub top_places: Vec<StatPlace>,
    pub unlocated_places: i64,
    pub countries: i64,
    pub regions: i64,
    pub subdivisions: i64,
    pub births_by_country: Vec<StatCount>,
    pub births_by_region: Vec<StatCount>,
    pub births_by_subdivision: Vec<StatCount>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct StatCount {
    pub label: String,
    pub count: i64,
}

/// What one year adds to an average: the sum of its values and how many
/// they are.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct StatYearSum {
    pub year: i32,
    pub sum: f64,
    pub count: i64,
}

/// One year's counts per category.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct StatYearCounts {
    pub year: i32,
    pub counts: Vec<i64>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct StatSexSeries {
    pub men: Vec<StatYearSum>,
    pub women: Vec<StatYearSum>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct StatParentAges {
    pub father_first_child: Vec<StatYearSum>,
    pub mother_first_child: Vec<StatYearSum>,
    pub father_last_child: Vec<StatYearSum>,
    pub mother_last_child: Vec<StatYearSum>,
    pub father_every_child: Vec<StatYearSum>,
    pub mother_every_child: Vec<StatYearSum>,
}

/// A set of values in brief; every figure is absent without a value.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct StatSummary {
    pub count: i64,
    pub mean: Option<f64>,
    pub median: Option<f64>,
    pub std_dev: Option<f64>,
    pub min: Option<f64>,
    pub max: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct StatSexSummary {
    pub all: StatSummary,
    pub men: StatSummary,
    pub women: StatSummary,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct StatPyramidBand {
    pub from: i64,
    pub men: i64,
    pub women: i64,
}

/// A date as recorded, formatted by the client in its language.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct StatDate {
    pub value: Option<String>,
    pub value2: Option<String>,
    pub qualifier: DateQualifier,
    pub calendar: Calendar,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct StatPerson {
    pub person_id: Uuid,
    pub name: String,
    pub sex: Sex,
    pub date: Option<StatDate>,
    pub place: Option<String>,
    pub birth: Option<StatDate>,
    pub death: Option<StatDate>,
    pub age: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct StatUnion {
    pub family_id: Uuid,
    pub spouses: Vec<StatPersonRef>,
    pub date: StatDate,
    pub place: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct StatFamily {
    pub family_id: Uuid,
    pub spouses: Vec<StatPersonRef>,
    pub children: i64,
    pub date: Option<StatDate>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct StatPersonRef {
    pub person_id: Uuid,
    pub name: String,
}

/// One of the tree's records: who holds it, with a value in days (ages
/// and durations) or a count, and the event it is about.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct StatRecord {
    pub kind: String,
    pub persons: Vec<StatPersonRef>,
    pub value: Option<f64>,
    pub value2: Option<i64>,
    pub date: Option<StatDate>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct StatPlace {
    pub place_id: Uuid,
    pub name: String,
    pub count: i64,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
}

/// A tree's potential duplicates (`docs/ui-tools.md`): every pair found,
/// and the best of them.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct PotentialDuplicates {
    pub count: i64,
    pub pairs: Vec<DuplicatePair>,
}

/// Two records that may be one person, how alike (0 to 100) and why.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct DuplicatePair {
    pub score: i64,
    pub reasons: Vec<String>,
    pub first: SearchEntry,
    pub second: SearchEntry,
    #[serde(default)]
    pub first_dates: LifeDates,
    #[serde(default)]
    pub second_dates: LifeDates,
}

/// A record's birth (or baptism) and death (or burial) dates, as recorded.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct LifeDates {
    pub birth: Option<StatDate>,
    pub death: Option<StatDate>,
}

/// A tree's anomalies (`docs/ui-tools.md`), the rules that found something
/// in catalogue order.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TreeAnomalies {
    pub persons: i64,
    pub rules: Vec<AnomalyRule>,
}

/// What one rule found: `count` in all, `items` listing at most a bounded
/// number of them.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct AnomalyRule {
    pub rule: String,
    pub category: String,
    pub severity: String,
    pub count: i64,
    pub items: Vec<Anomaly>,
}

/// One anomaly: its persons, the subject first, and what the rule measured
/// (years for ages and gaps in years, days for gaps in days).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Anomaly {
    pub persons: Vec<StatPersonRef>,
    pub family_id: Option<Uuid>,
    pub value: Option<i64>,
    /// An `EventType` in its snake_case form.
    pub event_type: Option<String>,
    /// Recorded text: an unreadable date, a relation.
    pub text: Option<String>,
}

/// Which ancestors of a tree's SOSA root are known, generation by
/// generation (`docs/ui-tools.md`); no root and no generations when the
/// tree has no SOSA root.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct AncestryCompleteness {
    pub root: Option<StatPersonRef>,
    pub generations: Vec<AncestryGeneration>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct AncestryGeneration {
    pub generation: i32,
    pub expected: i64,
    pub found: i64,
    pub with_birth: i64,
    pub with_death: i64,
    pub with_union: i64,
    pub living: i64,
    /// Missing ancestors whose child is missing too, not listed.
    pub implied_missing: i64,
    pub entries: Vec<AncestryEntry>,
}

/// One SOSA number: the ancestor there, or `None` when missing.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct AncestryEntry {
    pub sosa: i64,
    pub person: Option<AncestorFacts>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct AncestorFacts {
    pub person_id: Uuid,
    pub name: String,
    pub sex: Sex,
    pub birth: Option<StatDate>,
    pub death: Option<StatDate>,
    pub has_birth: bool,
    pub has_death: bool,
    pub has_union: bool,
    pub living: bool,
}

/// One country's outline for the statistics heat map: outer rings as flat
/// `longitude, latitude` pairs in tenths of a degree, and its populated
/// places.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct BasemapCountry {
    pub iso: String,
    pub name: String,
    pub rings: Vec<Vec<i32>>,
    pub cities: Vec<BasemapCity>,
}

/// A populated place, labelled on the map from `zoom` (tenths of a web map
/// zoom level) on; position in tenths of a degree, population in thousands.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct BasemapCity {
    pub name: String,
    pub names: Vec<BasemapName>,
    pub lon: i32,
    pub lat: i32,
    pub zoom: i32,
    pub population: i64,
}

/// A place's name in one interface language.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct BasemapName {
    pub lang: String,
    pub name: String,
}

/// A person resolved for a dictionary usage drill-down list: name parts +
/// birth/death years, computed server-side in one bulk query.
#[derive(Debug, Clone, Deserialize)]
pub struct PersonUsageEntry {
    pub person_id: Uuid,
    pub given_names: Option<String>,
    pub surname: Option<String>,
    pub birth_year: Option<i32>,
    #[serde(default)]
    pub birth_qualifier: DateQualifier,
    pub death_year: Option<i32>,
    #[serde(default)]
    pub death_qualifier: DateQualifier,
}

impl PersonUsageEntry {
    /// The birth/death years with their precision, ready for
    /// [`format_lifespan`](crate::components::pedigree_chart::format_lifespan).
    pub fn lifespan_years(&self) -> (Option<QualifiedYear>, Option<QualifiedYear>) {
        (
            self.birth_year
                .map(|y| QualifiedYear::new(y, self.birth_qualifier)),
            self.death_year
                .map(|y| QualifiedYear::new(y, self.death_qualifier)),
        )
    }
}

/// Body of the dictionary's bulk particle edit.
#[derive(Debug, Serialize)]
struct SetFamilyNameParticleBody {
    value: String,
    /// Empty means "this name has no particle".
    particle: String,
}

/// Outcome of a bulk particle edit.
#[derive(Debug, Clone, Deserialize)]
pub struct FamilyNameParticleUpdate {
    /// The surname as it will still be listed — re-cutting moves where the
    /// name files, not the text.
    pub value: String,
    pub surname_prefix: Option<String>,
    pub surname: String,
    pub names_updated: usize,
    pub persons_updated: usize,
}

/// Body of the dictionary's family-name rename.
#[derive(Debug, Serialize)]
struct RenameFamilyNameBody {
    value: String,
    new_value: String,
    /// Absent: keep the split `new_value` already has, or detect it.
    #[serde(skip_serializing_if = "Option::is_none")]
    particle: Option<String>,
}

/// Outcome of a family-name rename.
#[derive(Debug, Clone, Deserialize)]
pub struct FamilyNameRename {
    pub value: String,
    pub new_value: String,
    pub surname_prefix: Option<String>,
    pub surname: String,
    pub names_updated: usize,
    pub persons_updated: usize,
    /// `new_value` was already listed: the renamed names joined it.
    pub merged: bool,
}

// ── Reference content — occupation sheets, given-name meanings ──────

/// Occupation fiche content, localized to the requesting UI language.
#[derive(Debug, Clone, Deserialize)]
pub struct OccupationReference {
    pub label: String,
    pub summary: String,
    pub text: String,
}

/// Given-name meaning content, localized to the requesting UI language.
#[derive(Debug, Clone, Deserialize)]
pub struct GivenNameReference {
    pub label: String,
    pub origin: String,
    pub meaning: String,
    pub text: String,
    pub feast_day: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GivenNameReferenceMatch {
    pub term: String,
    #[serde(flatten)]
    pub reference: GivenNameReference,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OccupationReferenceMatch {
    pub term: String,
    #[serde(flatten)]
    pub reference: OccupationReference,
}

#[derive(Debug, Serialize)]
struct ReferenceTermsBody<'a> {
    terms: &'a [String],
}

const REFERENCE_TERM_BATCH_SIZE: usize = 128;

fn reference_term_batches(terms: &[String]) -> std::slice::Chunks<'_, String> {
    terms.chunks(REFERENCE_TERM_BATCH_SIZE)
}

// ── Tree request bodies ─────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct CreateTreeBody {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// A tree as listed on the home page, with transient server-job state.
#[derive(Debug, Clone, Deserialize)]
pub struct TreeListItem {
    #[serde(flatten)]
    pub tree: Tree,
    #[serde(default)]
    pub import_in_progress: bool,
    #[serde(default)]
    pub import_job_id: Option<Uuid>,
}

impl std::ops::Deref for TreeListItem {
    type Target = Tree;

    fn deref(&self) -> &Self::Target {
        &self.tree
    }
}

#[derive(Debug, Default, Serialize)]
pub struct UpdateTreeBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sosa_root_person_id: Option<Option<Uuid>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_person_id: Option<Option<Uuid>>,
    /// What `Privacy::Default` resolves to for everything in this tree.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_privacy: Option<TreeDefaultPrivacy>,
    /// Whether entry fields suggest values as the user types.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry_suggestions: Option<bool>,
    /// How much of a date the tree's pages write.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date_format: Option<oxidgene_core::enums::DateDisplayFormat>,
    /// Whether lifespans write the birth and death symbols.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date_symbols: Option<bool>,
    /// Whether an approximate date reads « c. ».
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date_circa: Option<bool>,
    /// The calendar a date recorded in another one is also given in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date_calendar: Option<oxidgene_core::enums::Calendar>,
    /// Whether surname fields write in capitals.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub surname_uppercase: Option<bool>,
    /// Whether adding a relative offers the persons already in the tree.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggest_persons: Option<bool>,
    /// The order and form of a date field's parts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date_input_format: Option<oxidgene_core::enums::DateInputFormat>,
    /// The calendar an empty date field starts in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date_input_calendar: Option<oxidgene_core::enums::Calendar>,
    /// Who the tree's GEDCOM exports say they are from.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub submitter_name: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub submitter_email: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub submitter_address: Option<Option<String>>,
}

#[derive(Debug, Serialize)]
pub struct DuplicateTreeBody {
    pub name: String,
}

// ── Person request bodies ───────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct CreatePersonBody {
    pub sex: Sex,
}

#[derive(Debug, Serialize)]
pub struct UpdatePersonBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sex: Option<Sex>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub privacy: Option<Privacy>,
}

/// Request body for recording that a person differs from their homonyms.
#[derive(Debug, Serialize)]
pub struct MarkPersonsDistinctBody {
    pub person_ids: Vec<Uuid>,
}

/// Request body for merging a duplicate into the person it names.
#[derive(Debug, Serialize)]
pub struct MergePersonBody<'a> {
    pub duplicate_id: Uuid,
    pub choices: &'a MergeChoices,
}

/// What the comparison of a merge chose (`docs/api.md`, merge). The default
/// keeps the kept person's name and sex and moves everything else.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct MergeChoices {
    /// Own events of either person left out of the merged record.
    pub left_out_events: Vec<Uuid>,
    /// The duplicate's direct media links not taken.
    pub left_out_media_links: Vec<Uuid>,
    pub surname_from_duplicate: bool,
    pub given_names_from_duplicate: bool,
    pub sex_from_duplicate: bool,
}

// ── PersonName request bodies ───────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct CreatePersonNameBody {
    pub name_type: NameType,
    pub given_names: Option<String>,
    /// Surname root only — split the particle off with
    /// `oxidgene_core::types::split_surname_particle` before sending.
    pub surname: Option<String>,
    pub surname_prefix: Option<String>,
    pub prefix: Option<String>,
    pub suffix: Option<String>,
    pub nickname: Option<String>,
    pub is_primary: bool,
    #[serde(skip_serializing_if = "is_zero")]
    pub sort_order: i32,
}

fn is_zero(v: &i32) -> bool {
    *v == 0
}

#[derive(Debug, Serialize)]
pub struct UpdatePersonNameBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name_type: Option<NameType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub given_names: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub surname: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub surname_prefix: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefix: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suffix: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nickname: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_primary: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort_order: Option<i32>,
}

// ── Family member request bodies ────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct AddSpouseBody {
    pub person_id: Uuid,
    pub role: SpouseRole,
    #[serde(default)]
    pub sort_order: i32,
}

#[derive(Debug, Serialize)]
pub struct AddChildBody {
    pub person_id: Uuid,
    pub child_type: ChildType,
    #[serde(default)]
    pub sort_order: i32,
}

// ── Event request bodies ────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct CreateEventBody {
    pub event_type: EventType,
    pub date_value: Option<String>,
    pub date_qualifier: DateQualifier,
    pub date_value2: Option<String>,
    pub calendar: Calendar,
    pub cause: Option<String>,
    pub age: Option<String>,
    pub agency: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub spouse_ages: Vec<SpouseAge>,
    pub place_id: Option<Uuid>,
    pub person_id: Option<Uuid>,
    pub family_id: Option<Uuid>,
    pub description: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct UpdateEventBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_type: Option<EventType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date_value: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date_qualifier: Option<DateQualifier>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date_value2: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub calendar: Option<Calendar>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cause: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub age: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agency: Option<Option<String>>,
    /// Replaces a family event's spouse ages.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spouse_ages: Option<Vec<SpouseAge>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub place_id: Option<Option<Uuid>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<Option<String>>,
}

/// Request body for adding a witness to an event.
#[derive(Debug, Serialize)]
pub struct AddEventWitnessBody {
    pub person_id: Uuid,
    pub relation: Option<String>,
    #[serde(default)]
    pub sort_order: i32,
}

// ── Place request bodies ────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct CreatePlaceBody {
    pub name: String,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct UpdatePlaceBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latitude: Option<Option<f64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub longitude: Option<Option<f64>>,
}

// ── Source request bodies ───────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct CreateSourceBody {
    pub title: String,
    pub author: Option<String>,
    pub publisher: Option<String>,
    pub abbreviation: Option<String>,
    pub agency: Option<String>,
}

/// A source update: `None` leaves a field alone, `Some(None)` clears it.
#[derive(Debug, Default, Serialize)]
pub struct UpdateSourceBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publisher: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub abbreviation: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agency: Option<Option<String>>,
}

// ── Repository request bodies ───────────────────────────────────────

#[derive(Debug, Default, Serialize)]
pub struct CreateRepositoryBody {
    pub name: String,
    pub address: Option<String>,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub website: Option<String>,
}

/// A repository update: every field written, a `None` one cleared.
#[derive(Debug, Default, Serialize)]
pub struct UpdateRepositoryBody {
    pub name: String,
    pub address: Option<String>,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub website: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AddSourceRepositoryBody {
    pub repository_id: Uuid,
    pub call_number: Option<String>,
    pub media_type: Option<SourceMediaType>,
}

/// A source held by a repository, with the link saying under which call
/// number.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct HeldSource {
    #[serde(flatten)]
    pub link: SourceRepository,
    pub source: Source,
}

// ── Citation request bodies ─────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct CreateCitationBody {
    pub source_id: Uuid,
    pub person_id: Option<Uuid>,
    pub event_id: Option<Uuid>,
    pub family_id: Option<Uuid>,
    pub page: Option<String>,
    /// `None` when the evidence is not assessed.
    pub confidence: Option<Confidence>,
    pub text: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct UpdateCitationBody {
    /// Repoints the citation at another source.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<Option<Confidence>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<Option<String>>,
}

// ── Note request bodies ─────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct CreateNoteBody {
    pub text: String,
    pub person_id: Option<Uuid>,
    pub event_id: Option<Uuid>,
    pub family_id: Option<Uuid>,
    pub source_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_id: Option<uuid::Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository_id: Option<Uuid>,
}

#[derive(Debug, Serialize)]
pub struct UpdateNoteBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

// ── MediaLink DTOs ───────────────────────────────────────────────────

/// Whether a `file_path` is an address rather than a path.
///
/// The column holds whatever produced the record wrote there: a Windows path
/// out of a GEDCOM, a relative name, or — when the media is one we deliberately
/// never fetched — the URL it lives at. Only the last is something a browser
/// can be pointed at.
fn is_remote(file_path: &str) -> bool {
    file_path.starts_with("http://") || file_path.starts_with("https://")
}

/// `value` as one percent-encoded path segment: every byte but the
/// unreserved ones of RFC 3986 is escaped, `/` and `%` included, so a tag
/// such as `1914/1918` stays one segment.
fn path_segment(value: &str) -> String {
    use std::fmt::Write as _;
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

#[derive(Debug, Serialize)]
struct PortraitImagesRequest {
    person_ids: Vec<Uuid>,
}

/// One portrait as the API sends it: where the picture lives, not the picture.
#[derive(Debug, Deserialize)]
struct WirePortraitImage {
    person_id: Uuid,
    #[serde(flatten)]
    image: WireCroppedSource,
}

/// A picture's address and the region to take out of it, as sent by the API.
/// [`ApiClient::resolve_pictures`] turns it into the drawable
/// [`CroppedSource`] the components take.
type WireCroppedSource = oxidgene_core::types::PortraitRef;

const PORTRAIT_BATCH_SIZE: usize = 1_024;

/// Matches the server's `MAX_IMAGES_PER_REQUEST`.
const IMAGE_DATA_BATCH_SIZE: usize = 1_024;

#[derive(Debug, Serialize)]
struct ImageDataRequest {
    sources: Vec<ImageSource>,
}

/// Matches the server's `MAX_PEDIGREES_PER_REQUEST`.
const PEDIGREE_BATCH_SIZE: usize = 64;

#[derive(Debug, Serialize)]
struct PedigreesRequest {
    root_person_ids: Vec<Uuid>,
    ancestor_depth: u32,
    descendant_depth: u32,
}

#[derive(Debug, Deserialize)]
struct PedigreeEntry {
    root_person_id: Uuid,
    pedigree: Pedigree,
}

fn portrait_batches(person_ids: &[Uuid]) -> impl Iterator<Item = &[Uuid]> {
    person_ids.chunks(PORTRAIT_BATCH_SIZE)
}

#[derive(Debug, Serialize)]
struct GalleryBundleRequest {
    media_ids: Vec<Uuid>,
    vignette_ids: Vec<Uuid>,
}

#[derive(Debug, Serialize)]
struct RelationLabelsRequest {
    person_ids: Vec<Uuid>,
    family_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RelationLabels {
    pub names: Vec<PersonName>,
    pub spouses: Vec<FamilySpouse>,
}

const RELATION_LABEL_BATCH_SIZE: usize = 1_024;

fn relation_label_batch_ranges(
    person_count: usize,
    family_count: usize,
) -> Vec<(std::ops::Range<usize>, std::ops::Range<usize>)> {
    let mut batches = Vec::new();
    let (mut person_offset, mut family_offset) = (0, 0);
    while person_offset < person_count || family_offset < family_count {
        let person_end = (person_offset + RELATION_LABEL_BATCH_SIZE).min(person_count);
        let remaining = RELATION_LABEL_BATCH_SIZE - (person_end - person_offset);
        let family_end = (family_offset + remaining).min(family_count);
        batches.push((person_offset..person_end, family_offset..family_end));
        person_offset = person_end;
        family_offset = family_end;
    }
    batches
}

/// A gallery's pictures, resolved to something drawable.
///
/// The wire form ([`GallerySources`]) carries addresses; the fields here
/// carry whatever this platform draws them from.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GalleryBundle {
    pub media: Vec<GalleryMedia>,
    pub vignettes: Vec<GalleryVignette>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GalleryMedia {
    pub media_id: Uuid,
    pub source: Option<String>,
    pub event_ids: Vec<Uuid>,
    pub document_previews: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GalleryVignette {
    pub vignette_id: Uuid,
    pub image: CroppedSource,
}

/// A gallery's picture addresses, as the API sends them: what a page holds
/// until [`ApiClient::resolve_pictures`] turns them, with the page's
/// portraits, into a [`GalleryBundle`] in one request.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct GallerySources {
    media: Vec<WireGalleryMedia>,
    vignettes: Vec<WireGalleryVignette>,
}

impl GallerySources {
    /// Every address, in the order [`Self::resolve`] takes them back.
    fn sources(&self) -> impl Iterator<Item = ImageSource> + '_ {
        let media = self.media.iter().flat_map(|item| {
            item.source
                .iter()
                .cloned()
                .chain(item.document_previews.iter().cloned())
        });
        let vignettes = self.vignettes.iter().map(|item| item.image.source.clone());
        media.chain(vignettes)
    }

    /// The gallery drawn from `drawn`, one slot per address of
    /// [`Self::sources`], in order.
    fn resolve(&self, drawn: &mut impl Iterator<Item = Option<String>>) -> GalleryBundle {
        let mut bundle = GalleryBundle::default();
        for item in &self.media {
            let source = item.source.as_ref().and_then(|_| drawn.next().flatten());
            let previews = item
                .document_previews
                .iter()
                .filter_map(|_| drawn.next().flatten())
                .collect();
            bundle.media.push(GalleryMedia {
                media_id: item.media_id,
                source,
                event_ids: item.event_ids.clone(),
                document_previews: previews,
            });
        }
        for item in &self.vignettes {
            if let Some(source) = drawn.next().flatten() {
                bundle.vignettes.push(GalleryVignette {
                    vignette_id: item.vignette_id,
                    image: CroppedSource {
                        source,
                        crop: item.image.crop,
                    },
                });
            }
        }
        bundle
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
struct WireGalleryMedia {
    media_id: Uuid,
    source: Option<ImageSource>,
    event_ids: Vec<Uuid>,
    document_previews: Vec<ImageSource>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
struct WireGalleryVignette {
    vignette_id: Uuid,
    #[serde(flatten)]
    image: WireCroppedSource,
}

/// A picture to draw, and the region of it to show.
///
/// One value rather than two loose fields, because they are only ever read
/// together: `crop` is set exactly when `source` is a whole picture the server
/// could not cut — a region of a file we do not hold and never fetch — and
/// absent for every image that arrives already cut.
/// A picture ready to draw, and the region of it to show.
///
/// `source` is whatever this platform puts in an `src`: a path on the
/// application's own origin, an address we do not own, or a `data:` URL. It is
/// never a backend address — see `image_host`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CroppedSource {
    pub source: String,
    pub crop: Option<ImageCrop>,
}

impl CroppedSource {
    /// A picture to be shown whole.
    pub fn whole(source: String) -> Self {
        Self { source, crop: None }
    }

    /// The default silhouette for someone with no portrait.
    ///
    /// One place rather than five, so the fallback cannot drift between the
    /// search list, the search grid, the pedigree cards and the profile header.
    ///
    /// Served by the shell where there is one, so a page full of cards carries
    /// one reference to the picture rather than a few kilobytes of it per card.
    /// `try_consume_context` and not `use_context`: this is called from render
    /// bodies and helper functions alike, and must not be a hook.
    pub fn silhouette(sex: oxidgene_core::Sex) -> Self {
        let hosted = dioxus::prelude::try_consume_context::<crate::image_host::ImageHost>()
            .and_then(|host| host.silhouette_path(sex));
        Self::whole(hosted.unwrap_or_else(|| {
            crate::components::pedigree_chart::default_portrait(sex).to_string()
        }))
    }
}

/// A media together with the link that attached it — one gallery tile.
///
/// Mirrors `MediaWithLink` on the API side, which flattens the media, so the
/// media's own fields sit at the top level here too.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct MediaWithLink {
    pub link_id: uuid::Uuid,
    pub sort_order: i32,
    #[serde(flatten)]
    pub media: Media,
}

#[derive(Debug, Deserialize)]
struct MediaDeletionStatus {
    can_delete: bool,
}

/// A document of the tree-wide media list, with how many records it is
/// attached to. Mirrors the API's `MediaListItem`, media flattened in.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct MediaListItem {
    #[serde(flatten)]
    pub media: Media,
    pub usage_count: i64,
}

/// The media list's filters. Every field is optional; the set ones combine
/// with AND on the server.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MediaListFilters {
    /// A document must carry every one of these tags.
    pub tags: Vec<String>,
    pub kind: Option<oxidgene_core::MediaFileKind>,
    pub category: Option<DocumentCategory>,
    pub name: Option<String>,
    pub linked_name: Option<String>,
    pub event_from: Option<i32>,
    pub event_to: Option<i32>,
    pub added_from: Option<chrono::NaiveDate>,
    pub added_to: Option<chrono::NaiveDate>,
}

impl MediaListFilters {
    fn query_pairs(&self) -> Vec<(&'static str, String)> {
        let mut pairs = Vec::new();
        let mut push = |key, value: Option<String>| {
            if let Some(value) = value {
                pairs.push((key, value));
            }
        };
        for tag in &self.tags {
            push("tag", Some(tag.clone()));
        }
        push("kind", self.kind.map(|kind| kind.as_str().to_string()));
        push(
            "category",
            self.category.map(|category| category.as_str().to_string()),
        );
        push("name", self.name.clone());
        push("linked_name", self.linked_name.clone());
        push("event_from", self.event_from.map(|year| year.to_string()));
        push("event_to", self.event_to.map(|year| year.to_string()));
        push("added_from", self.added_from.map(|day| day.to_string()));
        push("added_to", self.added_to.map(|day| day.to_string()));
        pairs
    }
}

/// A tag of the tree and how many documents carry it.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct MediaTagFacet {
    pub tag: String,
    pub count: i64,
}

/// A file kind and how many documents hold a page of it.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct MediaKindFacet {
    pub kind: oxidgene_core::MediaFileKind,
    pub count: i64,
}

/// A document category and how many documents are filed under it.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct MediaCategoryFacet {
    pub category: DocumentCategory,
    pub count: i64,
}

/// The values the media list's filters can take in a tree.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct MediaFacets {
    pub tags: Vec<MediaTagFacet>,
    pub kinds: Vec<MediaKindFacet>,
    pub categories: Vec<MediaCategoryFacet>,
}

/// Where a media's bytes actually are.
///
/// Three states, and every view has to tell them apart. A media OxidGene holds
/// is served by us, has a thumbnail and can be cropped. A remote one is a URL
/// someone else serves — worth recording, never fetched by us, and therefore
/// without a thumbnail or a crop. A record naming a file nobody ever uploaded
/// has no bytes at all, which is where every GEDCOM import starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaSource {
    /// The bytes are in our store.
    Stored,
    /// `file_path` is an http(s) URL, served by whoever owns it.
    Remote,
    /// A path we were told about and never received.
    Unheld,
}

/// How a media should be presented when there is room to show it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Image,
    Video,
    Audio,
    Pdf,
    Document,
    Other,
}

impl MediaKind {
    /// The glyph a tile draws when there is no picture to draw instead.
    pub fn icon(self) -> &'static str {
        match self {
            Self::Image => "\u{1F5BC}",
            Self::Video => "\u{1F3AC}",
            Self::Audio => "\u{1F3B5}",
            Self::Pdf => "\u{1F4C4}",
            Self::Document => "\u{1F4C3}",
            Self::Other => "\u{1F4C1}",
        }
    }
}

/// Which of the three states a media row is in.
///
/// Takes the row rather than the tile: the viewer asks this of the page it is
/// showing, which is where the bytes and the URL actually are — a document is
/// always `Unheld`, and answering that about the register somebody is reading
/// would be true of the shell and wrong about the file.
pub fn media_source(media: &Media) -> MediaSource {
    if media.storage_key.is_some() {
        MediaSource::Stored
    } else if is_remote(&media.file_path) {
        MediaSource::Remote
    } else {
        MediaSource::Unheld
    }
}

impl MediaWithLink {
    /// Which of the three states this media is in.
    pub fn source(&self) -> MediaSource {
        media_source(&self.media)
    }

    /// How to present it.
    ///
    /// Reads `mime_type` and trusts it: every write path normalises it, so a
    /// second opinion here would only be a second place for the rule to live.
    pub fn kind(&self) -> MediaKind {
        media_kind(&self.media.mime_type)
    }

    /// Whether this tile can be shown as a picture rather than a file icon.
    pub fn is_image(&self) -> bool {
        self.kind() == MediaKind::Image
    }

    /// Whether a crop can be drawn on it.
    ///
    /// Only a stored raster: a crop is served by re-decoding our own copy, so
    /// a remote URL has nothing to cut, and a record with no bytes has nothing
    /// at all.
    pub fn is_croppable(&self) -> bool {
        self.source() == MediaSource::Stored
            && self.is_image()
            && self.media.width.is_some()
            && self.media.height.is_some()
    }

    /// A short badge for the file type — "PDF", "JPEG", "MP4".
    pub fn kind_label(&self) -> String {
        media_kind_label(&self.media.mime_type)
    }

    /// What to write under a tile: the title if there is one, else the file name.
    pub fn caption(&self) -> &str {
        match self.media.title.as_deref() {
            Some(title) if !title.trim().is_empty() => title,
            _ => &self.media.file_name,
        }
    }
}

/// A short badge for a MIME type — "PDF", "JPEG", "MP4".
///
/// A document's own MIME type is an internal marker, not a format anybody
/// recognises: spelled out it reads "OXIDGENE-DOCUMENT", which tells a reader
/// nothing and leaks a private name into the interface.
pub fn media_kind_label(mime_type: &str) -> String {
    if mime_type.trim().eq_ignore_ascii_case(DOCUMENT_MIME) {
        return "DOCUMENT".to_string();
    }
    mime_type
        .rsplit('/')
        .next()
        .unwrap_or("file")
        .trim_start_matches("x-")
        .split('+')
        .next()
        .unwrap_or("file")
        .to_uppercase()
}

/// Classify a MIME type into what the UI can do with it.
pub fn media_kind(mime_type: &str) -> MediaKind {
    let mime = mime_type.trim().to_ascii_lowercase();
    if mime == DOCUMENT_MIME {
        // A document holds no bytes of its own — what can be drawn is its
        // pages. Reading its marker as a generic file would land it in
        // `Other`, which is why an imported photograph drew a folder.
        MediaKind::Document
    } else if mime.starts_with("image/") {
        MediaKind::Image
    } else if mime.starts_with("video/") {
        MediaKind::Video
    } else if mime.starts_with("audio/") {
        MediaKind::Audio
    } else if mime == "application/pdf" {
        MediaKind::Pdf
    } else if mime.starts_with("text/")
        || mime.contains("word")
        || mime.contains("opendocument")
        || mime.contains("officedocument")
    {
        MediaKind::Document
    } else {
        MediaKind::Other
    }
}

#[derive(Debug, Serialize)]
pub struct CreateMediaLinkBody {
    pub media_id: uuid::Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub person_id: Option<uuid::Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_id: Option<uuid::Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_id: Option<uuid::Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub family_id: Option<uuid::Uuid>,
    #[serde(default)]
    pub sort_order: i32,
}

impl CreateMediaLinkBody {
    /// A link attaching `media_id` to one event, first in its order.
    pub fn to_event(media_id: uuid::Uuid, event_id: uuid::Uuid) -> Self {
        Self {
            media_id,
            person_id: None,
            event_id: Some(event_id),
            source_id: None,
            family_id: None,
            sort_order: 0,
        }
    }

    /// A link attaching `media_id` to one source, first in its order.
    pub fn to_source(media_id: uuid::Uuid, source_id: uuid::Uuid) -> Self {
        Self {
            media_id,
            person_id: None,
            event_id: None,
            source_id: Some(source_id),
            family_id: None,
            sort_order: 0,
        }
    }
}

/// A page whose bytes somebody else serves.
///
/// The counterpart to [`MediaUpload`]: same destination — the next page of a
/// document — but the address travels instead of the content. Nothing here is
/// ever fetched; the address is recorded and handed to the browser when the
/// page is drawn.
#[derive(Debug, Clone, Serialize)]
pub struct CreateMediaBody {
    /// The document this becomes a page of. A page always belongs to one.
    pub document_id: Uuid,
    pub file_name: String,
    pub mime_type: String,
    /// The http(s) address the file lives at.
    pub file_path: String,
    /// Unknown until somebody fetches it, which is the point of not doing so.
    pub file_size: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// A small picture of the page its server also serves, which gallery
    /// tiles draw instead of the full one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumbnail_url: Option<String>,
    /// The picture's pixel size, when its server states it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<i32>,
}

/// One file on its way up, and what it should become on arrival.
///
/// A struct rather than six positional arguments: three of them are
/// `Option<Uuid>`, and a call site that reads `(None, None, Some(id))` tells
/// nobody which of "attach to this record" and "make it a page of this
/// document" was meant.
#[derive(Debug, Clone)]
pub struct MediaUpload {
    pub file_name: String,
    pub bytes: Vec<u8>,
    pub title: Option<String>,
    pub description: Option<String>,
    /// Fill in an existing record that named a file without holding it.
    pub attach_to: Option<Uuid>,
    /// Append as the next page of this multi-page document.
    pub as_page_of: Option<Uuid>,
}

#[derive(Debug, Serialize, Default)]
pub struct SetPortraitBody {
    /// A whole media.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_id: Option<uuid::Uuid>,
    /// A region of one — a face in a group photograph.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vignette_id: Option<uuid::Uuid>,
}

/// A media carries the same descriptive fields a fact does — and no source
/// field, because a media *is* a source document.
#[derive(Debug, Default, Serialize)]
pub struct UpdateMediaBody {
    /// `Some(None)` clears the field, absent leaves it alone.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date_value: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date_value2: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date_qualifier: Option<DateQualifier>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub calendar: Option<Calendar>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub place_id: Option<Option<uuid::Uuid>>,
    /// The URL of a remote media. The server refuses it for a media it stores.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    /// The picture's pixel size, sent together or not at all. Accepted only
    /// for a page we do not hold: nothing here ever opened that file, so the
    /// browser that displayed it is the only witness to how big it is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<i32>,
    /// Whether this is shown when the tree is published.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub privacy: Option<Privacy>,
    /// What the medium physically is, in GEDCOM's own vocabulary.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_media_type: Option<SourceMediaType>,
    /// What kind of record it is. Sending it without a `source_media_type`
    /// also sets the medium it implies, so a census return does not export as
    /// `OTHER`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document_category: Option<Option<DocumentCategory>>,
}

#[derive(Debug, Serialize)]
pub struct MediaTagBody {
    pub tag: String,
}

// ── Vignette DTOs ────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct CreateVignetteBody {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub person_id: Option<uuid::Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_id: Option<uuid::Uuid>,
}

/// The four rectangle fields travel together — send all or none.
#[derive(Debug, Default, Serialize)]
pub struct UpdateVignetteBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub y: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub person_id: Option<Option<uuid::Uuid>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_id: Option<Option<uuid::Uuid>>,
}

// ── Geneanet import wizard ──────────────────────────────────────────
//
// Mirrors `oxidgene_api::rest::dto`. Step 3 has no type here: signing in and
// collecting the person↔photo mapping happens in the desktop login window, and
// what it produces is carried by the steps that follow.

/// What a `.gw` file turned out to hold. Step 1.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct GwInspection {
    pub person_count: usize,
    pub family_count: usize,
    /// Blocks the lenient reader skipped — reported, never fatal.
    pub skipped_blocks: usize,
}

#[derive(Debug, Serialize)]
pub struct IndexArchivesBody {
    pub paths: Vec<String>,
}

/// One data archive's central directory, read without extracting anything.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct IndexedArchive {
    pub path: String,
    pub file_name: String,
    pub file_count: usize,
    pub image_count: usize,
    /// Set when this archive alone could not be read; the others still stand.
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ArchiveIndex {
    pub archives: Vec<IndexedArchive>,
    pub file_count: usize,
}

/// Which bytes a Geneanet import keeps for each medium.
///
/// `Renditions` is the default and needs nothing from the user but their
/// login: every page is stored as Geneanet's own `normal` variant, recompressed
/// and resized. `Originals` keeps the uploaded files, which means the data
/// archives — a separate Geneanet request and several gigabytes of ZIP — plus a
/// byte-length pass to match them on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaFidelity {
    #[default]
    Renditions,
    Originals,
}

impl MediaFidelity {
    /// Whether this import consults the user's data archives at all.
    #[must_use]
    pub const fn uses_archives(self) -> bool {
        matches!(self, Self::Originals)
    }
}

#[derive(Debug, Serialize)]
pub struct GeneanetPreviewBody {
    /// The `.gw`, base64-encoded: JSON cannot carry the raw bytes the
    /// ISO-8859-1-or-UTF-8 reader needs, and this body carries other fields
    /// alongside it.
    pub gw_base64: String,
    pub file_name: String,
    pub collection: String,
    pub deposit_sizes: std::collections::HashMap<i64, u64>,
    pub archive_paths: Vec<String>,
    pub media_fidelity: MediaFidelity,
}

/// A step-3 session, encoded for the file the wizard saves.
#[derive(Debug, Serialize)]
pub struct GeneanetSessionBody {
    pub collection: String,
    pub deposit_sizes: std::collections::HashMap<i64, u64>,
    pub account: Option<String>,
    /// Media already fetched. Saving after step 4 includes them, which is what
    /// makes the file importable with no connection.
    pub media: std::collections::HashMap<String, String>,
}

/// What a saved session held.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct GeneanetSession {
    pub collection: String,
    pub deposit_sizes: std::collections::HashMap<i64, u64>,
    pub account: Option<String>,
    /// Media the collection covers, pages included.
    pub photo_count: usize,
    /// Media the file carried. Empty means the wizard must still gather them.
    pub media: std::collections::HashMap<String, String>,
}

/// One medium the server cannot produce on its own.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct NeededMedia {
    pub deposit_id: i64,
    pub view_id: i64,
    pub page: Option<i64>,
    /// Where the login window should fetch it from.
    pub url: String,
    /// `true` for a deposit's exact original, `false` for a page rendition.
    pub original: bool,
}

/// What the login window has to fetch before an import can run.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct GeneanetPlan {
    pub needed: Vec<NeededMedia>,
}

/// The stat row and the explanatory lines of step 4.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct GeneanetPreview {
    pub person_count: usize,
    pub photo_count: usize,
    pub persons_with_photo: usize,
    pub attachment_count: usize,
    pub in_archives: usize,
    /// Document pages recognised in the archives by content rather than size.
    pub to_match: usize,
    pub to_download: usize,
    pub group_photos: usize,
    pub unlinked_views: usize,
    /// Multi-page deposits imported as documents.
    pub documents: usize,
    /// Pages those documents hold — all of them are imported.
    pub document_pages: usize,
    pub unlinked_names: usize,
    pub outside_tree: usize,
    pub ambiguous: usize,
    pub unlinked_names_sample: Vec<String>,
    pub outside_tree_names: Vec<String>,
    pub ambiguous_names: Vec<String>,
    /// `true` when almost no photo matched — the wizard blocks rather than
    /// importing a tree whose photos belong to a different one.
    pub mismatch: bool,
}

#[derive(Debug, Serialize)]
pub struct GeneanetImportBody {
    pub gw_base64: String,
    pub file_name: String,
    pub collection: String,
    pub deposit_sizes: std::collections::HashMap<i64, u64>,
    pub archive_paths: Vec<String>,
    /// Media the login window fetched, keyed by URL — **paths**, not bytes.
    ///
    /// The server never fetches anything itself: no direct request to Geneanet
    /// succeeds. The window writes each medium to a temp directory and this
    /// names them, which keeps the request small however many there are.
    pub fetched: std::collections::HashMap<String, String>,
    pub media_fidelity: MediaFidelity,
}

/// How far a running import has got.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ImportProgress {
    pub phase: String,
    pub done: usize,
    pub total: usize,
}

/// What the Geneanet import actually did: what every import reports, and
/// what only it does.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct GeneanetImportResult {
    #[serde(flatten)]
    pub receipt: ImportResult,
    /// Higher than the media records when a photo shows several people.
    pub links_count: usize,
    /// Links marked as a person's profile photo.
    pub portraits_count: usize,
    /// People created for identifications Geneanet marks "hors de l'arbre".
    pub isolated_count: usize,
    /// Those people, so the receipt can ask about the ones with homonyms.
    #[serde(default)]
    pub isolated_people: Vec<GeneanetIsolatedPerson>,
    /// Identification boxes kept as regions on the stored pictures.
    pub vignettes_count: usize,
    pub skipped: Vec<String>,
}

/// A person the Geneanet import created for an identification outside the
/// tree.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct GeneanetIsolatedPerson {
    pub person_id: Uuid,
    pub surname: String,
    pub given_names: String,
}

/// Summary returned by any import, whatever the source format.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ImportResult {
    pub persons_count: usize,
    pub families_count: usize,
    pub events_count: usize,
    pub sources_count: usize,
    /// Media records of a single page: photographs, single scans.
    pub images_count: usize,
    /// Media records of any other number of pages.
    pub documents_count: usize,
    /// The pages of those documents.
    pub document_pages_count: usize,
    pub places_count: usize,
    pub notes_count: usize,
    pub warnings: Vec<String>,
}

/// Pollable state of an asynchronous genealogy file import.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct FileImportJobStatus {
    pub phase: String,
    pub done: usize,
    pub total: usize,
    pub result: Option<ImportResult>,
    pub geneanet_result: Option<GeneanetImportResult>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ImportJobStarted {
    pub job_id: Uuid,
}

/// What an export holds and how it writes it, sent as its query options.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ExportChoices {
    /// Collapse each person's occupations into one `OCCU`, comma-separated,
    /// for importers (e.g. Geneanet) that read a single profession field.
    pub merge_occupations: bool,
    /// Collapse each person's other names into the primary name's `SURN`,
    /// for importers (e.g. Geneanet) that read the first `NAME` only.
    pub merge_names: bool,
    /// Write the notes, and the sources with their citations and
    /// repositories.
    pub include_notes_and_sources: bool,
    /// Pack the media of a GEDZIP; a plain GEDCOM ignores it.
    pub include_media: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ExportJobStarted {
    pub job_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ExportJobStatus {
    pub phase: String,
    pub done: usize,
    pub total: usize,
    pub download_url: Option<String>,
    /// When `download_url` stops working, set exactly when it is.
    #[serde(default)]
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
    /// The archive's size in bytes, once it is complete.
    #[serde(default)]
    pub size_bytes: Option<u64>,
    #[serde(default)]
    pub warnings: Vec<String>,
    pub error: Option<String>,
}

/// A completed export of a tree whose archive can still be downloaded.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct DownloadableExport {
    /// The archive's format, `gedzip`.
    pub format: String,
    pub download_url: String,
    /// When `download_url` stops working.
    pub expires_at: chrono::DateTime<chrono::Utc>,
    /// The archive's size in bytes, when the export recorded it.
    #[serde(default)]
    pub size_bytes: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExportGedcomResult {
    pub gedcom: String,
    pub warnings: Vec<String>,
}

/// Everything one person page renders. Its pictures are addresses: the page
/// draws its text at once and resolves them, with the person's portrait, in
/// one request of its own ([`ApiClient::resolve_pictures`]).
#[derive(Debug, Clone, Deserialize)]
pub struct PersonDetailBundle {
    pub sosa_number: Option<u64>,
    pub persons: Vec<oxidgene_core::types::Person>,
    pub names: Vec<oxidgene_core::types::PersonName>,
    pub events: Vec<oxidgene_core::types::Event>,
    pub places: Vec<oxidgene_core::types::Place>,
    pub spouses: Vec<oxidgene_core::types::FamilySpouse>,
    pub children: Vec<oxidgene_core::types::FamilyChild>,
    pub citations: Vec<oxidgene_core::types::Citation>,
    pub sources: Vec<oxidgene_core::types::Source>,
    pub profile_media: Vec<ProfileMediaTile>,
    pub profile_vignettes: Vec<Vignette>,
    pub event_media: Vec<EventMediaTile>,
    pub gallery: GallerySources,
    /// Where the person's own portrait is drawn from.
    #[serde(default)]
    pub portrait: Option<oxidgene_core::types::PortraitRef>,
    /// Those of `persons` who are the tree's SOSA root or one of its
    /// ancestors.
    #[serde(default)]
    pub sosa_ancestor_ids: Vec<Uuid>,
}

/// Everything one couple page renders: the family, each spouse's person
/// bundle, the family's and spouses' notes, and the family's own media.
#[derive(Debug, Clone, Deserialize)]
pub struct CoupleDetailBundle {
    pub family: Family,
    pub spouses: Vec<FamilySpouse>,
    /// One per spouse, in the order of `spouses`.
    pub persons: Vec<PersonDetailBundle>,
    pub notes: Vec<oxidgene_core::types::Note>,
    pub media: Vec<MediaWithLink>,
    pub gallery: GallerySources,
}

/// A screen's pictures, resolved in one request: its galleries as one — a
/// gallery looks its pictures up by media and vignette — and the portraits.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ResolvedPictures {
    /// Shared rather than owned: it carries every thumbnail as a base64 data
    /// URI, and every gallery on the page reads the same one.
    pub gallery: std::sync::Arc<GalleryBundle>,
    pub portraits: HashMap<Uuid, CroppedSource>,
}

/// A media a person's profile shows, and the couple it reaches it through.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ProfileMediaTile {
    /// The conjugal family the media is attached to, or `None` when it is
    /// attached to the person directly.
    #[serde(default)]
    pub family_id: Option<Uuid>,
    #[serde(flatten)]
    pub tile: MediaWithLink,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct EventMediaTile {
    pub event_id: Uuid,
    pub link_id: Uuid,
    pub sort_order: i32,
    #[serde(flatten)]
    pub media: Media,
}

// ── Response Cache ───────────────────────────────────────────────────

const CACHE_TTL_SECS: i64 = 30;

/// In-memory GET response cache with a fixed TTL.
///
/// Keyed by the request URL (path + serialised query string).
/// Values are raw JSON bytes + the Unix timestamp when they were stored.
type CacheInner =
    std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, (Vec<u8>, i64)>>>;

/// One in-flight request per cache key. Losers of the race wait on the gate
/// rather than issuing the same request again.
///
/// `futures_util`'s mutex rather than tokio's: this crate compiles to WASM,
/// where tokio is not a dependency at all.
type Gate = std::sync::Arc<futures_util::lock::Mutex<()>>;
type GateInner = std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, Gate>>>;

/// The most responses the cache holds. Inserting past it drops the oldest.
const CACHE_MAX_ENTRIES: usize = 256;

/// How many times the cache was invalidated: once per tree for the
/// invalidations scoped to one, and overall for the others.
///
/// A response is stored only if no invalidation covering it happened while
/// it was in flight: a read sent before a write and answered after it may
/// predate the write, and storing it would serve the old data for a TTL.
#[derive(Default)]
struct Generations {
    all: u64,
    trees: std::collections::HashMap<Uuid, u64>,
}

/// The generation a response under `key` must still match to be stored.
type Generation = (u64, u64);

#[derive(Clone, Default)]
struct ResponseCache {
    entries: CacheInner,
    gates: GateInner,
    generations: std::sync::Arc<std::sync::Mutex<Generations>>,
}

/// The tree a request path is about: the id after `/api/v1/trees/`, or
/// `None` for a path outside any one tree.
fn tree_of_path(path: &str) -> Option<Uuid> {
    let rest = path.strip_prefix("/api/v1/trees/")?;
    let id = rest.split(['/', '?']).next()?;
    Uuid::parse_str(id).ok()
}

impl std::fmt::Debug for ResponseCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ResponseCache({})",
            self.entries.lock().map(|c| c.len()).unwrap_or(0)
        )
    }
}

impl ResponseCache {
    fn get(&self, key: &str) -> Option<Vec<u8>> {
        let cache = self.entries.lock().ok()?;
        let (data, ts) = cache.get(key)?;
        let age = chrono::Utc::now().timestamp() - ts;
        if age < CACHE_TTL_SECS {
            Some(data.clone())
        } else {
            None
        }
    }

    /// Stores `data` under `key`, first dropping the expired entries and,
    /// at the size limit, the oldest ones.
    fn set(&self, key: String, data: Vec<u8>) {
        let Ok(mut cache) = self.entries.lock() else {
            return;
        };
        let now = chrono::Utc::now().timestamp();
        cache.retain(|_, (_, stored)| now - *stored < CACHE_TTL_SECS);
        cache.remove(&key);
        while cache.len() >= CACHE_MAX_ENTRIES {
            let Some(oldest) = cache
                .iter()
                .min_by_key(|(_, (_, stored))| *stored)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            cache.remove(&oldest);
        }
        cache.insert(key, (data, now));
    }

    /// The invalidation count a response for `key` is fetched under.
    fn generation(&self, key: &str) -> Generation {
        let Ok(generations) = self.generations.lock() else {
            return (u64::MAX, u64::MAX);
        };
        let tree = tree_of_path(key)
            .and_then(|tree| generations.trees.get(&tree).copied())
            .unwrap_or_default();
        (generations.all, tree)
    }

    /// Stores `data` under `key` unless the cache was invalidated for it
    /// since `generation` was read.
    fn set_if_current(&self, key: String, data: Vec<u8>, generation: Generation) {
        if self.generation(&key) == generation {
            self.set(key, data);
        }
    }

    /// Remove all entries whose key starts with `prefix`.
    fn invalidate_prefix(&self, prefix: &str) {
        if let Ok(mut generations) = self.generations.lock() {
            match tree_of_path(prefix) {
                Some(tree) => *generations.trees.entry(tree).or_default() += 1,
                None => generations.all += 1,
            }
        }
        if let Ok(mut cache) = self.entries.lock() {
            cache.retain(|k, _| !k.starts_with(prefix));
        }
    }

    /// The gate guarding network access for `key`, created on first use.
    fn gate(&self, key: &str) -> Gate {
        let Ok(mut gates) = self.gates.lock() else {
            // A poisoned gate map costs a duplicate request, never a wrong
            // answer: fall back to an ungated lock nobody else holds.
            return std::sync::Arc::default();
        };
        gates.entry(key.to_string()).or_default().clone()
    }

    /// Drops the gate for `key` once nothing is waiting on it. Callers must
    /// have released their own handle first, so a remaining reference means
    /// another request is still queued behind this key.
    fn release_gate(&self, key: &str) {
        if let Ok(mut gates) = self.gates.lock()
            && gates
                .get(key)
                .is_some_and(|gate| std::sync::Arc::strong_count(gate) == 1)
        {
            gates.remove(key);
        }
    }
}

/// Where the batch of several trees' recent persons is read from.
const RECENT_PERSONS_PATH: &str = "/api/v1/trees/recent-persons";

// ── Picture cache ───────────────────────────────────────────────────

/// How many bytes of picture data the session keeps.
const PICTURE_CACHE_MAX_BYTES: usize = 24 * 1024 * 1024;

/// How long a kept picture is used without asking again, in seconds: long
/// enough for going back and forth between pages, short enough that a change
/// made elsewhere — another tab, another person — shows within minutes.
const PICTURE_CACHE_TTL_SECS: i64 = 10 * 60;

/// The pictures the web client fetched as `data:` URLs, kept for the session
/// and keyed by the address they were fetched for, so a page visited again
/// draws them without downloading them again.
///
/// The response cache cannot do this: pictures are read with `POST`, never
/// cached there. Bounded in bytes, oldest dropped first; a tree's are dropped
/// whenever this client writes to it, as its cached reads are.
#[derive(Clone, Default)]
struct PictureCache(std::sync::Arc<std::sync::Mutex<PictureEntries>>);

#[derive(Default)]
struct PictureEntries {
    entries: HashMap<(Uuid, ImageSource), (String, i64)>,
    bytes: usize,
}

impl std::fmt::Debug for PictureCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let count = self.0.lock().map(|e| e.entries.len()).unwrap_or(0);
        write!(f, "PictureCache({count})")
    }
}

impl PictureCache {
    fn get(&self, tree_id: Uuid, source: &ImageSource) -> Option<String> {
        let entries = self.0.lock().ok()?;
        let (data, stored) = entries.entries.get(&(tree_id, source.clone()))?;
        (chrono::Utc::now().timestamp() - stored < PICTURE_CACHE_TTL_SECS).then(|| data.clone())
    }

    fn set(&self, tree_id: Uuid, source: ImageSource, data: String) {
        let Ok(mut entries) = self.0.lock() else {
            return;
        };
        if data.len() > PICTURE_CACHE_MAX_BYTES / 4 {
            return;
        }
        let now = chrono::Utc::now().timestamp();
        if let Some((old, _)) = entries.entries.remove(&(tree_id, source.clone())) {
            entries.bytes -= old.len();
        }
        while entries.bytes + data.len() > PICTURE_CACHE_MAX_BYTES {
            let Some(oldest) = entries
                .entries
                .iter()
                .min_by_key(|(_, (_, stored))| *stored)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            if let Some((old, _)) = entries.entries.remove(&oldest) {
                entries.bytes -= old.len();
            }
        }
        entries.bytes += data.len();
        entries.entries.insert((tree_id, source), (data, now));
    }

    fn invalidate_tree(&self, tree_id: Uuid) {
        if let Ok(mut entries) = self.0.lock() {
            let PictureEntries { entries, bytes } = &mut *entries;
            entries.retain(|(tree, _), (data, _)| {
                let keep = *tree != tree_id;
                if !keep {
                    *bytes -= data.len();
                }
                keep
            });
        }
    }
}

// ── API Client ──────────────────────────────────────────────────────

/// Typed HTTP client for the OxidGene REST API.
#[derive(Debug, Clone)]
pub struct ApiClient {
    client: reqwest::Client,
    base_url: String,
    cache: ResponseCache,
    /// The pictures fetched as `data:` URLs this session (see [`PictureCache`]).
    pictures: PictureCache,
    /// The shell that serves backend-held pictures from its own origin, when
    /// this build has one. Absent on the web, where pictures are fetched here
    /// and handed to the markup as `data:` URLs instead.
    image_host: Option<crate::image_host::ImageHost>,
    /// The `Authorization` value the desktop's embedded backend requires.
    /// Sent to that backend only — never to a remote address a download names.
    auth: Option<reqwest::header::HeaderValue>,
}

/// Errors returned by the API client.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("API error ({status}): {body}")]
    Api { status: u16, body: String },
}

impl ApiError {
    /// What a view reports when the tree id in its route does not parse.
    pub fn invalid_tree_id(i18n: &crate::i18n::I18n) -> Self {
        Self::Api {
            status: 400,
            body: i18n.t("common.invalid_tree_id"),
        }
    }

    /// What a view reports when one of the ids in its route does not parse.
    pub fn invalid_ids(i18n: &crate::i18n::I18n) -> Self {
        Self::Api {
            status: 400,
            body: i18n.t("common.invalid_ids"),
        }
    }

    /// A stable category for logs and spans.
    ///
    /// The error's message is no such thing: a transport error names the URL
    /// it failed on and an API error carries the server's answer, either of
    /// which can hold identifiers or searched values.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Http(_) => "http",
            Self::Json(_) => "json",
            Self::Io(_) => "io",
            Self::Api { .. } => "api",
        }
    }

    /// The stable code of the server's error envelope (`not_found`,
    /// `timeout`…), when the server answered with one.
    pub fn code(&self) -> Option<String> {
        let Self::Api { body, .. } = self else {
            return None;
        };
        let envelope: serde_json::Value = serde_json::from_str(body).ok()?;
        envelope["error"].as_str().map(str::to_owned)
    }

    /// The HTTP status the request ended with, when it got one.
    pub fn status(&self) -> Option<u16> {
        match self {
            Self::Http(error) => error.status().map(|status| status.as_u16()),
            Self::Api { status, .. } => Some(*status),
            Self::Json(_) | Self::Io(_) => None,
        }
    }
}

/// Starts the browser save picker during the click, before any network awaits.
#[cfg(target_arch = "wasm32")]
pub(crate) struct BrowserDownload {
    eval: dioxus::document::Eval,
}

#[cfg(target_arch = "wasm32")]
impl BrowserDownload {
    pub fn new(file_name: &str) -> Self {
        let name = serde_json::to_string(file_name).expect("a string is serializable");
        Self {
            eval: dioxus::document::eval(&format!(
                "const fileName = {name};\n{}",
                include_str!("download.js")
            )),
        }
    }

    pub async fn ready(&mut self) -> Result<bool, ApiError> {
        match self.eval.recv::<String>().await.as_deref() {
            Ok("ready") => Ok(true),
            Ok("cancelled") => Ok(false),
            _ => Err(Self::error()),
        }
    }

    fn error() -> ApiError {
        std::io::Error::other("browser download failed").into()
    }
}

#[cfg(target_arch = "wasm32")]
impl Drop for BrowserDownload {
    fn drop(&mut self) {
        // Release a pending picker session if export preparation fails.
        let _ = self.eval.send(Option::<String>::None);
    }
}

/// The route a request is logged and traced under: its path without the
/// query, which can carry searched names, and with every identifier — a UUID
/// or a number — replaced by `{id}`.
///
/// Spans name the endpoint so a trace says which request it waited on, but
/// an identifier is genealogy rather than routing metadata, and would also
/// give every person a span name of their own. The paths this client builds
/// hold no other values: the rest are fixed segments and language codes.
fn route_template(path: &str) -> String {
    path.split('?')
        .next()
        .unwrap_or(path)
        .split('/')
        .map(|segment| {
            let is_number = !segment.is_empty() && segment.bytes().all(|b| b.is_ascii_digit());
            if is_number || Uuid::parse_str(segment).is_ok() {
                "{id}"
            } else {
                segment
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Takes `gate`, inside a span when another request for `cache_key` holds
/// it: that wait is where a coalesced request spends its time.
async fn wait_for_gate<'a>(
    gate: &'a futures_util::lock::Mutex<()>,
    cache_key: &str,
) -> futures_util::lock::MutexGuard<'a, ()> {
    if let Some(guard) = gate.try_lock() {
        return guard;
    }
    #[cfg(feature = "telemetry-client")]
    {
        let route = route_template(cache_key);
        let span = tracing::info_span!(
            "ui.request.wait",
            otel.name = %format!("wait for GET {route}"),
            http.route = %route,
        );
        gate.lock().instrument(span).await
    }
    #[cfg(not(feature = "telemetry-client"))]
    {
        let _ = cache_key;
        gate.lock().await
    }
}

impl ApiClient {
    async fn read_response_body(response: reqwest::Response) -> Result<Vec<u8>, reqwest::Error> {
        #[cfg(feature = "telemetry-client")]
        {
            let status = response.status().as_u16();
            let expected_size = response.content_length();
            let span = tracing::info_span!(
                "ui.response.read",
                otel.name = "read HTTP response body",
                http.response.status_code = status,
                http.response.body.size = tracing::field::Empty,
                http.response.body.size.expected = expected_size,
                otel.status_code = tracing::field::Empty,
            );
            let result = response.bytes().instrument(span.clone()).await;
            match &result {
                Ok(bytes) => {
                    span.record("http.response.body.size", bytes.len());
                }
                Err(_) => {
                    span.record("otel.status_code", "ERROR");
                }
            }
            result.map(|bytes| bytes.to_vec())
        }

        #[cfg(not(feature = "telemetry-client"))]
        {
            response.bytes().await.map(|bytes| bytes.to_vec())
        }
    }

    fn deserialize<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, serde_json::Error> {
        #[cfg(feature = "telemetry-client")]
        {
            let span = tracing::info_span!(
                "ui.response.deserialize",
                otel.name = "deserialize JSON response",
                serialization.format = "json",
                response.body.size = bytes.len(),
                otel.status_code = tracing::field::Empty,
            );
            let result = span.in_scope(|| serde_json::from_slice(bytes));
            if result.is_err() {
                span.record("otel.status_code", "ERROR");
            }
            result
        }

        #[cfg(not(feature = "telemetry-client"))]
        {
            serde_json::from_slice(bytes)
        }
    }

    /// Create a new API client pointing at the given base URL.
    ///
    /// The `base_url` should include scheme and port, e.g.
    /// `http://127.0.0.1:3000`.
    pub fn new(base_url: &str) -> Self {
        let builder = reqwest::Client::builder();
        #[cfg(not(target_arch = "wasm32"))]
        let builder = builder.timeout(std::time::Duration::from_secs(300));
        Self {
            client: builder.build().expect("failed to build reqwest client"),
            base_url: base_url.trim_end_matches('/').to_string(),
            cache: ResponseCache::default(),
            pictures: PictureCache::default(),
            image_host: None,
            auth: None,
        }
    }

    /// Present `token` as a bearer credential on every request to the backend.
    ///
    /// # Panics
    ///
    /// If `token` cannot appear in an HTTP header.
    #[must_use]
    pub fn with_auth_token(mut self, token: &str) -> Self {
        let mut value = reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))
            .expect("the access token is a valid header value");
        value.set_sensitive(true);
        self.auth = Some(value);
        self
    }

    /// Whether a request goes to this client's backend, as opposed to a remote
    /// address that a download names. Only the backend gets the credential and
    /// the trace context; a third-party host is owed neither.
    fn targets_backend(&self, request: &reqwest::Request) -> bool {
        request
            .url()
            .as_str()
            .strip_prefix(&self.base_url)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with(['/', '?']))
    }

    /// Build `request`, adding the credential when it is bound for the backend.
    fn prepare(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<(reqwest::Request, bool), reqwest::Error> {
        let mut request = request.build()?;
        let backend = self.targets_backend(&request);
        if backend && let Some(auth) = &self.auth {
            request
                .headers_mut()
                .insert(reqwest::header::AUTHORIZATION, auth.clone());
        }
        Ok((request, backend))
    }

    /// Serve backend-held pictures through `host` rather than encoding them.
    #[must_use]
    pub fn with_image_host(mut self, host: crate::image_host::ImageHost) -> Self {
        self.image_host = Some(host);
        self
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    /// The REST base every `/api/v1/…` route hangs from.
    pub fn rest_url(&self) -> String {
        self.url("/api/v1")
    }

    pub fn openapi_url(&self) -> String {
        self.url("/api/v1/openapi.json")
    }

    pub fn graphql_url(&self) -> String {
        self.url("/graphql")
    }

    /// The bearer token this client presents to its backend, when the backend
    /// requires one — the desktop's per-launch token. App Settings hands it to
    /// the user so an external client they choose can reach the same backend.
    pub fn auth_token(&self) -> Option<String> {
        self.auth
            .as_ref()
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .map(str::to_owned)
    }

    #[cfg(feature = "telemetry-client")]
    async fn send_request(
        &self,
        method: &'static str,
        request: reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, reqwest::Error> {
        let (mut request, backend) = self.prepare(request)?;
        // A request to the backend is named by its route; a remote download
        // only by its method, its address being none of the trace's business.
        let route = backend.then(|| route_template(request.url().path()));
        let name = route
            .as_ref()
            .map_or_else(|| method.to_string(), |route| format!("{method} {route}"));
        let span = tracing::info_span!(
            "http.client.request",
            otel.name = %name,
            otel.kind = "client",
            http.request.method = method,
            http.route = route.as_deref(),
            http.response.status_code = tracing::field::Empty,
            otel.status_code = tracing::field::Empty,
        );
        if backend {
            global::get_text_map_propagator(|propagator| {
                propagator
                    .inject_context(&span.context(), &mut HeaderInjector(request.headers_mut()));
            });
        }

        let response = self.client.execute(request).instrument(span.clone()).await;
        match &response {
            Ok(response) => {
                let status = response.status();
                span.record("http.response.status_code", status.as_u16());
                if status.is_server_error() {
                    span.record("otel.status_code", "ERROR");
                }
            }
            Err(_) => {
                span.record("otel.status_code", "ERROR");
            }
        }
        response
    }

    #[cfg(not(feature = "telemetry-client"))]
    async fn send_request(
        &self,
        _method: &'static str,
        request: reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, reqwest::Error> {
        let (request, _) = self.prepare(request)?;
        self.client.execute(request).await
    }

    /// Invalidate all cached responses for a given tree.
    pub fn invalidate_tree(&self, tree_id: Uuid) {
        self.cache
            .invalidate_prefix(&format!("/api/v1/trees/{tree_id}"));
        // The batch of several trees' recent persons names no single tree.
        self.cache.invalidate_prefix(RECENT_PERSONS_PATH);
        self.pictures.invalidate_tree(tree_id);
    }

    /// Helper: send a cached GET request and deserialize JSON response.
    async fn get<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T, ApiError> {
        let url = self.url(path);
        self.get_deduplicated(path, || self.client.get(&url)).await
    }

    /// Serves `cache_key` from the cache when it is warm, and lets at most one
    /// request per key reach the network at a time.
    ///
    /// Without that gate the cache only ever helps the *next* render: a page
    /// whose components mount together has them all miss the still-empty cache
    /// and all send the same request. Whoever loses the race waits here and
    /// then finds the winner's response already cached.
    async fn get_deduplicated<T: serde::de::DeserializeOwned>(
        &self,
        cache_key: &str,
        request: impl FnOnce() -> reqwest::RequestBuilder,
    ) -> Result<T, ApiError> {
        if let Some(val) = self.cached(cache_key, false) {
            return Ok(val);
        }
        let result = {
            let gate = self.cache.gate(cache_key);
            let _guard = wait_for_gate(&gate, cache_key).await;
            match self.cached(cache_key, true) {
                Some(val) => Ok(val),
                None => self.fetch_and_cache(cache_key, request()).await,
            }
        };
        self.cache.release_gate(cache_key);
        result
    }

    /// The warm cache entry under `cache_key`, deserialized, logging the hit.
    ///
    /// `coalesced` says the caller waited for another request for the same
    /// key rather than finding the entry straight away.
    fn cached<T: serde::de::DeserializeOwned>(
        &self,
        cache_key: &str,
        coalesced: bool,
    ) -> Option<T> {
        let cached = self.cache.get(cache_key)?;
        // A hit sends nothing, so without a span of its own the data would
        // appear in the trace from nowhere.
        #[cfg(feature = "telemetry-client")]
        let val = {
            let route = route_template(cache_key);
            let span = tracing::info_span!(
                "ui.response.cached",
                otel.name = %format!("GET {route} (cached)"),
                http.route = %route,
                ui.request.coalesced = coalesced,
            );
            span.in_scope(|| Self::deserialize(&cached)).ok()?
        };
        #[cfg(not(feature = "telemetry-client"))]
        let val = Self::deserialize(&cached).ok()?;
        tracing::debug!(
            method = "GET",
            path = route_template(cache_key),
            cached = true,
            coalesced = coalesced.then_some(true),
            "API request completed"
        );
        Some(val)
    }

    /// Sends one GET, stores its body under `cache_key`, and deserializes it.
    ///
    /// The body is not stored when a write invalidated its tree while the
    /// request was in flight: it may predate that write.
    async fn fetch_and_cache<T: serde::de::DeserializeOwned>(
        &self,
        cache_key: &str,
        request: reqwest::RequestBuilder,
    ) -> Result<T, ApiError> {
        let generation = self.cache.generation(cache_key);
        let resp = self.send_request("GET", request).await?;
        let bytes = Self::successful_body("GET", resp).await?;
        let val: T = Self::deserialize(&bytes)?;
        self.cache
            .set_if_current(cache_key.to_string(), bytes, generation);
        Ok(val)
    }

    /// Helper: send a cached GET request with query parameters.
    async fn get_with_query<T: serde::de::DeserializeOwned, Q: Serialize>(
        &self,
        path: &str,
        query: &Q,
    ) -> Result<T, ApiError> {
        let cache_key = format!(
            "{}?{}",
            path,
            serde_json::to_string(query).unwrap_or_default()
        );
        let url = self.url(path);
        self.get_deduplicated(&cache_key, || self.client.get(&url).query(query))
            .await
    }

    /// Helper: read every page of a cursor-paginated list.
    ///
    /// `filters` are sent with each page; the cursor is the only parameter
    /// that changes between requests. A page that claims a successor but
    /// names no cursor ends the walk instead of asking for the first page
    /// again.
    async fn collect_pages<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        page_size: u64,
        filters: Vec<(&'static str, String)>,
    ) -> Result<Vec<T>, ApiError> {
        let mut all = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let mut params = filters.clone();
            params.push(("first", page_size.to_string()));
            if let Some(after) = cursor.take() {
                params.push(("after", after));
            }
            let page: PaginatedResponse<T> = self.get_with_query(path, &params).await?;
            all.extend(page.edges.into_iter().map(|edge| edge.node));
            match page.page_info.end_cursor {
                Some(next) if page.page_info.has_next_page => cursor = Some(next),
                _ => return Ok(all),
            }
        }
    }

    /// Sends a request that writes at `path`. Once the server accepted it,
    /// the cached reads of the tree `path` names are dropped, so no caller
    /// has to remember to; a path outside any one tree drops nothing (the
    /// tree list is invalidated by the tree methods themselves).
    async fn send_write(
        &self,
        method: &'static str,
        path: &str,
        request: reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, ApiError> {
        let resp = self.send_request(method, request).await?;
        if resp.status().is_success()
            && let Some(tree_id) = tree_of_path(path)
        {
            self.invalidate_tree(tree_id);
        }
        Ok(resp)
    }

    /// Helper: send a POST request with a JSON body, as a write.
    async fn post<T: serde::de::DeserializeOwned, B: Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, ApiError> {
        let url = self.url(path);
        let resp = self
            .send_write("POST", path, self.client.post(&url).json(body))
            .await?;
        Self::handle_response("POST", resp).await
    }

    /// Helper: send a POST request that only reads — a query too large for
    /// a URL, such as a batch of ids. It changes nothing, so it leaves the
    /// cache alone; it is not cached either.
    async fn post_read<T: serde::de::DeserializeOwned, B: Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, ApiError> {
        let url = self.url(path);
        let resp = self
            .send_request("POST", self.client.post(&url).json(body))
            .await?;
        Self::handle_response("POST", resp).await
    }

    /// Helper: send a POST request with a raw binary body.
    ///
    /// Used for uploads whose payload is a file whose encoding is the file's
    /// own business (see `inspect_geneweb`) — wrapping those bytes in JSON
    /// would force them through UTF-8 first.
    async fn post_bytes<T: serde::de::DeserializeOwned, Q: Serialize>(
        &self,
        path: &str,
        body: Vec<u8>,
        query: &Q,
    ) -> Result<T, ApiError> {
        let url = self.url(path);
        let bytes = body.len();
        let resp = self
            .send_write(
                "POST",
                path,
                self.client
                    .post(&url)
                    .query(query)
                    .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
                    .body(body),
            )
            .await?;
        tracing::debug!(
            method = "POST",
            path = route_template(path),
            bytes,
            "API binary request sent"
        );
        Self::handle_response("POST", resp).await
    }

    /// Helper: send a PUT request with a JSON body.
    async fn put<T: serde::de::DeserializeOwned, B: Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, ApiError> {
        let url = self.url(path);
        let resp = self
            .send_write("PUT", path, self.client.put(&url).json(body))
            .await?;
        Self::handle_response("PUT", resp).await
    }

    /// Helper: send a PATCH request with a JSON body.
    async fn patch<T: serde::de::DeserializeOwned, B: Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, ApiError> {
        let url = self.url(path);
        let resp = self
            .send_write("PATCH", path, self.client.patch(&url).json(body))
            .await?;
        Self::handle_response("PATCH", resp).await
    }

    /// Send a DELETE request and return its successful status code.
    async fn delete_status(&self, path: &str) -> Result<u16, ApiError> {
        let url = self.url(path);
        let resp = self
            .send_write("DELETE", path, self.client.delete(&url))
            .await?;
        let status = resp.status();
        Self::require_success(resp).await.inspect_err(|_| {
            tracing::debug!(method = "DELETE", path = route_template(path), %status, "API request failed");
        })?;
        tracing::debug!(method = "DELETE", path = route_template(path), %status, "API request completed");
        Ok(status.as_u16())
    }

    async fn delete_no_content(&self, path: &str) -> Result<(), ApiError> {
        self.delete_status(path).await.map(|_| ())
    }

    /// Send a POST request whose success carries no body (`204`).
    async fn post_no_content<B: Serialize>(&self, path: &str, body: &B) -> Result<(), ApiError> {
        let url = self.url(path);
        let resp = self
            .send_write("POST", path, self.client.post(&url).json(body))
            .await?;
        Self::require_success(resp).await?;
        Ok(())
    }

    /// Handle HTTP response: check status, parse JSON.
    async fn handle_response<T: serde::de::DeserializeOwned>(
        method: &str,
        resp: reqwest::Response,
    ) -> Result<T, ApiError> {
        let bytes = Self::successful_body(method, resp).await?;
        Ok(Self::deserialize(&bytes)?)
    }

    /// Pass a successful response through, or turn any other into
    /// [`ApiError::Api`] carrying its status and body.
    async fn require_success(resp: reqwest::Response) -> Result<reqwest::Response, ApiError> {
        let status = resp.status();
        if status.is_success() {
            return Ok(resp);
        }
        Err(ApiError::Api {
            status: status.as_u16(),
            body: resp.text().await.unwrap_or_default(),
        })
    }

    /// The body of a successful response, logging how the request ended.
    async fn successful_body(method: &str, resp: reqwest::Response) -> Result<Vec<u8>, ApiError> {
        let status = resp.status();
        let path = route_template(resp.url().path());
        let resp = Self::require_success(resp).await.inspect_err(|_| {
            tracing::debug!(method, path, %status, "API request failed");
        })?;
        let bytes = Self::read_response_body(resp).await?;
        tracing::debug!(method, path, %status, bytes = bytes.len(), "API request completed");
        Ok(bytes)
    }

    // ── Trees ───────────────────────────────────────────────────────

    pub async fn list_trees(
        &self,
        first: Option<u64>,
        after: Option<&str>,
    ) -> Result<PaginatedResponse<TreeListItem>, ApiError> {
        let mut params = Vec::new();
        if let Some(f) = first {
            params.push(("first", f.to_string()));
        }
        if let Some(a) = after {
            params.push(("after", a.to_string()));
        }
        self.get_with_query("/api/v1/trees", &params).await
    }

    /// Force the next home-page tree list request to observe live job state.
    pub fn invalidate_tree_list(&self) {
        self.cache.invalidate_prefix("/api/v1/trees");
    }

    pub async fn get_tree(&self, id: Uuid) -> Result<Tree, ApiError> {
        self.get(&format!("/api/v1/trees/{id}")).await
    }

    pub async fn create_tree(&self, body: &CreateTreeBody) -> Result<Tree, ApiError> {
        let result = self.post("/api/v1/trees", body).await?;
        self.cache.invalidate_prefix("/api/v1/trees");
        Ok(result)
    }

    pub async fn update_tree(&self, id: Uuid, body: &UpdateTreeBody) -> Result<Tree, ApiError> {
        let result = self.put(&format!("/api/v1/trees/{id}"), body).await?;
        self.cache.invalidate_prefix("/api/v1/trees");
        Ok(result)
    }

    pub async fn duplicate_tree(
        &self,
        id: Uuid,
        body: &DuplicateTreeBody,
    ) -> Result<Tree, ApiError> {
        let result = self
            .post(&format!("/api/v1/trees/{id}/duplicate"), body)
            .await?;
        self.cache.invalidate_prefix("/api/v1/trees");
        Ok(result)
    }

    pub async fn delete_tree(&self, id: Uuid) -> Result<(), ApiError> {
        self.delete_no_content(&format!("/api/v1/trees/{id}"))
            .await?;
        self.cache.invalidate_prefix("/api/v1/trees");
        Ok(())
    }

    pub async fn get_person_detail_bundle(
        &self,
        tree_id: Uuid,
        person_id: Uuid,
    ) -> Result<PersonDetailBundle, ApiError> {
        self.get(&format!(
            "/api/v1/trees/{tree_id}/persons/{person_id}/detail-bundle"
        ))
        .await
    }

    /// Everything one couple page draws, in one request.
    pub async fn get_couple_detail_bundle(
        &self,
        tree_id: Uuid,
        family_id: Uuid,
    ) -> Result<CoupleDetailBundle, ApiError> {
        self.get(&format!(
            "/api/v1/trees/{tree_id}/families/{family_id}/detail-bundle"
        ))
        .await
    }

    // ── Persons ─────────────────────────────────────────────────────

    /// Free-text person search, server-side (Sprint E.6).
    ///
    /// Backed by the `person_search_fts` DB table (SQLite FTS5 / PostgreSQL):
    /// accent-folded, every word of the query must match (prefix matching on
    /// SQLite). An empty query lists persons sorted by name (browse mode).
    pub async fn search_persons(
        &self,
        tree_id: Uuid,
        query: &str,
        limit: u32,
        offset: u32,
    ) -> Result<SearchResult, ApiError> {
        let params = [
            ("q", query.to_string()),
            ("limit", limit.to_string()),
            ("offset", offset.to_string()),
        ];
        self.get_with_query(&format!("/api/v1/trees/{tree_id}/persons/search"), &params)
            .await
    }

    pub async fn search_persons_filtered(
        &self,
        tree_id: Uuid,
        search: &PersonSearchParams,
    ) -> Result<SearchResult, ApiError> {
        let mut params = vec![
            ("q", search.query.clone()),
            ("limit", search.limit.to_string()),
            ("offset", search.offset.to_string()),
            ("sort", search.sort.as_str().to_string()),
        ];
        for (name, value) in [
            ("surname", search.surname.as_deref()),
            ("given_names", search.given_names.as_deref()),
            ("occupation", search.occupation.as_deref()),
            ("spouse_surname", search.spouse_surname.as_deref()),
            ("spouse_given_names", search.spouse_given_names.as_deref()),
            ("father_surname", search.father_surname.as_deref()),
            ("father_given_names", search.father_given_names.as_deref()),
            ("mother_surname", search.mother_surname.as_deref()),
            ("mother_given_names", search.mother_given_names.as_deref()),
            ("place", search.place.as_deref()),
        ] {
            if let Some(value) = value.filter(|value| !value.trim().is_empty()) {
                params.push((name, value.trim().to_string()));
            }
        }
        for (name, value) in [
            ("birth_from", search.birth_from),
            ("birth_to", search.birth_to),
            ("death_from", search.death_from),
            ("death_to", search.death_to),
            ("event_from", search.event_from),
            ("event_to", search.event_to),
        ] {
            if let Some(value) = value {
                params.push((name, value.to_string()));
            }
        }
        if let Some(sex) = search.sex {
            params.push(("sex", sex.to_string()));
        }
        if let Some(event_type) = search.event_type {
            params.push(("event_type", event_type.to_string()));
        }
        if search.has_media {
            params.push(("has_media", "true".to_string()));
        }
        self.get_with_query(&format!("/api/v1/trees/{tree_id}/persons/search"), &params)
            .await
    }

    pub async fn list_persons(
        &self,
        tree_id: Uuid,
        first: Option<u64>,
        after: Option<&str>,
    ) -> Result<PaginatedResponse<Person>, ApiError> {
        let mut params = Vec::new();
        if let Some(f) = first {
            params.push(("first", f.to_string()));
        }
        if let Some(a) = after {
            params.push(("after", a.to_string()));
        }
        self.get_with_query(&format!("/api/v1/trees/{tree_id}/persons"), &params)
            .await
    }

    pub async fn get_person(&self, tree_id: Uuid, id: Uuid) -> Result<PersonDetail, ApiError> {
        self.get(&format!("/api/v1/trees/{tree_id}/persons/{id}"))
            .await
    }

    pub async fn get_person_profile(
        &self,
        tree_id: Uuid,
        person_id: Uuid,
    ) -> Result<PersonProfile, ApiError> {
        self.get(&format!("/api/v1/trees/{tree_id}/profiles/{person_id}"))
            .await
    }

    /// Resolve a SOSA-Stradonitz number to a person, relative to the tree's
    /// configured SOSA root. Errors (including "not found") should be
    /// treated as a cue to fall back to a normal name search.
    pub async fn get_person_by_sosa(
        &self,
        tree_id: Uuid,
        number: u64,
    ) -> Result<PersonDetail, ApiError> {
        self.get(&format!("/api/v1/trees/{tree_id}/persons/sosa/{number}"))
            .await
    }

    pub async fn create_person(
        &self,
        tree_id: Uuid,
        body: &CreatePersonBody,
    ) -> Result<Person, ApiError> {
        self.post(&format!("/api/v1/trees/{tree_id}/persons"), body)
            .await
    }

    pub async fn update_person(
        &self,
        tree_id: Uuid,
        id: Uuid,
        body: &UpdatePersonBody,
    ) -> Result<Person, ApiError> {
        self.put(&format!("/api/v1/trees/{tree_id}/persons/{id}"), body)
            .await
    }

    pub async fn delete_person(&self, tree_id: Uuid, id: Uuid) -> Result<(), ApiError> {
        self.delete_no_content(&format!("/api/v1/trees/{tree_id}/persons/{id}"))
            .await
    }

    /// The other persons of the tree bearing the same name as `person_id`,
    /// less those already confirmed to be somebody else.
    pub async fn person_homonyms(
        &self,
        tree_id: Uuid,
        person_id: Uuid,
    ) -> Result<Vec<SearchEntry>, ApiError> {
        self.get(&format!(
            "/api/v1/trees/{tree_id}/persons/{person_id}/homonyms"
        ))
        .await
    }

    /// The persons modified most recently in each of `tree_ids`, in one
    /// request; a tree the server no longer has is left out.
    pub async fn recent_persons_of_trees(
        &self,
        tree_ids: &[Uuid],
        limit: usize,
    ) -> Result<HashMap<Uuid, Vec<SearchEntry>>, ApiError> {
        #[derive(Deserialize)]
        struct TreeRecentPersons {
            tree_id: Uuid,
            persons: Vec<SearchEntry>,
        }
        if tree_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let ids = tree_ids
            .iter()
            .map(Uuid::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let trees: Vec<TreeRecentPersons> = self
            .get(&format!(
                "{RECENT_PERSONS_PATH}?tree_ids={ids}&limit={limit}"
            ))
            .await?;
        Ok(trees
            .into_iter()
            .map(|tree| (tree.tree_id, tree.persons))
            .collect())
    }

    /// Record that `person_id` is a different person from each of `others`.
    pub async fn mark_persons_distinct(
        &self,
        tree_id: Uuid,
        person_id: Uuid,
        others: &[Uuid],
    ) -> Result<(), ApiError> {
        self.post_no_content(
            &format!("/api/v1/trees/{tree_id}/persons/{person_id}/distinct"),
            &MarkPersonsDistinctBody {
                person_ids: others.to_vec(),
            },
        )
        .await
    }

    /// Merge `duplicate` into `kept`, which survives, as `choices` says
    /// (`docs/api.md`, merge); returns the kept person.
    pub async fn merge_persons(
        &self,
        tree_id: Uuid,
        kept: Uuid,
        duplicate: Uuid,
        choices: &MergeChoices,
    ) -> Result<Person, ApiError> {
        self.post(
            &format!("/api/v1/trees/{tree_id}/persons/{kept}/merge"),
            &MergePersonBody {
                duplicate_id: duplicate,
                choices,
            },
        )
        .await
    }

    /// Every way found to go from `person_id` to `other_person_id`.
    pub async fn get_kinship(
        &self,
        tree_id: Uuid,
        person_id: Uuid,
        other_person_id: Uuid,
    ) -> Result<Kinship, ApiError> {
        self.get(&format!(
            "/api/v1/trees/{tree_id}/persons/{person_id}/kinship/{other_person_id}"
        ))
        .await
    }

    // ── Person Names ────────────────────────────────────────────────

    pub async fn list_person_names(
        &self,
        tree_id: Uuid,
        person_id: Uuid,
    ) -> Result<Vec<PersonName>, ApiError> {
        self.get(&format!(
            "/api/v1/trees/{tree_id}/persons/{person_id}/names"
        ))
        .await
    }

    /// Load names and spouse links in bounded batches, issuing as many
    /// requests as needed for a larger logical set.
    pub async fn relation_labels(
        &self,
        tree_id: Uuid,
        person_ids: &[Uuid],
        family_ids: &[Uuid],
    ) -> Result<RelationLabels, ApiError> {
        let mut labels = RelationLabels::default();
        for (person_range, family_range) in
            relation_label_batch_ranges(person_ids.len(), family_ids.len())
        {
            let body = RelationLabelsRequest {
                person_ids: person_ids[person_range].to_vec(),
                family_ids: family_ids[family_range].to_vec(),
            };
            let batch = self
                .post_read::<RelationLabels, _>(
                    &format!("/api/v1/trees/{tree_id}/relation-labels"),
                    &body,
                )
                .await?;
            labels.names.extend(batch.names);
            labels.spouses.extend(batch.spouses);
        }
        Ok(labels)
    }

    /// Every name of `person_ids`, filed by person, in bounded batches.
    pub async fn person_names(
        &self,
        tree_id: Uuid,
        person_ids: &[Uuid],
    ) -> Result<HashMap<Uuid, Vec<PersonName>>, ApiError> {
        let labels = self.relation_labels(tree_id, person_ids, &[]).await?;
        let mut names: HashMap<Uuid, Vec<PersonName>> = HashMap::new();
        for name in labels.names {
            names.entry(name.person_id).or_default().push(name);
        }
        Ok(names)
    }

    pub async fn create_person_name(
        &self,
        tree_id: Uuid,
        person_id: Uuid,
        body: &CreatePersonNameBody,
    ) -> Result<PersonName, ApiError> {
        self.post(
            &format!("/api/v1/trees/{tree_id}/persons/{person_id}/names"),
            body,
        )
        .await
    }

    pub async fn update_person_name(
        &self,
        tree_id: Uuid,
        person_id: Uuid,
        name_id: Uuid,
        body: &UpdatePersonNameBody,
    ) -> Result<PersonName, ApiError> {
        self.put(
            &format!("/api/v1/trees/{tree_id}/persons/{person_id}/names/{name_id}"),
            body,
        )
        .await
    }

    pub async fn delete_person_name(
        &self,
        tree_id: Uuid,
        person_id: Uuid,
        name_id: Uuid,
    ) -> Result<(), ApiError> {
        self.delete_no_content(&format!(
            "/api/v1/trees/{tree_id}/persons/{person_id}/names/{name_id}"
        ))
        .await
    }

    // ── Families ────────────────────────────────────────────────────

    pub async fn get_family(&self, tree_id: Uuid, id: Uuid) -> Result<Family, ApiError> {
        self.get(&format!("/api/v1/trees/{tree_id}/families/{id}"))
            .await
    }

    pub async fn create_family(&self, tree_id: Uuid) -> Result<Family, ApiError> {
        self.post(
            &format!("/api/v1/trees/{tree_id}/families"),
            &serde_json::json!({}),
        )
        .await
    }

    /// Set a couple's privacy.
    pub async fn update_family_privacy(
        &self,
        tree_id: Uuid,
        id: Uuid,
        privacy: Privacy,
    ) -> Result<Family, ApiError> {
        self.put(
            &format!("/api/v1/trees/{tree_id}/families/{id}"),
            &serde_json::json!({ "privacy": privacy }),
        )
        .await
    }

    pub async fn delete_family(&self, tree_id: Uuid, id: Uuid) -> Result<(), ApiError> {
        self.delete_no_content(&format!("/api/v1/trees/{tree_id}/families/{id}"))
            .await
    }

    // ── Family Spouses ──────────────────────────────────────────────

    pub async fn list_family_spouses(
        &self,
        tree_id: Uuid,
        family_id: Uuid,
    ) -> Result<Vec<FamilySpouse>, ApiError> {
        self.get(&format!(
            "/api/v1/trees/{tree_id}/families/{family_id}/spouses"
        ))
        .await
    }

    pub async fn add_spouse(
        &self,
        tree_id: Uuid,
        family_id: Uuid,
        body: &AddSpouseBody,
    ) -> Result<serde_json::Value, ApiError> {
        self.post(
            &format!("/api/v1/trees/{tree_id}/families/{family_id}/spouses"),
            body,
        )
        .await
    }

    // ── Family Children ─────────────────────────────────────────────

    pub async fn list_family_children(
        &self,
        tree_id: Uuid,
        family_id: Uuid,
    ) -> Result<Vec<FamilyChild>, ApiError> {
        self.get(&format!(
            "/api/v1/trees/{tree_id}/families/{family_id}/children"
        ))
        .await
    }

    pub async fn add_child(
        &self,
        tree_id: Uuid,
        family_id: Uuid,
        body: &AddChildBody,
    ) -> Result<serde_json::Value, ApiError> {
        self.post(
            &format!("/api/v1/trees/{tree_id}/families/{family_id}/children"),
            body,
        )
        .await
    }

    /// Detaches a child from a family by deleting `link_id`, the
    /// family-child link — not the child's person id.
    pub async fn remove_child(
        &self,
        tree_id: Uuid,
        family_id: Uuid,
        link_id: Uuid,
    ) -> Result<(), ApiError> {
        self.delete_no_content(&format!(
            "/api/v1/trees/{tree_id}/families/{family_id}/children/{link_id}"
        ))
        .await
    }

    // ── Events ──────────────────────────────────────────────────────

    pub async fn list_events(
        &self,
        tree_id: Uuid,
        first: Option<u64>,
        after: Option<&str>,
        event_type: Option<EventType>,
        person_id: Option<Uuid>,
        family_id: Option<Uuid>,
    ) -> Result<PaginatedResponse<Event>, ApiError> {
        let mut params: Vec<(&str, String)> = Vec::new();
        if let Some(f) = first {
            params.push(("first", f.to_string()));
        }
        if let Some(a) = after {
            params.push(("after", a.to_string()));
        }
        if let Some(et) = event_type {
            params.push((
                "event_type",
                serde_json::to_string(&et)
                    .unwrap()
                    .trim_matches('"')
                    .to_string(),
            ));
        }
        if let Some(pid) = person_id {
            params.push(("person_id", pid.to_string()));
        }
        if let Some(fid) = family_id {
            params.push(("family_id", fid.to_string()));
        }
        self.get_with_query(&format!("/api/v1/trees/{tree_id}/events"), &params)
            .await
    }

    pub async fn create_event(
        &self,
        tree_id: Uuid,
        body: &CreateEventBody,
    ) -> Result<Event, ApiError> {
        self.post(&format!("/api/v1/trees/{tree_id}/events"), body)
            .await
    }

    pub async fn update_event(
        &self,
        tree_id: Uuid,
        id: Uuid,
        body: &UpdateEventBody,
    ) -> Result<Event, ApiError> {
        self.put(&format!("/api/v1/trees/{tree_id}/events/{id}"), body)
            .await
    }

    pub async fn delete_event(&self, tree_id: Uuid, id: Uuid) -> Result<(), ApiError> {
        self.delete_no_content(&format!("/api/v1/trees/{tree_id}/events/{id}"))
            .await
    }

    // ── Event Witnesses ────────────────────────────────────────────────

    pub async fn list_event_witnesses(
        &self,
        tree_id: Uuid,
        event_id: Uuid,
    ) -> Result<Vec<EventWitness>, ApiError> {
        self.get(&format!(
            "/api/v1/trees/{tree_id}/events/{event_id}/witnesses"
        ))
        .await
    }

    pub async fn add_event_witness(
        &self,
        tree_id: Uuid,
        event_id: Uuid,
        body: &AddEventWitnessBody,
    ) -> Result<EventWitness, ApiError> {
        self.post(
            &format!("/api/v1/trees/{tree_id}/events/{event_id}/witnesses"),
            body,
        )
        .await
    }

    pub async fn remove_event_witness(
        &self,
        tree_id: Uuid,
        event_id: Uuid,
        witness_id: Uuid,
    ) -> Result<(), ApiError> {
        self.delete_no_content(&format!(
            "/api/v1/trees/{tree_id}/events/{event_id}/witnesses/{witness_id}"
        ))
        .await
    }

    // ── Places ──────────────────────────────────────────────────────

    /// The tree's place named `name` — trimmed, ignoring case — if any.
    pub async fn find_place(&self, tree_id: Uuid, name: &str) -> Result<Option<Place>, ApiError> {
        let page: PaginatedResponse<Place> = self
            .get_with_query(
                &format!("/api/v1/trees/{tree_id}/places"),
                &[("name", name.trim()), ("first", "1")],
            )
            .await?;
        Ok(page.edges.into_iter().next().map(|edge| edge.node))
    }

    /// The places of `ids` in the tree, a page of ids per request.
    pub async fn places_by_ids(&self, tree_id: Uuid, ids: &[Uuid]) -> Result<Vec<Place>, ApiError> {
        const PER_REQUEST: usize = 100;
        let mut places = Vec::with_capacity(ids.len());
        for chunk in ids.chunks(PER_REQUEST) {
            let joined = chunk
                .iter()
                .map(Uuid::to_string)
                .collect::<Vec<_>>()
                .join(",");
            let page: PaginatedResponse<Place> = self
                .get_with_query(
                    &format!("/api/v1/trees/{tree_id}/places"),
                    &[("ids", joined), ("first", PER_REQUEST.to_string())],
                )
                .await?;
            places.extend(page.edges.into_iter().map(|edge| edge.node));
        }
        Ok(places)
    }

    pub async fn get_place(&self, tree_id: Uuid, id: Uuid) -> Result<Place, ApiError> {
        self.get(&format!("/api/v1/trees/{tree_id}/places/{id}"))
            .await
    }

    pub async fn create_place(
        &self,
        tree_id: Uuid,
        body: &CreatePlaceBody,
    ) -> Result<Place, ApiError> {
        self.post(&format!("/api/v1/trees/{tree_id}/places"), body)
            .await
    }

    pub async fn update_place(
        &self,
        tree_id: Uuid,
        id: Uuid,
        body: &UpdatePlaceBody,
    ) -> Result<Place, ApiError> {
        self.put(&format!("/api/v1/trees/{tree_id}/places/{id}"), body)
            .await
    }

    // ── Sources ─────────────────────────────────────────────────────

    /// The tree's source titled `title` — trimmed, ignoring case — if any.
    pub async fn find_source(
        &self,
        tree_id: Uuid,
        title: &str,
    ) -> Result<Option<Source>, ApiError> {
        let page: PaginatedResponse<Source> = self
            .get_with_query(
                &format!("/api/v1/trees/{tree_id}/sources"),
                &[("title", title.trim()), ("first", "1")],
            )
            .await?;
        Ok(page.edges.into_iter().next().map(|edge| edge.node))
    }

    pub async fn get_source(&self, tree_id: Uuid, id: Uuid) -> Result<Source, ApiError> {
        self.get(&format!("/api/v1/trees/{tree_id}/sources/{id}"))
            .await
    }

    pub async fn create_source(
        &self,
        tree_id: Uuid,
        body: &CreateSourceBody,
    ) -> Result<Source, ApiError> {
        self.post(&format!("/api/v1/trees/{tree_id}/sources"), body)
            .await
    }

    pub async fn update_source(
        &self,
        tree_id: Uuid,
        id: Uuid,
        body: &UpdateSourceBody,
    ) -> Result<Source, ApiError> {
        self.put(&format!("/api/v1/trees/{tree_id}/sources/{id}"), body)
            .await
    }

    /// Where a source written as an archive citation opens on its archive's
    /// portal, as cited by `citation_id` when given. It may query the
    /// portal: call it once per reader's click, never ahead of one.
    ///
    /// `view` asks for that one view of the cited register instead of the
    /// cited ones: the previous or next view a reader pages to.
    pub async fn archive_target(
        &self,
        tree_id: Uuid,
        source_id: Uuid,
        citation_id: Option<Uuid>,
        view: Option<u16>,
    ) -> Result<oxidgene_archives::ArchiveTarget, ApiError> {
        self.post_read(
            &format!("/api/v1/trees/{tree_id}/sources/{source_id}/archive-target"),
            &serde_json::json!({ "citation_id": citation_id, "view": view }),
        )
        .await
    }

    // ── Repositories ────────────────────────────────────────────────

    /// Every repository of the tree, page after page.
    pub async fn list_all_repositories(&self, tree_id: Uuid) -> Result<Vec<Repository>, ApiError> {
        self.collect_pages(
            &format!("/api/v1/trees/{tree_id}/repositories"),
            500,
            Vec::new(),
        )
        .await
    }

    pub async fn create_repository(
        &self,
        tree_id: Uuid,
        body: &CreateRepositoryBody,
    ) -> Result<Repository, ApiError> {
        self.post(&format!("/api/v1/trees/{tree_id}/repositories"), body)
            .await
    }

    pub async fn update_repository(
        &self,
        tree_id: Uuid,
        id: Uuid,
        body: &UpdateRepositoryBody,
    ) -> Result<Repository, ApiError> {
        self.put(&format!("/api/v1/trees/{tree_id}/repositories/{id}"), body)
            .await
    }

    pub async fn delete_repository(&self, tree_id: Uuid, id: Uuid) -> Result<(), ApiError> {
        self.delete_no_content(&format!("/api/v1/trees/{tree_id}/repositories/{id}"))
            .await
    }

    /// The sources a repository holds, each with its call number.
    pub async fn repository_sources(
        &self,
        tree_id: Uuid,
        id: Uuid,
    ) -> Result<Vec<HeldSource>, ApiError> {
        self.get(&format!(
            "/api/v1/trees/{tree_id}/repositories/{id}/sources"
        ))
        .await
    }

    /// The notes about a repository.
    pub async fn list_repository_notes(
        &self,
        tree_id: Uuid,
        repository_id: Uuid,
    ) -> Result<Vec<Note>, ApiError> {
        let filters = owner_filters([("repository_id", Some(repository_id))]);
        self.collect_pages(&format!("/api/v1/trees/{tree_id}/notes"), 100, filters)
            .await
    }

    /// The repositories holding a source, in order.
    pub async fn source_repositories(
        &self,
        tree_id: Uuid,
        source_id: Uuid,
    ) -> Result<Vec<SourceRepository>, ApiError> {
        self.get(&format!(
            "/api/v1/trees/{tree_id}/sources/{source_id}/repositories"
        ))
        .await
    }

    pub async fn add_source_repository(
        &self,
        tree_id: Uuid,
        source_id: Uuid,
        body: &AddSourceRepositoryBody,
    ) -> Result<SourceRepository, ApiError> {
        self.post(
            &format!("/api/v1/trees/{tree_id}/sources/{source_id}/repositories"),
            body,
        )
        .await
    }

    pub async fn remove_source_repository(
        &self,
        tree_id: Uuid,
        source_id: Uuid,
        link_id: Uuid,
    ) -> Result<(), ApiError> {
        self.delete_no_content(&format!(
            "/api/v1/trees/{tree_id}/sources/{source_id}/repositories/{link_id}"
        ))
        .await
    }

    /// Deletes a source only if no citation, note, media link or repository link still points
    /// at it. Returns whether it was deleted — `false` means it is still in
    /// use and was kept.
    pub async fn delete_source_if_unused(&self, tree_id: Uuid, id: Uuid) -> Result<bool, ApiError> {
        let status = self
            .delete_status(&format!(
                "/api/v1/trees/{tree_id}/sources/{id}?only_if_unused=true"
            ))
            .await?;
        Ok(status == 204)
    }

    // ── Dictionary ───────────────────────────────────────────────────

    /// Distinct surnames in the tree, with the number of persons carrying each.
    pub async fn dictionary_family_names(
        &self,
        tree_id: Uuid,
    ) -> Result<Vec<DictionaryEntry>, ApiError> {
        self.get(&format!("/api/v1/trees/{tree_id}/dictionary/family-names"))
            .await
    }

    /// Persons carrying a given family name.
    pub async fn dictionary_family_name_usage(
        &self,
        tree_id: Uuid,
        value: &str,
    ) -> Result<Vec<PersonUsageEntry>, ApiError> {
        self.get_with_query(
            &format!("/api/v1/trees/{tree_id}/dictionary/family-names/usage"),
            &[("value", value)],
        )
        .await
    }

    /// Re-cut every occurrence of a family name at `particle` — the bulk
    /// repair for an import that guessed the particle wrong across a whole
    /// family. An empty `particle` means "this name has no particle".
    pub async fn set_family_name_particle(
        &self,
        tree_id: Uuid,
        value: &str,
        particle: &str,
    ) -> Result<FamilyNameParticleUpdate, ApiError> {
        self.patch(
            &format!("/api/v1/trees/{tree_id}/dictionary/family-names/particle"),
            &SetFamilyNameParticleBody {
                value: value.to_string(),
                particle: particle.to_string(),
            },
        )
        .await
    }

    /// Give every person whose primary name carries family name `value` the
    /// name `new_value`, merging into it when it is already listed.
    /// `particle` chooses where `new_value` splits; `None` keeps the split it
    /// already has in the tree, or detects one.
    pub async fn rename_family_name(
        &self,
        tree_id: Uuid,
        value: &str,
        new_value: &str,
        particle: Option<&str>,
    ) -> Result<FamilyNameRename, ApiError> {
        self.patch(
            &format!("/api/v1/trees/{tree_id}/dictionary/family-names/rename"),
            &RenameFamilyNameBody {
                value: value.to_string(),
                new_value: new_value.to_string(),
                particle: particle.map(str::to_string),
            },
        )
        .await
    }

    /// Distinct occupation labels in the tree, with the number of persons holding each.
    pub async fn dictionary_occupations(
        &self,
        tree_id: Uuid,
    ) -> Result<Vec<DictionaryEntry>, ApiError> {
        self.get(&format!("/api/v1/trees/{tree_id}/dictionary/occupations"))
            .await
    }

    /// Resolves the Sources tab's smart drill-down starting from `prefix`
    /// (empty = start from the top): the backend auto-skips forced
    /// single-choice levels and returns either the real next branch
    /// choices, or, once the count is small enough, an empty `groups` list
    /// and the level's sources. See ui-dictionary.md §8.10.
    pub async fn dictionary_source_groups(
        &self,
        tree_id: Uuid,
        prefix: &str,
    ) -> Result<SourceDrillResponse, ApiError> {
        self.get_with_query(
            &format!("/api/v1/trees/{tree_id}/dictionary/sources/groups"),
            &[("prefix", prefix)],
        )
        .await
    }

    /// All places in the tree, each paired with its usage count.
    pub async fn dictionary_places(
        &self,
        tree_id: Uuid,
    ) -> Result<Vec<PlaceDictionaryEntry>, ApiError> {
        self.get(&format!("/api/v1/trees/{tree_id}/dictionary/places"))
            .await
    }

    /// Persons citing a given source.
    pub async fn dictionary_source_usage(
        &self,
        tree_id: Uuid,
        source_id: Uuid,
    ) -> Result<Vec<PersonUsageEntry>, ApiError> {
        self.get(&format!(
            "/api/v1/trees/{tree_id}/dictionary/sources/{source_id}/usage"
        ))
        .await
    }

    /// Persons with an event at a given place.
    pub async fn dictionary_place_usage(
        &self,
        tree_id: Uuid,
        place_id: Uuid,
    ) -> Result<Vec<PersonUsageEntry>, ApiError> {
        self.get(&format!(
            "/api/v1/trees/{tree_id}/dictionary/places/{place_id}/usage"
        ))
        .await
    }

    /// Persons holding a given occupation label.
    pub async fn dictionary_occupation_usage(
        &self,
        tree_id: Uuid,
        value: &str,
    ) -> Result<Vec<PersonUsageEntry>, ApiError> {
        self.get_with_query(
            &format!("/api/v1/trees/{tree_id}/dictionary/occupations/usage"),
            &[("value", value)],
        )
        .await
    }

    // ── Citations ────────────────────────────────────────────────────

    pub async fn create_citation(
        &self,
        tree_id: Uuid,
        body: &CreateCitationBody,
    ) -> Result<Citation, ApiError> {
        self.post(&format!("/api/v1/trees/{tree_id}/citations"), body)
            .await
    }

    pub async fn update_citation(
        &self,
        tree_id: Uuid,
        citation_id: Uuid,
        body: &UpdateCitationBody,
    ) -> Result<Citation, ApiError> {
        self.put(
            &format!("/api/v1/trees/{tree_id}/citations/{citation_id}"),
            body,
        )
        .await
    }

    pub async fn delete_citation(&self, tree_id: Uuid, citation_id: Uuid) -> Result<(), ApiError> {
        self.delete_no_content(&format!("/api/v1/trees/{tree_id}/citations/{citation_id}"))
            .await
    }

    pub async fn list_citations(
        &self,
        tree_id: Uuid,
        person_id: Option<Uuid>,
        event_id: Option<Uuid>,
        family_id: Option<Uuid>,
        source_id: Option<Uuid>,
    ) -> Result<Vec<Citation>, ApiError> {
        let filters = owner_filters([
            ("person_id", person_id),
            ("event_id", event_id),
            ("family_id", family_id),
            ("source_id", source_id),
        ]);
        self.collect_pages(&format!("/api/v1/trees/{tree_id}/citations"), 100, filters)
            .await
    }

    // ── Notes ─────────────────────────────────────────────────────────

    pub async fn list_notes(
        &self,
        tree_id: Uuid,
        person_id: Option<Uuid>,
        event_id: Option<Uuid>,
        family_id: Option<Uuid>,
        source_id: Option<Uuid>,
        media_id: Option<Uuid>,
    ) -> Result<Vec<Note>, ApiError> {
        let filters = owner_filters([
            ("person_id", person_id),
            ("event_id", event_id),
            ("family_id", family_id),
            ("source_id", source_id),
            ("media_id", media_id),
        ]);
        self.collect_pages(&format!("/api/v1/trees/{tree_id}/notes"), 100, filters)
            .await
    }

    pub async fn create_note(
        &self,
        tree_id: Uuid,
        body: &CreateNoteBody,
    ) -> Result<Note, ApiError> {
        self.post(&format!("/api/v1/trees/{tree_id}/notes"), body)
            .await
    }

    pub async fn update_note(
        &self,
        tree_id: Uuid,
        note_id: Uuid,
        body: &UpdateNoteBody,
    ) -> Result<Note, ApiError> {
        self.put(&format!("/api/v1/trees/{tree_id}/notes/{note_id}"), body)
            .await
    }

    pub async fn delete_note(&self, tree_id: Uuid, note_id: Uuid) -> Result<(), ApiError> {
        self.delete_no_content(&format!("/api/v1/trees/{tree_id}/notes/{note_id}"))
            .await
    }

    // ── Media ───────────────────────────────────────────────────────

    /// Absolute URL of a media's stored bytes.
    ///
    /// Returned as a URL rather than as bytes because these go straight into
    /// an `<img src>`: letting the engine fetch them means it also gets the
    /// `ETag` revalidation the endpoint offers, which pulling them through
    /// this client would throw away.
    /// Fetch a file's raw bytes and its content type.
    ///
    /// Public so a shell that serves pictures from its own origin can answer
    /// through the same client, connection pool and tracing as every other
    /// request, rather than opening a second path to the backend.
    pub async fn get_binary(&self, path: &str) -> Result<(Vec<u8>, String), ApiError> {
        let response = self
            .send_request("GET", self.client.get(self.url(path)))
            .await?;
        let status = response.status();
        let path = route_template(response.url().path());
        let response = Self::require_success(response).await.inspect_err(|_| {
            tracing::debug!(method = "GET", path, %status, "API binary request failed");
        })?;
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("application/octet-stream")
            .to_string();
        let bytes = Self::read_response_body(response).await?;
        tracing::debug!(method = "GET", path, %status, bytes = bytes.len(), "API binary request completed");
        Ok((bytes, content_type))
    }

    async fn get_binary_data_url(&self, path: &str) -> Result<String, ApiError> {
        let (bytes, content_type) = self.get_binary(path).await?;
        let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
        Ok(format!("data:{content_type};base64,{encoded}"))
    }

    /// A media's stored file, ready to draw.
    ///
    /// Goes through the shell that serves pictures from its own origin when
    /// there is one — which matters most here, since this is the full-size
    /// file: inlining a multi-megabyte scan costs a third again in base64 and
    /// denies the engine its own caching and decoding.
    pub async fn media_file_data_url(
        &self,
        tree_id: Uuid,
        media_id: Uuid,
    ) -> Result<String, ApiError> {
        self.media_asset_url(tree_id, crate::image_host::MediaAsset::File { media_id })
            .await
    }

    /// The drawable form of one held picture, hosted or inlined.
    async fn media_asset_url(
        &self,
        tree_id: Uuid,
        asset: crate::image_host::MediaAsset,
    ) -> Result<String, ApiError> {
        if let Some(host) = &self.image_host
            && let Some(path) = host.path(tree_id, asset)
        {
            return Ok(path);
        }
        self.get_binary_data_url(&crate::image_host::api_path(tree_id, asset))
            .await
    }

    /// Resolve every source that needs no request: remote URLs, and whatever
    /// the shell serving pictures from its own origin can answer for.
    ///
    /// Returns one slot per source, in order, plus the sources left for the
    /// server, each with the slot it has to fill.
    fn resolve_locally(
        &self,
        tree_id: Uuid,
        sources: Vec<ImageSource>,
    ) -> (Vec<Option<String>>, Vec<(usize, ImageSource)>) {
        let mut resolved: Vec<Option<String>> = Vec::with_capacity(sources.len());
        let mut pending: Vec<(usize, ImageSource)> = Vec::new();
        for (index, source) in sources.into_iter().enumerate() {
            if let ImageSource::Remote { url } = source {
                resolved.push(Some(url));
                continue;
            }
            let hosted = crate::image_host::MediaAsset::from_source(&source)
                .zip(self.image_host.as_ref())
                .and_then(|(asset, host)| host.path(tree_id, asset));
            if hosted.is_none() {
                pending.push((index, source));
            }
            resolved.push(hosted);
        }
        (resolved, pending)
    }

    /// Turn a whole screen's picture addresses into things it can draw.
    ///
    /// A shell that serves pictures from its own origin (the desktop) answers
    /// per source with no network at all. Everywhere else the bytes have to be
    /// fetched and inlined as `data:` URLs — and that happens for the whole set
    /// in one request, because a pedigree resolving one portrait at a time is
    /// one round trip per person on screen. A picture fetched earlier in the
    /// session comes from the [`PictureCache`], and one asked for twice on the
    /// same screen is fetched once.
    ///
    /// Returns one slot per source, in order.
    async fn resolve_sources(
        &self,
        tree_id: Uuid,
        sources: Vec<ImageSource>,
    ) -> Vec<Option<String>> {
        let (mut resolved, pending) = self.resolve_locally(tree_id, sources);
        let mut wanted: Vec<ImageSource> = Vec::new();
        let mut slots: HashMap<ImageSource, Vec<usize>> = HashMap::new();
        for (index, source) in pending {
            if let Some(data) = self.pictures.get(tree_id, &source) {
                resolved[index] = Some(data);
                continue;
            }
            slots
                .entry(source.clone())
                .or_insert_with(|| {
                    wanted.push(source);
                    Vec::new()
                })
                .push(index);
        }

        for chunk in wanted.chunks(IMAGE_DATA_BATCH_SIZE) {
            let body = ImageDataRequest {
                sources: chunk.to_vec(),
            };
            match self
                .post_read::<Vec<Option<String>>, _>(
                    &format!("/api/v1/trees/{tree_id}/image-data"),
                    &body,
                )
                .await
            {
                Ok(urls) => {
                    for (source, url) in chunk.iter().zip(urls) {
                        let Some(url) = url else { continue };
                        for &index in slots.get(source).into_iter().flatten() {
                            resolved[index] = Some(url.clone());
                        }
                        self.pictures.set(tree_id, source.clone(), url);
                    }
                }
                Err(error) => {
                    tracing::warn!(
                        error.kind = error.kind(),
                        status = error.status(),
                        count = chunk.len(),
                        "pictures could not be loaded"
                    );
                }
            }
        }
        resolved
    }

    /// Turn a screen's galleries and portraits into things it can draw, in
    /// one request: every address flattened so one call answers for the lot,
    /// then handed back to the slot it came from.
    pub async fn resolve_pictures(
        &self,
        tree_id: Uuid,
        galleries: &[&GallerySources],
        portraits: &[(Uuid, oxidgene_core::types::PortraitRef)],
    ) -> ResolvedPictures {
        let sources = galleries
            .iter()
            .flat_map(|gallery| gallery.sources())
            .chain(
                portraits
                    .iter()
                    .map(|(_, portrait)| portrait.source.clone()),
            )
            .collect();
        let mut drawn = self.resolve_sources(tree_id, sources).await.into_iter();
        let mut gallery = GalleryBundle::default();
        for sources in galleries {
            let resolved = sources.resolve(&mut drawn);
            gallery.media.extend(resolved.media);
            gallery.vignettes.extend(resolved.vignettes);
        }
        let portraits = portraits
            .iter()
            .zip(drawn)
            .filter_map(|((person_id, portrait), source)| {
                Some((
                    *person_id,
                    CroppedSource {
                        source: source?,
                        crop: portrait.crop,
                    },
                ))
            })
            .collect();
        ResolvedPictures {
            gallery: std::sync::Arc::new(gallery),
            portraits,
        }
    }

    /// The portraits of people whose sources a payload already carried —
    /// pedigree nodes, search rows — in one request.
    pub async fn portraits_from_refs(
        &self,
        tree_id: Uuid,
        portraits: &[(Uuid, oxidgene_core::types::PortraitRef)],
    ) -> HashMap<Uuid, CroppedSource> {
        self.resolve_pictures(tree_id, &[], portraits)
            .await
            .portraits
    }

    /// The thumbnails of `media_ids`, drawable, in one request — a
    /// document's pages, which would otherwise be a request per page.
    pub async fn thumbnails(&self, tree_id: Uuid, media_ids: &[Uuid]) -> HashMap<Uuid, String> {
        let sources = media_ids
            .iter()
            .map(|&media_id| ImageSource::Thumbnail { media_id })
            .collect();
        media_ids
            .iter()
            .zip(self.resolve_sources(tree_id, sources).await)
            .filter_map(|(id, source)| Some((*id, source?)))
            .collect()
    }

    /// The portraits of search rows, which carry their sources.
    pub async fn entry_portraits(
        &self,
        tree_id: Uuid,
        entries: &[SearchEntry],
    ) -> HashMap<Uuid, CroppedSource> {
        let refs: Vec<_> = entries
            .iter()
            .filter_map(|entry| Some((entry.person_id, entry.portrait.clone()?)))
            .collect();
        if refs.is_empty() {
            return HashMap::new();
        }
        self.portraits_from_refs(tree_id, &refs).await
    }

    /// The portraits of people known only by id: their sources first, in
    /// bounded batches, then their pictures.
    pub async fn portrait_map_for_ids(
        &self,
        tree_id: Uuid,
        person_ids: &[Uuid],
    ) -> HashMap<Uuid, CroppedSource> {
        let mut portraits = HashMap::new();
        for person_ids in portrait_batches(person_ids) {
            let body = PortraitImagesRequest {
                person_ids: person_ids.to_vec(),
            };
            match self
                .post_read::<Vec<WirePortraitImage>, _>(
                    &format!("/api/v1/trees/{tree_id}/portrait-images"),
                    &body,
                )
                .await
            {
                Ok(images) => {
                    let refs: Vec<_> = images
                        .into_iter()
                        .map(|image| (image.person_id, image.image))
                        .collect();
                    portraits.extend(self.portraits_from_refs(tree_id, &refs).await);
                }
                Err(error) => {
                    tracing::warn!(
                        error.kind = error.kind(),
                        status = error.status(),
                        count = person_ids.len(),
                        "portrait image batch could not be loaded"
                    );
                }
            }
        }
        portraits
    }

    /// Choose what represents a person — a media, a crop of one, or nothing.
    pub async fn set_person_portrait(
        &self,
        tree_id: Uuid,
        person_id: Uuid,
        portrait: SetPortraitBody,
    ) -> Result<serde_json::Value, ApiError> {
        self.put(
            &format!("/api/v1/trees/{tree_id}/persons/{person_id}/portrait"),
            &portrait,
        )
        .await
    }

    /// One media's metadata.
    pub async fn get_media(&self, tree_id: Uuid, media_id: Uuid) -> Result<Media, ApiError> {
        self.get(&format!("/api/v1/trees/{tree_id}/media/{media_id}"))
            .await
    }

    /// Load a generated thumbnail without exposing its API URL.
    pub async fn media_thumbnail_data_url(
        &self,
        tree_id: Uuid,
        media_id: Uuid,
    ) -> Result<String, ApiError> {
        self.media_asset_url(
            tree_id,
            crate::image_host::MediaAsset::Thumbnail { media_id },
        )
        .await
    }

    /// Load a vignette image without exposing its API URL.
    pub async fn vignette_image_data_url(
        &self,
        tree_id: Uuid,
        vignette_id: Uuid,
    ) -> Result<String, ApiError> {
        self.media_asset_url(tree_id, crate::image_host::MediaAsset::Crop { vignette_id })
            .await
    }

    /// Upload a file and record it.
    ///
    /// `attach_to` fills in an existing record that named a file without
    /// holding it — the state every GEDCOM import leaves behind — instead of
    /// creating a new one.
    pub async fn upload_media(
        &self,
        tree_id: Uuid,
        upload: MediaUpload,
    ) -> Result<Media, ApiError> {
        let path = format!("/api/v1/trees/{tree_id}/media/upload");
        let url = self.url(&path);
        let MediaUpload {
            file_name,
            bytes,
            title,
            description,
            attach_to,
            as_page_of,
        } = upload;
        tracing::debug!(
            method = "POST",
            bytes = bytes.len(),
            "API media upload started"
        );

        let part = reqwest::multipart::Part::bytes(bytes).file_name(file_name);
        let mut form = reqwest::multipart::Form::new().part("file", part);
        if let Some(title) = title.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
            form = form.text("title", title.to_string());
        }
        if let Some(description) = description
            .as_deref()
            .map(str::trim)
            .filter(|d| !d.is_empty())
        {
            form = form.text("description", description.to_string());
        }
        if let Some(media_id) = attach_to {
            form = form.text("media_id", media_id.to_string());
        }
        if let Some(document_id) = as_page_of {
            form = form.text("document_id", document_id.to_string());
        }

        let resp = self
            .send_write("POST", &path, self.client.post(&url).multipart(form))
            .await?;
        Self::handle_response("POST", resp).await
    }

    /// Add a page that names a file without holding its bytes.
    ///
    /// The bytes-carrying counterpart of [`ApiClient::upload_media`]; both
    /// append a page to an existing document.
    pub async fn create_media(
        &self,
        tree_id: Uuid,
        body: &CreateMediaBody,
    ) -> Result<Media, ApiError> {
        self.post(&format!("/api/v1/trees/{tree_id}/media"), body)
            .await
    }

    /// Create an empty multi-page document.
    ///
    /// Pages are added by uploading images with `document_id` set; the
    /// document itself holds the title, date, place, description and note that
    /// describe the whole thing.
    pub async fn create_media_document(
        &self,
        tree_id: Uuid,
        title: Option<&str>,
    ) -> Result<Media, ApiError> {
        self.post(
            &format!("/api/v1/trees/{tree_id}/media/document"),
            &serde_json::json!({ "title": title }),
        )
        .await
    }

    /// A page of the tree's documents matching `filters`, each with its
    /// usage count.
    pub async fn list_media(
        &self,
        tree_id: Uuid,
        first: u64,
        after: Option<&str>,
        filters: &MediaListFilters,
    ) -> Result<PaginatedResponse<MediaListItem>, ApiError> {
        let mut params = vec![("first", first.to_string())];
        if let Some(after) = after {
            params.push(("after", after.to_string()));
        }
        params.extend(filters.query_pairs());
        self.get_with_query(&format!("/api/v1/trees/{tree_id}/media"), &params)
            .await
    }

    /// The tags, file kinds and categories the tree's documents carry; the
    /// tags counted among the documents carrying every tag of `with_tags`.
    pub async fn media_facets(
        &self,
        tree_id: Uuid,
        with_tags: &[String],
    ) -> Result<MediaFacets, ApiError> {
        let params: Vec<(&str, String)> =
            with_tags.iter().map(|tag| ("tag", tag.clone())).collect();
        self.get_with_query(&format!("/api/v1/trees/{tree_id}/media/facets"), &params)
            .await
    }

    /// The pages of a document, in order.
    pub async fn list_media_pages(
        &self,
        tree_id: Uuid,
        media_id: Uuid,
    ) -> Result<Vec<Media>, ApiError> {
        self.get(&format!("/api/v1/trees/{tree_id}/media/{media_id}/pages"))
            .await
    }

    /// Set a document's page order. Must name exactly its pages, once each.
    pub async fn reorder_media_pages(
        &self,
        tree_id: Uuid,
        media_id: Uuid,
        page_ids: &[Uuid],
    ) -> Result<Vec<Media>, ApiError> {
        self.put(
            &format!("/api/v1/trees/{tree_id}/media/{media_id}/pages"),
            &serde_json::json!({ "page_ids": page_ids }),
        )
        .await
    }

    /// Delete a document page and its external relations.
    pub async fn delete_media_page(
        &self,
        tree_id: Uuid,
        media_id: Uuid,
        page_id: Uuid,
    ) -> Result<(), ApiError> {
        self.delete_no_content(&format!(
            "/api/v1/trees/{tree_id}/media/{media_id}/pages/{page_id}"
        ))
        .await
    }

    /// Update a media's title and description.
    pub async fn update_media(
        &self,
        tree_id: Uuid,
        media_id: Uuid,
        body: &UpdateMediaBody,
    ) -> Result<Media, ApiError> {
        self.put(&format!("/api/v1/trees/{tree_id}/media/{media_id}"), body)
            .await
    }

    /// Add one media tag without replacing the other tags.
    pub async fn add_media_tag(
        &self,
        tree_id: Uuid,
        media_id: Uuid,
        tag: String,
    ) -> Result<Media, ApiError> {
        self.post(
            &format!("/api/v1/trees/{tree_id}/media/{media_id}/tags"),
            &MediaTagBody { tag },
        )
        .await
    }

    /// Remove one media tag without replacing the other tags.
    pub async fn remove_media_tag(
        &self,
        tree_id: Uuid,
        media_id: Uuid,
        tag: String,
    ) -> Result<(), ApiError> {
        self.delete_no_content(&format!(
            "/api/v1/trees/{tree_id}/media/{media_id}/tags/{}",
            path_segment(&tag)
        ))
        .await
    }

    /// Permanently delete a media record and its associated information.
    pub async fn delete_media(&self, tree_id: Uuid, media_id: Uuid) -> Result<(), ApiError> {
        self.delete_no_content(&format!("/api/v1/trees/{tree_id}/media/{media_id}"))
            .await
    }

    /// Delete a media only when the supplied gallery link is its sole external
    /// reference. Returns `false` when another reference keeps it alive.
    pub async fn delete_media_if_unreferenced_elsewhere(
        &self,
        tree_id: Uuid,
        media_id: Uuid,
        allowed_link_id: Uuid,
    ) -> Result<bool, ApiError> {
        let status = self
            .delete_status(&format!(
                "/api/v1/trees/{tree_id}/media/{media_id}?only_if_unreferenced_elsewhere=true&allowed_link_id={allowed_link_id}"
            ))
            .await?;
        Ok(status == 204)
    }

    /// Whether the supplied gallery link is the media's sole external
    /// reference, and therefore whether showing a definitive-delete
    /// confirmation is truthful.
    pub async fn can_delete_media_if_unreferenced_elsewhere(
        &self,
        tree_id: Uuid,
        media_id: Uuid,
        allowed_link_id: Uuid,
    ) -> Result<bool, ApiError> {
        let status: MediaDeletionStatus = self
            .get(&format!(
                "/api/v1/trees/{tree_id}/media/{media_id}/deletion-status?allowed_link_id={allowed_link_id}"
            ))
            .await?;
        Ok(status.can_delete)
    }

    // ── MediaLinks ──────────────────────────────────────────────────

    /// Load gallery thumbnails, mosaics, crops and event links in bounded batches.
    pub async fn gallery_bundle(
        &self,
        tree_id: Uuid,
        media_ids: &[Uuid],
        vignette_ids: &[Uuid],
    ) -> GalleryBundle {
        const BATCH_SIZE: usize = 1_024;

        let mut bundle = GalleryBundle::default();
        let (mut media_offset, mut vignette_offset) = (0, 0);
        while media_offset < media_ids.len() || vignette_offset < vignette_ids.len() {
            let media_end = (media_offset + BATCH_SIZE).min(media_ids.len());
            let remaining = BATCH_SIZE - (media_end - media_offset);
            let vignette_end = (vignette_offset + remaining).min(vignette_ids.len());
            let body = GalleryBundleRequest {
                media_ids: media_ids[media_offset..media_end].to_vec(),
                vignette_ids: vignette_ids[vignette_offset..vignette_end].to_vec(),
            };
            match self
                .post_read::<GallerySources, _>(
                    &format!("/api/v1/trees/{tree_id}/gallery-bundle"),
                    &body,
                )
                .await
            {
                Ok(batch) => {
                    let resolved = self.resolve_pictures(tree_id, &[&batch], &[]).await;
                    let batch = std::sync::Arc::unwrap_or_clone(resolved.gallery);
                    bundle.media.extend(batch.media);
                    bundle.vignettes.extend(batch.vignettes);
                }
                Err(error) => {
                    tracing::warn!(
                        error.kind = error.kind(),
                        status = error.status(),
                        "gallery bundle could not be loaded"
                    );
                }
            }
            media_offset = media_end;
            vignette_offset = vignette_end;
        }
        bundle
    }

    /// Every media attached to one entity — a person, a family, an event or a
    /// source — with the link that attached it.
    pub async fn list_entity_media(
        &self,
        tree_id: Uuid,
        entity_type: &str,
        entity_id: Uuid,
    ) -> Result<Vec<MediaWithLink>, ApiError> {
        self.get(&format!(
            "/api/v1/trees/{tree_id}/media-links?entity_type={entity_type}&entity_id={entity_id}"
        ))
        .await
    }

    /// Everything one media file is attached to.
    ///
    /// The other direction from [`Self::list_entity_media`]: what lets a
    /// media's own panel say which events it documents.
    pub async fn list_media_links_of(
        &self,
        tree_id: Uuid,
        media_id: Uuid,
    ) -> Result<Vec<oxidgene_core::types::MediaLink>, ApiError> {
        self.get(&format!(
            "/api/v1/trees/{tree_id}/media-links?media_id={media_id}"
        ))
        .await
    }

    /// Attach a media to an entity.
    pub async fn create_media_link(
        &self,
        tree_id: Uuid,
        body: &CreateMediaLinkBody,
    ) -> Result<serde_json::Value, ApiError> {
        self.post(&format!("/api/v1/trees/{tree_id}/media-links"), body)
            .await
    }

    /// Detach a media from an entity. The media itself is untouched.
    pub async fn delete_media_link(&self, tree_id: Uuid, link_id: Uuid) -> Result<(), ApiError> {
        self.delete_no_content(&format!("/api/v1/trees/{tree_id}/media-links/{link_id}"))
            .await
    }

    // ── Vignettes ───────────────────────────────────────────────────

    /// Every crop recorded on a media file, in page order.
    pub async fn list_media_vignettes(
        &self,
        tree_id: Uuid,
        media_id: Uuid,
    ) -> Result<Vec<Vignette>, ApiError> {
        self.get(&format!(
            "/api/v1/trees/{tree_id}/media/{media_id}/vignettes"
        ))
        .await
    }

    /// Crops attributed to a person.
    pub async fn list_person_vignettes(
        &self,
        tree_id: Uuid,
        person_id: Uuid,
    ) -> Result<Vec<Vignette>, ApiError> {
        self.get(&format!(
            "/api/v1/trees/{tree_id}/vignettes?person_id={person_id}"
        ))
        .await
    }

    pub async fn create_vignette(
        &self,
        tree_id: Uuid,
        media_id: Uuid,
        body: &CreateVignetteBody,
    ) -> Result<Vignette, ApiError> {
        self.post(
            &format!("/api/v1/trees/{tree_id}/media/{media_id}/vignettes"),
            body,
        )
        .await
    }

    pub async fn update_vignette(
        &self,
        tree_id: Uuid,
        vignette_id: Uuid,
        body: &UpdateVignetteBody,
    ) -> Result<Vignette, ApiError> {
        self.put(
            &format!("/api/v1/trees/{tree_id}/vignettes/{vignette_id}"),
            body,
        )
        .await
    }

    pub async fn delete_vignette(&self, tree_id: Uuid, vignette_id: Uuid) -> Result<(), ApiError> {
        self.delete_no_content(&format!("/api/v1/trees/{tree_id}/vignettes/{vignette_id}"))
            .await
    }

    // ── Import / export ─────────────────────────────────────────────

    /// Absolute endpoint used by the browser's XHR upload.
    pub fn file_import_upload_url(&self, tree_id: Uuid) -> String {
        self.url(&format!("/api/v1/trees/{tree_id}/import-jobs"))
    }

    /// Stream a native file to durable import-job storage.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn start_file_import(
        &self,
        tree_id: Uuid,
        format: &str,
        filename: Option<String>,
        body: reqwest::Body,
    ) -> Result<ImportJobStarted, ApiError> {
        let mut query = vec![("format", format.to_string())];
        if let Some(filename) = filename {
            query.push(("filename", filename));
        }
        let response = self
            .send_request(
                "POST",
                self.client
                    .post(self.file_import_upload_url(tree_id))
                    .query(&query)
                    .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
                    .body(body),
            )
            .await?;
        Self::handle_response("POST", response).await
    }

    /// Poll a file import without caching its deliberately changing response.
    pub async fn file_import_status(
        &self,
        tree_id: Uuid,
        job_id: Uuid,
    ) -> Result<FileImportJobStatus, ApiError> {
        let response = self
            .send_request(
                "GET",
                self.client
                    .get(self.url(&format!("/api/v1/trees/{tree_id}/import-jobs/{job_id}"))),
            )
            .await?;
        let response = Self::require_success(response).await?;
        Ok(response.json().await?)
    }

    // ── Geneanet import wizard ──────────────────────────────────────

    /// Parse a `.gw` and report what it holds, writing nothing. Step 1.
    ///
    /// Runs on every selection because it costs nothing and is the first
    /// moment the user learns whether they picked the right export — a `.ged`
    /// fails here rather than four steps later.
    pub async fn inspect_geneweb(
        &self,
        content: Vec<u8>,
        file_name: &str,
    ) -> Result<GwInspection, ApiError> {
        let query = [("filename", file_name.to_string())];
        self.post_bytes("/api/v1/geneweb/inspect", content, &query)
            .await
    }

    /// Index the named data archives in place, extracting nothing. Step 2.
    ///
    /// Desktop only: it sends **paths**, which is sound because there the
    /// server runs in-process on the same filesystem the user picked from.
    pub async fn index_geneanet_archives(
        &self,
        paths: Vec<String>,
    ) -> Result<ArchiveIndex, ApiError> {
        self.post_read("/api/v1/geneanet/archives", &IndexArchivesBody { paths })
            .await
    }

    /// Join the collected mapping onto the `.gw` and report what an import
    /// would do, without doing it. Step 4.
    pub async fn preview_geneanet_import(
        &self,
        body: &GeneanetPreviewBody,
    ) -> Result<GeneanetPreview, ApiError> {
        self.post_read("/api/v1/geneanet/preview", body).await
    }

    /// Encode a collected session as the JSON the wizard writes to disk.
    ///
    /// Done server-side so the file format lives in one place — the same
    /// module the loader validates against — rather than being assembled by
    /// hand in the UI.
    pub async fn encode_geneanet_session(
        &self,
        body: &GeneanetSessionBody,
    ) -> Result<Vec<u8>, ApiError> {
        // The archive itself, not JSON around it: the wizard writes these
        // bytes straight to the file the user chose, and wrapping a ZIP in
        // JSON would only base64 it again — the very thing the container
        // exists to stop.
        let url = self.url("/api/v1/geneanet/session/encode");
        let resp = self
            .send_request("POST", self.client.post(&url).json(body))
            .await?;
        let resp = Self::require_success(resp).await?;
        Self::read_response_body(resp).await.map_err(ApiError::from)
    }

    /// Read a saved session back, checking it really is one.
    pub async fn decode_geneanet_session(
        &self,
        body: reqwest::Body,
    ) -> Result<GeneanetSession, ApiError> {
        let response = self
            .send_request(
                "POST",
                self.client
                    .post(self.url("/api/v1/geneanet/session/decode"))
                    .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
                    .body(body),
            )
            .await?;
        Self::handle_response("POST", response).await
    }

    /// Let the backend delete the photos a decoded session staged, once the
    /// wizard no longer needs them. Paths it did not stage are ignored.
    pub async fn release_geneanet_session_media(&self, paths: Vec<String>) -> Result<(), ApiError> {
        self.post_no_content(
            "/api/v1/geneanet/session/release",
            &serde_json::json!({ "paths": paths }),
        )
        .await
    }

    /// Ask what the login window has to fetch before an import can run.
    ///
    /// The server never reaches Geneanet — every direct request is challenged
    /// whatever the cookie — so anything it cannot find in the local archives
    /// has to come through the window the user signed in to.
    pub async fn plan_geneanet_import(
        &self,
        body: &GeneanetPreviewBody,
    ) -> Result<GeneanetPlan, ApiError> {
        self.post_read("/api/v1/geneanet/plan", body).await
    }

    /// Stage every local input and queue the Geneanet import. Step 5.
    pub async fn import_geneanet(
        &self,
        tree_id: Uuid,
        body: &GeneanetImportBody,
    ) -> Result<ImportJobStarted, ApiError> {
        self.post(&format!("/api/v1/trees/{tree_id}/geneanet/import"), body)
            .await
    }

    /// Export the tree as GEDCOM text, made as `choices` say.
    pub async fn export_gedcom(
        &self,
        tree_id: Uuid,
        choices: ExportChoices,
    ) -> Result<ExportGedcomResult, ApiError> {
        self.get_with_query(&format!("/api/v1/trees/{tree_id}/gedcom/export"), &choices)
            .await
    }

    /// Queue a GEDZIP export made as `choices` say, without holding the HTTP
    /// request while it is built.
    pub async fn start_export_job(
        &self,
        tree_id: Uuid,
        choices: ExportChoices,
    ) -> Result<ExportJobStarted, ApiError> {
        let response = self
            .send_request(
                "POST",
                self.client
                    .post(self.url(&format!("/api/v1/trees/{tree_id}/export-jobs")))
                    .query(&choices),
            )
            .await?;
        Self::handle_response("POST", response).await
    }

    /// Poll an export job without caching its changing response.
    pub async fn export_job_status(
        &self,
        tree_id: Uuid,
        job_id: Uuid,
    ) -> Result<ExportJobStatus, ApiError> {
        let response = self
            .send_request(
                "GET",
                self.client
                    .get(self.url(&format!("/api/v1/trees/{tree_id}/export-jobs/{job_id}"))),
            )
            .await?;
        Self::handle_response("GET", response).await
    }

    /// The tree's most recent export that can still be downloaded, if any;
    /// never cached, since it expires.
    pub async fn downloadable_export(
        &self,
        tree_id: Uuid,
    ) -> Result<Option<DownloadableExport>, ApiError> {
        let response = self
            .send_request(
                "GET",
                self.client
                    .get(self.url(&format!("/api/v1/trees/{tree_id}/export-jobs/downloadable"))),
            )
            .await?;
        Self::handle_response("GET", response).await
    }

    fn download_url(&self, path: &str) -> String {
        if oxidgene_core::types::is_remote_url(path) {
            path.to_string()
        } else {
            self.url(path)
        }
    }

    /// Keep browser response bytes outside WASM and the JSON evaluation bridge.
    #[cfg(target_arch = "wasm32")]
    pub(crate) async fn download_in_browser(
        &self,
        download: BrowserDownload,
        path: &str,
    ) -> Result<(), ApiError> {
        download
            .eval
            .send(self.download_url(path))
            .map_err(|_| BrowserDownload::error())?;
        match download.eval.join::<String>().await.as_deref() {
            Ok("saved") => Ok(()),
            _ => Err(BrowserDownload::error()),
        }
    }

    /// Stream a download to disk, replacing the destination only after success.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn download_to_file(
        &self,
        path: &str,
        destination: &std::path::Path,
    ) -> Result<(), ApiError> {
        use tokio::io::AsyncWriteExt;

        let response = self
            .send_request("GET", self.client.get(self.download_url(path)))
            .await?;
        let mut response = Self::require_success(response).await?;
        let parent = destination
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| std::path::Path::new("."));
        let pending = tempfile::NamedTempFile::new_in(parent)?;
        let mut file = tokio::fs::File::from_std(pending.reopen()?);
        while let Some(chunk) = response.chunk().await? {
            file.write_all(&chunk).await?;
        }
        file.flush().await?;
        drop(file);
        pending.persist(destination).map_err(|error| error.error)?;
        Ok(())
    }

    // ── Pedigrees ───────────────────────────────────────────────────

    /// Assemble several pedigrees in one operation, in bounded batches.
    ///
    /// A screen that draws one small pedigree per row asks for the whole page
    /// at once: a request per row is both slower and drowns the load trace in
    /// one resource per row.
    pub async fn get_pedigrees(
        &self,
        tree_id: Uuid,
        root_person_ids: &[Uuid],
        ancestor_depth: u32,
        descendant_depth: u32,
    ) -> HashMap<Uuid, Pedigree> {
        let mut pedigrees = HashMap::new();
        for roots in root_person_ids.chunks(PEDIGREE_BATCH_SIZE) {
            let body = PedigreesRequest {
                root_person_ids: roots.to_vec(),
                ancestor_depth,
                descendant_depth,
            };
            match self
                .post_read::<Vec<PedigreeEntry>, _>(
                    &format!("/api/v1/trees/{tree_id}/pedigrees"),
                    &body,
                )
                .await
            {
                Ok(entries) => pedigrees.extend(
                    entries
                        .into_iter()
                        .map(|entry| (entry.root_person_id, entry.pedigree)),
                ),
                Err(error) => {
                    tracing::warn!(
                        error.kind = error.kind(),
                        status = error.status(),
                        count = roots.len(),
                        "pedigree batch could not be loaded"
                    );
                }
            }
        }
        pedigrees
    }

    /// Fetch a windowed pedigree for a root person.
    ///
    /// Assembled server-side from family links and the stored person
    /// projections on every call.
    pub async fn get_pedigree(
        &self,
        tree_id: Uuid,
        root_person_id: Uuid,
        ancestor_depth: u32,
        descendant_depth: u32,
    ) -> Result<Pedigree, ApiError> {
        let params = [
            ("ancestor_depth", ancestor_depth.to_string()),
            ("descendant_depth", descendant_depth.to_string()),
        ];
        self.get_with_query(
            &format!("/api/v1/trees/{tree_id}/pedigree/{root_person_id}"),
            &params,
        )
        .await
    }

    /// The pedigree around the tree's default root — its SOSA root, else its
    /// first person — chosen by the server, so nothing has to be read first.
    pub async fn get_default_pedigree(
        &self,
        tree_id: Uuid,
        ancestor_depth: u32,
        descendant_depth: u32,
    ) -> Result<Pedigree, ApiError> {
        let params = [
            ("ancestor_depth", ancestor_depth.to_string()),
            ("descendant_depth", descendant_depth.to_string()),
        ];
        self.get_with_query(&format!("/api/v1/trees/{tree_id}/pedigree"), &params)
            .await
    }

    /// A tree's statistics, time series filed by year; `approximate` lets
    /// ages and averages use approximate dates, and places are named in
    /// `lang`.
    pub async fn tree_statistics(
        &self,
        tree_id: Uuid,
        approximate: bool,
        lang: &str,
    ) -> Result<TreeStatistics, ApiError> {
        self.get_with_query(
            &format!("/api/v1/trees/{tree_id}/statistics"),
            &[
                ("approximate", approximate.to_string()),
                ("lang", lang.to_string()),
            ],
        )
        .await
    }

    /// How many persons a tree held over the days it was worked on.
    pub async fn tree_growth(&self, tree_id: Uuid) -> Result<TreeGrowth, ApiError> {
        self.get(&format!("/api/v1/trees/{tree_id}/statistics/growth"))
            .await
    }

    /// The pairs of a tree's records that may be one person, best first.
    pub async fn potential_duplicates(
        &self,
        tree_id: Uuid,
    ) -> Result<PotentialDuplicates, ApiError> {
        self.get(&format!("/api/v1/trees/{tree_id}/duplicates"))
            .await
    }

    /// A tree's anomalies, by rule.
    pub async fn tree_anomalies(&self, tree_id: Uuid) -> Result<TreeAnomalies, ApiError> {
        self.get(&format!("/api/v1/trees/{tree_id}/anomalies"))
            .await
    }

    /// The places of a tree the statistics cannot locate, most used first.
    pub async fn unlocated_places(&self, tree_id: Uuid) -> Result<Vec<StatPlace>, ApiError> {
        self.get(&format!("/api/v1/trees/{tree_id}/unlocated-places"))
            .await
    }

    /// A tree's ancestry completeness from its SOSA root over `generations`
    /// generations, the root's included.
    pub async fn ancestry_completeness(
        &self,
        tree_id: Uuid,
        generations: u32,
    ) -> Result<AncestryCompleteness, ApiError> {
        self.get_with_query(
            &format!("/api/v1/trees/{tree_id}/ancestry-completeness"),
            &[("generations", generations.to_string())],
        )
        .await
    }

    /// The country outlines the statistics heat map is drawn over.
    pub async fn basemap(&self) -> Result<Vec<BasemapCountry>, ApiError> {
        self.get("/api/v1/reference/basemap").await
    }

    /// What `field` suggests for `query`: the tree's values, then the
    /// reference sheets' terms. A name field may be scoped to the persons
    /// a surname or given-names search filter finds (`docs/api.md`).
    pub async fn value_suggestions(
        &self,
        tree_id: Uuid,
        field: SuggestionField,
        lang: &str,
        query: &str,
        limit: usize,
        scope: &NameScope,
    ) -> Result<Vec<ValueSuggestion>, ApiError> {
        let mut params = vec![
            ("q", query.to_string()),
            ("lang", lang.to_string()),
            ("limit", limit.to_string()),
        ];
        for (name, value) in [
            ("surname", &scope.surname),
            ("given_names", &scope.given_names),
        ] {
            if !value.trim().is_empty() {
                params.push((name, value.trim().to_string()));
            }
        }
        self.get_with_query(
            &format!("/api/v1/trees/{tree_id}/suggestions/{}", field.path()),
            &params,
        )
        .await
    }

    /// Places from the place dictionary matching `query`, best first.
    pub async fn place_suggestions(
        &self,
        lang: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<PlaceSuggestion>, ApiError> {
        self.get_with_query(
            &format!("/api/v1/reference/{lang}/places"),
            &[("q", query.to_string()), ("limit", limit.to_string())],
        )
        .await
    }

    /// Given-name references in bounded batches, issuing as many requests as
    /// needed when `terms` exceeds the server limit.
    pub async fn reference_given_names(
        &self,
        lang: &str,
        terms: &[String],
    ) -> Result<Vec<GivenNameReferenceMatch>, ApiError> {
        let mut matches = Vec::new();
        for terms in reference_term_batches(terms) {
            let mut batch = self
                .post_read::<Vec<GivenNameReferenceMatch>, _>(
                    &format!("/api/v1/reference/{lang}/given-names/bundle"),
                    &ReferenceTermsBody { terms },
                )
                .await?;
            matches.append(&mut batch);
        }
        Ok(matches)
    }

    /// Occupation references in bounded batches, issuing as many requests as
    /// needed when `terms` exceeds the server limit.
    pub async fn reference_occupations(
        &self,
        lang: &str,
        terms: &[String],
    ) -> Result<Vec<OccupationReferenceMatch>, ApiError> {
        let mut matches = Vec::new();
        for terms in reference_term_batches(terms) {
            let mut batch = self
                .post_read::<Vec<OccupationReferenceMatch>, _>(
                    &format!("/api/v1/reference/{lang}/occupations/bundle"),
                    &ReferenceTermsBody { terms },
                )
                .await?;
            matches.append(&mut batch);
        }
        Ok(matches)
    }
}

#[cfg(feature = "telemetry-client")]
struct HeaderInjector<'a>(&'a mut reqwest::header::HeaderMap);

#[cfg(feature = "telemetry-client")]
impl Injector for HeaderInjector<'_> {
    fn set(&mut self, key: &str, value: String) {
        if let (Ok(name), Ok(value)) = (
            reqwest::header::HeaderName::from_bytes(key.as_bytes()),
            reqwest::header::HeaderValue::from_str(&value),
        ) {
            self.0.insert(name, value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_api_error_reads_its_envelope_code() {
        let error = ApiError::Api {
            status: 504,
            body: r#"{"error":"timeout","message":"The archive portal did not answer in time"}"#
                .to_owned(),
        };
        assert_eq!(error.code().as_deref(), Some("timeout"));
        let plain = ApiError::Api {
            status: 502,
            body: "Bad Gateway".to_owned(),
        };
        assert_eq!(plain.code(), None);
    }

    #[test]
    fn a_kept_picture_is_found_until_its_tree_is_written_to() {
        let cache = PictureCache::default();
        let (tree, other) = (Uuid::now_v7(), Uuid::now_v7());
        let source = ImageSource::Thumbnail {
            media_id: Uuid::now_v7(),
        };
        cache.set(tree, source.clone(), "data:image/jpeg;base64,AAAA".into());
        cache.set(other, source.clone(), "data:image/jpeg;base64,BBBB".into());
        assert_eq!(
            cache.get(tree, &source).as_deref(),
            Some("data:image/jpeg;base64,AAAA")
        );
        cache.invalidate_tree(tree);
        assert_eq!(cache.get(tree, &source), None);
        assert!(cache.get(other, &source).is_some());
    }

    #[test]
    fn the_picture_cache_drops_its_oldest_pictures_past_its_size() {
        let cache = PictureCache::default();
        let tree = Uuid::now_v7();
        let picture = "x".repeat(PICTURE_CACHE_MAX_BYTES / 5);
        let sources: Vec<ImageSource> = (0..8)
            .map(|_| ImageSource::Thumbnail {
                media_id: Uuid::now_v7(),
            })
            .collect();
        for source in &sources {
            cache.set(tree, source.clone(), picture.clone());
        }
        let kept = sources
            .iter()
            .filter(|source| cache.get(tree, source).is_some())
            .count();
        assert_eq!(kept, 5);
        assert!(cache.0.lock().unwrap().bytes <= PICTURE_CACHE_MAX_BYTES);
        // A picture too large to be worth keeping is not kept.
        let huge = ImageSource::Thumbnail {
            media_id: Uuid::now_v7(),
        };
        cache.set(tree, huge.clone(), "x".repeat(PICTURE_CACHE_MAX_BYTES / 2));
        assert_eq!(cache.get(tree, &huge), None);
    }

    #[test]
    fn a_gallery_takes_its_pictures_back_in_the_order_it_gave_its_addresses() {
        let (page, preview, vignette) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
        let sources: GallerySources = serde_json::from_value(serde_json::json!({
            "media": [{
                "media_id": page,
                "source": { "kind": "thumbnail", "media_id": page },
                "event_ids": [],
                "document_previews": [{ "kind": "thumbnail", "media_id": preview }]
            }],
            "vignettes": [{
                "vignette_id": vignette,
                "source": { "kind": "crop", "vignette_id": vignette }
            }]
        }))
        .unwrap();
        assert_eq!(sources.sources().count(), 3);
        let mut drawn = ["a", "b", "c"].map(|s| Some(s.to_string())).into_iter();
        let gallery = sources.resolve(&mut drawn);
        assert_eq!(gallery.media[0].source.as_deref(), Some("a"));
        assert_eq!(gallery.media[0].document_previews, ["b"]);
        assert_eq!(gallery.vignettes[0].image.source, "c");
    }

    #[test]
    fn a_path_segment_escapes_everything_that_could_end_or_alter_it() {
        assert_eq!(path_segment("Civil record"), "Civil%20record");
        assert_eq!(path_segment("1914/1918"), "1914%2F1918");
        assert_eq!(path_segment("100%"), "100%25");
        assert_eq!(path_segment("Église"), "%C3%89glise");
        assert_eq!(path_segment("a-b_c.d~e"), "a-b_c.d~e");
    }

    #[test]
    fn a_logged_request_shows_its_route_but_not_its_query_or_ids() {
        let tree = Uuid::now_v7();
        let person = Uuid::now_v7();
        assert_eq!(
            route_template(&format!(
                r#"/api/v1/trees/{tree}/persons/search?{{"q":"Name A"}}"#
            )),
            "/api/v1/trees/{id}/persons/search"
        );
        assert_eq!(
            route_template(&format!("/api/v1/trees/{tree}/persons/{person}/history")),
            "/api/v1/trees/{id}/persons/{id}/history"
        );
        assert_eq!(
            route_template(&format!("/api/v1/trees/{tree}/persons/sosa/12")),
            "/api/v1/trees/{id}/persons/sosa/{id}"
        );
        assert_eq!(
            route_template("/api/v1/reference/fr/places"),
            "/api/v1/reference/fr/places"
        );
        assert_eq!(route_template("/api/v1/trees"), "/api/v1/trees");
    }

    #[test]
    fn an_error_is_logged_by_its_kind_and_status_only() {
        let error = ApiError::Api {
            status: 404,
            body: r#"{"message":"Name A not found"}"#.to_string(),
        };
        assert_eq!(error.kind(), "api");
        assert_eq!(error.status(), Some(404));
        let error = ApiError::Json(serde_json::from_str::<u8>("x").unwrap_err());
        assert_eq!(error.kind(), "json");
        assert_eq!(error.status(), None);
    }

    #[test]
    fn a_document_is_recognised_by_its_marker_and_never_spelled_out() {
        // Read as a generic file it lands in `Other`, whose glyph is a folder —
        // which is what an imported photograph drew — and whose badge spells
        // out an internal name nobody outside the codebase has heard of.
        assert_eq!(media_kind(DOCUMENT_MIME), MediaKind::Document);
        assert_eq!(media_kind_label(DOCUMENT_MIME), "DOCUMENT");
        assert_eq!(media_kind("image/jpeg"), MediaKind::Image);
        assert_eq!(media_kind_label("image/jpeg"), "JPEG");
        assert_eq!(media_kind_label("application/pdf"), "PDF");
        assert_eq!(media_kind_label("image/svg+xml"), "SVG");
    }

    #[test]
    fn a_media_is_stored_remote_or_held_by_nobody() {
        let mut media: Media = serde_json::from_value(serde_json::json!({
            "id": Uuid::from_u128(1),
            "tree_id": Uuid::from_u128(2),
            "file_name": "scan.jpg",
            "file_path": "media/scan.jpg",
            "mime_type": "image/jpeg",
            "page_count": 1,
            "file_size": 0,
            "created_at": "2000-01-01T00:00:00Z",
            "updated_at": "2000-01-01T00:00:00Z"
        }))
        .unwrap();
        assert_eq!(media_source(&media), MediaSource::Unheld);
        media.file_path = "https://archives.example.invalid/scan.jpg".to_string();
        assert_eq!(media_source(&media), MediaSource::Remote);
        media.storage_key = Some("tree/scan.jpg".to_string());
        assert_eq!(
            media_source(&media),
            MediaSource::Stored,
            "our own copy wins: the URL is then only where it came from"
        );
    }

    /// A one-request HTTP server on a free local port, for the tests that
    /// need the client to really send something.
    #[cfg(not(target_arch = "wasm32"))]
    mod test_server {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
        use tokio::net::{TcpListener, TcpStream};

        pub async fn listen() -> (TcpListener, String) {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = format!("http://{}", listener.local_addr().unwrap());
            (listener, address)
        }

        /// Accepts one connection and reads its request line and headers.
        pub async fn accept(listener: &TcpListener) -> (TcpStream, String) {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut head = String::new();
            let mut reader = tokio::io::BufReader::new(&mut socket);
            while !head.ends_with("\r\n\r\n") {
                assert!(reader.read_line(&mut head).await.unwrap() > 0);
            }
            (socket, head)
        }

        /// Answers one request with `response`, returning the request's
        /// line and headers.
        pub async fn serve_once(listener: &TcpListener, response: &[u8]) -> String {
            let (mut socket, head) = accept(listener).await;
            socket.write_all(response).await.unwrap();
            head
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn deleting_a_page_accepts_no_content_and_invalidates_the_tree() {
        let (listener, address) = test_server::listen().await;
        let api = ApiClient::new(&address);
        let tree = Uuid::now_v7();
        let document = Uuid::now_v7();
        let page = Uuid::now_v7();
        let cache_key = format!("/api/v1/trees/{tree}/media");
        api.cache.set(cache_key.clone(), b"cached".to_vec());
        let (result, request) = tokio::join!(
            api.delete_media_page(tree, document, page),
            test_server::serve_once(&listener, b"HTTP/1.1 204 No Content\r\n\r\n")
        );
        result.unwrap();
        assert!(request.starts_with(&format!(
            "DELETE /api/v1/trees/{tree}/media/{document}/pages/{page} "
        )));
        assert!(api.cache.get(&cache_key).is_none());
    }

    #[test]
    fn the_access_token_is_readable_only_when_one_was_given() {
        let api = ApiClient::new("http://127.0.0.1:1");
        assert_eq!(api.auth_token(), None);
        let api = api.with_auth_token("s3cret");
        assert_eq!(api.auth_token().as_deref(), Some("s3cret"));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn the_access_token_goes_to_the_backend_and_nowhere_else() {
        const EMPTY_OK: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n";

        let directory = tempfile::tempdir().unwrap();
        let (backend, backend_address) = test_server::listen().await;
        let (remote, remote_address) = test_server::listen().await;
        let api = ApiClient::new(&backend_address).with_auth_token("s3cret");

        let local = directory.path().join("a");
        let (result, request) = tokio::join!(
            api.download_to_file("/api/v1/x", &local),
            test_server::serve_once(&backend, EMPTY_OK)
        );
        result.unwrap();
        assert!(
            request
                .to_ascii_lowercase()
                .contains("authorization: bearer s3cret")
        );

        let remote_url = format!("{remote_address}/scan.jpg");
        let fetched = directory.path().join("b");
        let (result, request) = tokio::join!(
            api.download_to_file(&remote_url, &fetched),
            test_server::serve_once(&remote, EMPTY_OK)
        );
        result.unwrap();
        let request = request.to_ascii_lowercase();
        assert!(!request.contains("authorization"));
        assert!(!request.contains("s3cret"));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn downloads_write_chunks_to_disk_before_the_response_finishes() {
        use tokio::io::AsyncWriteExt;

        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("download.bin");
        let (listener, address) = test_server::listen().await;
        let api = ApiClient::new(&address);
        let server = async {
            let (mut socket, _) = test_server::accept(&listener).await;
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\n\r\npart")
                .await
                .unwrap();

            // The second chunk is withheld until the first reaches disk. A
            // response.bytes() implementation would deadlock until timeout.
            loop {
                let mut files = tokio::fs::read_dir(directory.path()).await.unwrap();
                if let Some(file) = files.next_entry().await.unwrap()
                    && file.metadata().await.unwrap().len() == 4
                {
                    assert!(!destination.exists());
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(1)).await;
            }
            socket.write_all(b"done").await.unwrap();
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            let (result, ()) =
                tokio::join!(api.download_to_file("/download", &destination), server);
            result.unwrap();
        })
        .await
        .unwrap();
        assert_eq!(tokio::fs::read(&destination).await.unwrap(), b"partdone");
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn failed_downloads_preserve_existing_files_and_remove_partial_data() {
        for response in [
            &b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n"[..],
            &b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\npartial"[..],
        ] {
            let directory = tempfile::tempdir().unwrap();
            let destination = directory.path().join("download.bin");
            tokio::fs::write(&destination, b"original").await.unwrap();
            let (listener, address) = test_server::listen().await;
            let remote = format!("{address}/download");
            let api = ApiClient::new("http://unused.invalid");
            let (result, _) = tokio::join!(
                api.download_to_file(&remote, &destination),
                test_server::serve_once(&listener, response)
            );
            assert!(result.is_err());
            assert_eq!(tokio::fs::read(&destination).await.unwrap(), b"original");
            assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
        }
    }

    #[test]
    fn a_path_names_the_tree_it_writes_to() {
        let tree = Uuid::from_u128(7);
        let cases: [(String, Option<Uuid>); 7] = [
            (format!("/api/v1/trees/{tree}"), Some(tree)),
            (format!("/api/v1/trees/{tree}/persons"), Some(tree)),
            (format!("/api/v1/trees/{tree}?q=x"), Some(tree)),
            (
                format!("/api/v1/trees/{tree}/media/{tree}/pages"),
                Some(tree),
            ),
            ("/api/v1/trees".to_string(), None),
            ("/api/v1/trees/not-an-id/persons".to_string(), None),
            ("/api/v1/geneanet/plan".to_string(), None),
        ];
        for (path, expected) in cases {
            assert_eq!(tree_of_path(&path), expected, "{path}");
        }
    }

    #[test]
    fn a_response_fetched_across_a_write_to_its_tree_is_not_stored() {
        let cache = ResponseCache::default();
        let (written, other) = (Uuid::from_u128(1), Uuid::from_u128(2));
        let key = format!("/api/v1/trees/{written}/persons/p");
        let other_key = format!("/api/v1/trees/{other}/persons/p");
        let (before, other_before) = (cache.generation(&key), cache.generation(&other_key));

        cache.invalidate_prefix(&format!("/api/v1/trees/{written}"));
        cache.set_if_current(key.clone(), b"stale".to_vec(), before);
        cache.set_if_current(other_key.clone(), b"fresh".to_vec(), other_before);

        assert!(cache.get(&key).is_none(), "may predate the write");
        assert!(
            cache.get(&other_key).is_some(),
            "another tree was not written"
        );

        // Invalidating the tree list covers every tree.
        let after = cache.generation(&other_key);
        cache.invalidate_prefix("/api/v1/trees");
        cache.set_if_current(other_key.clone(), b"stale".to_vec(), after);
        assert!(cache.get(&other_key).is_none());
    }

    #[test]
    fn the_cache_drops_expired_and_then_oldest_entries_on_insert() {
        let cache = ResponseCache::default();
        let now = chrono::Utc::now().timestamp();
        cache.entries.lock().unwrap().extend([
            ("expired".to_string(), (Vec::new(), now - CACHE_TTL_SECS)),
            ("oldest".to_string(), (Vec::new(), now - 2)),
        ]);
        for index in 0..CACHE_MAX_ENTRIES - 1 {
            cache.set(format!("k{index}"), Vec::new());
        }
        {
            let entries = cache.entries.lock().unwrap();
            assert!(!entries.contains_key("expired"));
            assert!(entries.contains_key("oldest"));
            assert_eq!(entries.len(), CACHE_MAX_ENTRIES);
        }

        cache.set("newest".to_string(), Vec::new());

        let entries = cache.entries.lock().unwrap();
        assert_eq!(entries.len(), CACHE_MAX_ENTRIES);
        assert!(!entries.contains_key("oldest"));
        assert!(entries.contains_key("newest"));
    }

    /// A write invalidates its tree's cached reads by itself; a POST that
    /// only reads leaves them.
    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn writes_invalidate_their_tree_and_read_posts_do_not() {
        use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let api = ApiClient::new(&format!("http://{}", listener.local_addr().unwrap()));
        let tree = Uuid::now_v7();
        let cache_key = format!("/api/v1/trees/{tree}/persons");
        api.cache.set(cache_key.clone(), b"cached".to_vec());
        let server = async {
            for body in [r#"{"names":[],"spouses":[]}"#, "{}"] {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut reader = tokio::io::BufReader::new(&mut socket);
                let mut head = String::new();
                while !head.ends_with("\r\n\r\n") {
                    assert!(reader.read_line(&mut head).await.unwrap() > 0);
                }
                let length = head
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length: ")
                            .map(str::to_string)
                    })
                    .and_then(|length| length.trim().parse::<usize>().ok())
                    .unwrap_or_default();
                let mut request_body = vec![0; length];
                reader.read_exact(&mut request_body).await.unwrap();
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                socket.write_all(response.as_bytes()).await.unwrap();
            }
        };
        let client = async {
            api.relation_labels(tree, &[Uuid::now_v7()], &[])
                .await
                .unwrap();
            assert!(
                api.cache.get(&cache_key).is_some(),
                "a read POST keeps the cache"
            );
            let _: serde_json::Value = api
                .post(
                    &format!("/api/v1/trees/{tree}/notes"),
                    &serde_json::json!({}),
                )
                .await
                .unwrap();
            assert!(api.cache.get(&cache_key).is_none(), "a write drops it");
        };
        tokio::join!(server, client);
    }

    #[test]
    fn a_list_is_filtered_only_by_the_owners_given() {
        let person = Uuid::from_u128(1);
        let media = Uuid::from_u128(2);
        assert_eq!(
            owner_filters([
                ("person_id", Some(person)),
                ("event_id", None),
                ("media_id", Some(media)),
            ]),
            vec![
                ("person_id", person.to_string()),
                ("media_id", media.to_string()),
            ]
        );
        assert!(owner_filters([("person_id", None)]).is_empty());
    }

    /// Every page of a paginated list is read, each with the list's filters,
    /// and a page claiming a successor without a cursor ends the walk. The
    /// pages are served from the response cache, so no server is needed.
    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn every_page_of_a_list_is_collected_until_the_last() {
        let api = ApiClient::new("http://127.0.0.1:9");
        let path = "/api/v1/trees/t/notes";
        let filter = ("person_id", "p".to_string());
        let page = |nodes: &[u32], has_next_page: bool, end_cursor: Option<&str>| {
            serde_json::to_vec(&serde_json::json!({
                "edges": nodes.iter().map(|node| {
                    serde_json::json!({ "cursor": node.to_string(), "node": node })
                }).collect::<Vec<_>>(),
                "page_info": { "has_next_page": has_next_page, "end_cursor": end_cursor },
                "total_count": 5,
            }))
            .unwrap()
        };
        let key = |after: Option<&str>| {
            let mut params = vec![filter.clone(), ("first", "2".to_string())];
            if let Some(after) = after {
                params.push(("after", after.to_string()));
            }
            format!("{path}?{}", serde_json::to_string(&params).unwrap())
        };
        api.cache.set(key(None), page(&[1, 2], true, Some("c2")));
        api.cache
            .set(key(Some("c2")), page(&[3, 4], true, Some("c4")));
        api.cache.set(key(Some("c4")), page(&[5], true, None));

        let nodes: Vec<u32> = api
            .collect_pages(path, 2, vec![filter.clone()])
            .await
            .unwrap();

        assert_eq!(nodes, [1, 2, 3, 4, 5]);
    }

    #[test]
    fn reference_terms_over_the_limit_are_split_into_subsequent_requests() {
        let terms = (0..129).map(|index| index.to_string()).collect::<Vec<_>>();
        let batches = reference_term_batches(&terms).collect::<Vec<_>>();

        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0].len(), 128);
        assert_eq!(batches[1].len(), 1);
    }

    #[test]
    fn relation_labels_over_the_limit_are_split_into_subsequent_requests() {
        let batches = relation_label_batch_ranges(1_000, 25);

        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0], (0..1_000, 0..24));
        assert_eq!(batches[1], (1_000..1_000, 24..25));
    }

    #[test]
    fn portraits_over_the_limit_are_split_into_subsequent_requests() {
        let person_ids = (0..1_025).map(|_| Uuid::now_v7()).collect::<Vec<_>>();
        let batches = portrait_batches(&person_ids).collect::<Vec<_>>();

        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0].len(), 1_024);
        assert_eq!(batches[1].len(), 1);
    }
}
