//! Repository for `PersonName` entities (CRUD, no soft delete, scoped by person_id).

use chrono::Utc;
use oxidgene_core::enums::NameType;
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::types::PersonName;
use sea_orm::entity::prelude::*;
use sea_orm::{
    ActiveModelTrait, ConnectionTrait, IntoActiveModel, JoinType, QueryFilter, QueryOrder,
    QuerySelect, Set,
};
use uuid::Uuid;

use crate::entities::person;
use crate::entities::person_name::{self, ActiveModel, Column, Entity};
use crate::entities::sea_enums;

/// The writable pieces of a name, as a group.
///
/// Passed as a struct rather than as positional arguments because `prefix`
/// (GEDCOM `NPFX`, "Dr.") and `surname_prefix` (GEDCOM `SPFX`, "de la") are
/// both `Option<String>` and mean entirely different things — positionally
/// they would be silently swappable.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PersonNamePieces {
    pub given_names: Option<String>,
    /// Surname root, particle excluded.
    pub surname: Option<String>,
    /// The surname particle (GEDCOM `SPFX`).
    pub surname_prefix: Option<String>,
    /// Name prefix / title (GEDCOM `NPFX`).
    pub prefix: Option<String>,
    pub suffix: Option<String>,
    pub nickname: Option<String>,
}

/// A partial update of [`PersonNamePieces`].
///
/// The double `Option` is deliberate: the outer level distinguishes "leave
/// this piece alone" from "set this piece", the inner one carries the new
/// value, which may itself be `None` to clear the piece.
#[derive(Debug, Clone, Default)]
pub struct PersonNamePiecesPatch {
    pub given_names: Option<Option<String>>,
    pub surname: Option<Option<String>>,
    pub surname_prefix: Option<Option<String>>,
    pub prefix: Option<Option<String>>,
    pub suffix: Option<Option<String>>,
    pub nickname: Option<Option<String>>,
}

/// Repository for person name operations.
pub struct PersonNameRepo;

impl PersonNameRepo {
    /// List all names for a person.
    pub async fn list_by_person(
        db: &impl ConnectionTrait,
        person_id: Uuid,
    ) -> Result<Vec<PersonName>, OxidGeneError> {
        let models = Entity::find()
            .filter(Column::PersonId.eq(person_id))
            // Primary name first, then the author's chosen order; `id` is a
            // UUID v7 so it breaks ties by creation time.
            .order_by_desc(Column::IsPrimary)
            .order_by_asc(Column::SortOrder)
            .order_by_asc(Column::Id)
            .all(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(models.into_iter().map(into_domain).collect())
    }

    /// List all names for multiple persons.
    pub async fn list_by_persons(
        db: &impl ConnectionTrait,
        person_ids: &[Uuid],
    ) -> Result<Vec<PersonName>, OxidGeneError> {
        let models = Entity::find()
            .filter(Column::PersonId.is_in(person_ids.iter().copied()))
            .order_by_desc(Column::IsPrimary)
            .order_by_asc(Column::SortOrder)
            .order_by_asc(Column::Id)
            .all(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(models.into_iter().map(into_domain).collect())
    }

    /// List names for a bounded set of active persons in one tree.
    pub async fn list_by_persons_in_tree(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        person_ids: &[Uuid],
    ) -> Result<Vec<PersonName>, OxidGeneError> {
        let models = Entity::find()
            .join(JoinType::InnerJoin, person_name::Relation::Person.def())
            .filter(person::Column::TreeId.eq(tree_id))
            .filter(person::Column::DeletedAt.is_null())
            .filter(Column::PersonId.is_in(person_ids.iter().copied()))
            .order_by_desc(Column::IsPrimary)
            .order_by_asc(Column::SortOrder)
            .order_by_asc(Column::Id)
            .all(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(models.into_iter().map(into_domain).collect())
    }

    /// Get a single person name by ID.
    pub async fn get(db: &impl ConnectionTrait, id: Uuid) -> Result<PersonName, OxidGeneError> {
        Entity::find_by_id(id)
            .one(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?
            .map(into_domain)
            .ok_or(OxidGeneError::NotFound {
                entity: "PersonName",
                id,
            })
    }

    /// Create a new person name.
    pub async fn create(
        db: &impl ConnectionTrait,
        id: Uuid,
        person_id: Uuid,
        name_type: NameType,
        pieces: PersonNamePieces,
        is_primary: bool,
        sort_order: i32,
    ) -> Result<PersonName, OxidGeneError> {
        let now = Utc::now();
        let model = person_name::ActiveModel {
            id: Set(id),
            person_id: Set(person_id),
            name_type: Set(sea_enums::NameType::from(name_type)),
            given_names: Set(pieces.given_names),
            surname: Set(pieces.surname),
            surname_prefix: Set(pieces.surname_prefix),
            prefix: Set(pieces.prefix),
            suffix: Set(pieces.suffix),
            nickname: Set(pieces.nickname),
            is_primary: Set(is_primary),
            sort_order: Set(sort_order),
            created_at: Set(now),
            updated_at: Set(now),
        };
        let result = model
            .insert(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(into_domain(result))
    }

    /// Update a person name.
    pub async fn update(
        db: &impl ConnectionTrait,
        id: Uuid,
        name_type: Option<NameType>,
        pieces: PersonNamePiecesPatch,
        is_primary: Option<bool>,
        sort_order: Option<i32>,
    ) -> Result<PersonName, OxidGeneError> {
        let existing = Entity::find_by_id(id)
            .one(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?
            .ok_or(OxidGeneError::NotFound {
                entity: "PersonName",
                id,
            })?;

        let mut active: ActiveModel = existing.into_active_model();
        if let Some(name_type) = name_type {
            active.name_type = Set(sea_enums::NameType::from(name_type));
        }
        if let Some(given_names) = pieces.given_names {
            active.given_names = Set(given_names);
        }
        if let Some(surname) = pieces.surname {
            active.surname = Set(surname);
        }
        if let Some(surname_prefix) = pieces.surname_prefix {
            active.surname_prefix = Set(surname_prefix);
        }
        if let Some(prefix) = pieces.prefix {
            active.prefix = Set(prefix);
        }
        if let Some(suffix) = pieces.suffix {
            active.suffix = Set(suffix);
        }
        if let Some(nickname) = pieces.nickname {
            active.nickname = Set(nickname);
        }
        if let Some(is_primary) = is_primary {
            active.is_primary = Set(is_primary);
        }
        if let Some(sort_order) = sort_order {
            active.sort_order = Set(sort_order);
        }
        active.updated_at = Set(Utc::now());

        let result = active
            .update(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(into_domain(result))
    }

    /// Hard-delete a person name.
    pub async fn delete(db: &impl ConnectionTrait, id: Uuid) -> Result<(), OxidGeneError> {
        let result = Entity::delete_by_id(id)
            .exec(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        if result.rows_affected == 0 {
            return Err(OxidGeneError::NotFound {
                entity: "PersonName",
                id,
            });
        }
        Ok(())
    }
}

fn into_domain(m: person_name::Model) -> PersonName {
    PersonName {
        id: m.id,
        person_id: m.person_id,
        name_type: m.name_type.into(),
        given_names: m.given_names,
        surname: m.surname,
        surname_prefix: m.surname_prefix,
        prefix: m.prefix,
        suffix: m.suffix,
        nickname: m.nickname,
        is_primary: m.is_primary,
        sort_order: m.sort_order,
        created_at: m.created_at,
        updated_at: m.updated_at,
    }
}
