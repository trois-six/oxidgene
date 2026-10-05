//! The register a cited view belongs to, as the document form attaching
//! cited views pages through it (docs/archives.md §6.3, §6.4).
//!
//! Every view the reader turns to is resolved by the backend on that click
//! (`view` of the archive-target request), never ahead of it: the portal is
//! asked once per click, and only for the view asked for.

use oxidgene_archives::{ArchiveImage, ArchiveTarget};
use oxidgene_core::enums::DocumentCategory;
use uuid::Uuid;

use super::ArchiveLink;
use crate::api::{ApiClient, ApiError};
use crate::i18n::I18n;

/// One view of a register whose image OxidGene may attach.
#[derive(Clone, Debug, PartialEq)]
pub struct ViewPage {
    /// One-based view number.
    pub view: u16,
    /// Whether the citation cites this view.
    pub cited: bool,
    pub image: ArchiveImage,
}

/// A cited register whose views OxidGene may attach: where its views are
/// asked for, and what its documents are called.
#[derive(Clone, Debug, PartialEq)]
pub struct ArchiveRegister {
    pub tree_id: Uuid,
    pub link: ArchiveLink,
    /// The register's image count, when the portal states it.
    pub view_count: Option<u16>,
    /// The register's call number, as the portal or the citation writes it.
    pub call_number: Option<String>,
}

impl ArchiveRegister {
    /// The register and the cited views of a resolved target, when the target
    /// is a view whose every view carries an image OxidGene may attach;
    /// `None` for any other target.
    pub fn of(
        tree_id: Uuid,
        link: &ArchiveLink,
        target: &ArchiveTarget,
    ) -> Option<(Self, Vec<ViewPage>)> {
        let ArchiveTarget::View {
            views,
            view_count,
            call_number,
            ..
        } = target
        else {
            return None;
        };
        let pages: Vec<ViewPage> = views
            .iter()
            .map(|view| {
                Some(ViewPage {
                    view: view.view,
                    cited: link
                        .citation
                        .views
                        .iter()
                        .any(|cited| cited.view == view.view),
                    image: view.image.clone()?,
                })
            })
            .collect::<Option<_>>()?;
        if pages.is_empty() {
            return None;
        }
        let register = Self {
            tree_id,
            link: link.clone(),
            view_count: *view_count,
            call_number: call_number.clone(),
        };
        Some((register, pages))
    }

    /// Whether the views of `target` can be attached as a document: views
    /// of an archive whose images OxidGene may use, each with its image.
    pub fn attachable(link: &ArchiveLink, target: &ArchiveTarget) -> bool {
        link.archive.display == oxidgene_archives::Display::Iiif
            && Self::of(Uuid::nil(), link, target).is_some()
    }

    /// Whether view `view` exists in the register, as far as its count says.
    pub fn has_view(&self, view: u16) -> bool {
        view >= 1 && self.view_count.is_none_or(|count| view <= count)
    }

    /// Resolves view `view` of the register: one request to the backend,
    /// which may query the portal. `None` when the register has no such view
    /// or the archive serves no image of it.
    pub async fn view(&self, api: &ApiClient, view: u16) -> Result<Option<ViewPage>, ApiError> {
        let target = api
            .archive_target(
                self.tree_id,
                self.link.source_id,
                self.link.citation_id,
                Some(view),
                self.link.supplied.as_ref(),
            )
            .await?;
        Ok(Self::of(self.tree_id, &self.link, &target)
            .and_then(|(_, pages)| pages.into_iter().find(|page| page.view == view)))
    }

    /// The archive's credit for `views`, as its reuse terms require.
    pub fn attribution(&self, views: &[u16]) -> Option<String> {
        self.link
            .archive
            .attribution_for(self.call_number.as_deref(), views)
    }

    /// A document's title for `views`: the call number, or the archive's
    /// name without one, and the views.
    pub fn document_title(&self, i18n: &I18n, views: &[u16]) -> String {
        let register = self
            .call_number
            .clone()
            .unwrap_or_else(|| self.link.archive.name.clone());
        let key = if views.len() > 1 {
            "archive_viewer.document_title_views"
        } else {
            "archive_viewer.document_title"
        };
        i18n.t_args(
            key,
            &[("register", &register), ("views", &views_label(views))],
        )
    }

    /// The kind of record the cited document is: an act, a table or a
    /// series (docs/archives.md §6.4).
    pub fn category(&self) -> Option<DocumentCategory> {
        self.link.citation.category()
    }
}

/// Views as a reader writes them: `5`, a run `5-6`, or a list `5, 7`.
pub(crate) fn views_label(views: &[u16]) -> String {
    let mut sorted = views.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    match sorted.as_slice() {
        [] => String::new(),
        [only] => only.to_string(),
        [first, .., last] if usize::from(last - first) + 1 == sorted.len() => {
            format!("{first}-{last}")
        }
        list => list
            .iter()
            .map(u16::to_string)
            .collect::<Vec<_>>()
            .join(", "),
    }
}

#[cfg(test)]
mod tests {
    use oxidgene_archives::ArchiveView;

    use super::*;

    fn link(title: &str) -> ArchiveLink {
        super::super::tests::link(title, None).expect("a catalogued citation")
    }

