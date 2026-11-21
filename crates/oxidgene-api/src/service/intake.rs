//! How many files the process takes in at once, and how many of them one
//! viewer.
//!
//! Every way a file's bytes enter the backend takes one of [`INTAKE_SLOTS`]
//! slots first and holds it until the bytes are stored: an import job's
//! upload, a media upload on either surface, a saved Geneanet session being
//! read or decoded, and a Geneanet import copying its inputs into job
//! storage. Each of those can hold a spool file, a decoded photograph or a
//! blocking thread; past the slots, callers wait their turn instead of
//! adding theirs. A caller still waiting when its client leaves is dropped
//! with its request, and the server's time limits bound how long one waits
//! (see [`crate::limits`]).
//!
//! A signed-in user holds one slot at a time: their second intake waits for
//! their first, not for a slot, so that no account can keep every slot —
//! and with them every other user's uploads — to itself. Visitors without
//! an account share one. The local owner is the machine's only user and is
//! bound by the slots alone.
//!
//! A slot is taken once the request has started sending its file
//! ([`first_chunk`]): a client that opens a request and sends nothing waits
//! on its own connection, never on a slot.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::body::{Body, BodyDataStream, Bytes};
use futures_util::StreamExt as _;
use oxidgene_core::OxidGeneError;
use oxidgene_core::access::{Principal, ViewerContext};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, SemaphorePermit};
use uuid::Uuid;

/// How many intakes run at once, process-wide.
pub(crate) const INTAKE_SLOTS: usize = 4;

static INTAKE: Semaphore = Semaphore::const_new(INTAKE_SLOTS);

/// The turn of each viewer with an intake running or waiting: one permit
/// each. An entry goes with the last intake of its viewer, so the map
/// holds only the viewers taking something in right now.
static TURNS: Mutex<Option<HashMap<Uuid, Arc<Semaphore>>>> = Mutex::new(None);

/// An intake slot, given back when dropped.
#[derive(Debug)]
pub(crate) struct Slot {
    _slot: SemaphorePermit<'static>,
    /// The viewer's turn, released after the slot.
    _turn: Option<Turn>,
}

/// A viewer's turn: their one intake at a time.
#[derive(Debug)]
struct Turn {
    owner: Uuid,
    /// Dropped before the entry is looked at, so that a viewer with nothing
    /// else running or waiting leaves the map.
    permit: Option<OwnedSemaphorePermit>,
    turn: Arc<Semaphore>,
}

impl Drop for Turn {
    fn drop(&mut self) {
        self.permit = None;
        let mut turns = TURNS.lock().unwrap_or_else(|poison| poison.into_inner());
        // The map's handle and this one: nobody else runs or waits.
        if Arc::strong_count(&self.turn) <= 2
            && let Some(turns) = turns.as_mut()
        {
            turns.remove(&self.owner);
        }
    }
}

/// Whose turn an intake of `viewer` takes: a user's own, the visitors'
/// common one, none for the local owner.
fn owner(viewer: &ViewerContext) -> Option<Uuid> {
    match viewer.principal() {
        Principal::Local => None,
        Principal::User(identity) => Some(identity.user_id),
        Principal::Anonymous => Some(Uuid::nil()),
    }
}

async fn turn(owner: Uuid) -> Result<Turn, OxidGeneError> {
    let turn = Arc::clone(
        TURNS
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .get_or_insert_default()
            .entry(owner)
            .or_insert_with(|| Arc::new(Semaphore::new(1))),
    );
    // Held from here: a wait that is dropped still leaves the map clean.
    let mut waiting = Turn {
        owner,
        permit: None,
        turn: Arc::clone(&turn),
    };
    waiting.permit = Some(turn.acquire_owned().await.map_err(|_| unavailable())?);
    Ok(waiting)
}

fn unavailable() -> OxidGeneError {
    OxidGeneError::Internal("file intake is unavailable".into())
}

/// Wait for `viewer`'s turn, then for an intake slot; both are given back
/// when the slot drops.
pub(crate) async fn slot(viewer: &ViewerContext) -> Result<Slot, OxidGeneError> {
    let turn = match owner(viewer) {
        Some(owner) => Some(turn(owner).await?),
        None => None,
    };
    let slot = INTAKE.acquire().await.map_err(|_| unavailable())?;
    Ok(Slot {
        _slot: slot,
        _turn: turn,
    })
}

