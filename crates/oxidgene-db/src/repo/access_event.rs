//! Repository for the access log (`access_event`, docs/data-model.md §6.7):
//! what the history does not record — refused requests, sign-ins and
//! sign-outs, administrator actions — with the principal's ids, never an
//! email, a name, a token, a search term or an address.

use chrono::{DateTime, Utc};
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::types::{Connection, Edge, PageInfo};
use sea_orm::entity::prelude::*;
use sea_orm::{ConnectionTrait, Order, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, Set};
use serde::Serialize;
use uuid::Uuid;

use crate::entities::access_event::{self, ActiveModel, Column, Entity};
use crate::repo::db_err;
use crate::repo::pagination::{PaginationParams, encode_cursor};

/// One event of the access log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AccessEvent {
    pub id: Uuid,
    pub occurred_at: DateTime<Utc>,
    /// What happened: `request_refused`, `login_succeeded`, `membership_expired`…
    pub kind: String,
    /// `user`, `api_client`, `link`, `anonymous` or `local`.
    pub principal_kind: String,
    pub user_id: Option<Uuid>,
    pub api_client_id: Option<Uuid>,
    pub share_link_id: Option<Uuid>,
    pub client_id: Option<String>,
    pub tree_id: Option<Uuid>,
    /// The route template, or the GraphQL root fields.
    pub route: Option<String>,
    /// The permission refused.
    pub permission: Option<String>,
    /// The outcome code.
    pub outcome: String,
    pub request_id: Option<String>,
}

/// An event to record; its id and time are given when it is stored.
#[derive(Debug, Clone, Default)]
pub struct NewAccessEvent {
    pub kind: &'static str,
    pub principal_kind: &'static str,
    pub user_id: Option<Uuid>,
    pub api_client_id: Option<Uuid>,
    /// The share link the viewer's access came from, or the one redeemed.
    pub share_link_id: Option<Uuid>,
    pub client_id: Option<String>,
    pub tree_id: Option<Uuid>,
    pub route: Option<String>,
    pub permission: Option<&'static str>,
    pub outcome: String,
    pub request_id: Option<String>,
}

/// Repository for the access log.
pub struct AccessEventRepo;

impl AccessEventRepo {
    pub async fn insert(
        db: &impl ConnectionTrait,
        event: NewAccessEvent,
    ) -> Result<AccessEvent, OxidGeneError> {
        ActiveModel {
            id: Set(Uuid::now_v7()),
            occurred_at: Set(Utc::now()),
            kind: Set(event.kind.to_string()),
            principal_kind: Set(event.principal_kind.to_string()),
            user_id: Set(event.user_id),
            api_client_id: Set(event.api_client_id),
            share_link_id: Set(event.share_link_id),
            client_id: Set(event.client_id),
            tree_id: Set(event.tree_id),
            route: Set(event.route),
            permission: Set(event.permission.map(str::to_string)),
            outcome: Set(event.outcome),
            request_id: Set(event.request_id),
        }
        .insert(db)
        .await
        .map(into_domain)
        .map_err(db_err)
    }

    /// The events of tree `tree_id`, or of the whole instance, newest first.
    pub async fn list(
        db: &impl ConnectionTrait,
        tree_id: Option<Uuid>,
        params: &PaginationParams,
    ) -> Result<Connection<AccessEvent>, OxidGeneError> {
        let limit = params.clamped_first();
        let mut query = Entity::find();
        if let Some(tree_id) = tree_id {
            query = query.filter(Column::TreeId.eq(tree_id));
        }
        let total_count = PaginatorTrait::count(query.clone(), db)
            .await
            .map_err(db_err)?;
        if let Some(after) = params.decode_cursor()? {
            query = query.filter(Column::Id.lt(after));
        }
        let mut rows = query
            .order_by(Column::Id, Order::Desc)
            .limit(limit + 1)
            .all(db)
            .await
            .map_err(db_err)?;
        let has_next_page = rows.len() as u64 > limit;
        rows.truncate(limit as usize);
        let edges: Vec<Edge<AccessEvent>> = rows
            .into_iter()
            .map(|row| Edge {
                cursor: encode_cursor(&row.id),
                node: into_domain(row),
            })
            .collect();
        let end_cursor = edges.last().map(|edge| edge.cursor.clone());
        Ok(Connection {
            edges,
            page_info: PageInfo {
                has_next_page,
                end_cursor,
            },
            total_count: total_count as i64,
        })
    }

    /// Every event of user `user_id`, oldest first: their account data.
    pub async fn of_user(
        db: &impl ConnectionTrait,
        user_id: Uuid,
    ) -> Result<Vec<AccessEvent>, OxidGeneError> {
        Ok(Entity::find()
            .filter(Column::UserId.eq(user_id))
            .order_by(Column::Id, Order::Asc)
            .all(db)
            .await
            .map_err(db_err)?
            .into_iter()
            .map(into_domain)
            .collect())
    }

    /// Delete the events that occurred before `cutoff`: the retention
    /// sweep. Returns how many went.
    pub async fn delete_before(
        db: &impl ConnectionTrait,
        cutoff: DateTime<Utc>,
    ) -> Result<u64, OxidGeneError> {
        Ok(Entity::delete_many()
            .filter(Column::OccurredAt.lt(cutoff))
            .exec(db)
            .await
            .map_err(db_err)?
            .rows_affected)
    }
}

fn into_domain(model: access_event::Model) -> AccessEvent {
    AccessEvent {
        id: model.id,
        occurred_at: model.occurred_at,
        kind: model.kind,
        principal_kind: model.principal_kind,
        user_id: model.user_id,
        api_client_id: model.api_client_id,
        share_link_id: model.share_link_id,
        client_id: model.client_id,
        tree_id: model.tree_id,
        route: model.route,
        permission: model.permission,
        outcome: model.outcome,
        request_id: model.request_id,
    }
}
