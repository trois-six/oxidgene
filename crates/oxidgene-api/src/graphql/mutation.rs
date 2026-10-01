//! GraphQL mutation root with all write operations.
//!
//! Every resolver is a thin adapter over a `service` function. Those whose
//! futures are large are awaited through `Box::pin`: async-graphql resolves
//! a field inside one future sized for the largest resolver, and with the
//! media writes inlined it outgrew a test thread's 2 MiB stack.

use crate::service::citation::{self, CitationPatch, NewCitation};
use crate::service::history::{self};
use crate::service::media::{NewUpload, UploadTarget};
use crate::service::note::{self, NewNote, NotePatch};
use crate::service::pedigrees::Expansion;
use crate::service::scope::{begin_tx, commit_tx};
use crate::service::{
    duplicates, event, family, family_names, media, media_link, person, person_name, place, source,
    tree, vignette,
};
use async_graphql::{Context, ID, MaybeUndefined, Object, Result};
use base64::Engine as _;
use uuid::Uuid;

use super::history::{GqlAuditEntry, GqlRecordType};
use super::inputs::{
    AddChildInput, AddEventWitnessInput, AddSpouseInput, CreateCitationInput, CreateEventInput,
    CreateMediaLinkInput, CreateNoteInput, CreatePersonInput, CreatePlaceInput, CreateSourceInput,
    CreateTreeInput, CreateVignetteInput, GeneanetImportInput, GeneanetSessionEncodeInput,
    MergeChoicesInput, PersonNameInput, RenameFamilyNameInput, SetFamilyNameParticleInput,
    UpdateCitationInput, UpdateEventInput, UpdateFamilyInput, UpdateMediaInput, UpdateNoteInput,
    UpdatePersonInput, UpdatePersonNameInput, UpdatePlaceInput, UpdateSourceInput, UpdateTreeInput,
    UpdateVignetteInput, UploadMediaFileInput, UploadMediaInput, geneanet_deposit_sizes,
    geneanet_media_paths,
};
use super::scope::{live_tree, opt_uuid, uuid, uuids};
use super::types::{
    GqlBackgroundJobStarted, GqlCitation, GqlEvent, GqlEventWitness, GqlFamily, GqlFamilyChild,
    GqlFamilyNameParticleUpdate, GqlFamilyNameRename, GqlFamilySpouse, GqlGeneanetDepositSize,
    GqlGeneanetMediaPath, GqlGeneanetSession, GqlGeneanetSessionArchive, GqlMedia, GqlMediaLink,
    GqlNote, GqlPedigreeDelta, GqlPedigreeDirection, GqlPerson, GqlPersonName, GqlPlace,
    GqlProfileRebuildResult, GqlSource, GqlTree, GqlVignette, db_from_ctx, media_from_ctx,
    profiles_from_ctx, purge_from_ctx, require_local_file_access,
};

/// Maps a GraphQL nullable update field onto the repositories' patch shape.
///
/// `None` leaves the column alone, `Some(None)` clears it, `Some(Some(v))` sets
/// it. Only [`MaybeUndefined`] can express the first two distinctly — a plain
/// `Option<T>` collapses an omitted field and an explicit `null` into the same
/// `None`, which is why nullable fields could previously be set but never
/// cleared over GraphQL. Mirrors `double_option` on the REST side.
pub(crate) fn patch<T>(value: MaybeUndefined<T>) -> Option<Option<T>> {
    match value {
        MaybeUndefined::Undefined => None,
        MaybeUndefined::Null => Some(None),
        MaybeUndefined::Value(v) => Some(Some(v)),
    }
}

/// [`patch`], for an identifier. A `null` clears without parsing anything;
/// only a real value can be malformed.
pub(crate) fn patch_id(value: MaybeUndefined<String>) -> Result<Option<Option<Uuid>>> {
    match value {
        MaybeUndefined::Undefined => Ok(None),
        MaybeUndefined::Null => Ok(Some(None)),
        MaybeUndefined::Value(v) => uuid(v).map(|id| Some(Some(id))),
    }
}

/// Maps a non-nullable update field (one that can be set or left alone, but
/// never cleared) from `MaybeUndefined`. `Undefined` and `Null` both leave the
/// column untouched; only a real value updates it.
pub(crate) fn patch_scalar<T, U>(value: MaybeUndefined<T>) -> Option<U>
where
    U: From<T>,
{
    match value {
        MaybeUndefined::Value(v) => Some(v.into()),
        _ => None,
    }
}

/// The root mutation type.
pub struct MutationRoot;

#[Object]
impl MutationRoot {
    // ── Tree Mutations ───────────────────────────────────────────────

    /// Create a new tree. A blank name is refused.
    async fn create_tree(&self, ctx: &Context<'_>, input: CreateTreeInput) -> Result<GqlTree> {
        Ok(tree::create_tree(db_from_ctx(ctx), input.into())
            .await?
            .into())
    }

