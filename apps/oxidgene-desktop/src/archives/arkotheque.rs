//! Driver for archive portals built on Arkothèque.
//!
//! The portal is a client-rendered application whose search the driver works
//! through the page itself: an initialization script fills the portal's own
//! filters, then the register viewer's page field. Each collection's field
//! identifiers and labels live in its catalogue `portal` settings.

use std::collections::BTreeMap;

use oxidgene_archives::{ActKind, CitationParts, Collection};
use oxidgene_ui::archive_viewer::ArchiveViewerRequest;
use serde::{Deserialize, Serialize};

pub const PLATFORM: &str = "arkotheque";

const SCRIPT: &str = include_str!("arkotheque.js");
const PLACEHOLDER: &str = "__OXIDGENE_ARCHIVE_REQUEST__";

/// The `portal` settings of an Arkothèque collection.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all(serialize = "camelCase"))]
pub struct Portal {
    origin: String,
    search_path: String,
    root: String,
    locality_field: String,
    act_field: String,
    period_field: String,
    content_ids: Vec<String>,
    display_mode: String,
    locality_input: String,
    locality_list_label: String,
    open_images_label: String,
    /// Search settings by act kind name: `birth`, `baptism`…
    #[serde(skip_serializing)]
    acts: BTreeMap<String, ActSearch>,
}

/// How one act kind is searched and recognized in the results.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all(serialize = "camelCase"))]
struct ActSearch {
    category: String,
    row_label: String,
}

/// The key of an act kind in the driver's `acts` settings.
const fn act_key(kind: ActKind) -> &'static str {
    match kind {
        ActKind::Birth => "birth",
        ActKind::Baptism => "baptism",
        ActKind::Marriage => "marriage",
        ActKind::Death => "death",
        ActKind::Burial => "burial",
    }
}

impl Portal {
    pub fn of(collection: &Collection) -> Result<Self, String> {
        let portal: Self = serde_json::from_value(collection.portal.clone())
            .map_err(|error| format!("{}: {error}", collection.id))?;
        if !portal.origin.starts_with("https://") || !portal.search_path.starts_with('/') {
            return Err(format!(
                "{}: the portal must be an https origin and path",
                collection.id
            ));
        }
        Ok(portal)
    }

    /// The search settings of a citation's act: a single act kind the
    /// settings name.
    fn act(&self, citation: &CitationParts) -> Option<&ActSearch> {
        match citation.act.kinds() {
            [kind] => self.acts.get(act_key(*kind)),
            _ => None,
        }
    }

    /// Whether the driver can search for this citation: it filters by a
    /// single act category and by year.
    pub fn drives(&self, citation: &CitationParts) -> bool {
        citation.year.is_some() && self.act(citation).is_some()
    }

    /// The page the window opens on.
    pub fn start_url(&self) -> String {
        format!("{}{}", self.origin, self.search_path)
    }
}

/// The initialization script for one request.
pub fn script(portal: &Portal, request: &ArchiveViewerRequest) -> Result<String, String> {
    let citation = &request.link.citation;
    let act = portal
        .act(citation)
        .ok_or_else(|| format!("no search settings for {}", citation.act))?;
    let year = citation.year.ok_or("the driver searches by year")?;
    let view = citation
        .views
        .first()
        .zip(citation.view_count)
        .map(|(view, count)| serde_json::json!({ "index": view.view, "count": count }));
    let payload = serde_json::json!({
        "portal": portal,
        "act": act,
        "citation": {
            "locality": citation.locality,
            "year": year,
            "view": view,
        },
        "messages": request.messages,
    });
    // JSON is a JavaScript expression; `</script>` cannot occur since the
    // script is injected rather than parsed from HTML.
    let payload = serde_json::to_string(&payload).map_err(|error| error.to_string())?;
    Ok(SCRIPT.replace(PLACEHOLDER, &payload))
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxidgene_archives::ArchiveRegistry;
    use oxidgene_ui::archive_viewer::{ArchiveLink, ArchiveViewerMessages};

    fn request(title: &str) -> ArchiveViewerRequest {
        ArchiveViewerRequest {
            link: ArchiveLink::from_source_title(title).expect("a catalogued act"),
            messages: ArchiveViewerMessages {
                searching: "searching".to_owned(),
                not_found: "not found".to_owned(),
                ambiguous: "ambiguous".to_owned(),
                view_not_selected: "view".to_owned(),
                failed: "failed \"quoted\"".to_owned(),
                close: "close".to_owned(),
            },
        }
    }

    fn portal() -> Portal {
        let archive = ArchiveRegistry::embedded()
            .archive("AD44")
            .expect("the Loire-Atlantique archive");
        Portal::of(&archive.collections[0]).unwrap()
    }

    #[test]
    fn every_catalogued_arkotheque_collection_is_complete() {
        for archive in ArchiveRegistry::embedded().archives() {
            for collection in &archive.collections {
                if collection.platform == PLATFORM {
                    Portal::of(collection).unwrap();
                }
            }
        }
    }

    #[test]
    fn the_script_carries_the_request_and_nothing_personal() {
        let request =
            request("AD44 - Exampleville - (aucun) - B - 1791 - 3E1/2 - acte 4 - vue 3g/12");
        let script = script(&portal(), &request).unwrap();

        assert!(!script.contains(PLACEHOLDER));
        assert!(script.contains(r#""locality":"Exampleville""#));
        assert!(script.contains(r#""rowLabel":"Baptêmes""#));
        assert!(script.contains(r#""view":{"count":12,"index":3}"#));
        assert!(script.contains(r#"failed \"quoted\""#));
        // The act number and the rest of the title stay in the application.
        assert!(!script.contains("acte 4"));
        assert!(!script.contains("3E1/2"));
    }

    #[test]
    fn an_act_without_settings_is_not_driven() {
        let portal = portal();
        let request = request("AD44 - Exampleville - (aucun) - M - 1850");
        assert!(!portal.drives(&request.link.citation));
        assert!(script(&portal, &request).is_err());
        let combined = request_citation("AD44 - Exampleville - (aucun) - NB - 1850");
        assert!(!portal.drives(&combined));
    }

    fn request_citation(title: &str) -> CitationParts {
        ArchiveRegistry::embedded().parse(title).unwrap()
    }
}
