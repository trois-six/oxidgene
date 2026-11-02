//! Repository for the accounts of the web build (`app_user`,
//! docs/data-model.md §6.1).
//!
//! A user is keyed by the identity provider's issuer and subject, never by
//! email. Erasure keeps the row as a tombstone — identity, email, names,
//! locale and avatar cleared, status `erased` — so that what points at it
//! keeps pointing at one anonymous author.

use chrono::{DateTime, Utc};
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::types::Connection;
use sea_orm::entity::prelude::*;
use sea_orm::{
    ActiveModelTrait, ConnectionTrait, IntoActiveModel, QueryFilter, QueryOrder, QuerySelect, Set,
};
use uuid::Uuid;

use crate::entities::app_user::{self, ActiveModel, Column, Entity};
use crate::repo::db_err;
use crate::repo::pagination::{PaginationParams, paginate};

pub use oxidgene_core::access::{InstanceRole, UserStatus};

/// An account of the web build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppUser {
    pub id: Uuid,
    pub issuer: Option<String>,
    pub subject: Option<String>,
    pub email: Option<String>,
    pub email_verified: bool,
    pub provider_name: Option<String>,
    pub username: Option<String>,
    pub display_name: Option<String>,
    pub locale: Option<String>,
    pub avatar_sha256: Option<String>,
    pub avatar_updated_at: Option<DateTime<Utc>>,
    pub instance_role: InstanceRole,
    pub can_create_trees: bool,
    pub status: UserStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_sign_in_at: DateTime<Utc>,
    pub deactivated_at: Option<DateTime<Utc>>,
    pub erased_at: Option<DateTime<Utc>>,
}

impl AppUser {
    /// The name shown for the user: their own, else the provider's.
    pub fn shown_name(&self) -> Option<&str> {
        self.display_name
            .as_deref()
            .or(self.provider_name.as_deref())
    }
}

/// A user created at their first sign-in.
#[derive(Debug, Clone)]
pub struct NewAppUser {
    pub issuer: String,
    pub subject: String,
    pub email: Option<String>,
    pub email_verified: bool,
    pub provider_name: Option<String>,
    pub username: Option<String>,
    pub locale: Option<String>,
    pub instance_role: InstanceRole,
    pub can_create_trees: bool,
}

/// What [`AppUserRepo::update`] changes; `None` keeps a field, and a
/// doubled `Option` clears one with `Some(None)`.
#[derive(Debug, Clone, Default)]
pub struct AppUserChanges {
    pub issuer: Option<String>,
    pub subject: Option<String>,
    pub email: Option<Option<String>>,
    pub email_verified: Option<bool>,
    pub provider_name: Option<Option<String>>,
    pub username: Option<Option<String>>,
    pub display_name: Option<Option<String>>,
    pub locale: Option<Option<String>>,
    pub avatar_sha256: Option<Option<String>>,
    pub instance_role: Option<InstanceRole>,
    pub can_create_trees: Option<bool>,
    pub status: Option<UserStatus>,
    pub last_sign_in_at: Option<DateTime<Utc>>,
}

/// Set `field` to `value`, unless there is none.
fn set<T: Into<sea_orm::Value>>(field: &mut sea_orm::ActiveValue<T>, value: Option<T>) {
    if let Some(value) = value {
        *field = Set(value);
    }
}

impl AppUserChanges {
    fn apply(self, active: &mut ActiveModel, now: DateTime<Utc>) {
        set(&mut active.issuer, self.issuer.map(Some));
        set(&mut active.subject, self.subject.map(Some));
        set(&mut active.email, self.email);
        set(&mut active.email_verified, self.email_verified);
        set(&mut active.provider_name, self.provider_name);
        set(&mut active.username, self.username);
        set(&mut active.display_name, self.display_name);
        set(&mut active.locale, self.locale);
        if let Some(avatar) = self.avatar_sha256 {
            active.avatar_updated_at = Set(avatar.as_ref().map(|_| now));
            active.avatar_sha256 = Set(avatar);
        }
        set(
            &mut active.instance_role,
            self.instance_role.map(|role| role.as_str().to_string()),
        );
        set(&mut active.can_create_trees, self.can_create_trees);
        if let Some(status) = self.status {
            active.deactivated_at = Set((status == UserStatus::Deactivated).then_some(now));
            active.status = Set(status.as_str().to_string());
        }
        set(&mut active.last_sign_in_at, self.last_sign_in_at);
    }
}

