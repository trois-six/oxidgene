//! What keeps a SQLite database file safe to open and possible to recover:
//! the identity of its schema, a copy of the file, and a check of its
//! integrity (docs/architecture.md §8.4).
//!
//! **Schema identity.** While the product is unreleased the schema is the
//! single initial migration, edited in place: a file created by an earlier
//! edit of it counts as migrated and silently holds another schema. So a
//! file records, when it is created, the digest of the schema this build
//! created in it ([`record_schema`]); and a file that already exists is
//! opened only when the digest it recorded is the one this build creates
//! ([`schema_state`]), which is computed by migrating a scratch in-memory
//! database, never kept by hand. PostgreSQL databases are not checked.
//!
//! **Copies and checks.** [`snapshot_into`] writes a consistent copy of the
//! database with `VACUUM INTO`, and [`check_integrity`] runs `PRAGMA
//! quick_check` and `PRAGMA foreign_key_check`. When and where is the
//! caller's policy (`oxidgene_api::backup`).

use std::path::Path;

use sea_orm::{
    ConnectOptions, ConnectionTrait, Database, DatabaseBackend, DatabaseConnection, DbErr,
    Statement,
};
use sea_orm_migration::MigratorTrait;
use sha2::{Digest, Sha256};

use crate::Migrator;

/// The key of the schema's digest in `schema_meta`.
const SCHEMA_DIGEST: &str = "schema_digest";

/// Why a database could not be brought to this build's schema.
#[derive(Debug)]
pub enum MigrationError {
    /// A statement failed.
    Database(DbErr),
    /// The database file was created by another version of the schema than
    /// the one this build creates: it must be recreated and its genealogy
    /// reimported (docs/data-model.md, Migrations).
    SchemaMismatch,
}

impl std::fmt::Display for MigrationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Database(error) => write!(f, "{error}"),
            Self::SchemaMismatch => {
                f.write_str("the database was created with another schema than this version's")
            }
        }
    }
}

impl std::error::Error for MigrationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            Self::SchemaMismatch => None,
        }
    }
}

impl From<DbErr> for MigrationError {
    fn from(error: DbErr) -> Self {
        Self::Database(error)
    }
}

/// What a database holds before this run migrates it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchemaState {
    /// No migration was ever applied: this run creates the schema.
    Fresh,
    /// A SQLite file that already holds this build's schema, or an earlier
    /// released one with `pending` migrations to apply.
    Existing { pending: bool },
    /// A PostgreSQL database: its schema's identity is not checked, and
    /// whether it is new is the migrator's to find under its lock.
    Unchecked,
}

/// The digest of the schema of SQLite database `db`: the SHA-256, in
/// hexadecimal, of every object of `sqlite_master` — its kind, name, table
/// and the SQL that created it — in name order. SQLite's own tables
/// (`sqlite_stat1`, `sqlite_sequence`) come and go with use and are left
/// out.
pub async fn schema_digest(db: &impl ConnectionTrait) -> Result<String, DbErr> {
    let rows = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT type, name, tbl_name, COALESCE(sql, '') AS sql FROM sqlite_master \
             WHERE name NOT LIKE 'sqlite_%' ORDER BY type, name",
        ))
        .await?;
    let mut hasher = Sha256::new();
    for row in rows {
        for column in ["type", "name", "tbl_name", "sql"] {
            hasher.update(row.try_get::<String>("", column)?.as_bytes());
            hasher.update([0]);
        }
    }
    let mut digest = String::with_capacity(64);
    for byte in hasher.finalize() {
        use std::fmt::Write as _;
        let _ = write!(digest, "{byte:02x}");
    }
    Ok(digest)
}

/// The digest of the schema this build creates, computed by migrating a
/// scratch in-memory database: a few tens of milliseconds, paid once, when
/// an existing file is opened.
pub async fn expected_digest() -> Result<String, DbErr> {
    let mut options = ConnectOptions::new("sqlite::memory:");
    options.sqlx_logging(false);
    let scratch = Database::connect(options).await?;
    Migrator::up(&scratch, None).await?;
    let digest = schema_digest(&scratch).await?;
    scratch.close().await?;
    Ok(digest)
}

/// Whether the identity of a file's schema is checked at all: while the
/// schema is still the single initial migration. Once an incremental
/// migration exists the initial one is frozen, a migrated file's schema no
/// longer reads like a fresh one's, and the check gives way to the golden
/// digests of the released schemas (docs/development.md, First release).
fn is_checked() -> bool {
    Migrator::migrations().len() == 1
}

