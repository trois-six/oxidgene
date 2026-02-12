//! Shared GEDCOM import/export service logic.
//!
//! Extracted so both REST and GraphQL handlers can reuse the same
//! persist-all-entities and load-all-entities workflows.

use oxidgene_core::OxidGeneError;
use oxidgene_db::repo::{
    CitationRepo, EventRepo, FamilyChildRepo, FamilyRepo, FamilySpouseRepo, MediaLinkRepo,
    MediaRepo, NoteRepo, PersonAncestryRepo, PersonNameRepo, PersonRepo, PlaceRepo, SourceRepo,
    TreeRepo,
};
use oxidgene_gedcom::import::import_gedcom;
use sea_orm::DatabaseConnection;
use uuid::Uuid;

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

/// Parse a GEDCOM string and persist all extracted entities into the database.
///
/// Verifies the tree exists, parses the GEDCOM, then persists entities in
/// FK-safe order: places → sources → media → persons → person_names →
/// families → family_spouses → family_children → events → citations →
/// media_links → notes → person_ancestry.
pub async fn import_and_persist(
    db: &DatabaseConnection,
    tree_id: Uuid,
    gedcom_str: &str,
) -> Result<ImportSummary, OxidGeneError> {
    // Verify tree exists
    let _tree = TreeRepo::get(db, tree_id).await?;

    // Parse GEDCOM
    let result = import_gedcom(gedcom_str, tree_id).map_err(OxidGeneError::Gedcom)?;

    // Persist entities in FK-safe order:

    // 1. Places (no FKs to other imported entities)
    for p in &result.places {
        PlaceRepo::create(db, p.id, p.tree_id, p.name.clone(), p.latitude, p.longitude).await?;
    }

    // 2. Sources (no FKs to other imported entities)
    for s in &result.sources {
        SourceRepo::create(
            db,
            s.id,
            s.tree_id,
            s.title.clone(),
            s.author.clone(),
            s.publisher.clone(),
            s.abbreviation.clone(),
            s.repository_name.clone(),
        )
        .await?;
    }

    // 3. Media (no FKs to other imported entities)
    for m in &result.media {
        MediaRepo::create(
            db,
            m.id,
            m.tree_id,
            m.file_name.clone(),
            m.mime_type.clone(),
            m.file_path.clone(),
            m.file_size,
            m.title.clone(),
            m.description.clone(),
        )
        .await?;
    }

    // 4. Persons (FK → tree)
    for p in &result.persons {
        PersonRepo::create(db, p.id, p.tree_id, p.sex).await?;
    }

    // 5. Person names (FK → person)
    for pn in &result.person_names {
        PersonNameRepo::create(
            db,
            pn.id,
            pn.person_id,
            pn.name_type,
            pn.given_names.clone(),
            pn.surname.clone(),
            pn.prefix.clone(),
            pn.suffix.clone(),
            pn.nickname.clone(),
            pn.is_primary,
        )
        .await?;
    }

    // 6. Families (FK → tree)
    for f in &result.families {
        FamilyRepo::create(db, f.id, f.tree_id).await?;
    }

    // 7. Family spouses (FK → family, person)
    for fs in &result.family_spouses {
        FamilySpouseRepo::create(
            db,
            fs.id,
            fs.family_id,
            fs.person_id,
            fs.role,
            fs.sort_order,
        )
        .await?;
    }

    // 8. Family children (FK → family, person)
    for fc in &result.family_children {
        FamilyChildRepo::create(
            db,
            fc.id,
            fc.family_id,
            fc.person_id,
            fc.child_type,
            fc.sort_order,
        )
        .await?;
    }

    // 9. Events (FK → tree, person?, family?, place?)
    for e in &result.events {
        EventRepo::create(
            db,
            e.id,
            e.tree_id,
            e.event_type,
            e.date_value.clone(),
            e.date_sort,
            e.place_id,
            e.person_id,
            e.family_id,
            e.description.clone(),
        )
        .await?;
    }

    // 10. Citations (FK → source, person?, event?, family?)
    for c in &result.citations {
        CitationRepo::create(
            db,
            c.id,
            c.source_id,
            c.person_id,
            c.event_id,
            c.family_id,
            c.page.clone(),
            c.confidence,
            c.text.clone(),
        )
        .await?;
    }

    // 11. Media links (FK → media, person?, event?, source?, family?)
    for ml in &result.media_links {
        MediaLinkRepo::create(
            db,
            ml.id,
            ml.media_id,
            ml.person_id,
            ml.event_id,
            ml.source_id,
            ml.family_id,
            ml.sort_order,
        )
        .await?;
    }

    // 12. Notes (FK → tree, person?, event?, family?, source?)
    for n in &result.notes {
        NoteRepo::create(
            db,
            n.id,
            n.tree_id,
            n.text.clone(),
            n.person_id,
            n.event_id,
            n.family_id,
            n.source_id,
        )
        .await?;
    }

    // 13. Person ancestry closure table
    for pa in &result.person_ancestry {
        PersonAncestryRepo::create(
            db,
            pa.id,
            pa.tree_id,
            pa.ancestor_id,
            pa.descendant_id,
            pa.depth,
        )
        .await?;
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
