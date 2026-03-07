//! Repository for `Person` entities (CRUD with soft delete, search filter).

use chrono::Utc;
use oxidgene_core::enums::Sex;
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::types::{Connection, Person};
use sea_orm::entity::prelude::*;
use sea_orm::{ActiveModelTrait, IntoActiveModel, JoinType, QueryFilter, QuerySelect, Set};
use uuid::Uuid;

use crate::entities::person::{self, ActiveModel, Column, Entity};
use crate::entities::person_name;
use crate::entities::sea_enums;
use crate::repo::pagination::{PaginationParams, paginate};

/// Parameters for server-side person search.
#[derive(Debug, Clone)]
pub struct PersonSearchParams {
    /// Surname fragment (case-insensitive contains).
    pub surname: Option<String>,
    /// Given names fragment (case-insensitive contains).
    pub given_names: Option<String>,
    /// Filter by sex.
    pub sex: Option<Sex>,
    /// Maximum results to return.
    pub limit: u64,
    /// Cursor (person ID) to start after.
    pub after: Option<Uuid>,
}

/// A person search result row, combining person + primary name data.
#[derive(Debug, Clone)]
pub struct PersonSearchRow {
    pub id: Uuid,
    pub tree_id: Uuid,
    pub sex: Sex,
    pub surname: Option<String>,
    pub given_names: Option<String>,
}

/// Repository for person CRUD operations.
pub struct PersonRepo;

impl PersonRepo {
    /// List persons in a tree with pagination (excludes soft-deleted).
    pub async fn list(
        db: &DatabaseConnection,
        tree_id: Uuid,
        params: &PaginationParams,
    ) -> Result<Connection<Person>, OxidGeneError> {
        let query = Entity::find()
            .filter(Column::TreeId.eq(tree_id))
            .filter(Column::DeletedAt.is_null());
        paginate(db, query, Column::Id, params, |m| (m.id, into_domain(m))).await
    }

    /// List all persons in a tree without pagination (excludes soft-deleted).
    pub async fn list_all(
        db: &DatabaseConnection,
        tree_id: Uuid,
    ) -> Result<Vec<Person>, OxidGeneError> {
        let models = Entity::find()
            .filter(Column::TreeId.eq(tree_id))
            .filter(Column::DeletedAt.is_null())
            .all(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(models.into_iter().map(into_domain).collect())
    }

    /// Get a single person by ID (excludes soft-deleted).
    pub async fn get(db: &DatabaseConnection, id: Uuid) -> Result<Person, OxidGeneError> {
        Entity::find_by_id(id)
            .filter(Column::DeletedAt.is_null())
            .one(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?
            .map(into_domain)
            .ok_or(OxidGeneError::NotFound {
                entity: "Person",
                id,
            })
    }

    /// Create a new person.
    pub async fn create(
        db: &DatabaseConnection,
        id: Uuid,
        tree_id: Uuid,
        sex: Sex,
    ) -> Result<Person, OxidGeneError> {
        let now = Utc::now();
        let model = person::ActiveModel {
            id: Set(id),
            tree_id: Set(tree_id),
            sex: Set(sea_enums::Sex::from(sex)),
            created_at: Set(now),
            updated_at: Set(now),
            deleted_at: Set(None),
        };
        let result = model
            .insert(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(into_domain(result))
    }

    /// Update a person's sex.
    pub async fn update(
        db: &DatabaseConnection,
        id: Uuid,
        sex: Option<Sex>,
    ) -> Result<Person, OxidGeneError> {
        let existing = Entity::find_by_id(id)
            .filter(Column::DeletedAt.is_null())
            .one(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?
            .ok_or(OxidGeneError::NotFound {
                entity: "Person",
                id,
            })?;

        let mut active: ActiveModel = existing.into_active_model();
        if let Some(sex) = sex {
            active.sex = Set(sea_enums::Sex::from(sex));
        }
        active.updated_at = Set(Utc::now());

        let result = active
            .update(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(into_domain(result))
    }

    /// Soft-delete a person.
    pub async fn delete(db: &DatabaseConnection, id: Uuid) -> Result<(), OxidGeneError> {
        let existing = Entity::find_by_id(id)
            .filter(Column::DeletedAt.is_null())
            .one(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?
            .ok_or(OxidGeneError::NotFound {
                entity: "Person",
                id,
            })?;

        let mut active: ActiveModel = existing.into_active_model();
        active.deleted_at = Set(Some(Utc::now()));
        active
            .update(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(())
    }

    /// Search persons by name (case-insensitive LIKE on surname/given_names).
    ///
    /// Performs a LEFT JOIN with `person_name` and returns matching rows with
    /// pre-resolved name data, avoiding the N+1 query pattern.
    pub async fn search(
        db: &DatabaseConnection,
        tree_id: Uuid,
        params: &PersonSearchParams,
    ) -> Result<Vec<PersonSearchRow>, OxidGeneError> {
        use sea_orm::{Condition, Order, QueryOrder, Value};

        let mut query = Entity::find()
            .join(JoinType::LeftJoin, person::Relation::PersonName.def())
            .filter(Column::TreeId.eq(tree_id))
            .filter(Column::DeletedAt.is_null());

        let mut name_cond = Condition::all();

        if let Some(ref surname) = params.surname {
            let pattern = format!("%{}%", surname.to_lowercase());
            name_cond = name_cond.add(Expr::cust_with_values(
                "LOWER(\"person_name\".\"surname\") LIKE $1",
                [Value::String(Some(Box::new(pattern)))],
            ));
        }

        if let Some(ref given) = params.given_names {
            let pattern = format!("%{}%", given.to_lowercase());
            name_cond = name_cond.add(Expr::cust_with_values(
                "LOWER(\"person_name\".\"given_names\") LIKE $1",
                [Value::String(Some(Box::new(pattern)))],
            ));
        }

        if let Some(sex) = params.sex {
            query = query.filter(Column::Sex.eq(sea_enums::Sex::from(sex)));
        }

        query = query.filter(name_cond);

        if let Some(after) = params.after {
            query = query.filter(Column::Id.gt(after));
        }

        query = query.order_by(Column::Id, Order::Asc).limit(params.limit);

        // Select person columns + name columns.
        let models: Vec<(person::Model, Option<person_name::Model>)> = query
            .find_also_related(crate::entities::person_name::Entity)
            .all(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;

        Ok(models
            .into_iter()
            .map(|(p, n)| PersonSearchRow {
                id: p.id,
                tree_id: p.tree_id,
                sex: p.sex.into(),
                surname: n.as_ref().and_then(|n| n.surname.clone()),
                given_names: n.as_ref().and_then(|n| n.given_names.clone()),
            })
            .collect())
    }
}

fn into_domain(m: person::Model) -> Person {
    Person {
        id: m.id,
        tree_id: m.tree_id,
        sex: m.sex.into(),
        created_at: m.created_at,
        updated_at: m.updated_at,
        deleted_at: m.deleted_at,
    }
}
