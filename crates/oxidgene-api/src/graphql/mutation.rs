//! GraphQL mutation root with all write operations.

use crate::profile::invalidation;
use crate::rest::state::{begin_tx, commit_tx};
use async_graphql::{Context, ID, Object, Result};
use chrono::NaiveDate;
use uuid::Uuid;

use oxidgene_db::repo::{
    CitationRepo, EventRepo, EventWitnessRepo, FamilyChildRepo, FamilyRepo, FamilySpouseRepo,
    MediaLinkRepo, MediaRepo, NoteRepo, PersonNameRepo, PersonRepo, PlaceRepo, SourceRepo,
    TreeRepo,
};

use super::inputs::{
    AddChildInput, AddEventWitnessInput, AddSpouseInput, CreateCitationInput, CreateEventInput,
    CreateMediaLinkInput, CreateNoteInput, CreatePersonInput, CreatePlaceInput, CreateSourceInput,
    CreateTreeInput, ImportGedcomInput, ImportGenewebInput, PersonNameInput, UpdateCitationInput,
    UpdateEventInput, UpdateMediaInput, UpdateNoteInput, UpdatePersonInput, UpdatePersonNameInput,
    UpdatePlaceInput, UpdateSourceInput, UpdateTreeInput, UploadMediaInput,
};
use super::types::{
    GqlCitation, GqlEvent, GqlEventWitness, GqlFamily, GqlFamilyChild, GqlFamilySpouse,
    GqlImportResult, GqlMedia, GqlMediaLink, GqlNote, GqlPedigreeDelta, GqlPedigreeDirection,
    GqlPerson, GqlPersonName, GqlPlace, GqlProfileRebuildResult, GqlSource, GqlTree, db_from_ctx,
    profiles_from_ctx, purge_from_ctx,
};

/// Convert a service-layer import summary into its GraphQL shape.
///
/// Shared by every import mutation — the summary is format-agnostic.
fn import_result(summary: crate::service::gedcom::ImportSummary) -> GqlImportResult {
    GqlImportResult {
        persons_count: summary.persons_count as i32,
        families_count: summary.families_count as i32,
        events_count: summary.events_count as i32,
        sources_count: summary.sources_count as i32,
        media_count: summary.media_count as i32,
        places_count: summary.places_count as i32,
        notes_count: summary.notes_count as i32,
        warnings: summary.warnings,
    }
}

/// The root mutation type.
pub struct MutationRoot;

#[Object]
impl MutationRoot {
    // ── Tree Mutations ───────────────────────────────────────────────

    /// Create a new tree.
    async fn create_tree(&self, ctx: &Context<'_>, input: CreateTreeInput) -> Result<GqlTree> {
        let db = db_from_ctx(ctx);
        let id = Uuid::now_v7();
        let tree = TreeRepo::create(db, id, input.name, input.description).await?;
        Ok(tree.into())
    }

    /// Update an existing tree.
    async fn update_tree(
        &self,
        ctx: &Context<'_>,
        id: ID,
        input: UpdateTreeInput,
    ) -> Result<GqlTree> {
        let db = db_from_ctx(ctx);
        let uuid = Uuid::parse_str(id.as_str())?;
        let sosa_root = input
            .sosa_root_person_id
            .map(|s| Uuid::parse_str(&s).map(Some))
            .transpose()
            .map_err(|e| async_graphql::Error::new(format!("Invalid sosa_root_person_id: {e}")))?;
        let tree =
            TreeRepo::update(db, uuid, input.name, input.description.map(Some), sosa_root).await?;
        Ok(tree.into())
    }

    /// Delete a tree.
    ///
    /// Flags it as deleted and returns straight away; the rows it owns and its
    /// projections are removed by the background purge worker. See
    /// [`crate::service::purge`].
    async fn delete_tree(&self, ctx: &Context<'_>, id: ID) -> Result<bool> {
        let db = db_from_ctx(ctx);
        let uuid = Uuid::parse_str(id.as_str())?;
        TreeRepo::soft_delete(db, uuid).await?;
        purge_from_ctx(ctx).enqueue(uuid);
        Ok(true)
    }

    // ── Person Mutations ─────────────────────────────────────────────

