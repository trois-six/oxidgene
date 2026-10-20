//! Bulk tagging of persons, and the target that names the persons a bulk
//! change or a count reads (docs/api.md, Tags; docs/data-model.md, Audit log). REST and GraphQL both go through
//! here, so the bounds, the errors and the audit entry are the same on both.
//!
//! A bulk change is one transaction, one audit entry counting the persons it
//! changed, and one refresh of their projections. A person already carrying
//! the tag, or lacking it on removal, is not changed and not counted.

use std::collections::HashSet;
use std::str::FromStr;

use oxidgene_core::collections::sorted_unique;
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::history::{AuditAction, AuditCategory, AuditDetails, AuditEntity};
use oxidgene_core::types::RecordKind;
use oxidgene_db::entities::person;
use oxidgene_db::repo::{
    AncestryRepo, PersonSearchFilters, PersonSearchRepo, RecordTagRepo, TagRepo, db_err,
};
use oxidgene_db::sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait, QueryFilter, QuerySelect,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::profile::ProfileService;
use crate::service::history::Change;
use crate::service::scope::{
    TreeResource, begin_tx, commit_tx, require_live_tree, require_tree_resource,
};

/// Most persons a target may list by id.
pub const MAX_LISTED_PERSONS: usize = 1_000;
/// Most persons a target may hold, whatever names them.
pub const MAX_TARGET_PERSONS: usize = 10_000;
/// Shortest and longest depth of a branch, in generations.
pub const MIN_DEPTH: i64 = 1;
pub const MAX_DEPTH: i64 = 30;

/// Ids bound by one lookup, well inside what one statement may bind.
const LOOKUP_CHUNK: usize = 5_000;

/// The persons a bulk change or a target read names. Exactly one of
/// `persons`, `ancestors_of`, `descendants_of`, `ancestors_and_descendants_of`
/// or `search` is set; a request with none or several is `validation_error`.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct TagTargetSpec {
    /// Persons named by id, at most [`MAX_LISTED_PERSONS`].
    pub persons: Option<Vec<Uuid>>,
    /// The person whose ancestors are named.
    pub ancestors_of: Option<Uuid>,
    /// The person whose descendants are named.
    pub descendants_of: Option<Uuid>,
    /// The person whose ancestors and descendants are named together. A
    /// person named on both sides is counted once.
    pub ancestors_and_descendants_of: Option<Uuid>,
    /// Generations walked, 1 to 30; absent means all of them, up to 30.
    pub depth: Option<i64>,
    /// Whether the root person is named as well.
    pub include_self: bool,
    /// Whether the spouses of the descendants are named as well.
    pub include_spouses: bool,
    /// Every person the search returns, not one page of it.
    pub search: Option<SearchTarget>,
    /// Every person carrying this tag.
    pub tag: Option<Uuid>,
}

/// A person search as a target: the query and the filters of the search.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct SearchTarget {
    pub q: String,
    #[serde(flatten)]
    pub filters: PersonSearchFilters,
}

/// What a bulk change does to the target's persons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BulkAction {
    Add,
    Remove,
}

impl FromStr for BulkAction {
    type Err = OxidGeneError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "add" => Ok(Self::Add),
            "remove" => Ok(Self::Remove),
            _ => Err(OxidGeneError::Validation(
                "a bulk action is either add or remove".to_owned(),
            )),
        }
    }
}

/// One tag carried by at least one person of a target, with how many carry it.
#[derive(Debug, Clone, Serialize)]
pub struct TagTargetCount {
    pub tag_id: Uuid,
    pub count: i64,
}

/// The size of a target and, per tag, how many of its persons carry the tag.
#[derive(Debug, Clone, Serialize)]
pub struct TargetCounts {
    pub total: u64,
    pub tags: Vec<TagTargetCount>,
}

