//! What a pedigree theme decides about the shape of the chart.
//!
//! A theme is not a skin. Colors alone live in CSS variables and need nothing
//! here, but the things that make a pedigree look like an eighteenth-century
//! *Stammtafel* rather than a flat diagram — a framed card, a ruled connector,
//! room around a portrait for an ornament — are all *sizes*, and sizes are an
//! input to the layout, not a coat of paint over it.
//!
//! [`PedigreeMetrics`] is that input: the dimensions the Reingold-Tilford pass,
//! the connector generators and the card renderer must all agree on. Give a
//! card 20 more pixels of frame and every one of them has to hear about it, or
//! the cards overlap and the connectors stop landing on their edges.
//!
//! Anything only one card renderer cares about — where *this* theme puts its
//! photograph, how it spaces its two name lines — stays with that renderer.
//! Hoisting it here would grow a struct every theme has to fill in with values
//! most of them never read.

/// Every dimension the pedigree's geometry is derived from.
///
/// `Copy` and `PartialEq` so it can ride in a Dioxus prop without forcing a
/// re-render: comparing 20 floats is cheaper than laying the tree out again.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PedigreeMetrics {
    // ── Card boxes ───────────────────────────────────────────────────────
    /// Standard card width. Doubles as the layout's horizontal tree unit:
    /// `size_node` multiplies Reingold-Tilford coordinates by it.
    pub card_w: f64,
    /// Standard card height, and the vertical step between generations.
    pub card_h: f64,
    /// Width of a deepest-ancestor card, drawn portrait instead of landscape.
    pub compact_w: f64,
    /// Height of a deepest-ancestor card.
    pub compact_h: f64,
    /// Height of the first descendant row.
    pub desc_h: f64,

    // ── Card frame ───────────────────────────────────────────────────────
    /// Corner radius of the card rectangle.
    pub border_radius: f64,
    /// Gap between the card's own origin and its drawn rectangle. Connectors
    /// enter and leave at this inset, so the layout and the paths share it.
    pub padding: f64,
    /// Drawn width of a standard card's rectangle.
    pub inner_w: f64,
    /// Drawn height of a standard card's rectangle.
    pub inner_h: f64,
    /// Drawn width of a compact card's rectangle.
    pub compact_inner_w: f64,
    /// Drawn height of a compact card's rectangle.
    pub compact_inner_h: f64,

    // ── Connector attachment ─────────────────────────────────────────────
    /// Y offset from the card bottom where downward connectors enter.
    pub card_bottom_offset: f64,
    /// Y offset from the card top where upward connectors exit.
    pub card_top_offset: f64,
    /// Vertical indent of a connector's entry and exit segments.
    pub card_top_indent: f64,
    /// Horizontal control-point offset for the S-curve segments.
    pub bezier_ctrl_offset: f64,
    /// How far a spouse link stops short of the two card edges.
    pub spouse_link_inset: f64,

    // ── Canvas spacing ───────────────────────────────────────────────────
    /// Outer margin around the tree before the SVG viewBox is computed.
    pub layout_margin: f64,
    /// Horizontal spacing between the root's biological siblings, which are
    /// placed beside the tree rather than by the layout pass.
    pub sibling_spacing: f64,
    /// Per-spouse vertical step in a spouse/child connector row.
    pub sibling_vertical_step: f64,
    /// Floor on a spouse row's vertical offset.
    pub sibling_min_offset: f64,
}

impl PedigreeMetrics {
    /// The geometry OxidGene has always drawn.
    ///
    /// These numbers are load-bearing: the classic theme is specified to be
    /// pixel-for-pixel what shipped before themes existed, and
    /// `geometry_golden_tests` in `pedigree_chart` fails if any of them moves.
    pub const CLASSIC: Self = Self {
        card_w: 185.0,
        card_h: 96.0,
        compact_w: 95.0,
        compact_h: 144.0,
        desc_h: 140.0,

        border_radius: 5.0,
        padding: 5.0,
        inner_w: 175.0,
        inner_h: 67.0,
        compact_inner_w: 82.0,
        compact_inner_h: 115.0,

        card_bottom_offset: 23.0,
        card_top_offset: 4.0,
        card_top_indent: 5.0,
        bezier_ctrl_offset: 8.0,
        spouse_link_inset: 15.0,

        layout_margin: 50.0,
        sibling_spacing: 200.0,
        sibling_vertical_step: 4.0,
        sibling_min_offset: 6.0,
    };

    /// The drawn rectangle of a card at this depth.
    #[must_use]
    pub const fn rect(&self, is_compact: bool) -> (f64, f64) {
        if is_compact {
            (self.compact_inner_w, self.compact_inner_h)
        } else {
            (self.inner_w, self.inner_h)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The drawn rectangle has to fit inside the box the layout reserved, or
    /// neighbouring cards touch even though the layout believes they do not.
    /// A theme that widens its frame has to widen the box with it, and this is
    /// what says so.
    #[test]
    fn a_card_rectangle_fits_the_box_the_layout_reserves() {
        let m = PedigreeMetrics::CLASSIC;
        for (is_compact, box_w, box_h) in [
            (false, m.card_w, m.card_h),
            (true, m.compact_w, m.compact_h),
        ] {
            let (rw, rh) = m.rect(is_compact);
            assert!(
                rw + 2.0 * m.padding <= box_w,
                "compact={is_compact}: rectangle {rw} + padding overflows {box_w}"
            );
            assert!(
                rh + 2.0 * m.padding <= box_h,
                "compact={is_compact}: rectangle {rh} + padding overflows {box_h}"
            );
        }
    }

    /// Connectors attach between the card's top and bottom edges. An offset
    /// past either one would have a line leave from outside the card it
    /// belongs to — visible immediately, but only on a theme nobody has
    /// drawn yet, which is exactly when a cheap assertion is worth having.
    #[test]
    fn connectors_attach_within_the_card() {
        let m = PedigreeMetrics::CLASSIC;
        assert!(m.card_bottom_offset > 0.0 && m.card_bottom_offset < m.card_h);
        assert!(m.card_top_offset >= 0.0 && m.card_top_offset < m.card_h);
        assert!(m.spouse_link_inset >= 0.0 && m.spouse_link_inset < m.card_w / 2.0);
    }
}