/// Whether SQLite database `db` holds a schema already: a migration was
/// recorded in it.
async fn is_migrated(db: &DatabaseConnection) -> Result<bool, DbErr> {
    let recorded = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'seaql_migrations'",
        ))
        .await?;
    if recorded.is_none() {
        return Ok(false);
    }
    Ok(!Migrator::get_applied_migrations(db).await?.is_empty())
}

/// The digest `db` recorded when it was created, if it recorded one. A file
/// that predates the record has no `schema_meta`: the query fails, which is
/// as much a mismatch as another digest.
async fn recorded_digest(db: &DatabaseConnection) -> Option<String> {
    db.query_one_raw(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "SELECT value FROM schema_meta WHERE key = ?",
        [SCHEMA_DIGEST.into()],
    ))
    .await
    .ok()
    .flatten()
    .and_then(|row| row.try_get::<String>("", "value").ok())
}

/// What `db` holds, before it is migrated: see [`SchemaState`].
///
/// # Errors
///
/// [`MigrationError::SchemaMismatch`] for a SQLite file created with
/// another schema than this build's.
pub async fn schema_state(db: &DatabaseConnection) -> Result<SchemaState, MigrationError> {
    if db.get_database_backend() != DatabaseBackend::Sqlite {
        return Ok(SchemaState::Unchecked);
    }
    if !is_migrated(db).await? {
        return Ok(SchemaState::Fresh);
    }
    if is_checked() && recorded_digest(db).await != Some(expected_digest().await?) {
        return Err(MigrationError::SchemaMismatch);
    }
    let pending = !Migrator::get_pending_migrations(db).await?.is_empty();
    Ok(SchemaState::Existing { pending })
}

/// Record in the SQLite database `db`, just created, the digest of its
/// schema.
pub(crate) async fn record_schema(db: &DatabaseConnection) -> Result<(), DbErr> {
    if !is_checked() {
        return Ok(());
    }
    let digest = schema_digest(db).await?;
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "INSERT INTO schema_meta (key, value) VALUES (?, ?) \
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        [SCHEMA_DIGEST.into(), digest.into()],
    ))
    .await?;
    Ok(())
}

/// What `PRAGMA quick_check` and `PRAGMA foreign_key_check` found: counts
/// only, their messages naming tables, row ids and values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct IntegrityReport {
    /// The problems `quick_check` reported in the file's pages and indexes.
    pub damaged: usize,
    /// The rows naming a row that does not exist.
    pub dangling: usize,
}

impl IntegrityReport {
    /// Whether the file is sound: neither check found anything.
    #[must_use]
    pub const fn is_sound(&self) -> bool {
        self.damaged == 0 && self.dangling == 0
    }
}

/// Check the integrity of SQLite database `db`: the structure of its pages
/// and indexes (`PRAGMA quick_check`, which reads every page but skips the
/// comparison of each index with its table that makes `integrity_check`
/// several times slower), and its foreign keys (`PRAGMA
/// foreign_key_check`).
pub async fn check_integrity(db: &DatabaseConnection) -> Result<IntegrityReport, DbErr> {
    let pragma = |sql: &'static str| Statement::from_string(DatabaseBackend::Sqlite, sql);
    let damaged = db
        .query_all_raw(pragma("PRAGMA quick_check"))
        .await?
        .iter()
        .filter(|row| {
            row.try_get_by_index::<String>(0)
                .map_or(true, |line| line != "ok")
        })
        .count();
    // One row per broken key of a row: a row naming a missing person
    // breaks its single and its composite key alike, and counts once.
    let dangling = db
        .query_one_raw(pragma(
            "SELECT COUNT(*) FROM (SELECT DISTINCT \"table\", rowid FROM pragma_foreign_key_check)",
        ))
        .await?
        .map_or(Ok(0), |row| row.try_get_by_index::<i64>(0))?;
    let dangling = usize::try_from(dangling).unwrap_or(usize::MAX);
    Ok(IntegrityReport { damaged, dangling })
}

/// Write a consistent copy of SQLite database `db` to `destination`, which
/// must not exist, with `VACUUM INTO`: the copy holds what was committed
/// when it started, without the free pages, whatever is written meanwhile.
pub async fn snapshot_into(db: &DatabaseConnection, destination: &Path) -> Result<(), DbErr> {
    let Some(path) = destination.to_str() else {
        return Err(DbErr::Custom(
            "the snapshot's path is not valid UTF-8".to_owned(),
        ));
    };
    // `VACUUM INTO` takes a string literal, not a parameter.
    let literal = path.replace('\'', "''");
    db.execute_unprepared(&format!("VACUUM INTO '{literal}'"))
        .await?;
    Ok(())
}