    /// Duplicate a tree through a lossless GEDCOM round trip.
    ///
    /// The duplication path deliberately never enables export compatibility
    /// options: those are for third-party interchange, not a copy inside
    /// OxidGene. Mirrors `POST /trees/:tree_id/duplicate`.
    async fn duplicate_tree(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        name: String,
    ) -> Result<GqlTree> {
        let source_tree_id = live_tree(ctx, &tree_id).await?;
        let tree = tree::duplicate_tree(
            db_from_ctx(ctx),
            profiles_from_ctx(ctx),
            source_tree_id,
            name,
        )
        .await?;
        Ok(tree.into())
    }

    /// Update an existing tree. A blank name is refused, and a SOSA root or
    /// own record from another tree is not found.
    async fn update_tree(
        &self,
        ctx: &Context<'_>,
        id: ID,
        input: UpdateTreeInput,
    ) -> Result<GqlTree> {
        let id = live_tree(ctx, &id).await?;
        let tree = tree::update_tree(db_from_ctx(ctx), id, input.try_into()?).await?;
        Ok(tree.into())
    }

    /// Delete a tree.
    ///
    /// Flags it as deleted and returns straight away; the rows it owns and its
    /// projections are removed by the background purge worker. See
    /// [`crate::service::purge`].
    async fn delete_tree(&self, ctx: &Context<'_>, id: ID) -> Result<bool> {
        let id = uuid(&id)?;
        tree::delete_tree(db_from_ctx(ctx), purge_from_ctx(ctx), id).await?;
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
        let tree_id = live_tree(ctx, &tree_id).await?;
        let person = person::create_person(
            db_from_ctx(ctx),
            profiles_from_ctx(ctx),
            tree_id,
            input.into(),
        )
        .await?;
        Ok(person.into())
    }

    /// Update a person.
    async fn update_person(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        id: ID,
        input: UpdatePersonInput,
    ) -> Result<GqlPerson> {
        let tree_id = live_tree(ctx, &tree_id).await?;
        let person = person::update_person(
            db_from_ctx(ctx),
            profiles_from_ctx(ctx),
            tree_id,
            uuid(&id)?,
            input.into(),
        )
        .await?;
        Ok(person.into())
    }

    /// Delete a person (soft delete).
    async fn delete_person(&self, ctx: &Context<'_>, tree_id: ID, id: ID) -> Result<bool> {
        let tree_id = live_tree(ctx, &tree_id).await?;
        person::delete_person(
            db_from_ctx(ctx),
            profiles_from_ctx(ctx),
            tree_id,
            uuid(&id)?,
        )
        .await?;
        Ok(true)
    }

    /// Record that a person differs from each of `otherPersonIds`, so those
    /// pairs stop being offered as homonyms.
    async fn mark_persons_distinct(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        person_id: ID,
        other_person_ids: Vec<ID>,
    ) -> Result<bool> {
        let db = db_from_ctx(ctx);
        let tid = live_tree(ctx, &tree_id).await?;
        let pid = uuid(&person_id)?;
        let others = uuids(&other_person_ids)?;
        let txn = begin_tx(db).await?;
        duplicates::mark_distinct(&txn, tid, pid, &others).await?;
        commit_tx(txn).await?;
        Ok(true)
    }

    /// Merge `duplicateId` into `personId`, which is kept; the duplicate is
    /// soft-deleted. `choices` carries what the comparison chose. Returns the
    /// kept person.
    async fn merge_persons(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        person_id: ID,
        duplicate_id: ID,
        #[graphql(default)] choices: MergeChoicesInput,
    ) -> Result<GqlPerson> {
        let db = db_from_ctx(ctx);
        let profiles = profiles_from_ctx(ctx);
        let tid = live_tree(ctx, &tree_id).await?;
        let kept = uuid(&person_id)?;
        let duplicate = uuid(&duplicate_id)?;
        let choices = choices.into_choices()?;
        let txn = begin_tx(db).await?;
        let person =
            duplicates::merge_persons(&txn, profiles, tid, kept, duplicate, &choices).await?;
        commit_tx(txn).await?;
        Ok(person.into())
    }

    // ── PersonName Mutations ─────────────────────────────────────────

    /// Add a name to a person.
    async fn add_person_name(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        person_id: ID,
        input: PersonNameInput,
    ) -> Result<GqlPersonName> {
        let tree_id = live_tree(ctx, &tree_id).await?;
        let name = person_name::create_person_name(
            db_from_ctx(ctx),
            profiles_from_ctx(ctx),
            tree_id,
            uuid(&person_id)?,
            input.into(),
        )
        .await?;
        Ok(name.into())
    }

