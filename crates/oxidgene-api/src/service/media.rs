//! Definitive media deletion shared by REST and GraphQL.

use crate::media::MediaStore;
use oxidgene_core::OxidGeneError;
use oxidgene_db::repo::MediaRepo;
use sea_orm::{DatabaseConnection, TransactionTrait};
use uuid::Uuid;

/// Remove a media permanently and delete its stored objects.
///
/// Passing a gallery link turns this into a conditional cleanup: the record is
/// retained when it has any other reference. The database cleanup is committed
/// before objects leave the store, so readers never observe rows whose
/// relationships were only partly removed.
pub async fn purge_media(
    db: &DatabaseConnection,
    store: &dyn MediaStore,
    media_id: Uuid,
    allowed_link_id: Option<Uuid>,
) -> Result<bool, OxidGeneError> {
    let tx = db
        .begin()
        .await
        .map_err(|error| OxidGeneError::Database(error.to_string()))?;
    let purge = match allowed_link_id {
        Some(link_id) => MediaRepo::purge_if_unreferenced_elsewhere(&tx, media_id, link_id).await?,
        None => Some(MediaRepo::purge(&tx, media_id).await?),
    };
    let Some(purge) = purge else {
        return Ok(false);
    };
    tx.commit()
        .await
        .map_err(|error| OxidGeneError::Database(error.to_string()))?;

    for key in purge.storage_keys {
        store.delete(&key).await?;
    }
    Ok(true)
}
