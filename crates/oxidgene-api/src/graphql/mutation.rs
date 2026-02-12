//! GraphQL mutation root with all write operations.

use async_graphql::{Context, ID, Object, Result};
use chrono::NaiveDate;
use uuid::Uuid;

use oxidgene_db::repo::{
    CitationRepo, EventRepo, FamilyChildRepo, FamilyRepo, FamilySpouseRepo, MediaLinkRepo,
    MediaRepo, NoteRepo, PersonNameRepo, PersonRepo, PlaceRepo, SourceRepo, TreeRepo,
};

use super::inputs::{
    AddChildInput, AddSpouseInput, CreateCitationInput, CreateEventInput, CreateMediaLinkInput,
    CreateNoteInput, CreatePersonInput, CreatePlaceInput, CreateSourceInput, CreateTreeInput,
    ImportGedcomInput, PersonNameInput, UpdateCitationInput, UpdateEventInput, UpdateMediaInput,
    UpdateNoteInput, UpdatePersonInput, UpdatePersonNameInput, UpdatePlaceInput, UpdateSourceInput,
    UpdateTreeInput, UploadMediaInput,
};
use super::types::{
    GqlCitation, GqlEvent, GqlFamily, GqlFamilyChild, GqlFamilySpouse, GqlImportGedcomResult,
    GqlMedia, GqlMediaLink, GqlNote, GqlPerson, GqlPersonName, GqlPlace, GqlSource, GqlTree,
    db_from_ctx,
};

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
        let tree = TreeRepo::update(db, uuid, input.name, input.description.map(Some)).await?;
        Ok(tree.into())
    }

    /// Delete a tree (soft delete).
    async fn delete_tree(&self, ctx: &Context<'_>, id: ID) -> Result<bool> {
        let db = db_from_ctx(ctx);
        let uuid = Uuid::parse_str(id.as_str())?;
        TreeRepo::delete(db, uuid).await?;
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
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let id = Uuid::now_v7();
        let person = PersonRepo::create(db, id, tid, input.sex.into()).await?;
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
        let uuid = Uuid::parse_str(id.as_str())?;
        let person = PersonRepo::update(db, uuid, input.sex.map(|s| s.into())).await?;
        Ok(person.into())
    }

    /// Delete a person (soft delete).
    async fn delete_person(&self, ctx: &Context<'_>, id: ID) -> Result<bool> {
        let db = db_from_ctx(ctx);
        let uuid = Uuid::parse_str(id.as_str())?;
        PersonRepo::delete(db, uuid).await?;
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
        let pid = Uuid::parse_str(person_id.as_str())?;
        let id = Uuid::now_v7();
        let name = PersonNameRepo::create(
            db,
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
        let uuid = Uuid::parse_str(id.as_str())?;
        let name = PersonNameRepo::update(
            db,
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
        Ok(name.into())
    }

    /// Delete a person name (hard delete).
    async fn delete_person_name(&self, ctx: &Context<'_>, id: ID) -> Result<bool> {
        let db = db_from_ctx(ctx);
        let uuid = Uuid::parse_str(id.as_str())?;
        PersonNameRepo::delete(db, uuid).await?;
        Ok(true)
    }

    // ── Family Mutations ─────────────────────────────────────────────

    /// Create a new family in a tree.
    async fn create_family(&self, ctx: &Context<'_>, tree_id: ID) -> Result<GqlFamily> {
        let db = db_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let id = Uuid::now_v7();
        let family = FamilyRepo::create(db, id, tid).await?;
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
        let uuid = Uuid::parse_str(id.as_str())?;
        FamilyRepo::delete(db, uuid).await?;
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
        let fid = Uuid::parse_str(family_id.as_str())?;
        let pid = Uuid::parse_str(&input.person_id)?;
        let id = Uuid::now_v7();
        let spouse =
            FamilySpouseRepo::create(db, id, fid, pid, input.role.into(), input.sort_order).await?;
        Ok(spouse.into())
    }

    /// Remove a spouse from a family (hard delete).
    async fn remove_spouse(&self, ctx: &Context<'_>, id: ID) -> Result<bool> {
        let db = db_from_ctx(ctx);
        let uuid = Uuid::parse_str(id.as_str())?;
        FamilySpouseRepo::delete(db, uuid).await?;
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
        let fid = Uuid::parse_str(family_id.as_str())?;
        let pid = Uuid::parse_str(&input.person_id)?;
        let id = Uuid::now_v7();
        let child =
            FamilyChildRepo::create(db, id, fid, pid, input.child_type.into(), input.sort_order)
                .await?;
        Ok(child.into())
    }

    /// Remove a child from a family (hard delete).
    async fn remove_child(&self, ctx: &Context<'_>, id: ID) -> Result<bool> {
        let db = db_from_ctx(ctx);
        let uuid = Uuid::parse_str(id.as_str())?;
        FamilyChildRepo::delete(db, uuid).await?;
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
        let event = EventRepo::create(
            db,
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
        let uuid = Uuid::parse_str(id.as_str())?;
        let place_id = input.place_id.as_deref().map(Uuid::parse_str).transpose()?;
        let date_sort = input
            .date_sort
            .as_deref()
            .map(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d"))
            .transpose()
            .map_err(|e| async_graphql::Error::new(format!("Invalid date_sort: {e}")))?;
        let event = EventRepo::update(
            db,
            uuid,
            input.event_type.map(|et| et.into()),
            input.date_value.map(Some),
            date_sort.map(Some),
            place_id.map(Some),
            input.description.map(Some),
        )
        .await?;
        Ok(event.into())
    }

    /// Delete an event (soft delete).
    async fn delete_event(&self, ctx: &Context<'_>, id: ID) -> Result<bool> {
        let db = db_from_ctx(ctx);
        let uuid = Uuid::parse_str(id.as_str())?;
        EventRepo::delete(db, uuid).await?;
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

    // ── GEDCOM Mutations ──────────────────────────────────────────────

    /// Import a GEDCOM string into a tree, persisting all extracted entities.
    async fn import_gedcom(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        input: ImportGedcomInput,
    ) -> Result<GqlImportGedcomResult> {
        let db = db_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let summary = crate::service::gedcom::import_and_persist(db, tid, &input.gedcom).await?;
        Ok(GqlImportGedcomResult {
            persons_count: summary.persons_count as i32,
            families_count: summary.families_count as i32,
            events_count: summary.events_count as i32,
            sources_count: summary.sources_count as i32,
            media_count: summary.media_count as i32,
            places_count: summary.places_count as i32,
            notes_count: summary.notes_count as i32,
            warnings: summary.warnings,
        })
    }
}
