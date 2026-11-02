//! The rules an Editor's writes follow, checked for REST, GraphQL and MCP
//! alike before the operation runs:
//!
//! - a write naming a person the owner excluded from sharing is `404
//!   not_found`, as a read of that person is, and one touching a family
//!   with an excluded spouse `403 forbidden` (docs/privacy.md §6.4), for
//!   every viewer the exclusions apply to;
//! - a scoped Editor's (docs/authorization.md §3.3): every person a write
//!   names — the one written, and every one it links to, through a family,
//!   an event, a name, a note, a citation, a medium or a vignette — must be
//!   in the scope (`404 not_found` otherwise, as a person outside it is to
//!   a read). A person without any family link yet is accepted: it is the
//!   one a scoped Editor just created, to link it to a person already in
//!   the scope, which brings it in.
//! - for a scoped Editor too, places, sources, repositories and tags are
//!   read and linked, not edited, merged or deleted, since a change would
//!   reach records outside the scope; the dictionary's renames, bulk
//!   tagging and restoring a version are refused too (`403 forbidden`).
//!
//! What the write names is every identifier it carries — its path, its
//! body or its GraphQL arguments — that is a record of the tree: an
//! identifier of another kind names nothing to check.

use std::collections::HashSet;

use oxidgene_core::OxidGeneError;
use oxidgene_core::access::{Permission, TreeAccess};
use oxidgene_db::repo::AccessScopeRepo;
use sea_orm::DatabaseConnection;
use uuid::Uuid;

/// The permissions of a write: what a scoped viewer's rules apply to.
const WRITES: [Permission; 6] = [
    Permission::Create,
    Permission::Edit,
    Permission::PrivacyEdit,
    Permission::Delete,
    Permission::Merge,
    Permission::HistoryRestore,
];

/// The GraphQL mutations a scoped Editor may not run: changes to records
/// the whole tree shares.
const REFUSED_FIELDS: &[&str] = &[
    "revertRecord",
    "setFamilyNameParticle",
    "renameFamilyName",
    "renameOccupation",
    "updateTag",
    "deleteTag",
    "mergeTag",
    "bulkTag",
    "tagTarget",
    "updatePlace",
    "deletePlace",
    "mergePlace",
    "updateRepository",
    "deleteRepository",
    "mergeRepository",
    "updateSource",
    "deleteSource",
    "addSourceRepository",
    "updateSourceRepository",
    "removeSourceRepository",
];

/// Whether the rules on the persons a write names apply to an operation
/// needing `permission` with `access`: the writes of a scoped viewer, and
/// of a viewer the owner's exclusions apply to.
pub fn applies(access: &TreeAccess, permission: Permission) -> bool {
    (access.scope_id.is_some() || access.exclusions_apply) && WRITES.contains(&permission)
}

/// Whether the writes `access` allows may not change the records the
/// whole tree shares ([`refuses_route`], [`refuses_field`]): a scoped
/// viewer's.
pub fn refuses_shared(access: &TreeAccess) -> bool {
    access.scope_id.is_some()
}

/// Whether the REST write `route` (its template, `/api/v1` included)
/// changes a record the whole tree shares.
pub fn refuses_route(route: &str) -> bool {
    let shared = [
        "/places/{place_id}",
        "/repositories/{repository_id}",
        "/sources/{source_id}",
        "/dictionary/",
        "/history/",
        "/tags/target",
    ];
    let tag = route.contains("/tags/{tag_id}") && !route.contains("/records/");
    tag || shared.iter().any(|segment| route.contains(segment))
}

/// Whether the GraphQL mutation `field` changes a record the whole tree
/// shares.
pub fn refuses_field(field: &str) -> bool {
    REFUSED_FIELDS.contains(&field)
}

/// What the persons a write names are checked against: the owner's
/// exclusions, and the viewer's scope.
#[derive(Debug)]
pub struct NamedCheck {
    scope_id: Option<Uuid>,
    excluded: HashSet<Uuid>,
}

impl NamedCheck {
    /// The check of `access`'s writes in `tree_id`, the exclusions and the
    /// scope made current first on the writer `db`; none when nothing a
    /// write names can be refused — no scope, and no exclusion applying.
    pub async fn of(
        db: &DatabaseConnection,
        access: &TreeAccess,
        tree_id: Uuid,
    ) -> Result<Option<Self>, OxidGeneError> {
        let excluded = if access.exclusions_apply {
            crate::service::exclusion::excluded_persons(db, tree_id).await?
        } else {
            HashSet::new()
        };
        if access.scope_id.is_none() && excluded.is_empty() {
            return Ok(None);
        }
        Ok(Some(Self {
            scope_id: access.scope_id,
            excluded,
        }))
    }

