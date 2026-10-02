//! Durable queue operations for import and export workers.

use std::collections::HashSet;

use chrono::{Duration, Utc};
use oxidgene_core::OxidGeneError;
use sea_orm::entity::prelude::*;
use sea_orm::{
    ActiveValue::Set, Condition, ExprTrait, FromQueryResult, QueryOrder, QuerySelect,
    sea_query::Expr,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entities::background_job::{self, Column, Entity};
use crate::repo::batch::{MAX_BOUND_IDS, in_chunks};
use crate::repo::db_err;

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
    pub payload_json: Option<String>,
    pub original_filename: Option<String>,
    pub merge_occupations: bool,
    pub merge_names: bool,
    pub include_notes_and_sources: bool,
    pub include_media: bool,
}

pub type BackgroundJob = background_job::Model;

/// The only active-import fields needed by the tree list.
#[derive(Debug, Clone, Copy, FromQueryResult)]
pub struct ActiveImport {
    pub id: Uuid,
    pub tree_id: Uuid,
}

pub struct BackgroundJobRepo;

impl BackgroundJobRepo {
    pub async fn create(
        db: &impl ConnectionTrait,
        input: NewBackgroundJob,
    ) -> Result<BackgroundJob, OxidGeneError> {
        let now = Utc::now();
        #[cfg(feature = "telemetry-context")]
        let (trace_parent, trace_state) = oxidgene_observability::current_trace_context();
        #[cfg(not(feature = "telemetry-context"))]
        let (trace_parent, trace_state) = (None, None);
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
            payload_json: Set(input.payload_json),
            original_filename: Set(input.original_filename),
            merge_occupations: Set(input.merge_occupations),
            merge_names: Set(input.merge_names),
            include_notes_and_sources: Set(input.include_notes_and_sources),
            include_media: Set(input.include_media),
            done: Set(0),
            total: Set(0),
            attempt: Set(0),
            lease_owner: Set(None),
            lease_until: Set(None),
            cancel_requested: Set(false),
            result_json: Set(None),
            error_code: Set(None),
            trace_parent: Set(trace_parent),
            trace_state: Set(trace_state),
            created_at: Set(now),
            updated_at: Set(now),
            started_at: Set(None),
            finished_at: Set(None),
        }
        .insert(db)
        .await
        .map_err(|error| match error.sql_err() {
            // `idx_background_job_active_tree` lets a tree hold one queued or
            // running job at a time: a second one is the caller's conflict,
            // not a database failure.
            Some(sea_orm::SqlErr::UniqueConstraintViolation(_)) => OxidGeneError::Conflict(
                "the tree already has an import or export in progress".into(),
            ),
            _ => db_err(error),
        })
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
            .map_err(db_err)?
            .ok_or(OxidGeneError::NotFound {
                entity: "BackgroundJob",
                id,
            })
    }

    pub async fn active_imports(
        db: &impl ConnectionTrait,
    ) -> Result<Vec<ActiveImport>, OxidGeneError> {
        Entity::find()
            .select_only()
            .columns([Column::Id, Column::TreeId])
            .filter(Column::ActiveTreeId.is_not_null())
            .filter(Column::Kind.eq(BackgroundJobKind::Import.as_str()))
            .into_model::<ActiveImport>()
            .all(db)
            .await
            .map_err(db_err)
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
            .map_err(db_err)
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
            .map_err(db_err)?
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
            .map_err(db_err)?;
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
            .map_err(db_err)?;
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
            .map_err(db_err)?;
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

    #[expect(
        clippy::too_many_arguments,
        reason = "one parameter per column a job's completion writes"
    )]
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
            // Nothing reads a job's inputs once it has ended, and a Geneanet
            // import's are megabytes of the account's collection.
            .col_expr(Column::PayloadJson, Expr::value(Option::<String>::None))
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
            .map_err(db_err)?;
        Ok(result.rows_affected == 1)
    }
}

/// The statuses of a job that has ended.
const TERMINAL: [BackgroundJobStatus; 3] = [
    BackgroundJobStatus::Completed,
    BackgroundJobStatus::Failed,
    BackgroundJobStatus::Cancelled,
];

