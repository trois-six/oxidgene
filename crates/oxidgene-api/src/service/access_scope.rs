//! Access scopes (docs/authorization.md §6.5): the persons a membership, an
//! invitation, a share link or the visitor scope is limited to — storing a
//! definition, keeping its persons current, and previewing what one reaches.
//! REST and GraphQL both go through here. Web build only.
//!
//! The persons of a scope are materialized in `access_scope_person`, so a
//! read filtering on a scope joins an index rather than walking the tree. A
//! scope records the tree's ancestry generation it was built from; the
//! triggers that move `sosa_state.generation` on every change to a family
//! link, a family or a person's deletion (docs/data-model.md §4.8) make it
//! stale, and [`scope_persons`] rebuilds a stale scope on the writer, in one
//! transaction, before reading it. A child added under a shared ancestor is
//! therefore in the share at the next read.

use std::collections::{HashMap, HashSet};

use oxidgene_core::OxidGeneError;
use oxidgene_core::privacy::{FamilyMembers, Kinship, withheld};
use oxidgene_core::sharing::ScopeDefinition;
use oxidgene_db::repo::{
    AccessScope, AccessScopeRepo, AncestryRepo, PersonRepo, TreeRepo, display_names,
};
use oxidgene_db::sea_orm::{ConnectionTrait, DatabaseConnection};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::profile::ProfileService;
use crate::service::scope::{TreeResource, begin_tx, commit_tx, require_tree_resource};

/// The live persons of `tree_id` and how their families link them: what a
/// scope or an exclusion walks.
pub(crate) async fn kinship(
    conn: &impl ConnectionTrait,
    tree_id: Uuid,
) -> Result<(Kinship, HashSet<Uuid>), OxidGeneError> {
    let (live, links) = tokio::try_join!(
        PersonRepo::live_ids(conn, tree_id),
        AncestryRepo::family_links(conn, tree_id),
    )?;
    let mut families: HashMap<Uuid, FamilyMembers> = HashMap::new();
    for link in links.into_iter().filter(|l| live.contains(&l.person_id)) {
        let family = families.entry(link.family_id).or_default();
        if link.spouse_role.is_some() {
            family.spouses.push(link.person_id);
        } else {
            family.children.push(link.person_id);
        }
    }
    Ok((Kinship::of_families(families.values()), live))
}

/// A scope as the sharing surfaces show it: its definition, its root's
/// name, and whether the root is deleted, which leaves the scope empty until
/// the root is restored.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ScopeView {
    pub id: Uuid,
    #[serde(flatten)]
    pub definition: ScopeDefinition,
    /// The root's display name; none when they have none.
    pub person_name: Option<String>,
    /// The root is deleted: the scope reaches nobody until it is restored.
    pub person_deleted: bool,
}

/// The views of `scope_ids`, by scope; a scope not found is left out.
pub async fn views(
    conn: &impl ConnectionTrait,
    scope_ids: &[Uuid],
) -> Result<HashMap<Uuid, ScopeView>, OxidGeneError> {
    let ids: Vec<Uuid> = scope_ids
        .iter()
        .copied()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    let scopes = AccessScopeRepo::get_many(conn, &ids).await?;
    if scopes.is_empty() {
        return Ok(HashMap::new());
    }
    let roots: Vec<Uuid> = scopes.values().map(|s| s.definition.person_id).collect();
    let names: HashMap<Uuid, String> = display_names(conn, &roots).await?.into_iter().collect();
    let live = live_among(conn, &roots).await?;
    Ok(scopes
        .into_values()
        .map(|scope| {
            let root = scope.definition.person_id;
            let view = ScopeView {
                id: scope.id,
                definition: scope.definition,
                person_name: names.get(&root).cloned(),
                person_deleted: !live.contains(&root),
            };
            (scope.id, view)
        })
        .collect())
}

/// Those of `persons` that are live.
async fn live_among(
    conn: &impl ConnectionTrait,
    persons: &[Uuid],
) -> Result<HashSet<Uuid>, OxidGeneError> {
    Ok(PersonRepo::get_many(conn, persons)
        .await?
        .into_iter()
        .map(|person| person.id)
        .collect())
}

