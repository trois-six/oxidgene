//! Shared GEDCOM import/export service logic.
//!
//! Extracted so both REST and GraphQL handlers can reuse the same
//! persist-all-entities and load-all-entities workflows. The persist half —
//! [`persist_import_result`] — is format-agnostic and also backs the GeneWeb
//! importer in [`crate::service::geneweb`].

use chrono::Utc;
use oxidgene_core::OxidGeneError;
use oxidgene_db::entities::{
    citation, event, event_witness, family, family_child, family_spouse, media, media_link,
    media_tag, note, person, person_name, place, sea_enums, source, vignette,
};
use oxidgene_db::html::sanitize_note_html;
use oxidgene_db::repo::{
    CitationRepo, EventRepo, EventWitnessRepo, FamilyChildRepo, FamilyRepo, FamilySpouseRepo,
    MediaLinkRepo, MediaRepo, NoteRepo, PersonNameRepo, PersonRepo, PlaceRepo, SourceRepo,
    TreeRepo, VignetteRepo,
};
use oxidgene_gedcom::import::import_gedcom;
use sea_orm::{
    ActiveModelTrait, ConnectionTrait, DatabaseConnection, EntityTrait, Set, TransactionTrait,
};
use std::path::Path;
use tracing::Instrument as _;
use uuid::Uuid;

/// Maximum number of rows per `insert_many` batch.
///
/// Five hundred rows keep the widest imported entity below 15,000 bind
/// parameters while substantially reducing database round trips.
const BATCH_SIZE: usize = 500;

/// Summary returned after a GEDCOM import.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct ImportSummary {
    pub persons_count: usize,
    pub families_count: usize,
    pub events_count: usize,
    pub sources_count: usize,
    pub media_count: usize,
    pub places_count: usize,
    pub notes_count: usize,
    pub warnings: Vec<String>,
}

/// Server-side stages of a file-backed GEDZIP import.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FileImportPhase {
    #[default]
    Starting,
    Parsing,
    Media,
    Database,
    Projections,
    Completed,
    Failed,
}

/// Progress shared by a background GEDZIP worker and its status endpoint.
#[derive(Debug, Default)]
pub struct FileImportProgress {
    done: std::sync::atomic::AtomicUsize,
    total: std::sync::atomic::AtomicUsize,
    phase: std::sync::Mutex<FileImportPhase>,
    result: std::sync::Mutex<Option<ImportSummary>>,
    error: std::sync::Mutex<Option<&'static str>>,
}

impl FileImportProgress {
    pub fn enter(&self, phase: FileImportPhase) {
        if let Ok(mut current) = self.phase.lock() {
            *current = phase;
        }
    }

    fn expect(&self, total: usize) {
        self.total
            .store(total, std::sync::atomic::Ordering::Relaxed);
    }

