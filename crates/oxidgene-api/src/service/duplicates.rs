//! Same-named persons: recording that two records are different people,
//! merging two records that turn out to be one, and finding the pairs of
//! records that may be one person.
//!
//! The homonyms of one person are a read of the search projection
//! ([`ProfileService::homonyms`]); the potential duplicates of a whole tree
//! are computed here from the person projections. The two answers a user
//! can give about either, shared by REST and GraphQL, live here too.

use std::collections::{HashMap, HashSet};

use chrono::Datelike;
use oxidgene_core::collections::sorted_unique;
use oxidgene_core::projection::{PersonProfile, ProfileEvent, SearchEntry};
use oxidgene_core::search::fold_words;
use serde::Serialize;

use oxidgene_core::error::OxidGeneError;
use oxidgene_core::history::{AuditAction, AuditDetails, AuditEntity};
use oxidgene_core::types::{Person, PersonName};
use oxidgene_db::repo::{
    AncestryRepo, EventRepo, EventWitnessRepo, FamilySpouseRepo, MediaLinkRepo, PersonDistinctRepo,
    PersonMergeRepo, PersonNamePieces, PersonNamePiecesPatch, PersonNameRepo, PersonRepo,
    display_names,
};
use sea_orm::ConnectionTrait;
use uuid::Uuid;

use crate::profile::builder::build_search_entry;
use crate::profile::invalidation;
use crate::profile::service::{ProfileService, SEARCH_MAX_LIMIT};
use crate::service::history::Change;

/// Record that `person_id` is a different person from each of `others`.
///
/// Every person must belong to `tree_id`. Recording a pair twice is a no-op,
/// so answering the same question again changes nothing.
///
/// # Errors
///
/// `NotFound` if any person is missing from the tree; `Validation` if
/// `person_id` is among `others`, or `others` is empty or longer than
/// [`SEARCH_MAX_LIMIT`] — the most homonyms a single answer can concern.
pub async fn mark_distinct(
    conn: &impl ConnectionTrait,
    tree_id: Uuid,
    person_id: Uuid,
    others: &[Uuid],
) -> Result<(), OxidGeneError> {
    if others.is_empty() || others.len() > SEARCH_MAX_LIMIT {
        return Err(OxidGeneError::Validation(format!(
            "between 1 and {SEARCH_MAX_LIMIT} other persons are required"
        )));
    }
    if others.contains(&person_id) {
        return Err(OxidGeneError::Validation(
            "a person cannot be distinct from themselves".to_string(),
        ));
    }
    PersonRepo::get_in_tree(conn, tree_id, person_id).await?;
    for other in others {
        PersonRepo::get_in_tree(conn, tree_id, *other).await?;
    }
    PersonDistinctRepo::mark(conn, tree_id, person_id, others).await?;
    Change::new(
        tree_id,
        AuditAction::Create,
        AuditEntity::PersonDistinct,
        None,
    )
    .person(person_id)
    .record(conn)
    .await?;
    Ok(())
}

/// What the user chose while comparing the two records. The default keeps
/// the kept person's name and sex and moves everything the duplicate
/// carried.
#[derive(Debug, Clone, Default)]
pub struct MergeChoices {
    /// Own events of either person left out of the merged record:
    /// soft-deleted. The kept person's birth, say, when the duplicate's is
    /// the one chosen.
    pub left_out_events: Vec<Uuid>,
    /// The duplicate's direct media links not taken: removed, the media
    /// staying in the library.
    pub left_out_media_links: Vec<Uuid>,
    /// The merged primary name takes the duplicate's surname, particle
    /// included.
    pub surname_from_duplicate: bool,
    /// The merged primary name takes the duplicate's given names.
    pub given_names_from_duplicate: bool,
    /// The merged record takes the duplicate's sex.
    pub sex_from_duplicate: bool,
}