    /// Create a new person in a tree.
    async fn create_person(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        input: CreatePersonInput,
    ) -> Result<GqlPerson> {
        let db = db_from_ctx(ctx);
        let profiles = profiles_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let id = Uuid::now_v7();
        let txn = begin_tx(db).await?;
        let person = PersonRepo::create(&txn, id, tid, input.sex.into()).await?;
        // New person is not linked to any family yet — just build its projection.
        profiles.rebuild_person(&txn, tid, id).await?;
        commit_tx(txn).await?;
        Ok(person.into())
    }

    /// Update a person.
    async fn update_person(
        &self,
        ctx: &Context<'_>,
        id: ID,
        input: UpdatePersonInput,
    ) -> Result<GqlPerson> {
        let db = db_from_ctx(ctx);
        let profiles = profiles_from_ctx(ctx);
        let uuid = Uuid::parse_str(id.as_str())?;
        let txn = begin_tx(db).await?;
        let person = PersonRepo::update(
            &txn,
            uuid,
            input.sex.map(|s| s.into()),
            input.privacy.map(|p| p.into()),
        )
        .await?;
        // Rebuild the affected set (person + spouses + children + parents).
        let affected = invalidation::affected_persons(&txn, uuid).await?;
        profiles
            .invalidate_for_mutation(&txn, person.tree_id, &affected)
            .await?;
        commit_tx(txn).await?;
        Ok(person.into())
    }

    /// Delete a person (soft delete).
    async fn delete_person(&self, ctx: &Context<'_>, id: ID) -> Result<bool> {
        let db = db_from_ctx(ctx);
        let profiles = profiles_from_ctx(ctx);
        let uuid = Uuid::parse_str(id.as_str())?;
        let txn = begin_tx(db).await?;
        let person = PersonRepo::get(&txn, uuid).await?;
        PersonRepo::delete(&txn, uuid).await?;
        // Drops the person's projection + search row and refreshes the
        // relatives that referenced them.
        profiles
            .invalidate_for_person_delete(&txn, person.tree_id, uuid)
            .await?;
        commit_tx(txn).await?;
        Ok(true)
    }

    // ── PersonName Mutations ─────────────────────────────────────────

    /// Add a name to a person.
    async fn add_person_name(
        &self,
        ctx: &Context<'_>,
        person_id: ID,
        input: PersonNameInput,
    ) -> Result<GqlPersonName> {
        let db = db_from_ctx(ctx);
        let profiles = profiles_from_ctx(ctx);
        let pid = Uuid::parse_str(person_id.as_str())?;
        let id = Uuid::now_v7();
        let txn = begin_tx(db).await?;
        let name = PersonNameRepo::create(
            &txn,
            id,
            pid,
            input.name_type.into(),
            input.given_names,
            input.surname,
            input.prefix,
            input.suffix,
            input.nickname,
            input.is_primary,
        )
        .await?;
        // Name changes affect display_name references across relatives.
        let affected = invalidation::affected_persons(&txn, pid).await?;
        let person = PersonRepo::get(&txn, pid).await?;
        profiles
            .invalidate_for_mutation(&txn, person.tree_id, &affected)
            .await?;
        commit_tx(txn).await?;
        Ok(name.into())
    }

    /// Update a person name.
    async fn update_person_name(
        &self,
        ctx: &Context<'_>,
        id: ID,
        input: UpdatePersonNameInput,
    ) -> Result<GqlPersonName> {
        let db = db_from_ctx(ctx);
        let profiles = profiles_from_ctx(ctx);
        let uuid = Uuid::parse_str(id.as_str())?;
        let txn = begin_tx(db).await?;
        let name = PersonNameRepo::update(
            &txn,
            uuid,
            input.name_type.map(|nt| nt.into()),
            input.given_names.map(Some),
            input.surname.map(Some),
            input.prefix.map(Some),
            input.suffix.map(Some),
            input.nickname.map(Some),
            input.is_primary,
        )
        .await?;
        let affected = invalidation::affected_persons(&txn, name.person_id).await?;
        let person = PersonRepo::get(&txn, name.person_id).await?;
        profiles
            .invalidate_for_mutation(&txn, person.tree_id, &affected)
            .await?;
        commit_tx(txn).await?;
        Ok(name.into())
    }