    /// Check what `ids` name in `tree_id`: no excluded person (`404`), no
    /// family with an excluded spouse (`403`), and for a scoped viewer every
    /// person in the scope (`404`).
    pub async fn check(
        &self,
        db: &DatabaseConnection,
        tree_id: Uuid,
        ids: &HashSet<Uuid>,
    ) -> Result<(), OxidGeneError> {
        let ids: Vec<Uuid> = ids.iter().copied().filter(|id| *id != tree_id).collect();
        if ids.is_empty() {
            return Ok(());
        }
        let named = AccessScopeRepo::records_named(db, tree_id, &ids).await?;
        if let Some(person) = named.persons.iter().find(|p| self.excluded.contains(p)) {
            return Err(OxidGeneError::NotFound {
                entity: "Person",
                id: *person,
            });
        }
        if named.spouses.iter().any(|p| self.excluded.contains(p)) {
            return Err(OxidGeneError::Forbidden);
        }
        let Some(scope_id) = self.scope_id else {
            return Ok(());
        };
        let named = named.all_persons();
        if named.is_empty() {
            return Ok(());
        }
        let scope = crate::service::access_scope::scope_persons(db, scope_id).await?;
        let outside: Vec<Uuid> = named
            .into_iter()
            .filter(|person| !scope.contains(person))
            .collect();
        let linked = AccessScopeRepo::linked_persons(db, &outside).await?;
        match outside.into_iter().find(|person| linked.contains(person)) {
            Some(person) => Err(OxidGeneError::NotFound {
                entity: "Person",
                id: person,
            }),
            None => Ok(()),
        }
    }
}

/// Check what `ids` name in `tree_id` against `access`'s
/// [`NamedCheck`], made on the writer `db`.
pub async fn check_named(
    db: &DatabaseConnection,
    access: &TreeAccess,
    tree_id: Uuid,
    ids: &HashSet<Uuid>,
) -> Result<(), OxidGeneError> {
    match NamedCheck::of(db, access, tree_id).await? {
        Some(check) => check.check(db, tree_id, ids).await,
        None => Ok(()),
    }
}

/// Every identifier in the JSON value `value`, at any depth.
pub fn ids_in_json(value: &serde_json::Value, ids: &mut HashSet<Uuid>) {
    match value {
        serde_json::Value::String(text) => {
            if let Ok(id) = Uuid::parse_str(text) {
                ids.insert(id);
            }
        }
        serde_json::Value::Array(items) => items.iter().for_each(|item| ids_in_json(item, ids)),
        serde_json::Value::Object(fields) => {
            fields.values().for_each(|item| ids_in_json(item, ids))
        }
        _ => {}
    }
}

/// Every identifier among the segments of `path`.
pub fn ids_in_path(path: &str, ids: &mut HashSet<Uuid>) {
    ids.extend(
        path.split('/')
            .filter_map(|segment| Uuid::parse_str(segment).ok()),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxidgene_core::access::TreeRole;

    #[test]
    fn shared_records_are_refused_and_records_of_persons_are_not() {
        for refused in [
            "/api/v1/trees/{tree_id}/places/{place_id}",
            "/api/v1/trees/{tree_id}/places/{place_id}/merge",
            "/api/v1/trees/{tree_id}/sources/{source_id}/repositories",
            "/api/v1/trees/{tree_id}/tags/{tag_id}/bulk",
            "/api/v1/trees/{tree_id}/tags/target",
            "/api/v1/trees/{tree_id}/dictionary/family-names/rename",
            "/api/v1/trees/{tree_id}/history/{record_type}/{record_id}/revert",
        ] {
            assert!(refuses_route(refused), "{refused}");
        }
        for allowed in [
            "/api/v1/trees/{tree_id}/places",
            "/api/v1/trees/{tree_id}/persons/{person_id}",
            "/api/v1/trees/{tree_id}/tags/{tag_id}/records/{kind}/{record_id}",
            "/api/v1/trees/{tree_id}/families/{family_id}/children",
        ] {
            assert!(!refuses_route(allowed), "{allowed}");
        }
        assert!(refuses_field("mergePlace") && !refuses_field("createPlace"));
    }

    #[test]
    fn an_editors_writes_follow_the_rules_and_the_owners_do_not() {
        let owner = TreeAccess::of_role(TreeRole::Owner);
        assert!(!applies(&owner, Permission::Edit));
        let editor = TreeAccess::of_role(TreeRole::Editor);
        assert!(applies(&editor, Permission::Edit) && !refuses_shared(&editor));
        let scoped = editor.scoped(Some(Uuid::now_v7()));
        assert!(applies(&scoped, Permission::Edit) && refuses_shared(&scoped));
        assert!(!applies(&scoped, Permission::Read));
    }

    #[test]
    fn identifiers_are_found_at_any_depth() {
        let (a, b, c) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
        let mut ids = HashSet::new();
        ids_in_json(
            &serde_json::json!({ "person_id": a.to_string(), "names": [{ "x": b.to_string() }], "n": 3 }),
            &mut ids,
        );
        ids_in_path(&format!("/api/v1/trees/{c}/persons"), &mut ids);
        assert_eq!(ids, HashSet::from([a, b, c]));
    }
}
