//! Database errors as the domain reports them.
//!
//! A `DbErr` is classified here, once, from what the driver itself says —
//! SQLite's result code, PostgreSQL's SQLSTATE — into the
//! [`DatabaseErrorKind`] every layer above reads: the error contract turns a
//! broken constraint into a conflict and a busy database into a retryable
//! answer, and the logs name the kind. Nothing above this module reads an
//! error's text, which may hold SQL, values and paths.

use oxidgene_core::error::{DatabaseError, DatabaseErrorKind, OxidGeneError};
use sea_orm::sqlx;
use sea_orm::{DbErr, RuntimeErr};

/// A database error as the domain reports it: its kind, with the driver's
/// error as its cause.
///
/// Public so that every layer maps a `DbErr` the same way: the API's services
/// open and commit transactions and run a few queries of their own.
pub fn db_err(error: DbErr) -> OxidGeneError {
    OxidGeneError::Database(DatabaseError::new(db_error_kind(&error), error))
}

/// A stored value that could not be read as its type.
pub(crate) fn conversion_err(error: impl std::fmt::Display) -> OxidGeneError {
    OxidGeneError::database(DatabaseErrorKind::Conversion, error.to_string())
}

/// What kind of failure `error` is.
pub fn db_error_kind(error: &DbErr) -> DatabaseErrorKind {
    use DatabaseErrorKind as Kind;
    match error {
        DbErr::ConnectionAcquire(_) => Kind::ConnectionAcquire,
        DbErr::Conn(cause) => driver_kind(cause).unwrap_or(Kind::Connection),
        DbErr::Exec(cause) => driver_kind(cause).unwrap_or(Kind::Execution),
        DbErr::Query(cause) => driver_kind(cause).unwrap_or(Kind::Query),
        DbErr::RecordNotFound(_) => Kind::RecordNotFound,
        DbErr::RecordNotInserted | DbErr::RecordNotUpdated => Kind::RecordNotWritten,
        DbErr::TryIntoErr { .. } | DbErr::ConvertFromU64(_) | DbErr::Type(_) | DbErr::Json(_) => {
            Kind::Conversion
        }
        DbErr::Migration(_) => Kind::Migration,
        _ => Kind::Other,
    }
}

/// The kind the driver's own error names, when it names one.
fn driver_kind(cause: &RuntimeErr) -> Option<DatabaseErrorKind> {
    use DatabaseErrorKind as Kind;
    let RuntimeErr::SqlxError(error) = cause else {
        return None;
    };
    match &**error {
        sqlx::Error::Database(error) => server_kind(error.as_ref()),
        sqlx::Error::PoolTimedOut => Some(Kind::ConnectionAcquire),
        sqlx::Error::PoolClosed
        | sqlx::Error::Io(_)
        | sqlx::Error::Tls(_)
        | sqlx::Error::Protocol(_)
        | sqlx::Error::WorkerCrashed => Some(Kind::Connection),
        _ => None,
    }
}

/// The kind of an error the database itself returned.
fn server_kind(error: &dyn sqlx::error::DatabaseError) -> Option<DatabaseErrorKind> {
    use DatabaseErrorKind as Kind;
    use sqlx::error::ErrorKind;
    match error.kind() {
        ErrorKind::UniqueViolation => return Some(Kind::UniqueViolation),
        ErrorKind::ForeignKeyViolation => return Some(Kind::ForeignKeyViolation),
        ErrorKind::NotNullViolation => return Some(Kind::NotNullViolation),
        ErrorKind::CheckViolation => return Some(Kind::CheckViolation),
        _ => {}
    }
    if error
        .try_downcast_ref::<sqlx::sqlite::SqliteError>()
        .is_some()
    {
        return error.code().and_then(|code| sqlite_kind(&code));
    }
    #[cfg(feature = "postgres")]
    if let Some(error) = error.try_downcast_ref::<sqlx::postgres::PgDatabaseError>() {
        return postgres_kind(error.code());
    }
    None
}

/// The kind of SQLite's extended result code `code`, a decimal number whose
/// low byte is the primary code (<https://www.sqlite.org/rescode.html>).
fn sqlite_kind(code: &str) -> Option<DatabaseErrorKind> {
    /// `SQLITE_BUSY`: another connection holds the lock this one needs.
    const BUSY: u32 = 5;
    /// `SQLITE_LOCKED`: a conflict within the connection or a shared cache.
    const LOCKED: u32 = 6;
    /// `SQLITE_CANTOPEN`: the database file could not be opened.
    const CANTOPEN: u32 = 14;
    match code.parse::<u32>().ok()? & 0xff {
        BUSY | LOCKED => Some(DatabaseErrorKind::Busy),
        CANTOPEN => Some(DatabaseErrorKind::Connection),
        _ => None,
    }
}

