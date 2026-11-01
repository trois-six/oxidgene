//! Repository layer: CRUD operations, pagination, and database utilities.
//!
//! This module provides:
//! - Database connection and migration helpers (`connect`, `run_migrations`)
//! - A generic cursor-based pagination helper
//! - Repository implementations for all entities

mod access_event;
mod access_scope;
mod ancestry;
mod app_user;
mod background_job;
pub(crate) mod batch;
mod citation;
mod connection;
mod descent;
mod dictionary;
mod event;
mod event_spouse_age;
mod event_witness;
mod exclusion;
mod family;
mod family_child;
mod family_spouse;
mod gedcom_kept;
mod history;
mod invitation;
mod media;
mod media_library;
mod media_link;
mod membership;
mod note;
mod pagination;
mod person;

mod person_denorm;
mod person_distinct;
mod person_merge;
mod person_name;
mod person_search;
mod person_visibility;
mod place;
mod record_tag;
mod repository;
mod share_link;
mod snapshot;
mod sosa;
mod source;
mod tag;
mod tree;
mod vignette;

pub use access_event::{AccessEvent, AccessEventRepo, NewAccessEvent};
pub use access_scope::{AccessScope, AccessScopeRepo};
pub use ancestry::{AncestryRepo, FamilyLink, MAX_GENERATIONS};
pub use app_user::{AppUser, AppUserChanges, AppUserRepo, InstanceRole, NewAppUser, UserStatus};
pub use background_job::{
    BackgroundJob, BackgroundJobKind, BackgroundJobRepo, BackgroundJobStatus, JOB_CHANNEL,
    NewBackgroundJob, QUEUE_CHANNEL, job_actor,
};
pub use citation::{CitationFilter, CitationRepo};
pub use connection::{
    Connections, ErasingTransaction, connect, connect_read_pool, erase_deleted_content,
    refresh_statistics, rollback_migrations, run_migrations,
};
pub use descent::DescentRows;
pub use dictionary::{
    DictionaryRepo, DictionaryValueEntry, FamilyNameParticleUpdate, FamilyNameRename,
    HiddenRecords, OccupationRename, PersonUsageEntry, SOURCE_DRILL_THRESHOLD,
};
pub use event::{EventDetails, EventDetailsPatch, EventFilter, EventRepo};
pub use event_spouse_age::EventSpouseAgeRepo;
pub use event_witness::EventWitnessRepo;
pub use exclusion::{Exclusion, ExclusionRepo};
pub use family::FamilyRepo;
pub use family_child::FamilyChildRepo;
pub use family_spouse::FamilySpouseRepo;
pub use gedcom_kept::GedcomKeptRepo;
pub use history::{AuditFilter, HistoryRepo, NewRecordVersion, VersionHead};
pub use invitation::{InvitationEnd, InvitationRepo, NewInvitation, TreeInvitation};
pub use media::{MediaPatch, MediaRepo, UploadedMedia, UploadedMediaMetadata};
pub use media_library::{MediaFacets, MediaFilter, MediaLibraryRepo, MediaTagCount};
pub use media_link::{MediaLinkRepo, MediaLinkRow, MediaLinkTarget};
pub use membership::{MembershipChanges, MembershipRepo, SharedMembership, TreeMembership};
pub use note::{NoteFilter, NoteRepo};
pub use pagination::{PaginationParams, encode_cursor};
pub use person::{PersonRepo, PortraitRow};

pub use person_denorm::{EncodedProfiles, PersonDenormRepo};
pub use person_distinct::PersonDistinctRepo;
pub use person_merge::PersonMergeRepo;
pub use person_name::{PersonNamePieces, PersonNamePiecesPatch, PersonNameRepo};
pub use person_search::{
    PersonSearchEntry, PersonSearchFilters, PersonSearchPage, PersonSearchRepo, PersonSearchSort,
    RELATIVE_SEP,
};
pub use person_visibility::{Membership, MembershipsOf, PersonVisibilityRepo};
pub use place::{PlaceFilter, PlaceRepo};
pub use record_tag::RecordTagRepo;
pub use repository::{
    RepositoryFields, RepositoryPatch, RepositoryRepo, SourceRepositoryFields,
    SourceRepositoryPatch, SourceRepositoryRepo,
};
pub use share_link::{NewShareLink, ShareLink, ShareLinkChanges, ShareLinkRepo};
pub use snapshot::{BuiltSnapshot, SnapshotRepo, display_names};
pub use sosa::{ParentLink, SosaAncestor, SosaRepo};
pub use source::{SourceFilter, SourceRepo};
pub use tag::{TagRepo, TagUsage, checked_color, checked_label};
pub use tree::{TreeChanges, TreeRepo};
pub use vignette::{VignetteInput, VignettePatch, VignetteRepo};

