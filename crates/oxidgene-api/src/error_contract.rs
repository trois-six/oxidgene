use oxidgene_core::OxidGeneError;
use oxidgene_core::error::{ArchiveFailure, DatabaseErrorKind};

pub(crate) struct ErrorContract {
    pub code: &'static str,
    pub message: &'static str,
    pub unexpected: bool,
}

/// What a client is told of a write the state refuses.
const CONFLICT: ErrorContract = ErrorContract {
    code: "conflict",
    message: "The request conflicts with the current state",
    unexpected: false,
};

pub(crate) fn classify(error: &OxidGeneError) -> ErrorContract {
    match error {
        OxidGeneError::NotFound { .. } => ErrorContract {
            code: "not_found",
            message: "The requested resource was not found",
            unexpected: false,
        },
        OxidGeneError::Validation(_) => ErrorContract {
            code: "validation_error",
            message: "The request is invalid",
            unexpected: false,
        },
        OxidGeneError::Conflict(_) => CONFLICT,
        OxidGeneError::Unauthenticated => ErrorContract {
            code: "unauthenticated",
            message: "Authentication is required",
            unexpected: false,
        },
        OxidGeneError::Forbidden => ErrorContract {
            code: "forbidden",
            message: "The request is not allowed",
            unexpected: false,
        },
        OxidGeneError::RateLimited { .. } => ErrorContract {
            code: "rate_limited",
            message: "Too many requests; try again later",
            unexpected: false,
        },
        OxidGeneError::TooLarge(_) => ErrorContract {
            code: "payload_too_large",
            message: "The request exceeds the size limit",
            unexpected: false,
        },
        OxidGeneError::QuotaExceeded => ErrorContract {
            code: "quota_exceeded",
            message: "The tree's media storage quota is exceeded",
            unexpected: false,
        },
        OxidGeneError::Database(error) => database_contract(error.kind()),
        OxidGeneError::Gedcom(_) => ErrorContract {
            code: "gedcom_error",
            message: "The genealogy data is invalid or unsupported",
            unexpected: false,
        },
        OxidGeneError::Io(_) => ErrorContract {
            code: "io_error",
            message: "The request could not be completed",
            unexpected: true,
        },
        OxidGeneError::Internal(_) => ErrorContract {
            code: "internal_error",
            message: "The request could not be completed",
            unexpected: true,
        },
        // An archive portal's failure is the portal's, not the server's: no
        // correlation ID, and the service has logged the archive and code.
        OxidGeneError::Archive(failure) => ErrorContract {
            code: failure.code(),
            message: archive_message(*failure),
            unexpected: false,
        },
    }
}

/// A database failure, by its kind: a row a constraint of the schema
/// refused — a second link where one may exist, a record that is no longer
/// there, a broken invariant — is the conflict its service would have
/// reported had it seen it first, not a server failure.
fn database_contract(kind: DatabaseErrorKind) -> ErrorContract {
    if kind.is_constraint_violation() {
        return CONFLICT;
    }
    ErrorContract {
        code: "database_error",
        message: "The request could not be completed",
        unexpected: true,
    }
}

fn archive_message(failure: ArchiveFailure) -> &'static str {
    match failure {
        ArchiveFailure::NotACitation => "The source is not an archive citation",
        ArchiveFailure::NoAdapter => "No catalogued archive collection holds the cited act",
        ArchiveFailure::UnexpectedResponse => "The archive portal did not answer as expected",
        ArchiveFailure::Challenged => "The archive portal answered with an anti-bot challenge",
        ArchiveFailure::Timeout => "The archive portal did not answer in time",
        ArchiveFailure::Unreachable => "The archive portal could not be reached",
    }
}