    /// Delete a person name (hard delete).
    async fn delete_person_name(&self, ctx: &Context<'_>, person_id: ID, id: ID) -> Result<bool> {
        let db = db_from_ctx(ctx);
        let profiles = profiles_from_ctx(ctx);
        let pid = Uuid::parse_str(person_id.as_str())?;
        let uuid = Uuid::parse_str(id.as_str())?;
        let txn = begin_tx(db).await?;
        PersonNameRepo::delete(&txn, uuid).await?;
        let affected = invalidation::affected_persons(&txn, pid).await?;
        let person = PersonRepo::get(&txn, pid).await?;
        profiles
            .invalidate_for_mutation(&txn, person.tree_id, &affected)
            .await?;
        commit_tx(txn).await?;
        Ok(true)
    }

    // ── Family Mutations ─────────────────────────────────────────────

    /// Create a new family in a tree.
    async fn create_family(&self, ctx: &Context<'_>, tree_id: ID) -> Result<GqlFamily> {
        let db = db_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let id = Uuid::now_v7();
        let family = FamilyRepo::create(db, id, tid).await?;
        // No projection impact — empty family.
        Ok(family.into())
    }

    /// Update a family (touches updated_at).
    async fn update_family(&self, ctx: &Context<'_>, id: ID) -> Result<GqlFamily> {
        let db = db_from_ctx(ctx);
        let uuid = Uuid::parse_str(id.as_str())?;
        let family = FamilyRepo::update(db, uuid).await?;
        Ok(family.into())
    }

    /// Delete a family (soft delete).
    async fn delete_family(&self, ctx: &Context<'_>, id: ID) -> Result<bool> {
        let db = db_from_ctx(ctx);
        let profiles = profiles_from_ctx(ctx);
        let uuid = Uuid::parse_str(id.as_str())?;
        let txn = begin_tx(db).await?;
        let family = FamilyRepo::get(&txn, uuid).await?;
        // Compute affected BEFORE delete.
        let affected = invalidation::affected_persons_for_family(&txn, uuid).await?;
        FamilyRepo::delete(&txn, uuid).await?;
        if !affected.is_empty() {
            profiles
                .invalidate_for_mutation(&txn, family.tree_id, &affected)
                .await?;
        }
        commit_tx(txn).await?;
        Ok(true)
    }

    /// Add a spouse to a family.
    async fn add_spouse(
        &self,
        ctx: &Context<'_>,
        family_id: ID,
        input: AddSpouseInput,
    ) -> Result<GqlFamilySpouse> {
        let db = db_from_ctx(ctx);
        let profiles = profiles_from_ctx(ctx);
        let fid = Uuid::parse_str(family_id.as_str())?;
        let pid = Uuid::parse_str(&input.person_id)?;
        let id = Uuid::now_v7();
        let txn = begin_tx(db).await?;
        let spouse =
            FamilySpouseRepo::create(&txn, id, fid, pid, input.role.into(), input.sort_order)
                .await?;
        let affected =
            invalidation::affected_persons_for_family_spouse_change(&txn, fid, pid).await?;
        let family = FamilyRepo::get(&txn, fid).await?;
        profiles
            .invalidate_for_mutation(&txn, family.tree_id, &affected)
            .await?;
        commit_tx(txn).await?;
        Ok(spouse.into())
    }

    /// Remove a spouse from a family (hard delete).
    async fn remove_spouse(&self, ctx: &Context<'_>, family_id: ID, id: ID) -> Result<bool> {
        let db = db_from_ctx(ctx);
        let profiles = profiles_from_ctx(ctx);
        let fid = Uuid::parse_str(family_id.as_str())?;
        let uuid = Uuid::parse_str(id.as_str())?;
        let txn = begin_tx(db).await?;
        // Look up which person this spouse link refers to BEFORE deletion.
        let spouses = FamilySpouseRepo::list_by_families(&txn, &[fid]).await?;
        let person_id = spouses.iter().find(|s| s.id == uuid).map(|s| s.person_id);
        let family = FamilyRepo::get(&txn, fid).await?;
        // Compute affected BEFORE delete.
        let affected = if let Some(pid) = person_id {
            invalidation::affected_persons_for_family_spouse_change(&txn, fid, pid).await?
        } else {
            vec![]
        };
        FamilySpouseRepo::delete(&txn, uuid).await?;
        if !affected.is_empty() {
            profiles
                .invalidate_for_mutation(&txn, family.tree_id, &affected)
                .await?;
        }
        commit_tx(txn).await?;
        Ok(true)
    }