/// Check `definition` for tree `tree_id`: a depth within bounds, and a root
/// that is a live person of the tree (`not_found` otherwise).
pub async fn check_definition(
    conn: &impl ConnectionTrait,
    tree_id: Uuid,
    definition: &ScopeDefinition,
) -> Result<(), OxidGeneError> {
    definition.check().map_err(OxidGeneError::Validation)?;
    require_tree_resource(conn, tree_id, TreeResource::Person, definition.person_id).await
}

/// Store `definition` as a new scope of `tree_id` in the caller's
/// transaction, its persons built at once.
pub async fn create(
    conn: &impl ConnectionTrait,
    tree_id: Uuid,
    definition: &ScopeDefinition,
) -> Result<AccessScope, OxidGeneError> {
    check_definition(conn, tree_id, definition).await?;
    let scope = AccessScopeRepo::create(conn, tree_id, definition).await?;
    rebuild(conn, &scope).await?;
    Ok(scope)
}

/// The scope a share asks for, created in the caller's transaction: none
/// for the whole tree.
pub async fn create_optional(
    conn: &impl ConnectionTrait,
    tree_id: Uuid,
    definition: Option<&ScopeDefinition>,
) -> Result<Option<Uuid>, OxidGeneError> {
    match definition {
        Some(definition) => Ok(Some(create(conn, tree_id, definition).await?.id)),
        None => Ok(None),
    }
}

/// Rebuild the persons of `scope` as of the tree's current ancestry
/// generation.
async fn rebuild(conn: &impl ConnectionTrait, scope: &AccessScope) -> Result<(), OxidGeneError> {
    // The generation first: a change committed while the links are read
    // leaves the scope stale rather than marked current.
    let Some((generation, _)) = AccessScopeRepo::generations(conn, scope.id).await? else {
        return Ok(());
    };
    let (kin, live) = kinship(conn, scope.tree_id).await?;
    let persons = scope.definition.persons(&kin, &live);
    AccessScopeRepo::replace_persons(conn, scope.id, &persons, generation).await
}

/// Make scope `scope_id` current: rebuild it on the writer `db`, in one
/// transaction, when the tree's ancestry generation moved since it was
/// built. A missing scope is `not_found`.
pub async fn ensure_current(db: &DatabaseConnection, scope_id: Uuid) -> Result<(), OxidGeneError> {
    let Some((generation, built)) = AccessScopeRepo::generations(db, scope_id).await? else {
        return Err(scope_not_found(scope_id));
    };
    if built == Some(generation) {
        return Ok(());
    }
    let txn = begin_tx(db).await?;
    if let Some(scope) = AccessScopeRepo::get(&txn, scope_id).await? {
        // Another request may have rebuilt it meanwhile.
        match AccessScopeRepo::generations(&txn, scope_id).await? {
            Some((generation, Some(built))) if built == generation => {}
            _ => rebuild(&txn, &scope).await?,
        }
    }
    commit_tx(txn).await
}

/// The persons of scope `scope_id`, rebuilt first on the writer `db` when
/// the tree's family links moved since it was built: the set a read
/// filtering on the scope keeps.
pub async fn scope_persons(
    db: &DatabaseConnection,
    scope_id: Uuid,
) -> Result<HashSet<Uuid>, OxidGeneError> {
    ensure_current(db, scope_id).await?;
    AccessScopeRepo::persons(db, scope_id).await
}

/// Whether person `person_id` is in scope `scope_id`, the scope made
/// current first: what a write naming a person checks.
pub async fn contains(
    db: &DatabaseConnection,
    scope_id: Uuid,
    person_id: Uuid,
) -> Result<bool, OxidGeneError> {
    ensure_current(db, scope_id).await?;
    AccessScopeRepo::contains(db, scope_id, person_id).await
}

fn scope_not_found(id: Uuid) -> OxidGeneError {
    OxidGeneError::NotFound {
        entity: "AccessScope",
        id,
    }
}

/// What a preview asks: a scope, or none for the whole tree.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ScopePreviewRequest {
    #[serde(default)]
    pub scope: Option<ScopeDefinition>,
}

