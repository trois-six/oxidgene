//! Shared GeneWeb `.gw` import service logic.
//!
//! Reading a `.gw` file yields the same domain-model entities a GEDCOM import
//! does, so persistence is delegated to
//! [`crate::service::gedcom::persist_import_result`] and only the parse step
//! differs. There is no `.gw` export: the format is read-only in OxidGene, as
//! it is in the underlying `geneweb` crate.

use oxidgene_core::OxidGeneError;
use oxidgene_db::repo::TreeRepo;
use oxidgene_gedcom::geneweb::import_geneweb;
use sea_orm::DatabaseConnection;
use uuid::Uuid;

use super::gedcom::{FileImportPhase, FileImportProgress, ImportSummary, persist_import_result};

/// Read a GeneWeb `.gw` file and persist all extracted entities into the tree.
///
/// `input` is the raw file content: `.gw` is ISO-8859-1 unless the file opts
/// into UTF-8, so it must not be decoded before it reaches the reader.
/// `origin_file` is the uploaded file's name, which GeneWeb records on every
/// family and which is echoed back in parse warnings.
pub async fn import_and_persist(
    db: &DatabaseConnection,
    tree_id: Uuid,
    input: &[u8],
    origin_file: &str,
) -> Result<ImportSummary, OxidGeneError> {
    // Verify tree exists
    let _tree = TreeRepo::get(db, tree_id).await?;

    let result = import_geneweb(input, origin_file, tree_id).map_err(OxidGeneError::Gedcom)?;

    persist_import_result(db, result).await
}

/// Read a GeneWeb temporary file and persist its entities.
pub async fn import_file_and_persist(
    db: &DatabaseConnection,
    tree_id: Uuid,
    path: &std::path::Path,
    origin_file: &str,
    progress: &FileImportProgress,
) -> Result<ImportSummary, OxidGeneError> {
    progress.enter(FileImportPhase::Parsing);
    let input = tokio::fs::read(path).await?;
    let _tree = TreeRepo::get(db, tree_id).await?;
    let result = import_geneweb(&input, origin_file, tree_id).map_err(OxidGeneError::Gedcom)?;
    progress.enter(FileImportPhase::Database);
    persist_import_result(db, result).await
}
