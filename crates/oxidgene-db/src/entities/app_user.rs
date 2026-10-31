//! `app_user` table entity: an account of the web build (docs/data-model.md
//! §6.1). The table exists on every backend and stays empty on the desktop.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "app_user")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    /// The identity provider's issuer; cleared on erasure.
    pub issuer: Option<String>,
    /// The subject the issuer knows the user by; cleared on erasure.
    pub subject: Option<String>,
    pub email: Option<String>,
    pub email_verified: bool,
    /// The provider's `name`, else its `preferred_username`.
    pub provider_name: Option<String>,
    /// The provider's `preferred_username`.
    pub username: Option<String>,
    /// The name the user set on their profile.
    pub display_name: Option<String>,
    pub locale: Option<String>,
    /// The uploaded avatar's SHA-256, naming its files in the media store.
    pub avatar_sha256: Option<String>,
    pub avatar_updated_at: Option<DateTimeUtc>,
    /// `admin` or `user`.
    pub instance_role: String,
    pub can_create_trees: bool,
    /// `active`, `deactivated` or `erased`.
    pub status: String,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
    pub last_sign_in_at: DateTimeUtc,
    pub deactivated_at: Option<DateTimeUtc>,
    pub erased_at: Option<DateTimeUtc>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
