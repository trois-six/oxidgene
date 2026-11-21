//! Initial migration: create the full OxidGene schema in one shot.
//!
//! The product is unreleased and its databases are recreated from source
//! imports, so this migration is the complete current schema — tables,
//! columns, indexes and the backend-specific search table — rather than an
//! incremental history of intermediate representations. A schema change
//! edits it; nothing converts data from an earlier shape.

use oxidgene_core::access::AuthorKind;
use oxidgene_core::enums::{DocumentCategory, ExclusionKind};
use oxidgene_core::history::{AuditAction, AuditCategory, AuditEntity, AuditSubject, RecordType};
use sea_orm_migration::sea_orm::{ActiveEnum, ConnectionTrait, DbBackend, Statement};
use sea_orm_migration::{prelude::*, schema::*};

use crate::entities::sea_enums;

#[derive(DeriveMigrationName)]
pub struct Migration;

/// A `CHECK` holding the enumerated column `column` to the `values` the
/// code writes. The access columns' readers turn a value they do not know
/// into the least privilege; refusing one at the write keeps a bad write or
/// a hand edit from reaching them at all (docs/data-model.md §6).
fn one_of(column: &str, values: &[&str]) -> Expr {
    let values: Vec<String> = values.iter().map(|value| format!("'{value}'")).collect();
    Expr::cust(format!("{column} IN ({})", values.join(", ")))
}

/// The named `CHECK` holding `table.column` to the values of the SeaORM
/// enum `E` the column is read as (docs/data-model.md §2). The list is the
/// enum's own, so a variant added to it is accepted by the schema this
/// migration creates without a second list to keep in step. A nullable
/// column passes while it is `NULL`.
fn enum_check<E>(table: &str, column: &str) -> (Alias, Expr)
where
    E: ActiveEnum<Value = String>,
{
    let values = E::values();
    let values: Vec<&str> = values.iter().map(String::as_str).collect();
    named_one_of(table, column, &values)
}

/// [`one_of`], named `ck_{table}_{column}`.
fn named_one_of(table: &str, column: &str, values: &[&str]) -> (Alias, Expr) {
    (
        Alias::new(format!("ck_{table}_{column}")),
        one_of(column, values),
    )
}

/// The stored value of each of `variants`.
fn values_of<T: Copy>(variants: &[T], value: fn(T) -> &'static str) -> Vec<&'static str> {
    variants.iter().map(|variant| value(*variant)).collect()
}

/// A foreign key of a column whose table is created before the one it
/// names: a tree names persons and an access scope, a person a medium and a
/// vignette, all of which reference the tree or the person in turn.
///
/// SQLite accepts a reference to a table that does not exist yet in
/// `CREATE TABLE` and cannot add a constraint to an existing table;
/// PostgreSQL refuses the forward reference and adds the constraint once
/// both tables exist. So each key is declared inline under SQLite
/// ([`with_forward_keys`]) and added at the end under PostgreSQL
/// ([`create_forward_keys`]).
struct ForwardKey {
    name: &'static str,
    table: &'static str,
    column: &'static str,
    target: &'static str,
    /// What deleting the named row does to the pointer; `None` refuses the
    /// deletion while the pointer names it.
    on_delete: Option<ForeignKeyAction>,
}

const FORWARD_KEYS: &[ForwardKey] = &[
    // A deleted person is no longer the tree's SOSA root nor its "self".
    ForwardKey {
        name: "fk_tree_sosa_root_person",
        table: "tree",
        column: "sosa_root_person_id",
        target: "person",
        on_delete: Some(ForeignKeyAction::SetNull),
    },
    ForwardKey {
        name: "fk_tree_self_person",
        table: "tree",
        column: "self_person_id",
        target: "person",
        on_delete: Some(ForeignKeyAction::SetNull),
    },
    // The scope visitors are limited to cannot be deleted while the tree
    // names it: nulling the pointer would widen them to the whole tree.
    // The sharing service points the tree elsewhere first.
    ForwardKey {
        name: "fk_tree_visitor_scope",
        table: "tree",
        column: "visitor_scope_id",
        target: "access_scope",
        on_delete: None,
    },
    // A portrait whose medium or crop goes is no portrait.
    ForwardKey {
        name: "fk_person_portrait_media",
        table: "person",
        column: "portrait_media_id",
        target: "media",
        on_delete: Some(ForeignKeyAction::SetNull),
    },
    ForwardKey {
        name: "fk_person_portrait_vignette",
        table: "person",
        column: "portrait_vignette_id",
        target: "vignette",
        on_delete: Some(ForeignKeyAction::SetNull),
    },
];

/// The constraint of `key`.
fn forward_key(key: &ForwardKey) -> ForeignKeyCreateStatement {
    let mut constraint = ForeignKey::create();
    constraint
        .name(key.name)
        .from(Alias::new(key.table), Alias::new(key.column))
        .to(Alias::new(key.target), Alias::new("id"));
    if let Some(action) = key.on_delete {
        constraint.on_delete(action);
    }
    constraint
}

/// The creation of table `name`, with its [`FORWARD_KEYS`] declared inline
/// under SQLite.
fn with_forward_keys(
    manager: &SchemaManager<'_>,
    name: &str,
    mut table: TableCreateStatement,
) -> TableCreateStatement {
    if manager.get_database_backend() == DbBackend::Sqlite {
        for key in FORWARD_KEYS.iter().filter(|key| key.table == name) {
            table.foreign_key(&mut forward_key(key));
        }
    }
    table
}

/// Add the [`FORWARD_KEYS`] under PostgreSQL, once every table exists.
async fn create_forward_keys(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    if manager.get_database_backend() == DbBackend::Sqlite {
        return Ok(());
    }
    for key in FORWARD_KEYS {
        manager.create_foreign_key(forward_key(key)).await?;
    }
    Ok(())
}

/// How many of `columns` are set, as SQL.
fn set_count(columns: &[&str]) -> String {
    let terms: Vec<String> = columns
        .iter()
        .map(|column| format!("CASE WHEN {column} IS NULL THEN 0 ELSE 1 END"))
        .collect();
    format!("({})", terms.join(" + "))
}

/// The `CHECK` named `ck_{table}_{name}` that exactly one of `columns` is
/// set: a row attached to one record, never to none nor to two.
fn exactly_one(table: &str, name: &str, columns: &[&str]) -> (Alias, Expr) {
    (
        Alias::new(format!("ck_{table}_{name}")),
        Expr::cust(format!("{} = 1", set_count(columns))),
    )
}

/// The `CHECK` named `ck_{table}_{name}` that at most one of `columns` is
/// set.
fn at_most_one(table: &str, name: &str, columns: &[&str]) -> (Alias, Expr) {
    (
        Alias::new(format!("ck_{table}_{name}")),
        Expr::cust(format!("{} <= 1", set_count(columns))),
    )
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        create_records(manager).await?;
        create_derived(manager).await?;
        create_accounts(manager).await?;
        create_schema_meta(manager).await?;
        create_forward_keys(manager).await?;
        create_redaction_triggers(manager).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();
        for table in ["person_search_fts", "person_search_key", "schema_meta"] {
            conn.execute_raw(Statement::from_string(
                manager.get_database_backend(),
                format!("DROP TABLE IF EXISTS {table}"),
            ))
            .await?;
        }
        match manager.get_database_backend() {
            DbBackend::Sqlite => drop_sqlite_triggers(manager).await?,
            _ => drop_postgres_functions_and_forward_keys(manager).await?,
        }

        // Drop in reverse dependency order: versions before their audit
        // entries, and everything holding a foreign key onto `person` or
        // `tree` — the history, `person_distinct`, the `person_denorm`
        // projection — before either of them.
        let tables = [
            MailOutbox::Table.into_table_ref(),
            NotificationPreference::Table.into_table_ref(),
            Notification::Table.into_table_ref(),
            AccessEvent::Table.into_table_ref(),
            ShareLink::Table.into_table_ref(),
            TreeInvitation::Table.into_table_ref(),
            TreeMembership::Table.into_table_ref(),
            AccessScopePerson::Table.into_table_ref(),
            AccessScope::Table.into_table_ref(),
            AppUser::Table.into_table_ref(),
            RecordVersion::Table.into_table_ref(),
            AuditEntry::Table.into_table_ref(),
            PersonDistinct::Table.into_table_ref(),
            BackgroundJob::Table.into_table_ref(),
            PersonDenorm::Table.into_table_ref(),
            ProjectionState::Table.into_table_ref(),
            PersonVisibility::Table.into_table_ref(),
            ExcludedPerson::Table.into_table_ref(),
            ExclusionState::Table.into_table_ref(),
            PrivacyExclusion::Table.into_table_ref(),
            SosaAncestor::Table.into_table_ref(),
            SosaState::Table.into_table_ref(),
            RecordTag::Table.into_table_ref(),
            Tag::Table.into_table_ref(),
            GedcomKept::Table.into_table_ref(),
            Vignette::Table.into_table_ref(),
            Note::Table.into_table_ref(),
            MediaLink::Table.into_table_ref(),
            Media::Table.into_table_ref(),
            Citation::Table.into_table_ref(),
            SourceRepository::Table.into_table_ref(),
            Source::Table.into_table_ref(),
            Repository::Table.into_table_ref(),
            EventSpouseAge::Table.into_table_ref(),
            EventWitness::Table.into_table_ref(),
            Event::Table.into_table_ref(),
            Place::Table.into_table_ref(),
            FamilyChild::Table.into_table_ref(),
            FamilySpouse::Table.into_table_ref(),
            Family::Table.into_table_ref(),
            PersonName::Table.into_table_ref(),
            Person::Table.into_table_ref(),
            Tree::Table.into_table_ref(),
        ];
        for table in tables {
            manager
                .drop_table(Table::drop().table(table).if_exists().to_owned())
                .await?;
        }
        Ok(())
    }
}

/// What a SQLite database says of its own schema, by key: the digest of
/// the schema the file was created with (`oxidgene_db::repo::schema_state`).
/// A PostgreSQL database's schema is not checked, and has no such table.
async fn create_schema_meta(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    if manager.get_database_backend() != DbBackend::Sqlite {
        return Ok(());
    }
    manager
        .create_table(
            Table::create()
                .table(SchemaMeta::Table)
                .if_not_exists()
                .col(string(SchemaMeta::Key).primary_key())
                .col(text(SchemaMeta::Value))
                .to_owned(),
        )
        .await
}

/// Drop every trigger, the first step of `down` under SQLite, which
/// compiles, with each `DROP TABLE`, the foreign-key actions of the tables
/// naming it and the triggers those actions fire: a trigger of a table
/// still standing that writes one already dropped fails the statement.
async fn drop_sqlite_triggers(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    let conn = manager.get_connection();
    let triggers = conn
        .query_all_raw(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT name FROM sqlite_master WHERE type = 'trigger'",
        ))
        .await?;
    for trigger in triggers {
        let name: String = trigger.try_get("", "name")?;
        conn.execute_raw(Statement::from_string(
            DbBackend::Sqlite,
            format!("DROP TRIGGER IF EXISTS \"{name}\""),
        ))
        .await?;
    }
    Ok(())
}