/// Put tag `tag_id` on, or take it off, every person of the target. Returns
/// the number of persons changed. Writes one audit entry when any changed.
pub async fn bulk_tag(
    db: &DatabaseConnection,
    profiles: &ProfileService,
    tree_id: Uuid,
    tag_id: Uuid,
    action: BulkAction,
    spec: &TagTargetSpec,
) -> Result<u64, OxidGeneError> {
    let txn = begin_tx(db).await?;
    require_live_tree(&txn, tree_id).await?;
    let tag = TagRepo::get(&txn, tree_id, tag_id).await?;
    let ids = resolve(&txn, tree_id, spec, Some(MAX_TARGET_PERSONS)).await?;
    let (changed, audit_action) = match action {
        BulkAction::Add => (
            RecordTagRepo::add_persons(&txn, tree_id, tag.id, &ids).await?,
            AuditAction::Create,
        ),
        BulkAction::Remove => (
            RecordTagRepo::remove_persons(&txn, tree_id, tag.id, &ids).await?,
            AuditAction::Delete,
        ),
    };
    if !changed.is_empty() {
        // A person's tags show on that person's own profile only.
        profiles
            .invalidate_for_mutation(&txn, tree_id, &changed)
            .await?;
        Change::new(tree_id, audit_action, AuditEntity::RecordTag, tag.id)
            .category(AuditCategory::of_record_tag(RecordKind::Person))
            .details(AuditDetails {
                count: Some(changed.len() as u64),
                ..AuditDetails::default()
            })
            .record_unversioned(&txn)
            .await?;
    }
    commit_tx(txn).await?;
    Ok(changed.len() as u64)
}

/// The size of the target and the tags its persons carry, writing nothing.
pub async fn target_counts(
    db: &DatabaseConnection,
    tree_id: Uuid,
    spec: &TagTargetSpec,
) -> Result<TargetCounts, OxidGeneError> {
    require_live_tree(db, tree_id).await?;
    let ids = resolve(db, tree_id, spec, Some(MAX_TARGET_PERSONS)).await?;
    let counts = RecordTagRepo::person_tag_counts(db, tree_id, &ids).await?;
    let mut tags: Vec<TagTargetCount> = counts
        .into_iter()
        .map(|(tag_id, count)| TagTargetCount { tag_id, count })
        .collect();
    tags.sort_by_key(|tag| tag.tag_id);
    Ok(TargetCounts {
        total: ids.len() as u64,
        tags,
    })
}

/// The live persons of `tree_id` that `spec` names, sorted, without
/// duplicates. Fails with `validation_error` on a malformed target, or on one
/// holding more than `bound` persons when a bound is given, and with
/// `not_found` on a listed or walked-from person, or on a tag, that is not
/// live in the tree. A bulk change and a count are bounded by
/// [`MAX_TARGET_PERSONS`]; an export is not.
async fn resolve(
    db: &impl ConnectionTrait,
    tree_id: Uuid,
    spec: &TagTargetSpec,
    bound: Option<usize>,
) -> Result<Vec<Uuid>, OxidGeneError> {
    let named = [
        spec.persons.is_some(),
        spec.ancestors_of.is_some(),
        spec.descendants_of.is_some(),
        spec.ancestors_and_descendants_of.is_some(),
        spec.search.is_some(),
        spec.tag.is_some(),
    ];
    if named.iter().filter(|&&set| set).count() != 1 {
        return Err(OxidGeneError::Validation(
            "a target names exactly one of persons, ancestors_of, descendants_of, ancestors_and_descendants_of, search or tag"
                .to_owned(),
        ));
    }
    let ids = if let Some(persons) = &spec.persons {
        listed_persons(db, tree_id, persons).await?
    } else if let Some(root) = spec.ancestors_of {
        branch(db, tree_id, root, spec, Direction::Up).await?
    } else if let Some(root) = spec.descendants_of {
        branch(db, tree_id, root, spec, Direction::Down).await?
    } else if let Some(root) = spec.ancestors_and_descendants_of {
        both_sides(db, tree_id, root, spec).await?
    } else if let Some(search) = &spec.search {
        PersonSearchRepo::search_ids(db, tree_id, &search.q, &search.filters, search_limit(bound))
            .await?
    } else if let Some(tag) = spec.tag {
        tagged_persons(db, tree_id, tag).await?
    } else {
        return Err(OxidGeneError::Validation(
            "a target names exactly one of persons, ancestors_of, descendants_of, ancestors_and_descendants_of, search or tag"
                .to_owned(),
        ));
    };
    enforce_bound(ids, bound)
}

