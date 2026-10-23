//! The one pager: the step and end buttons around the page numbers, the
//! same for every paged list and document; with it, what every paged list
//! shares: its page-size choice and its foot ([`PageSizeSelect`],
//! [`Pagination`]).

use dioxus::prelude::*;

use crate::components::print::PrintPageNote;
use crate::i18n::use_i18n;

/// How many entries a page of a list holds, or all of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageSize {
    Fixed(usize),
    All,
}

impl PageSize {
    pub(crate) fn as_option(self) -> Option<usize> {
        match self {
            PageSize::Fixed(n) => Some(n),
            PageSize::All => None,
        }
    }
}

/// The page sizes a list offers.
const PAGE_SIZES: [usize; 3] = [25, 50, 100];

/// The page size a list opens on.
pub(crate) const DEFAULT_PER_PAGE: usize = PAGE_SIZES[0];

/// The choice of a list's page size: 25, 50 or 100 entries, and *All* when
/// `with_all` — which a list the server pages, at most 100 at a time, leaves
/// out.
#[component]
pub fn PageSizeSelect(
    value: PageSize,
    on_change: EventHandler<PageSize>,
    #[props(default = true)] with_all: bool,
) -> Element {
    let i18n = use_i18n();
    let current = match value {
        PageSize::Fixed(n) => n.to_string(),
        PageSize::All => "all".to_string(),
    };
    rsx! {
        div { class: "dict-page-size",
            select {
                aria_label: i18n.t("pager.per_page"),
                value: "{current}",
                onchange: move |e: Event<FormData>| {
                    on_change.call(match e.value().as_str() {
                        "all" => PageSize::All,
                        other => PageSize::Fixed(other.parse().unwrap_or(DEFAULT_PER_PAGE)),
                    });
                },
                for size in PAGE_SIZES {
                    option { value: "{size}", "{size}" }
                }
                if with_all {
                    option { value: "all", {i18n.t("pager.page_size_all")} }
                }
            }
        }
    }
}

/// The foot of a paged list: its [`Pager`], and on paper which page was
/// printed. `page` is the page shown, from 1, of `pages`; choosing one sets
/// `current_page`.
#[component]
pub fn Pagination(mut current_page: Signal<usize>, page: usize, pages: usize) -> Element {
    rsx! {
        Pager {
            current: page.saturating_sub(1),
            total: pages,
            on_select: move |index: usize| current_page.set(index + 1),
        }
        PrintPageNote { page, pages }
    }
}

/// Moves between the `total` pages of a list or a document, `current` being
/// the one shown (both zero-based); `on_select` receives the page asked for.
///
/// Every paged list and document draws this one form, so they all page
/// alike: buttons to the first and the previous page, the page numbers, and
/// buttons to the next and the last page. The numbers strip keeps both ends
/// and a window around the current page ([`page_window`]), narrower as the
/// pager is ([`page_strips`]); a pager too narrow for the end buttons leaves
/// them to its first and last numbers.
///
/// Draws nothing for a single page.
#[component]
pub fn Pager(
    current: usize,
    total: usize,
    on_select: EventHandler<usize>,
    #[props(default)] disabled: bool,
    /// Classes added to the pager, to place it.
    #[props(default)]
    class: String,
) -> Element {
    let i18n = use_i18n();
    if total <= 1 {
        return rsx! {};
    }
    let last = total - 1;
    let current = current.min(last);
    let (at_start, at_end) = (disabled || current == 0, disabled || current == last);
    rsx! {
        nav { class: "pager {class}", aria_label: i18n.t("pager.label"),
            {step_button(i18n.t("pager.first"), "\u{23EE}", "pager-btn pager-end", at_start, move || on_select.call(0))}
            {step_button(i18n.t("pager.previous"), "\u{25C0}", "pager-btn", at_start, move || on_select.call(current.saturating_sub(1)))}
            for (slots , widths) in page_strips(current, total) {
                div { key: "{widths}", class: "pager-numbers {widths}",
                    for (slot_index , slot) in slots.into_iter().enumerate() {
                        match slot {
                            PagerSlot::Page(index) => rsx! {
                                button {
                                    key: "p{index}",
                                    class: if index == current { "pager-num is-current" } else { "pager-num" },
                                    r#type: "button",
                                    disabled,
                                    aria_label: i18n.t_args("pager.page", &[("page", &(index + 1).to_string())]),
                                    "aria-current": if index == current { "page" } else { "false" },
                                    onclick: move |_| on_select.call(index),
                                    "{index + 1}"
                                }
                            },
                            PagerSlot::Gap => rsx! {
                                span { key: "g{slot_index}", class: "pager-gap", "aria-hidden": "true", "\u{2026}" }
                            },
                        }
                    }
                }
            }
            {step_button(i18n.t("pager.next"), "\u{25B6}", "pager-btn", at_end, move || on_select.call((current + 1).min(last)))}
            {step_button(i18n.t("pager.last"), "\u{23ED}", "pager-btn pager-end", at_end, move || on_select.call(last))}
        }
    }
}