    /// Update a name of a person; a name of somebody else is not found.
    async fn update_person_name(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        person_id: ID,
        id: ID,
        input: UpdatePersonNameInput,
    ) -> Result<GqlPersonName> {
        let tree_id = live_tree(ctx, &tree_id).await?;
        let name = person_name::update_person_name(
            db_from_ctx(ctx),
            profiles_from_ctx(ctx),
            tree_id,
            uuid(&person_id)?,
            uuid(&id)?,
            input.into(),
        )
        .await?;
        Ok(name.into())
    }

    /// Delete a name of a person (hard delete); a name of somebody else is
    /// not found.
    async fn delete_person_name(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        person_id: ID,
        id: ID,
    ) -> Result<bool> {
        let tree_id = live_tree(ctx, &tree_id).await?;
        person_name::delete_person_name(
            db_from_ctx(ctx),
            profiles_from_ctx(ctx),
            tree_id,
            uuid(&person_id)?,
            uuid(&id)?,
        )
        .await?;
        Ok(true)
    }

    // ── Family Mutations ─────────────────────────────────────────────

    /// Create a new family in a tree.
    async fn create_family(&self, ctx: &Context<'_>, tree_id: ID) -> Result<GqlFamily> {
        let tree_id = live_tree(ctx, &tree_id).await?;
        Ok(family::create_family(db_from_ctx(ctx), tree_id)
            .await?
            .into())
    }

    /// Update a family: its privacy, and `updatedAt` either way.
    async fn update_family(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        id: ID,
        input: UpdateFamilyInput,
    ) -> Result<GqlFamily> {
        let tree_id = live_tree(ctx, &tree_id).await?;
        let family =
            family::update_family(db_from_ctx(ctx), tree_id, uuid(&id)?, input.into()).await?;
        Ok(family.into())
    }

    /// Delete a family (soft delete).
    async fn delete_family(&self, ctx: &Context<'_>, tree_id: ID, id: ID) -> Result<bool> {
        let tree_id = live_tree(ctx, &tree_id).await?;
        family::delete_family(
            db_from_ctx(ctx),
            profiles_from_ctx(ctx),
            tree_id,
            uuid(&id)?,
        )
        .await?;
        Ok(true)
    }

    /// Add a spouse to a family.
    async fn add_spouse(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        family_id: ID,
        input: AddSpouseInput,
    ) -> Result<GqlFamilySpouse> {
        let tree_id = live_tree(ctx, &tree_id).await?;
        let spouse = family::add_spouse(
            db_from_ctx(ctx),
            profiles_from_ctx(ctx),
            tree_id,
            uuid(&family_id)?,
            input.try_into()?,
        )
        .await?;
        Ok(spouse.into())
    }

    /// Remove a spouse link from a family (hard delete); a link of another
    /// family is not found.
    async fn remove_spouse(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        family_id: ID,
        id: ID,
    ) -> Result<bool> {
        let tree_id = live_tree(ctx, &tree_id).await?;
        family::remove_spouse(
            db_from_ctx(ctx),
            profiles_from_ctx(ctx),
            tree_id,
            uuid(&family_id)?,
            uuid(&id)?,
        )
        .await?;
        Ok(true)
    }

    /// Add a child to a family.
    async fn add_child(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        family_id: ID,
        input: AddChildInput,
    ) -> Result<GqlFamilyChild> {
        let tree_id = live_tree(ctx, &tree_id).await?;
        let child = family::add_child(
            db_from_ctx(ctx),
            profiles_from_ctx(ctx),
            tree_id,
            uuid(&family_id)?,
            input.try_into()?,
        )
        .await?;
        Ok(child.into())
    }

    /// Remove a child link from a family (hard delete); a link of another
    /// family is not found.
    async fn remove_child(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        family_id: ID,
        id: ID,
    ) -> Result<bool> {
        let tree_id = live_tree(ctx, &tree_id).await?;
        family::remove_child(
            db_from_ctx(ctx),
            profiles_from_ctx(ctx),
            tree_id,
            uuid(&family_id)?,
            uuid(&id)?,
        )
        .await?;
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
        let tree_id = live_tree(ctx, &tree_id).await?;
        let event = event::create_event(
            db_from_ctx(ctx),
            profiles_from_ctx(ctx),
            tree_id,
            input.try_into()?,
        )
        .await?;
        Ok(event.into())
    }

    /// Update an event. A place of another tree is not found.
    async fn update_event(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        id: ID,
        input: UpdateEventInput,
    ) -> Result<GqlEvent> {
        let tree_id = live_tree(ctx, &tree_id).await?;
        let event = event::update_event(
            db_from_ctx(ctx),
            profiles_from_ctx(ctx),
            tree_id,
            uuid(&id)?,
            input.try_into()?,
        )
        .await?;
        Ok(event.into())
    }