impl BackgroundJobRepo {
    /// The ids of a tree's jobs, whatever their state.
    pub async fn ids_in_tree(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<Vec<Uuid>, OxidGeneError> {
        Entity::find()
            .select_only()
            .column(Column::Id)
            .filter(Column::TreeId.eq(tree_id))
            .into_tuple()
            .all(db)
            .await
            .map_err(db_err)
    }

    /// The completed exports that finished before `cutoff` and still record
    /// an artifact, with its key.
    pub async fn artifacts_finished_before(
        db: &impl ConnectionTrait,
        cutoff: DateTimeUtc,
    ) -> Result<Vec<(Uuid, String)>, OxidGeneError> {
        Entity::find()
            .select_only()
            .columns([Column::Id, Column::ArtifactKey])
            .filter(Column::Kind.eq(BackgroundJobKind::Export.as_str()))
            .filter(Column::Status.eq(BackgroundJobStatus::Completed.as_str()))
            .filter(Column::ArtifactKey.is_not_null())
            .filter(Column::FinishedAt.lt(cutoff))
            .into_tuple()
            .all(db)
            .await
            .map_err(db_err)
    }

    /// Forget job `id`'s artifact once its object is gone. `false` when the
    /// job no longer records `key`.
    pub async fn clear_artifact(
        db: &impl ConnectionTrait,
        id: Uuid,
        key: &str,
    ) -> Result<bool, OxidGeneError> {
        let result = Entity::update_many()
            .col_expr(Column::ArtifactKey, Expr::value(Option::<String>::None))
            .col_expr(Column::UpdatedAt, Expr::value(Utc::now()))
            .filter(Column::Id.eq(id))
            .filter(Column::ArtifactKey.eq(key))
            .exec(db)
            .await
            .map_err(db_err)?;
        Ok(result.rows_affected == 1)
    }

    /// The jobs that ended before `cutoff`.
    pub async fn ended_before(
        db: &impl ConnectionTrait,
        cutoff: DateTimeUtc,
    ) -> Result<Vec<Uuid>, OxidGeneError> {
        Entity::find()
            .select_only()
            .column(Column::Id)
            .filter(Column::Status.is_in(TERMINAL.map(BackgroundJobStatus::as_str)))
            .filter(Column::FinishedAt.lt(cutoff))
            .into_tuple()
            .all(db)
            .await
            .map_err(db_err)
    }

    /// Delete the ended jobs among `ids`; a job that has not ended is kept.
    pub async fn delete_ended(
        db: &impl ConnectionTrait,
        ids: &[Uuid],
    ) -> Result<u64, OxidGeneError> {
        let mut deleted = 0;
        for chunk in ids.chunks(MAX_BOUND_IDS) {
            deleted += Entity::delete_many()
                .filter(Column::Id.is_in(chunk.iter().copied()))
                .filter(Column::Status.is_in(TERMINAL.map(BackgroundJobStatus::as_str)))
                .exec(db)
                .await
                .map_err(db_err)?
                .rows_affected;
        }
        Ok(deleted)
    }

    /// Of `ids`, the jobs whose stored objects are still needed: those not
    /// ended yet, which will read their inputs, and those holding an
    /// artifact still to be downloaded.
    pub async fn holding_objects(
        db: &impl ConnectionTrait,
        ids: &[Uuid],
    ) -> Result<HashSet<Uuid>, OxidGeneError> {
        let held = in_chunks(ids, |chunk| async move {
            Entity::find()
                .select_only()
                .column(Column::Id)
                .filter(Column::Id.is_in(chunk))
                .filter(
                    Condition::any()
                        .add(Column::Status.is_not_in(TERMINAL.map(BackgroundJobStatus::as_str)))
                        .add(Column::ArtifactKey.is_not_null()),
                )
                .into_tuple::<Uuid>()
                .all(db)
                .await
                .map_err(db_err)
        })
        .await?;
        Ok(held.into_iter().collect())
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