/// The first step of `down` under PostgreSQL: the trigger functions, which
/// PostgreSQL keeps when their tables go, and the [`FORWARD_KEYS`], without
/// which the tables they name could not be dropped before the ones holding
/// them.
async fn drop_postgres_functions_and_forward_keys(
    manager: &SchemaManager<'_>,
) -> Result<(), DbErr> {
    let conn = manager.get_connection();
    let backend = manager.get_database_backend();
    let functions = SOSA_TRIGGER_FUNCTIONS
        .iter()
        .chain(PROJECTION_TRIGGER_FUNCTIONS)
        .map(|function| format!("{function}()"))
        .chain(REDACTION_TRIGGER_FUNCTIONS.iter().map(|f| (*f).to_owned()));
    for function in functions {
        conn.execute_raw(Statement::from_string(
            backend,
            format!("DROP FUNCTION IF EXISTS {function} CASCADE"),
        ))
        .await?;
    }
    for key in FORWARD_KEYS {
        conn.execute_raw(Statement::from_string(
            backend,
            format!(
                "ALTER TABLE IF EXISTS {} DROP CONSTRAINT IF EXISTS {}",
                key.table, key.name
            ),
        ))
        .await?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Iden enums for table/column names
// ---------------------------------------------------------------------------

#[derive(DeriveIden)]
enum AppUser {
    Table,
    Id,
    Issuer,
    Subject,
    Email,
    EmailVerified,
    ProviderName,
    Username,
    DisplayName,
    Locale,
    AvatarSha256,
    AvatarUpdatedAt,
    InstanceRole,
    CanCreateTrees,
    Status,
    CreatedAt,
    UpdatedAt,
    LastSignInAt,
    DeactivatedAt,
    ErasedAt,
}

#[derive(DeriveIden)]
enum TreeMembership {
    Table,
    Id,
    TreeId,
    UserId,
    Role,
    ScopeId,
    SelfPersonId,
    ExpiresAt,
    CreatedBy,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum AccessScope {
    Table,
    Id,
    TreeId,
    Definition,
    BuiltGeneration,
    CreatedAt,
}

#[derive(DeriveIden)]
enum AccessScopePerson {
    Table,
    TreeId,
    ScopeId,
    PersonId,
}

#[derive(DeriveIden)]
enum TreeInvitation {
    Table,
    Id,
    TreeId,
    Email,
    Role,
    ScopeId,
    MembershipExpiresAt,
    ExpiresAt,
    CreatedBy,
    CreatedAt,
    LastSentAt,
    AcceptedAt,
    AcceptedBy,
    DeclinedAt,
    RevokedAt,
}

#[derive(DeriveIden)]
enum ShareLink {
    Table,
    Id,
    TreeId,
    TokenHash,
    Label,
    ScopeId,
    ExpiresAt,
    CreatedBy,
    CreatedAt,
    RevokedAt,
    LastUsedAt,
    UseCount,
}

#[derive(DeriveIden)]
enum Notification {
    Table,
    Id,
    UserId,
    Kind,
    TreeId,
    ActorUserId,
    SubjectId,
    Count,
    CreatedAt,
    ReadAt,
}

#[derive(DeriveIden)]
enum NotificationPreference {
    Table,
    UserId,
    Kind,
    Email,
}

#[derive(DeriveIden)]
enum MailOutbox {
    Table,
    Id,
    UserId,
    ToAddress,
    Kind,
    Language,
    Params,
    Status,
    Attempts,
    NextAttemptAt,
    LeaseUntil,
    SentAt,
    ErrorCategory,
    CreatedAt,
}

#[derive(DeriveIden)]
enum AccessEvent {
    Table,
    Id,
    OccurredAt,
    Kind,
    PrincipalKind,
    UserId,
    ApiClientId,
    ShareLinkId,
    ClientId,
    TreeId,
    Route,
    Permission,
    Outcome,
    RequestId,
}

#[derive(DeriveIden)]
enum Tree {
    Table,
    Id,
    Name,
    Description,
    SosaRootPersonId,
    SelfPersonId,
    DefaultPrivacy,
    ContemporaryRule,
    ContemporaryYears,
    HiddenDisplay,
    HiddenNavigation,
    ContemporaryPhotos,
    GuestsMayExport,
    Visibility,
    VisitorScopeId,
    SubmitterName,
    SubmitterEmail,
    SubmitterAddress,
    AnomalyThresholds,
    TrustedMediaHosts,
    CreatedAt,
    UpdatedAt,
    DeletedAt,
}

#[derive(DeriveIden)]
enum Person {
    Table,
    Id,
    TreeId,
    Sex,
    Privacy,
    PortraitMediaId,
    PortraitVignetteId,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum PersonName {
    Table,
    SurnamePrefix,
    SortOrder,
    Id,
    TreeId,
    PersonId,
    NameType,
    GivenNames,
    Surname,
    Prefix,
    Suffix,
    Nickname,
    IsPrimary,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Family {
    Table,
    Id,
    TreeId,
    Privacy,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum FamilySpouse {
    Table,
    Id,
    TreeId,
    FamilyId,
    PersonId,
    Role,
    SortOrder,
}

#[derive(DeriveIden)]
enum FamilyChild {
    Table,
    Id,
    TreeId,
    FamilyId,
    PersonId,
    ChildType,
    SortOrder,
}

#[derive(DeriveIden)]
enum Place {
    Table,
    Id,
    TreeId,
    Name,
    Latitude,
    Longitude,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
#[expect(
    clippy::enum_variant_names,
    reason = "each identifier names a column of the event table: event_type, date_value, …"
)]
enum Event {
    Table,
    Id,
    TreeId,
    EventType,
    DateValue,
    DateSort,
    DateQualifier,
    DateValue2,
    Calendar,
    Cause,
    Age,
    Agency,
    Classification,
    PlaceId,
    PersonId,
    FamilyId,
    RelatedFamilyId,
    Description,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum EventSpouseAge {
    Table,
    Id,
    TreeId,
    EventId,
    FamilySpouseId,
    Age,
}

#[derive(DeriveIden)]
enum EventWitness {
    Table,
    Id,
    TreeId,
    EventId,
    PersonId,
    Relation,
    SortOrder,
}

#[derive(DeriveIden)]
enum Source {
    Table,
    Id,
    TreeId,
    Title,
    Author,
    Publisher,
    Abbreviation,
    Agency,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Repository {
    Table,
    Id,
    TreeId,
    Name,
    Address,
    Phone,
    Email,
    Website,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum SourceRepository {
    Table,
    Id,
    TreeId,
    SourceId,
    RepositoryId,
    CallNumber,
    MediaType,
    SortOrder,
}

#[derive(DeriveIden)]
enum Citation {
    Table,
    Id,
    TreeId,
    SourceId,
    PersonId,
    EventId,
    FamilyId,
    Page,
    Confidence,
    Text,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Media {
    Table,
    Id,
    TreeId,
    FileName,
    MimeType,
    FilePath,
    StorageKey,
    Sha256,
    ThumbnailKey,
    CardKey,
    ThumbnailUrl,
    Width,
    Height,
    PageCount,
    ParentMediaId,
    PageIndex,
    FileSize,
    Title,
    Description,
    DateValue,
    DateSort,
    DateQualifier,
    DateValue2,
    Calendar,
    SourceMediaType,
    DocumentCategory,
    PlaceId,
    Privacy,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum MediaLink {
    Table,
    Id,
    TreeId,
    MediaId,
    PersonId,
    EventId,
    SourceId,
    FamilyId,
    SortOrder,
}

#[derive(DeriveIden)]
enum Note {
    Table,
    Id,
    TreeId,
    Text,
    PersonId,
    EventId,
    FamilyId,
    SourceId,
    MediaId,
    RepositoryId,
    SharedGroup,
    Confidential,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum PersonDenorm {
    Table,
    PersonId,
    TreeId,
    Payload,
    SchemaVersion,
    UpdatedAt,
    Generation,
}

#[derive(DeriveIden)]
enum SosaAncestor {
    Table,
    PersonId,
    TreeId,
    SosaNumber,
}

#[derive(DeriveIden)]
enum ProjectionState {
    Table,
    TreeId,
    Generation,
    RedactionGeneration,
}

#[derive(DeriveIden)]
enum PersonVisibility {
    Table,
    PersonId,
    TreeId,
    Privacy,
    Deceased,
    LatestBirthYear,
    EarliestBirthYear,
    RecordedBirthYear,
    KinBirthYear,
    Descent,
    RuleVersion,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum PrivacyExclusion {
    Table,
    Id,
    TreeId,
    Kind,
    PersonId,
    Depth,
    IncludeRoot,
    IncludeSpouses,
    CreatedBy,
    CreatedAt,
}

#[derive(DeriveIden)]
enum ExcludedPerson {
    Table,
    TreeId,
    PersonId,
}

#[derive(DeriveIden)]
enum ExclusionState {
    Table,
    TreeId,
    BuiltGeneration,
}

#[derive(DeriveIden)]
enum SchemaMeta {
    Table,
    Key,
    Value,
}

#[derive(DeriveIden)]
enum SosaState {
    Table,
    TreeId,
    Generation,
    BuiltGeneration,
}

#[derive(DeriveIden)]
enum GedcomKept {
    Table,
    Id,
    TreeId,
    EntityKind,
    EntityId,
    Nodes,
    CreatedAt,
}

#[derive(DeriveIden)]
enum Vignette {
    Table,
    Id,
    TreeId,
    MediaId,
    X,
    Y,
    Width,
    Height,
    PersonId,
    EventId,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Tag {
    Table,
    Id,
    TreeId,
    Label,
    NormalizedLabel,
    Color,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum RecordTag {
    Table,
    Id,
    TreeId,
    TagId,
    PersonId,
    FamilyId,
    EventId,
    PlaceId,
    SourceId,
    CitationId,
    RepositoryId,
    MediaId,
    NoteId,
    CreatedAt,
}

#[derive(DeriveIden)]
enum BackgroundJob {
    Table,
    Id,
    TreeId,
    ActiveTreeId,
    Kind,
    Format,
    Status,
    Phase,
    SourceKey,
    ArtifactKey,
    PayloadJson,
    OriginalFilename,
    IncludeNotesAndSources,
    IncludeMedia,
    Done,
    Total,
    Attempt,
    InterruptedPhase,
    LeaseOwner,
    LeaseUntil,
    CancelRequested,
    ResultJson,
    ErrorCode,
    TraceParent,
    TraceState,
    ActorIssuer,
    ActorSubject,
    ActorUserId,
    ActorClientId,
    ActorApiClientId,
    ActorScopes,
    ActorSession,
    CreatedAt,
    UpdatedAt,
    StartedAt,
    FinishedAt,
}

/// The genealogy itself: trees and every record kind they hold.
async fn create_records(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    create_persons(manager).await?;
    create_families(manager).await?;
    create_events(manager).await?;
    create_sources(manager).await?;
    create_media(manager).await?;
    create_notes_and_vignettes(manager).await?;
    create_tags(manager).await?;
    create_gedcom_kept(manager).await
}

/// What is derived from or kept beside the records: jobs and search,
/// traversal indexes, projections, SOSA numbers, distinct pairs, history.
async fn create_derived(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    create_jobs_and_search(manager).await?;
    create_traversal_indexes(manager).await?;
    create_projections(manager).await?;
    create_sosa(manager).await?;
    create_person_visibility(manager).await?;
    create_exclusions(manager).await?;
    create_person_distinct(manager).await?;
    create_history(manager).await
}

/// The tree, its persons and their names.
async fn create_persons(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    // 1. tree (root table, no FKs)
    manager
        .create_table(with_forward_keys(
            manager,
            "tree",
            Table::create()
                .table(Tree::Table)
                .if_not_exists()
                .col(uuid(Tree::Id).primary_key())
                .col(string(Tree::Name))
                .col(string_null(Tree::Description))
                // Both name a person, by a forward key (`FORWARD_KEYS`).
                .col(uuid_null(Tree::SosaRootPersonId))
                .col(uuid_null(Tree::SelfPersonId))
                .col(
                    ColumnDef::new(Tree::DefaultPrivacy)
                        .string_len(10)
                        .not_null()
                        .default("public"),
                )
                // The privacy settings (docs/privacy.md §9).
                .col(boolean(Tree::ContemporaryRule).default(true))
                .col(integer(Tree::ContemporaryYears).default(100))
                .col(
                    ColumnDef::new(Tree::HiddenDisplay)
                        .string_len(10)
                        .not_null()
                        .default("stub"),
                )
                .col(boolean(Tree::HiddenNavigation).default(true))
                .col(boolean(Tree::ContemporaryPhotos).default(false))
                .col(boolean(Tree::GuestsMayExport).default(false))
                // Who is a Visitor (docs/authorization.md §6.4), and the
                // access scope Visitors are limited to, by a forward key
                // (`FORWARD_KEYS`): the sharing service replaces it, then
                // deletes the former one.
                .col(string_len(Tree::Visibility, 8).default("private"))
                .check(one_of("visibility", &["private", "site", "public"]))
                .col(uuid_null(Tree::VisitorScopeId))
                // Who the tree's GEDCOM exports say they are from (`SUBM`).
                .col(string_null(Tree::SubmitterName))
                .col(string_null(Tree::SubmitterEmail))
                .col(text_null(Tree::SubmitterAddress))
                // The anomaly thresholds the tree sets, as JSON; none, the
                // defaults (`AnomalyThresholds`).
                .col(text_null(Tree::AnomalyThresholds))
                // The hosts whose pictures the tree's readers load without
                // being asked, as a JSON array; none, no host
                // (docs/cross-cutting.md §7.2).
                .col(text_null(Tree::TrustedMediaHosts))
                .col(timestamp_with_time_zone(Tree::CreatedAt))
                .col(timestamp_with_time_zone(Tree::UpdatedAt))
                .col(timestamp_with_time_zone_null(Tree::DeletedAt))
                .check(enum_check::<sea_enums::TreeDefaultPrivacy>(
                    "tree",
                    "default_privacy",
                ))
                .check(enum_check::<sea_enums::HiddenDisplay>(
                    "tree",
                    "hidden_display",
                ))
                .to_owned(),
        ))
        .await?;

    // 2. person (FK → tree)
    manager
        .create_table(with_forward_keys(
            manager,
            "person",
            Table::create()
                .table(Person::Table)
                .if_not_exists()
                .col(uuid(Person::Id).primary_key())
                .col(uuid(Person::TreeId))
                .col(string_len(Person::Sex, 10))
                .col(
                    ColumnDef::new(Person::Privacy)
                        .string_len(10)
                        .not_null()
                        .default("default"),
                )
                .col(uuid_null(Person::PortraitMediaId))
                .col(uuid_null(Person::PortraitVignetteId))
                .col(timestamp_with_time_zone(Person::CreatedAt))
                .col(timestamp_with_time_zone(Person::UpdatedAt))
                .check(enum_check::<sea_enums::Sex>("person", "sex"))
                .check(enum_check::<sea_enums::Privacy>("person", "privacy"))
                // A portrait is a whole medium or a crop, never both.
                .check(at_most_one(
                    "person",
                    "portrait",
                    &["portrait_media_id", "portrait_vignette_id"],
                ))
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_person_tree")
                        .from(Person::Table, Person::TreeId)
                        .to(Tree::Table, Tree::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        ))
        .await?;
    // `(tree_id, id)` is unique so that a row naming this record can pair it
    // with its own tree in a composite foreign key: it cannot name a record
    // of another tree (docs/data-model.md, Integrity constraints). The index
    // also reads the tree's records in id order, a page of a list.
    manager
        .create_index(
            Index::create()
                .name("uq_person_tree_id")
                .table(Person::Table)
                .col(Person::TreeId)
                .col(Person::Id)
                .unique()
                .to_owned(),
        )
        .await?;
    manager
        .create_index(
            Index::create()
                .name("idx_person_portrait_media_id")
                .table(Person::Table)
                .col(Person::PortraitMediaId)
                .to_owned(),
        )
        .await?;
    manager
        .create_index(
            Index::create()
                .name("idx_person_portrait_vignette_id")
                .table(Person::Table)
                .col(Person::PortraitVignetteId)
                .to_owned(),
        )
        .await?;

    // 3. person_name (FK → person)
    manager
        .create_table(
            Table::create()
                .table(PersonName::Table)
                .if_not_exists()
                .col(uuid(PersonName::Id).primary_key())
                .col(uuid(PersonName::TreeId))
                .col(uuid(PersonName::PersonId))
                .col(string_len(PersonName::NameType, 20))
                .col(string_null(PersonName::GivenNames))
                .col(string_null(PersonName::Surname))
                .col(string_null(PersonName::SurnamePrefix))
                .col(string_null(PersonName::Prefix))
                .col(string_null(PersonName::Suffix))
                .col(string_null(PersonName::Nickname))
                .col(boolean(PersonName::IsPrimary))
                .col(integer(PersonName::SortOrder).default(0))
                .col(timestamp_with_time_zone(PersonName::CreatedAt))
                .col(timestamp_with_time_zone(PersonName::UpdatedAt))
                .check(enum_check::<sea_enums::NameType>(
                    "person_name",
                    "name_type",
                ))
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_person_name_person")
                        .from(
                            PersonName::Table,
                            (PersonName::PersonId, PersonName::TreeId),
                        )
                        .to(Person::Table, (Person::Id, Person::TreeId))
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;
    manager
        .create_index(
            Index::create()
                .name("idx_person_name_person_id")
                .table(PersonName::Table)
                .col(PersonName::PersonId)
                .to_owned(),
        )
        .await?;
    // A person has at most one primary name: a partial unique index, which
    // SQLite and PostgreSQL both support and sea-query does not express.
    manager
        .get_connection()
        .execute_raw(Statement::from_string(
            manager.get_database_backend(),
            "CREATE UNIQUE INDEX IF NOT EXISTS uq_person_name_primary \
             ON person_name (person_id) WHERE is_primary",
        ))
        .await?;
    // The persons of a tree bearing a surname: the dictionary's usage list
    // and descent. Leading with the tree, it reads one tree's names, not
    // every tree's.
    manager
        .create_index(
            Index::create()
                .name("idx_person_name_tree_surname")
                .table(PersonName::Table)
                .col(PersonName::TreeId)
                .col(PersonName::Surname)
                .to_owned(),
        )
        .await?;
    Ok(())
}

/// Families and their spouses and children.
async fn create_families(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    // 4. family (FK → tree)
    manager
        .create_table(
            Table::create()
                .table(Family::Table)
                .if_not_exists()
                .col(uuid(Family::Id).primary_key())
                .col(uuid(Family::TreeId))
                .col(
                    ColumnDef::new(Family::Privacy)
                        .string_len(10)
                        .not_null()
                        .default("default"),
                )
                .col(timestamp_with_time_zone(Family::CreatedAt))
                .col(timestamp_with_time_zone(Family::UpdatedAt))
                .check(enum_check::<sea_enums::Privacy>("family", "privacy"))
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_family_tree")
                        .from(Family::Table, Family::TreeId)
                        .to(Tree::Table, Tree::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;
    // `(tree_id, id)` is unique so that a row naming this record can pair it
    // with its own tree in a composite foreign key: it cannot name a record
    // of another tree (docs/data-model.md, Integrity constraints). The index
    // also reads the tree's records in id order, a page of a list.
    manager
        .create_index(
            Index::create()
                .name("uq_family_tree_id")
                .table(Family::Table)
                .col(Family::TreeId)
                .col(Family::Id)
                .unique()
                .to_owned(),
        )
        .await?;

    // 5. family_spouse (FK → family, person)
    manager
        .create_table(
            Table::create()
                .table(FamilySpouse::Table)
                .if_not_exists()
                .col(uuid(FamilySpouse::Id).primary_key())
                .col(uuid(FamilySpouse::TreeId))
                .col(uuid(FamilySpouse::FamilyId))
                .col(uuid(FamilySpouse::PersonId))
                .col(string_len(FamilySpouse::Role, 10))
                .col(integer(FamilySpouse::SortOrder))
                .check(enum_check::<sea_enums::SpouseRole>("family_spouse", "role"))
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_family_spouse_family")
                        .from(
                            FamilySpouse::Table,
                            (FamilySpouse::FamilyId, FamilySpouse::TreeId),
                        )
                        .to(Family::Table, (Family::Id, Family::TreeId))
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_family_spouse_person")
                        .from(
                            FamilySpouse::Table,
                            (FamilySpouse::PersonId, FamilySpouse::TreeId),
                        )
                        .to(Person::Table, (Person::Id, Person::TreeId))
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;
    // A person is a spouse of a family once, whatever the role. The index
    // leads with the family, so it also reads a family's spouses.
    manager
        .create_index(
            Index::create()
                .name("uq_family_spouse_family_person")
                .table(FamilySpouse::Table)
                .col(FamilySpouse::FamilyId)
                .col(FamilySpouse::PersonId)
                .unique()
                .to_owned(),
        )
        .await?;
    // The target of `event_spouse_age`'s composite foreign key.
    manager
        .create_index(
            Index::create()
                .name("uq_family_spouse_tree_id")
                .table(FamilySpouse::Table)
                .col(FamilySpouse::TreeId)
                .col(FamilySpouse::Id)
                .unique()
                .to_owned(),
        )
        .await?;
    manager
        .create_index(
            Index::create()
                .name("idx_family_spouse_person_id")
                .table(FamilySpouse::Table)
                .col(FamilySpouse::PersonId)
                .to_owned(),
        )
        .await?;

    // 6. family_child (FK → family, person)
    manager
        .create_table(
            Table::create()
                .table(FamilyChild::Table)
                .if_not_exists()
                .col(uuid(FamilyChild::Id).primary_key())
                .col(uuid(FamilyChild::TreeId))
                .col(uuid(FamilyChild::FamilyId))
                .col(uuid(FamilyChild::PersonId))
                .col(string_len(FamilyChild::ChildType, 15))
                .col(integer(FamilyChild::SortOrder))
                .check(enum_check::<sea_enums::ChildType>(
                    "family_child",
                    "child_type",
                ))
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_family_child_family")
                        .from(
                            FamilyChild::Table,
                            (FamilyChild::FamilyId, FamilyChild::TreeId),
                        )
                        .to(Family::Table, (Family::Id, Family::TreeId))
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_family_child_person")
                        .from(
                            FamilyChild::Table,
                            (FamilyChild::PersonId, FamilyChild::TreeId),
                        )
                        .to(Person::Table, (Person::Id, Person::TreeId))
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;
    // A person is a child of a family once. The index leads with the
    // family, so it also reads a family's children.
    manager
        .create_index(
            Index::create()
                .name("uq_family_child_family_person")
                .table(FamilyChild::Table)
                .col(FamilyChild::FamilyId)
                .col(FamilyChild::PersonId)
                .unique()
                .to_owned(),
        )
        .await?;
    manager
        .create_index(
            Index::create()
                .name("idx_family_child_person_id")
                .table(FamilyChild::Table)
                .col(FamilyChild::PersonId)
                .to_owned(),
        )
        .await?;
    Ok(())
}

/// Places, events and their witnesses.
async fn create_events(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    // 7. place (FK → tree)
    manager
        .create_table(
            Table::create()
                .table(Place::Table)
                .if_not_exists()
                .col(uuid(Place::Id).primary_key())
                .col(uuid(Place::TreeId))
                .col(string(Place::Name))
                .col(double_null(Place::Latitude))
                .col(double_null(Place::Longitude))
                .col(timestamp_with_time_zone(Place::CreatedAt))
                .col(timestamp_with_time_zone(Place::UpdatedAt))
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_place_tree")
                        .from(Place::Table, Place::TreeId)
                        .to(Tree::Table, Tree::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;
    // `(tree_id, id)` is unique so that a row naming this record can pair it
    // with its own tree in a composite foreign key: it cannot name a record
    // of another tree (docs/data-model.md, Integrity constraints). The index
    // also reads the tree's records in id order, a page of a list.
    manager
        .create_index(
            Index::create()
                .name("uq_place_tree_id")
                .table(Place::Table)
                .col(Place::TreeId)
                .col(Place::Id)
                .unique()
                .to_owned(),
        )
        .await?;

    // 8. event (FK → tree, place?, person?, family?)
    manager
        .create_table(
            Table::create()
                .table(Event::Table)
                .if_not_exists()
                .col(uuid(Event::Id).primary_key())
                .col(uuid(Event::TreeId))
                .col(string_len(Event::EventType, 25))
                .col(string_null(Event::DateValue))
                .col(date_null(Event::DateSort))
                .col(
                    ColumnDef::new(Event::DateQualifier)
                        .string_len(10)
                        .not_null()
                        .default("exact"),
                )
                .col(string_null(Event::DateValue2))
                .col(
                    ColumnDef::new(Event::Calendar)
                        .string_len(20)
                        .not_null()
                        .default("gregorian"),
                )
                .col(string_null(Event::Cause))
                .col(string_null(Event::Age))
                .col(string_null(Event::Agency))
                .col(string_null(Event::Classification))
                .col(uuid_null(Event::PlaceId))
                .col(uuid_null(Event::PersonId))
                .col(uuid_null(Event::FamilyId))
                .col(uuid_null(Event::RelatedFamilyId))
                .col(string_null(Event::Description))
                .col(timestamp_with_time_zone(Event::CreatedAt))
                .col(timestamp_with_time_zone(Event::UpdatedAt))
                .check(enum_check::<sea_enums::EventType>("event", "event_type"))
                .check(enum_check::<sea_enums::DateQualifier>(
                    "event",
                    "date_qualifier",
                ))
                .check(enum_check::<sea_enums::Calendar>("event", "calendar"))
                // An individual event or a family event, never both.
                .check(at_most_one("event", "owner", &["person_id", "family_id"]))
                // Only an individual event names a family it does not
                // belong to.
                .check((
                    Alias::new("ck_event_related_family"),
                    Expr::cust(
                        "related_family_id IS NULL \
                         OR (person_id IS NOT NULL AND family_id IS NULL)",
                    ),
                ))
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_event_tree")
                        .from(Event::Table, Event::TreeId)
                        .to(Tree::Table, Tree::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_event_place")
                        .from(Event::Table, Event::PlaceId)
                        .to(Place::Table, Place::Id)
                        .on_delete(ForeignKeyAction::SetNull),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_event_person")
                        .from(Event::Table, (Event::PersonId, Event::TreeId))
                        .to(Person::Table, (Person::Id, Person::TreeId))
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_event_family")
                        .from(Event::Table, (Event::FamilyId, Event::TreeId))
                        .to(Family::Table, (Family::Id, Family::TreeId))
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_event_related_family")
                        .from(Event::Table, Event::RelatedFamilyId)
                        .to(Family::Table, Family::Id)
                        .on_delete(ForeignKeyAction::SetNull),
                )
                .to_owned(),
        )
        .await?;
    // `(tree_id, id)` is unique so that a row naming this record can pair it
    // with its own tree in a composite foreign key: it cannot name a record
    // of another tree (docs/data-model.md, Integrity constraints). The index
    // also reads the tree's records in id order, a page of a list.
    manager
        .create_index(
            Index::create()
                .name("uq_event_tree_id")
                .table(Event::Table)
                .col(Event::TreeId)
                .col(Event::Id)
                .unique()
                .to_owned(),
        )
        .await?;
    manager
        .create_index(
            Index::create()
                .name("idx_event_person_id")
                .table(Event::Table)
                .col(Event::PersonId)
                .to_owned(),
        )
        .await?;
    manager
        .create_index(
            Index::create()
                .name("idx_event_family_id")
                .table(Event::Table)
                .col(Event::FamilyId)
                .to_owned(),
        )
        .await?;
    manager
        .create_index(
            Index::create()
                .name("idx_event_related_family_id")
                .table(Event::Table)
                .col(Event::RelatedFamilyId)
                .to_owned(),
        )
        .await?;

    // 9. event_witness (FK → event, person)
    manager
        .create_table(
            Table::create()
                .table(EventWitness::Table)
                .if_not_exists()
                .col(uuid(EventWitness::Id).primary_key())
                .col(uuid(EventWitness::TreeId))
                .col(uuid(EventWitness::EventId))
                .col(uuid(EventWitness::PersonId))
                .col(string_null(EventWitness::Relation))
                .col(integer(EventWitness::SortOrder))
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_event_witness_event")
                        .from(
                            EventWitness::Table,
                            (EventWitness::EventId, EventWitness::TreeId),
                        )
                        .to(Event::Table, (Event::Id, Event::TreeId))
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_event_witness_person")
                        .from(
                            EventWitness::Table,
                            (EventWitness::PersonId, EventWitness::TreeId),
                        )
                        .to(Person::Table, (Person::Id, Person::TreeId))
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;
    // A person witnesses an event once per relation, a missing relation
    // counting as one: an expression index, which SQLite and PostgreSQL both
    // support and sea-query does not express. It leads with the event, so it
    // also reads an event's witnesses.
    manager
        .get_connection()
        .execute_raw(Statement::from_string(
            manager.get_database_backend(),
            "CREATE UNIQUE INDEX IF NOT EXISTS uq_event_witness_event_person_relation \
             ON event_witness (event_id, person_id, (COALESCE(relation, '')))",
        ))
        .await?;
    manager
        .create_index(
            Index::create()
                .name("idx_event_witness_person_id")
                .table(EventWitness::Table)
                .col(EventWitness::PersonId)
                .to_owned(),
        )
        .await?;

    // 10. event_spouse_age (FK → event, family_spouse): a family event's
    // age of one spouse, keyed by the membership so it survives a merge.
    manager
        .create_table(
            Table::create()
                .table(EventSpouseAge::Table)
                .if_not_exists()
                .col(uuid(EventSpouseAge::Id).primary_key())
                .col(uuid(EventSpouseAge::TreeId))
                .col(uuid(EventSpouseAge::EventId))
                .col(uuid(EventSpouseAge::FamilySpouseId))
                .col(string(EventSpouseAge::Age))
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_event_spouse_age_event")
                        .from(
                            EventSpouseAge::Table,
                            (EventSpouseAge::EventId, EventSpouseAge::TreeId),
                        )
                        .to(Event::Table, (Event::Id, Event::TreeId))
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_event_spouse_age_family_spouse")
                        .from(
                            EventSpouseAge::Table,
                            (EventSpouseAge::FamilySpouseId, EventSpouseAge::TreeId),
                        )
                        .to(
                            FamilySpouse::Table,
                            (FamilySpouse::Id, FamilySpouse::TreeId),
                        )
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;
    manager
        .create_index(
            Index::create()
                .name("idx_event_spouse_age_event_spouse")
                .table(EventSpouseAge::Table)
                .col(EventSpouseAge::EventId)
                .col(EventSpouseAge::FamilySpouseId)
                .unique()
                .to_owned(),
        )
        .await?;
    manager
        .create_index(
            Index::create()
                .name("idx_event_spouse_age_family_spouse_id")
                .table(EventSpouseAge::Table)
                .col(EventSpouseAge::FamilySpouseId)
                .to_owned(),
        )
        .await?;
    Ok(())
}

/// Sources and their citations.
async fn create_sources(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    // 10. source (FK → tree)
    manager
        .create_table(
            Table::create()
                .table(Source::Table)
                .if_not_exists()
                .col(uuid(Source::Id).primary_key())
                .col(uuid(Source::TreeId))
                .col(string(Source::Title))
                .col(string_null(Source::Author))
                .col(string_null(Source::Publisher))
                .col(string_null(Source::Abbreviation))
                .col(string_null(Source::Agency))
                .col(timestamp_with_time_zone(Source::CreatedAt))
                .col(timestamp_with_time_zone(Source::UpdatedAt))
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_source_tree")
                        .from(Source::Table, Source::TreeId)
                        .to(Tree::Table, Tree::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;
    // `(tree_id, id)` is unique so that a row naming this record can pair it
    // with its own tree in a composite foreign key: it cannot name a record
    // of another tree (docs/data-model.md, Integrity constraints). The index
    // also reads the tree's records in id order, a page of a list.
    manager
        .create_index(
            Index::create()
                .name("uq_source_tree_id")
                .table(Source::Table)
                .col(Source::TreeId)
                .col(Source::Id)
                .unique()
                .to_owned(),
        )
        .await?;

    // 11. citation (FK → source, person?, event?, family?)
    manager
        .create_table(
            Table::create()
                .table(Citation::Table)
                .if_not_exists()
                .col(uuid(Citation::Id).primary_key())
                .col(uuid(Citation::TreeId))
                .col(uuid(Citation::SourceId))
                .col(uuid_null(Citation::PersonId))
                .col(uuid_null(Citation::EventId))
                .col(uuid_null(Citation::FamilyId))
                .col(string_null(Citation::Page))
                .col(string_len_null(Citation::Confidence, 10))
                .col(text_null(Citation::Text))
                .col(timestamp_with_time_zone(Citation::CreatedAt))
                .col(timestamp_with_time_zone(Citation::UpdatedAt))
                .check(enum_check::<sea_enums::Confidence>(
                    "citation",
                    "confidence",
                ))
                // A citation hangs off one record.
                .check(exactly_one(
                    "citation",
                    "owner",
                    &["person_id", "event_id", "family_id"],
                ))
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_citation_source")
                        .from(Citation::Table, (Citation::SourceId, Citation::TreeId))
                        .to(Source::Table, (Source::Id, Source::TreeId))
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_citation_person")
                        .from(Citation::Table, (Citation::PersonId, Citation::TreeId))
                        .to(Person::Table, (Person::Id, Person::TreeId))
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_citation_event")
                        .from(Citation::Table, (Citation::EventId, Citation::TreeId))
                        .to(Event::Table, (Event::Id, Event::TreeId))
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_citation_family")
                        .from(Citation::Table, (Citation::FamilyId, Citation::TreeId))
                        .to(Family::Table, (Family::Id, Family::TreeId))
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;
    // `(tree_id, id)` is unique so that a row naming this record can pair it
    // with its own tree in a composite foreign key: it cannot name a record
    // of another tree (docs/data-model.md, Integrity constraints). The index
    // also reads the tree's records in id order, a page of a list.
    manager
        .create_index(
            Index::create()
                .name("uq_citation_tree_id")
                .table(Citation::Table)
                .col(Citation::TreeId)
                .col(Citation::Id)
                .unique()
                .to_owned(),
        )
        .await?;
    manager
        .create_index(
            Index::create()
                .name("idx_citation_source_id")
                .table(Citation::Table)
                .col(Citation::SourceId)
                .to_owned(),
        )
        .await?;
    create_repositories(manager).await
}

/// Repositories, and the links saying which sources each holds.
async fn create_repositories(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    // 12. repository (FK → tree)
    manager
        .create_table(
            Table::create()
                .table(Repository::Table)
                .if_not_exists()
                .col(uuid(Repository::Id).primary_key())
                .col(uuid(Repository::TreeId))
                .col(string(Repository::Name))
                .col(text_null(Repository::Address))
                .col(string_null(Repository::Phone))
                .col(string_null(Repository::Email))
                .col(string_null(Repository::Website))
                .col(timestamp_with_time_zone(Repository::CreatedAt))
                .col(timestamp_with_time_zone(Repository::UpdatedAt))
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_repository_tree")
                        .from(Repository::Table, Repository::TreeId)
                        .to(Tree::Table, Tree::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;
    // `(tree_id, id)` is unique so that a row naming this record can pair it
    // with its own tree in a composite foreign key: it cannot name a record
    // of another tree (docs/data-model.md, Integrity constraints). The index
    // also reads the tree's records in id order, a page of a list.
    manager
        .create_index(
            Index::create()
                .name("uq_repository_tree_id")
                .table(Repository::Table)
                .col(Repository::TreeId)
                .col(Repository::Id)
                .unique()
                .to_owned(),
        )
        .await?;

    // 13. source_repository (FK → source, repository): one row per call
    // number a source has at a repository.
    manager
        .create_table(
            Table::create()
                .table(SourceRepository::Table)
                .if_not_exists()
                .col(uuid(SourceRepository::Id).primary_key())
                .col(uuid(SourceRepository::TreeId))
                .col(uuid(SourceRepository::SourceId))
                .col(uuid(SourceRepository::RepositoryId))
                .col(string_null(SourceRepository::CallNumber))
                .col(string_len_null(SourceRepository::MediaType, 20))
                .col(integer(SourceRepository::SortOrder))
                .check(enum_check::<sea_enums::SourceMediaType>(
                    "source_repository",
                    "media_type",
                ))
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_source_repository_source")
                        .from(
                            SourceRepository::Table,
                            (SourceRepository::SourceId, SourceRepository::TreeId),
                        )
                        .to(Source::Table, (Source::Id, Source::TreeId))
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_source_repository_repository")
                        .from(
                            SourceRepository::Table,
                            (SourceRepository::RepositoryId, SourceRepository::TreeId),
                        )
                        .to(Repository::Table, (Repository::Id, Repository::TreeId))
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;
    for (name, column) in [
        (
            "idx_source_repository_source_id",
            SourceRepository::SourceId,
        ),
        (
            "idx_source_repository_repository_id",
            SourceRepository::RepositoryId,
        ),
    ] {
        manager
            .create_index(
                Index::create()
                    .name(name)
                    .table(SourceRepository::Table)
                    .col(column)
                    .to_owned(),
            )
            .await?;
    }
    Ok(())
}

/// Media and their links.
async fn create_media(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    // 12. media (FK → tree, media?, place?)
    manager
        .create_table(
            Table::create()
                .table(Media::Table)
                .if_not_exists()
                .col(uuid(Media::Id).primary_key())
                .col(uuid(Media::TreeId))
                .col(string(Media::FileName))
                .col(string(Media::MimeType))
                .col(string(Media::FilePath))
                .col(string_null(Media::StorageKey))
                .col(string_null(Media::Sha256))
                .col(string_null(Media::ThumbnailKey))
                .col(string_null(Media::CardKey))
                .col(string_null(Media::ThumbnailUrl))
                .col(integer_null(Media::Width))
                .col(integer_null(Media::Height))
                .col(integer(Media::PageCount).default(1))
                .col(uuid_null(Media::ParentMediaId))
                .col(integer(Media::PageIndex).default(0))
                .col(big_integer(Media::FileSize))
                .col(string_null(Media::Title))
                .col(string_null(Media::Description))
                .col(string_null(Media::DateValue))
                .col(date_null(Media::DateSort))
                .col(
                    ColumnDef::new(Media::DateQualifier)
                        .string_len(10)
                        .not_null()
                        .default("exact"),
                )
                .col(string_null(Media::DateValue2))
                .col(
                    ColumnDef::new(Media::Calendar)
                        .string_len(20)
                        .not_null()
                        .default("gregorian"),
                )
                .col(
                    ColumnDef::new(Media::SourceMediaType)
                        .string_len(20)
                        .not_null()
                        .default("other"),
                )
                .col(string_null(Media::DocumentCategory))
                .col(uuid_null(Media::PlaceId))
                .col(
                    ColumnDef::new(Media::Privacy)
                        .string_len(10)
                        .not_null()
                        .default("default"),
                )
                .col(timestamp_with_time_zone(Media::CreatedAt))
                .col(timestamp_with_time_zone(Media::UpdatedAt))
                .check(enum_check::<sea_enums::DateQualifier>(
                    "media",
                    "date_qualifier",
                ))
                .check(enum_check::<sea_enums::Calendar>("media", "calendar"))
                .check(enum_check::<sea_enums::SourceMediaType>(
                    "media",
                    "source_media_type",
                ))
                .check(named_one_of(
                    "media",
                    "document_category",
                    &DocumentCategory::ALL.map(DocumentCategory::as_str),
                ))
                .check(enum_check::<sea_enums::Privacy>("media", "privacy"))
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_media_tree")
                        .from(Media::Table, Media::TreeId)
                        .to(Tree::Table, Tree::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                // A page goes with its document. Single-column: the
                // `(tree_id, id)` index a composite key would name is
                // created after the table.
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_media_parent")
                        .from(Media::Table, Media::ParentMediaId)
                        .to(Media::Table, Media::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                // A medium outlives the place it was taken at.
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_media_place")
                        .from(Media::Table, Media::PlaceId)
                        .to(Place::Table, Place::Id)
                        .on_delete(ForeignKeyAction::SetNull),
                )
                .to_owned(),
        )
        .await?;
    // `(tree_id, id)` is unique so that a row naming this record can pair it
    // with its own tree in a composite foreign key: it cannot name a record
    // of another tree (docs/data-model.md, Integrity constraints). The index
    // also reads the tree's records in id order, a page of a list.
    manager
        .create_index(
            Index::create()
                .name("uq_media_tree_id")
                .table(Media::Table)
                .col(Media::TreeId)
                .col(Media::Id)
                .unique()
                .to_owned(),
        )
        .await?;
    manager
        .create_index(
            Index::create()
                .name("idx_media_tree_sha256")
                .table(Media::Table)
                .col(Media::TreeId)
                .col(Media::Sha256)
                .to_owned(),
        )
        .await?;
    manager
        .create_index(
            Index::create()
                .name("idx_media_parent_page")
                .table(Media::Table)
                .col(Media::ParentMediaId)
                .col(Media::PageIndex)
                .to_owned(),
        )
        .await?;

    // 13. media_link (FK → media, person?, event?, source?, family?)
    manager
        .create_table(
            Table::create()
                .table(MediaLink::Table)
                .if_not_exists()
                .col(uuid(MediaLink::Id).primary_key())
                .col(uuid(MediaLink::TreeId))
                .col(uuid(MediaLink::MediaId))
                .col(uuid_null(MediaLink::PersonId))
                .col(uuid_null(MediaLink::EventId))
                .col(uuid_null(MediaLink::SourceId))
                .col(uuid_null(MediaLink::FamilyId))
                .col(integer(MediaLink::SortOrder))
                // A link attaches a medium to one record.
                .check(exactly_one(
                    "media_link",
                    "target",
                    &["person_id", "event_id", "source_id", "family_id"],
                ))
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_media_link_media")
                        .from(MediaLink::Table, (MediaLink::MediaId, MediaLink::TreeId))
                        .to(Media::Table, (Media::Id, Media::TreeId))
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_media_link_person")
                        .from(MediaLink::Table, (MediaLink::PersonId, MediaLink::TreeId))
                        .to(Person::Table, (Person::Id, Person::TreeId))
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_media_link_event")
                        .from(MediaLink::Table, (MediaLink::EventId, MediaLink::TreeId))
                        .to(Event::Table, (Event::Id, Event::TreeId))
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_media_link_source")
                        .from(MediaLink::Table, (MediaLink::SourceId, MediaLink::TreeId))
                        .to(Source::Table, (Source::Id, Source::TreeId))
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_media_link_family")
                        .from(MediaLink::Table, (MediaLink::FamilyId, MediaLink::TreeId))
                        .to(Family::Table, (Family::Id, Family::TreeId))
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;
    manager
        .create_index(
            Index::create()
                .name("idx_media_link_media_id")
                .table(MediaLink::Table)
                .col(MediaLink::MediaId)
                .to_owned(),
        )
        .await?;
    Ok(())
}

/// Notes, vignettes and media tags.
async fn create_notes_and_vignettes(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    // 14. note (FK → tree, person?, event?, family?, source?, media?, repository?)
    manager
        .create_table(
            Table::create()
                .table(Note::Table)
                .if_not_exists()
                .col(uuid(Note::Id).primary_key())
                .col(uuid(Note::TreeId))
                .col(text(Note::Text))
                .col(uuid_null(Note::PersonId))
                .col(uuid_null(Note::EventId))
                .col(uuid_null(Note::FamilyId))
                .col(uuid_null(Note::SourceId))
                .col(uuid_null(Note::MediaId))
                .col(uuid_null(Note::RepositoryId))
                .col(uuid_null(Note::SharedGroup))
                .col(boolean(Note::Confidential).default(false))
                .col(timestamp_with_time_zone(Note::CreatedAt))
                .col(timestamp_with_time_zone(Note::UpdatedAt))
                // A note is about one record.
                .check(exactly_one(
                    "note",
                    "owner",
                    &[
                        "person_id",
                        "event_id",
                        "family_id",
                        "source_id",
                        "media_id",
                        "repository_id",
                    ],
                ))
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_note_tree")
                        .from(Note::Table, Note::TreeId)
                        .to(Tree::Table, Tree::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_note_person")
                        .from(Note::Table, (Note::PersonId, Note::TreeId))
                        .to(Person::Table, (Person::Id, Person::TreeId))
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_note_event")
                        .from(Note::Table, (Note::EventId, Note::TreeId))
                        .to(Event::Table, (Event::Id, Event::TreeId))
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_note_family")
                        .from(Note::Table, (Note::FamilyId, Note::TreeId))
                        .to(Family::Table, (Family::Id, Family::TreeId))
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_note_source")
                        .from(Note::Table, (Note::SourceId, Note::TreeId))
                        .to(Source::Table, (Source::Id, Source::TreeId))
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_note_repository")
                        .from(Note::Table, (Note::RepositoryId, Note::TreeId))
                        .to(Repository::Table, (Repository::Id, Repository::TreeId))
                        .on_delete(ForeignKeyAction::Cascade),
                )
                // A note about a medium has no meaning without it.
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_note_media")
                        .from(Note::Table, (Note::MediaId, Note::TreeId))
                        .to(Media::Table, (Media::Id, Media::TreeId))
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;
    // `(tree_id, id)` is unique so that a row naming this record can pair it
    // with its own tree in a composite foreign key: it cannot name a record
    // of another tree (docs/data-model.md, Integrity constraints). The index
    // also reads the tree's records in id order, a page of a list.
    manager
        .create_index(
            Index::create()
                .name("uq_note_tree_id")
                .table(Note::Table)
                .col(Note::TreeId)
                .col(Note::Id)
                .unique()
                .to_owned(),
        )
        .await?;
    manager
        .create_index(
            Index::create()
                .name("idx_note_media_id")
                .table(Note::Table)
                .col(Note::MediaId)
                .to_owned(),
        )
        .await?;
    manager
        .create_index(
            Index::create()
                .name("idx_note_repository_id")
                .table(Note::Table)
                .col(Note::RepositoryId)
                .to_owned(),
        )
        .await?;
    manager
        .create_index(
            Index::create()
                .name("idx_note_shared_group")
                .table(Note::Table)
                .col(Note::SharedGroup)
                .to_owned(),
        )
        .await?;

    // 15. vignette (FK → media, person?, event?)
    manager
        .create_table(
            Table::create()
                .table(Vignette::Table)
                .if_not_exists()
                .col(uuid(Vignette::Id).primary_key())
                .col(uuid(Vignette::TreeId))
                .col(uuid(Vignette::MediaId))
                .col(integer(Vignette::X))
                .col(integer(Vignette::Y))
                .col(integer(Vignette::Width))
                .col(integer(Vignette::Height))
                .col(uuid_null(Vignette::PersonId))
                .col(uuid_null(Vignette::EventId))
                .col(timestamp_with_time_zone(Vignette::CreatedAt))
                .col(timestamp_with_time_zone(Vignette::UpdatedAt))
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_vignette_media")
                        .from(Vignette::Table, (Vignette::MediaId, Vignette::TreeId))
                        .to(Media::Table, (Media::Id, Media::TreeId))
                        .on_delete(ForeignKeyAction::Cascade),
                )
                // Who the region shows and what it is evidence for are
                // attributions, not owners: deleting the person or the event
                // clears them (`VignetteRepo::detach_*`, in the deletion's
                // transaction) and the region stays on its page. No action
                // here, so a deletion that forgot to clear them is refused
                // rather than taking the region with it; a composite key
                // cannot set only its own column to `NULL` under SQLite.
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_vignette_person")
                        .from(Vignette::Table, (Vignette::PersonId, Vignette::TreeId))
                        .to(Person::Table, (Person::Id, Person::TreeId))
                        .on_delete(ForeignKeyAction::NoAction),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_vignette_event")
                        .from(Vignette::Table, (Vignette::EventId, Vignette::TreeId))
                        .to(Event::Table, (Event::Id, Event::TreeId))
                        .on_delete(ForeignKeyAction::NoAction),
                )
                .to_owned(),
        )
        .await?;
    for (name, column) in [
        ("idx_vignette_media_id", Vignette::MediaId),
        ("idx_vignette_person_id", Vignette::PersonId),
        ("idx_vignette_event_id", Vignette::EventId),
    ] {
        manager
            .create_index(
                Index::create()
                    .name(name)
                    .table(Vignette::Table)
                    .col(column)
                    .to_owned(),
            )
            .await?;
    }

    Ok(())
}

/// The tree's tags and the records they are put on (docs/data-model.md, RecordTag).
///
/// `record_tag` has one nullable foreign key per kind of record and a `CHECK`
/// that exactly one of them is set, so a link can never name a missing record
/// nor two at once. Each kind has a partial unique index on `(tag_id, column)`,
/// so a tag is put on a record once, and an index on the column, so a
/// record's tags are one index range.
async fn create_tags(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(Tag::Table)
                .if_not_exists()
                .col(uuid(Tag::Id).primary_key())
                .col(uuid(Tag::TreeId))
                .col(string_len(Tag::Label, 80))
                .col(string(Tag::NormalizedLabel))
                .col(ColumnDef::new(Tag::Color).small_integer().not_null())
                .col(timestamp_with_time_zone(Tag::CreatedAt))
                .col(timestamp_with_time_zone(Tag::UpdatedAt))
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_tag_tree")
                        .from(Tag::Table, Tag::TreeId)
                        .to(Tree::Table, Tree::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;
    // One spelling per folded label within a tree.
    manager
        .create_index(
            Index::create()
                .name("uq_tag_tree_normalized_label")
                .table(Tag::Table)
                .col(Tag::TreeId)
                .col(Tag::NormalizedLabel)
                .unique()
                .to_owned(),
        )
        .await?;
    // The target of `record_tag`'s composite foreign key: a link can only name
    // a tag of its own tree.
    manager
        .create_index(
            Index::create()
                .name("uq_tag_id_tree")
                .table(Tag::Table)
                .col(Tag::Id)
                .col(Tag::TreeId)
                .unique()
                .to_owned(),
        )
        .await?;

    let mut table = Table::create();
    table
        .table(RecordTag::Table)
        .if_not_exists()
        .col(uuid(RecordTag::Id).primary_key())
        .col(uuid(RecordTag::TreeId))
        .col(uuid(RecordTag::TagId))
        .col(uuid_null(RecordTag::PersonId))
        .col(uuid_null(RecordTag::FamilyId))
        .col(uuid_null(RecordTag::EventId))
        .col(uuid_null(RecordTag::PlaceId))
        .col(uuid_null(RecordTag::SourceId))
        .col(uuid_null(RecordTag::CitationId))
        .col(uuid_null(RecordTag::RepositoryId))
        .col(uuid_null(RecordTag::MediaId))
        .col(uuid_null(RecordTag::NoteId))
        .col(timestamp_with_time_zone(RecordTag::CreatedAt))
        .check(Expr::cust(
            "(CASE WHEN person_id IS NULL THEN 0 ELSE 1 END \
             + CASE WHEN family_id IS NULL THEN 0 ELSE 1 END \
             + CASE WHEN event_id IS NULL THEN 0 ELSE 1 END \
             + CASE WHEN place_id IS NULL THEN 0 ELSE 1 END \
             + CASE WHEN source_id IS NULL THEN 0 ELSE 1 END \
             + CASE WHEN citation_id IS NULL THEN 0 ELSE 1 END \
             + CASE WHEN repository_id IS NULL THEN 0 ELSE 1 END \
             + CASE WHEN media_id IS NULL THEN 0 ELSE 1 END \
             + CASE WHEN note_id IS NULL THEN 0 ELSE 1 END) = 1",
        ))
        .foreign_key(
            ForeignKey::create()
                .name("fk_record_tag_tree")
                .from(RecordTag::Table, RecordTag::TreeId)
                .to(Tree::Table, Tree::Id)
                .on_delete(ForeignKeyAction::Cascade),
        )
        // Composite, so a link's tag belongs to the link's tree.
        .foreign_key(
            ForeignKey::create()
                .name("fk_record_tag_tag")
                .from(RecordTag::Table, (RecordTag::TagId, RecordTag::TreeId))
                .to(Tag::Table, (Tag::Id, Tag::TreeId))
                .on_delete(ForeignKeyAction::Cascade),
        );
    for (column, name) in [
        (RecordTag::PersonId, "person"),
        (RecordTag::FamilyId, "family"),
        (RecordTag::EventId, "event"),
        (RecordTag::PlaceId, "place"),
        (RecordTag::SourceId, "source"),
        (RecordTag::CitationId, "citation"),
        (RecordTag::RepositoryId, "repository"),
        (RecordTag::MediaId, "media"),
        (RecordTag::NoteId, "note"),
    ] {
        // Composite, so a link's record belongs to the link's tree.
        table.foreign_key(
            ForeignKey::create()
                .name(format!("fk_record_tag_{name}"))
                .from(RecordTag::Table, (column, RecordTag::TreeId))
                .to(Alias::new(name), (Alias::new("id"), Alias::new("tree_id")))
                .on_delete(ForeignKeyAction::Cascade),
        );
    }
    manager.create_table(table.to_owned()).await?;

    for (name, column) in [
        ("idx_record_tag_tree_id", RecordTag::TreeId),
        ("idx_record_tag_tag_id", RecordTag::TagId),
    ] {
        manager
            .create_index(
                Index::create()
                    .name(name)
                    .table(RecordTag::Table)
                    .col(column)
                    .to_owned(),
            )
            .await?;
    }
    for name in [
        "person",
        "family",
        "event",
        "place",
        "source",
        "citation",
        "repository",
        "media",
        "note",
    ] {
        let column = Alias::new(format!("{name}_id"));
        // Adding one tag twice to one record is a no-op.
        manager
            .create_index(
                Index::create()
                    .name(format!("uq_record_tag_{name}"))
                    .table(RecordTag::Table)
                    .col(RecordTag::TagId)
                    .col(column.clone())
                    .unique()
                    .and_where(Expr::col(column.clone()).is_not_null())
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name(format!("idx_record_tag_{name}_id"))
                    .table(RecordTag::Table)
                    .col(column)
                    .to_owned(),
            )
            .await?;
    }
    Ok(())
}

/// GEDCOM structures kept per entity, written back on export (docs/api.md,
/// "Kept GEDCOM"). The entity is named by its kind and id, with no foreign
/// key: one column names several tables.
async fn create_gedcom_kept(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(GedcomKept::Table)
                .if_not_exists()
                .col(uuid(GedcomKept::Id).primary_key())
                .col(uuid(GedcomKept::TreeId))
                .col(string(GedcomKept::EntityKind))
                .col(uuid(GedcomKept::EntityId))
                .col(text(GedcomKept::Nodes))
                .col(timestamp_with_time_zone(GedcomKept::CreatedAt))
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_gedcom_kept_tree")
                        .from(GedcomKept::Table, GedcomKept::TreeId)
                        .to(Tree::Table, Tree::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;
    manager
        .create_index(
            Index::create()
                .name("idx_gedcom_kept_tree_id")
                .table(GedcomKept::Table)
                .col(GedcomKept::TreeId)
                .to_owned(),
        )
        .await?;
    manager
        .create_index(
            Index::create()
                .name("idx_gedcom_kept_entity")
                .table(GedcomKept::Table)
                .col(GedcomKept::EntityKind)
                .col(GedcomKept::EntityId)
                .to_owned(),
        )
        .await?;
    Ok(())
}

/// Background jobs and the person search table.
async fn create_jobs_and_search(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    // 17. background_job (FK → tree)
    manager
        .create_table(
            Table::create()
                .table(BackgroundJob::Table)
                .if_not_exists()
                .col(uuid(BackgroundJob::Id).primary_key())
                .col(uuid(BackgroundJob::TreeId))
                .col(uuid_null(BackgroundJob::ActiveTreeId))
                .col(string_len(BackgroundJob::Kind, 16))
                .col(string_len(BackgroundJob::Format, 16))
                .col(string_len(BackgroundJob::Status, 16))
                .col(string_len(BackgroundJob::Phase, 32))
                .col(string_null(BackgroundJob::SourceKey))
                .col(string_null(BackgroundJob::ArtifactKey))
                .col(text_null(BackgroundJob::PayloadJson))
                .col(string_null(BackgroundJob::OriginalFilename))
                .col(boolean(BackgroundJob::IncludeNotesAndSources).default(true))
                .col(boolean(BackgroundJob::IncludeMedia).default(true))
                .col(big_integer(BackgroundJob::Done).default(0))
                .col(big_integer(BackgroundJob::Total).default(0))
                .col(integer(BackgroundJob::Attempt).default(0))
                .col(string_len_null(BackgroundJob::InterruptedPhase, 32))
                .col(string_null(BackgroundJob::LeaseOwner))
                .col(timestamp_with_time_zone_null(BackgroundJob::LeaseUntil))
                .col(boolean(BackgroundJob::CancelRequested).default(false))
                .col(text_null(BackgroundJob::ResultJson))
                .col(string_null(BackgroundJob::ErrorCode))
                .col(string_null(BackgroundJob::TraceParent))
                .col(string_null(BackgroundJob::TraceState))
                // Who submitted the job, which the worker acts for and
                // re-checks before each commit: never a token.
                .col(string_null(BackgroundJob::ActorIssuer))
                .col(string_null(BackgroundJob::ActorSubject))
                .col(uuid_null(BackgroundJob::ActorUserId))
                .col(string_null(BackgroundJob::ActorClientId))
                .col(uuid_null(BackgroundJob::ActorApiClientId))
                .col(text_null(BackgroundJob::ActorScopes))
                .col(string_len_null(BackgroundJob::ActorSession, 64))
                .col(timestamp_with_time_zone(BackgroundJob::CreatedAt))
                .col(timestamp_with_time_zone(BackgroundJob::UpdatedAt))
                .col(timestamp_with_time_zone_null(BackgroundJob::StartedAt))
                .col(timestamp_with_time_zone_null(BackgroundJob::FinishedAt))
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_background_job_tree")
                        .from(BackgroundJob::Table, BackgroundJob::TreeId)
                        .to(Tree::Table, Tree::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_background_job_active_tree")
                        .from(BackgroundJob::Table, BackgroundJob::ActiveTreeId)
                        .to(Tree::Table, Tree::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;
    manager
        .create_index(
            Index::create()
                .name("idx_background_job_active_tree")
                .table(BackgroundJob::Table)
                .col(BackgroundJob::ActiveTreeId)
                .unique()
                .to_owned(),
        )
        .await?;
    manager
        .create_index(
            Index::create()
                .name("idx_background_job_claim")
                .table(BackgroundJob::Table)
                .col(BackgroundJob::Status)
                .col(BackgroundJob::LeaseUntil)
                .col(BackgroundJob::CreatedAt)
                .to_owned(),
        )
        .await?;

    // 18. person_search_fts: a real FTS5 virtual table on
    // SQLite (desktop), or a plain table + index on PostgreSQL (web,
    // where FTS5 isn't available — matching falls back to LIKE on
    // pre-normalized token columns computed in Rust before insert).
    let conn = manager.get_connection();
    match manager.get_database_backend() {
        DbBackend::Sqlite => {
            conn.execute_raw(Statement::from_string(
                DbBackend::Sqlite,
                r#"
                    CREATE VIRTUAL TABLE IF NOT EXISTS person_search_fts USING fts5(
                        surname,
                        given_names,
                        maiden_name,
                        birth_year,
                        death_year,
                        person_id UNINDEXED,
                        tree_id UNINDEXED,
                        sex UNINDEXED,
                        display_name UNINDEXED,
                        surname_display UNINDEXED,
                        given_names_display UNINDEXED,
                        birth_place UNINDEXED,
                        date_sort UNINDEXED,
                        birth_qualifier UNINDEXED,
                        death_qualifier UNINDEXED,
                        spouse_names UNINDEXED,
                        spouse_surnames UNINDEXED,
                        spouse_given_names UNINDEXED,
                        father_name UNINDEXED,
                        father_surname UNINDEXED,
                        father_given_names UNINDEXED,
                        mother_name UNINDEXED,
                        mother_surname UNINDEXED,
                        mother_given_names UNINDEXED,
                        children_count UNINDEXED
                    )
                    "#
                .to_owned(),
            ))
            .await?;
            // `person_id` and `tree_id` are UNINDEXED above: FTS5 indexes only
            // the words it matches, so an equality on either reads the whole
            // table. This side table maps each person to its row's rowid, with
            // an index on the tree; `PersonSearchRepo` writes both in step.
            conn.execute_raw(Statement::from_string(
                DbBackend::Sqlite,
                "CREATE TABLE IF NOT EXISTS person_search_key ( \
                     fts_rowid INTEGER PRIMARY KEY, \
                     person_id TEXT NOT NULL UNIQUE, \
                     tree_id TEXT NOT NULL \
                 )"
                .to_owned(),
            ))
            .await?;
            conn.execute_raw(Statement::from_string(
                DbBackend::Sqlite,
                "CREATE INDEX IF NOT EXISTS idx_person_search_key_tree_id \
                     ON person_search_key (tree_id)"
                    .to_owned(),
            ))
            .await?;
        }
        backend => {
            conn.execute_raw(Statement::from_string(
                backend,
                r#"
                    CREATE TABLE IF NOT EXISTS person_search_fts (
                        person_id TEXT NOT NULL PRIMARY KEY,
                        tree_id TEXT NOT NULL,
                        surname TEXT NOT NULL DEFAULT '',
                        given_names TEXT NOT NULL DEFAULT '',
                        maiden_name TEXT,
                        birth_year TEXT,
                        death_year TEXT,
                        sex TEXT NOT NULL DEFAULT 'unknown',
                        display_name TEXT NOT NULL DEFAULT '',
                        surname_display TEXT NOT NULL DEFAULT '',
                        given_names_display TEXT NOT NULL DEFAULT '',
                        birth_place TEXT,
                        date_sort TEXT,
                        birth_qualifier TEXT NOT NULL DEFAULT 'exact',
                        death_qualifier TEXT NOT NULL DEFAULT 'exact',
                        spouse_names TEXT NOT NULL DEFAULT '',
                        spouse_surnames TEXT NOT NULL DEFAULT '',
                        spouse_given_names TEXT NOT NULL DEFAULT '',
                        father_name TEXT,
                        father_surname TEXT,
                        father_given_names TEXT,
                        mother_name TEXT,
                        mother_surname TEXT,
                        mother_given_names TEXT,
                        children_count TEXT NOT NULL DEFAULT '0'
                    )
                    "#
                .to_owned(),
            ))
            .await?;
            conn.execute_raw(Statement::from_string(
                backend,
                "CREATE INDEX IF NOT EXISTS idx_person_search_fts_tree_id \
                     ON person_search_fts (tree_id)"
                    .to_owned(),
            ))
            .await?;
        }
    }
    Ok(())
}

/// Indexes for walking relationships.
async fn create_traversal_indexes(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    // Supporting indexes for relationship traversal.
    manager
        .create_index(
            Index::create()
                .name("idx_citation_person_id")
                .table(Citation::Table)
                .col(Citation::PersonId)
                .to_owned(),
        )
        .await?;
    manager
        .create_index(
            Index::create()
                .name("idx_citation_event_id")
                .table(Citation::Table)
                .col(Citation::EventId)
                .to_owned(),
        )
        .await?;
    manager
        .create_index(
            Index::create()
                .name("idx_citation_family_id")
                .table(Citation::Table)
                .col(Citation::FamilyId)
                .to_owned(),
        )
        .await?;
    manager
        .create_index(
            Index::create()
                .name("idx_media_link_person_id")
                .table(MediaLink::Table)
                .col(MediaLink::PersonId)
                .to_owned(),
        )
        .await?;
    manager
        .create_index(
            Index::create()
                .name("idx_media_link_event_id")
                .table(MediaLink::Table)
                .col(MediaLink::EventId)
                .to_owned(),
        )
        .await?;
    manager
        .create_index(
            Index::create()
                .name("idx_media_link_source_id")
                .table(MediaLink::Table)
                .col(MediaLink::SourceId)
                .to_owned(),
        )
        .await?;
    manager
        .create_index(
            Index::create()
                .name("idx_media_link_family_id")
                .table(MediaLink::Table)
                .col(MediaLink::FamilyId)
                .to_owned(),
        )
        .await?;
    Ok(())
}

/// The durable person projections, their generation, and the indexes of
/// the tree-wide reads.
async fn create_projections(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    // Durable person projections.
    manager
        .create_table(
            Table::create()
                .table(PersonDenorm::Table)
                .if_not_exists()
                .col(uuid(PersonDenorm::PersonId).primary_key())
                .col(uuid(PersonDenorm::TreeId))
                .col(text(PersonDenorm::Payload))
                .col(integer(PersonDenorm::SchemaVersion).default(0))
                .col(timestamp_with_time_zone(PersonDenorm::UpdatedAt))
                // The tree's projection generation as of the row's last
                // write, set by the projection triggers.
                .col(big_integer(PersonDenorm::Generation).default(0))
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_person_denorm_person")
                        .from(PersonDenorm::Table, PersonDenorm::PersonId)
                        .to(Person::Table, Person::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_person_denorm_tree")
                        .from(PersonDenorm::Table, PersonDenorm::TreeId)
                        .to(Tree::Table, Tree::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;
    manager
        .create_index(
            Index::create()
                .name("idx_person_denorm_tree_schema_version")
                .table(PersonDenorm::Table)
                .col(PersonDenorm::TreeId)
                .col(PersonDenorm::SchemaVersion)
                .to_owned(),
        )
        .await?;

    // A page of a tree's projections in person order, read in one range
    // without sorting the tree's rows first.
    manager
        .create_index(
            Index::create()
                .name("idx_person_denorm_tree_person")
                .table(PersonDenorm::Table)
                .col(PersonDenorm::TreeId)
                .col(PersonDenorm::PersonId)
                .to_owned(),
        )
        .await?;

    // The rows of a tree written since a generation, and its person ids,
    // from the index alone: what patches a tree's decoded projections after
    // a write instead of reading them all again.
    manager
        .create_index(
            Index::create()
                .name("idx_person_denorm_tree_generation")
                .table(PersonDenorm::Table)
                .col(PersonDenorm::TreeId)
                .col(PersonDenorm::Generation)
                .col(PersonDenorm::PersonId)
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .name("idx_event_place_id")
                .table(Event::Table)
                .col(Event::PlaceId)
                .to_owned(),
        )
        .await?;
    // The places dictionary counts a place's uses across both events and
    // media. Its event side had an index and its media side did not, so
    // opening the tab scanned every media row in the database.
    manager
        .create_index(
            Index::create()
                .name("idx_media_place_id")
                .table(Media::Table)
                .col(Media::PlaceId)
                .to_owned(),
        )
        .await?;
    // Every "events of this tree, of this type" read — the occupations
    // dictionary, the event list's type filter — and, through its leading
    // column, every "events of this tree" read: a `tree_id` index of its own
    // would serve no read this one cannot, and cost every insert.
    manager
        .create_index(
            Index::create()
                .name("idx_event_tree_event_type")
                .table(Event::Table)
                .col(Event::TreeId)
                .col(Event::EventType)
                .to_owned(),
        )
        .await?;
    manager
        .create_index(
            Index::create()
                .name("idx_note_person_id")
                .table(Note::Table)
                .col(Note::PersonId)
                .to_owned(),
        )
        .await?;
    manager
        .create_index(
            Index::create()
                .name("idx_note_event_id")
                .table(Note::Table)
                .col(Note::EventId)
                .to_owned(),
        )
        .await?;
    manager
        .create_index(
            Index::create()
                .name("idx_note_family_id")
                .table(Note::Table)
                .col(Note::FamilyId)
                .to_owned(),
        )
        .await?;
    manager
        .create_index(
            Index::create()
                .name("idx_note_source_id")
                .table(Note::Table)
                .col(Note::SourceId)
                .to_owned(),
        )
        .await?;
    create_projection_state(manager).await
}

/// The generation of each tree's projections (docs/data-model.md §4.1):
/// one row per tree, inserted with it, whose `generation` triggers move in
/// the transaction of every write to the tree's `person_denorm` rows,
/// whatever code path makes it — a refresh, an import, a rebuild after a
/// schema version change, a purge. A process holding a tree's decoded
/// projections in memory compares it with the generation they were read at
/// (the tree cache, docs/architecture.md §4.1), which works across processes.
async fn create_projection_state(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(ProjectionState::Table)
                .if_not_exists()
                .col(uuid(ProjectionState::TreeId).primary_key())
                .col(big_integer(ProjectionState::Generation).default(0))
                .col(big_integer(ProjectionState::RedactionGeneration).default(0))
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_projection_state_tree")
                        .from(ProjectionState::Table, ProjectionState::TreeId)
                        .to(Tree::Table, Tree::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;
    let backend = manager.get_database_backend();
    let statements = match backend {
        DbBackend::Sqlite => SQLITE_PROJECTION_TRIGGERS,
        _ => POSTGRES_PROJECTION_TRIGGERS,
    };
    let statements = statements.iter().chain(match backend {
        DbBackend::Sqlite => &[][..],
        _ => POSTGRES_REDACTION_FUNCTIONS,
    });
    let conn = manager.get_connection();
    for sql in statements {
        conn.execute_raw(Statement::from_string(backend, (*sql).to_owned()))
            .await?;
    }
    Ok(())
}

/// The SQLite triggers that keep `projection_state.generation` moving: one
/// bump per row written, SQLite having no statement-level trigger. A row
/// inserted or rewritten is then stamped with the generation it moved the
/// tree to (`person_denorm.generation`), so that a reader holding the
/// tree's rows as of a generation finds those written since. The stamp is
/// an update of `generation` alone, which the update trigger, declared on
/// the other columns, does not fire for.
const SQLITE_PROJECTION_TRIGGERS: &[&str] = &[
    "CREATE TRIGGER projection_tree_insert AFTER INSERT ON tree BEGIN \
         INSERT INTO projection_state (tree_id, generation) VALUES (NEW.id, 0); END",
    "CREATE TRIGGER projection_insert AFTER INSERT ON person_denorm BEGIN \
         UPDATE projection_state SET generation = generation + 1 \
         WHERE tree_id = NEW.tree_id; \
         UPDATE person_denorm SET generation = \
             (SELECT generation FROM projection_state WHERE tree_id = NEW.tree_id) \
         WHERE person_id = NEW.person_id; END",
    "CREATE TRIGGER projection_update \
         AFTER UPDATE OF tree_id, payload, schema_version, updated_at ON person_denorm BEGIN \
         UPDATE projection_state SET generation = generation + 1 \
         WHERE tree_id IN (OLD.tree_id, NEW.tree_id); \
         UPDATE person_denorm SET generation = \
             (SELECT generation FROM projection_state WHERE tree_id = NEW.tree_id) \
         WHERE person_id = NEW.person_id; END",
    "CREATE TRIGGER projection_delete AFTER DELETE ON person_denorm BEGIN \
         UPDATE projection_state SET generation = generation + 1 \
         WHERE tree_id = OLD.tree_id; END",
];

/// The functions behind the PostgreSQL projection triggers, which `down`
/// drops.
const PROJECTION_TRIGGER_FUNCTIONS: &[&str] = &[
    "projection_tree_insert",
    "projection_row_stamp",
    "projection_rows_added",
    "projection_rows_changed",
    "projection_rows_removed",
];

/// The PostgreSQL counterpart of [`SQLITE_PROJECTION_TRIGGERS`]: one bump per
/// statement and tree, through its transition tables, rather than one per
/// row — an import writes a tree's projections in a few statements, and ten
/// thousand updates of one row in one transaction would leave as many
/// versions of it for every later read to walk until vacuumed. An upsert
/// fires both the insert and the update trigger.
const POSTGRES_PROJECTION_TRIGGERS: &[&str] = &[
    "CREATE FUNCTION projection_tree_insert() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN \
         INSERT INTO projection_state (tree_id, generation) VALUES (NEW.id, 0); \
         RETURN NULL; END $$",
    "CREATE FUNCTION projection_rows_added() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN \
         UPDATE projection_state SET generation = generation + 1 \
         WHERE tree_id IN (SELECT tree_id FROM added_rows); RETURN NULL; END $$",
    "CREATE FUNCTION projection_rows_changed() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN \
         UPDATE projection_state SET generation = generation + 1 \
         WHERE tree_id IN (SELECT tree_id FROM added_rows \
         UNION SELECT tree_id FROM removed_rows); RETURN NULL; END $$",
    "CREATE FUNCTION projection_rows_removed() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN \
         UPDATE projection_state SET generation = generation + 1 \
         WHERE tree_id IN (SELECT tree_id FROM removed_rows); RETURN NULL; END $$",
    "CREATE TRIGGER projection_tree_insert AFTER INSERT ON tree \
         FOR EACH ROW EXECUTE FUNCTION projection_tree_insert()",
    // Each row written is stamped with the generation its statement moves
    // the tree to (`person_denorm.generation`). The state row is locked
    // first, so the stamp is past every generation committed before this
    // transaction's own commit: a reader holding the tree's rows as of a
    // generation finds every row written since by `generation > it`.
    "CREATE FUNCTION projection_row_stamp() RETURNS trigger LANGUAGE plpgsql AS $$ \
         DECLARE current_generation bigint; BEGIN \
         SELECT generation INTO current_generation FROM projection_state \
         WHERE tree_id = NEW.tree_id FOR UPDATE; \
         NEW.generation := COALESCE(current_generation, 0) + 1; \
         RETURN NEW; END $$",
    "CREATE TRIGGER projection_stamp BEFORE INSERT OR UPDATE ON person_denorm \
         FOR EACH ROW EXECUTE FUNCTION projection_row_stamp()",
    "CREATE TRIGGER projection_insert AFTER INSERT ON person_denorm \
         REFERENCING NEW TABLE AS added_rows \
         FOR EACH STATEMENT EXECUTE FUNCTION projection_rows_added()",
    "CREATE TRIGGER projection_update AFTER UPDATE ON person_denorm \
         REFERENCING OLD TABLE AS removed_rows NEW TABLE AS added_rows \
         FOR EACH STATEMENT EXECUTE FUNCTION projection_rows_changed()",
    "CREATE TRIGGER projection_delete AFTER DELETE ON person_denorm \
         REFERENCING OLD TABLE AS removed_rows \
         FOR EACH STATEMENT EXECUTE FUNCTION projection_rows_removed()",
];

/// The PostgreSQL functions that move `projection_state.redaction_generation`
/// (docs/data-model.md §4.1), created with the table so that the SOSA
/// functions may call `redaction_touch` too.
///
/// `redaction_touch` moves a tree's generation once per transaction: a
/// reader stamping a redaction needs it to differ once the transaction
/// commits, not once per row, and ten thousand updates of one row in one
/// transaction would leave as many versions of it for every later read to
/// walk. The trees already moved are kept in a transaction-local setting.
/// The statement triggers pass `redaction_rows_written` the query naming the
/// trees a statement touched, over its transition tables.
const POSTGRES_REDACTION_FUNCTIONS: &[&str] = &[
    "CREATE FUNCTION redaction_touch(tree uuid) RETURNS void LANGUAGE plpgsql AS $$ \
         DECLARE touched text := \
         coalesce(current_setting('oxidgene.redaction_touched', true), ''); \
         BEGIN \
         IF tree IS NULL OR position(tree::text IN touched) > 0 THEN RETURN; END IF; \
         UPDATE projection_state SET redaction_generation = redaction_generation + 1 \
         WHERE tree_id = tree; \
         PERFORM set_config('oxidgene.redaction_touched', touched || tree::text || ',', true); \
         END $$",
    "CREATE FUNCTION redaction_rows_written() RETURNS trigger LANGUAGE plpgsql AS $$ \
         DECLARE tree uuid; BEGIN \
         FOR tree IN EXECUTE TG_ARGV[0] LOOP PERFORM redaction_touch(tree); END LOOP; \
         RETURN NULL; END $$",
    "CREATE FUNCTION redaction_access_changed() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN \
         PERFORM redaction_touch(OLD.tree_id); RETURN NULL; END $$",
];

/// The PostgreSQL functions of the redaction generation, with their
/// signatures, which `down` drops.
const REDACTION_TRIGGER_FUNCTIONS: &[&str] = &[
    "redaction_touch(uuid)",
    "redaction_rows_written()",
    "redaction_access_changed()",
];

/// Every table a restricted viewer's redaction reads (docs/architecture.md
/// §4.1, Tree cache): a write to any of their rows moves the tree's
/// redaction generation. Each row names its tree in its own `tree_id`.
/// `tree` itself is moved on update only — inserted, it has no redaction
/// yet; deleted, it takes its `projection_state` row.
const REDACTION_INPUTS: &[&str] = &[
    "person",
    "family",
    "family_spouse",
    "family_child",
    "event",
    "citation",
    "note",
    "media",
    "media_link",
    "vignette",
    "source",
    "source_repository",
    "repository",
    "place",
    "person_visibility",
    "privacy_exclusion",
    "excluded_person",
    "access_scope",
    "access_scope_person",
];

/// The changes to a viewer's access that end or change what a kept
/// redaction was built for: a membership's role, scope or expiry, a share
/// link's revocation, scope or expiry, and either's deletion. A share
/// link's use, which updates its counters, moves nothing.
const REDACTION_ACCESS: &[(&str, &str)] = &[
    ("tree_membership", "role, scope_id, expires_at"),
    ("share_link", "revoked_at, scope_id, expires_at"),
];

/// The SQLite triggers moving the redaction generation: one bump per row.
fn sqlite_redaction_triggers() -> Vec<String> {
    const BUMP: &str =
        "UPDATE projection_state SET redaction_generation = redaction_generation + 1";
    let mut out = Vec::new();
    for table in REDACTION_INPUTS {
        out.push(format!(
            "CREATE TRIGGER redaction_{table}_insert AFTER INSERT ON {table} BEGIN \
             {BUMP} WHERE tree_id = NEW.tree_id; END"
        ));
        out.push(format!(
            "CREATE TRIGGER redaction_{table}_update AFTER UPDATE ON {table} BEGIN \
             {BUMP} WHERE tree_id IN (OLD.tree_id, NEW.tree_id); END"
        ));
        out.push(format!(
            "CREATE TRIGGER redaction_{table}_delete AFTER DELETE ON {table} BEGIN \
             {BUMP} WHERE tree_id = OLD.tree_id; END"
        ));
    }
    out.push(format!(
        "CREATE TRIGGER redaction_tree_update AFTER UPDATE ON tree BEGIN \
         {BUMP} WHERE tree_id = NEW.id; END"
    ));
    for &(table, columns) in REDACTION_ACCESS {
        out.push(format!(
            "CREATE TRIGGER redaction_{table}_update AFTER UPDATE OF {columns} ON {table} \
             BEGIN {BUMP} WHERE tree_id = OLD.tree_id; END"
        ));
        out.push(format!(
            "CREATE TRIGGER redaction_{table}_delete AFTER DELETE ON {table} \
             BEGIN {BUMP} WHERE tree_id = OLD.tree_id; END"
        ));
    }
    out
}

/// The PostgreSQL triggers moving the redaction generation: one statement
/// trigger per table and operation, through `redaction_touch`, and one row
/// trigger on the rare changes of an access.
fn postgres_redaction_triggers() -> Vec<String> {
    let statement = |table: &str, operation: &str, transition: &str, trees: &str| {
        format!(
            "CREATE TRIGGER redaction_{table}_{} AFTER {operation} ON {table} \
             REFERENCING {transition} FOR EACH STATEMENT \
             EXECUTE FUNCTION redaction_rows_written('{trees}')",
            operation.to_lowercase()
        )
    };
    let mut out = Vec::new();
    const ADDED: &str = "SELECT tree_id FROM added_rows";
    const REMOVED: &str = "SELECT tree_id FROM removed_rows";
    for table in REDACTION_INPUTS {
        out.push(statement(table, "INSERT", "NEW TABLE AS added_rows", ADDED));
        out.push(statement(
            table,
            "UPDATE",
            "OLD TABLE AS removed_rows NEW TABLE AS added_rows",
            &format!("{ADDED} UNION {REMOVED}"),
        ));
        out.push(statement(
            table,
            "DELETE",
            "OLD TABLE AS removed_rows",
            REMOVED,
        ));
    }
    out.push(statement(
        "tree",
        "UPDATE",
        "NEW TABLE AS added_rows",
        "SELECT id FROM added_rows",
    ));
    for &(table, columns) in REDACTION_ACCESS {
        out.push(format!(
            "CREATE TRIGGER redaction_{table}_update AFTER UPDATE OF {columns} ON {table} \
             FOR EACH ROW EXECUTE FUNCTION redaction_access_changed()"
        ));
        out.push(format!(
            "CREATE TRIGGER redaction_{table}_delete AFTER DELETE ON {table} \
             FOR EACH ROW EXECUTE FUNCTION redaction_access_changed()"
        ));
    }
    out
}

/// Create the triggers moving the redaction generation (docs/data-model.md
/// §4.1), once every table they watch exists.
async fn create_redaction_triggers(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    let backend = manager.get_database_backend();
    let statements = match backend {
        DbBackend::Sqlite => sqlite_redaction_triggers(),
        _ => postgres_redaction_triggers(),
    };
    let conn = manager.get_connection();
    for sql in statements {
        conn.execute_raw(Statement::from_string(backend, sql))
            .await?;
    }
    Ok(())
}

/// The stored SOSA ancestry of each tree (docs/data-model.md §4.8): the
/// root and its ancestors, with their SOSA numbers, and the state saying
/// whether they are current.
///
/// Triggers bump a tree's `generation` whenever an input of the ancestry
/// changes — a family link, a family's or a person's deletion, the root —
/// whatever code path writes it, imports, merges and restores included. The
/// rows are current while `built_generation` equals it; the next read
/// rebuilds them otherwise.
async fn create_sosa(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(SosaState::Table)
                .if_not_exists()
                .col(uuid(SosaState::TreeId).primary_key())
                .col(big_integer(SosaState::Generation).default(0))
                .col(big_integer_null(SosaState::BuiltGeneration))
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_sosa_state_tree")
                        .from(SosaState::Table, SosaState::TreeId)
                        .to(Tree::Table, Tree::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;
    manager
        .create_table(
            Table::create()
                .table(SosaAncestor::Table)
                .if_not_exists()
                .col(uuid(SosaAncestor::PersonId).primary_key())
                .col(uuid(SosaAncestor::TreeId))
                .col(big_integer_null(SosaAncestor::SosaNumber))
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_sosa_ancestor_person")
                        .from(SosaAncestor::Table, SosaAncestor::PersonId)
                        .to(Person::Table, Person::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_sosa_ancestor_tree")
                        .from(SosaAncestor::Table, SosaAncestor::TreeId)
                        .to(Tree::Table, Tree::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;
    // A rebuild replaces a tree's rows.
    manager
        .create_index(
            Index::create()
                .name("idx_sosa_ancestor_tree_id")
                .table(SosaAncestor::Table)
                .col(SosaAncestor::TreeId)
                .to_owned(),
        )
        .await?;

    let backend = manager.get_database_backend();
    let statements = match backend {
        DbBackend::Sqlite => SQLITE_SOSA_TRIGGERS,
        _ => POSTGRES_SOSA_TRIGGERS,
    };
    let conn = manager.get_connection();
    for sql in statements {
        conn.execute_raw(Statement::from_string(backend, (*sql).to_owned()))
            .await?;
    }
    Ok(())
}

/// The bump of the tree of family link `$row` (`NEW` or `OLD`), as a SQLite
/// trigger body writes it: the link names its tree itself.
macro_rules! bump_link_tree {
    ($row:literal) => {
        concat!(
            "UPDATE sosa_state SET generation = generation + 1 WHERE tree_id = ",
            $row,
            ".tree_id;"
        )
    };
}

/// The SQLite triggers that keep `sosa_state.generation` moving. A row
/// deleted by a cascade fires its table's trigger too.
const SQLITE_SOSA_TRIGGERS: &[&str] = &[
    "CREATE TRIGGER sosa_tree_insert AFTER INSERT ON tree BEGIN \
         INSERT INTO sosa_state (tree_id, generation) VALUES (NEW.id, 0); END",
    "CREATE TRIGGER sosa_tree_root AFTER UPDATE OF sosa_root_person_id ON tree \
         WHEN OLD.sosa_root_person_id IS NOT NEW.sosa_root_person_id BEGIN \
         UPDATE sosa_state SET generation = generation + 1 WHERE tree_id = NEW.id; END",
    concat!(
        "CREATE TRIGGER sosa_family_child_insert AFTER INSERT ON family_child BEGIN ",
        bump_link_tree!("NEW"),
        " END"
    ),
    concat!(
        "CREATE TRIGGER sosa_family_child_delete AFTER DELETE ON family_child BEGIN ",
        bump_link_tree!("OLD"),
        " END"
    ),
    concat!(
        "CREATE TRIGGER sosa_family_child_update AFTER UPDATE OF family_id, person_id \
         ON family_child WHEN OLD.family_id IS NOT NEW.family_id \
         OR OLD.person_id IS NOT NEW.person_id BEGIN ",
        bump_link_tree!("OLD"),
        bump_link_tree!("NEW"),
        " END"
    ),
    concat!(
        "CREATE TRIGGER sosa_family_spouse_insert AFTER INSERT ON family_spouse BEGIN ",
        bump_link_tree!("NEW"),
        " END"
    ),
    concat!(
        "CREATE TRIGGER sosa_family_spouse_delete AFTER DELETE ON family_spouse BEGIN ",
        bump_link_tree!("OLD"),
        " END"
    ),
    concat!(
        "CREATE TRIGGER sosa_family_spouse_update AFTER UPDATE OF family_id, person_id, role \
         ON family_spouse WHEN OLD.family_id IS NOT NEW.family_id \
         OR OLD.person_id IS NOT NEW.person_id OR OLD.role IS NOT NEW.role BEGIN ",
        bump_link_tree!("OLD"),
        bump_link_tree!("NEW"),
        " END"
    ),
    "CREATE TRIGGER sosa_family_delete AFTER DELETE ON family BEGIN \
         UPDATE sosa_state SET generation = generation + 1 WHERE tree_id = OLD.tree_id; END",
    "CREATE TRIGGER sosa_person_delete AFTER DELETE ON person BEGIN \
         UPDATE sosa_state SET generation = generation + 1 WHERE tree_id = OLD.tree_id; END",
];

/// The trees whose family links of kind `$changed` a PostgreSQL `UPDATE`
/// statement changed: those of the rows whose family, person or — for a
/// spouse — role differ between its transition tables. A statement that
/// only reorders the links names none.
macro_rules! trees_of_changed_links {
    ($changed:literal) => {
        concat!(
            "SELECT o.tree_id FROM removed_rows o JOIN added_rows n ON n.id = o.id WHERE ",
            $changed,
            " UNION SELECT n.tree_id FROM removed_rows o JOIN added_rows n ON n.id = o.id WHERE ",
            $changed,
            " ORDER BY 1"
        )
    };
}

/// A PostgreSQL statement trigger named `$name` moving, after `$event` on
/// `$table`, the SOSA generation of each tree the query `$trees` names over
/// the statement's transition tables `$tables`.
macro_rules! sosa_links_trigger {
    ($name:literal, $event:literal, $table:literal, $tables:literal, $trees:expr) => {
        concat!(
            "CREATE TRIGGER ",
            $name,
            " AFTER ",
            $event,
            " ON ",
            $table,
            " REFERENCING ",
            $tables,
            " FOR EACH STATEMENT EXECUTE FUNCTION sosa_links_written('",
            $trees,
            "')"
        )
    };
}

/// The functions behind the PostgreSQL triggers, which `down` drops.
const SOSA_TRIGGER_FUNCTIONS: &[&str] = &[
    "sosa_tree_insert",
    "sosa_tree_root",
    "sosa_links_written",
    "sosa_owner_changed",
];

/// The PostgreSQL counterpart of [`SQLITE_SOSA_TRIGGERS`].
///
/// The family links move a tree's generation once per statement, through
/// its transition tables, as the projection triggers do
/// ([`POSTGRES_PROJECTION_TRIGGERS`]): an import writes a tree's links in a
/// few statements, and one update of `sosa_state` per link would leave as
/// many versions of that one row for every later read to walk. Not once per
/// transaction, unlike the redaction generation: the exclusion union and
/// the access scopes are rebuilt inside the transaction that changes the
/// links and stamped with the generation they read, so a link written after
/// such a rebuild has to move it again.
const POSTGRES_SOSA_TRIGGERS: &[&str] = &[
    "CREATE FUNCTION sosa_tree_insert() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN \
         INSERT INTO sosa_state (tree_id, generation) VALUES (NEW.id, 0); RETURN NULL; END $$",
    "CREATE FUNCTION sosa_tree_root() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN \
         PERFORM redaction_touch(NEW.id); \
         UPDATE sosa_state SET generation = generation + 1 WHERE tree_id = NEW.id; \
         RETURN NULL; END $$",
    // The family links a statement wrote: each tree its argument, a query
    // over the transition tables, names. The redaction generation first,
    // so that `projection_state` is locked before `sosa_state`
    // ([`POSTGRES_REDACTION_FUNCTIONS`]); the trees in one order, so that
    // two statements over several trees never wait on each other in a cycle.
    "CREATE FUNCTION sosa_links_written() RETURNS trigger LANGUAGE plpgsql AS $$ \
         DECLARE tree uuid; BEGIN \
         FOR tree IN EXECUTE TG_ARGV[0] LOOP \
         PERFORM redaction_touch(tree); \
         UPDATE sosa_state SET generation = generation + 1 WHERE tree_id = tree; \
         END LOOP; RETURN NULL; END $$",
    // A family or a person deleted: the tree it belonged to.
    "CREATE FUNCTION sosa_owner_changed() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN \
         PERFORM redaction_touch(OLD.tree_id); \
         UPDATE sosa_state SET generation = generation + 1 WHERE tree_id = OLD.tree_id; \
         RETURN NULL; END $$",
    "CREATE TRIGGER sosa_tree_insert AFTER INSERT ON tree \
         FOR EACH ROW EXECUTE FUNCTION sosa_tree_insert()",
    "CREATE TRIGGER sosa_tree_root AFTER UPDATE OF sosa_root_person_id ON tree FOR EACH ROW \
         WHEN (OLD.sosa_root_person_id IS DISTINCT FROM NEW.sosa_root_person_id) \
         EXECUTE FUNCTION sosa_tree_root()",
    sosa_links_trigger!(
        "sosa_family_child_insert",
        "INSERT",
        "family_child",
        "NEW TABLE AS added_rows",
        "SELECT DISTINCT tree_id FROM added_rows ORDER BY 1"
    ),
    sosa_links_trigger!(
        "sosa_family_child_delete",
        "DELETE",
        "family_child",
        "OLD TABLE AS removed_rows",
        "SELECT DISTINCT tree_id FROM removed_rows ORDER BY 1"
    ),
    // A transition table cannot be combined with a column list: the trigger
    // fires on every update and its query keeps the links that changed.
    sosa_links_trigger!(
        "sosa_family_child_update",
        "UPDATE",
        "family_child",
        "OLD TABLE AS removed_rows NEW TABLE AS added_rows",
        trees_of_changed_links!(
            "(o.family_id IS DISTINCT FROM n.family_id \
             OR o.person_id IS DISTINCT FROM n.person_id)"
        )
    ),
    sosa_links_trigger!(
        "sosa_family_spouse_insert",
        "INSERT",
        "family_spouse",
        "NEW TABLE AS added_rows",
        "SELECT DISTINCT tree_id FROM added_rows ORDER BY 1"
    ),
    sosa_links_trigger!(
        "sosa_family_spouse_delete",
        "DELETE",
        "family_spouse",
        "OLD TABLE AS removed_rows",
        "SELECT DISTINCT tree_id FROM removed_rows ORDER BY 1"
    ),
    sosa_links_trigger!(
        "sosa_family_spouse_update",
        "UPDATE",
        "family_spouse",
        "OLD TABLE AS removed_rows NEW TABLE AS added_rows",
        trees_of_changed_links!(
            "(o.family_id IS DISTINCT FROM n.family_id \
             OR o.person_id IS DISTINCT FROM n.person_id OR o.role IS DISTINCT FROM n.role)"
        )
    ),
    "CREATE TRIGGER sosa_family_delete AFTER DELETE ON family \
         FOR EACH ROW EXECUTE FUNCTION sosa_owner_changed()",
    "CREATE TRIGGER sosa_person_delete AFTER DELETE ON person \
         FOR EACH ROW EXECUTE FUNCTION sosa_owner_changed()",
];

/// The stored inputs of the contemporary rule (docs/data-model.md §6.8,
/// docs/privacy.md §4.5): one row per live person, written by the web
/// build only, in the transaction of every write that changes them. The
/// threshold and the current year are applied when reading, so a change of
/// threshold rewrites nothing.
async fn create_person_visibility(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(PersonVisibility::Table)
                .if_not_exists()
                .col(uuid(PersonVisibility::PersonId).primary_key())
                .col(uuid(PersonVisibility::TreeId))
                .col(string_len(PersonVisibility::Privacy, 10))
                .col(boolean(PersonVisibility::Deceased))
                .col(integer_null(PersonVisibility::LatestBirthYear))
                .col(integer_null(PersonVisibility::EarliestBirthYear))
                .col(integer_null(PersonVisibility::RecordedBirthYear))
                .col(integer_null(PersonVisibility::KinBirthYear))
                // The bounds descendants give by depth (`Descent::encode`).
                .col(string(PersonVisibility::Descent))
                .check(one_of("privacy", &["default", "public", "private"]))
                // `VISIBILITY_RULE_VERSION` the row was derived with.
                .col(integer(PersonVisibility::RuleVersion))
                .col(timestamp_with_time_zone(PersonVisibility::UpdatedAt))
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_person_visibility_person")
                        .from(PersonVisibility::Table, PersonVisibility::PersonId)
                        .to(Person::Table, Person::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_person_visibility_tree")
                        .from(PersonVisibility::Table, PersonVisibility::TreeId)
                        .to(Tree::Table, Tree::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;
    manager
        .create_index(
            Index::create()
                .name("idx_person_visibility_tree_version")
                .table(PersonVisibility::Table)
                .col(PersonVisibility::TreeId)
                .col(PersonVisibility::RuleVersion)
                .to_owned(),
        )
        .await
}

/// The owner's exclusions from sharing (docs/data-model.md §6.9,
/// docs/privacy.md §6): each exclusion, the union of the persons they cover,
/// and the SOSA ancestry generation that union was built from. Written by
/// the web build only.
async fn create_exclusions(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(PrivacyExclusion::Table)
                .if_not_exists()
                .col(uuid(PrivacyExclusion::Id).primary_key())
                .col(uuid(PrivacyExclusion::TreeId))
                .col(string_len(PrivacyExclusion::Kind, 12))
                .col(uuid(PrivacyExclusion::PersonId))
                .col(integer_null(PrivacyExclusion::Depth))
                .col(boolean(PrivacyExclusion::IncludeRoot))
                .col(boolean(PrivacyExclusion::IncludeSpouses))
                .col(uuid_null(PrivacyExclusion::CreatedBy))
                .col(timestamp_with_time_zone(PrivacyExclusion::CreatedAt))
                .check(named_one_of(
                    "privacy_exclusion",
                    "kind",
                    &ExclusionKind::ALL.map(ExclusionKind::as_str),
                ))
                // `person_id` names the root by id alone, as an access
                // scope's definition does: an exclusion outlives the
                // deletion of its root, covers nobody meanwhile, and applies
                // again once the history restores the root under its id.
                // The tree's purge takes it.
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_privacy_exclusion_tree")
                        .from(PrivacyExclusion::Table, PrivacyExclusion::TreeId)
                        .to(Tree::Table, Tree::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;
    for (name, column) in [
        ("idx_privacy_exclusion_tree_id", PrivacyExclusion::TreeId),
        (
            "idx_privacy_exclusion_person_id",
            PrivacyExclusion::PersonId,
        ),
    ] {
        manager
            .create_index(
                Index::create()
                    .name(name)
                    .table(PrivacyExclusion::Table)
                    .col(column)
                    .to_owned(),
            )
            .await?;
    }
    manager
        .create_table(
            Table::create()
                .table(ExcludedPerson::Table)
                .if_not_exists()
                .col(uuid(ExcludedPerson::TreeId))
                .col(uuid(ExcludedPerson::PersonId))
                .primary_key(
                    Index::create()
                        .col(ExcludedPerson::TreeId)
                        .col(ExcludedPerson::PersonId),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_excluded_person_tree")
                        .from(ExcludedPerson::Table, ExcludedPerson::TreeId)
                        .to(Tree::Table, Tree::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_excluded_person_person")
                        .from(ExcludedPerson::Table, ExcludedPerson::PersonId)
                        .to(Person::Table, Person::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;
    manager
        .create_index(
            Index::create()
                .name("idx_excluded_person_person_id")
                .table(ExcludedPerson::Table)
                .col(ExcludedPerson::PersonId)
                .to_owned(),
        )
        .await?;
    manager
        .create_table(
            Table::create()
                .table(ExclusionState::Table)
                .if_not_exists()
                .col(uuid(ExclusionState::TreeId).primary_key())
                .col(big_integer_null(ExclusionState::BuiltGeneration))
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_exclusion_state_tree")
                        .from(ExclusionState::Table, ExclusionState::TreeId)
                        .to(Tree::Table, Tree::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;
    Ok(())
}

/// Answers that two same-named persons are different people.
///
/// Saving a person whose name another person of the tree already bears asks
/// whether they are the same individual; answering "no" is remembered here, one
/// row per pair. A pair is stored once, with `person_id < other_person_id`, so
/// the unique index is what makes recording the same answer twice a no-op.
/// Both columns cascade from `person`, and `tree_id` from `tree`, so neither a
/// deleted person nor a purged tree leaves a row behind.
async fn create_person_distinct(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(PersonDistinct::Table)
                .if_not_exists()
                .col(uuid(PersonDistinct::Id).primary_key())
                .col(uuid(PersonDistinct::TreeId))
                .col(uuid(PersonDistinct::PersonId))
                .col(uuid(PersonDistinct::OtherPersonId))
                .col(timestamp_with_time_zone(PersonDistinct::CreatedAt))
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_person_distinct_tree")
                        .from(PersonDistinct::Table, PersonDistinct::TreeId)
                        .to(Tree::Table, Tree::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_person_distinct_person")
                        .from(PersonDistinct::Table, PersonDistinct::PersonId)
                        .to(Person::Table, Person::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_person_distinct_other_person")
                        .from(PersonDistinct::Table, PersonDistinct::OtherPersonId)
                        .to(Person::Table, Person::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;
    // Leads with `person_id`, so it also answers "who is this person
    // distinct from" for the lower id of each pair.
    manager
        .create_index(
            Index::create()
                .name("idx_person_distinct_pair")
                .table(PersonDistinct::Table)
                .col(PersonDistinct::PersonId)
                .col(PersonDistinct::OtherPersonId)
                .unique()
                .to_owned(),
        )
        .await?;
    manager
        .create_index(
            Index::create()
                .name("idx_person_distinct_other_person_id")
                .table(PersonDistinct::Table)
                .col(PersonDistinct::OtherPersonId)
                .to_owned(),
        )
        .await
}

/// The history: one audit entry per write to a tree, and the versions of its
/// persons, places, sources and settings.
///
/// `audit_entry` is the tree's audit log, read newest first, optionally
/// filtered by category — the two indexes lead with `tree_id` for exactly those
/// reads. `record_version` holds the states writes replaced — never the live
/// one — as JSON, numbered per record, its time being its audit entry's; a
/// deletion stores the record's last state, and the deleted state a restore
/// replaced stores no JSON. The unique index on the record and its number
/// is both how a record's history is read and what keeps two writers from
/// numbering the same version twice. Both tables cascade from `tree`, and
/// versions from their audit entry, so the purge of a tree takes its history
/// with it. Neither references the persons or places it describes: a history
/// has to outlive what it records.
async fn create_history(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(AuditEntry::Table)
                .if_not_exists()
                .col(uuid(AuditEntry::Id).primary_key())
                .col(uuid(AuditEntry::TreeId))
                .col(timestamp_with_time_zone(AuditEntry::OccurredAt))
                .col(string_len(AuditEntry::Category, 16))
                .col(string_len(AuditEntry::Action, 16))
                .col(string_len(AuditEntry::Entity, 32))
                .col(uuid_null(AuditEntry::EntityId))
                .col(string_len_null(AuditEntry::Subject, 16))
                .col(uuid_null(AuditEntry::SubjectId))
                .col(text_null(AuditEntry::Label))
                .col(text_null(AuditEntry::Details))
                // The author: `local` on the desktop and in the `none` mode.
                .col(string_len(AuditEntry::AuthorKind, 16).default("local"))
                .col(uuid_null(AuditEntry::AuthorUserId))
                .col(uuid_null(AuditEntry::AuthorApiClientId))
                .col(string_null(AuditEntry::AuthorClientId))
                .check(named_one_of(
                    "audit_entry",
                    "category",
                    &values_of(AuditCategory::ALL, AuditCategory::as_str),
                ))
                .check(named_one_of(
                    "audit_entry",
                    "action",
                    &values_of(AuditAction::ALL, AuditAction::as_str),
                ))
                .check(named_one_of(
                    "audit_entry",
                    "entity",
                    &values_of(AuditEntity::ALL, AuditEntity::as_str),
                ))
                .check(named_one_of(
                    "audit_entry",
                    "subject",
                    &values_of(AuditSubject::ALL, AuditSubject::as_str),
                ))
                .check(named_one_of(
                    "audit_entry",
                    "author_kind",
                    &AuthorKind::ALL.map(AuthorKind::as_str),
                ))
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_audit_entry_tree")
                        .from(AuditEntry::Table, AuditEntry::TreeId)
                        .to(Tree::Table, Tree::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;
    manager
        .create_index(
            Index::create()
                .name("idx_audit_entry_tree")
                .table(AuditEntry::Table)
                .col(AuditEntry::TreeId)
                .col(AuditEntry::Id)
                .to_owned(),
        )
        .await?;
    // A record's own log — the history panel of a person, a place or a
    // source — filters on the subject and pages newest first.
    manager
        .create_index(
            Index::create()
                .name("idx_audit_entry_tree_subject")
                .table(AuditEntry::Table)
                .col(AuditEntry::TreeId)
                .col(AuditEntry::SubjectId)
                .col(AuditEntry::Id)
                .to_owned(),
        )
        .await?;
    manager
        .create_index(
            Index::create()
                .name("idx_audit_entry_tree_category")
                .table(AuditEntry::Table)
                .col(AuditEntry::TreeId)
                .col(AuditEntry::Category)
                .col(AuditEntry::Id)
                .to_owned(),
        )
        .await?;

    manager
        .create_table(
            Table::create()
                .table(RecordVersion::Table)
                .if_not_exists()
                .col(uuid(RecordVersion::Id).primary_key())
                .col(uuid(RecordVersion::TreeId))
                .col(uuid(RecordVersion::AuditEntryId))
                .col(string_len(RecordVersion::RecordType, 16))
                .col(uuid(RecordVersion::RecordId))
                .col(integer(RecordVersion::Version))
                .col(boolean(RecordVersion::Deleted).default(false))
                .col(text_null(RecordVersion::Snapshot))
                .col(text_null(RecordVersion::Labels))
                .check(named_one_of(
                    "record_version",
                    "record_type",
                    &values_of(RecordType::ALL, RecordType::as_str),
                ))
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_record_version_tree")
                        .from(RecordVersion::Table, RecordVersion::TreeId)
                        .to(Tree::Table, Tree::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_record_version_audit_entry")
                        .from(RecordVersion::Table, RecordVersion::AuditEntryId)
                        .to(AuditEntry::Table, AuditEntry::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;
    manager
        .create_index(
            Index::create()
                .name("idx_record_version_record")
                .table(RecordVersion::Table)
                .col(RecordVersion::RecordType)
                .col(RecordVersion::RecordId)
                .col(RecordVersion::Version)
                .unique()
                .to_owned(),
        )
        .await?;
    manager
        .create_index(
            Index::create()
                .name("idx_record_version_audit_entry")
                .table(RecordVersion::Table)
                .col(RecordVersion::AuditEntryId)
                // A page of an entry's versions, in id order.
                .col(RecordVersion::Id)
                .to_owned(),
        )
        .await?;
    manager
        .create_index(
            Index::create()
                .name("idx_record_version_tree")
                .table(RecordVersion::Table)
                .col(RecordVersion::TreeId)
                .to_owned(),
        )
        .await
}

#[derive(DeriveIden)]
enum PersonDistinct {
    Table,
    Id,
    TreeId,
    PersonId,
    OtherPersonId,
    CreatedAt,
}

#[derive(DeriveIden)]
enum AuditEntry {
    Table,
    Id,
    TreeId,
    OccurredAt,
    Category,
    Action,
    Entity,
    EntityId,
    Subject,
    SubjectId,
    Label,
    Details,
    AuthorKind,
    AuthorUserId,
    AuthorApiClientId,
    AuthorClientId,
}

#[derive(DeriveIden)]
enum RecordVersion {
    Table,
    Id,
    TreeId,
    AuditEntryId,
    RecordType,
    RecordId,
    Version,
    Deleted,
    Snapshot,
    Labels,
}

/// The accounts of the web build (docs/data-model.md §6.1). Only the web
/// build writes them; the table stays empty on the desktop.
async fn create_accounts(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(AppUser::Table)
                .if_not_exists()
                .col(uuid(AppUser::Id).primary_key())
                .col(string_null(AppUser::Issuer))
                .col(string_null(AppUser::Subject))
                .col(string_null(AppUser::Email))
                .col(boolean(AppUser::EmailVerified).default(false))
                .col(string_null(AppUser::ProviderName))
                .col(string_null(AppUser::Username))
                .col(string_null(AppUser::DisplayName))
                .col(string_len_null(AppUser::Locale, 16))
                .col(string_len_null(AppUser::AvatarSha256, 64))
                .col(timestamp_with_time_zone_null(AppUser::AvatarUpdatedAt))
                .col(string_len(AppUser::InstanceRole, 8).default("user"))
                .col(boolean(AppUser::CanCreateTrees).default(false))
                .col(string_len(AppUser::Status, 12).default("active"))
                .col(timestamp_with_time_zone(AppUser::CreatedAt))
                .col(timestamp_with_time_zone(AppUser::UpdatedAt))
                .col(timestamp_with_time_zone(AppUser::LastSignInAt))
                .col(timestamp_with_time_zone_null(AppUser::DeactivatedAt))
                .col(timestamp_with_time_zone_null(AppUser::ErasedAt))
                .check(one_of("instance_role", &["admin", "user"]))
                .check(one_of("status", &["active", "deactivated", "erased"]))
                .to_owned(),
        )
        .await?;
    // The identity is the user's key. Both columns are cleared on erasure,
    // and NULLs never collide in a unique index on either backend, so any
    // number of tombstones coexist.
    manager
        .create_index(
            Index::create()
                .name("idx_app_user_identity")
                .table(AppUser::Table)
                .col(AppUser::Issuer)
                .col(AppUser::Subject)
                .unique()
                .to_owned(),
        )
        .await?;
    create_access_scopes(manager).await?;
    create_memberships(manager).await?;
    create_invitations(manager).await?;
    create_share_links(manager).await?;
    create_access_events(manager).await?;
    create_notifications(manager).await?;
    create_mail_outbox(manager).await
}

/// The persons a share is limited to (docs/data-model.md §6.5): a scope's
/// definition, and its persons materialized as of the tree's ancestry
/// generation `built_generation`, rebuilt when it moves.
async fn create_access_scopes(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(AccessScope::Table)
                .if_not_exists()
                .col(uuid(AccessScope::Id).primary_key())
                .col(uuid(AccessScope::TreeId))
                .col(text(AccessScope::Definition))
                .col(big_integer_null(AccessScope::BuiltGeneration))
                .col(timestamp_with_time_zone(AccessScope::CreatedAt))
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_access_scope_tree")
                        .from(AccessScope::Table, AccessScope::TreeId)
                        .to(Tree::Table, Tree::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;
    // `(tree_id, id)` is unique so that a row naming this record can pair it
    // with its own tree in a composite foreign key: it cannot name a record
    // of another tree (docs/data-model.md, Integrity constraints). The index
    // also reads the tree's records in id order, a page of a list.
    manager
        .create_index(
            Index::create()
                .name("uq_access_scope_tree_id")
                .table(AccessScope::Table)
                .col(AccessScope::TreeId)
                .col(AccessScope::Id)
                .unique()
                .to_owned(),
        )
        .await?;
    manager
        .create_table(
            Table::create()
                .table(AccessScopePerson::Table)
                .if_not_exists()
                .col(uuid(AccessScopePerson::TreeId))
                .col(uuid(AccessScopePerson::ScopeId))
                .col(uuid(AccessScopePerson::PersonId))
                .primary_key(
                    Index::create()
                        .col(AccessScopePerson::ScopeId)
                        .col(AccessScopePerson::PersonId),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_access_scope_person_scope")
                        .from(
                            AccessScopePerson::Table,
                            (AccessScopePerson::ScopeId, AccessScopePerson::TreeId),
                        )
                        .to(AccessScope::Table, (AccessScope::Id, AccessScope::TreeId))
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_access_scope_person_person")
                        .from(
                            AccessScopePerson::Table,
                            (AccessScopePerson::PersonId, AccessScopePerson::TreeId),
                        )
                        .to(Person::Table, (Person::Id, Person::TreeId))
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;
    manager
        .create_index(
            Index::create()
                .name("idx_access_scope_person_person_id")
                .table(AccessScopePerson::Table)
                .col(AccessScopePerson::PersonId)
                .to_owned(),
        )
        .await?;
    Ok(())
}

/// Invitations by email (docs/data-model.md §6.3): an address, the role
/// and scope it grants once accepted, and its own expiry.
async fn create_invitations(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(TreeInvitation::Table)
                .if_not_exists()
                .col(uuid(TreeInvitation::Id).primary_key())
                .col(uuid(TreeInvitation::TreeId))
                .col(string(TreeInvitation::Email))
                .col(string_len(TreeInvitation::Role, 8))
                .check(one_of("role", &["editor", "guest"]))
                .col(uuid_null(TreeInvitation::ScopeId))
                .col(timestamp_with_time_zone_null(
                    TreeInvitation::MembershipExpiresAt,
                ))
                .col(timestamp_with_time_zone(TreeInvitation::ExpiresAt))
                .col(uuid_null(TreeInvitation::CreatedBy))
                .col(timestamp_with_time_zone(TreeInvitation::CreatedAt))
                .col(timestamp_with_time_zone_null(TreeInvitation::LastSentAt))
                .col(timestamp_with_time_zone_null(TreeInvitation::AcceptedAt))
                .col(uuid_null(TreeInvitation::AcceptedBy))
                .col(timestamp_with_time_zone_null(TreeInvitation::DeclinedAt))
                .col(timestamp_with_time_zone_null(TreeInvitation::RevokedAt))
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_tree_invitation_tree")
                        .from(TreeInvitation::Table, TreeInvitation::TreeId)
                        .to(Tree::Table, Tree::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                // Removing a scope ends what it limited rather than
                // widening it to the whole tree.
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_tree_invitation_scope")
                        .from(TreeInvitation::Table, TreeInvitation::ScopeId)
                        .to(AccessScope::Table, AccessScope::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;
    // The owner lists a tree's invitations a page at a time, in id order,
    // which `(tree_id, id)` reads without sorting; a signed-in user's are
    // found by their address.
    manager
        .create_index(
            Index::create()
                .name("idx_tree_invitation_tree_id")
                .table(TreeInvitation::Table)
                .col(TreeInvitation::TreeId)
                .col(TreeInvitation::Id)
                .to_owned(),
        )
        .await?;
    manager
        .create_index(
            Index::create()
                .name("idx_tree_invitation_email")
                .table(TreeInvitation::Table)
                .col(TreeInvitation::Email)
                .to_owned(),
        )
        .await?;
    // One unanswered invitation per address and tree, so that two owners'
    // concurrent invitations of one address mail it once: a partial unique
    // index, which SQLite and PostgreSQL both support and sea-query does not
    // express. An expired one is deleted before its address is invited again.
    manager
        .get_connection()
        .execute_raw(Statement::from_string(
            manager.get_database_backend(),
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_tree_invitation_unanswered \
             ON tree_invitation (tree_id, email) \
             WHERE accepted_at IS NULL AND declined_at IS NULL AND revoked_at IS NULL",
        ))
        .await?;
    Ok(())
}

/// Share links (docs/data-model.md §6.4): only the token's digest is
/// stored.
async fn create_share_links(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(ShareLink::Table)
                .if_not_exists()
                .col(uuid(ShareLink::Id).primary_key())
                .col(uuid(ShareLink::TreeId))
                .col(string_len(ShareLink::TokenHash, 64))
                .col(string_null(ShareLink::Label))
                .col(uuid_null(ShareLink::ScopeId))
                .col(timestamp_with_time_zone_null(ShareLink::ExpiresAt))
                .col(uuid_null(ShareLink::CreatedBy))
                .col(timestamp_with_time_zone(ShareLink::CreatedAt))
                .col(timestamp_with_time_zone_null(ShareLink::RevokedAt))
                .col(timestamp_with_time_zone_null(ShareLink::LastUsedAt))
                .col(big_integer(ShareLink::UseCount).default(0))
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_share_link_tree")
                        .from(ShareLink::Table, ShareLink::TreeId)
                        .to(Tree::Table, Tree::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_share_link_scope")
                        .from(ShareLink::Table, ShareLink::ScopeId)
                        .to(AccessScope::Table, AccessScope::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;
    manager
        .create_index(
            Index::create()
                .name("idx_share_link_token_hash")
                .table(ShareLink::Table)
                .col(ShareLink::TokenHash)
                .unique()
                .to_owned(),
        )
        .await?;
    // A tree's links, listed a page at a time in id order.
    manager
        .create_index(
            Index::create()
                .name("idx_share_link_tree_id")
                .table(ShareLink::Table)
                .col(ShareLink::TreeId)
                .col(ShareLink::Id)
                .to_owned(),
        )
        .await?;
    Ok(())
}

/// Who may do what with each tree (docs/data-model.md §6.2): one row per
/// member, one `owner` per tree.
async fn create_memberships(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(TreeMembership::Table)
                .if_not_exists()
                .col(uuid(TreeMembership::Id).primary_key())
                .col(uuid(TreeMembership::TreeId))
                .col(uuid(TreeMembership::UserId))
                .col(string_len(TreeMembership::Role, 8))
                .check(one_of("role", &["owner", "editor", "guest"]))
                .col(uuid_null(TreeMembership::ScopeId))
                .col(uuid_null(TreeMembership::SelfPersonId))
                .col(timestamp_with_time_zone_null(TreeMembership::ExpiresAt))
                .col(uuid_null(TreeMembership::CreatedBy))
                .col(timestamp_with_time_zone(TreeMembership::CreatedAt))
                .col(timestamp_with_time_zone(TreeMembership::UpdatedAt))
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_tree_membership_tree")
                        .from(TreeMembership::Table, TreeMembership::TreeId)
                        .to(Tree::Table, Tree::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_tree_membership_user")
                        .from(TreeMembership::Table, TreeMembership::UserId)
                        .to(AppUser::Table, AppUser::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                // Removing a scope ends the membership it limited rather
                // than widening it to the whole tree.
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_tree_membership_scope")
                        .from(TreeMembership::Table, TreeMembership::ScopeId)
                        .to(AccessScope::Table, AccessScope::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                // A deleted person is nobody's "Who am I?" any more.
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_tree_membership_self_person")
                        .from(TreeMembership::Table, TreeMembership::SelfPersonId)
                        .to(Person::Table, Person::Id)
                        .on_delete(ForeignKeyAction::SetNull),
                )
                .to_owned(),
        )
        .await?;
    // What a person's deletion clears.
    manager
        .create_index(
            Index::create()
                .name("idx_tree_membership_self_person")
                .table(TreeMembership::Table)
                .col(TreeMembership::SelfPersonId)
                .to_owned(),
        )
        .await?;
    // One membership per user and tree; the access check reads it by both.
    manager
        .create_index(
            Index::create()
                .name("idx_tree_membership_tree_user")
                .table(TreeMembership::Table)
                .col(TreeMembership::TreeId)
                .col(TreeMembership::UserId)
                .unique()
                .to_owned(),
        )
        .await?;
    // A user's trees, for the tree list and erasure.
    manager
        .create_index(
            Index::create()
                .name("idx_tree_membership_user")
                .table(TreeMembership::Table)
                .col(TreeMembership::UserId)
                .col(TreeMembership::TreeId)
                .to_owned(),
        )
        .await?;
    // Exactly one owner per tree: a partial unique index, which SQLite and
    // PostgreSQL both support and sea-query does not express.
    manager
        .get_connection()
        .execute_raw(Statement::from_string(
            manager.get_database_backend(),
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_tree_membership_owner \
             ON tree_membership (tree_id) WHERE role = 'owner'",
        ))
        .await?;
    Ok(())
}

/// The access log (docs/data-model.md §6.7): no foreign key, since events
/// outlive the trees and users they name until their retention ends.
async fn create_access_events(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(AccessEvent::Table)
                .if_not_exists()
                .col(uuid(AccessEvent::Id).primary_key())
                .col(timestamp_with_time_zone(AccessEvent::OccurredAt))
                .col(string_len(AccessEvent::Kind, 32))
                .col(string_len(AccessEvent::PrincipalKind, 16))
                .col(uuid_null(AccessEvent::UserId))
                .col(uuid_null(AccessEvent::ApiClientId))
                .col(uuid_null(AccessEvent::ShareLinkId))
                .col(string_null(AccessEvent::ClientId))
                .col(uuid_null(AccessEvent::TreeId))
                .col(string_null(AccessEvent::Route))
                .col(string_len_null(AccessEvent::Permission, 32))
                .col(string_len(AccessEvent::Outcome, 32))
                .col(string_len_null(AccessEvent::RequestId, 64))
                .to_owned(),
        )
        .await?;
    // An owner reads their tree's events, a user's export their own, newest
    // first; the retention sweep deletes by age.
    for (name, first, second) in [
        (
            "idx_access_event_tree",
            AccessEvent::TreeId,
            Some(AccessEvent::Id),
        ),
        (
            "idx_access_event_user",
            AccessEvent::UserId,
            Some(AccessEvent::Id),
        ),
        (
            "idx_access_event_occurred_at",
            AccessEvent::OccurredAt,
            None,
        ),
    ] {
        let mut index = Index::create();
        index.name(name).table(AccessEvent::Table).col(first);
        if let Some(second) = second {
            index.col(second);
        }
        manager.create_index(index.to_owned()).await?;
    }
    Ok(())
}

/// In-app notifications and their email preferences
/// (docs/messaging.md §2, §10): rows of one user each, holding ids only.
/// `tree_id`, `actor_user_id` and `subject_id` have no foreign key: a
/// notification outlives what it names, and reads *no longer shared* once
/// its reader may not see the tree, until its 90 days are over.
async fn create_notifications(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(Notification::Table)
                .if_not_exists()
                .col(uuid(Notification::Id).primary_key())
                .col(uuid(Notification::UserId))
                .col(string_len(Notification::Kind, 32))
                .col(uuid_null(Notification::TreeId))
                .col(uuid_null(Notification::ActorUserId))
                .col(uuid_null(Notification::SubjectId))
                .col(integer(Notification::Count).default(1))
                .col(timestamp_with_time_zone(Notification::CreatedAt))
                .col(timestamp_with_time_zone_null(Notification::ReadAt))
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_notification_user")
                        .from(Notification::Table, Notification::UserId)
                        .to(AppUser::Table, AppUser::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;
    // A user's notifications, newest first, and those after a cursor: ids
    // are UUID v7, so they sort by creation.
    manager
        .create_index(
            Index::create()
                .name("idx_notification_user")
                .table(Notification::Table)
                .col(Notification::UserId)
                .col(Notification::Id)
                .to_owned(),
        )
        .await?;
    // The retention sweep deletes by age.
    manager
        .create_index(
            Index::create()
                .name("idx_notification_created_at")
                .table(Notification::Table)
                .col(Notification::CreatedAt)
                .to_owned(),
        )
        .await?;
    manager
        .create_table(
            Table::create()
                .table(NotificationPreference::Table)
                .if_not_exists()
                .col(uuid(NotificationPreference::UserId))
                .col(string_len(NotificationPreference::Kind, 32))
                .col(string_len(NotificationPreference::Email, 12))
                .primary_key(
                    Index::create()
                        .col(NotificationPreference::UserId)
                        .col(NotificationPreference::Kind),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_notification_preference_user")
                        .from(
                            NotificationPreference::Table,
                            NotificationPreference::UserId,
                        )
                        .to(AppUser::Table, AppUser::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;
    Ok(())
}

/// The mail OxidGene queues for its SMTP relay (docs/messaging.md §3.3):
/// written in the transaction of what causes it, sent by the worker.
async fn create_mail_outbox(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(MailOutbox::Table)
                .if_not_exists()
                .col(uuid(MailOutbox::Id).primary_key())
                .col(uuid_null(MailOutbox::UserId))
                .col(string(MailOutbox::ToAddress))
                .col(string_len(MailOutbox::Kind, 32))
                .col(string_len(MailOutbox::Language, 16))
                .col(text(MailOutbox::Params))
                .col(string_len(MailOutbox::Status, 12))
                .col(integer(MailOutbox::Attempts).default(0))
                .col(timestamp_with_time_zone(MailOutbox::NextAttemptAt))
                .col(timestamp_with_time_zone_null(MailOutbox::LeaseUntil))
                .col(timestamp_with_time_zone_null(MailOutbox::SentAt))
                .col(string_len_null(MailOutbox::ErrorCategory, 32))
                .col(timestamp_with_time_zone(MailOutbox::CreatedAt))
                // A mail to a user goes with their account's deletion; the
                // erasure, which keeps the row, deletes it itself.
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_mail_outbox_user")
                        .from(MailOutbox::Table, MailOutbox::UserId)
                        .to(AppUser::Table, AppUser::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;
    // The worker claims the due rows of a status; a user's pending digest is
    // found by user and kind.
    for (name, columns) in [
        (
            "idx_mail_outbox_due",
            [MailOutbox::Status, MailOutbox::NextAttemptAt],
        ),
        (
            "idx_mail_outbox_user",
            [MailOutbox::UserId, MailOutbox::Kind],
        ),
    ] {
        let mut index = Index::create();
        index.name(name).table(MailOutbox::Table);
        for column in columns {
            index.col(column);
        }
        manager.create_index(index.to_owned()).await?;
    }
    Ok(())
}