/// Row `id` of entity `E` unless it is missing or soft-deleted (its
/// `deleted_at` column set); `NotFound` naming `entity` otherwise. What every
/// repository's own `find_live` reads through.
pub(crate) async fn find_live<E>(
    db: &impl sea_orm::ConnectionTrait,
    id: uuid::Uuid,
    deleted_at: E::Column,
    entity: &'static str,
) -> Result<E::Model, oxidgene_core::OxidGeneError>
where
    E: sea_orm::EntityTrait,
    E::PrimaryKey: sea_orm::PrimaryKeyTrait<ValueType = uuid::Uuid>,
{
    use sea_orm::{ColumnTrait, QueryFilter};
    E::find_by_id(id)
        .filter(deleted_at.is_null())
        .one(db)
        .await
        .map_err(db_err)?
        .ok_or(oxidgene_core::OxidGeneError::NotFound { entity, id })
}

/// A database error as the domain reports it.
///
/// Public so that every layer maps a `DbErr` the same way: the API's services
/// open and commit transactions and run a few queries of their own.
pub fn db_err(error: sea_orm::DbErr) -> oxidgene_core::error::OxidGeneError {
    oxidgene_core::error::OxidGeneError::Database(error.to_string())
}

/// `column` contains `value`, ignoring case: a list's `search` filter. Both
/// sides are lowered by the database, as in [`lower_trim_eq`]: a bare `LIKE`
/// ignores case on SQLite but not on PostgreSQL, so the same search would
/// find different records on the desktop and on the web.
/// `%` and `_` in `value` match themselves, see [`escape_like`].
pub(crate) fn lower_contains<C: sea_orm::ColumnTrait>(
    column: C,
    value: &str,
) -> sea_orm::sea_query::SimpleExpr {
    use sea_orm::sea_query::{BinOper, Expr, ExprTrait, Func};
    let pattern = ExprTrait::binary(
        Func::lower(Expr::val(format!("%{}%", escape_like(value)))),
        BinOper::Escape,
        Expr::Constant('\\'.into()),
    );
    ExprTrait::binary(
        Expr::expr(Func::lower(Expr::col(column))),
        BinOper::Like,
        pattern,
    )
}

/// Escape `LIKE`'s wildcards so user text matches literally, under
/// `ESCAPE '\'`: a search for `50%` finds "50%", not every text holding
/// "50".
pub(crate) fn escape_like(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// `column`, trimmed, equals `value`, trimmed, ignoring case: the way an
/// entry form's typed title or name finds the record it names. Both sides
/// are lowered by the database, so what "case" covers is the backend's own
/// rule (ASCII on SQLite, Unicode on PostgreSQL), identical for both sides.
pub(crate) fn lower_trim_eq<C: sea_orm::ColumnTrait>(
    column: C,
    value: &str,
) -> sea_orm::sea_query::SimpleExpr {
    use sea_orm::sea_query::{Alias, Expr, ExprTrait, Func};
    let trimmed = |expr: Expr| Func::lower(Func::cust(Alias::new("TRIM")).arg(expr));
    ExprTrait::eq(
        Expr::expr(trimmed(Expr::col(column))),
        trimmed(Expr::val(value.to_string())),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::sea_query::{PostgresQueryBuilder, Query, SqliteQueryBuilder};

    #[test]
    fn like_wildcards_in_user_text_match_literally() {
        assert_eq!(escape_like("100%_a\\b"), "100\\%\\_a\\\\b");
    }

    /// The list filters escape `%` and `_` and say so with `ESCAPE`, under
    /// the string syntax of each backend.
    #[test]
    fn a_list_search_escapes_its_wildcards() {
        let query = Query::select()
            .column(crate::entities::place::Column::Id)
            .from(crate::entities::place::Entity)
            .and_where(lower_contains(crate::entities::place::Column::Name, "50%_"))
            .to_owned();
        let (sqlite, values) = query.build(SqliteQueryBuilder);
        assert!(sqlite.ends_with("LIKE LOWER(?) ESCAPE '\\'"), "{sqlite}");
        assert_eq!(values.0, [sea_orm::Value::from("%50\\%\\_%")]);
        let (postgres, _) = query.build(PostgresQueryBuilder);
        assert!(
            postgres.ends_with("LIKE LOWER($1) ESCAPE E'\\\\'"),
            "{postgres}"
        );
    }
}