/// Repository for accounts.
pub struct AppUserRepo;

impl AppUserRepo {
    /// The user `id`, whatever its status.
    pub async fn get(db: &impl ConnectionTrait, id: Uuid) -> Result<AppUser, OxidGeneError> {
        find(db, id).await.map(into_domain)
    }

    /// The user `id`, its row locked until transaction `txn` ends on
    /// PostgreSQL, so that two changes of the same user take turns; SQLite's
    /// single writer does the same there.
    pub async fn get_for_update(
        txn: &impl ConnectionTrait,
        id: Uuid,
    ) -> Result<AppUser, OxidGeneError> {
        Entity::find_by_id(id)
            .lock_exclusive()
            .one(txn)
            .await
            .map_err(db_err)?
            .map(into_domain)
            .ok_or(OxidGeneError::NotFound { entity: "User", id })
    }

    /// The user an identity names, if one was provisioned.
    pub async fn find_by_identity(
        db: &impl ConnectionTrait,
        issuer: &str,
        subject: &str,
    ) -> Result<Option<AppUser>, OxidGeneError> {
        Ok(Entity::find()
            .filter(Column::Issuer.eq(issuer))
            .filter(Column::Subject.eq(subject))
            .one(db)
            .await
            .map_err(db_err)?
            .map(into_domain))
    }

    /// The active user whose verified address is `email`, compared without
    /// regard to case; the first created when several accounts share it
    /// (docs/authorization.md §6.1).
    pub async fn find_by_verified_email(
        db: &impl ConnectionTrait,
        email: &str,
    ) -> Result<Option<AppUser>, OxidGeneError> {
        Ok(Entity::find()
            .filter(crate::repo::lower_trim_eq(Column::Email, email))
            .filter(Column::EmailVerified.eq(true))
            .filter(Column::Status.eq(UserStatus::Active.as_str()))
            .order_by_asc(Column::Id)
            .one(db)
            .await
            .map_err(db_err)?
            .map(into_domain))
    }

    /// The active user whose username is exactly `username`; the first
    /// created when several providers gave the same one.
    pub async fn find_by_username(
        db: &impl ConnectionTrait,
        username: &str,
    ) -> Result<Option<AppUser>, OxidGeneError> {
        Ok(Entity::find()
            .filter(Column::Username.eq(username))
            .filter(Column::Status.eq(UserStatus::Active.as_str()))
            .order_by_asc(Column::Id)
            .one(db)
            .await
            .map_err(db_err)?
            .map(into_domain))
    }

    /// Every user, tombstones included, by cursor in creation order.
    pub async fn list(
        db: &impl ConnectionTrait,
        params: &PaginationParams,
    ) -> Result<Connection<AppUser>, OxidGeneError> {
        paginate(db, Entity::find(), Column::Id, params, |model| {
            (model.id, into_domain(model))
        })
        .await
    }

    /// The names of `ids` as their entries' readers see them: the user's
    /// own, else the provider's; none for an erased user, whom readers see
    /// as a former member (docs/authentication.md §6.6). Users not found
    /// are left out.
    pub async fn shown_names(
        db: &impl ConnectionTrait,
        ids: &[Uuid],
    ) -> Result<std::collections::HashMap<Uuid, Option<String>>, OxidGeneError> {
        if ids.is_empty() {
            return Ok(std::collections::HashMap::new());
        }
        Ok(Entity::find()
            .filter(Column::Id.is_in(ids.iter().copied()))
            .all(db)
            .await
            .map_err(db_err)?
            .into_iter()
            .map(into_domain)
            .map(|user| {
                let name = (user.status != UserStatus::Erased)
                    .then(|| user.shown_name().map(str::to_owned))
                    .flatten();
                (user.id, name)
            })
            .collect())
    }

    /// Whether an active instance administrator exists.
    pub async fn exists_admin(db: &impl ConnectionTrait) -> Result<bool, OxidGeneError> {
        Ok(Entity::find()
            .filter(Column::InstanceRole.eq(InstanceRole::Admin.as_str()))
            .filter(Column::Status.eq(UserStatus::Active.as_str()))
            .one(db)
            .await
            .map_err(db_err)?
            .is_some())
    }