/// Merge `duplicate` into `kept`: one individual recorded twice becomes one
/// record, the kept one, and the duplicate is soft-deleted.
///
/// Everything the duplicate carried moves to the kept person — see
/// [`PersonMergeRepo::absorb`] for what is re-pointed and what is dropped as
/// already present. Projections of both people's relatives are rebuilt in the
/// same transaction, and the duplicate's are removed.
///
/// # Errors
///
/// `NotFound` if either person is missing from the tree. `Validation` if they
/// are the same person, spouses of the same union, or one is an ancestor of
/// the other: each of those would leave a person married to, or descended
/// from, themselves; and if `choices` leaves out an event that is neither
/// person's own, or a media link that is not the duplicate's own.
pub async fn merge_persons(
    conn: &impl ConnectionTrait,
    profiles: &ProfileService,
    tree_id: Uuid,
    kept: Uuid,
    duplicate: Uuid,
    choices: &MergeChoices,
) -> Result<Person, OxidGeneError> {
    ensure_mergeable(conn, tree_id, kept, duplicate).await?;

    // Both relative sets are read while the family links still say who the
    // duplicate's relatives are.
    let (kept_affected, duplicate_affected) = tokio::try_join!(
        invalidation::affected_persons(conn, kept),
        invalidation::affected_persons(conn, duplicate),
    )?;

    // Read before the merge re-points them: the duplicate's name, and the
    // events whose witness lists are about to name the kept person instead.
    let duplicate_label = display_names(conn, &[duplicate]).await?.remove(&duplicate);
    let witnessed: Vec<Uuid> = EventWitnessRepo::list_by_person(conn, duplicate)
        .await?
        .into_iter()
        .map(|witness| witness.event_id)
        .collect();

    write_merge(conn, tree_id, kept, duplicate, choices).await?;
    profiles
        .invalidate_for_person_delete(conn, tree_id, duplicate)
        .await?;

    let affected = sorted_unique(
        kept_affected
            .into_iter()
            .chain(duplicate_affected)
            .filter(|id| *id != duplicate),
    );
    profiles
        .invalidate_for_mutation(conn, tree_id, &affected)
        .await?;

    let mut change = Change::new(tree_id, AuditAction::Merge, AuditEntity::Person, kept)
        .person(kept)
        .persons([duplicate])
        .persons(affected)
        .details(AuditDetails {
            other_label: duplicate_label,
            ..AuditDetails::default()
        });
    for event_id in witnessed
        .into_iter()
        .chain(choices.left_out_events.iter().copied())
    {
        change = change.event(event_id);
    }
    change.record(conn).await?;

    PersonRepo::get(conn, kept).await
}

/// The merge's writes: what was left out dropped, the rest moved onto the
/// kept person, the picked sex and name applied, the duplicate deleted.
async fn write_merge(
    conn: &impl ConnectionTrait,
    tree_id: Uuid,
    kept: Uuid,
    duplicate: Uuid,
    choices: &MergeChoices,
) -> Result<(), OxidGeneError> {
    // Read before the absorb moves the duplicate's names onto the kept person.
    let duplicate_person = PersonRepo::get(conn, duplicate).await?;
    let duplicate_primary = primary_name(PersonNameRepo::list_by_person(conn, duplicate).await?);

    drop_left_out(conn, tree_id, [kept, duplicate], duplicate, choices).await?;
    PersonMergeRepo::absorb(conn, tree_id, kept, duplicate).await?;
    if choices.sex_from_duplicate {
        PersonRepo::update(conn, kept, Some(duplicate_person.sex), None).await?;
    }
    if let Some(taken) = duplicate_primary {
        choose_primary_name(conn, kept, &taken, choices).await?;
    }
    PersonRepo::delete(conn, duplicate).await
}

/// Refuses a merge that would leave a person married to, or descended from,
/// themselves: the same person twice, spouses of one union, or an ancestor
/// and their descendant. Both must be persons of the tree.
async fn ensure_mergeable(
    conn: &impl ConnectionTrait,
    tree_id: Uuid,
    kept: Uuid,
    duplicate: Uuid,
) -> Result<(), OxidGeneError> {
    if kept == duplicate {
        return Err(OxidGeneError::Validation(
            "a person cannot be merged with themselves".to_string(),
        ));
    }
    PersonRepo::get_in_tree(conn, tree_id, kept).await?;
    PersonRepo::get_in_tree(conn, tree_id, duplicate).await?;

    let (kept_unions, duplicate_unions) = tokio::try_join!(
        FamilySpouseRepo::list_by_person(conn, kept),
        FamilySpouseRepo::list_by_person(conn, duplicate),
    )?;
    let kept_unions: HashSet<Uuid> = kept_unions.iter().map(|link| link.family_id).collect();
    if duplicate_unions
        .iter()
        .any(|link| kept_unions.contains(&link.family_id))
    {
        return Err(OxidGeneError::Validation(
            "spouses of the same union cannot be merged".to_string(),
        ));
    }

    let (ancestors, descendants) = tokio::try_join!(
        AncestryRepo::ancestors(conn, duplicate, None),
        AncestryRepo::descendants(conn, duplicate, None),
    )?;
    if ancestors
        .iter()
        .chain(descendants.iter())
        .any(|link| link.person_id == kept)
    {
        return Err(OxidGeneError::Validation(
            "a person cannot be merged with their own ancestor or descendant".to_string(),
        ));
    }
    Ok(())
}

