//! Note writes: the same steps whether REST or GraphQL asked.
//!
//! Each write checks that every record it names belongs to the tree, rewrites
//! the projection of the person the note is about, and records the change, in
//! one transaction.

use oxidgene_core::OxidGeneError;
use oxidgene_core::history::AuditAction;
use oxidgene_core::types::Note;
use oxidgene_db::repo::NoteRepo;
use oxidgene_db::sea_orm::DatabaseConnection;
use uuid::Uuid;

use crate::profile::ProfileService;
use crate::service::history;
use crate::service::scope::{TreeResource, begin_tx, commit_tx, require_tree_resource};

/// A note to create: its text and what it is about.
pub struct NewNote {
    pub text: String,
    pub person_id: Option<Uuid>,
    pub event_id: Option<Uuid>,
    pub family_id: Option<Uuid>,
    pub source_id: Option<Uuid>,
    pub media_id: Option<Uuid>,
}

/// Create a note in `tree_id`. A note with no text is refused.
pub async fn create_note(
    db: &DatabaseConnection,
    profiles: &ProfileService,
    tree_id: Uuid,
    new: NewNote,
) -> Result<Note, OxidGeneError> {
    if new.text.trim().is_empty() {
        return Err(OxidGeneError::Validation(
            "text must not be empty".to_string(),
        ));
    }
    let txn = begin_tx(db).await?;
    for (resource, id) in [
        (TreeResource::Person, new.person_id),
        (TreeResource::Event, new.event_id),
        (TreeResource::Family, new.family_id),
        (TreeResource::Source, new.source_id),
        (TreeResource::Media, new.media_id),
    ] {
        if let Some(id) = id {
            require_tree_resource(&txn, tree_id, resource, id).await?;
        }
    }
    let note = NoteRepo::create(
        &txn,
        Uuid::now_v7(),
        tree_id,
        new.text,
        new.person_id,
        new.event_id,
        new.family_id,
        new.source_id,
        new.media_id,
    )
    .await?;
    if let Some(person_id) = note.person_id {
        profiles
            .invalidate_for_mutation(&txn, tree_id, &[person_id])
            .await?;
    }
    history::note_change(tree_id, AuditAction::Create, &note)
        .record(&txn)
        .await?;
    commit_tx(txn).await?;
    Ok(note)
}

/// Update note `id` of `tree_id`; `None` keeps its text.
pub async fn update_note(
    db: &DatabaseConnection,
    profiles: &ProfileService,
    tree_id: Uuid,
    id: Uuid,
    text: Option<String>,
) -> Result<Note, OxidGeneError> {
    let txn = begin_tx(db).await?;
    require_tree_resource(&txn, tree_id, TreeResource::Note, id).await?;
    let previous = NoteRepo::get(&txn, id).await?;
    let note = NoteRepo::update(&txn, id, text).await?;
    if let Some(person_id) = previous.person_id {
        profiles
            .invalidate_for_mutation(&txn, tree_id, &[person_id])
            .await?;
    }
    history::note_change(tree_id, AuditAction::Update, &previous)
        .record(&txn)
        .await?;
    commit_tx(txn).await?;
    Ok(note)
}

/// Delete note `id` of `tree_id` (a soft delete).
pub async fn delete_note(
    db: &DatabaseConnection,
    profiles: &ProfileService,
    tree_id: Uuid,
    id: Uuid,
) -> Result<(), OxidGeneError> {
    let txn = begin_tx(db).await?;
    require_tree_resource(&txn, tree_id, TreeResource::Note, id).await?;
    let note = NoteRepo::get(&txn, id).await?;
    NoteRepo::delete(&txn, id).await?;
    if let Some(person_id) = note.person_id {
        profiles
            .invalidate_for_mutation(&txn, tree_id, &[person_id])
            .await?;
    }
    history::note_change(tree_id, AuditAction::Delete, &note)
        .record(&txn)
        .await?;
    commit_tx(txn).await
}