    fn image(view: u16) -> ArchiveImage {
        ArchiveImage {
            picture: format!("https://archives.example.org/iiif/{view}/full/max/0/default.jpg"),
            thumbnail: format!("https://archives.example.org/images/{view}_thumbnail.jpg"),
            width: 3000,
            height: 2000,
        }
    }

    fn target(views: &[(u16, bool)]) -> ArchiveTarget {
        ArchiveTarget::View {
            url: "https://archives.example.org/ark:/00000/a1".to_owned(),
            views: views
                .iter()
                .map(|&(view, with_image)| ArchiveView {
                    view,
                    url: format!("https://archives.example.org/ark:/00000/a1/{view}"),
                    ark: None,
                    image: with_image.then(|| image(view)),
                })
                .collect(),
            view_count: Some(13),
            call_number: Some("3E1/2".to_owned()),
            attribution: None,
        }
    }

    #[test]
    fn the_views_of_a_target_with_images_are_the_registers() {
        let link = link("AD37 - Exampleville - (aucun) - N - 1877 - 3E1/2 - vue 5d-6g/13");
        let (register, pages) = ArchiveRegister::of(
            Uuid::nil(),
            &link,
            &target(&[(5, true), (6, true), (7, true)]),
        )
        .unwrap();
        assert_eq!(register.view_count, Some(13));
        assert_eq!(register.call_number.as_deref(), Some("3E1/2"));
        assert_eq!(
            pages
                .iter()
                .map(|page| (page.view, page.cited))
                .collect::<Vec<_>>(),
            [(5, true), (6, true), (7, false)]
        );
        assert!(register.has_view(13) && !register.has_view(14) && !register.has_view(0));
    }

    #[test]
    fn anything_but_views_with_images_cannot_be_attached() {
        let link = link("AD37 - Exampleville - (aucun) - N - 1877 - 3E1/2 - vue 5/13");
        assert!(ArchiveRegister::attachable(&link, &target(&[(5, true)])));
        // An archive whose images OxidGene may not use.
        let portal =
            super::super::tests::link("AD44 - Exampleville - (aucun) - N - 1877 - vue 5/13", None)
                .expect("a catalogued citation");
        assert!(!ArchiveRegister::attachable(&portal, &target(&[(5, true)])));
        // A view without an image, no view at all, and the search results.
        assert!(ArchiveRegister::of(Uuid::nil(), &link, &target(&[(5, false)])).is_none());
        assert!(ArchiveRegister::of(Uuid::nil(), &link, &target(&[])).is_none());
        let results = ArchiveTarget::Results {
            url: "https://archives.example.org/search".to_owned(),
            matches: Some(2),
        };
        assert!(ArchiveRegister::of(Uuid::nil(), &link, &results).is_none());
    }

    #[test]
    fn the_kind_of_record_follows_the_act() {
        let category = |title: &str| {
            ArchiveRegister::of(Uuid::nil(), &link(title), &target(&[(5, true)]))
                .unwrap()
                .0
                .category()
        };
        assert_eq!(
            category("AD37 - Exampleville - (aucun) - B - 1702 - vue 5/13"),
            Some(DocumentCategory::ParishRecord)
        );
        assert_eq!(
            category("AD37 - Exampleville - (aucun) - D - 1877 - vue 5/13"),
            Some(DocumentCategory::CivilRecord)
        );
        assert_eq!(
            category("AD37 - Exampleville - (aucun) - M - 1702 - vue 5/13"),
            Some(DocumentCategory::ParishRecord)
        );
        assert_eq!(
            category("AD37 - Exampleville - (aucun) - M - 1877 - vue 5/13"),
            Some(DocumentCategory::CivilRecord)
        );
        assert_eq!(
            category("AD37 - Exampleville - (aucun) - TD - 1803 - vue 5/13"),
            Some(DocumentCategory::CivilRecord)
        );

        // The series, cited in words, are of their own kinds.
        let series = |title: &str| {
            let mut link = link("AD37 - Exampleville - (aucun) - N - 1877 - vue 5/13");
            link.citation = oxidgene_archives::ArchiveRegistry::embedded()
                .parse(title)
                .expect("a series citation");
            ArchiveRegister::of(Uuid::nil(), &link, &target(&[(5, true)]))
                .unwrap()
                .0
                .category()
        };
        for (title, expected) in [
            (
                "AD37 - Exampleville - Recensement - 1872 - vue 5/13",
                DocumentCategory::Census,
            ),
            (
                "AD37 - Exampleville - Registres matricules - 1898 - 348 - 5/13",
                DocumentCategory::MilitaryArchive,
            ),
            (
                "AD37 - Exampleville - Conscrits militaires - 1897 - vue 5/13",
                DocumentCategory::MilitaryArchive,
            ),
            (
                "AD37 - Exampleville - Tables des successions et absences - 1897-1898 - vue 5/13",
                DocumentCategory::NotarialArchive,
            ),
        ] {
            assert_eq!(series(title), Some(expected), "{title}");
        }
    }

    #[test]
    fn views_are_written_as_a_reader_writes_them() {
        assert_eq!(views_label(&[5]), "5");
        assert_eq!(views_label(&[6, 5]), "5-6");
        assert_eq!(views_label(&[5, 7]), "5, 7");
        assert_eq!(views_label(&[]), "");
    }
}