/// Removes what a merge leaves out before the rest moves: own events of
/// either person, soft-deleted, and the duplicate's direct media links.
/// Anything else is refused, so a merge can never drop someone else's
/// record.
async fn drop_left_out(
    conn: &impl ConnectionTrait,
    tree_id: Uuid,
    pair: [Uuid; 2],
    duplicate: Uuid,
    choices: &MergeChoices,
) -> Result<(), OxidGeneError> {
    for &event_id in &choices.left_out_events {
        let event = EventRepo::get(conn, event_id).await?;
        if event.tree_id != tree_id || !event.person_id.is_some_and(|p| pair.contains(&p)) {
            return Err(OxidGeneError::Validation(
                "only the two persons' own events can be left out".to_string(),
            ));
        }
        EventRepo::delete(conn, event_id).await?;
    }
    for &link_id in &choices.left_out_media_links {
        let link = MediaLinkRepo::get(conn, link_id).await?;
        if link.person_id != Some(duplicate) {
            return Err(OxidGeneError::Validation(
                "only the duplicate's own media links can be left out".to_string(),
            ));
        }
        MediaLinkRepo::delete(conn, link_id).await?;
    }
    Ok(())
}

fn primary_name(names: Vec<PersonName>) -> Option<PersonName> {
    names.into_iter().find(|name| name.is_primary)
}

fn pieces_of(name: &PersonName) -> PersonNamePieces {
    PersonNamePieces {
        given_names: name.given_names.clone(),
        surname: name.surname.clone(),
        surname_prefix: name.surname_prefix.clone(),
        prefix: name.prefix.clone(),
        suffix: name.suffix.clone(),
        nickname: name.nickname.clone(),
    }
}

/// Whether two names read the same, ignoring case as the merge does when it
/// drops a name the kept person already bears.
fn same_pieces(a: &PersonNamePieces, b: &PersonNamePieces) -> bool {
    let same = |x: &Option<String>, y: &Option<String>| {
        x.as_deref().map(str::to_lowercase) == y.as_deref().map(str::to_lowercase)
    };
    same(&a.given_names, &b.given_names)
        && same(&a.surname, &b.surname)
        && same(&a.surname_prefix, &b.surname_prefix)
        && same(&a.prefix, &b.prefix)
        && same(&a.suffix, &b.suffix)
        && same(&a.nickname, &b.nickname)
}

/// Gives the merged record the primary name the user composed: the kept
/// primary name with the duplicate's surname, given names, or both. The
/// former primary name stays as a secondary one, so nothing is lost; a name
/// the person already bears is promoted rather than written twice.
async fn choose_primary_name(
    conn: &impl ConnectionTrait,
    kept: Uuid,
    taken: &PersonName,
    choices: &MergeChoices,
) -> Result<(), OxidGeneError> {
    let (surname, given_names) = (
        choices.surname_from_duplicate,
        choices.given_names_from_duplicate,
    );
    if !surname && !given_names {
        return Ok(());
    }
    let names = PersonNameRepo::list_by_person(conn, kept).await?;
    let Some(current) = names.iter().find(|name| name.is_primary) else {
        return Ok(());
    };
    // All of the duplicate's name when both pieces are taken, otherwise the
    // kept name with one piece replaced.
    let (base, name_type) = if surname && given_names {
        (taken, taken.name_type)
    } else {
        (current, current.name_type)
    };
    let mut target = pieces_of(base);
    if surname {
        target.surname = taken.surname.clone();
        target.surname_prefix = taken.surname_prefix.clone();
    }
    if given_names {
        target.given_names = taken.given_names.clone();
    }
    if same_pieces(&target, &pieces_of(current)) && name_type == current.name_type {
        return Ok(());
    }

    let demoted = PersonNamePiecesPatch::default();
    PersonNameRepo::update(conn, current.id, None, demoted, Some(false), None).await?;
    let borne = names
        .iter()
        .find(|name| name.name_type == name_type && same_pieces(&pieces_of(name), &target));
    match borne {
        Some(name) => {
            let unchanged = PersonNamePiecesPatch::default();
            PersonNameRepo::update(conn, name.id, None, unchanged, Some(true), None).await?;
        }
        None => {
            let next = names.iter().map(|name| name.sort_order).max().unwrap_or(0) + 1;
            PersonNameRepo::create(conn, Uuid::now_v7(), kept, name_type, target, true, next)
                .await?;
        }
    }
    Ok(())
}