/// The persons an export's `spec` keeps: the target, unbounded. A scope
/// holding no person is a `validation_error`.
pub async fn export_persons(
    db: &impl ConnectionTrait,
    tree_id: Uuid,
    spec: &TagTargetSpec,
) -> Result<Vec<Uuid>, OxidGeneError> {
    let ids = resolve(db, tree_id, spec, None).await?;
    if ids.is_empty() {
        return Err(OxidGeneError::Validation(
            "a scope holds no person".to_owned(),
        ));
    }
    Ok(ids)
}

/// A target as the JSON text a query or a job carries it in, parsed.
pub fn parse_scope(text: &str) -> Result<TagTargetSpec, OxidGeneError> {
    serde_json::from_str(text)
        .map_err(|error| OxidGeneError::Validation(format!("a scope is a target as JSON: {error}")))
}

/// The rows a search target fetches: one past `bound`, so that a search which
/// returns more is refused without reading the rest of it. Unbounded, the
/// largest count a row limit takes.
fn search_limit(bound: Option<usize>) -> u64 {
    bound.map_or(i64::MAX as u64, |max| (max + 1) as u64)
}

/// The sorted, unique ids of a target, refused when there are more than
/// `bound`.
fn enforce_bound(ids: Vec<Uuid>, bound: Option<usize>) -> Result<Vec<Uuid>, OxidGeneError> {
    let ids = sorted_unique(ids);
    match bound {
        Some(max) if ids.len() > max => Err(OxidGeneError::Validation(format!(
            "a target holds at most {max} persons"
        ))),
        _ => Ok(ids),
    }
}

/// The live persons carrying `tag`, which must be a tag of the tree.
async fn tagged_persons(
    db: &impl ConnectionTrait,
    tree_id: Uuid,
    tag: Uuid,
) -> Result<Vec<Uuid>, OxidGeneError> {
    TagRepo::get(db, tree_id, tag).await?;
    let carriers = RecordTagRepo::persons_with_tag(db, tree_id, tag).await?;
    live_only(db, tree_id, carriers).await
}

/// The direction a branch target walks from its root.
#[derive(Clone, Copy)]
enum Direction {
    Up,
    Down,
}

/// The persons named by a listed target: at most [`MAX_LISTED_PERSONS`], all
/// live persons of the tree.
async fn listed_persons(
    db: &impl ConnectionTrait,
    tree_id: Uuid,
    persons: &[Uuid],
) -> Result<Vec<Uuid>, OxidGeneError> {
    let ids = sorted_unique(persons.iter().copied());
    if ids.len() > MAX_LISTED_PERSONS {
        return Err(OxidGeneError::Validation(format!(
            "a target lists at most {MAX_LISTED_PERSONS} persons"
        )));
    }
    let live = live_persons(db, tree_id, &ids).await?;
    if let Some(missing) = ids.iter().find(|id| !live.contains(id)) {
        return Err(OxidGeneError::NotFound {
            entity: "Person",
            id: *missing,
        });
    }
    Ok(ids)
}

/// The ancestors or descendants of `root` within the depth of `spec`, the
/// root itself and, for descendants, their spouses when the spec asks for
/// them.
async fn branch(
    db: &impl ConnectionTrait,
    tree_id: Uuid,
    root: Uuid,
    spec: &TagTargetSpec,
    direction: Direction,
) -> Result<Vec<Uuid>, OxidGeneError> {
    require_tree_resource(db, tree_id, TreeResource::Person, root).await?;
    let depth = depth_bound(spec.depth)?;
    let mut walked = match direction {
        Direction::Up => AncestryRepo::ancestors_of_many(db, &[root], Some(depth)).await?,
        Direction::Down => AncestryRepo::descendants_of_many(db, &[root], Some(depth)).await?,
    };
    let mut ids: Vec<Uuid> = walked
        .remove(&root)
        .unwrap_or_default()
        .into_iter()
        .map(|link| link.person_id)
        .collect();
    if spec.include_spouses && matches!(direction, Direction::Down) {
        // The spouses of the descendants, not of the root unless it is named
        // as well.
        let spouses = spouses_of(db, tree_id, &ids).await?;
        ids.extend(spouses);
    }
    if spec.include_self {
        ids.push(root);
    }
    live_only(db, tree_id, ids).await
}

