//! Turning a chosen register into a `View` target, the same way on every
//! platform (Archive Portals §5.2, §7).
//!
//! An adapter that has chosen a register and counted its images asks
//! [`cited_views`] which cited views it can open, builds one [`ArchiveView`]
//! per view with its platform's addresses (and, for a `display: "iiif"`
//! archive, its image through [`super::iiif`]), and hands them to
//! [`view_target`], which fills the call number and the attribution.

use super::select::period_ranges;
use crate::catalog::Archive;
use crate::citation::{CitationParts, CitedView};
use crate::{ArchiveTarget, ArchiveView, Renumbering};

/// The views a register of `image_count` images opens for the cited ones:
/// all of them, or none when one lies beyond its images, in which case the
/// register opens on its first image.
///
/// A register counting more images than the citation, whose period
/// (`register_period`, as the portal shows it) ends with the cited period
/// but starts before it, is a volume the portal merged with earlier years
/// since the citation was numbered: the cited views are moved by the images
/// added before them (a cited view 38 of 184 for 1832-1851 is view 155 of
/// the 301 of 1818-1851). Otherwise they are kept; [`view_target`] says
/// either way that the numbering differs.
pub(crate) fn cited_views(
    citation: &CitationParts,
    image_count: usize,
    register_period: Option<&str>,
) -> Vec<CitedView> {
    let shift = earlier_images(citation, image_count, register_period);
    let views: Vec<CitedView> = citation
        .views
        .iter()
        .map(|view| CitedView {
            view: view.view.saturating_add(shift),
            side: view.side,
        })
        .collect();
    if views
        .iter()
        .all(|view| usize::from(view.view) <= image_count)
    {
        views
    } else {
        Vec::new()
    }
}

/// The images a register holds before the cited part of it: the difference
/// of the counts, when the register counts more and its period ends with
/// the cited one but starts earlier; none otherwise.
fn earlier_images(
    citation: &CitationParts,
    image_count: usize,
    register_period: Option<&str>,
) -> u16 {
    let (Some(cited_count), Ok(count)) = (citation.view_count, u16::try_from(image_count)) else {
        return 0;
    };
    let bounds = |text: &str| {
        let ranges = period_ranges(text);
        Some((
            ranges.iter().map(|range| range.0).min()?,
            ranges.iter().map(|range| range.1).max()?,
        ))
    };
    let (Some(cited), Some(register)) = (
        citation.period.as_deref().and_then(bounds),
        register_period.and_then(bounds),
    ) else {
        return 0;
    };
    if count > cited_count && cited.1 == register.1 && cited.0 > register.0 {
        count - cited_count
    } else {
        0
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
    // A register counting another number of images than the citation: the
    // views opened may not be the cited pages, which the reader is told.
    let renumbering = citation
        .view_count
        .zip(u16::try_from(image_count).ok())
        .filter(|(cited, count)| cited != count)
        .zip(views.first().zip(citation.views.first()))
        .map(|((cited_count, _), (opened, cited))| Renumbering {
            cited_count,
            shifted_by: opened.view.saturating_sub(cited.view),
        });
    ArchiveTarget::View {
        url: views.first().map_or(register_url, |view| view.url.clone()),
        attribution: archive.attribution_for(call_number.as_deref(), &numbers),
        views,
        view_count: u16::try_from(image_count).ok(),
        call_number,
        renumbering,
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
        assert_eq!(cited_views(&cited, 13, None).len(), 2);
        assert_eq!(cited_views(&cited, 6, None).len(), 2);
        assert!(cited_views(&cited, 5, None).is_empty());
    }

    /// A regression: a register merged with earlier years since the
    /// citation was numbered opened on the cited view number, an earlier
    /// year's page.
    #[test]
    fn moves_the_views_of_a_register_merged_with_earlier_years() {
        let numbers = |title: &str, count: usize, period: &str| -> Vec<u16> {
            cited_views(&citation(title), count, Some(period))
                .iter()
                .map(|view| view.view)
                .collect()
        };
        let end = "AD44 - Exampleville - (aucun) - N - 1832-1851 - 4E 999 - vue 38d/184";
        assert_eq!(numbers(end, 301, "1818-1851"), [155]);
        // The cited period the register's start, or the whole of it: kept.
        let start = "AD44 - Exampleville - (aucun) - N - 1818-1831 - 4E 999 - vue 38d/184";
        assert_eq!(numbers(start, 301, "1818-1851"), [38]);
        assert_eq!(numbers(end, 301, "1832-1851"), [38]);
        // A register counting fewer images: kept.
        assert_eq!(numbers(end, 150, "1818-1851"), [38]);
        // No period on either side: kept.
        let bare = "AD44 - Exampleville - (aucun) - N - 4E 999 - vue 38d/184";
        assert_eq!(numbers(bare, 301, "1818-1851"), [38]);
    }

    #[test]
    fn says_when_the_register_counts_other_images_than_the_citation() {
        let archive = ArchiveRegistry::embedded().archive("AD44").unwrap();
        let cited =
            citation("AD44 - Exampleville - (aucun) - N - 1832-1851 - 4E 999 - vue 38d/184");
        let opened = |view: u16| ArchiveView {
            view,
            url: format!("https://archives.example.org/register#{view}"),
            ark: None,
            image: None,
        };
        let renumbering = |count: usize, views: Vec<ArchiveView>| match view_target(
            archive,
            &cited,
            None,
            count,
            "https://archives.example.org/register#0".to_owned(),
            views,
        ) {
            ArchiveTarget::View { renumbering, .. } => renumbering,
            ArchiveTarget::Results { .. } => unreachable!("a view target"),
        };
        assert_eq!(
            renumbering(301, vec![opened(155)]),
            Some(Renumbering {
                cited_count: 184,
                shifted_by: 117
            })
        );
        assert_eq!(
            renumbering(190, vec![opened(38)]),
            Some(Renumbering {
                cited_count: 184,
                shifted_by: 0
            })
        );
        assert_eq!(renumbering(184, vec![opened(38)]), None);
        // The register opened on its first image says nothing of the view.
        assert_eq!(renumbering(301, Vec::new()), None);
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
                renumbering: None,
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