// ── Potential duplicates ─────────────────────────────────────────────────

/// The fewest points a pair needs to be listed: the same name alone
/// ([`SAME_NAME_POINTS`]) is not enough, a tree holds many namesakes; a
/// second clue is needed.
pub const MIN_DUPLICATE_SCORE: i64 = 40;
/// The most pairs one answer lists, best first; `count` says how many
/// there are.
pub const MAX_DUPLICATE_PAIRS: usize = 500;
/// Births or deaths further apart than this rule a pair out.
const MAX_YEAR_GAP: i32 = 5;

// Points each shared clue adds to a pair's score, capped at 100
// (`docs/ui-tools.md` §6).

/// The same surname and given names, folded.
const SAME_NAME_POINTS: i64 = 30;
/// Only a similar name: the same sound key.
const SIMILAR_NAME_POINTS: i64 = 15;
/// The same complete birth date.
const SAME_BIRTH_DATE_POINTS: i64 = 30;
/// Otherwise the same birth year.
const SAME_BIRTH_YEAR_POINTS: i64 = 20;
/// Otherwise births at most [`MAX_YEAR_GAP`] years apart.
const CLOSE_BIRTH_POINTS: i64 = 5;
/// The same birthplace, folded.
const SAME_BIRTH_PLACE_POINTS: i64 = 10;
/// The same death year.
const SAME_DEATH_YEAR_POINTS: i64 = 15;
/// Children of the same family.
const SAME_PARENTS_POINTS: i64 = 20;
/// Otherwise a father, or a mother, of the same name: each.
const SAME_PARENT_NAME_POINTS: i64 = 10;
/// A spouse of the same name.
const SAME_SPOUSE_POINTS: i64 = 10;

/// Two records of a tree that may be one person.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DuplicatePair {
    /// From 0 to 100, how alike the two records are (`docs/ui-tools.md` §6).
    pub score: i64,
    /// What they share, strongest first: `same_name`, `similar_name`,
    /// `same_birth_date`, `same_birth_year`, `close_birth`,
    /// `same_birth_place`, `same_death_year`, `same_parents`, `same_father`,
    /// `same_mother`, `same_spouse`.
    pub reasons: Vec<String>,
    /// Both records as the search rows show them.
    pub first: SearchEntry,
    pub second: SearchEntry,
    /// Their full birth and death dates, which the search rows reduce to a
    /// year: a comparison needs the day.
    pub first_dates: LifeDates,
    pub second_dates: LifeDates,
}

/// A record's birth (or baptism) and death (or burial) dates, as recorded.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[cfg_attr(feature = "graphql", derive(async_graphql::SimpleObject))]
pub struct LifeDates {
    pub birth: Option<crate::service::statistics::RecordDate>,
    pub death: Option<crate::service::statistics::RecordDate>,
}

impl LifeDates {
    fn of(profile: &PersonProfile) -> Self {
        let date = |event: Option<&ProfileEvent>| {
            event
                .filter(|e| e.date_value.is_some() || e.date_sort.is_some())
                .map(crate::service::statistics::record_date)
        };
        Self {
            birth: date(profile.birth_or_baptism()),
            death: date(profile.death_or_burial()),
        }
    }
}

