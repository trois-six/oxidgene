//! The two rules a scoped Editor's writes follow (docs/authorization.md
//! §3.3), checked for REST and GraphQL alike before the operation runs:
//!
//! - every person a write names — the one written, and every one it links
//!   to, through a family, an event, a name, a note, a citation, a medium
//!   or a vignette — must be in the scope (`404 not_found` otherwise, as a
//!   person outside it is to a read). A person without any family link yet
//!   is accepted: it is the one a scoped Editor just created, to link it to
//!   a person already in the scope, which brings it in.
//! - places, sources, repositories and tags are read and linked, not
//!   edited, merged or deleted, since a change would reach records outside
//!   the scope; the dictionary's renames, bulk tagging and restoring a
//!   version are refused too (`403 forbidden`).
//!
//! What the write names is every identifier it carries — its path, its
//! JSON body or its GraphQL arguments — that is a record of the tree: an
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

/// Whether a scoped viewer's rules apply to an operation needing
/// `permission` with `access`.
pub fn applies(access: &TreeAccess, permission: Permission) -> bool {
    access.scope_id.is_some() && WRITES.contains(&permission)
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

/// Check that every person `ids` name in `tree_id` is in the scope of
/// `access`, the scope made current first on the writer `db`.
pub async fn check_named(
    db: &DatabaseConnection,
    access: &TreeAccess,
    tree_id: Uuid,
    ids: &HashSet<Uuid>,
) -> Result<(), OxidGeneError> {
    let Some(scope_id) = access.scope_id else {
        return Ok(());
    };
    let ids: Vec<Uuid> = ids.iter().copied().filter(|id| *id != tree_id).collect();
    if ids.is_empty() {
        return Ok(());
    }
    let named = AccessScopeRepo::persons_named(db, tree_id, &ids).await?;
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
    fn only_a_scoped_viewers_writes_follow_the_rules() {
        let editor = TreeAccess::of_role(TreeRole::Editor);
        assert!(!applies(&editor, Permission::Edit));
        let scoped = editor.scoped(Some(Uuid::now_v7()));
        assert!(applies(&scoped, Permission::Edit));
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