    fn advance(&self) {
        self.done.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn complete(&self, result: ImportSummary) {
        if let Ok(mut current) = self.result.lock() {
            *current = Some(result);
        }
        self.enter(FileImportPhase::Completed);
    }

    pub fn fail(&self, error: &'static str) {
        if let Ok(mut current) = self.error.lock() {
            *current = Some(error);
        }
        self.enter(FileImportPhase::Failed);
    }

    #[must_use]
    pub fn read(
        &self,
    ) -> (
        FileImportPhase,
        usize,
        usize,
        Option<ImportSummary>,
        Option<&'static str>,
    ) {
        (
            self.phase.lock().map(|phase| *phase).unwrap_or_default(),
            self.done.load(std::sync::atomic::Ordering::Relaxed),
            self.total.load(std::sync::atomic::Ordering::Relaxed),
            self.result.lock().ok().and_then(|result| result.clone()),
            self.error.lock().ok().and_then(|error| *error),
        )
    }
}

/// Result returned after a GEDCOM export.
pub struct ExportData {
    pub gedcom: String,
    pub warnings: Vec<String>,
    /// What a GEDZIP of this export must contain, as (storage key, path inside
    /// the archive, MIME type). Empty unless the export asked for archive
    /// paths: a plain `.ged` references the producer's own paths and carries
    /// no files.
    pub media_files: Vec<(String, String, String)>,
}

/// Insert a batch of active models using `insert_many`, chunked to stay within
/// SQLite's variable limit.
async fn batch_insert<E, A>(
    txn: &impl sea_orm::ConnectionTrait,
    models: Vec<A>,
    on_inserted: &mut impl FnMut(usize),
) -> Result<(), OxidGeneError>
where
    E: EntityTrait,
    A: ActiveModelTrait<Entity = E> + Send + 'static,
{
    let mut models = models.into_iter();
    loop {
        let chunk: Vec<_> = models.by_ref().take(BATCH_SIZE).collect();
        if chunk.is_empty() {
            break;
        }
        let inserted = chunk.len();
        E::insert_many(chunk)
            .exec(txn)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        on_inserted(inserted);
    }
    Ok(())
}

/// Parse a GEDCOM string and persist all extracted entities into the database.
///
/// See [`persist_import_result`] for the persistence guarantees.
#[tracing::instrument(name = "import.gedcom", skip_all)]
pub async fn import_and_persist(
    db: &DatabaseConnection,
    tree_id: Uuid,
    gedcom_str: &str,
) -> Result<ImportSummary, OxidGeneError> {
    // Verify tree exists
    let _tree = TreeRepo::get(db, tree_id).await?;

    // Parse GEDCOM
    let result = tracing::info_span!("import.parse", import.format = "gedcom")
        .in_scope(|| import_gedcom(gedcom_str, tree_id))
        .map_err(OxidGeneError::Gedcom)?;

    persist_import_result(db, result).await
}

/// Read a UTF-8 GEDCOM temporary file and persist its entities.
#[tracing::instrument(name = "import.gedcom", skip_all)]
pub async fn import_file_and_persist(
    db: &DatabaseConnection,
    tree_id: Uuid,
    path: &Path,
    progress: &FileImportProgress,
) -> Result<ImportSummary, OxidGeneError> {
    progress.enter(FileImportPhase::Parsing);
    let gedcom = tokio::fs::read_to_string(path).await?;
    let _tree = TreeRepo::get(db, tree_id).await?;
    let result = tracing::info_span!("import.parse", import.format = "gedcom")
        .in_scope(|| import_gedcom(&gedcom, tree_id))
        .map_err(OxidGeneError::Gedcom)?;
    progress.enter(FileImportPhase::Database);
    persist_import_result(db, result).await
}

/// Read a GEDZIP archive (`.gdz`) and persist everything it holds — the
/// genealogy *and* the media files it carries.
///
/// The genealogy half is [`import_and_persist`] by another name. What the
/// format adds is that the files travel with it, so every medium whose `FILE`
/// names an entry in the archive is ingested into the media store first and
/// its row written as a held medium — thumbnail, dimensions and all — rather
/// than as the unheld stub a plain `.ged` produces.
///
/// A file the store refuses (an unsupported type, or one over the upload
/// ceiling) costs that medium its bytes and nothing else: the record is still
/// written, the reason is reported in
/// [`ImportSummary::warnings`], and the rest of the archive still lands. The
/// alternative — failing a ten-thousand-person import over one stray file —
/// would be worse.
#[tracing::instrument(name = "import.gedzip", skip_all)]
pub async fn import_gedzip_and_persist(
    db: &DatabaseConnection,
    store: &dyn crate::media::MediaStore,
    tree_id: Uuid,
    archive: &[u8],
) -> Result<ImportSummary, OxidGeneError> {
    let _tree = TreeRepo::get(db, tree_id).await?;

    let (mut reader, plan) = tracing::info_span!("import.parse", import.format = "gedzip")
        .in_scope(|| {
            oxidgene_gedcom::import::prepare_gedzip(std::io::Cursor::new(archive), tree_id)
        })
        .map_err(OxidGeneError::Gedcom)?;
    let oxidgene_gedcom::import::GedzipImportPlan { mut result, files } = plan;

    // The name to store each file under is the one its own record carries —
    // the archive path is `media/<uuid>.jpg` in an OxidGene export and
    // whatever the producer chose in anyone else's, neither of which is a name
    // worth showing.
    let names: std::collections::HashMap<Uuid, String> = result
        .media
        .iter()
        .map(|m| (m.id, m.file_name.clone()))
        .collect();

    let media_by_id: std::collections::HashMap<Uuid, usize> = result
        .media
        .iter()
        .enumerate()
        .map(|(index, media)| (media.id, index))
        .collect();

    let media_span = tracing::info_span!("import.media", import.format = "gedzip");
    async {
        for (media_id, entry_name) in files {
            let display_name = names.get(&media_id).map_or("upload", String::as_str);
            let outcome = match reader.read_media_file(&entry_name) {
                Ok(bytes) => crate::media::ingest(store, tree_id, display_name, bytes).await,
                Err(error) => {
                    result.warnings.push(format!(
                        "GEDZIP: '{entry_name}' could not be read out of the archive: {error}"
                    ));
                    continue;
                }
            };

            match outcome {
                Ok(ingested) => {
                    if let Some(media) = media_by_id
                        .get(&media_id)
                        .and_then(|index| result.media.get_mut(*index))
                    {
                        apply_ingested_media(media, ingested);
                    }
                }
                Err(error) => result
                    .warnings
                    .push(format!("GEDZIP: '{display_name}' was not stored: {error}")),
            }
        }
    }
    .instrument(media_span)
    .await;

    persist_import_result(db, result).await
}

/// Import a GEDZIP from a seekable temporary file without holding the archive
/// or all of its decompressed media in memory.
#[tracing::instrument(name = "import.gedzip", skip_all)]
pub async fn import_gedzip_file_and_persist(
    db: &DatabaseConnection,
    store: &dyn crate::media::MediaStore,
    tree_id: Uuid,
    archive_path: &Path,
    progress: &FileImportProgress,
) -> Result<ImportSummary, OxidGeneError> {
    let _tree = TreeRepo::get(db, tree_id).await?;
    let result = prepare_gedzip_file(store, tree_id, archive_path, progress).await?;
    progress.enter(FileImportPhase::Database);
    persist_import_result(db, result).await
}

/// Parse and ingest a seekable GEDZIP source without writing database rows.
#[tracing::instrument(name = "import.gedzip.prepare", skip_all)]
pub(crate) async fn prepare_gedzip_file(
    store: &dyn crate::media::MediaStore,
    tree_id: Uuid,
    archive_path: &Path,
    progress: &FileImportProgress,
) -> Result<oxidgene_gedcom::ImportResult, OxidGeneError> {
    progress.enter(FileImportPhase::Parsing);

    let file = std::fs::File::open(archive_path).map_err(OxidGeneError::Io)?;
    let (mut reader, plan) = tracing::info_span!("import.parse", import.format = "gedzip")
        .in_scope(|| oxidgene_gedcom::import::prepare_gedzip(file, tree_id))
        .map_err(OxidGeneError::Gedcom)?;
    let oxidgene_gedcom::import::GedzipImportPlan { mut result, files } = plan;
    let names: std::collections::HashMap<Uuid, String> = result
        .media
        .iter()
        .map(|media| (media.id, media.file_name.clone()))
        .collect();
    let media_by_id: std::collections::HashMap<Uuid, usize> = result
        .media
        .iter()
        .enumerate()
        .map(|(index, media)| (media.id, index))
        .collect();

    progress.expect(files.len());
    progress.enter(FileImportPhase::Media);
    let media_span = tracing::info_span!("import.media", import.format = "gedzip");
    async {
        for (media_id, entry_name) in files {
            let display_name = names.get(&media_id).map_or("upload", String::as_str);
            let outcome = match reader.read_media_file(&entry_name) {
                Ok(bytes) => crate::media::ingest(store, tree_id, display_name, bytes).await,
                Err(error) => {
                    result.warnings.push(format!(
                        "GEDZIP: '{entry_name}' could not be read out of the archive: {error}"
                    ));
                    progress.advance();
                    continue;
                }
            };

            match outcome {
                Ok(ingested) => {
                    if let Some(media) = media_by_id
                        .get(&media_id)
                        .and_then(|index| result.media.get_mut(*index))
                    {
                        apply_ingested_media(media, ingested);
                    }
                }
                Err(error) => result
                    .warnings
                    .push(format!("GEDZIP: '{display_name}' was not stored: {error}")),
            }
            progress.advance();
        }
    }
    .instrument(media_span)
    .await;

    Ok(result)
}

fn apply_ingested_media(
    media: &mut oxidgene_core::types::Media,
    ingested: crate::media::IngestedMedia,
) {
    media.file_path.clone_from(&ingested.file_name);
    media.file_name = ingested.file_name;
    media.mime_type = ingested.mime_type;
    media.storage_key = Some(ingested.storage_key);
    media.sha256 = Some(ingested.sha256);
    media.file_size = ingested.file_size;
    media.thumbnail_key = ingested.thumbnail_key;
    media.width = ingested.width;
    media.height = ingested.height;
    media.page_count = ingested.page_count;
}

/// Persist every entity of a parsed import into the database.
///
/// Format-agnostic: it takes the domain-model output of any importer (GEDCOM,
/// GeneWeb `.gw`), so all import formats share one persistence path.
///
/// Uses a single database transaction for atomicity, and batch inserts for
/// performance. Entities are inserted in FK-safe order: places → sources →
/// media → persons → person_names → families → family_spouses →
/// family_children → events → citations → media_links → vignettes → notes.
pub(crate) async fn persist_import_result(
    db: &DatabaseConnection,
    result: oxidgene_gedcom::ImportResult,
) -> Result<ImportSummary, OxidGeneError> {
    persist_import_result_with_progress(db, result, |_| {}).await
}

#[tracing::instrument(name = "import.persist", skip_all)]
pub(crate) async fn persist_import_result_with_progress(
    db: &DatabaseConnection,
    result: oxidgene_gedcom::ImportResult,
    mut on_inserted: impl FnMut(usize),
) -> Result<ImportSummary, OxidGeneError> {
    let txn = db
        .begin()
        .await
        .map_err(|e| OxidGeneError::Database(e.to_string()))?;
    let summary = persist_import_result_in_with_progress(&txn, result, &mut on_inserted).await?;
    txn.commit()
        .await
        .map_err(|e| OxidGeneError::Database(e.to_string()))?;
    Ok(summary)
}

#[tracing::instrument(name = "import.persist", skip_all)]
pub(crate) async fn persist_import_result_in(
    db: &impl ConnectionTrait,
    result: oxidgene_gedcom::ImportResult,
) -> Result<ImportSummary, OxidGeneError> {
    persist_import_result_in_with_progress(db, result, &mut |_| {}).await
}

async fn persist_import_result_in_with_progress(
    db: &impl ConnectionTrait,
    result: oxidgene_gedcom::ImportResult,
    on_inserted: &mut impl FnMut(usize),
) -> Result<ImportSummary, OxidGeneError> {
    let now = Utc::now();

    // 1. Places (no FKs to other imported entities)
    if !result.places.is_empty() {
        let models: Vec<place::ActiveModel> = result
            .places
            .iter()
            .map(|p| place::ActiveModel {
                id: Set(p.id),
                tree_id: Set(p.tree_id),
                name: Set(p.name.clone()),
                latitude: Set(p.latitude),
                longitude: Set(p.longitude),
                created_at: Set(now),
                updated_at: Set(now),
            })
            .collect();
        batch_insert::<place::Entity, _>(db, models, on_inserted).await?;
    }

    // 2. Sources (no FKs to other imported entities)
    if !result.sources.is_empty() {
        let models: Vec<source::ActiveModel> = result
            .sources
            .iter()
            .map(|s| source::ActiveModel {
                id: Set(s.id),
                tree_id: Set(s.tree_id),
                title: Set(s.title.clone()),
                author: Set(s.author.clone()),
                publisher: Set(s.publisher.clone()),
                abbreviation: Set(s.abbreviation.clone()),
                repository_name: Set(s.repository_name.clone()),
                created_at: Set(now),
                updated_at: Set(now),
                deleted_at: Set(None),
            })
            .collect();
        batch_insert::<source::Entity, _>(db, models, on_inserted).await?;
    }

    // 3. Media (no FKs to other imported entities)
    if !result.media.is_empty() {
        let models: Vec<media::ActiveModel> = result
            .media
            .iter()
            .map(|m| media::ActiveModel {
                id: Set(m.id),
                tree_id: Set(m.tree_id),
                file_name: Set(m.file_name.clone()),
                mime_type: Set(m.mime_type.clone()),
                file_path: Set(m.file_path.clone()),
                storage_key: Set(m.storage_key.clone()),
                sha256: Set(m.sha256.clone()),
                thumbnail_key: Set(m.thumbnail_key.clone()),
                width: Set(m.width),
                height: Set(m.height),
                page_count: Set(m.page_count),
                parent_media_id: Set(m.parent_media_id),
                page_index: Set(m.page_index),
                is_document: Set(m.is_document),
                file_size: Set(m.file_size),
                title: Set(m.title.clone()),
                description: Set(m.description.clone()),
                date_value: Set(m.date_value.clone()),
                date_sort: Set(m.date_sort),
                date_qualifier: Set(m.date_qualifier.into()),
                date_value2: Set(m.date_value2.clone()),
                calendar: Set(m.calendar.into()),
                privacy: Set(m.privacy.into()),
                source_media_type: Set(m.source_media_type.into()),
                document_category: Set(m.document_category.map(|c| c.as_str().to_string())),
                place_id: Set(m.place_id),
                created_at: Set(now),
                updated_at: Set(now),
                deleted_at: Set(None),
            })
            .collect();
        batch_insert::<media::Entity, _>(db, models, on_inserted).await?;

        let mut seen_tags = std::collections::HashSet::new();
        let mut tags = Vec::new();
        for media in &result.media {
            for tag in &media.tags {
                let tag = tag.trim();
                let normalized_tag = tag.to_lowercase();
                if !tag.is_empty() && seen_tags.insert((media.id, normalized_tag.clone())) {
                    tags.push(media_tag::ActiveModel {
                        media_id: Set(media.id),
                        normalized_tag: Set(normalized_tag),
                        tag: Set(tag.to_string()),
                        created_at: Set(now),
                    });
                }
            }
        }
        batch_insert::<media_tag::Entity, _>(db, tags, &mut |_| {}).await?;
    }

    // 4. Persons (FK → tree)
    if !result.persons.is_empty() {
        let models: Vec<person::ActiveModel> = result
            .persons
            .iter()
            .map(|p| person::ActiveModel {
                id: Set(p.id),
                tree_id: Set(p.tree_id),
                sex: Set(sea_enums::Sex::from(p.sex)),
                portrait_media_id: Set(p.portrait_media_id),
                portrait_vignette_id: Set(p.portrait_vignette_id),
                privacy: Set(sea_enums::Privacy::from(p.privacy)),
                created_at: Set(now),
                updated_at: Set(now),
                deleted_at: Set(None),
            })
            .collect();
        batch_insert::<person::Entity, _>(db, models, on_inserted).await?;
    }

    // 5. Person names (FK → person)
    if !result.person_names.is_empty() {
        let models: Vec<person_name::ActiveModel> = result
            .person_names
            .iter()
            .map(|pn| person_name::ActiveModel {
                id: Set(pn.id),
                person_id: Set(pn.person_id),
                name_type: Set(sea_enums::NameType::from(pn.name_type)),
                given_names: Set(pn.given_names.clone()),
                surname: Set(pn.surname.clone()),
                surname_prefix: Set(pn.surname_prefix.clone()),
                prefix: Set(pn.prefix.clone()),
                suffix: Set(pn.suffix.clone()),
                nickname: Set(pn.nickname.clone()),
                is_primary: Set(pn.is_primary),
                sort_order: Set(pn.sort_order),
                created_at: Set(now),
                updated_at: Set(now),
            })
            .collect();
        batch_insert::<person_name::Entity, _>(db, models, on_inserted).await?;
    }

    // 6. Families (FK → tree)
    if !result.families.is_empty() {
        let models: Vec<family::ActiveModel> = result
            .families
            .iter()
            .map(|f| family::ActiveModel {
                id: Set(f.id),
                tree_id: Set(f.tree_id),
                privacy: Set(f.privacy.into()),
                created_at: Set(now),
                updated_at: Set(now),
                deleted_at: Set(None),
            })
            .collect();
        batch_insert::<family::Entity, _>(db, models, on_inserted).await?;
    }

    // 7. Family spouses (FK → family, person)
    if !result.family_spouses.is_empty() {
        let models: Vec<family_spouse::ActiveModel> = result
            .family_spouses
            .iter()
            .map(|fs| family_spouse::ActiveModel {
                id: Set(fs.id),
                family_id: Set(fs.family_id),
                person_id: Set(fs.person_id),
                role: Set(sea_enums::SpouseRole::from(fs.role)),
                sort_order: Set(fs.sort_order),
            })
            .collect();
        batch_insert::<family_spouse::Entity, _>(db, models, on_inserted).await?;
    }

    // 8. Family children (FK → family, person)
    if !result.family_children.is_empty() {
        let models: Vec<family_child::ActiveModel> = result
            .family_children
            .iter()
            .map(|fc| family_child::ActiveModel {
                id: Set(fc.id),
                family_id: Set(fc.family_id),
                person_id: Set(fc.person_id),
                child_type: Set(sea_enums::ChildType::from(fc.child_type)),
                sort_order: Set(fc.sort_order),
            })
            .collect();
        batch_insert::<family_child::Entity, _>(db, models, on_inserted).await?;
    }

    // 9. Events (FK → tree, person?, family?, place?)
    if !result.events.is_empty() {
        let models: Vec<event::ActiveModel> = result
            .events
            .iter()
            .map(|e| event::ActiveModel {
                id: Set(e.id),
                tree_id: Set(e.tree_id),
                event_type: Set(sea_enums::EventType::from(e.event_type)),
                date_value: Set(e.date_value.clone()),
                date_sort: Set(e.date_sort),
                date_qualifier: Set(sea_enums::DateQualifier::from(e.date_qualifier)),
                date_value2: Set(e.date_value2.clone()),
                calendar: Set(sea_enums::Calendar::from(e.calendar)),
                cause: Set(e.cause.clone()),
                place_id: Set(e.place_id),
                person_id: Set(e.person_id),
                family_id: Set(e.family_id),
                description: Set(e.description.clone()),
                created_at: Set(now),
                updated_at: Set(now),
                deleted_at: Set(None),
            })
            .collect();
        batch_insert::<event::Entity, _>(db, models, on_inserted).await?;
    }

    // 9b. Event witnesses (FK → event, person)
    if !result.event_witnesses.is_empty() {
        let models: Vec<event_witness::ActiveModel> = result
            .event_witnesses
            .iter()
            .map(|w| event_witness::ActiveModel {
                id: Set(w.id),
                event_id: Set(w.event_id),
                person_id: Set(w.person_id),
                relation: Set(w.relation.clone()),
                sort_order: Set(w.sort_order),
            })
            .collect();
        batch_insert::<event_witness::Entity, _>(db, models, on_inserted).await?;
    }

    // 10. Citations (FK → source, person?, event?, family?)
    if !result.citations.is_empty() {
        let models: Vec<citation::ActiveModel> = result
            .citations
            .iter()
            .map(|c| citation::ActiveModel {
                id: Set(c.id),
                source_id: Set(c.source_id),
                person_id: Set(c.person_id),
                event_id: Set(c.event_id),
                family_id: Set(c.family_id),
                page: Set(c.page.clone()),
                confidence: Set(sea_enums::Confidence::from(c.confidence)),
                text: Set(c.text.clone()),
                created_at: Set(now),
                updated_at: Set(now),
            })
            .collect();
        batch_insert::<citation::Entity, _>(db, models, on_inserted).await?;
    }

    // 11. Media links (FK → media, person?, event?, source?, family?)
    if !result.media_links.is_empty() {
        let models: Vec<media_link::ActiveModel> = result
            .media_links
            .iter()
            .map(|ml| media_link::ActiveModel {
                id: Set(ml.id),
                media_id: Set(ml.media_id),
                person_id: Set(ml.person_id),
                event_id: Set(ml.event_id),
                source_id: Set(ml.source_id),
                family_id: Set(ml.family_id),
                sort_order: Set(ml.sort_order),
            })
            .collect();
        batch_insert::<media_link::Entity, _>(db, models, on_inserted).await?;
    }

    // 12. Vignettes (FK → media, person?, event?)
    if !result.vignettes.is_empty() {
        let models: Vec<vignette::ActiveModel> = result
            .vignettes
            .iter()
            .map(|v| vignette::ActiveModel {
                id: Set(v.id),
                media_id: Set(v.media_id),
                page: Set(v.page),
                x: Set(v.x),
                y: Set(v.y),
                width: Set(v.width),
                height: Set(v.height),
                person_id: Set(v.person_id),
                event_id: Set(v.event_id),
                created_at: Set(now),
                updated_at: Set(now),
            })
            .collect();
        batch_insert::<vignette::Entity, _>(db, models, on_inserted).await?;
    }

    // 13. Notes (FK → tree, person?, event?, family?, source?)
    if !result.notes.is_empty() {
        let models: Vec<note::ActiveModel> = result
            .notes
            .iter()
            .map(|n| note::ActiveModel {
                id: Set(n.id),
                tree_id: Set(n.tree_id),
                // Imported bodies are rendered as HTML and reach here as a
                // batch insert, bypassing `NoteRepo`'s own sanitizing.
                text: Set(sanitize_note_html(&n.text)),
                person_id: Set(n.person_id),
                event_id: Set(n.event_id),
                family_id: Set(n.family_id),
                source_id: Set(n.source_id),
                media_id: Set(n.media_id),
                created_at: Set(now),
                updated_at: Set(now),
                deleted_at: Set(None),
            })
            .collect();
        batch_insert::<note::Entity, _>(db, models, on_inserted).await?;
    }

    Ok(ImportSummary {
        persons_count: result.persons.len(),
        families_count: result.families.len(),
        events_count: result.events.len(),
        sources_count: result.sources.len(),
        media_count: result.media.len(),
        places_count: result.places.len(),
        notes_count: result.notes.len(),
        warnings: result.warnings,
    })
}

/// Load all entities from a tree and export them as a GEDCOM string.
///
/// Verifies the tree exists, loads all entities, then calls the GEDCOM
/// exporter to produce the output string. `merge_occupations` collapses each
/// person's multiple `OCCU` tags back into one, and `merge_names` collapses
/// each person's non-primary names into the primary name's `SURN` tag (see
/// `oxidgene_gedcom::export::export_gedcom`).
#[tracing::instrument(name = "export.load", skip_all, fields(export.for_archive = for_archive))]
pub async fn load_and_export(
    db: &DatabaseConnection,
    tree_id: Uuid,
    merge_occupations: bool,
    merge_names: bool,
    for_archive: bool,
) -> Result<ExportData, OxidGeneError> {
    // Verify tree exists
    let _tree = TreeRepo::get(db, tree_id).await?;

    // Load all entities for the tree
    let persons = PersonRepo::list_all(db, tree_id).await?;
    let person_ids: Vec<_> = persons.iter().map(|p| p.id).collect();

    let person_names = PersonNameRepo::list_by_persons(db, &person_ids).await?;

    let families = FamilyRepo::list_all(db, tree_id).await?;
    let family_ids: Vec<_> = families.iter().map(|f| f.id).collect();

    let family_spouses = FamilySpouseRepo::list_by_families(db, &family_ids).await?;
    let family_children = FamilyChildRepo::list_by_families(db, &family_ids).await?;

    let events = EventRepo::list_all(db, tree_id).await?;
    let event_ids: Vec<_> = events.iter().map(|e| e.id).collect();
    let event_witnesses = EventWitnessRepo::list_by_events(db, &event_ids).await?;
    let places = PlaceRepo::list_all(db, tree_id).await?;

    let sources = SourceRepo::list_all(db, tree_id).await?;
    let source_ids: Vec<_> = sources.iter().map(|s| s.id).collect();
    let citations = CitationRepo::list_by_sources(db, &source_ids).await?;

    let media = MediaRepo::list_all(db, tree_id).await?;
    let media_ids: Vec<_> = media.iter().map(|m| m.id).collect();
    let media_links = MediaLinkRepo::list_by_medias(db, &media_ids).await?;
    let vignettes = VignetteRepo::list_for_medias(db, &media_ids).await?;

    let notes = NoteRepo::list_all(db, tree_id).await?;

    // A GEDZIP carries the bytes, so its `FILE` lines name entries inside the
    // archive rather than the paths whatever produced the record used. Media
    // we hold no bytes for keep their original value — there is nothing to
    // pack for them and nothing better to say.
    let mut media_paths = std::collections::HashMap::new();
    let mut media_files = Vec::new();
    if for_archive {
        for medium in &media {
            let (Some(path), Some(key)) = (
                oxidgene_gedcom::export::archive_path(medium),
                medium.storage_key.clone(),
            ) else {
                continue;
            };
            media_paths.insert(medium.id, path.clone());
            media_files.push((key, path, medium.mime_type.clone()));
        }
    }

    // Export to GEDCOM
    let export_result = tracing::info_span!("export.serialize", export.format = "gedcom")
        .in_scope(|| {
            oxidgene_gedcom::export::export_gedcom(
                &persons,
                &person_names,
                &families,
                &family_spouses,
                &family_children,
                &events,
                &event_witnesses,
                &places,
                &sources,
                &citations,
                &media,
                &media_links,
                &vignettes,
                &notes,
                merge_occupations,
                merge_names,
                &media_paths,
            )
        })
        .map_err(OxidGeneError::Gedcom)?;

    Ok(ExportData {
        gedcom: export_result.gedcom,
        warnings: export_result.warnings,
        media_files,
    })
}