/// A tree's potential duplicates.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PotentialDuplicates {
    /// Every pair found, of which `pairs` lists at most
    /// [`MAX_DUPLICATE_PAIRS`].
    pub count: i64,
    pub pairs: Vec<DuplicatePair>,
}

/// Loads a tree's projections and its distinct-person confirmations, and
/// finds the pairs of records that may be one person.
pub async fn load_potential_duplicates(
    db: &sea_orm::DatabaseConnection,
    profiles: &ProfileService,
    tree_id: Uuid,
) -> Result<PotentialDuplicates, OxidGeneError> {
    oxidgene_db::repo::TreeRepo::get(db, tree_id).await?;
    let persons = profiles.get_all_persons(db, tree_id).await?;
    let distinct = PersonDistinctRepo::pairs_in_tree(db, tree_id).await?;
    tokio::task::spawn_blocking(move || potential_duplicates(&persons, &distinct))
        .await
        .map_err(|e| OxidGeneError::Internal(e.to_string()))
}

/// A name folded so that spellings that sound alike meet: lowercase and
/// accents aside, letters only, `y` read as `i` and `ph` as `f`, doubled
/// letters single, and a final `s`, `x`, `z`, `t` or `d` dropped from a word
/// longer than four letters (`Martins` meets `Martin`, `Dupond` `Dupont`).
pub fn sound_key(name: &str) -> String {
    fold_words(name)
        .split(|c: char| !c.is_alphabetic())
        .filter(|word| !word.is_empty())
        .map(|word| {
            let word = word.replace("ph", "f").replace('y', "i");
            let mut folded = String::with_capacity(word.len());
            for c in word.chars() {
                if !folded.ends_with(c) {
                    folded.push(c);
                }
            }
            if folded.chars().count() > 4 && folded.ends_with(['s', 'x', 'z', 't', 'd']) {
                folded.pop();
            }
            folded
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// What a pair of records is compared on.
struct Record<'a> {
    profile: &'a PersonProfile,
    surname: String,
    given: String,
    /// The block the record is compared within: its surname and first given
    /// name, by sound.
    key: (String, String),
    birth: Option<&'a ProfileEvent>,
    death: Option<&'a ProfileEvent>,
    /// The birth place, folded; read once here rather than for each pair.
    birth_place: Option<String>,
    /// The spouses' folded names.
    spouses: Vec<&'a str>,
}

impl<'a> Record<'a> {
    fn new(profile: &'a PersonProfile, names: &'a HashMap<Uuid, String>) -> Option<Self> {
        let name = profile.primary_name.as_ref()?;
        let surname = name.surname.as_deref().unwrap_or("").trim();
        let given = name.given_names.as_deref().unwrap_or("").trim();
        // Half a name says too little, as for homonyms.
        if surname.is_empty() || given.is_empty() {
            return None;
        }
        let first_given = given.split_whitespace().next().unwrap_or(given);
        let birth = profile.birth_or_baptism();
        Some(Self {
            profile,
            surname: fold_words(surname),
            given: fold_words(given),
            key: (sound_key(surname), sound_key(first_given)),
            birth,
            death: profile.death_or_burial(),
            birth_place: birth
                .and_then(|e| e.place_name.as_deref())
                .map(fold_words)
                .filter(|p| !p.trim().is_empty()),
            spouses: profile
                .families_as_spouse
                .iter()
                .filter_map(|l| names.get(&l.spouse_id?))
                .map(String::as_str)
                .filter(|n| !n.is_empty())
                .collect(),
        })
    }

    fn year(event: Option<&ProfileEvent>) -> Option<i32> {
        event.and_then(|e| e.date_sort).map(|d| d.year())
    }

    fn parents(&self) -> (Option<Uuid>, Option<Uuid>) {
        self.profile
            .family_as_child
            .as_ref()
            .map_or((None, None), |l| (l.father_id, l.mother_id))
    }
}

/// How alike two records are, with why, or `None` when something rules the
/// pair out: another sex, births or deaths years apart, one dead before the
/// other was born, or a family link between the two.
fn compare(
    a: &Record<'_>,
    b: &Record<'_>,
    names: &HashMap<Uuid, String>,
) -> Option<(i64, Vec<&'static str>)> {
    if !may_be_one(a, b) {
        return None;
    }
    let mut clues = Clues::default();
    if a.surname == b.surname && a.given == b.given {
        clues.add(SAME_NAME_POINTS, "same_name");
    } else {
        clues.add(SIMILAR_NAME_POINTS, "similar_name");
    }
    clues.births(a, b)?;
    if a.birth_place.is_some() && a.birth_place == b.birth_place {
        clues.add(SAME_BIRTH_PLACE_POINTS, "same_birth_place");
    }
    clues.deaths(a, b)?;
    clues.parents(a, b, names)?;
    if a.spouses.iter().any(|n| b.spouses.contains(n)) {
        clues.add(SAME_SPOUSE_POINTS, "same_spouse");
    }
    Some((clues.score.min(100), clues.reasons))
}

/// Whether nothing certain tells the two apart: a known sex each, and
/// different; a family link between them; one dead before the other's birth.
fn may_be_one(a: &Record<'_>, b: &Record<'_>) -> bool {
    use oxidgene_core::Sex;
    let (x, y) = (a.profile.sex, b.profile.sex);
    let sexes_differ = x != Sex::Unknown && y != Sex::Unknown && x != y;
    // A spouse, a parent or a child of the other is somebody else, and the
    // merge refuses them anyway.
    let related = |x: &Record<'_>, other: Uuid| {
        x.profile
            .families_as_spouse
            .iter()
            .any(|l| l.spouse_id == Some(other))
            || [x.parents().0, x.parents().1].contains(&Some(other))
    };
    let dead_before_born = |x: &Record<'_>, y: &Record<'_>| {
        matches!(
            (x.death.and_then(|e| e.date_sort), Record::year(y.birth)),
            (Some(died), Some(born)) if died.year() < born
        )
    };
    !sexes_differ
        && !related(a, b.profile.person_id)
        && !related(b, a.profile.person_id)
        && !dead_before_born(a, b)
        && !dead_before_born(b, a)
}

/// A birth or death dated to the day, not just to a month or a year.
fn full_date(event: Option<&ProfileEvent>) -> Option<chrono::NaiveDate> {
    event
        .filter(|e| {
            e.date_value
                .as_deref()
                .is_some_and(|v| v.split_whitespace().count() >= 3)
        })
        .and_then(|e| e.date_sort)
}

/// What a pair shares, scored.
#[derive(Default)]
struct Clues {
    score: i64,
    reasons: Vec<&'static str>,
}

impl Clues {
    fn add(&mut self, points: i64, reason: &'static str) {
        self.score += points;
        self.reasons.push(reason);
    }

    /// The births' clue; `None` when they are too far apart.
    fn births(&mut self, a: &Record<'_>, b: &Record<'_>) -> Option<()> {
        let (Some(x), Some(y)) = (Record::year(a.birth), Record::year(b.birth)) else {
            return Some(());
        };
        if (x - y).abs() > MAX_YEAR_GAP {
            return None;
        }
        let day = full_date(a.birth);
        if day.is_some() && day == full_date(b.birth) {
            self.add(SAME_BIRTH_DATE_POINTS, "same_birth_date");
        } else if x == y {
            self.add(SAME_BIRTH_YEAR_POINTS, "same_birth_year");
        } else {
            self.add(CLOSE_BIRTH_POINTS, "close_birth");
        }
        Some(())
    }

    /// The deaths' clue; `None` when they are too far apart.
    fn deaths(&mut self, a: &Record<'_>, b: &Record<'_>) -> Option<()> {
        match (Record::year(a.death), Record::year(b.death)) {
            (Some(x), Some(y)) if (x - y).abs() > MAX_YEAR_GAP => return None,
            (Some(x), Some(y)) if x == y => self.add(SAME_DEATH_YEAR_POINTS, "same_death_year"),
            _ => {}
        }
        Some(())
    }

    /// The parents' clue: the same family, else parents of the same names.
    /// `None` for two children of one family born on known different days,
    /// the second named after the first.
    fn parents(
        &mut self,
        a: &Record<'_>,
        b: &Record<'_>,
        names: &HashMap<Uuid, String>,
    ) -> Option<()> {
        let family = |r: &Record<'_>| r.profile.family_as_child.as_ref().map(|l| l.family_id);
        if family(a).is_some() && family(a) == family(b) {
            if let (Some(x), Some(y)) = (full_date(a.birth), full_date(b.birth))
                && x != y
            {
                return None;
            }
            self.add(SAME_PARENTS_POINTS, "same_parents");
            return Some(());
        }
        let name = |id: Option<Uuid>| id.and_then(|id| names.get(&id)).filter(|n| !n.is_empty());
        let ((a_father, a_mother), (b_father, b_mother)) = (a.parents(), b.parents());
        for (x, y, reason) in [
            (a_father, b_father, "same_father"),
            (a_mother, b_mother, "same_mother"),
        ] {
            if name(x).is_some() && name(x) == name(y) {
                self.add(SAME_PARENT_NAME_POINTS, reason);
            }
        }
        Some(())
    }
}

/// The pairs of a block worth comparing, each once. Births more than
/// [`MAX_YEAR_GAP`] years apart rule a pair out, so with the block sorted by
/// birth year, undated last, a dated record meets only the dated records
/// that follow it within the gap, then every undated one; the undated meet
/// each other. A common name then costs its neighbours in time, not the whole
/// block — except for the records without a birth year, which nothing rules
/// out in advance.
fn candidate_pairs<'b, 'a>(
    records: &'b mut [Record<'a>],
) -> impl Iterator<Item = (&'b Record<'a>, &'b Record<'a>)> {
    records.sort_by_key(|r| (Record::year(r.birth).is_none(), Record::year(r.birth)));
    let records: &'b [Record<'a>] = records;
    let (dated, undated) =
        records.split_at(records.partition_point(|r| Record::year(r.birth).is_some()));
    let dated_pairs = dated.iter().enumerate().flat_map(move |(i, a)| {
        let last = Record::year(a.birth).map(|year| year + MAX_YEAR_GAP);
        dated[i + 1..]
            .iter()
            .take_while(move |b| Record::year(b.birth) <= last)
            .chain(undated)
            .map(move |b| (a, b))
    });
    let undated_pairs = undated
        .iter()
        .enumerate()
        .flat_map(move |(i, a)| undated[i + 1..].iter().map(move |b| (a, b)));
    dated_pairs.chain(undated_pairs)
}

