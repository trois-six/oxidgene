//! Shared GEDCOM import/export service logic.
//!
//! Extracted so both REST and GraphQL handlers can reuse the same
//! persist-all-entities and load-all-entities workflows.

use chrono::Utc;
use oxidgene_core::OxidGeneError;
use oxidgene_db::entities::{
    citation, event, family, family_child, family_spouse, media, media_link, note, person,
    person_ancestry, person_name, place, sea_enums, source,
};
use oxidgene_db::repo::{
    CitationRepo, EventRepo, FamilyChildRepo, FamilyRepo, FamilySpouseRepo, MediaLinkRepo,
    MediaRepo, NoteRepo, PersonNameRepo, PersonRepo, PlaceRepo, SourceRepo, TreeRepo,
};
use oxidgene_gedcom::import::import_gedcom;
use sea_orm::{ActiveModelTrait, DatabaseConnection, EntityTrait, Set, TransactionTrait};
use uuid::Uuid;

/// Maximum number of rows per `insert_many` batch.
///
/// SQLite has a variable limit of ~999; with 7 columns per row that's ~142 rows.
/// We use 100 as a safe default that works for all entity shapes.
const BATCH_SIZE: usize = 100;

/// Summary returned after a GEDCOM import.
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

/// Result returned after a GEDCOM export.
pub struct ExportData {
    pub gedcom: String,
    pub warnings: Vec<String>,
}

/// Insert a batch of active models using `insert_many`, chunked to stay within
/// SQLite's variable limit.
async fn batch_insert<E, A>(
    txn: &impl sea_orm::ConnectionTrait,
    models: Vec<A>,
) -> Result<(), OxidGeneError>
where
    E: EntityTrait,
    A: ActiveModelTrait<Entity = E> + Send + 'static,
{
    for chunk in models.chunks(BATCH_SIZE) {
        E::insert_many(chunk.to_vec())
            .exec(txn)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
    }
    Ok(())
}

/// Parse a GEDCOM string and persist all extracted entities into the database.
///
/// Uses a single database transaction for atomicity, and batch inserts for
/// performance. Entities are inserted in FK-safe order: places → sources →
/// media → persons → person_names → families → family_spouses →
/// family_children → events → citations → media_links → notes →
/// person_ancestry.
pub async fn import_and_persist(
    db: &DatabaseConnection,
    tree_id: Uuid,
    gedcom_str: &str,
) -> Result<ImportSummary, OxidGeneError> {
    // Verify tree exists
    let _tree = TreeRepo::get(db, tree_id).await?;

    // Parse GEDCOM
    let result = import_gedcom(gedcom_str, tree_id).map_err(OxidGeneError::Gedcom)?;

    let now = Utc::now();

    // Start a transaction for atomicity
    let txn = db
        .begin()
        .await
        .map_err(|e| OxidGeneError::Database(e.to_string()))?;

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
        batch_insert::<place::Entity, _>(&txn, models).await?;
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
        batch_insert::<source::Entity, _>(&txn, models).await?;
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
                file_size: Set(m.file_size),
                title: Set(m.title.clone()),
                description: Set(m.description.clone()),
                created_at: Set(now),
                updated_at: Set(now),
                deleted_at: Set(None),
            })
            .collect();
        batch_insert::<media::Entity, _>(&txn, models).await?;
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
                created_at: Set(now),
                updated_at: Set(now),
                deleted_at: Set(None),
            })
            .collect();
        batch_insert::<person::Entity, _>(&txn, models).await?;
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
                prefix: Set(pn.prefix.clone()),
                suffix: Set(pn.suffix.clone()),
                nickname: Set(pn.nickname.clone()),
                is_primary: Set(pn.is_primary),
                created_at: Set(now),
                updated_at: Set(now),
            })
            .collect();
        batch_insert::<person_name::Entity, _>(&txn, models).await?;
    }

    // 6. Families (FK → tree)
    if !result.families.is_empty() {
        let models: Vec<family::ActiveModel> = result
            .families
            .iter()
            .map(|f| family::ActiveModel {
                id: Set(f.id),
                tree_id: Set(f.tree_id),
                created_at: Set(now),
                updated_at: Set(now),
                deleted_at: Set(None),
            })
            .collect();
        batch_insert::<family::Entity, _>(&txn, models).await?;
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
        batch_insert::<family_spouse::Entity, _>(&txn, models).await?;
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
        batch_insert::<family_child::Entity, _>(&txn, models).await?;
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
                place_id: Set(e.place_id),
                person_id: Set(e.person_id),
                family_id: Set(e.family_id),
                description: Set(e.description.clone()),
                created_at: Set(now),
                updated_at: Set(now),
                deleted_at: Set(None),
            })
            .collect();
        batch_insert::<event::Entity, _>(&txn, models).await?;
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
        batch_insert::<citation::Entity, _>(&txn, models).await?;
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
        batch_insert::<media_link::Entity, _>(&txn, models).await?;
    }

    // 12. Notes (FK → tree, person?, event?, family?, source?)
    if !result.notes.is_empty() {
        let models: Vec<note::ActiveModel> = result
            .notes
            .iter()
            .map(|n| note::ActiveModel {
                id: Set(n.id),
                tree_id: Set(n.tree_id),
                text: Set(n.text.clone()),
                person_id: Set(n.person_id),
                event_id: Set(n.event_id),
                family_id: Set(n.family_id),
                source_id: Set(n.source_id),
                created_at: Set(now),
                updated_at: Set(now),
                deleted_at: Set(None),
            })
            .collect();
        batch_insert::<note::Entity, _>(&txn, models).await?;
    }

    // 13. Person ancestry closure table
    if !result.person_ancestry.is_empty() {
        let models: Vec<person_ancestry::ActiveModel> = result
            .person_ancestry
            .iter()
            .map(|pa| person_ancestry::ActiveModel {
                id: Set(pa.id),
                tree_id: Set(pa.tree_id),
                ancestor_id: Set(pa.ancestor_id),
                descendant_id: Set(pa.descendant_id),
                depth: Set(pa.depth),
            })
            .collect();
        batch_insert::<person_ancestry::Entity, _>(&txn, models).await?;
    }

    // Commit the transaction
    txn.commit()
        .await
        .map_err(|e| OxidGeneError::Database(e.to_string()))?;

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
/// exporter to produce the output string.
pub async fn load_and_export(
    db: &DatabaseConnection,
    tree_id: Uuid,
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
    let places = PlaceRepo::list_all(db, tree_id).await?;

    let sources = SourceRepo::list_all(db, tree_id).await?;
    let source_ids: Vec<_> = sources.iter().map(|s| s.id).collect();
    let citations = CitationRepo::list_by_sources(db, &source_ids).await?;

    let media = MediaRepo::list_all(db, tree_id).await?;
    let media_ids: Vec<_> = media.iter().map(|m| m.id).collect();
    let media_links = MediaLinkRepo::list_by_medias(db, &media_ids).await?;

    let notes = NoteRepo::list_all(db, tree_id).await?;

    // Export to GEDCOM
    let export_result = oxidgene_gedcom::export::export_gedcom(
        &persons,
        &person_names,
        &families,
        &family_spouses,
        &family_children,
        &events,
        &places,
        &sources,
        &citations,
        &media,
        &media_links,
        &notes,
    )
    .map_err(OxidGeneError::Gedcom)?;

    Ok(ExportData {
        gedcom: export_result.gedcom,
        warnings: export_result.warnings,
    })
}