    /// Add a child to a family.
    async fn add_child(
        &self,
        ctx: &Context<'_>,
        family_id: ID,
        input: AddChildInput,
    ) -> Result<GqlFamilyChild> {
        let db = db_from_ctx(ctx);
        let profiles = profiles_from_ctx(ctx);
        let fid = Uuid::parse_str(family_id.as_str())?;
        let pid = Uuid::parse_str(&input.person_id)?;
        let id = Uuid::now_v7();
        let txn = begin_tx(db).await?;
        let child = FamilyChildRepo::create(
            &txn,
            id,
            fid,
            pid,
            input.child_type.into(),
            input.sort_order,
        )
        .await?;
        let affected =
            invalidation::affected_persons_for_family_child_change(&txn, fid, pid).await?;
        let family = FamilyRepo::get(&txn, fid).await?;
        profiles
            .invalidate_for_mutation(&txn, family.tree_id, &affected)
            .await?;
        commit_tx(txn).await?;
        Ok(child.into())
    }

    /// Remove a child from a family (hard delete).
    async fn remove_child(&self, ctx: &Context<'_>, family_id: ID, id: ID) -> Result<bool> {
        let db = db_from_ctx(ctx);
        let profiles = profiles_from_ctx(ctx);
        let fid = Uuid::parse_str(family_id.as_str())?;
        let uuid = Uuid::parse_str(id.as_str())?;
        let txn = begin_tx(db).await?;
        // Look up which person this child link refers to BEFORE deletion.
        let children = FamilyChildRepo::list_by_families(&txn, &[fid]).await?;
        let person_id = children.iter().find(|c| c.id == uuid).map(|c| c.person_id);
        let family = FamilyRepo::get(&txn, fid).await?;
        let affected = if let Some(pid) = person_id {
            invalidation::affected_persons_for_family_child_change(&txn, fid, pid).await?
        } else {
            vec![]
        };
        FamilyChildRepo::delete(&txn, uuid).await?;
        if !affected.is_empty() {
            profiles
                .invalidate_for_mutation(&txn, family.tree_id, &affected)
                .await?;
        }
        commit_tx(txn).await?;
        Ok(true)
    }

    // ── Event Mutations ──────────────────────────────────────────────