/// A step button: its glyph shown, its `label` read out and shown on hover.
fn step_button(
    label: String,
    glyph: &'static str,
    class: &'static str,
    disabled: bool,
    mut on_press: impl FnMut() + 'static,
) -> Element {
    rsx! {
        button {
            class,
            r#type: "button",
            disabled,
            title: "{label}",
            aria_label: "{label}",
            onclick: move |_| on_press(),
            span { "aria-hidden": "true", "{glyph}" }
        }
    }
}

/// One slot in the pager strip: a page to jump to, or a gap where pages were
/// left out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PagerSlot {
    Page(usize),
    Gap,
}

/// The widths a numbered pager adapts to, widest first: the most slots its
/// strip holds there, and the class of that strip. The stylesheet's container
/// queries on the pager's own width show the strip of the width it has
/// (`.pager` in `layout.rs`, whose breakpoints are the widths of
/// these slot counts).
const WIDTHS: [(usize, &str); 3] = [(9, "fits-wide"), (7, "fits-medium"), (5, "fits-narrow")];

/// The windows tried for a width, widest first: pages either side of the
/// current one. The narrowest never holds more than five slots — both ends,
/// the current page and a gap either side — so it fits every width.
const RADII: [usize; 3] = [2, 1, 0];

/// The strips of page numbers a pager draws, each with the classes of the
/// widths it is shown at: at each width, the widest window holding no more
/// slots than the width does.
///
/// The width a pager has is only known to the layout, which measures it
/// without a round trip, so every strip is drawn and the stylesheet hides
/// all but one. Widths whose windows are the same share one strip: a short
/// document draws a single strip whatever the width.
pub(crate) fn page_strips(current: usize, total: usize) -> Vec<(Vec<PagerSlot>, String)> {
    let windows = RADII.map(|radius| page_window(current, total, radius));
    let mut strips: Vec<(Vec<PagerSlot>, String)> = Vec::new();
    for (most, width) in WIDTHS {
        let slots = windows
            .iter()
            .find(|slots| slots.len() <= most)
            .cloned()
            .unwrap_or_default();
        match strips.last_mut() {
            Some((shown, widths)) if *shown == slots => {
                widths.push(' ');
                widths.push_str(width);
            }
            _ => strips.push((slots, width.to_owned())),
        }
    }
    strips
}

