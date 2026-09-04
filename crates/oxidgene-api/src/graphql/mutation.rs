//! GraphQL mutation root with all write operations.

use crate::profile::invalidation;
use crate::rest::state::{TreeResource, begin_tx, commit_tx, require_tree_resource};
use crate::service::event_date;
use async_graphql::{Context, ID, MaybeUndefined, Object, Result};
use base64::Engine as _;
use uuid::Uuid;

use oxidgene_db::repo::{
    BackgroundJobKind, BackgroundJobRepo, CitationRepo, DictionaryRepo, EventRepo,
    EventWitnessRepo, FamilyChildRepo, FamilyRepo, FamilySpouseRepo, MediaLinkRepo, MediaRepo,
    MediaTagRepo, NewBackgroundJob, NoteRepo, PersonNamePieces, PersonNamePiecesPatch,
    PersonNameRepo, PersonRepo, PlaceRepo, SourceRepo, TreeRepo, UploadedMedia, VignetteInput,
    VignettePatch, VignetteRepo,
};

use super::inputs::{
    AddChildInput, AddEventWitnessInput, AddSpouseInput, CreateCitationInput, CreateEventInput,
    CreateMediaLinkInput, CreateNoteInput, CreatePersonInput, CreatePlaceInput, CreateSourceInput,
    CreateTreeInput, CreateVignetteInput, GeneanetImportInput, GeneanetSessionEncodeInput,
    PersonNameInput, SetFamilyNameParticleInput, UpdateCitationInput, UpdateEventInput,
    UpdateFamilyInput, UpdateMediaInput, UpdateNoteInput, UpdatePersonInput, UpdatePersonNameInput,
    UpdatePlaceInput, UpdateSourceInput, UpdateTreeInput, UpdateVignetteInput,
    UploadMediaFileInput, UploadMediaInput, geneanet_deposit_sizes, geneanet_media_paths,
};
use super::types::{
    GqlBackgroundJobStarted, GqlCitation, GqlEvent, GqlEventWitness, GqlFamily, GqlFamilyChild,
    GqlFamilyNameParticleUpdate, GqlFamilySpouse, GqlGeneanetDepositSize, GqlGeneanetMediaPath,
    GqlGeneanetSession, GqlGeneanetSessionArchive, GqlMedia, GqlMediaLink, GqlNote,
    GqlPedigreeDelta, GqlPedigreeDirection, GqlPerson, GqlPersonName, GqlPlace,
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

/// Same, for a field that has to be parsed on the way through.
///
/// A `null` clears without parsing anything; only a real value can fail.
fn patch_parse<T, U, E>(
    value: MaybeUndefined<T>,
    parse: impl FnOnce(T) -> Result<U, E>,
    field: &str,
) -> Result<Option<Option<U>>>
where
    E: std::fmt::Display,
{
    match value {
        MaybeUndefined::Undefined => Ok(None),
        MaybeUndefined::Null => Ok(Some(None)),
        MaybeUndefined::Value(v) => parse(v)
            .map(|parsed| Some(Some(parsed)))
            .map_err(|e| async_graphql::Error::new(format!("Invalid {field}: {e}"))),
    }
}

/// Maps a non-nullable update field (one that can be set or left alone, but
/// never cleared) from `MaybeUndefined`. `Undefined` and `Null` both leave the
/// column untouched; only a real value updates it.
fn patch_scalar<T, U>(value: MaybeUndefined<T>) -> Option<U>
where
    U: From<T>,
{
    match value {
        MaybeUndefined::Value(v) => Some(v.into()),
        _ => None,
    }
}

fn stage_geneanet_media(
    media: &std::collections::HashMap<String, String>,
) -> Result<Vec<GqlGeneanetMediaPath>> {
    Ok(crate::service::session_media::stage(media)?
        .into_iter()
        .map(|(url, path)| GqlGeneanetMediaPath { url, path })
        .collect())
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
        if name.trim().is_empty() {
            return Err(async_graphql::Error::new("name must not be empty"));
        }
        let db = db_from_ctx(ctx);
        let profiles = profiles_from_ctx(ctx);
        let source_tree_id = Uuid::parse_str(tree_id.as_str())?;
        let export =
            crate::service::gedcom::load_and_export(db, source_tree_id, false, false, false)
                .await?;
        let new_tree_id = Uuid::now_v7();
        let tree = TreeRepo::create(db, new_tree_id, name, None).await?;
        crate::service::gedcom::import_and_persist(db, new_tree_id, &export.gedcom).await?;
        profiles.rebuild_tree_full(db, new_tree_id).await?;
        Ok(tree.into())
    }

    /// Update an existing tree.
    async fn update_tree(
        &self,
        ctx: &Context<'_>,
        id: ID,
        input: UpdateTreeInput,
    ) -> Result<GqlTree> {
        if input
            .name
            .as_deref()
            .is_some_and(|name| name.trim().is_empty())
        {
            return Err(async_graphql::Error::new("name must not be empty"));
        }
        let db = db_from_ctx(ctx);
        let uuid = Uuid::parse_str(id.as_str())?;
        let sosa_root = patch_parse(
            input.sosa_root_person_id,
            |s| Uuid::parse_str(&s),
            "sosa_root_person_id",
        )?;
        let self_person = patch_parse(
            input.self_person_id,
            |s| Uuid::parse_str(&s),
            "self_person_id",
        )?;
        let tree = TreeRepo::update(
            db,
            uuid,
            input.name,
            patch(input.description),
            sosa_root,
            self_person,
            input.default_privacy.map(Into::into),
        )
        .await?;
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
        tree_id: ID,
        id: ID,
        input: UpdatePersonInput,
    ) -> Result<GqlPerson> {
        let db = db_from_ctx(ctx);
        let profiles = profiles_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let uuid = Uuid::parse_str(id.as_str())?;
        let txn = begin_tx(db).await?;
        PersonRepo::get_in_tree(&txn, tid, uuid).await?;
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
            .invalidate_for_mutation(&txn, tid, &affected)
            .await?;
        commit_tx(txn).await?;
        Ok(person.into())
    }

    /// Delete a person (soft delete).
    async fn delete_person(&self, ctx: &Context<'_>, tree_id: ID, id: ID) -> Result<bool> {
        let db = db_from_ctx(ctx);
        let profiles = profiles_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let uuid = Uuid::parse_str(id.as_str())?;
        let txn = begin_tx(db).await?;
        PersonRepo::get_in_tree(&txn, tid, uuid).await?;
        PersonRepo::delete(&txn, uuid).await?;
        // Drops the person's projection + search row and refreshes the
        // relatives that referenced them.
        profiles
            .invalidate_for_person_delete(&txn, tid, uuid)
            .await?;
        commit_tx(txn).await?;
        Ok(true)
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
        let db = db_from_ctx(ctx);
        let profiles = profiles_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let pid = Uuid::parse_str(person_id.as_str())?;
        let id = Uuid::now_v7();
        let txn = begin_tx(db).await?;
        require_tree_resource(&txn, tid, TreeResource::Person, pid).await?;
        let name = PersonNameRepo::create(
            &txn,
            id,
            pid,
            input.name_type.into(),
            PersonNamePieces {
                given_names: input.given_names,
                surname: input.surname,
                surname_prefix: input.surname_prefix,
                prefix: input.prefix,
                suffix: input.suffix,
                nickname: input.nickname,
            },
            input.is_primary,
            input.sort_order.unwrap_or(0),
        )
        .await?;
        // Name changes affect display_name references across relatives.
        let affected = invalidation::affected_persons(&txn, pid).await?;
        profiles
            .invalidate_for_mutation(&txn, tid, &affected)
            .await?;
        commit_tx(txn).await?;
        Ok(name.into())
    }

    /// Update a person name.
    async fn update_person_name(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        person_id: ID,
        id: ID,
        input: UpdatePersonNameInput,
    ) -> Result<GqlPersonName> {
        let db = db_from_ctx(ctx);
        let profiles = profiles_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let pid = Uuid::parse_str(person_id.as_str())?;
        let uuid = Uuid::parse_str(id.as_str())?;
        let txn = begin_tx(db).await?;
        require_tree_resource(&txn, tid, TreeResource::Person, pid).await?;
        require_tree_resource(&txn, tid, TreeResource::PersonName, uuid).await?;
        let name = PersonNameRepo::update(
            &txn,
            uuid,
            input.name_type.map(|nt| nt.into()),
            PersonNamePiecesPatch {
                given_names: patch(input.given_names),
                surname: patch(input.surname),
                surname_prefix: patch(input.surname_prefix),
                prefix: patch(input.prefix),
                suffix: patch(input.suffix),
                nickname: patch(input.nickname),
            },
            input.is_primary,
            input.sort_order,
        )
        .await?;
        let affected = invalidation::affected_persons(&txn, name.person_id).await?;
        profiles
            .invalidate_for_mutation(&txn, tid, &affected)
            .await?;
        commit_tx(txn).await?;
        Ok(name.into())
    }

    /// Delete a person name (hard delete).
    async fn delete_person_name(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        person_id: ID,
        id: ID,
    ) -> Result<bool> {
        let db = db_from_ctx(ctx);
        let profiles = profiles_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let pid = Uuid::parse_str(person_id.as_str())?;
        let uuid = Uuid::parse_str(id.as_str())?;
        let txn = begin_tx(db).await?;
        require_tree_resource(&txn, tid, TreeResource::Person, pid).await?;
        require_tree_resource(&txn, tid, TreeResource::PersonName, uuid).await?;
        PersonNameRepo::delete(&txn, uuid).await?;
        let affected = invalidation::affected_persons(&txn, pid).await?;
        profiles
            .invalidate_for_mutation(&txn, tid, &affected)
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

    /// Update a family: its privacy, and `updatedAt` either way.
    async fn update_family(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        id: ID,
        input: UpdateFamilyInput,
    ) -> Result<GqlFamily> {
        let db = db_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let uuid = Uuid::parse_str(id.as_str())?;
        require_tree_resource(db, tid, TreeResource::Family, uuid).await?;
        let family = FamilyRepo::update(db, uuid, input.privacy.map(Into::into)).await?;
        Ok(family.into())
    }

    /// Delete a family (soft delete).
    async fn delete_family(&self, ctx: &Context<'_>, tree_id: ID, id: ID) -> Result<bool> {
        let db = db_from_ctx(ctx);
        let profiles = profiles_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let uuid = Uuid::parse_str(id.as_str())?;
        let txn = begin_tx(db).await?;
        require_tree_resource(&txn, tid, TreeResource::Family, uuid).await?;
        // Compute affected BEFORE delete.
        let affected = invalidation::affected_persons_for_family(&txn, uuid).await?;
        FamilyRepo::delete(&txn, uuid).await?;
        if !affected.is_empty() {
            profiles
                .invalidate_for_mutation(&txn, tid, &affected)
                .await?;
        }
        commit_tx(txn).await?;
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
        let db = db_from_ctx(ctx);
        let profiles = profiles_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let fid = Uuid::parse_str(family_id.as_str())?;
        let pid = Uuid::parse_str(&input.person_id)?;
        let id = Uuid::now_v7();
        let txn = begin_tx(db).await?;
        require_tree_resource(&txn, tid, TreeResource::Family, fid).await?;
        require_tree_resource(&txn, tid, TreeResource::Person, pid).await?;
        let spouse =
            FamilySpouseRepo::create(&txn, id, fid, pid, input.role.into(), input.sort_order)
                .await?;
        let affected =
            invalidation::affected_persons_for_family_spouse_change(&txn, fid, pid).await?;
        profiles
            .invalidate_for_mutation(&txn, tid, &affected)
            .await?;
        commit_tx(txn).await?;
        Ok(spouse.into())
    }

    /// Remove a spouse from a family (hard delete).
    async fn remove_spouse(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        family_id: ID,
        id: ID,
    ) -> Result<bool> {
        let db = db_from_ctx(ctx);
        let profiles = profiles_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let fid = Uuid::parse_str(family_id.as_str())?;
        let uuid = Uuid::parse_str(id.as_str())?;
        let txn = begin_tx(db).await?;
        require_tree_resource(&txn, tid, TreeResource::Family, fid).await?;
        require_tree_resource(&txn, tid, TreeResource::FamilySpouse, uuid).await?;
        // Look up which person this spouse link refers to BEFORE deletion.
        let spouses = FamilySpouseRepo::list_by_families(&txn, &[fid]).await?;
        let person_id = spouses.iter().find(|s| s.id == uuid).map(|s| s.person_id);
        // Compute affected BEFORE delete.
        let affected = if let Some(pid) = person_id {
            invalidation::affected_persons_for_family_spouse_change(&txn, fid, pid).await?
        } else {
            vec![]
        };
        FamilySpouseRepo::delete(&txn, uuid).await?;
        if !affected.is_empty() {
            profiles
                .invalidate_for_mutation(&txn, tid, &affected)
                .await?;
        }
        commit_tx(txn).await?;
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
        let db = db_from_ctx(ctx);
        let profiles = profiles_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let fid = Uuid::parse_str(family_id.as_str())?;
        let pid = Uuid::parse_str(&input.person_id)?;
        let id = Uuid::now_v7();
        let txn = begin_tx(db).await?;
        require_tree_resource(&txn, tid, TreeResource::Family, fid).await?;
        require_tree_resource(&txn, tid, TreeResource::Person, pid).await?;
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
        profiles
            .invalidate_for_mutation(&txn, tid, &affected)
            .await?;
        commit_tx(txn).await?;
        Ok(child.into())
    }

    /// Remove a child from a family (hard delete).
    async fn remove_child(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        family_id: ID,
        id: ID,
    ) -> Result<bool> {
        let db = db_from_ctx(ctx);
        let profiles = profiles_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let fid = Uuid::parse_str(family_id.as_str())?;
        let uuid = Uuid::parse_str(id.as_str())?;
        let txn = begin_tx(db).await?;
        require_tree_resource(&txn, tid, TreeResource::Family, fid).await?;
        require_tree_resource(&txn, tid, TreeResource::FamilyChild, uuid).await?;
        // Look up which person this child link refers to BEFORE deletion.
        let children = FamilyChildRepo::list_by_families(&txn, &[fid]).await?;
        let person_id = children.iter().find(|c| c.id == uuid).map(|c| c.person_id);
        let affected = if let Some(pid) = person_id {
            invalidation::affected_persons_for_family_child_change(&txn, fid, pid).await?
        } else {
            vec![]
        };
        FamilyChildRepo::delete(&txn, uuid).await?;
        if !affected.is_empty() {
            profiles
                .invalidate_for_mutation(&txn, tid, &affected)
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
        let calendar = input.calendar.map(Into::into).unwrap_or_default();
        // Derived here, never taken from the input — see `service::event_date`.
        let date_sort = event_date::derive(calendar, input.date_value.as_deref());
        let txn = begin_tx(db).await?;
        if let Some(place_id) = place_id {
            require_tree_resource(&txn, tid, TreeResource::Place, place_id).await?;
        }
        if let Some(person_id) = person_id {
            require_tree_resource(&txn, tid, TreeResource::Person, person_id).await?;
        }
        if let Some(family_id) = family_id {
            require_tree_resource(&txn, tid, TreeResource::Family, family_id).await?;
        }
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
            input.date_qualifier.map(Into::into).unwrap_or_default(),
            input.date_value2,
            calendar,
            input.cause,
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
        tree_id: ID,
        id: ID,
        input: UpdateEventInput,
    ) -> Result<GqlEvent> {
        let db = db_from_ctx(ctx);
        let profiles = profiles_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let uuid = Uuid::parse_str(id.as_str())?;
        let place_id = patch_parse(input.place_id, |s| Uuid::parse_str(&s), "place_id")?;
        let date_value = patch(input.date_value);
        let calendar = patch_scalar(input.calendar);
        let txn = begin_tx(db).await?;
        require_tree_resource(&txn, tid, TreeResource::Event, uuid).await?;
        if let Some(Some(place_id)) = place_id {
            require_tree_resource(&txn, tid, TreeResource::Place, place_id).await?;
        }
        // Derived from the patched state, reading whichever half the patch
        // leaves alone off the stored event — see `service::event_date`.
        let stored = EventRepo::get(&txn, uuid).await?;
        let date_sort = Some(event_date::derive_patch(
            stored.calendar,
            stored.date_value.as_deref(),
            calendar,
            date_value.as_ref().map(Option::as_deref),
        ));
        let event = EventRepo::update(
            &txn,
            uuid,
            input.event_type.map(|et| et.into()),
            date_value,
            date_sort,
            place_id,
            patch(input.description),
            patch_scalar(input.date_qualifier),
            patch(input.date_value2),
            calendar,
            patch(input.cause),
        )
        .await?;
        // Invalidate based on event ownership.
        if let Some(pid) = event.person_id {
            let affected = invalidation::affected_persons(&txn, pid).await?;
            profiles
                .invalidate_for_mutation(&txn, tid, &affected)
                .await?;
        } else if let Some(fid) = event.family_id {
            let affected = invalidation::affected_persons_for_family(&txn, fid).await?;
            profiles
                .invalidate_for_mutation(&txn, tid, &affected)
                .await?;
        }
        commit_tx(txn).await?;
        Ok(event.into())
    }

    /// Delete an event (soft delete).
    async fn delete_event(&self, ctx: &Context<'_>, tree_id: ID, id: ID) -> Result<bool> {
        let db = db_from_ctx(ctx);
        let profiles = profiles_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let uuid = Uuid::parse_str(id.as_str())?;
        let txn = begin_tx(db).await?;
        require_tree_resource(&txn, tid, TreeResource::Event, uuid).await?;
        let event = EventRepo::get(&txn, uuid).await?;
        EventRepo::delete(&txn, uuid).await?;
        if let Some(pid) = event.person_id {
            let affected = invalidation::affected_persons(&txn, pid).await?;
            profiles
                .invalidate_for_mutation(&txn, tid, &affected)
                .await?;
        } else if let Some(fid) = event.family_id {
            let affected = invalidation::affected_persons_for_family(&txn, fid).await?;
            profiles
                .invalidate_for_mutation(&txn, tid, &affected)
                .await?;
        }
        commit_tx(txn).await?;
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
        let db = db_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let eid = Uuid::parse_str(event_id.as_str())?;
        let pid = Uuid::parse_str(&input.person_id)?;
        let id = Uuid::now_v7();
        require_tree_resource(db, tid, TreeResource::Event, eid).await?;
        require_tree_resource(db, tid, TreeResource::Person, pid).await?;
        let witness =
            EventWitnessRepo::create(db, id, eid, pid, input.relation, input.sort_order).await?;
        Ok(witness.into())
    }

    /// Remove a witness from an event (hard delete).
    async fn remove_event_witness(&self, ctx: &Context<'_>, tree_id: ID, id: ID) -> Result<bool> {
        let db = db_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let uuid = Uuid::parse_str(id.as_str())?;
        require_tree_resource(db, tid, TreeResource::EventWitness, uuid).await?;
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
        tree_id: ID,
        id: ID,
        input: UpdatePlaceInput,
    ) -> Result<GqlPlace> {
        let db = db_from_ctx(ctx);
        let profiles = profiles_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let uuid = Uuid::parse_str(id.as_str())?;
        let txn = begin_tx(db).await?;
        require_tree_resource(&txn, tid, TreeResource::Place, uuid).await?;
        let affected = invalidation::affected_persons_for_place(&txn, uuid).await?;
        let place = PlaceRepo::update(
            &txn,
            uuid,
            input.name,
            patch(input.latitude),
            patch(input.longitude),
        )
        .await?;
        profiles
            .invalidate_for_mutation(&txn, tid, &affected)
            .await?;
        commit_tx(txn).await?;
        Ok(place.into())
    }

    /// Delete a place (hard delete).
    async fn delete_place(&self, ctx: &Context<'_>, tree_id: ID, id: ID) -> Result<bool> {
        let db = db_from_ctx(ctx);
        let profiles = profiles_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let uuid = Uuid::parse_str(id.as_str())?;
        let txn = begin_tx(db).await?;
        require_tree_resource(&txn, tid, TreeResource::Place, uuid).await?;
        let affected = invalidation::affected_persons_for_place(&txn, uuid).await?;
        PlaceRepo::delete(&txn, uuid).await?;
        profiles
            .invalidate_for_mutation(&txn, tid, &affected)
            .await?;
        commit_tx(txn).await?;
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
        tree_id: ID,
        id: ID,
        input: UpdateSourceInput,
    ) -> Result<GqlSource> {
        let db = db_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let uuid = Uuid::parse_str(id.as_str())?;
        require_tree_resource(db, tid, TreeResource::Source, uuid).await?;
        let source = SourceRepo::update(
            db,
            uuid,
            input.title,
            patch(input.author),
            patch(input.publisher),
            patch(input.abbreviation),
            patch(input.repository_name),
        )
        .await?;
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
        let db = db_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let uuid = Uuid::parse_str(id.as_str())?;
        require_tree_resource(db, tid, TreeResource::Source, uuid).await?;
        if only_if_unused {
            return Ok(SourceRepo::delete_if_unused(db, uuid).await?);
        }
        SourceRepo::delete(db, uuid).await?;
        Ok(true)
    }

    // ── Citation Mutations ───────────────────────────────────────────

    /// Create a new citation.
    async fn create_citation(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        input: CreateCitationInput,
    ) -> Result<GqlCitation> {
        let db = db_from_ctx(ctx);
        let profiles = profiles_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
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
        let txn = begin_tx(db).await?;
        require_tree_resource(&txn, tid, TreeResource::Source, source_id).await?;
        if let Some(person_id) = person_id {
            require_tree_resource(&txn, tid, TreeResource::Person, person_id).await?;
        }
        if let Some(event_id) = event_id {
            require_tree_resource(&txn, tid, TreeResource::Event, event_id).await?;
        }
        if let Some(family_id) = family_id {
            require_tree_resource(&txn, tid, TreeResource::Family, family_id).await?;
        }
        let citation = CitationRepo::create(
            &txn,
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
        if let Some(person_id) = citation.person_id {
            profiles
                .invalidate_for_mutation(&txn, tid, &[person_id])
                .await?;
        }
        commit_tx(txn).await?;
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
        let db = db_from_ctx(ctx);
        let profiles = profiles_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let uuid = Uuid::parse_str(id.as_str())?;
        let txn = begin_tx(db).await?;
        require_tree_resource(&txn, tid, TreeResource::Citation, uuid).await?;
        let previous = CitationRepo::get(&txn, uuid).await?;
        let source_id = input
            .source_id
            .map(|id| Uuid::parse_str(id.as_str()))
            .transpose()?;
        if let Some(source_id) = source_id {
            require_tree_resource(&txn, tid, TreeResource::Source, source_id).await?;
        }
        let citation = CitationRepo::update(
            &txn,
            uuid,
            source_id,
            patch(input.page),
            input.confidence.map(|c| c.into()),
            patch(input.text),
        )
        .await?;
        if let Some(person_id) = previous.person_id {
            profiles
                .invalidate_for_mutation(&txn, tid, &[person_id])
                .await?;
        }
        commit_tx(txn).await?;
        Ok(citation.into())
    }

    /// Delete a citation (hard delete).
    async fn delete_citation(&self, ctx: &Context<'_>, tree_id: ID, id: ID) -> Result<bool> {
        let db = db_from_ctx(ctx);
        let profiles = profiles_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let uuid = Uuid::parse_str(id.as_str())?;
        let txn = begin_tx(db).await?;
        require_tree_resource(&txn, tid, TreeResource::Citation, uuid).await?;
        let citation = CitationRepo::get(&txn, uuid).await?;
        CitationRepo::delete(&txn, uuid).await?;
        if let Some(person_id) = citation.person_id {
            profiles
                .invalidate_for_mutation(&txn, tid, &[person_id])
                .await?;
        }
        commit_tx(txn).await?;
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
        // Normalised for the same reason as the REST twin: a MIME type the
        // caller supplied is a claim, not evidence.
        let mime_type = oxidgene_core::types::normalize_mime(
            Some(&input.mime_type),
            if input.file_path.is_empty() {
                &input.file_name
            } else {
                &input.file_path
            },
        );
        let media = MediaRepo::create(
            db,
            id,
            tid,
            input.file_name,
            mime_type,
            input.file_path,
            input.file_size,
            input.title,
            input.description,
        )
        .await?;
        Ok(media.into())
    }

    /// Upload a file's bytes, base64-encoded.
    ///
    /// Creates a media record, or fills in an existing one when `mediaId` is
    /// given. Mirrors `POST /trees/{treeId}/media/upload`; see
    /// [`UploadMediaFileInput`] for why the content is base64 rather than an
    /// `Upload` scalar.
    async fn upload_media_file(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        input: UploadMediaFileInput,
    ) -> Result<GqlMedia> {
        use base64::Engine as _;

        let db = db_from_ctx(ctx);
        let store = media_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&input.content_base64)
            .map_err(|e| async_graphql::Error::new(format!("contentBase64 is not base64: {e}")))?;

        let ingested = crate::media::ingest(&**store, tid, &input.file_name, bytes).await?;
        let upload = UploadedMedia {
            file_name: ingested.file_name,
            mime_type: ingested.mime_type,
            storage_key: ingested.storage_key,
            sha256: ingested.sha256,
            file_size: ingested.file_size,
            thumbnail_key: ingested.thumbnail_key,
            width: ingested.width,
            height: ingested.height,
            page_count: ingested.page_count,
            title: input.title,
            description: input.description,
            created_at: chrono::Utc::now(),
            metadata: Default::default(),
        };

        let media = match input.media_id {
            Some(media_id) => {
                let media_id = Uuid::parse_str(&media_id)?;
                require_tree_resource(db, tid, TreeResource::Media, media_id).await?;
                MediaRepo::attach_file(db, media_id, upload).await?
            }
            None => MediaRepo::create_uploaded(db, Uuid::now_v7(), tid, upload).await?,
        };
        Ok(media.into())
    }

    /// Update media metadata.
    async fn update_media(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        id: ID,
        input: UpdateMediaInput,
    ) -> Result<GqlMedia> {
        let db = db_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let uuid = Uuid::parse_str(id.as_str())?;
        require_tree_resource(db, tid, TreeResource::Media, uuid).await?;
        let stored = MediaRepo::get(db, uuid).await?;

        // Built as the REST request shape and handed to the REST patch
        // builder, so the two surfaces cannot drift: the rules about which
        // media may be repointed, and how `date_sort` is derived, live once.
        let request = crate::rest::dto::UpdateMediaRequest {
            title: patch(input.title),
            description: patch(input.description),
            date_value: patch(input.date_value),
            date_value2: patch(input.date_value2),
            date_qualifier: input.date_qualifier.map(Into::into),
            calendar: input.calendar.map(Into::into),
            place_id: patch_parse(input.place_id, |s| Uuid::parse_str(&s), "placeId")?,
            file_path: input.file_path,
            mime_type: input.mime_type,
            privacy: input.privacy.map(Into::into),
            source_media_type: input.source_media_type.map(Into::into),
            document_category: match input.document_category {
                MaybeUndefined::Undefined => None,
                MaybeUndefined::Null => Some(None),
                MaybeUndefined::Value(c) => Some(Some(c.into())),
            },
        };
        let media_patch = crate::rest::media::media_patch(&stored, request)
            .map_err(|e| async_graphql::Error::new(e.0.to_string()))?;
        let media = MediaRepo::update(db, uuid, media_patch).await?;
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
        let db = db_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let media_id = Uuid::parse_str(id.as_str())?;
        require_tree_resource(db, tid, TreeResource::Media, media_id).await?;
        let media = MediaRepo::get(db, media_id).await?;
        let (tag, normalized_tag) = crate::rest::media::normalize_tag(tag)
            .ok_or_else(|| async_graphql::Error::new("tag must not be empty"))?;
        let target_id = media.parent_media_id.unwrap_or(media.id);
        MediaTagRepo::create(db, target_id, tag, normalized_tag).await?;
        Ok(MediaRepo::get(db, target_id).await?.into())
    }

    /// Atomically remove one tag without replacing the media's other tags.
    async fn remove_media_tag(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        id: ID,
        tag: String,
    ) -> Result<bool> {
        let db = db_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let media_id = Uuid::parse_str(id.as_str())?;
        require_tree_resource(db, tid, TreeResource::Media, media_id).await?;
        let media = MediaRepo::get(db, media_id).await?;
        let (_, normalized_tag) = crate::rest::media::normalize_tag(tag)
            .ok_or_else(|| async_graphql::Error::new("tag must not be empty"))?;
        MediaTagRepo::delete(
            db,
            media.parent_media_id.unwrap_or(media.id),
            &normalized_tag,
        )
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
        let db = db_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let uuid = Uuid::parse_str(id.as_str())?;
        require_tree_resource(db, tid, TreeResource::Media, uuid).await?;
        let allowed_link_id = if only_if_unreferenced_elsewhere {
            let link_id = allowed_link_id.ok_or_else(|| {
                async_graphql::Error::new(
                    "allowedLinkId is required for conditional media deletion",
                )
            })?;
            let link_id = Uuid::parse_str(link_id.as_str())?;
            require_tree_resource(db, tid, TreeResource::MediaLink, link_id).await?;
            Some(link_id)
        } else {
            None
        };
        Ok(crate::service::media::purge_media(
            db,
            media_from_ctx(ctx).as_ref(),
            uuid,
            allowed_link_id,
        )
        .await?)
    }

    /// Make a media link the person's profile image, or clear the flag.
    ///
    /// Setting one clears the person's others in the same statement, so the
    /// tree never shows two stars. Rebuilds the person's projection, since the
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
        let db = db_from_ctx(ctx);
        let profiles = profiles_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let pid = Uuid::parse_str(person_id.as_str())?;
        require_tree_resource(db, tid, TreeResource::Person, pid).await?;

        let request = crate::rest::dto::SetPortraitRequest {
            media_id: media_id
                .map(|id| Uuid::parse_str(id.as_str()))
                .transpose()?,
            vignette_id: vignette_id
                .map(|id| Uuid::parse_str(id.as_str()))
                .transpose()?,
        };
        if let Some(media_id) = request.media_id {
            require_tree_resource(db, tid, TreeResource::Media, media_id).await?;
        }
        if let Some(vignette_id) = request.vignette_id {
            require_tree_resource(db, tid, TreeResource::Vignette, vignette_id).await?;
        }
        let portrait = request.portrait().map_err(async_graphql::Error::new)?;
        let person = PersonRepo::set_portrait(db, pid, portrait).await?;
        // The portrait is embedded in `person_denorm`, so the projection has
        // to be rebuilt or the tree keeps drawing the old one.
        profiles.rebuild_person(db, tid, pid).await?;
        Ok(person.into())
    }

    /// Create an empty multi-page document.
    async fn create_media_document(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        title: Option<String>,
    ) -> Result<GqlMedia> {
        let db = db_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let media =
            MediaRepo::create_document(db, Uuid::now_v7(), tid, title, chrono::Utc::now()).await?;
        Ok(media.into())
    }

    /// Append an already-uploaded media as the next page of a document.
    async fn append_media_page(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        document_id: ID,
        media_id: ID,
    ) -> Result<GqlMedia> {
        let db = db_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let document_id = Uuid::parse_str(document_id.as_str())?;
        let media_id = Uuid::parse_str(media_id.as_str())?;
        require_tree_resource(db, tid, TreeResource::Media, document_id).await?;
        require_tree_resource(db, tid, TreeResource::Media, media_id).await?;
        let page = MediaRepo::append_page(db, document_id, media_id).await?;
        Ok(page.into())
    }

    /// Set a document's page order. The list must name exactly its pages.
    async fn reorder_media_pages(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        document_id: ID,
        page_ids: Vec<ID>,
    ) -> Result<Vec<GqlMedia>> {
        let db = db_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let document_id = Uuid::parse_str(document_id.as_str())?;
        require_tree_resource(db, tid, TreeResource::Media, document_id).await?;
        let ids: Vec<Uuid> = page_ids
            .iter()
            .map(|id| Uuid::parse_str(id.as_str()))
            .collect::<Result<_, _>>()?;
        for page_id in &ids {
            require_tree_resource(db, tid, TreeResource::Media, *page_id).await?;
        }
        let pages = MediaRepo::reorder_pages(db, document_id, &ids).await?;
        Ok(pages.into_iter().map(Into::into).collect())
    }

    /// Detach a page as ordinary media and remove its external relations.
    async fn detach_media_page(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        document_id: ID,
        page_id: ID,
    ) -> Result<GqlMedia> {
        let db = db_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let document_id = Uuid::parse_str(document_id.as_str())?;
        let page_id = Uuid::parse_str(page_id.as_str())?;
        let txn = begin_tx(db).await?;
        require_tree_resource(&txn, tid, TreeResource::Media, document_id).await?;
        require_tree_resource(&txn, tid, TreeResource::Media, page_id).await?;
        let page = MediaRepo::detach_page(&txn, document_id, page_id).await?;
        commit_tx(txn).await?;
        Ok(page.into())
    }

    // ── Vignette Mutations ───────────────────────────────────────────

    /// Crop a region out of a media file.
    async fn create_vignette(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        input: CreateVignetteInput,
    ) -> Result<GqlVignette> {
        let db = db_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let media_id = Uuid::parse_str(&input.media_id)?;
        require_tree_resource(db, tid, TreeResource::Media, media_id).await?;
        let media = MediaRepo::get(db, media_id).await?;
        crate::media::validate_crop(
            &media,
            input.page,
            input.x,
            input.y,
            input.width,
            input.height,
        )?;

        let vignette = VignetteRepo::create(
            db,
            Uuid::now_v7(),
            VignetteInput {
                media_id,
                page: input.page,
                x: input.x,
                y: input.y,
                width: input.width,
                height: input.height,
                person_id: input
                    .person_id
                    .as_deref()
                    .map(Uuid::parse_str)
                    .transpose()?,
                event_id: input.event_id.as_deref().map(Uuid::parse_str).transpose()?,
            },
        )
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
        let db = db_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let uuid = Uuid::parse_str(id.as_str())?;
        require_tree_resource(db, tid, TreeResource::Vignette, uuid).await?;
        let existing = VignetteRepo::get(db, uuid).await?;

        let rect = match (input.x, input.y, input.width, input.height) {
            (None, None, None, None) => None,
            (Some(x), Some(y), Some(width), Some(height)) => Some((x, y, width, height)),
            _ => {
                return Err(async_graphql::Error::new(
                    "x, y, width and height must be sent together",
                ));
            }
        };

        if rect.is_some() || input.page.is_some() {
            let media = MediaRepo::get(db, existing.media_id).await?;
            let page = input.page.unwrap_or(existing.page);
            let (x, y, width, height) =
                rect.unwrap_or((existing.x, existing.y, existing.width, existing.height));
            crate::media::validate_crop(&media, page, x, y, width, height)?;
        }

        let vignette = VignetteRepo::update(
            db,
            uuid,
            VignettePatch {
                page: input.page,
                rect,
                person_id: patch_parse(input.person_id, |s| Uuid::parse_str(&s), "personId")?,
                event_id: patch_parse(input.event_id, |s| Uuid::parse_str(&s), "eventId")?,
            },
        )
        .await?;
        Ok(vignette.into())
    }

    /// Delete a vignette. The media it cropped is untouched.
    async fn delete_vignette(&self, ctx: &Context<'_>, tree_id: ID, id: ID) -> Result<bool> {
        let db = db_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let uuid = Uuid::parse_str(id.as_str())?;
        require_tree_resource(db, tid, TreeResource::Vignette, uuid).await?;
        VignetteRepo::delete(db, uuid).await?;
        Ok(true)
    }

    /// Create a media link.
    async fn create_media_link(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        input: CreateMediaLinkInput,
    ) -> Result<GqlMediaLink> {
        let db = db_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
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
        require_tree_resource(db, tid, TreeResource::Media, media_id).await?;
        for (resource, id) in [
            (TreeResource::Person, person_id),
            (TreeResource::Event, event_id),
            (TreeResource::Source, source_id),
            (TreeResource::Family, family_id),
        ] {
            if let Some(id) = id {
                require_tree_resource(db, tid, resource, id).await?;
            }
        }
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
    async fn delete_media_link(&self, ctx: &Context<'_>, tree_id: ID, id: ID) -> Result<bool> {
        let db = db_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let uuid = Uuid::parse_str(id.as_str())?;
        require_tree_resource(db, tid, TreeResource::MediaLink, uuid).await?;
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
        let profiles = profiles_from_ctx(ctx);
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
        let media_id = input.media_id.as_deref().map(Uuid::parse_str).transpose()?;
        let txn = begin_tx(db).await?;
        for (resource, id) in [
            (TreeResource::Person, person_id),
            (TreeResource::Event, event_id),
            (TreeResource::Family, family_id),
            (TreeResource::Source, source_id),
            (TreeResource::Media, media_id),
        ] {
            if let Some(id) = id {
                require_tree_resource(&txn, tid, resource, id).await?;
            }
        }
        let note = NoteRepo::create(
            &txn, id, tid, input.text, person_id, event_id, family_id, source_id, media_id,
        )
        .await?;
        if let Some(person_id) = note.person_id {
            profiles
                .invalidate_for_mutation(&txn, tid, &[person_id])
                .await?;
        }
        commit_tx(txn).await?;
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
        let db = db_from_ctx(ctx);
        let profiles = profiles_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let uuid = Uuid::parse_str(id.as_str())?;
        let txn = begin_tx(db).await?;
        require_tree_resource(&txn, tid, TreeResource::Note, uuid).await?;
        let previous = NoteRepo::get(&txn, uuid).await?;
        let note = NoteRepo::update(&txn, uuid, input.text).await?;
        if let Some(person_id) = previous.person_id {
            profiles
                .invalidate_for_mutation(&txn, tid, &[person_id])
                .await?;
        }
        commit_tx(txn).await?;
        Ok(note.into())
    }

    /// Delete a note (soft delete).
    async fn delete_note(&self, ctx: &Context<'_>, tree_id: ID, id: ID) -> Result<bool> {
        let db = db_from_ctx(ctx);
        let profiles = profiles_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let uuid = Uuid::parse_str(id.as_str())?;
        let txn = begin_tx(db).await?;
        require_tree_resource(&txn, tid, TreeResource::Note, uuid).await?;
        let note = NoteRepo::get(&txn, uuid).await?;
        NoteRepo::delete(&txn, uuid).await?;
        if let Some(person_id) = note.person_id {
            profiles
                .invalidate_for_mutation(&txn, tid, &[person_id])
                .await?;
        }
        commit_tx(txn).await?;
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
        let db = db_from_ctx(ctx);
        let profiles = profiles_from_ctx(ctx);
        let tid = Uuid::parse_str(tree_id.as_str())?;
        let txn = begin_tx(db).await?;
        let update =
            DictionaryRepo::set_family_name_particle(&txn, tid, &input.value, &input.particle)
                .await?;
        commit_tx(txn).await?;
        // Same reasoning as the REST handler: a surname reaches every
        // projection embedding a display name, so rebuild the tree eagerly and
        // outside the transaction rather than bounding an unbounded set.
        if update.names_updated > 0 {
            profiles.rebuild_tree_full(db, tid).await?;
        }
        Ok(update.into())
    }

    /// Queue a durable GEDZIP export. The artifact is downloaded through the
    /// URL exposed by `exportJobStatus` once the worker completes it.
    async fn start_export_job(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        merge_occupations: Option<bool>,
        merge_names: Option<bool>,
    ) -> Result<GqlBackgroundJobStarted> {
        let db = db_from_ctx(ctx);
        let tree_id = Uuid::parse_str(tree_id.as_str())?;
        TreeRepo::get(db, tree_id).await?;
        let job_id = Uuid::now_v7();
        BackgroundJobRepo::create(
            db,
            NewBackgroundJob {
                id: job_id,
                tree_id,
                kind: BackgroundJobKind::Export,
                format: "gedzip".into(),
                source_key: None,
                payload_json: None,
                original_filename: None,
                merge_occupations: merge_occupations.unwrap_or(false),
                merge_names: merge_names.unwrap_or(false),
            },
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

    /// Decode a saved Geneanet session. Its media are staged as local files
    /// for a following desktop import, just as they are through REST.
    async fn decode_geneanet_session(
        &self,
        ctx: &Context<'_>,
        archive_base64: String,
    ) -> Result<GqlGeneanetSession> {
        require_local_file_access(ctx)?;
        let archive = base64::engine::general_purpose::STANDARD
            .decode(archive_base64)
            .map_err(|error| {
                async_graphql::Error::new(format!("invalid session base64: {error}"))
            })?;
        let session = oxidgene_geneanet::session::decode(&archive)
            .map_err(|error| async_graphql::Error::new(error.to_string()))?;
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
            media: stage_geneanet_media(&session.media)?,
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
        let tree_id = Uuid::parse_str(tree_id.as_str())?;
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