/// The walk depth a branch target asks for: 1 to 30, all of them when absent.
fn depth_bound(depth: Option<i64>) -> Result<i32, OxidGeneError> {
    match depth {
        None => Ok(MAX_DEPTH as i32),
        Some(depth) if (MIN_DEPTH..=MAX_DEPTH).contains(&depth) => Ok(depth as i32),
        Some(_) => Err(OxidGeneError::Validation(format!(
            "a branch depth is between {MIN_DEPTH} and {MAX_DEPTH} generations"
        ))),
    }
}

/// The ancestors and the descendants of `root` within the depth of `spec`,
/// with the root when `spec` names it. Both walks may name the same person
/// (a spouse of a descendant who is also an ancestor), so the result is
/// deduplicated by [`enforce_bound`] before it is counted.
async fn both_sides(
    db: &impl ConnectionTrait,
    tree_id: Uuid,
    root: Uuid,
    spec: &TagTargetSpec,
) -> Result<Vec<Uuid>, OxidGeneError> {
    let mut ids = branch(db, tree_id, root, spec, Direction::Up).await?;
    ids.extend(branch(db, tree_id, root, spec, Direction::Down).await?);
    Ok(ids)
}

/// The spouses of `ids` in the families where they are a spouse, not a child:
/// the parents of a person are not their spouses.
async fn spouses_of(
    db: &impl ConnectionTrait,
    tree_id: Uuid,
    ids: &[Uuid],
) -> Result<Vec<Uuid>, OxidGeneError> {
    let members: HashSet<Uuid> = ids.iter().copied().collect();
    let links = AncestryRepo::family_links(db, tree_id).await?;
    let families: HashSet<Uuid> = links
        .iter()
        .filter(|link| link.spouse_role.is_some() && members.contains(&link.person_id))
        .map(|link| link.family_id)
        .collect();
    Ok(links
        .iter()
        .filter(|link| link.spouse_role.is_some() && families.contains(&link.family_id))
        .map(|link| link.person_id)
        .collect())
}

/// `ids` less the persons that are not live persons of `tree_id`.
async fn live_only(
    db: &impl ConnectionTrait,
    tree_id: Uuid,
    ids: Vec<Uuid>,
) -> Result<Vec<Uuid>, OxidGeneError> {
    let ids = sorted_unique(ids);
    let live = live_persons(db, tree_id, &ids).await?;
    Ok(ids.into_iter().filter(|id| live.contains(id)).collect())
}

/// The ids among `ids` that are live persons of `tree_id`.
async fn live_persons(
    db: &impl ConnectionTrait,
    tree_id: Uuid,
    ids: &[Uuid],
) -> Result<HashSet<Uuid>, OxidGeneError> {
    let mut live = HashSet::with_capacity(ids.len());
    for chunk in ids.chunks(LOOKUP_CHUNK) {
        let rows: Vec<(Uuid,)> = person::Entity::find()
            .select_only()
            .column(person::Column::Id)
            .filter(person::Column::TreeId.eq(tree_id))
            .filter(person::Column::DeletedAt.is_null())
            .filter(person::Column::Id.is_in(chunk.to_vec()))
            .into_tuple::<(Uuid,)>()
            .all(db)
            .await
            .map_err(db_err)?;
        live.extend(rows.into_iter().map(|(id,)| id));
    }
    Ok(live)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(count: usize) -> Vec<Uuid> {
        (0..count as u128).map(Uuid::from_u128).collect()
    }

    #[test]
    fn a_search_target_fetches_one_past_the_bound() {
        assert_eq!(search_limit(Some(MAX_TARGET_PERSONS)), 10_001);
        assert_eq!(search_limit(Some(3)), 4);
    }

    #[test]
    fn a_target_at_the_bound_is_accepted_and_one_past_it_is_refused() {
        assert_eq!(
            enforce_bound(ids(3), Some(3)).map(|ids| ids.len()).ok(),
            Some(3)
        );
        assert!(matches!(
            enforce_bound(ids(4), Some(3)),
            Err(OxidGeneError::Validation(message)) if message == "a target holds at most 3 persons"
        ));
    }

    #[test]
    fn duplicate_ids_do_not_count_towards_the_bound() {
        let mut doubled = ids(3);
        doubled.extend(ids(3));
        assert_eq!(
            enforce_bound(doubled, Some(3)).map(|ids| ids.len()).ok(),
            Some(3)
        );
    }
}
