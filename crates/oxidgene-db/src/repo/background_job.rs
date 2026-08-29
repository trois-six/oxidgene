//! Durable queue operations for import and export workers.

use chrono::{Duration, Utc};
use oxidgene_core::OxidGeneError;
use sea_orm::entity::prelude::*;
use sea_orm::{ActiveValue::Set, Condition, ExprTrait, QueryOrder, sea_query::Expr};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entities::background_job::{self, Column, Entity};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundJobKind {
    Import,
    Export,
}

impl BackgroundJobKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Import => "import",
            Self::Export => "export",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundJobStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl BackgroundJobStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone)]
pub struct NewBackgroundJob {
    pub id: Uuid,
    pub tree_id: Uuid,
    pub kind: BackgroundJobKind,
    pub format: String,
    pub source_key: Option<String>,
    pub original_filename: Option<String>,
    pub merge_occupations: bool,
    pub merge_names: bool,
}

pub type BackgroundJob = background_job::Model;

pub struct BackgroundJobRepo;

impl BackgroundJobRepo {
    pub async fn create(
        db: &impl ConnectionTrait,
        input: NewBackgroundJob,
    ) -> Result<BackgroundJob, OxidGeneError> {
        let now = Utc::now();
        background_job::ActiveModel {
            id: Set(input.id),
            tree_id: Set(input.tree_id),
            active_tree_id: Set(Some(input.tree_id)),
            kind: Set(input.kind.as_str().to_string()),
            format: Set(input.format),
            status: Set(BackgroundJobStatus::Queued.as_str().to_string()),
            phase: Set("queued".to_string()),
            source_key: Set(input.source_key),
            artifact_key: Set(None),
            original_filename: Set(input.original_filename),
            merge_occupations: Set(input.merge_occupations),
            merge_names: Set(input.merge_names),
            done: Set(0),
            total: Set(0),
            attempt: Set(0),
            lease_owner: Set(None),
            lease_until: Set(None),
            cancel_requested: Set(false),
            result_json: Set(None),
            error_code: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
            started_at: Set(None),
            finished_at: Set(None),
        }
        .insert(db)
        .await
        .map_err(|error| OxidGeneError::Database(error.to_string()))
    }

    pub async fn get_in_tree(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        id: Uuid,
    ) -> Result<BackgroundJob, OxidGeneError> {
        Entity::find_by_id(id)
            .filter(Column::TreeId.eq(tree_id))
            .one(db)
            .await
            .map_err(|error| OxidGeneError::Database(error.to_string()))?
            .ok_or(OxidGeneError::NotFound {
                entity: "BackgroundJob",
                id,
            })
    }

