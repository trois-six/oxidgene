//! `access_event` table entity: the access log (docs/data-model.md §6.7).
//! No address, email, name or token; no foreign key, since an event
//! outlives what it names until its retention ends.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "access_event")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub occurred_at: DateTimeUtc,
    /// What happened: `request_refused`, `login_succeeded`…
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
    /// The outcome code: `unauthenticated`, `forbidden`, `not_found`, a
    /// cause…
    pub outcome: String,
    pub request_id: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
