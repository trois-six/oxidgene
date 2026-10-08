//! How each platform's viewer shows a register, read in the reader's page
//! (Archive Portals §6.1, §9.1).
//!
//! One JSON document, `viewers.json`, keyed by platform id, describes each
//! viewer's controls as CSS selectors: the element showing the view number,
//! the view count, and a reuse licence to accept. The live checks' browser
//! reads it to verify that a target opens on the cited view; the desktop's
//! archive window reads it to drive a viewer without an address per view to
//! the cited view (`go_to`). Selectors may be lists, for the viewers a
//! platform runs on different portals.

use std::collections::HashMap;
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};

/// The viewers of every platform, as the live checks' browser reads them.
pub const VIEWERS_JSON: &str = include_str!("viewers.json");

/// One platform's viewer. Its `comment` in the JSON is for the reader of
/// the file only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Viewer {
    #[serde(default, rename = "comment", skip_serializing)]
    _comment: Option<String>,
    /// The button accepting the reuse licence the viewer shows first, or
    /// the one of the licence page a target stands behind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub licence: Option<String>,
    /// The element showing the one-based view number: an input's value or
    /// an element's text, whose first number is read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<String>,
    /// The element showing the register's view count: its last number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view_count: Option<String>,
    /// A regular expression of the viewer page's source showing that it asks
    /// the reader to accept a reuse licence before an image, which the live
    /// check refuses for a `display: "iiif"` archive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub licence_wall: Option<String>,
    /// Whether the live check aborts the viewer's image requests, the view
    /// number showing without them.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub block_images: bool,
    /// How to bring the viewer to a view, for a viewer without an address
    /// per view.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub go_to: Option<GoTo>,
}

/// How the viewer's page-number control is set to a view: the view number
/// is written in `input` (the `view` element by default), then submitted.
/// The view is read back from `view`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoTo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<String>,
    pub submit: Submit,
    /// The button submitting the number, for [`Submit::Click`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub button: Option<String>,
}

/// How the number written in the control is submitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Submit {
    /// A `change` event, which the control listens to.
    Change,
    /// A `change` event, then the Enter key.
    Enter,
    /// A `blur` event: the control applies the number once left.
    Blur,
    /// A click of the `button` beside the control.
    Click,
}

static VIEWERS: LazyLock<HashMap<String, Viewer>> = LazyLock::new(|| {
    serde_json::from_str(VIEWERS_JSON)
        .unwrap_or_else(|error| panic!("the embedded viewers: {error}"))
});

/// The viewer of the platform `id`.
pub fn viewer(id: &str) -> Option<&'static Viewer> {
    VIEWERS.get(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ArchiveRegistry;

    #[test]
    fn every_catalogued_platform_has_a_viewer() {
        let registry = ArchiveRegistry::embedded();
        for archive in registry.archives() {
            for collection in &archive.collections {
                let viewer = viewer(&collection.platform).unwrap_or_else(|| {
                    panic!("{}: no viewer of `{}`", archive.id, collection.platform)
                });
                assert!(viewer.view.is_some(), "`{}`: no view", collection.platform);
            }
        }
    }

    #[test]
    fn a_viewer_driven_to_a_view_reads_it_back() {
        for (id, viewer) in VIEWERS.iter() {
            let Some(go_to) = &viewer.go_to else {
                continue;
            };
            assert!(viewer.view.is_some(), "`{id}`: no view to read back");
            assert_eq!(
                go_to.button.is_some(),
                go_to.submit == Submit::Click,
                "`{id}`: a button for a click only"
            );
        }
    }

    #[test]
    fn serializes_what_the_page_reads_without_the_comment() {
        let gaia = viewer("gaia").unwrap();
        let json = serde_json::to_value(gaia).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "view": "#pagination input[type=text]",
                "view_count": "#pagination input[type=text]",
                "block_images": true,
                "go_to": { "submit": "blur" },
            })
        );
    }
}
