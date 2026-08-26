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
