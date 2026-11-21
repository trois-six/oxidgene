//! Shared error types for OxidGene.

use thiserror::Error;
use uuid::Uuid;

/// Top-level error type for OxidGene operations.
#[derive(Debug, Error)]
pub enum OxidGeneError {
    /// Entity not found.
    #[error("{entity} with id {id} not found")]
    NotFound { entity: &'static str, id: Uuid },

    /// Validation error.
    #[error("Validation error: {0}")]
    Validation(String),

    /// The request conflicts with the current state: another operation holds
    /// what it needs, or it would break an invariant.
    #[error("Conflict: {0}")]
    Conflict(String),

    /// The request carries no valid credential, where one is required: the
    /// web build's authentication (docs/authentication.md).
    #[error("Unauthenticated")]
    Unauthenticated,

    /// The viewer is identified but may not do what the request asks.
    #[error("Forbidden")]
    Forbidden,

    /// Over a rate limit: the request may be tried again after
    /// `retry_after` seconds (docs/authorization.md §4.3).
    #[error("Rate limited")]
    RateLimited { retry_after: u64 },

    /// What the request sends is larger than the server takes: a file, an
    /// archive or what it expands to, over a size limit.
    #[error("Too large: {0}")]
    TooLarge(String),

    /// A tree's media would pass its storage quota.
    #[error("Storage quota exceeded")]
    QuotaExceeded,

    /// Database error: its kind, classified once where the driver's error
    /// is read (`oxidgene_db::repo::db_err`), and its cause.
    #[error("Database error: {0}")]
    Database(DatabaseError),

    /// GEDCOM parsing error.
    #[error("GEDCOM error: {0}")]
    Gedcom(String),

    /// IO error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Internal error.
    #[error("Internal error: {0}")]
    Internal(String),

    /// A cited source could not be resolved to its page on an archive portal.
    #[error("Archive error: {}", .0.code())]
    Archive(ArchiveFailure),
}

/// What kind of failure a database error is: a bounded list, decided from
/// the driver's own error codes on both backends, never from its message.
///
/// The public error contract and the logs are written from it
/// (docs/cross-cutting.md §4, §5.4): a broken constraint is a conflict, a
/// busy database a retryable answer, and `as_str` is the `error.kind` of a
/// log line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DatabaseErrorKind {
    /// A unique index or a primary key refused a second row.
    UniqueViolation,
    /// A foreign key refused a row naming a record that is not there, or
    /// the deletion of a record still named.
    ForeignKeyViolation,
    /// A `CHECK` refused a row breaking an invariant of the model.
    CheckViolation,
    /// A required column was given no value.
    NotNullViolation,
    /// The database is locked by another writer and the wait for it ran
    /// out: SQLite's `SQLITE_BUSY` and `SQLITE_LOCKED`, PostgreSQL's
    /// `lock_not_available`.
    Busy,
    /// PostgreSQL ended the transaction for a concurrent one to go on: a
    /// serialization failure or a deadlock. Running it again succeeds.
    SerializationFailure,
    /// The database cancelled a statement or a transaction that ran past
    /// its time limit.
    Timeout,
    /// No connection could be taken from the pool in time.
    ConnectionAcquire,
    /// The connection to the database failed or was lost.
    Connection,
    /// A statement that writes failed for another reason.
    Execution,
    /// A statement that reads failed for another reason.
    Query,
    /// A row expected to exist was not found.
    RecordNotFound,
    /// A write changed no row.
    RecordNotWritten,
    /// A stored value could not be read as its type.
    Conversion,
    /// A migration failed.
    Migration,
    /// A stored person projection could not be encoded or decoded.
    ProjectionDecode,
    /// Anything else.
    Other,
}

impl DatabaseErrorKind {
    /// The kind's stable name, as logs write it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UniqueViolation => "unique_violation",
            Self::ForeignKeyViolation => "foreign_key_violation",
            Self::CheckViolation => "check_violation",
            Self::NotNullViolation => "not_null_violation",
            Self::Busy => "busy",
            Self::SerializationFailure => "serialization_failure",
            Self::Timeout => "timeout",
            Self::ConnectionAcquire => "connection_acquire",
            Self::Connection => "connection",
            Self::Execution => "execution",
            Self::Query => "query",
            Self::RecordNotFound => "record_not_found",
            Self::RecordNotWritten => "record_not_written",
            Self::Conversion => "conversion",
            Self::Migration => "migration",
            Self::ProjectionDecode => "projection_decode",
            Self::Other => "other",
        }
    }

    /// Whether a constraint of the schema refused the write: the request
    /// conflicts with the stored state or with an invariant of the model.
    #[must_use]
    pub const fn is_constraint_violation(self) -> bool {
        matches!(
            self,
            Self::UniqueViolation | Self::ForeignKeyViolation | Self::CheckViolation
        )
    }

    /// Whether the same request, sent again, may succeed: the database was
    /// busy, ended the transaction for a concurrent one, or had no
    /// connection free. The write changed nothing.
    #[must_use]
    pub const fn is_retryable(self) -> bool {
        matches!(
            self,
            Self::Busy | Self::SerializationFailure | Self::ConnectionAcquire
        )
    }
}

