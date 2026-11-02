//! The repositories of sharing (docs/authorization.md §6): invitations,
//! memberships and the mail outbox, and the state transitions they make
//! atomic.

use chrono::{DateTime, Duration, DurationRound, Utc};
use oxidgene_core::OxidGeneError;
use oxidgene_core::access::{InstanceRole, TreeRole};
use oxidgene_db::repo::{
    AppUserRepo, InvitationEnd, InvitationRepo, MailOutboxRepo, MailStatus, MembershipRepo,
    NewAppUser, NewInvitation, NewMail, TreeRepo,
};
use sea_orm::{ConnectionTrait, DatabaseConnection};
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

/// A worker whose lease ran out while it was sending, and whose mail
/// another worker claimed and sent, records nothing over the later claim.
#[tokio::test]
async fn a_lapsed_claim_records_nothing() {
    let db = setup_db().await;
    let now = now();
    let id = MailOutboxRepo::enqueue(
        &db,
        NewMail {
            user_id: None,
            to_address: "recipient_1@example.test".into(),
            kind: "invitation",
            language: "en".into(),
            params: serde_json::json!({}),
            due: now,
        },
    )
    .await
    .unwrap();
    let lease = Duration::minutes(5);
    let first = MailOutboxRepo::claim_next(&db, now, lease)
        .await
        .unwrap()
        .unwrap();
    let later = now + Duration::minutes(6);
    let second = MailOutboxRepo::claim_next(&db, later, lease)
        .await
        .unwrap()
        .unwrap();
    assert_eq!((first.id, second.id), (id, id));
    assert_eq!((first.attempts, second.attempts), (1, 2));

    MailOutboxRepo::mark_sent(&db, &second, later)
        .await
        .unwrap();
    MailOutboxRepo::mark_failed(&db, &first, later, "timeout")
        .await
        .unwrap();
    MailOutboxRepo::mark_dropped(&db, &first, later, "revoked")
        .await
        .unwrap();
    MailOutboxRepo::retry_at(&db, &first, later + lease, "timeout")
        .await
        .unwrap();

    let stored = MailOutboxRepo::get(&db, id).await.unwrap().unwrap();
    assert_eq!(stored.status, MailStatus::Sent.as_str());
    assert_eq!(stored.sent_at, Some(later));
    assert_eq!(stored.error_category, None);
}

/// The columns access is decided on hold only the values the code writes:
/// a value a reader would not know is refused at the write.
#[tokio::test]
async fn access_columns_refuse_unknown_values() {
    let db = setup_db().await;
    let tree_id = tree(&db).await;
    let member = user(&db, "member_1").await;
    MembershipRepo::create(&db, tree_id, member, TreeRole::Editor, None)
        .await
        .unwrap();
    for sql in [
        "UPDATE app_user SET status = 'enabled'",
        "UPDATE app_user SET instance_role = 'root'",
        "UPDATE tree SET visibility = 'open'",
        "UPDATE tree_membership SET role = 'admin'",
    ] {
        assert!(db.execute_unprepared(sql).await.is_err(), "{sql}");
    }
    for sql in [
        "UPDATE app_user SET status = 'deactivated'",
        "UPDATE tree SET visibility = 'site'",
        "UPDATE tree_membership SET role = 'guest'",
    ] {
        assert_eq!(
            db.execute_unprepared(sql).await.unwrap().rows_affected(),
            1,
            "{sql}"
        );
    }
}

/// A deleted tree grants none of the roles its memberships held, as
/// `live_access` already answers.
#[tokio::test]
async fn a_deleted_tree_grants_no_role() {
    let db = setup_db().await;
    let kept = tree(&db).await;
    let deleted = tree(&db).await;
    let member = user(&db, "member_1").await;
    for tree_id in [kept, deleted] {
        MembershipRepo::create(&db, tree_id, member, TreeRole::Guest, None)
            .await
            .unwrap();
    }
    TreeRepo::soft_delete(&db, deleted).await.unwrap();

    let roles = MembershipRepo::live_roles(&db, member, &[kept, deleted], Utc::now())
        .await
        .unwrap();
    assert_eq!(roles.len(), 1);
    assert_eq!(roles.get(&kept), Some(&(TreeRole::Guest, None)));
}
