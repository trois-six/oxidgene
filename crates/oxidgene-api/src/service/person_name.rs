//! Person-name writes: the same steps whether REST or GraphQL asked.
//!
//! A name is addressed through its person, and must be that person's: a name
//! of somebody else — in this tree or another — is not found. Each write
//! rewrites the projections that show the name — the person's own and their
//! relatives' — and records the change against the name's owner, in one
//! transaction.

use oxidgene_core::OxidGeneError;
use oxidgene_core::enums::NameType;
use oxidgene_core::history::AuditEntity;
use oxidgene_core::types::PersonName;
use oxidgene_db::repo::{PersonNamePieces, PersonNamePiecesPatch, PersonNameRepo};
use oxidgene_db::sea_orm::{ConnectionTrait, DatabaseConnection};
use serde::Deserialize;
use uuid::Uuid;

use crate::profile::{ProfileService, invalidation};
use crate::service::history::Change;
use crate::service::patch::double_option;
use crate::service::scope::{TreeResource, begin_tx, commit_tx, require_tree_resource};

/// A name to give a person.
#[derive(Debug, Deserialize)]
pub struct NewPersonName {
    pub name_type: NameType,
    pub given_names: Option<String>,
    /// The surname root, particle excluded.
    ///
    /// Stored verbatim: the server does not detect a particle hiding in it.
    /// Callers holding a full surname split it with
    /// `oxidgene_core::types::split_surname_particle` first, as the UI does.
    pub surname: Option<String>,
    /// The surname particle, GEDCOM `SPFX` ("de la", "van der").
    #[serde(default)]
    pub surname_prefix: Option<String>,
    pub prefix: Option<String>,
    pub suffix: Option<String>,
    pub nickname: Option<String>,
    pub is_primary: bool,
    #[serde(default)]
    pub sort_order: i32,
}

/// The fields a name update changes: `None` keeps a field, and `Some(None)`
/// clears an optional one.
#[derive(Debug, Default, Deserialize)]
pub struct PersonNamePatch {
    pub name_type: Option<NameType>,
    #[serde(default, deserialize_with = "double_option")]
    pub given_names: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub surname: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub surname_prefix: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub prefix: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub suffix: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub nickname: Option<Option<String>>,
    pub is_primary: Option<bool>,
    pub sort_order: Option<i32>,
}

/// The names of person `person_id` of `tree_id`.
pub async fn list_person_names(
    db: &DatabaseConnection,
    tree_id: Uuid,
    person_id: Uuid,
) -> Result<Vec<PersonName>, OxidGeneError> {
    require_tree_resource(db, tree_id, TreeResource::Person, person_id).await?;
    PersonNameRepo::list_by_person(db, person_id).await
}

/// Give person `person_id` of `tree_id` a name.
pub async fn create_person_name(
    db: &DatabaseConnection,
    profiles: &ProfileService,
    tree_id: Uuid,
    person_id: Uuid,
    new: NewPersonName,
) -> Result<PersonName, OxidGeneError> {
    let id = Uuid::now_v7();
    let txn = begin_tx(db).await?;
    require_tree_resource(&txn, tree_id, TreeResource::Person, person_id).await?;
    let pending = Change::create(tree_id, AuditEntity::PersonName, id)
        .person(person_id)
        .prepare(&txn)
        .await?;
    let name = PersonNameRepo::create(
        &txn,
        id,
        person_id,
        new.name_type,
        PersonNamePieces {
            given_names: new.given_names,
            surname: new.surname,
            surname_prefix: new.surname_prefix,
            prefix: new.prefix,
            suffix: new.suffix,
            nickname: new.nickname,
        },
        new.is_primary,
        new.sort_order,
    )
    .await?;
    refresh(&txn, profiles, tree_id, person_id).await?;
    pending.record(&txn).await?;
    commit_tx(txn).await?;
    Ok(name)
}

/// Update name `name_id` of person `person_id` of `tree_id`.
pub async fn update_person_name(
    db: &DatabaseConnection,
    profiles: &ProfileService,
    tree_id: Uuid,
    person_id: Uuid,
    name_id: Uuid,
    patch: PersonNamePatch,
) -> Result<PersonName, OxidGeneError> {
    let txn = begin_tx(db).await?;
    require_owned_name(&txn, tree_id, person_id, name_id).await?;
    let pending = Change::update(tree_id, AuditEntity::PersonName, name_id)
        .person(person_id)
        .prepare(&txn)
        .await?;
    let name = PersonNameRepo::update(
        &txn,
        name_id,
        patch.name_type,
        PersonNamePiecesPatch {
            given_names: patch.given_names,
            surname: patch.surname,
            surname_prefix: patch.surname_prefix,
            prefix: patch.prefix,
            suffix: patch.suffix,
            nickname: patch.nickname,
        },
        patch.is_primary,
        patch.sort_order,
    )
    .await?;
    refresh(&txn, profiles, tree_id, person_id).await?;
    pending.record(&txn).await?;
    commit_tx(txn).await?;
    Ok(name)
}

/// Delete name `name_id` of person `person_id` of `tree_id`.
pub async fn delete_person_name(
    db: &DatabaseConnection,
    profiles: &ProfileService,
    tree_id: Uuid,
    person_id: Uuid,
    name_id: Uuid,
) -> Result<(), OxidGeneError> {
    let txn = begin_tx(db).await?;
    require_owned_name(&txn, tree_id, person_id, name_id).await?;
    let pending = Change::delete(tree_id, AuditEntity::PersonName, name_id)
        .person(person_id)
        .prepare(&txn)
        .await?;
    PersonNameRepo::delete(&txn, name_id).await?;
    refresh(&txn, profiles, tree_id, person_id).await?;
    pending.record(&txn).await?;
    commit_tx(txn).await
}

/// Fail with `NotFound` unless name `name_id` belongs to person `person_id`,
/// and that person to tree `tree_id`.
async fn require_owned_name(
    db: &impl ConnectionTrait,
    tree_id: Uuid,
    person_id: Uuid,
    name_id: Uuid,
) -> Result<(), OxidGeneError> {
    require_tree_resource(db, tree_id, TreeResource::Person, person_id).await?;
    if PersonNameRepo::get(db, name_id).await?.person_id != person_id {
        return Err(OxidGeneError::NotFound {
            entity: "PersonName",
            id: name_id,
        });
    }
    Ok(())
}

/// Rewrite the projections showing `person_id`'s names: their own, and the
/// relatives' that embed their display name.
async fn refresh(
    db: &impl ConnectionTrait,
    profiles: &ProfileService,
    tree_id: Uuid,
    person_id: Uuid,
) -> Result<(), OxidGeneError> {
    let affected = invalidation::affected_persons(db, person_id).await?;
    profiles
        .invalidate_for_mutation(db, tree_id, &affected)
        .await
}