/// What a scope reaches (docs/authorization.md §6.5, Preview).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ScopePreview {
    /// The persons the definition reaches, or the tree's live persons
    /// without one.
    pub person_count: u64,
    /// Of those, the persons a Visitor sees: neither excluded from sharing,
    /// nor private, nor contemporary under the tree's settings
    /// (docs/privacy.md §3.1). What the visibility settings announce before
    /// saving (§6.4).
    pub visitor_count: u64,
    /// Of those, the persons excluded from sharing, whom no share reaches.
    pub excluded_count: u64,
}

/// The persons `request` reaches in `tree_id`, writing nothing.
pub async fn preview(
    db: &DatabaseConnection,
    profiles: &ProfileService,
    tree_id: Uuid,
    request: &ScopePreviewRequest,
) -> Result<ScopePreview, OxidGeneError> {
    let tree = TreeRepo::get(db, tree_id).await?;
    let (kin, live) = kinship(db, tree_id).await?;
    let persons = match &request.scope {
        Some(definition) => {
            check_definition(db, tree_id, definition).await?;
            definition.persons(&kin, &live)
        }
        None => live,
    };
    let excluded = crate::service::exclusion::excluded_persons(db, tree_id).await?;
    let shared: Vec<Uuid> = persons
        .iter()
        .copied()
        .filter(|person| !excluded.contains(person))
        .collect();
    let visibility = profiles.visibility(tree_id, &shared).await?;
    let rule = tree.privacy.contemporary_rule();
    let year = chrono::Datelike::year(&chrono::Utc::now());
    let visitor_count = shared
        .iter()
        .filter(|person| {
            visibility.get(person).is_some_and(|row| {
                withheld(
                    false,
                    row.privacy,
                    tree.default_privacy,
                    row.is_contemporary(rule, year),
                )
                .is_none()
            })
        })
        .count();
    Ok(ScopePreview {
        person_count: persons.len() as u64,
        visitor_count: visitor_count as u64,
        excluded_count: (persons.len() - shared.len()) as u64,
    })
}

/// The scope Visitors of `tree_id` are limited to, none for the whole tree:
/// what the owner's sharing section edits. Its root may be a person
/// Visitors do not see, so only the owner reads it.
pub async fn visitor_scope(
    conn: &impl ConnectionTrait,
    tree_id: Uuid,
) -> Result<Option<ScopeView>, OxidGeneError> {
    match TreeRepo::get(conn, tree_id).await?.visitor_scope_id {
        Some(scope_id) => Ok(views(conn, &[scope_id]).await?.remove(&scope_id)),
        None => Ok(None),
    }
}

/// What a viewer limited to a scope sees of the tree's persons (§6.5): the
/// scope's own, and the parents, spouses and children of theirs outside
/// it, who appear as *Not shared* stubs — no name, no dates, no identifier,
/// no navigation — so that a chart keeps its shape.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScopedTree {
    pub inside: HashSet<Uuid>,
    pub boundary: HashSet<Uuid>,
}

/// The persons scope `scope_id` of `tree_id` shows and stubs, the scope
/// made current first on the writer `db`.
pub async fn scoped_tree(
    db: &DatabaseConnection,
    tree_id: Uuid,
    scope_id: Uuid,
) -> Result<ScopedTree, OxidGeneError> {
    let inside = scope_persons(db, scope_id).await?;
    let (kin, _) = kinship(db, tree_id).await?;
    let boundary = oxidgene_core::sharing::boundary(&inside, &kin);
    Ok(ScopedTree { inside, boundary })
}

/// The scope an update asks for, created in the caller's transaction:
/// `None` keeps the current one, `Some(None)` clears it.
pub async fn replacement(
    conn: &impl ConnectionTrait,
    tree_id: Uuid,
    asked: Option<&Option<ScopeDefinition>>,
) -> Result<Option<Option<Uuid>>, OxidGeneError> {
    match asked {
        Some(definition) => Ok(Some(
            create_optional(conn, tree_id, definition.as_ref()).await?,
        )),
        None => Ok(None),
    }
}

/// Delete `former` once its holder has the scope `replaced` set, if one
/// was: deleting it first would end the share it limited, never widen it.
pub async fn drop_replaced(
    conn: &impl ConnectionTrait,
    replaced: Option<Option<Uuid>>,
    former: Option<Uuid>,
) -> Result<(), OxidGeneError> {
    match (replaced, former) {
        (Some(_), Some(former)) => AccessScopeRepo::delete(conn, former).await,
        _ => Ok(()),
    }
}