impl std::fmt::Display for DatabaseErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A database failure: its [`DatabaseErrorKind`] and its cause.
///
/// It displays as its kind alone. The cause — the driver's error, whose
/// text may hold SQL, values and paths — is reachable through
/// [`std::error::Error::source`] and `Debug`, for a developer reading a
/// failed test, and is never written to a client or a log
/// (docs/cross-cutting.md §4.1).
#[derive(Debug)]
pub struct DatabaseError {
    kind: DatabaseErrorKind,
    source: Box<dyn std::error::Error + Send + Sync>,
}

impl DatabaseError {
    /// A failure of kind `kind` caused by `source`.
    pub fn new(
        kind: DatabaseErrorKind,
        source: impl Into<Box<dyn std::error::Error + Send + Sync>>,
    ) -> Self {
        Self {
            kind,
            source: source.into(),
        }
    }

    /// What kind of failure it is.
    #[must_use]
    pub const fn kind(&self) -> DatabaseErrorKind {
        self.kind
    }
}

impl std::fmt::Display for DatabaseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.kind.as_str())
    }
}

impl std::error::Error for DatabaseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&*self.source)
    }
}

/// A failure described in words, of no kind the driver names.
impl From<String> for DatabaseError {
    fn from(message: String) -> Self {
        Self::new(DatabaseErrorKind::Other, message)
    }
}

/// A failure described in words, of no kind the driver names.
impl From<&str> for DatabaseError {
    fn from(message: &str) -> Self {
        Self::new(DatabaseErrorKind::Other, message)
    }
}

impl OxidGeneError {
    /// A database failure of kind `kind`, described by `message`.
    pub fn database(kind: DatabaseErrorKind, message: impl Into<String>) -> Self {
        Self::Database(DatabaseError::new(kind, message.into()))
    }
}

/// Why a cited source has no archive target, each with a stable code.
///
/// The resolution's own failures mirror `oxidgene-archives`' `ResolveError`
/// codes; the interface translates each as `archive_viewer.<code>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveFailure {
    /// Nothing the source, the citation, its repositories or its event say
    /// names a register of an archive: no archive citation.
    NotACitation,
    /// The archive is not catalogued, or no collection with an adapter holds
    /// the cited act.
    NoAdapter,
    /// The portal answered, but not as its adapter expects.
    UnexpectedResponse,
    /// The portal answered with an anti-bot challenge instead of its page.
    Challenged,
    /// The portal did not answer in time.
    Timeout,
    /// The portal could not be reached, or answered with a server error.
    Unreachable,
}

impl ArchiveFailure {
    pub const fn code(self) -> &'static str {
        match self {
            Self::NotACitation => "not_an_archive_citation",
            Self::NoAdapter => "no_adapter",
            Self::UnexpectedResponse => "unexpected_response",
            Self::Challenged => "challenged",
            Self::Timeout => "timeout",
            Self::Unreachable => "unreachable",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_not_found_error_display() {
        let id = Uuid::nil();
        let err = OxidGeneError::NotFound {
            entity: "Person",
            id,
        };
        assert_eq!(err.to_string(), format!("Person with id {id} not found"));
    }

    #[test]
    fn a_database_error_displays_its_kind_and_keeps_its_cause_aside() {
        let err = OxidGeneError::database(
            DatabaseErrorKind::UniqueViolation,
            "UNIQUE constraint failed: fixture.private_column",
        );
        assert_eq!(err.to_string(), "Database error: unique_violation");
        let OxidGeneError::Database(error) = &err else {
            panic!("a database error");
        };
        assert_eq!(error.kind(), DatabaseErrorKind::UniqueViolation);
        let cause = std::error::Error::source(error).expect("the cause");
        assert!(cause.to_string().contains("private_column"));

        let described: DatabaseError = "something else".into();
        assert_eq!(described.kind(), DatabaseErrorKind::Other);
        assert!(DatabaseErrorKind::CheckViolation.is_constraint_violation());
        assert!(!DatabaseErrorKind::NotNullViolation.is_constraint_violation());
        assert!(DatabaseErrorKind::Busy.is_retryable());
        assert!(DatabaseErrorKind::SerializationFailure.is_retryable());
        assert!(DatabaseErrorKind::ConnectionAcquire.is_retryable());
        assert!(!DatabaseErrorKind::Timeout.is_retryable());
    }

    #[test]
    fn archive_errors_display_their_code_only() {
        let err = OxidGeneError::Archive(ArchiveFailure::UnexpectedResponse);
        assert_eq!(err.to_string(), "Archive error: unexpected_response");
    }

    #[test]
    fn test_validation_error_display() {
        let err = OxidGeneError::Validation("name is required".to_string());
        assert_eq!(err.to_string(), "Validation error: name is required");
    }
}