    /// Create a new event.
    async fn create_event(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        input: CreateEventInput,
    ) -> Result<GqlEvent> {
        let db = db_from_ctx(ctx);
        let profiles = profiles_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let id = Uuid::now_v7();
        let place_id = input.place_id.as_deref().map(Uuid::parse_str).transpose()?;
        let person_id = input
            .person_id
            .as_deref()
            .map(Uuid::parse_str)
            .transpose()?;
        let family_id = input
            .family_id
            .as_deref()
            .map(Uuid::parse_str)
            .transpose()?;
        let date_sort = input
            .date_sort
            .as_deref()
            .map(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d"))
            .transpose()
            .map_err(|e| async_graphql::Error::new(format!("Invalid date_sort: {e}")))?;
        let txn = begin_tx(db).await?;
        let event = EventRepo::create(
            &txn,
            id,
            tid,
            input.event_type.into(),
            input.date_value,
            date_sort,
            place_id,
            person_id,
            family_id,
            input.description,
        )
        .await?;
        // Invalidate: person event or family event.
        if let Some(pid) = person_id {
            let affected = invalidation::affected_persons(&txn, pid).await?;
            profiles
                .invalidate_for_mutation(&txn, tid, &affected)
                .await?;
        } else if let Some(fid) = family_id {
            let affected = invalidation::affected_persons_for_family(&txn, fid).await?;
            profiles
                .invalidate_for_mutation(&txn, tid, &affected)
                .await?;
        }
        commit_tx(txn).await?;
        Ok(event.into())
    }

    /// Update an event.
    async fn update_event(
        &self,
        ctx: &Context<'_>,
        id: ID,
        input: UpdateEventInput,
    ) -> Result<GqlEvent> {
        let db = db_from_ctx(ctx);
        let profiles = profiles_from_ctx(ctx);
        let uuid = Uuid::parse_str(id.as_str())?;
        let place_id = input.place_id.as_deref().map(Uuid::parse_str).transpose()?;
        let date_sort = input
            .date_sort
            .as_deref()
            .map(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d"))
            .transpose()
            .map_err(|e| async_graphql::Error::new(format!("Invalid date_sort: {e}")))?;
        let txn = begin_tx(db).await?;
        let event = EventRepo::update(
            &txn,
            uuid,
            input.event_type.map(|et| et.into()),
            input.date_value.map(Some),
            date_sort.map(Some),
            place_id.map(Some),
            input.description.map(Some),
            None,
            None,
            None,
            None,
        )
        .await?;
        // Invalidate based on event ownership.
        if let Some(pid) = event.person_id {
            let affected = invalidation::affected_persons(&txn, pid).await?;
            profiles
                .invalidate_for_mutation(&txn, event.tree_id, &affected)
                .await?;
        } else if let Some(fid) = event.family_id {
            let affected = invalidation::affected_persons_for_family(&txn, fid).await?;
            profiles
                .invalidate_for_mutation(&txn, event.tree_id, &affected)
                .await?;
        }
        commit_tx(txn).await?;
        Ok(event.into())
    }

    /// Delete an event (soft delete).
    async fn delete_event(&self, ctx: &Context<'_>, id: ID) -> Result<bool> {
        let db = db_from_ctx(ctx);
        let profiles = profiles_from_ctx(ctx);
        let uuid = Uuid::parse_str(id.as_str())?;
        let txn = begin_tx(db).await?;
        let event = EventRepo::get(&txn, uuid).await?;
        EventRepo::delete(&txn, uuid).await?;
        if let Some(pid) = event.person_id {
            let affected = invalidation::affected_persons(&txn, pid).await?;
            profiles
                .invalidate_for_mutation(&txn, event.tree_id, &affected)
                .await?;
        } else if let Some(fid) = event.family_id {
            let affected = invalidation::affected_persons_for_family(&txn, fid).await?;
            profiles
                .invalidate_for_mutation(&txn, event.tree_id, &affected)
                .await?;
        }
        commit_tx(txn).await?;
        Ok(true)
    }

    /// Add a witness to an event.
    async fn add_event_witness(
        &self,
        ctx: &Context<'_>,
        event_id: ID,
        input: AddEventWitnessInput,
    ) -> Result<GqlEventWitness> {
        let db = db_from_ctx(ctx);
        let eid = Uuid::parse_str(event_id.as_str())?;
        let pid = Uuid::parse_str(&input.person_id)?;
        let id = Uuid::now_v7();
        let witness =
            EventWitnessRepo::create(db, id, eid, pid, input.relation, input.sort_order).await?;
        Ok(witness.into())
    }

    /// Remove a witness from an event (hard delete).
    async fn remove_event_witness(&self, ctx: &Context<'_>, id: ID) -> Result<bool> {
        let db = db_from_ctx(ctx);
        let uuid = Uuid::parse_str(id.as_str())?;
        EventWitnessRepo::delete(db, uuid).await?;
        Ok(true)
    }

    // ── Place Mutations ──────────────────────────────────────────────

    /// Create a new place.
    async fn create_place(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        input: CreatePlaceInput,
    ) -> Result<GqlPlace> {
        let db = db_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let id = Uuid::now_v7();
        let place =
            PlaceRepo::create(db, id, tid, input.name, input.latitude, input.longitude).await?;
        Ok(place.into())
    }

    /// Update a place.
    async fn update_place(
        &self,
        ctx: &Context<'_>,
        id: ID,
        input: UpdatePlaceInput,
    ) -> Result<GqlPlace> {
        let db = db_from_ctx(ctx);
        let uuid = Uuid::parse_str(id.as_str())?;
        let place = PlaceRepo::update(
            db,
            uuid,
            input.name,
            input.latitude.map(Some),
            input.longitude.map(Some),
        )
        .await?;
        // Place changes could affect event display — but a person projection
        // stores the place *name* snapshot. For now, place edits don't refresh
        // the projections that embed the old name; a full rebuild is needed
        // after a place rename.
        Ok(place.into())
    }

    /// Delete a place (hard delete).
    async fn delete_place(&self, ctx: &Context<'_>, id: ID) -> Result<bool> {
        let db = db_from_ctx(ctx);
        let uuid = Uuid::parse_str(id.as_str())?;
        PlaceRepo::delete(db, uuid).await?;
        Ok(true)
    }

    // ── Source Mutations ─────────────────────────────────────────────

    /// Create a new source.
    async fn create_source(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        input: CreateSourceInput,
    ) -> Result<GqlSource> {
        let db = db_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let id = Uuid::now_v7();
        let source = SourceRepo::create(
            db,
            id,
            tid,
            input.title,
            input.author,
            input.publisher,
            input.abbreviation,
            input.repository_name,
        )
        .await?;
        Ok(source.into())
    }

    /// Update a source.
    async fn update_source(
        &self,
        ctx: &Context<'_>,
        id: ID,
        input: UpdateSourceInput,
    ) -> Result<GqlSource> {
        let db = db_from_ctx(ctx);
        let uuid = Uuid::parse_str(id.as_str())?;
        let source = SourceRepo::update(
            db,
            uuid,
            input.title,
            input.author.map(Some),
            input.publisher.map(Some),
            input.abbreviation.map(Some),
            input.repository_name.map(Some),
        )
        .await?;
        Ok(source.into())
    }

    /// Delete a source (soft delete).
    async fn delete_source(&self, ctx: &Context<'_>, id: ID) -> Result<bool> {
        let db = db_from_ctx(ctx);
        let uuid = Uuid::parse_str(id.as_str())?;
        SourceRepo::delete(db, uuid).await?;
        Ok(true)
    }

    // ── Citation Mutations ───────────────────────────────────────────

    /// Create a new citation.
    async fn create_citation(
        &self,
        ctx: &Context<'_>,
        input: CreateCitationInput,
    ) -> Result<GqlCitation> {
        let db = db_from_ctx(ctx);
        let id = Uuid::now_v7();
        let source_id = Uuid::parse_str(&input.source_id)?;
        let person_id = input
            .person_id
            .as_deref()
            .map(Uuid::parse_str)
            .transpose()?;
        let event_id = input.event_id.as_deref().map(Uuid::parse_str).transpose()?;
        let family_id = input
            .family_id
            .as_deref()
            .map(Uuid::parse_str)
            .transpose()?;
        let citation = CitationRepo::create(
            db,
            id,
            source_id,
            person_id,
            event_id,
            family_id,
            input.page,
            input.confidence.into(),
            input.text,
        )
        .await?;
        Ok(citation.into())
    }

    /// Update a citation.
    async fn update_citation(
        &self,
        ctx: &Context<'_>,
        id: ID,
        input: UpdateCitationInput,
    ) -> Result<GqlCitation> {
        let db = db_from_ctx(ctx);
        let uuid = Uuid::parse_str(id.as_str())?;
        let citation = CitationRepo::update(
            db,
            uuid,
            input.page.map(Some),
            input.confidence.map(|c| c.into()),
            input.text.map(Some),
        )
        .await?;
        Ok(citation.into())
    }

    /// Delete a citation (hard delete).
    async fn delete_citation(&self, ctx: &Context<'_>, id: ID) -> Result<bool> {
        let db = db_from_ctx(ctx);
        let uuid = Uuid::parse_str(id.as_str())?;
        CitationRepo::delete(db, uuid).await?;
        Ok(true)
    }

    // ── Media Mutations ──────────────────────────────────────────────

    /// Upload media metadata (no actual file upload in MVP).
    async fn upload_media(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        input: UploadMediaInput,
    ) -> Result<GqlMedia> {
        let db = db_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let id = Uuid::now_v7();
        let media = MediaRepo::create(
            db,
            id,
            tid,
            input.file_name,
            input.mime_type,
            input.file_path,
            input.file_size,
            input.title,
            input.description,
        )
        .await?;
        Ok(media.into())
    }

    /// Update media metadata.
    async fn update_media(
        &self,
        ctx: &Context<'_>,
        id: ID,
        input: UpdateMediaInput,
    ) -> Result<GqlMedia> {
        let db = db_from_ctx(ctx);
        let uuid = Uuid::parse_str(id.as_str())?;
        let media =
            MediaRepo::update(db, uuid, input.title.map(Some), input.description.map(Some)).await?;
        Ok(media.into())
    }

    /// Delete media (soft delete).
    async fn delete_media(&self, ctx: &Context<'_>, id: ID) -> Result<bool> {
        let db = db_from_ctx(ctx);
        let uuid = Uuid::parse_str(id.as_str())?;
        MediaRepo::delete(db, uuid).await?;
        Ok(true)
    }

    /// Create a media link.
    async fn create_media_link(
        &self,
        ctx: &Context<'_>,
        input: CreateMediaLinkInput,
    ) -> Result<GqlMediaLink> {
        let db = db_from_ctx(ctx);
        let id = Uuid::now_v7();
        let media_id = Uuid::parse_str(&input.media_id)?;
        let person_id = input
            .person_id
            .as_deref()
            .map(Uuid::parse_str)
            .transpose()?;
        let event_id = input.event_id.as_deref().map(Uuid::parse_str).transpose()?;
        let source_id = input
            .source_id
            .as_deref()
            .map(Uuid::parse_str)
            .transpose()?;
        let family_id = input
            .family_id
            .as_deref()
            .map(Uuid::parse_str)
            .transpose()?;
        let link = MediaLinkRepo::create(
            db,
            id,
            media_id,
            person_id,
            event_id,
            source_id,
            family_id,
            input.sort_order,
        )
        .await?;
        Ok(link.into())
    }

    /// Delete a media link (hard delete).
    async fn delete_media_link(&self, ctx: &Context<'_>, id: ID) -> Result<bool> {
        let db = db_from_ctx(ctx);
        let uuid = Uuid::parse_str(id.as_str())?;
        MediaLinkRepo::delete(db, uuid).await?;
        Ok(true)
    }

    // ── Note Mutations ───────────────────────────────────────────────

    /// Create a new note.
    async fn create_note(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        input: CreateNoteInput,
    ) -> Result<GqlNote> {
        let db = db_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let id = Uuid::now_v7();
        let person_id = input
            .person_id
            .as_deref()
            .map(Uuid::parse_str)
            .transpose()?;
        let event_id = input.event_id.as_deref().map(Uuid::parse_str).transpose()?;
        let family_id = input
            .family_id
            .as_deref()
            .map(Uuid::parse_str)
            .transpose()?;
        let source_id = input
            .source_id
            .as_deref()
            .map(Uuid::parse_str)
            .transpose()?;
        let note = NoteRepo::create(
            db, id, tid, input.text, person_id, event_id, family_id, source_id,
        )
        .await?;
        Ok(note.into())
    }

    /// Update a note.
    async fn update_note(
        &self,
        ctx: &Context<'_>,
        id: ID,
        input: UpdateNoteInput,
    ) -> Result<GqlNote> {
        let db = db_from_ctx(ctx);
        let uuid = Uuid::parse_str(id.as_str())?;
        let note = NoteRepo::update(db, uuid, input.text).await?;
        Ok(note.into())
    }

    /// Delete a note (soft delete).
    async fn delete_note(&self, ctx: &Context<'_>, id: ID) -> Result<bool> {
        let db = db_from_ctx(ctx);
        let uuid = Uuid::parse_str(id.as_str())?;
        NoteRepo::delete(db, uuid).await?;
        Ok(true)
    }

    // ── Import Mutations ──────────────────────────────────────────────

    /// Import a GEDCOM string into a tree, persisting all extracted entities.
    /// Triggers a full projection rebuild after import.
    async fn import_gedcom(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        input: ImportGedcomInput,
    ) -> Result<GqlImportResult> {
        let db = db_from_ctx(ctx);
        let profiles = profiles_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let summary = crate::service::gedcom::import_and_persist(db, tid, &input.gedcom).await?;
        // Eager full rebuild after GEDCOM import — deliberately outside a
        // transaction: it is an idempotent bulk operation over the whole tree,
        // and wrapping 100K rows would hold a very long-lived write lock. The
        // import itself is already atomic (see `gedcom::import_and_persist`).
        profiles.rebuild_tree_full(db, tid).await?;
        Ok(import_result(summary))
    }

    /// Import a GeneWeb `.gw` file into a tree, persisting all extracted
    /// entities. Triggers a full projection rebuild after import.
    ///
    /// The file content is base64-encoded because `.gw` is ISO-8859-1 unless
    /// the file opts into UTF-8 — see [`ImportGenewebInput`]. There is no
    /// matching export: `.gw` is a read-only format in OxidGene.
    async fn import_geneweb(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        input: ImportGenewebInput,
    ) -> Result<GqlImportResult> {
        use base64::Engine as _;

        let db = db_from_ctx(ctx);
        let profiles = profiles_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&input.content_base64)
            .map_err(|e| async_graphql::Error::new(format!("contentBase64 is not base64: {e}")))?;
        let filename = input.filename.as_deref().unwrap_or("import.gw");
        let summary =
            crate::service::geneweb::import_and_persist(db, tid, &bytes, filename).await?;
        // Same rationale as `import_gedcom` above.
        profiles.rebuild_tree_full(db, tid).await?;
        Ok(import_result(summary))
    }

    // ── Projection Admin Mutations ───────────────────────────────────

    /// Rebuild every projection of a tree (all persons + search index).
    async fn rebuild_tree_profiles(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
    ) -> Result<GqlProfileRebuildResult> {
        let db = db_from_ctx(ctx);
        let profiles = profiles_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let count = profiles.rebuild_tree_full(db, tid).await?;
        Ok(GqlProfileRebuildResult {
            rebuilt: true,
            persons_count: count as i32,
        })
    }

    /// Rebuild the projection of a single person.
    async fn rebuild_person_profile(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        person_id: ID,
    ) -> Result<GqlProfileRebuildResult> {
        let db = db_from_ctx(ctx);
        let profiles = profiles_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let pid = Uuid::parse_str(person_id.as_str())?;
        let txn = begin_tx(db).await?;
        profiles.rebuild_person(&txn, tid, pid).await?;
        commit_tx(txn).await?;
        Ok(GqlProfileRebuildResult {
            rebuilt: true,
            persons_count: 1,
        })
    }

    /// Drop every projection of a tree. For debugging or after bulk operations.
    async fn drop_tree_profiles(&self, ctx: &Context<'_>, tree_id: ID) -> Result<bool> {
        let db = db_from_ctx(ctx);
        let profiles = profiles_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let txn = begin_tx(db).await?;
        profiles.invalidate_tree(&txn, tid).await?;
        commit_tx(txn).await?;
        Ok(true)
    }

    /// Expand a pedigree in one direction, returning only the new nodes and
    /// edges (delta). The client merges the delta into its current view.
    ///
    /// `otherDepth` is the depth already loaded in the opposite direction —
    /// pass it so the returned `*DepthLoaded` values match what you hold.
    #[allow(clippy::too_many_arguments)]
    async fn expand_pedigree(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        root_person_id: ID,
        direction: GqlPedigreeDirection,
        from_depth: i32,
        to_depth: i32,
        #[graphql(default = 0)] other_depth: i32,
    ) -> Result<GqlPedigreeDelta> {
        let profiles = profiles_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let rid = Uuid::parse_str(root_person_id.as_str())?;

        if to_depth <= from_depth {
            return Err(async_graphql::Error::new(format!(
                "toDepth ({to_depth}) must be greater than fromDepth ({from_depth})"
            )));
        }

        let dir: oxidgene_core::projection::PedigreeDirection = direction.into();
        let delta = profiles
            .expand_pedigree(
                tid,
                rid,
                dir,
                from_depth.max(0) as u32,
                to_depth.max(0) as u32,
                other_depth.max(0) as u32,
            )
            .await?;
        Ok(delta.into())
    }
}