    /// Delete an event (soft delete).
    async fn delete_event(&self, ctx: &Context<'_>, tree_id: ID, id: ID) -> Result<bool> {
        let tree_id = live_tree(ctx, &tree_id).await?;
        event::delete_event(
            db_from_ctx(ctx),
            profiles_from_ctx(ctx),
            tree_id,
            uuid(&id)?,
        )
        .await?;
        Ok(true)
    }

    /// Add a witness to an event.
    async fn add_event_witness(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        event_id: ID,
        input: AddEventWitnessInput,
    ) -> Result<GqlEventWitness> {
        let tree_id = live_tree(ctx, &tree_id).await?;
        let witness = event::add_witness(
            db_from_ctx(ctx),
            tree_id,
            uuid(&event_id)?,
            input.try_into()?,
        )
        .await?;
        Ok(witness.into())
    }

    /// Remove a witness from an event (hard delete). With `eventId`, a
    /// witness of another event is not found.
    async fn remove_event_witness(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        id: ID,
        event_id: Option<ID>,
    ) -> Result<bool> {
        let tree_id = live_tree(ctx, &tree_id).await?;
        event::remove_witness(db_from_ctx(ctx), tree_id, opt_uuid(event_id)?, uuid(&id)?).await?;
        Ok(true)
    }

    // ── Place Mutations ──────────────────────────────────────────────

    /// Create a new place. A blank name is refused.
    async fn create_place(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        input: CreatePlaceInput,
    ) -> Result<GqlPlace> {
        let tree_id = live_tree(ctx, &tree_id).await?;
        Ok(place::create_place(db_from_ctx(ctx), tree_id, input.into())
            .await?
            .into())
    }

    /// Update a place. A blank name is refused.
    async fn update_place(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        id: ID,
        input: UpdatePlaceInput,
    ) -> Result<GqlPlace> {
        let tree_id = live_tree(ctx, &tree_id).await?;
        let place = place::update_place(
            db_from_ctx(ctx),
            profiles_from_ctx(ctx),
            tree_id,
            uuid(&id)?,
            input.into(),
        )
        .await?;
        Ok(place.into())
    }

    /// Delete a place (hard delete).
    async fn delete_place(&self, ctx: &Context<'_>, tree_id: ID, id: ID) -> Result<bool> {
        let tree_id = live_tree(ctx, &tree_id).await?;
        place::delete_place(
            db_from_ctx(ctx),
            profiles_from_ctx(ctx),
            tree_id,
            uuid(&id)?,
        )
        .await?;
        Ok(true)
    }

    // ── Source Mutations ─────────────────────────────────────────────

    /// Create a new source. A blank title is refused.
    async fn create_source(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        input: CreateSourceInput,
    ) -> Result<GqlSource> {
        let tree_id = live_tree(ctx, &tree_id).await?;
        Ok(
            source::create_source(db_from_ctx(ctx), tree_id, input.into())
                .await?
                .into(),
        )
    }

    /// Update a source. A blank title is refused.
    async fn update_source(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        id: ID,
        input: UpdateSourceInput,
    ) -> Result<GqlSource> {
        let tree_id = live_tree(ctx, &tree_id).await?;
        let source =
            source::update_source(db_from_ctx(ctx), tree_id, uuid(&id)?, input.into()).await?;
        Ok(source.into())
    }

    /// Delete a source (soft delete).
    /// With `onlyIfUnused`, the source is kept if any citation, note or media
    /// link still points at it; the return value says whether it was deleted.
    async fn delete_source(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        id: ID,
        #[graphql(default = false)] only_if_unused: bool,
    ) -> Result<bool> {
        let tree_id = live_tree(ctx, &tree_id).await?;
        Ok(source::delete_source(db_from_ctx(ctx), tree_id, uuid(&id)?, only_if_unused).await?)
    }

    // ── Citation Mutations ───────────────────────────────────────────

    /// Create a new citation.
    async fn create_citation(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        input: CreateCitationInput,
    ) -> Result<GqlCitation> {
        let tid = live_tree(ctx, &tree_id).await?;
        let new = NewCitation {
            source_id: uuid(&input.source_id)?,
            person_id: opt_uuid(input.person_id)?,
            event_id: opt_uuid(input.event_id)?,
            family_id: opt_uuid(input.family_id)?,
            page: input.page,
            confidence: input.confidence.into(),
            text: input.text,
        };
        let citation =
            citation::create_citation(db_from_ctx(ctx), profiles_from_ctx(ctx), tid, new).await?;
        Ok(citation.into())
    }

