//! Repository for `PersonName` entities (CRUD, no soft delete, scoped by person_id).

use chrono::Utc;
use oxidgene_core::enums::NameType;
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::types::PersonName;
use sea_orm::entity::prelude::*;
use sea_orm::{ActiveModelTrait, IntoActiveModel, QueryFilter, Set};
use uuid::Uuid;

use crate::entities::person_name::{self, ActiveModel, Column, Entity};
use crate::entities::sea_enums;

/// Repository for person name operations.
pub struct PersonNameRepo;

impl PersonNameRepo {
    /// List all names for a person.
    pub async fn list_by_person(
        db: &DatabaseConnection,
        person_id: Uuid,
    ) -> Result<Vec<PersonName>, OxidGeneError> {
        let models = Entity::find()
            .filter(Column::PersonId.eq(person_id))
            .all(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(models.into_iter().map(into_domain).collect())
    }

    /// Get a single person name by ID.
    pub async fn get(db: &DatabaseConnection, id: Uuid) -> Result<PersonName, OxidGeneError> {
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
    #[allow(clippy::too_many_arguments)]
    pub async fn create(
        db: &DatabaseConnection,
        id: Uuid,
        person_id: Uuid,
        name_type: NameType,
        given_names: Option<String>,
        surname: Option<String>,
        prefix: Option<String>,
        suffix: Option<String>,
        nickname: Option<String>,
        is_primary: bool,
    ) -> Result<PersonName, OxidGeneError> {
        let now = Utc::now();
        let model = person_name::ActiveModel {
            id: Set(id),
            person_id: Set(person_id),
            name_type: Set(sea_enums::NameType::from(name_type)),
            given_names: Set(given_names),
            surname: Set(surname),
            prefix: Set(prefix),
            suffix: Set(suffix),
            nickname: Set(nickname),
            is_primary: Set(is_primary),
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
    #[allow(clippy::too_many_arguments)]
    pub async fn update(
        db: &DatabaseConnection,
        id: Uuid,
        name_type: Option<NameType>,
        given_names: Option<Option<String>>,
        surname: Option<Option<String>>,
        prefix: Option<Option<String>>,
        suffix: Option<Option<String>>,
        nickname: Option<Option<String>>,
        is_primary: Option<bool>,
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
        if let Some(given_names) = given_names {
            active.given_names = Set(given_names);
        }
        if let Some(surname) = surname {
            active.surname = Set(surname);
        }
        if let Some(prefix) = prefix {
            active.prefix = Set(prefix);
        }
        if let Some(suffix) = suffix {
            active.suffix = Set(suffix);
        }
        if let Some(nickname) = nickname {
            active.nickname = Set(nickname);
        }
        if let Some(is_primary) = is_primary {
            active.is_primary = Set(is_primary);
        }
        active.updated_at = Set(Utc::now());

        let result = active
            .update(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(into_domain(result))
    }

    /// Hard-delete a person name.
    pub async fn delete(db: &DatabaseConnection, id: Uuid) -> Result<(), OxidGeneError> {
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
        prefix: m.prefix,
        suffix: m.suffix,
        nickname: m.nickname,
        is_primary: m.is_primary,
        created_at: m.created_at,
        updated_at: m.updated_at,
    }
}
