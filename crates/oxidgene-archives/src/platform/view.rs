//! Turning a chosen register into a `View` target, the same way on every
//! platform (Archive Portals §5.2, §7).
//!
//! An adapter that has chosen a register and counted its images asks
//! [`cited_views`] which cited views it can open, builds one [`ArchiveView`]
//! per view with its platform's addresses (and, for a `display: "iiif"`
//! archive, its image through [`super::iiif`]), and hands them to
//! [`view_target`], which fills the call number and the attribution.

use crate::catalog::Archive;
use crate::citation::{CitationParts, CitedView};
use crate::{ArchiveTarget, ArchiveView};

/// The cited views a register of `image_count` images can open: all of
/// them, or none when one lies beyond its images, in which case the register
/// opens on its first image.
pub(crate) fn cited_views(citation: &CitationParts, image_count: usize) -> &[CitedView] {
    if citation
        .views
        .iter()
        .all(|view| usize::from(view.view) <= image_count)
    {
        &citation.views
    } else {
        &[]
    }
}

/// The `View` target of a chosen register.
///
/// `register_url` opens the register on its first image, the target's
/// address when `views` is empty. The call number is the register's as the
/// portal displays it, or the cited one when the portal shows none.
pub(crate) fn view_target(
    archive: &Archive,
    citation: &CitationParts,
    register_call_number: Option<&str>,
    image_count: usize,
    register_url: String,
    views: Vec<ArchiveView>,
) -> ArchiveTarget {
    let call_number = register_call_number
        .map(str::trim)
        .filter(|call_number| !call_number.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            citation
                .call_number
                .as_ref()
                .map(|call_number| call_number.as_str().to_owned())
        });
    let numbers: Vec<u16> = views.iter().map(|view| view.view).collect();
    ArchiveTarget::View {
        url: views.first().map_or(register_url, |view| view.url.clone()),
        attribution: archive.attribution_for(call_number.as_deref(), &numbers),
        views,
        view_count: u16::try_from(image_count).ok(),
        call_number,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ArchiveRegistry;

    fn citation(title: &str) -> CitationParts {
        ArchiveRegistry::embedded().parse(title).unwrap()
    }

    #[test]
    fn opens_the_register_when_a_view_lies_beyond_it() {
        let cited = citation("AD44 - Exampleville - (aucun) - B - 1660 - vue 5d-6g/13");
        assert_eq!(cited_views(&cited, 13).len(), 2);
        assert_eq!(cited_views(&cited, 6).len(), 2);
        assert!(cited_views(&cited, 5).is_empty());
    }

    #[test]
    fn fills_the_target_of_a_register() {
        let archive = ArchiveRegistry::embedded().archive("AD44").unwrap();
        let cited = citation("AD44 - Exampleville - (aucun) - B - 1660 - 9 E 1 / 2");
        let view = ArchiveView {
            view: 3,
            url: "https://archives.example.org/register#3".to_owned(),
            ark: None,
            image: None,
        };
        assert_eq!(
            view_target(
                archive,
                &cited,
                Some(" "),
                40,
                "https://archives.example.org/register#0".to_owned(),
                vec![view.clone()]
            ),
            ArchiveTarget::View {
                url: view.url.clone(),
                views: vec![view],
                view_count: Some(40),
                call_number: Some("9 E 1 / 2".to_owned()),
                attribution: None,
            }
        );
        let ArchiveTarget::View {
            url, call_number, ..
        } = view_target(
            archive,
            &cited,
            Some("9 E 1/2"),
            40,
            "https://archives.example.org/register#0".to_owned(),
            Vec::new(),
        )
        else {
            unreachable!("a view target");
        };
        assert_eq!(url, "https://archives.example.org/register#0");
        assert_eq!(call_number.as_deref(), Some("9 E 1/2"));
    }
}