    /// Update a citation.
    async fn update_citation(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        id: ID,
        input: UpdateCitationInput,
    ) -> Result<GqlCitation> {
        let tid = live_tree(ctx, &tree_id).await?;
        let id = uuid(&id)?;
        let patch = CitationPatch {
            source_id: opt_uuid(input.source_id)?,
            page: patch(input.page),
            confidence: input.confidence.map(|c| c.into()),
            text: patch(input.text),
        };
        let citation =
            citation::update_citation(db_from_ctx(ctx), profiles_from_ctx(ctx), tid, id, patch)
                .await?;
        Ok(citation.into())
    }

    /// Delete a citation (hard delete).
    async fn delete_citation(&self, ctx: &Context<'_>, tree_id: ID, id: ID) -> Result<bool> {
        let tid = live_tree(ctx, &tree_id).await?;
        let id = uuid(&id)?;
        citation::delete_citation(db_from_ctx(ctx), profiles_from_ctx(ctx), tid, id).await?;
        Ok(true)
    }

    // ── Media Mutations ──────────────────────────────────────────────

    /// Add a page naming a file we do not hold — a URL, or a path a GEDCOM
    /// mentioned — to a document. Mirrors `POST /trees/{treeId}/media`.
    async fn upload_media(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        input: UploadMediaInput,
    ) -> Result<GqlMedia> {
        let tree_id = live_tree(ctx, &tree_id).await?;
        let page = Box::pin(media::create_page(
            db_from_ctx(ctx),
            profiles_from_ctx(ctx),
            tree_id,
            input.try_into()?,
        ))
        .await?;
        Ok(page.into())
    }

