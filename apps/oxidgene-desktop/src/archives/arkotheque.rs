//! Driver for archive portals built on Arkothèque.
//!
//! The portal is a client-rendered application with no public search API, so
//! the driver works through the page itself: an initialization script fills
//! the portal's own filters, then the register viewer's page field. Each
//! archive's field identifiers and labels live in its catalogue document.

use std::collections::BTreeMap;

use oxidgene_ui::archive_viewer::{ActKind, ArchiveSource, ArchiveViewerRequest};
use serde::{Deserialize, Serialize};

pub const PLATFORM: &str = "arkotheque";

const SCRIPT: &str = include_str!("arkotheque.js");
const PLACEHOLDER: &str = "__OXIDGENE_ARCHIVE_REQUEST__";

/// The `portal` object of an Arkothèque archive document.
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
    #[serde(skip_serializing)]
    acts: BTreeMap<ActKind, ActSearch>,
}

/// How one act kind is searched and recognized in the results.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all(serialize = "camelCase"))]
struct ActSearch {
    category: String,
    row_label: String,
}

impl Portal {
    pub fn of(source: &ArchiveSource) -> Result<Self, String> {
        let portal: Self = serde_json::from_value(source.portal.clone())
            .map_err(|error| format!("{}: {error}", source.id))?;
        if !portal.origin.starts_with("https://") || !portal.search_path.starts_with('/') {
            return Err(format!(
                "{}: the portal must be an https origin and path",
                source.id
            ));
        }
        if let Some(act) = source
            .acts
            .iter()
            .find(|act| !portal.acts.contains_key(act))
        {
            return Err(format!("{}: no search settings for {act:?}", source.id));
        }
        Ok(portal)
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
        .acts
        .get(&citation.act)
        .ok_or_else(|| format!("no search settings for {:?}", citation.act))?;
    let payload = serde_json::json!({
        "portal": portal,
        "act": act,
        "citation": {
            "locality": citation.locality,
            "year": citation.year,
            "view": citation.view,
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
    use oxidgene_ui::archive_viewer::{ArchiveLink, ArchiveViewerMessages, catalog};

    fn request() -> ArchiveViewerRequest {
        ArchiveViewerRequest {
            link: ArchiveLink::from_source_title(
                "AD44 - Exampleville - (aucun) - B - 1791 - 3E1/2 - acte 4 - vue 3g/12",
            )
            .expect("a catalogued baptism"),
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

    #[test]
    fn every_catalogued_arkotheque_archive_is_complete() {
        for source in catalog()
            .iter()
            .filter(|source| source.platform == PLATFORM)
        {
            Portal::of(source).unwrap();
        }
    }

    #[test]
    fn the_script_carries_the_request_and_nothing_personal() {
        let request = request();
        let portal = Portal::of(&request.link.source).unwrap();
        let script = script(&portal, &request).unwrap();

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
    fn a_portal_without_settings_for_a_listed_act_is_rejected() {
        let mut source = (*request().link.source).clone();
        source.acts.push(ActKind::Marriage);
        assert!(Portal::of(&source).is_err());
    }
}