/// Which page numbers to show, for `total` pages sitting on `current` (both
/// zero-based), `radius` pages either side of the current one.
///
/// A parish register runs to hundreds of pages, and drawing a button for each
/// produces a strip longer than the image above it. This keeps the ends —
/// "back to the start" and "how long is this" are both things a reader asks —
/// plus a window around where they are, and elides the rest.
///
/// A gap is only worth drawing if it hides more than one page: replacing a
/// single number with an ellipsis costs the same width and takes away a
/// destination, so a lone skipped page is shown instead.
pub(crate) fn page_window(current: usize, total: usize, radius: usize) -> Vec<PagerSlot> {
    if total == 0 {
        return Vec::new();
    }
    let last = total - 1;
    let current = current.min(last);
    let window_start = current.saturating_sub(radius);
    let window_end = (current + radius).min(last);

    let mut slots = Vec::new();
    let mut previous: Option<usize> = None;
    for page in (0..total)
        .filter(|page| *page == 0 || *page == last || (window_start..=window_end).contains(page))
    {
        match previous {
            // Two pages apart means exactly one was skipped; show it rather
            // than spend the same space on an ellipsis.
            Some(prev) if page == prev + 2 => slots.push(PagerSlot::Page(prev + 1)),
            Some(prev) if page > prev + 1 => slots.push(PagerSlot::Gap),
            _ => {}
        }
        slots.push(PagerSlot::Page(page));
        previous = Some(page);
    }
    slots
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The window of a pager at its widest.
    fn page_window(current: usize, total: usize) -> Vec<PagerSlot> {
        super::page_window(current, total, RADII[0])
    }

    #[test]
    fn a_short_document_shows_every_page() {
        // Nothing to elide, so eliding would only take away destinations.
        assert_eq!(
            page_window(0, 4),
            vec![
                PagerSlot::Page(0),
                PagerSlot::Page(1),
                PagerSlot::Page(2),
                PagerSlot::Page(3)
            ]
        );
    }

    #[test]
    fn a_register_of_hundreds_of_pages_stays_one_row() {
        let slots = page_window(50, 300);
        assert!(
            slots.len() <= 9,
            "the strip must not grow with the document: {slots:?}"
        );
        // Both ends stay reachable, and where the reader is stays visible.
        assert_eq!(slots.first(), Some(&PagerSlot::Page(0)));
        assert_eq!(slots.last(), Some(&PagerSlot::Page(299)));
        assert!(slots.contains(&PagerSlot::Page(50)));
        assert!(slots.contains(&PagerSlot::Gap));
    }

    #[test]
    fn the_ends_have_no_gap_beside_them_when_the_reader_is_there() {
        let slots = page_window(0, 300);
        // At the start the window already reaches page 0, so a gap belongs
        // only on the far side.
        assert_eq!(slots[0], PagerSlot::Page(0));
        assert_eq!(slots[1], PagerSlot::Page(1));
        assert_eq!(slots.iter().filter(|s| **s == PagerSlot::Gap).count(), 1);

        let slots = page_window(299, 300);
        assert_eq!(slots.iter().filter(|s| **s == PagerSlot::Gap).count(), 1);
    }

    #[test]
    fn a_single_skipped_page_is_shown_rather_than_elided() {
        // An ellipsis hiding one page costs the same width as the page and
        // takes away somewhere to go.
        let slots = page_window(4, 8);
        assert!(!slots.contains(&PagerSlot::Gap), "{slots:?}");
        assert!(slots.contains(&PagerSlot::Page(1)));
    }

    #[test]
    fn a_page_beyond_the_end_is_clamped_rather_than_panicking() {
        // Detaching a page while the viewer is open leaves the signal past the
        // end for one render.
        assert_eq!(page_window(99, 3).last(), Some(&PagerSlot::Page(2)));
        assert!(page_window(0, 0).is_empty());
    }

    #[test]
    fn each_width_has_exactly_one_strip_that_fits_it() {
        for (current, total) in [
            (0, 2),
            (0, 4),
            (0, 7),
            (3, 7),
            (2, 6),
            (50, 300),
            (299, 300),
        ] {
            let strips = page_strips(current, total);
            for (most, width) in WIDTHS {
                let shown: Vec<_> = strips
                    .iter()
                    .filter(|(_, widths)| widths.split(' ').any(|w| w == width))
                    .collect();
                assert_eq!(shown.len(), 1, "{width} at {current}/{total}: {strips:?}");
                let slots = &shown[0].0;
                assert!(
                    slots.len() <= most,
                    "{width} at {current}/{total}: {slots:?}"
                );
                // Both ends and the current page, at every width.
                assert_eq!(slots.first(), Some(&PagerSlot::Page(0)));
                assert_eq!(slots.last(), Some(&PagerSlot::Page(total - 1)));
                assert!(slots.contains(&PagerSlot::Page(current)));
            }
        }
    }

    #[test]
    fn a_narrow_pager_keeps_every_number_that_fits() {
        // A short document keeps all its pages however narrow the pager:
        // one strip, no copies.
        let strips = page_strips(0, 4);
        assert_eq!(strips.len(), 1, "{strips:?}");
        assert_eq!(strips[0].1, "fits-wide fits-medium fits-narrow");

        // Seven pages from the first: the wide window already fits.
        let strips = page_strips(0, 7);
        assert_eq!(strips.len(), 1, "{strips:?}");
        assert_eq!(
            strips[0].0,
            vec![
                PagerSlot::Page(0),
                PagerSlot::Page(1),
                PagerSlot::Page(2),
                PagerSlot::Gap,
                PagerSlot::Page(6)
            ]
        );

        // In the middle of a register the window narrows step by step, down
        // to the current page between the two ends.
        let strips = page_strips(50, 300);
        assert_eq!(strips.len(), 3, "{strips:?}");
        assert_eq!(
            strips[2],
            (
                vec![
                    PagerSlot::Page(0),
                    PagerSlot::Gap,
                    PagerSlot::Page(50),
                    PagerSlot::Gap,
                    PagerSlot::Page(299)
                ],
                "fits-narrow".to_owned()
            )
        );
    }
}