    /// Upload a file's bytes, base64-encoded.
    ///
    /// Creates a page of `documentId`, or fills in an existing page when
    /// `mediaId` is given. Mirrors `POST /trees/{treeId}/media/upload`; see
    /// [`UploadMediaFileInput`] for why the content is base64 rather than an
    /// `Upload` scalar.
    async fn upload_media_file(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        input: UploadMediaFileInput,
    ) -> Result<GqlMedia> {
        let tree_id = live_tree(ctx, &tree_id).await?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&input.content_base64)
            .map_err(|_| {
                oxidgene_core::OxidGeneError::Validation("contentBase64 is not base64".into())
            })?;
        let target = match (opt_uuid(input.media_id)?, opt_uuid(input.document_id)?) {
            (Some(media_id), _) => UploadTarget::Attach { media_id },
            (None, Some(document_id)) => UploadTarget::NewPage { document_id },
            (None, None) => {
                return Err(oxidgene_core::OxidGeneError::Validation(
                    "documentId is required".into(),
                )
                .into());
            }
        };
        let (page, _) = Box::pin(media::upload(
            db_from_ctx(ctx),
            profiles_from_ctx(ctx),
            &**media_from_ctx(ctx),
            tree_id,
            NewUpload {
                file_name: input.file_name,
                bytes,
                title: input.title,
                description: input.description,
                target,
            },
        ))
        .await?;
        Ok(page.into())
    }

    /// Update media metadata.
    async fn update_media(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        id: ID,
        input: UpdateMediaInput,
    ) -> Result<GqlMedia> {
        let tree_id = live_tree(ctx, &tree_id).await?;
        let media = Box::pin(media::update_media(
            db_from_ctx(ctx),
            profiles_from_ctx(ctx),
            tree_id,
            uuid(&id)?,
            input.try_into()?,
        ))
        .await?;
        Ok(media.into())
    }

    /// Atomically add a tag without replacing the media's other tags.
    async fn add_media_tag(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        id: ID,
        tag: String,
    ) -> Result<GqlMedia> {
        let tree_id = live_tree(ctx, &tree_id).await?;
        Ok(
            Box::pin(media::add_tag(db_from_ctx(ctx), tree_id, uuid(&id)?, &tag))
                .await?
                .into(),
        )
    }

    /// Atomically remove one tag without replacing the media's other tags.
    async fn remove_media_tag(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        id: ID,
        tag: String,
    ) -> Result<bool> {
        let tree_id = live_tree(ctx, &tree_id).await?;
        Box::pin(media::remove_tag(
            db_from_ctx(ctx),
            tree_id,
            uuid(&id)?,
            &tag,
        ))
        .await?;
        Ok(true)
    }

    /// Permanently delete media and its associated data.
    ///
    /// With `onlyIfUnreferencedElsewhere`, the supplied gallery link is
    /// ignored while checking references; `false` means another reference
    /// retained the media. This is the GraphQL mirror of REST's
    /// `only_if_unreferenced_elsewhere` query parameter.
    async fn delete_media(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        id: ID,
        #[graphql(default = false)] only_if_unreferenced_elsewhere: bool,
        allowed_link_id: Option<ID>,
    ) -> Result<bool> {
        let tree_id = live_tree(ctx, &tree_id).await?;
        let allowed_link_id = match (only_if_unreferenced_elsewhere, opt_uuid(allowed_link_id)?) {
            (false, _) => None,
            (true, Some(link_id)) => Some(link_id),
            (true, None) => {
                return Err(oxidgene_core::OxidGeneError::Validation(
                    "allowedLinkId is required for conditional media deletion".into(),
                )
                .into());
            }
        };
        Ok(Box::pin(media::delete_media(
            db_from_ctx(ctx),
            profiles_from_ctx(ctx),
            &**media_from_ctx(ctx),
            tree_id,
            uuid(&id)?,
            allowed_link_id,
        ))
        .await?)
    }

    /// Choose what represents a person: a whole media, a region of one, or
    /// nothing.
    ///
    /// One write on the person. Passing neither id clears the portrait;
    /// passing both is refused, since that is not a state the model holds.
    async fn set_person_portrait(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        person_id: ID,
        media_id: Option<ID>,
        vignette_id: Option<ID>,
    ) -> Result<GqlPerson> {
        let choice = crate::service::portrait::PortraitChoice {
            media_id: opt_uuid(media_id)?,
            vignette_id: opt_uuid(vignette_id)?,
        };
        let person = crate::service::portrait::set_person_portrait(
            db_from_ctx(ctx),
            profiles_from_ctx(ctx),
            live_tree(ctx, &tree_id).await?,
            uuid(&person_id)?,
            choice,
        )
        .await?;
        Ok(person.into())
    }

    /// Create an empty multi-page document.
    async fn create_media_document(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        title: Option<String>,
    ) -> Result<GqlMedia> {
        let tree_id = live_tree(ctx, &tree_id).await?;
        Ok(
            Box::pin(media::create_document(db_from_ctx(ctx), tree_id, title))
                .await?
                .into(),
        )
    }

    /// Set a document's page order. The list must name exactly its pages.
    async fn reorder_media_pages(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        document_id: ID,
        page_ids: Vec<ID>,
    ) -> Result<Vec<GqlMedia>> {
        let tree_id = live_tree(ctx, &tree_id).await?;
        let pages = Box::pin(media::reorder_pages(
            db_from_ctx(ctx),
            profiles_from_ctx(ctx),
            tree_id,
            uuid(&document_id)?,
            &uuids(&page_ids)?,
        ))
        .await?;
        Ok(pages.into_iter().map(Into::into).collect())
    }

    /// Remove a page from its document, permanently.
    ///
    /// The bytes, transcript, links and identifications go with it. Removing
    /// the last page leaves the document standing and empty.
    async fn delete_media_page(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        document_id: ID,
        page_id: ID,
    ) -> Result<bool> {
        let tree_id = live_tree(ctx, &tree_id).await?;
        Box::pin(media::delete_page(
            db_from_ctx(ctx),
            profiles_from_ctx(ctx),
            &**media_from_ctx(ctx),
            tree_id,
            uuid(&document_id)?,
            uuid(&page_id)?,
        ))
        .await?;
        Ok(true)
    }

    // ── Vignette Mutations ───────────────────────────────────────────

    /// Crop a region out of a media file.
    async fn create_vignette(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        input: CreateVignetteInput,
    ) -> Result<GqlVignette> {
        let tree_id = live_tree(ctx, &tree_id).await?;
        let media_id = uuid(&input.media_id)?;
        let vignette =
            vignette::create_vignette(db_from_ctx(ctx), tree_id, media_id, input.try_into()?)
                .await?;
        Ok(vignette.into())
    }

    /// Move or re-attribute a vignette.
    async fn update_vignette(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        id: ID,
        input: UpdateVignetteInput,
    ) -> Result<GqlVignette> {
        let tree_id = live_tree(ctx, &tree_id).await?;
        let vignette =
            vignette::update_vignette(db_from_ctx(ctx), tree_id, uuid(&id)?, input.try_into()?)
                .await?;
        Ok(vignette.into())
    }

    /// Delete a vignette. The media it cropped is untouched.
    async fn delete_vignette(&self, ctx: &Context<'_>, tree_id: ID, id: ID) -> Result<bool> {
        let tree_id = live_tree(ctx, &tree_id).await?;
        vignette::delete_vignette(
            db_from_ctx(ctx),
            profiles_from_ctx(ctx),
            tree_id,
            uuid(&id)?,
        )
        .await?;
        Ok(true)
    }

    /// Create a media link.
    async fn create_media_link(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        input: CreateMediaLinkInput,
    ) -> Result<GqlMediaLink> {
        let tree_id = live_tree(ctx, &tree_id).await?;
        let link = media_link::create_media_link(
            db_from_ctx(ctx),
            profiles_from_ctx(ctx),
            tree_id,
            input.try_into()?,
        )
        .await?;
        Ok(link.into())
    }

    /// Delete a media link (hard delete).
    async fn delete_media_link(&self, ctx: &Context<'_>, tree_id: ID, id: ID) -> Result<bool> {
        let tree_id = live_tree(ctx, &tree_id).await?;
        media_link::delete_media_link(
            db_from_ctx(ctx),
            profiles_from_ctx(ctx),
            tree_id,
            uuid(&id)?,
        )
        .await?;
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
        let tid = live_tree(ctx, &tree_id).await?;
        let new = NewNote {
            text: input.text,
            person_id: opt_uuid(input.person_id)?,
            event_id: opt_uuid(input.event_id)?,
            family_id: opt_uuid(input.family_id)?,
            source_id: opt_uuid(input.source_id)?,
            media_id: opt_uuid(input.media_id)?,
        };
        let note = note::create_note(db_from_ctx(ctx), profiles_from_ctx(ctx), tid, new).await?;
        Ok(note.into())
    }

    /// Update a note.
    async fn update_note(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        id: ID,
        input: UpdateNoteInput,
    ) -> Result<GqlNote> {
        let tid = live_tree(ctx, &tree_id).await?;
        let id = uuid(&id)?;
        let note = note::update_note(
            db_from_ctx(ctx),
            profiles_from_ctx(ctx),
            tid,
            id,
            NotePatch { text: input.text },
        )
        .await?;
        Ok(note.into())
    }

    /// Delete a note (soft delete).
    async fn delete_note(&self, ctx: &Context<'_>, tree_id: ID, id: ID) -> Result<bool> {
        let tid = live_tree(ctx, &tree_id).await?;
        let id = uuid(&id)?;
        note::delete_note(db_from_ctx(ctx), profiles_from_ctx(ctx), tid, id).await?;
        Ok(true)
    }

    // ── Import Mutations ──────────────────────────────────────────────

    /// Re-cut every occurrence of one surname at the given particle — the
    /// dictionary's bulk repair for an import that guessed wrong across a
    /// whole family. Triggers a full projection rebuild when anything changed.
    async fn set_family_name_particle(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        input: SetFamilyNameParticleInput,
    ) -> Result<GqlFamilyNameParticleUpdate> {
        let tree_id = live_tree(ctx, &tree_id).await?;
        let update = family_names::set_particle(
            db_from_ctx(ctx),
            profiles_from_ctx(ctx),
            tree_id,
            input.into(),
        )
        .await?;
        Ok(update.into())
    }

    /// Give every person whose primary name carries one surname another one,
    /// merging into that name when it is already listed. Mirrors
    /// `PATCH /trees/{id}/dictionary/family-names/rename`.
    async fn rename_family_name(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        input: RenameFamilyNameInput,
    ) -> Result<GqlFamilyNameRename> {
        let tree_id = live_tree(ctx, &tree_id).await?;
        let renamed = family_names::rename(
            db_from_ctx(ctx),
            profiles_from_ctx(ctx),
            tree_id,
            input.into(),
        )
        .await?;
        Ok(renamed.into())
    }

    /// Queue a durable GEDZIP export. The artifact is downloaded through the
    /// URL exposed by `exportJobStatus` once the worker completes it. A tree
    /// already running a job answers CONFLICT.
    async fn start_export_job(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        merge_occupations: Option<bool>,
        merge_names: Option<bool>,
    ) -> Result<GqlBackgroundJobStarted> {
        let tree_id = live_tree(ctx, &tree_id).await?;
        let job_id = crate::service::background_job::start_export_job(
            db_from_ctx(ctx),
            tree_id,
            merge_occupations.unwrap_or(false),
            merge_names.unwrap_or(false),
        )
        .await?;
        Ok(GqlBackgroundJobStarted {
            job_id: ID(job_id.to_string()),
        })
    }

    // ── Geneanet import wizard ───────────────────────────────────────

    /// Encode a Geneanet wizard session as a base64 archive.
    async fn encode_geneanet_session(
        &self,
        ctx: &Context<'_>,
        input: GeneanetSessionEncodeInput,
    ) -> Result<GqlGeneanetSessionArchive> {
        require_local_file_access(ctx)?;
        let media = input
            .media
            .iter()
            .filter_map(|entry| {
                std::fs::read(&entry.path).ok().map(|bytes| {
                    (
                        entry.url.clone(),
                        base64::engine::general_purpose::STANDARD.encode(bytes),
                    )
                })
            })
            .collect();
        let archive = oxidgene_geneanet::session::encode(&oxidgene_geneanet::session::Session {
            collection: input.collection,
            deposit_sizes: geneanet_deposit_sizes(&input.deposit_sizes)?,
            account: input.account,
            media,
        })
        .map_err(|error| async_graphql::Error::new(error.to_string()))?;
        Ok(GqlGeneanetSessionArchive {
            archive_base64: base64::engine::general_purpose::STANDARD.encode(archive),
        })
    }

    /// Delete the staged media of a decoded session the wizard closed or
    /// reset without importing, as `POST /geneanet/session/release` does. A
    /// path the backend did not stage is ignored.
    async fn release_geneanet_session_media(
        &self,
        ctx: &Context<'_>,
        paths: Vec<String>,
    ) -> Result<bool> {
        require_local_file_access(ctx)?;
        crate::service::session_media::remove_owned(paths.iter().map(String::as_str));
        Ok(true)
    }

    /// Decode a saved Geneanet session. Its media are staged as local files
    /// for a following desktop import, just as they are through REST.
    async fn decode_geneanet_session(
        &self,
        ctx: &Context<'_>,
        archive_base64: String,
    ) -> Result<GqlGeneanetSession> {
        require_local_file_access(ctx)?;
        let permit = crate::service::session_media::LOADS
            .acquire()
            .await
            .map_err(|_| async_graphql::Error::new("session loading is unavailable"))?;
        let session = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let mut reader = base64::read::DecoderReader::new(
                archive_base64.as_bytes(),
                &base64::engine::general_purpose::STANDARD,
            );
            let mut upload = tempfile::tempfile()?;
            std::io::copy(&mut reader, &mut upload)?;
            std::io::Seek::rewind(&mut upload)?;
            crate::service::session_media::decode(upload)
        })
        .await
        .map_err(|_| async_graphql::Error::new("session decoding failed"))??;
        let photo_count = oxidgene_geneanet::manifest_from_collection(&session.collection)
            .map(|manifest| manifest.view_count as i64)
            .unwrap_or(0);
        Ok(GqlGeneanetSession {
            collection: session.collection,
            deposit_sizes: session
                .deposit_sizes
                .into_iter()
                .map(|(deposit_id, size)| GqlGeneanetDepositSize {
                    deposit_id,
                    size: size as i64,
                })
                .collect(),
            account: session.account,
            photo_count,
            media: session
                .media
                .into_iter()
                .map(|(url, path)| GqlGeneanetMediaPath { url, path })
                .collect(),
        })
    }

    /// Queue a Geneanet tree import with media collected by the desktop window.
    async fn import_geneanet(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        input: GeneanetImportInput,
    ) -> Result<GqlBackgroundJobStarted> {
        require_local_file_access(ctx)?;
        let db = db_from_ctx(ctx);
        let media = media_from_ctx(ctx);
        let tree_id = live_tree(ctx, &tree_id).await?;
        let gw = base64::engine::general_purpose::STANDARD
            .decode(&input.gw_base64)
            .map_err(|error| async_graphql::Error::new(format!("invalid .gw base64: {error}")))?;
        let job_id = crate::service::background_job::stage_geneanet_import(
            db,
            &**media,
            tree_id,
            &gw,
            input.file_name,
            input.collection,
            geneanet_deposit_sizes(&input.deposit_sizes)?,
            &input.archive_paths,
            &geneanet_media_paths(&input.fetched),
            input.media_fidelity.into(),
        )
        .await?;
        Ok(GqlBackgroundJobStarted {
            job_id: ID(job_id.to_string()),
        })
    }

    // ── History Mutations ────────────────────────────────────────────

    /// Put a record back as one of its versions had it. The restore is a
    /// write of its own, returned as its audit entry. Mirrors
    /// `POST /trees/{treeId}/history/{recordType}/{recordId}/revert`.
    async fn revert_record(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        record_type: GqlRecordType,
        record_id: ID,
        version: i32,
    ) -> Result<GqlAuditEntry> {
        let db = db_from_ctx(ctx);
        let profiles = profiles_from_ctx(ctx);
        let tid = live_tree(ctx, &tree_id).await?;
        let rid = uuid(&record_id)?;
        let txn = begin_tx(db).await?;
        let entry = history::revert(&txn, profiles, tid, record_type.into(), rid, version).await?;
        commit_tx(txn).await?;
        Ok(entry.into())
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
        let tid = live_tree(ctx, &tree_id).await?;
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
        let tid = live_tree(ctx, &tree_id).await?;
        let pid = uuid(&person_id)?;
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
        let tid = live_tree(ctx, &tree_id).await?;
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
        let tid = live_tree(ctx, &tree_id).await?;
        let rid = uuid(&root_person_id)?;

        // Boxed: the expansion assembles two pedigrees, and inlining that
        // future into the mutation root's pushed the compiler's Send check
        // past its recursion limit (rust-lang/rust#159228).
        let delta = Box::pin(crate::service::pedigrees::expand_pedigree(
            profiles,
            tid,
            rid,
            Expansion {
                direction: direction.into(),
                from_depth: from_depth.into(),
                to_depth: to_depth.into(),
                other_depth: other_depth.into(),
            },
        ))
        .await?;
        Ok(delta.into())
    }
}
