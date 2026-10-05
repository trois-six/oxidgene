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

    /// Database error.
    #[error("Database error: {0}")]
    Database(String),

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

/// Why a cited source has no archive target, each with a stable code.
///
/// The resolution's own failures mirror `oxidgene-archives`' `ResolveError`
/// codes; the interface translates each as `archive_viewer.<code>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveFailure {
    /// The source title, completed by the citation's page, is not a
    /// normalized archive citation.
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