/// A bounded category of an error's cause, for logs and span fields.
///
/// The public code says which family failed (`database_error`); this says
/// what kind of failure it was (`busy`, `connection_acquire`, `not_found`),
/// which is what an operator investigating a request ID needs first. It is
/// always one of a fixed set of names, never the message: messages carry
/// SQL values, filesystem paths, and genealogy.
pub(crate) fn error_kind(error: &OxidGeneError) -> &'static str {
    match error {
        OxidGeneError::NotFound { .. } => "not_found",
        OxidGeneError::Validation(_) => "validation",
        OxidGeneError::Conflict(_) => "conflict",
        OxidGeneError::Unauthenticated => "unauthenticated",
        OxidGeneError::Forbidden => "forbidden",
        OxidGeneError::RateLimited { .. } => "rate_limited",
        OxidGeneError::TooLarge(_) => "too_large",
        OxidGeneError::QuotaExceeded => "quota_exceeded",
        OxidGeneError::Gedcom(_) => "gedcom",
        OxidGeneError::Database(error) => error.kind().as_str(),
        OxidGeneError::Io(error) => io_kind(error.kind()),
        OxidGeneError::Internal(message) => internal_kind(message),
        OxidGeneError::Archive(failure) => failure.code(),
    }
}

fn io_kind(kind: std::io::ErrorKind) -> &'static str {
    use std::io::ErrorKind;
    match kind {
        ErrorKind::NotFound => "io_not_found",
        ErrorKind::PermissionDenied => "io_permission_denied",
        ErrorKind::AlreadyExists => "io_already_exists",
        ErrorKind::StorageFull | ErrorKind::QuotaExceeded => "io_storage_full",
        ErrorKind::TimedOut => "io_timed_out",
        ErrorKind::UnexpectedEof | ErrorKind::InvalidData => "io_invalid_data",
        ErrorKind::ConnectionRefused
        | ErrorKind::ConnectionReset
        | ErrorKind::ConnectionAborted
        | ErrorKind::BrokenPipe => "io_connection",
        _ => "io_other",
    }
}

/// Internal errors mostly wrap a blocking task that did not finish: Tokio
/// renders those as "task … panicked" or "task … was cancelled".
fn internal_kind(message: &str) -> &'static str {
    if message.contains("panicked") {
        "panic"
    } else if message.contains("cancelled") {
        "cancelled"
    } else {
        "internal"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn database_failures_are_named_by_kind_without_their_message() {
        let cases = [
            (DatabaseErrorKind::Busy, "busy"),
            (DatabaseErrorKind::ConnectionAcquire, "connection_acquire"),
            (DatabaseErrorKind::Query, "query"),
            (DatabaseErrorKind::UniqueViolation, "unique_violation"),
            (DatabaseErrorKind::ProjectionDecode, "projection_decode"),
            (DatabaseErrorKind::Other, "other"),
        ];
        for (kind, name) in cases {
            let error = OxidGeneError::database(kind, "private SQL and values");
            assert_eq!(error_kind(&error), name);
            assert!(!error.to_string().contains("private"), "{error}");
        }
    }

    /// A row the schema refused is the conflict a service would have
    /// reported: an expected answer, without a correlation id. Any other
    /// database failure stays an unexpected one.
    #[test]
    fn a_broken_constraint_is_a_conflict_and_other_failures_are_unexpected() {
        for kind in [
            DatabaseErrorKind::UniqueViolation,
            DatabaseErrorKind::ForeignKeyViolation,
            DatabaseErrorKind::CheckViolation,
        ] {
            let contract = classify(&OxidGeneError::database(kind, "private"));
            assert_eq!(contract.code, "conflict", "{kind}");
            assert!(!contract.unexpected, "{kind}");
        }
        for kind in [
            DatabaseErrorKind::NotNullViolation,
            DatabaseErrorKind::Connection,
            DatabaseErrorKind::Execution,
            DatabaseErrorKind::Other,
        ] {
            let contract = classify(&OxidGeneError::database(kind, "private"));
            assert_eq!(contract.code, "database_error", "{kind}");
            assert!(contract.unexpected, "{kind}");
        }
    }

    #[test]
    fn io_and_task_failures_are_named_by_kind() {
        let io = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "/private/path");
        assert_eq!(error_kind(&OxidGeneError::Io(io)), "io_permission_denied");
        assert_eq!(
            error_kind(&OxidGeneError::Internal(
                "task 12 panicked with message \"private\"".to_string()
            )),
            "panic"
        );
        assert_eq!(
            error_kind(&OxidGeneError::Internal("task 3 was cancelled".to_string())),
            "cancelled"
        );
        assert_eq!(
            error_kind(&OxidGeneError::Internal("private".to_string())),
            "internal"
        );
    }
}
