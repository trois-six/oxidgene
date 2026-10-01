use oxidgene_core::OxidGeneError;

pub(crate) struct ErrorContract {
    pub code: &'static str,
    pub message: &'static str,
    pub unexpected: bool,
}

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
        OxidGeneError::Conflict(_) => ErrorContract {
            code: "conflict",
            message: "The request conflicts with the current state",
            unexpected: false,
        },
        OxidGeneError::Database(_) => ErrorContract {
            code: "database_error",
            message: "The request could not be completed",
            unexpected: true,
        },
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
        OxidGeneError::Gedcom(_) => "gedcom",
        OxidGeneError::Database(message) => database_kind(message),
        OxidGeneError::Io(error) => io_kind(error.kind()),
        OxidGeneError::Internal(message) => internal_kind(message),
    }
}

/// Database failures arrive as SeaORM's rendered `DbErr`, whose variant
/// opens the message; a few driver conditions are worth naming on their own.
fn database_kind(message: &str) -> &'static str {
    const CONDITIONS: [(&str, &str); 6] = [
        ("database is locked", "busy"),
        ("database table is locked", "busy"),
        ("UNIQUE constraint failed", "unique_violation"),
        ("duplicate key value", "unique_violation"),
        ("FOREIGN KEY constraint failed", "foreign_key_violation"),
        ("violates foreign key constraint", "foreign_key_violation"),
    ];
    const VARIANTS: [(&str, &str); 13] = [
        ("Failed to acquire connection", "connection_acquire"),
        ("Connection pool timed out", "connection_acquire"),
        ("Connection Error", "connection"),
        ("Connection closed", "connection"),
        ("Execution Error", "execution"),
        ("Query Error", "query"),
        ("RecordNotFound", "record_not_found"),
        ("None of the records", "record_not_written"),
        ("Json Error", "conversion"),
        ("Type Error", "conversion"),
        ("Error converting", "conversion"),
        ("Migration Error", "migration"),
        ("person_denorm decode", "projection_decode"),
    ];
    CONDITIONS
        .iter()
        .find(|(needle, _)| message.contains(needle))
        .or_else(|| {
            VARIANTS
                .iter()
                .find(|(prefix, _)| message.starts_with(prefix))
        })
        .map_or("other", |(_, kind)| kind)
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
            (
                "Execution Error: error returned from database: (code: 5) database is locked",
                "busy",
            ),
            (
                "Failed to acquire connection from pool: Connection pool timed out",
                "connection_acquire",
            ),
            ("Query Error: no such column: private_value", "query"),
            (
                "Execution Error: UNIQUE constraint failed: person.id",
                "unique_violation",
            ),
            (
                "person_denorm decode for person 00000000-0000-0000-0000-000000000000: EOF",
                "projection_decode",
            ),
            ("something private", "other"),
        ];
        for (message, kind) in cases {
            assert_eq!(
                error_kind(&OxidGeneError::Database(message.to_string())),
                kind,
                "{message}"
            );
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
