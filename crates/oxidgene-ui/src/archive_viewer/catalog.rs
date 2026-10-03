//! The archives whose citations can be opened at the cited view.
//!
//! Each archive is one JSON document in `assets/archives/`, discovered at
//! build time: adding an archive served by an already supported portal
//! platform is a data change. The `portal` object belongs to the platform's
//! driver, which the desktop application implements; this crate only reads
//! what decides whether a citation is offered.

use std::sync::{Arc, LazyLock};

use serde::Deserialize;

use super::citation::ActKind;

/// One archive service.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchiveSource {
    /// Stable identifier: the lowercase country code, then the archive.
    pub id: String,
    /// ISO 3166-1 alpha-2 country code.
    pub country: String,
    /// The archive's own name, shown verbatim.
    pub name: String,
    /// The code normalized citations of this archive start with.
    pub citation_code: String,
    /// The portal software, which selects the desktop driver.
    pub platform: String,
    /// The act kinds the driver can search for.
    pub acts: Vec<ActKind>,
    /// Driver settings, opaque to the interface.
    pub portal: serde_json::Value,
}

static CATALOG: LazyLock<Vec<Arc<ArchiveSource>>> = LazyLock::new(|| {
    let documents: &[&str] = include!(concat!(env!("OUT_DIR"), "/archives.rs"));
    let sources: Vec<_> = documents
        .iter()
        .map(|document| {
            let source: ArchiveSource =
                serde_json::from_str(document).expect("valid embedded archive document");
            validate(&source).unwrap_or_else(|error| panic!("archive `{}`: {error}", source.id));
            Arc::new(source)
        })
        .collect();
    for (index, source) in sources.iter().enumerate() {
        assert!(
            !sources[..index]
                .iter()
                .any(|other| other.id == source.id || other.citation_code == source.citation_code),
            "archive `{}` repeats an id or a citation code",
            source.id
        );
    }
    sources
});

fn validate(source: &ArchiveSource) -> Result<(), &'static str> {
    let slug = |text: &str| {
        !text.is_empty()
            && text
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    };
    if !slug(&source.id) || !slug(&source.platform) {
        return Err("id and platform must be lowercase slugs");
    }
    if source.country.len() != 2 || !source.country.bytes().all(|b| b.is_ascii_uppercase()) {
        return Err("country must be an ISO 3166-1 alpha-2 code");
    }
    if !source
        .id
        .starts_with(&format!("{}-", source.country.to_ascii_lowercase()))
    {
        return Err("id must start with the lowercase country code");
    }
    if source.name.trim().is_empty() || source.acts.is_empty() {
        return Err("name and acts must not be empty");
    }
    if source.citation_code.is_empty()
        || !source
            .citation_code
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
    {
        return Err("citation code must be uppercase letters and digits");
    }
    Ok(())
}

/// Every embedded archive, sorted by file name.
pub fn catalog() -> &'static [Arc<ArchiveSource>] {
    &CATALOG
}

/// The archive normalized citations starting with `code` belong to.
pub fn source_for(code: &str) -> Option<&'static Arc<ArchiveSource>> {
    CATALOG.iter().find(|source| source.citation_code == code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_embedded_catalogue_is_valid_and_indexed_by_citation_code() {
        assert!(!catalog().is_empty());
        let source = source_for("AD44").expect("the Loire-Atlantique archive");
        assert_eq!(source.id, "fr-ad44");
        assert_eq!(source.country, "FR");
        assert_eq!(source.platform, "arkotheque");
        assert!(source.acts.contains(&ActKind::Birth));
        assert_eq!(source_for("AD00"), None);
    }

    #[test]
    fn rejects_inconsistent_documents() {
        let mut source = (**source_for("AD44").unwrap()).clone();
        assert_eq!(validate(&source), Ok(()));
        source.id = "ch-ad44".to_owned();
        assert!(validate(&source).is_err());
        source.id = "fr-ad44".to_owned();
        source.citation_code = "ad44".to_owned();
        assert!(validate(&source).is_err());
        source.citation_code = "AD44".to_owned();
        source.acts.clear();
        assert!(validate(&source).is_err());
    }
}