    /// Create a user at their first sign-in.
    pub async fn create(
        db: &impl ConnectionTrait,
        user: NewAppUser,
    ) -> Result<AppUser, OxidGeneError> {
        let now = Utc::now();
        let model = app_user::ActiveModel {
            id: Set(Uuid::now_v7()),
            issuer: Set(Some(user.issuer)),
            subject: Set(Some(user.subject)),
            email: Set(user.email),
            email_verified: Set(user.email_verified),
            provider_name: Set(user.provider_name),
            username: Set(user.username),
            display_name: Set(None),
            locale: Set(user.locale),
            avatar_sha256: Set(None),
            avatar_updated_at: Set(None),
            instance_role: Set(user.instance_role.as_str().to_string()),
            can_create_trees: Set(user.can_create_trees),
            status: Set(UserStatus::Active.as_str().to_string()),
            created_at: Set(now),
            updated_at: Set(now),
            last_sign_in_at: Set(now),
            deactivated_at: Set(None),
            erased_at: Set(None),
        };
        model.insert(db).await.map(into_domain).map_err(db_err)
    }

    /// Change the fields `changes` names.
    pub async fn update(
        db: &impl ConnectionTrait,
        id: Uuid,
        changes: AppUserChanges,
    ) -> Result<AppUser, OxidGeneError> {
        let now = Utc::now();
        let mut active = find(db, id).await?.into_active_model();
        changes.apply(&mut active, now);
        active.updated_at = Set(now);
        active.update(db).await.map(into_domain).map_err(db_err)
    }

    /// Turn the user into a tombstone: everything that identifies them is
    /// cleared and the status becomes `erased`.
    pub async fn erase(db: &impl ConnectionTrait, id: Uuid) -> Result<AppUser, OxidGeneError> {
        let now = Utc::now();
        let mut active = find(db, id).await?.into_active_model();
        active.issuer = Set(None);
        active.subject = Set(None);
        active.email = Set(None);
        active.email_verified = Set(false);
        active.provider_name = Set(None);
        active.username = Set(None);
        active.display_name = Set(None);
        active.locale = Set(None);
        active.avatar_sha256 = Set(None);
        active.avatar_updated_at = Set(None);
        active.instance_role = Set(InstanceRole::User.as_str().to_string());
        active.can_create_trees = Set(false);
        active.status = Set(UserStatus::Erased.as_str().to_string());
        active.updated_at = Set(now);
        active.erased_at = Set(Some(now));
        active.update(db).await.map(into_domain).map_err(db_err)
    }
}

async fn find(db: &impl ConnectionTrait, id: Uuid) -> Result<app_user::Model, OxidGeneError> {
    Entity::find_by_id(id)
        .one(db)
        .await
        .map_err(db_err)?
        .ok_or(OxidGeneError::NotFound { entity: "User", id })
}

fn into_domain(model: app_user::Model) -> AppUser {
    AppUser {
        id: model.id,
        issuer: model.issuer,
        subject: model.subject,
        email: model.email,
        email_verified: model.email_verified,
        provider_name: model.provider_name,
        username: model.username,
        display_name: model.display_name,
        locale: model.locale,
        avatar_sha256: model.avatar_sha256,
        avatar_updated_at: model.avatar_updated_at,
        instance_role: InstanceRole::parse(&model.instance_role),
        can_create_trees: model.can_create_trees,
        status: UserStatus::parse(&model.status),
        created_at: model.created_at,
        updated_at: model.updated_at,
        last_sign_in_at: model.last_sign_in_at,
        deactivated_at: model.deactivated_at,
        erased_at: model.erased_at,
    }
}

#[cfg(test)]
mod tests {
    use super::UserStatus;

    #[test]
    fn an_unknown_status_is_deactivated() {
        for status in [
            UserStatus::Active,
            UserStatus::Deactivated,
            UserStatus::Erased,
        ] {
            assert_eq!(UserStatus::parse(status.as_str()), status);
        }
        for unknown in ["", "Active", "enabled", "suspended"] {
            assert_eq!(UserStatus::parse(unknown), UserStatus::Deactivated);
        }
    }
}
