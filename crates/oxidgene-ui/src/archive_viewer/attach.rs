//! Attaching cited views of an archive whose images OxidGene may use
//! (`display: "iiif"`) as a document of the event (docs/archives.md §6.4).
//!
//! The views come from a resolved target: the desktop's archive window sends
//! the one it shows when the reader asks, the web asks the backend on the
//! reader's click beside the source. Either way the canonical
//! `DocumentForm` opens prefilled, and nothing is written before Save.

use dioxus::prelude::*;
use oxidgene_core::enums::SourceMediaType;
use uuid::Uuid;

use super::{ArchiveRegister, ViewPage};
use crate::components::document_form::{DocumentDraft, DocumentForm, RemotePage};
use crate::components::media_gallery::MediaOwner;
use crate::i18n::{I18n, use_i18n};

/// The draft of the document attaching the cited views among `pages` to
/// event `event_id`, linked to the cited source.
pub(super) fn draft(
    i18n: &I18n,
    register: &ArchiveRegister,
    pages: &[ViewPage],
    event_id: Uuid,
) -> DocumentDraft {
    let cited: Vec<&ViewPage> = pages.iter().filter(|page| page.cited).collect();
    let views: Vec<u16> = cited.iter().map(|page| page.view).collect();
    DocumentDraft {
        title: register.document_title(i18n, &views),
        description: register.attribution(&views).unwrap_or_default(),
        category: register.category(),
        medium: SourceMediaType::Manuscript,
        pages: cited.into_iter().map(RemotePage::of_view).collect(),
        event_ids: vec![event_id],
        source_id: Some(register.link.source_id),
        register: Some(register.clone()),
    }
}

/// The canonical document form, prefilled with the cited views.
#[component]
pub(super) fn AttachForm(
    register: ArchiveRegister,
    pages: Vec<ViewPage>,
    event: (Uuid, String),
    on_created: EventHandler<Uuid>,
    on_close: EventHandler<()>,
) -> Element {
    let i18n = use_i18n();
    let tree_id = register.tree_id;
    let draft = draft(&i18n, &register, &pages, event.0);
    rsx! {
        DocumentForm {
            tree_id,
            owner: MediaOwner::Event(event.0),
            events: vec![event.clone()],
            draft: Some(draft),
            on_created: move |document_id: Uuid| on_created.call(document_id),
            on_close,
        }
    }
}

#[cfg(test)]
mod tests {
    use oxidgene_archives::ArchiveImage;
    use oxidgene_core::enums::DocumentCategory;

    use super::*;
    use crate::i18n::Language;

    fn page(view: u16, cited: bool) -> ViewPage {
        ViewPage {
            view,
            cited,
            image: ArchiveImage {
                picture: format!("https://archives.example.org/iiif/{view}/full/max/0/default.jpg"),
                thumbnail: format!("https://archives.example.org/images/{view}_thumbnail.jpg"),
                width: 1200,
                height: 800,
            },
        }
    }

    /// The document attaching cited views — offered once `ATTACH_OFFERED` is
    /// on — is prefilled with the cited views only, named after the register
    /// and its views, of the record's kind, linked to the cited source and
    /// checked as documenting the event.
    #[test]
    fn the_draft_holds_the_cited_views() {
        let i18n = I18n::new(Language::english());
        let link = super::super::tests::link(
            "AD37 - Exampleville - (aucun) - N - 1877 - 3E1/2 - vue 5-6/13",
            None,
        )
        .expect("a catalogued citation");
        let register = ArchiveRegister {
            tree_id: Uuid::nil(),
            link: link.clone(),
            view_count: Some(13),
            call_number: Some("3E1/2".to_owned()),
        };
        let event_id = Uuid::now_v7();
        let draft = draft(
            &i18n,
            &register,
            &[page(5, true), page(6, true), page(7, false)],
            event_id,
        );
        assert_eq!(draft.title, "3E1/2, views 5-6");
        assert_eq!(draft.category, Some(DocumentCategory::CivilRecord));
        assert_eq!(draft.medium, SourceMediaType::Manuscript);
        assert_eq!(
            draft
                .pages
                .iter()
                .map(|page| (page.view, page.file_name.as_str()))
                .collect::<Vec<_>>(),
            [(Some(5), "5.jpg"), (Some(6), "6.jpg")]
        );
        assert_eq!(draft.event_ids, [event_id]);
        assert_eq!(draft.source_id, Some(link.source_id));
        assert_eq!(draft.register, Some(register));
    }
}
