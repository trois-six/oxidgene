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
    fn test_validation_error_display() {
        let err = OxidGeneError::Validation("name is required".to_string());
        assert_eq!(err.to_string(), "Validation error: name is required");
    }
}