/// The pairs of records that may be one person, best first, leaving out the
/// pairs already confirmed to be different people (`distinct`, lower id
/// first).
pub fn potential_duplicates(
    profiles: &[PersonProfile],
    distinct: &HashSet<(Uuid, Uuid)>,
) -> PotentialDuplicates {
    let names: HashMap<Uuid, String> = profiles
        .iter()
        .map(|p| {
            let name = p
                .primary_name
                .as_ref()
                .map(|n| fold_words(&n.display_name))
                .unwrap_or_default();
            (p.person_id, name)
        })
        .collect();
    let mut blocks: HashMap<(String, String), Vec<Record<'_>>> = HashMap::new();
    for profile in profiles {
        if let Some(record) = Record::new(profile, &names) {
            blocks.entry(record.key.clone()).or_default().push(record);
        }
    }
    let mut found = Vec::new();
    for records in blocks.values_mut() {
        for (a, b) in candidate_pairs(records) {
            let (a, b) = if a.profile.person_id < b.profile.person_id {
                (a, b)
            } else {
                (b, a)
            };
            if distinct.contains(&(a.profile.person_id, b.profile.person_id)) {
                continue;
            }
            if let Some((score, reasons)) = compare(a, b, &names)
                && score >= MIN_DUPLICATE_SCORE
            {
                found.push((score, reasons, a.profile, b.profile));
            }
        }
    }
    found.sort_by(|x, y| {
        y.0.cmp(&x.0)
            .then_with(|| x.2.person_id.cmp(&y.2.person_id))
            .then_with(|| x.3.person_id.cmp(&y.3.person_id))
    });
    let count = found.len() as i64;
    let pairs = found
        .into_iter()
        .take(MAX_DUPLICATE_PAIRS)
        .map(|(score, reasons, a, b)| DuplicatePair {
            score,
            reasons: reasons.into_iter().map(str::to_string).collect(),
            first: build_search_entry(a),
            second: build_search_entry(b),
            first_dates: LifeDates::of(a),
            second_dates: LifeDates::of(b),
        })
        .collect();
    PotentialDuplicates { count, pairs }
}

#[cfg(test)]
mod tests;