/// The kind of PostgreSQL's SQLSTATE `code`
/// (<https://www.postgresql.org/docs/current/errcodes-appendix.html>).
#[cfg_attr(
    all(not(feature = "postgres"), not(test)),
    expect(dead_code, reason = "read by the PostgreSQL driver's errors only")
)]
fn postgres_kind(code: &str) -> Option<DatabaseErrorKind> {
    use DatabaseErrorKind as Kind;
    match code {
        // serialization_failure, deadlock_detected.
        "40001" | "40P01" => Some(Kind::SerializationFailure),
        // lock_not_available: `lock_timeout` ran out, or `NOWAIT`.
        "55P03" => Some(Kind::Busy),
        // query_canceled (`statement_timeout`),
        // idle_in_transaction_session_timeout, transaction_timeout.
        "57014" | "25P03" | "25P04" => Some(Kind::Timeout),
        // too_many_connections.
        "53300" => Some(Kind::ConnectionAcquire),
        // Class 08, connection exception; class 57P, the server going away.
        code if code.starts_with("08") || code.starts_with("57P") => Some(Kind::Connection),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use sea_orm::ConnAcquireErr;

    use super::*;

    #[test]
    fn sea_orm_errors_without_a_driver_cause_are_named_by_their_variant() {
        use DatabaseErrorKind as Kind;
        let internal = || RuntimeErr::Internal("private".to_owned());
        for (error, kind) in [
            (
                DbErr::ConnectionAcquire(ConnAcquireErr::Timeout),
                Kind::ConnectionAcquire,
            ),
            (DbErr::Conn(internal()), Kind::Connection),
            (DbErr::Exec(internal()), Kind::Execution),
            (DbErr::Query(internal()), Kind::Query),
            (
                DbErr::RecordNotFound("private".to_owned()),
                Kind::RecordNotFound,
            ),
            (DbErr::RecordNotInserted, Kind::RecordNotWritten),
            (DbErr::RecordNotUpdated, Kind::RecordNotWritten),
            (DbErr::Json("private".to_owned()), Kind::Conversion),
            (DbErr::Type("private".to_owned()), Kind::Conversion),
            (DbErr::Migration("private".to_owned()), Kind::Migration),
            (DbErr::Custom("private".to_owned()), Kind::Other),
        ] {
            assert_eq!(db_error_kind(&error), kind, "{error:?}");
        }
    }

    #[test]
    fn a_pool_timeout_is_a_failure_to_acquire_a_connection() {
        let error = DbErr::Conn(RuntimeErr::SqlxError(std::sync::Arc::new(
            sqlx::Error::PoolTimedOut,
        )));
        assert_eq!(db_error_kind(&error), DatabaseErrorKind::ConnectionAcquire);
    }

    #[test]
    fn sqlite_result_codes_are_read_by_their_primary_code() {
        use DatabaseErrorKind as Kind;
        // SQLITE_BUSY, SQLITE_BUSY_SNAPSHOT, SQLITE_BUSY_TIMEOUT,
        // SQLITE_LOCKED, SQLITE_LOCKED_SHAREDCACHE.
        for code in ["5", "517", "773", "6", "262"] {
            assert_eq!(sqlite_kind(code), Some(Kind::Busy), "{code}");
        }
        assert_eq!(sqlite_kind("14"), Some(Kind::Connection));
        // SQLITE_ERROR, SQLITE_FULL: no kind of their own.
        assert_eq!(sqlite_kind("1"), None);
        assert_eq!(sqlite_kind("13"), None);
        assert_eq!(sqlite_kind("not a code"), None);
    }

    #[test]
    fn postgres_states_are_read_by_their_code() {
        use DatabaseErrorKind as Kind;
        for (code, kind) in [
            ("40001", Some(Kind::SerializationFailure)),
            ("40P01", Some(Kind::SerializationFailure)),
            ("55P03", Some(Kind::Busy)),
            ("57014", Some(Kind::Timeout)),
            ("25P03", Some(Kind::Timeout)),
            ("53300", Some(Kind::ConnectionAcquire)),
            ("08006", Some(Kind::Connection)),
            ("57P01", Some(Kind::Connection)),
            ("42601", None),
        ] {
            assert_eq!(postgres_kind(code), kind, "{code}");
        }
    }
}