/// The stream of an uploaded `body` once its first bytes arrived, with
/// them: what a handler waits for before it takes a slot.
///
/// # Errors
///
/// `Validation` for a body that holds nothing or cannot be read.
pub(crate) async fn first_chunk(body: Body) -> Result<(Bytes, BodyDataStream), OxidGeneError> {
    let mut chunks = body.into_data_stream();
    loop {
        match chunks.next().await {
            Some(Ok(chunk)) if chunk.is_empty() => {}
            Some(Ok(chunk)) => return Ok((chunk, chunks)),
            Some(Err(_)) => {
                return Err(OxidGeneError::Validation(
                    "the upload was interrupted".into(),
                ));
            }
            None => return Err(OxidGeneError::Validation("uploaded file is empty".into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use oxidgene_core::access::{Identity, InstanceRole};

    use super::*;

    fn user() -> ViewerContext {
        ViewerContext::new(Principal::User(Identity {
            issuer: "https://id.example.invalid".into(),
            subject: "subject".into(),
            user_id: Uuid::now_v7(),
            client_id: None,
            scopes: Vec::new(),
            session_id: None,
            api_client_id: None,
            instance_role: InstanceRole::User,
            can_create_trees: true,
        }))
    }

    async fn waits(viewer: &ViewerContext) -> bool {
        tokio::time::timeout(Duration::from_millis(50), slot(viewer))
            .await
            .is_err()
    }

    /// The slots are the process's: the tests that fill them run one at a
    /// time.
    static SLOTS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    #[tokio::test]
    async fn intakes_past_the_slots_wait_for_one_to_end() {
        let _alone = SLOTS.lock().await;
        let local = ViewerContext::local();
        let mut held = Vec::new();
        for _ in 0..INTAKE_SLOTS {
            held.push(slot(&local).await.expect("a free slot"));
        }
        assert!(waits(&local).await, "an intake past the slots did not wait");
        assert!(waits(&user()).await, "nor did another viewer's");

        held.pop();
        let next = tokio::time::timeout(Duration::from_secs(5), slot(&local))
            .await
            .expect("a slot freed for the waiting intake");
        assert!(next.is_ok());
    }

    #[tokio::test]
    async fn a_user_takes_in_one_file_at_a_time_and_leaves_the_others_their_slots() {
        let _alone = SLOTS.lock().await;
        let (greedy, other) = (user(), user());
        let first = slot(&greedy).await.expect("their turn");
        assert!(waits(&greedy).await, "a second intake of the same user ran");
        // The slots the user could not take are everybody else's.
        let mut others = Vec::new();
        for _ in 1..INTAKE_SLOTS {
            others.push(slot(&user()).await.expect("a free slot"));
        }
        drop(others);
        let theirs = slot(&other).await.expect("another user's turn");

        drop(first);
        let second = tokio::time::timeout(Duration::from_secs(5), slot(&greedy))
            .await
            .expect("their turn came back");
        assert!(second.is_ok());
        drop((second, theirs));
        let turns = TURNS.lock().unwrap();
        let kept = |viewer: &ViewerContext| {
            let owner = owner(viewer).expect("a user");
            turns
                .as_ref()
                .is_some_and(|turns| turns.contains_key(&owner))
        };
        assert!(
            !kept(&greedy) && !kept(&other),
            "a viewer with no intake left stays in the map"
        );
    }

    #[tokio::test]
    async fn visitors_share_one_turn() {
        let _alone = SLOTS.lock().await;
        let visitor = ViewerContext::anonymous();
        let held = slot(&visitor).await.expect("the visitors' turn");
        assert!(waits(&ViewerContext::anonymous()).await);
        drop(held);
        assert!(!waits(&ViewerContext::anonymous()).await);
    }

    #[tokio::test]
    async fn an_upload_is_known_by_its_first_bytes() {
        let (first, _) = first_chunk(Body::from("gedcom")).await.expect("bytes");
        assert_eq!(&first[..], b"gedcom");
        assert!(matches!(
            first_chunk(Body::empty()).await,
            Err(OxidGeneError::Validation(_))
        ));
    }
}
