//! The repositories of sharing (docs/authorization.md §6): invitations,
//! memberships and the mail outbox, and the state transitions they make
//! atomic.

use chrono::{DateTime, Duration, DurationRound, Utc};
use oxidgene_core::OxidGeneError;
use oxidgene_core::access::{InstanceRole, TreeRole};
use oxidgene_db::repo::{
    AppUserRepo, InvitationEnd, InvitationRepo, NewAppUser, NewInvitation, TreeRepo,
};
use sea_orm::DatabaseConnection;
use uuid::Uuid;

use crate::common::setup_db;

async fn tree(db: &DatabaseConnection) -> Uuid {
    let id = Uuid::now_v7();
    TreeRepo::create(db, id, "Fictional tree".into(), None)
        .await
        .unwrap();
    id
}

async fn user(db: &DatabaseConnection, subject: &str) -> Uuid {
    AppUserRepo::create(
        db,
        NewAppUser {
            issuer: "https://idp.example.test".into(),
            subject: subject.into(),
            email: Some(format!("{subject}@example.test")),
            email_verified: true,
            provider_name: None,
            username: None,
            locale: None,
            instance_role: InstanceRole::User,
            can_create_trees: true,
        },
    )
    .await
    .unwrap()
    .id
}

async fn invitation(db: &DatabaseConnection, tree_id: Uuid, owner: Uuid) -> Uuid {
    InvitationRepo::create(
        db,
        NewInvitation {
            tree_id,
            email: "invitee_1@example.test".into(),
            role: TreeRole::Editor,
            scope_id: None,
            membership_expires_at: None,
            expires_at: Utc::now() + Duration::days(14),
            created_by: Some(owner),
        },
    )
    .await
    .unwrap()
    .id
}

/// Now, to the second: what both backends store exactly.
fn now() -> DateTime<Utc> {
    Utc::now().duration_trunc(Duration::seconds(1)).unwrap()
}

fn is_not_found(result: Result<impl std::fmt::Debug, OxidGeneError>) -> bool {
    matches!(result, Err(OxidGeneError::NotFound { .. }))
}

/// A revocation and an acceptance that both read the invitation pending:
/// whichever writes second finds it ended, so a revoked invitation is never
/// accepted as well (and the acceptance's membership rolls back with it).
#[tokio::test]
async fn an_invitation_ends_only_once() {
    let db = setup_db().await;
    let tree_id = tree(&db).await;
    let owner = user(&db, "owner_1").await;
    let invitee = user(&db, "invitee_1").await;
    let id = invitation(&db, tree_id, owner).await;
    let now = now();

    let revoked = InvitationRepo::end(&db, id, InvitationEnd::Revoked, now)
        .await
        .unwrap();
    assert_eq!(revoked.revoked_at, Some(now));

    assert!(is_not_found(
        InvitationRepo::end(&db, id, InvitationEnd::Accepted(invitee), now).await
    ));
    assert!(is_not_found(
        InvitationRepo::end(&db, id, InvitationEnd::Declined, now).await
    ));
    assert!(is_not_found(
        InvitationRepo::resent(&db, id, now, now + Duration::days(14)).await
    ));
    let stored = InvitationRepo::get(&db, id).await.unwrap().unwrap();
    assert_eq!(stored.accepted_at, None);
    assert_eq!(stored.accepted_by, None);
    assert_eq!(stored.declined_at, None);
    assert_eq!(stored.revoked_at, Some(now));
}

/// An expired invitation can no longer be answered nor sent again.
#[tokio::test]
async fn an_expired_invitation_ends_no_more() {
    let db = setup_db().await;
    let tree_id = tree(&db).await;
    let owner = user(&db, "owner_1").await;
    let invitee = user(&db, "invitee_1").await;
    let id = invitation(&db, tree_id, owner).await;
    let later = now() + Duration::days(15);

    assert!(is_not_found(
        InvitationRepo::end(&db, id, InvitationEnd::Accepted(invitee), later).await
    ));
    assert!(is_not_found(
        InvitationRepo::resent(&db, id, later, later + Duration::days(14)).await
    ));

    let now = now();
    let resent = InvitationRepo::resent(&db, id, now, now + Duration::days(20))
        .await
        .unwrap();
    assert_eq!(resent.last_sent_at, Some(now));
    assert_eq!(resent.expires_at, now + Duration::days(20));
}