    pub async fn active_for_tree(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<Option<BackgroundJob>, OxidGeneError> {
        Entity::find()
            .filter(Column::ActiveTreeId.eq(tree_id))
            .one(db)
            .await
            .map_err(|error| OxidGeneError::Database(error.to_string()))
    }

    pub async fn active_imports(
        db: &impl ConnectionTrait,
    ) -> Result<Vec<BackgroundJob>, OxidGeneError> {
        Entity::find()
            .filter(Column::ActiveTreeId.is_not_null())
            .filter(Column::Kind.eq(BackgroundJobKind::Import.as_str()))
            .all(db)
            .await
            .map_err(|error| OxidGeneError::Database(error.to_string()))
    }

    /// Requeue interrupted jobs when starting the single-worker SQLite runtime.
    pub async fn requeue_running(db: &impl ConnectionTrait) -> Result<u64, OxidGeneError> {
        let now = Utc::now();
        Entity::update_many()
            .col_expr(
                Column::Status,
                Expr::value(BackgroundJobStatus::Queued.as_str()),
            )
            .col_expr(Column::LeaseOwner, Expr::value(Option::<String>::None))
            .col_expr(Column::LeaseUntil, Expr::value(Option::<DateTimeUtc>::None))
            .col_expr(Column::UpdatedAt, Expr::value(now))
            .filter(Column::Status.eq(BackgroundJobStatus::Running.as_str()))
            .exec(db)
            .await
            .map(|result| result.rows_affected)
            .map_err(|error| OxidGeneError::Database(error.to_string()))
    }

    /// Claim the oldest queued job or a running job whose worker lease expired.
    pub async fn claim_next(
        db: &impl ConnectionTrait,
        worker_id: &str,
        lease_duration: Duration,
    ) -> Result<Option<BackgroundJob>, OxidGeneError> {
        let now = Utc::now();
        let claimable = claimable_condition(now);
        let Some(candidate) = Entity::find()
            .filter(claimable.clone())
            .order_by_asc(Column::CreatedAt)
            .one(db)
            .await
            .map_err(|error| OxidGeneError::Database(error.to_string()))?
        else {
            return Ok(None);
        };

        let updated = Entity::update_many()
            .col_expr(
                Column::Status,
                Expr::value(BackgroundJobStatus::Running.as_str()),
            )
            .col_expr(Column::LeaseOwner, Expr::value(worker_id))
            .col_expr(Column::LeaseUntil, Expr::value(now + lease_duration))
            .col_expr(Column::Attempt, Expr::col(Column::Attempt).add(1))
            .col_expr(Column::StartedAt, Expr::value(Some(now)))
            .col_expr(Column::UpdatedAt, Expr::value(now))
            .filter(Column::Id.eq(candidate.id))
            .filter(claimable)
            .exec(db)
            .await
            .map_err(|error| OxidGeneError::Database(error.to_string()))?;
        if updated.rows_affected == 0 {
            return Ok(None);
        }
        Self::get_in_tree(db, candidate.tree_id, candidate.id)
            .await
            .map(Some)
    }

    pub async fn progress(
        db: &impl ConnectionTrait,
        id: Uuid,
        worker_id: &str,
        phase: &str,
        done: i64,
        total: i64,
        lease_duration: Duration,
    ) -> Result<bool, OxidGeneError> {
        let now = Utc::now();
        let result = Entity::update_many()
            .col_expr(Column::Phase, Expr::value(phase))
            .col_expr(Column::Done, Expr::value(done))
            .col_expr(Column::Total, Expr::value(total))
            .col_expr(Column::LeaseUntil, Expr::value(now + lease_duration))
            .col_expr(Column::UpdatedAt, Expr::value(now))
            .filter(Column::Id.eq(id))
            .filter(Column::Status.eq(BackgroundJobStatus::Running.as_str()))
            .filter(Column::LeaseOwner.eq(worker_id))
            .exec(db)
            .await
            .map_err(|error| OxidGeneError::Database(error.to_string()))?;
        Ok(result.rows_affected == 1)
    }

    pub async fn checkpoint_import_persisted(
        db: &impl ConnectionTrait,
        id: Uuid,
        worker_id: &str,
        result_json: String,
        lease_duration: Duration,
    ) -> Result<bool, OxidGeneError> {
        let now = Utc::now();
        let result = Entity::update_many()
            .col_expr(Column::Phase, Expr::value("projections"))
            .col_expr(Column::ResultJson, Expr::value(result_json))
            .col_expr(Column::LeaseUntil, Expr::value(now + lease_duration))
            .col_expr(Column::UpdatedAt, Expr::value(now))
            .filter(Column::Id.eq(id))
            .filter(Column::Status.eq(BackgroundJobStatus::Running.as_str()))
            .filter(Column::LeaseOwner.eq(worker_id))
            .exec(db)
            .await
            .map_err(|error| OxidGeneError::Database(error.to_string()))?;
        Ok(result.rows_affected == 1)
    }

    pub async fn complete(
        db: &impl ConnectionTrait,
        id: Uuid,
        worker_id: &str,
        artifact_key: Option<String>,
        result_json: Option<String>,
    ) -> Result<bool, OxidGeneError> {
        Self::finish(
            db,
            id,
            worker_id,
            BackgroundJobStatus::Completed,
            "completed",
            artifact_key,
            result_json,
            None,
        )
        .await
    }

    pub async fn fail(
        db: &impl ConnectionTrait,
        id: Uuid,
        worker_id: &str,
        error_code: &str,
    ) -> Result<bool, OxidGeneError> {
        Self::finish(
            db,
            id,
            worker_id,
            BackgroundJobStatus::Failed,
            "failed",
            None,
            None,
            Some(error_code.to_string()),
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn finish(
        db: &impl ConnectionTrait,
        id: Uuid,
        worker_id: &str,
        status: BackgroundJobStatus,
        phase: &str,
        artifact_key: Option<String>,
        result_json: Option<String>,
        error_code: Option<String>,
    ) -> Result<bool, OxidGeneError> {
        let now = Utc::now();
        let result = Entity::update_many()
            .col_expr(Column::ActiveTreeId, Expr::value(Option::<Uuid>::None))
            .col_expr(Column::Status, Expr::value(status.as_str()))
            .col_expr(Column::Phase, Expr::value(phase))
            .col_expr(Column::ArtifactKey, Expr::value(artifact_key))
            .col_expr(Column::ResultJson, Expr::value(result_json))
            .col_expr(Column::ErrorCode, Expr::value(error_code))
            .col_expr(Column::LeaseOwner, Expr::value(Option::<String>::None))
            .col_expr(Column::LeaseUntil, Expr::value(Option::<DateTimeUtc>::None))
            .col_expr(Column::FinishedAt, Expr::value(Some(now)))
            .col_expr(Column::UpdatedAt, Expr::value(now))
            .filter(Column::Id.eq(id))
            .filter(Column::Status.eq(BackgroundJobStatus::Running.as_str()))
            .filter(Column::LeaseOwner.eq(worker_id))
            .exec(db)
            .await
            .map_err(|error| OxidGeneError::Database(error.to_string()))?;
        Ok(result.rows_affected == 1)
    }
}

fn claimable_condition(now: DateTimeUtc) -> Condition {
    Condition::any()
        .add(Column::Status.eq(BackgroundJobStatus::Queued.as_str()))
        .add(
            Condition::all()
                .add(Column::Status.eq(BackgroundJobStatus::Running.as_str()))
                .add(Column::LeaseUntil.lt(now)),
        )
}
