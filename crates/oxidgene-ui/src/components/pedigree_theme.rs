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

use serde::{Deserialize, Serialize};

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

/// A point in layout coordinates — the space cards are placed in, before the
/// canvas transforms are applied.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    #[must_use]
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

/// How a theme draws the line between two cards.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkStyle {
    /// Elbows, softened into an S-curve wherever a connector has to step
    /// sideways — the shape OxidGene has always drawn.
    Bezier,
    /// Ruled elbows throughout: right angles, no curve anywhere. What a drawn
    /// pedigree does, where every line was laid down with a straightedge.
    Ruled,
}

/// One connector to draw, named by what it joins rather than by its shape.
///
/// The endpoints are derived from the cards and the metrics, so both styles
/// attach in the same places and only the run between them differs. That is
/// the whole seam: a theme changes how a line travels, never where it lands.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LinkSpec {
    /// A node across to one of its spouses.
    Spouse {
        from: Point,
        to: Point,
        y_offset: f64,
    },
    /// A parent down to a child, where the parent carries no spouse card.
    SimpleChild {
        from: Point,
        to: Point,
        is_edge: bool,
    },
    /// A child up to one of its ancestors.
    Ancestor {
        from: Point,
        to: Point,
        from_has_prev_sibling: bool,
        from_has_next_sibling: bool,
        from_depth: i32,
        to_depth: i32,
        last_level: i32,
    },
    /// A parent across to one of the root's own biological siblings, which are
    /// placed beside the tree rather than by the layout pass.
    RootSibling {
        from: Point,
        to: Point,
        from_depth: i32,
        index: usize,
        count: usize,
        simple: bool,
        last_level: i32,
    },
    /// A spouse down to one of the couple's children.
    Child {
        from: Point,
        to: Point,
        parent_after: i32,
        y_offset: f64,
        is_edge: bool,
    },
}

/// Where a connector starts, where it ends, and the row it turns on.
struct Ends {
    sx: f64,
    sy: f64,
    ex: f64,
    ey: f64,
    /// Y of the horizontal run joining the two verticals.
    mid: f64,
}

/// Horizontal control-point X for an S-curve, stepping `offset` inward toward
/// the destination from the source.
fn ctrl_x_toward(src: f64, dst: f64, offset: f64) -> f64 {
    if src > dst {
        dst + offset
    } else {
        dst - offset
    }
}

/// Horizontal control-point X stepping `offset` outward from the source.
fn ctrl_x_outward(src: f64, dst: f64, offset: f64) -> f64 {
    if src > dst {
        src - offset
    } else {
        src + offset
    }
}

/// The attachment points of a connector, which every style shares.
fn endpoints(spec: &LinkSpec, m: &PedigreeMetrics) -> Ends {
    match *spec {
        // Handled without endpoints — a spouse link is one horizontal run.
        LinkSpec::Spouse { from, to, y_offset } => Ends {
            sx: from.x + m.card_w - m.spouse_link_inset,
            sy: from.y + y_offset,
            ex: to.x + m.padding,
            ey: from.y + y_offset,
            mid: from.y + y_offset,
        },
        LinkSpec::SimpleChild { from, to, .. } => {
            let sx = from.x + m.card_w / 2.0;
            let sy = from.y + m.card_h - m.card_bottom_offset;
            let ex = to.x + m.card_w / 2.0;
            let ey = to.y + m.padding;
            Ends {
                sx,
                sy,
                ex,
                ey,
                mid: (sy + ey) / 2.0,
            }
        }
        LinkSpec::Ancestor {
            from,
            to,
            to_depth,
            last_level,
            ..
        } => {
            let sw = if to_depth == last_level {
                m.compact_w
            } else {
                m.card_w
            };
            let sh = if to_depth == last_level {
                m.compact_h
            } else if to_depth > 0 {
                m.desc_h
            } else {
                m.card_h
            };
            let sx = from.x + m.card_w / 2.0;
            let sy = from.y + m.card_top_offset;
            let ex = to.x + sw / 2.0;
            let ey = to.y + sh - m.card_bottom_offset;
            Ends {
                sx,
                sy,
                ex,
                ey,
                mid: (sy + ey) / 2.0,
            }
        }
        LinkSpec::RootSibling {
            from,
            to,
            from_depth,
            last_level,
            ..
        } => {
            let sw = if from_depth == last_level {
                m.compact_w
            } else {
                m.card_w
            };
            let sh = if from_depth == last_level {
                m.compact_h
            } else {
                m.card_h
            };
            let sx = from.x + sw / 2.0;
            let sy = from.y + sh - m.card_bottom_offset;
            let ex = to.x + m.card_w / 2.0;
            let ey = to.y + m.card_top_offset;
            Ends {
                sx,
                sy,
                ex,
                ey,
                mid: (sy + ey) / 2.0,
            }
        }
        LinkSpec::Child {
            from,
            to,
            parent_after,
            y_offset,
            ..
        } => {
            let sx = if parent_after == 1 {
                from.x + m.card_w
            } else {
                from.x
            };
            Ends {
                sx,
                sy: from.y + y_offset,
                ex: to.x + m.card_w / 2.0,
                ey: to.y + m.card_top_offset,
                // Not the midpoint: the row is pinned relative to the child so
                // a fan of siblings turns on one line rather than on a dozen.
                mid: to.y + (m.card_h - m.card_bottom_offset) / 2.0 - m.layout_margin,
            }
        }
    }
}

/// Down, across, up: the right-angled run every style falls back to.
///
/// The trailing pairs carry no command letter, which SVG reads as more `L`
/// segments. That is how these paths have always been written.
fn elbow(e: &Ends) -> String {
    let Ends {
        sx,
        sy,
        ex,
        ey,
        mid,
    } = *e;
    format!("M{sx},{sy} L{sx},{mid} {ex},{mid} {ex},{ey}")
}

/// The `d` attribute for one connector.
///
/// [`LinkStyle::Ruled`] is not a reduced version of the Bézier style — it is
/// the same elbow the Bézier style already draws for every connector that
/// does not have to step sideways, applied throughout.
#[must_use]
pub fn link_path(spec: &LinkSpec, style: LinkStyle, m: &PedigreeMetrics) -> String {
    let e = endpoints(spec, m);
    let Ends {
        sx,
        sy,
        ex,
        ey,
        mid,
    } = e;

    // A spouse link is one horizontal run whatever the style.
    if matches!(spec, LinkSpec::Spouse { .. }) {
        return format!("M{sx},{sy} L{ex},{ey}");
    }

    let ruled = style == LinkStyle::Ruled;
    let off = m.bezier_ctrl_offset;

    match *spec {
        LinkSpec::Spouse { .. } => unreachable!("handled above"),

        LinkSpec::SimpleChild { is_edge, .. } => {
            if ruled || !(is_edge && (sx - ex).abs() > 0.5) {
                return elbow(&e);
            }
            // This one spells out the second `L`, unlike its siblings below.
            // Same geometry, and kept verbatim so the classic theme stays
            // byte-for-byte what shipped.
            let ctrl = ctrl_x_toward(sx, ex, off);
            format!(
                "M{sx},{sy} L{sx},{mid} L{ctrl},{mid} S{ex},{mid} {ex},{} L{ex},{ey}",
                mid + off
            )
        }

        LinkSpec::Ancestor {
            from_has_prev_sibling,
            from_has_next_sibling,
            from_depth,
            ..
        } => {
            // The root's own siblings would be crossed by a full run, so the
            // connector stops on the turning row instead of climbing to the
            // ancestor's edge.
            let would_cross = from_depth == 0
                && ((from_has_prev_sibling && sx > ex) || (from_has_next_sibling && sx < ex));
            if would_cross {
                return if ruled {
                    format!("M{sx},{sy} L{sx},{mid} {ex},{mid}")
                } else {
                    let c1x = ctrl_x_outward(sx, ex, off);
                    format!(
                        "M{sx},{sy} L{sx},{} S{sx},{mid} {c1x},{mid} L{ex},{mid}",
                        sy - m.card_top_indent
                    )
                };
            }
            if ruled {
                return elbow(&e);
            }
            let c1x = ctrl_x_outward(sx, ex, off);
            let c2x = ctrl_x_toward(sx, ex, off);
            format!(
                "M{sx},{sy} L{sx},{} S{sx},{mid} {c1x},{mid} L{c2x},{mid} S{ex},{mid} {ex},{} L{ex},{ey}",
                sy - m.card_top_indent,
                ey + m.card_top_indent
            )
        }

        LinkSpec::RootSibling {
            index,
            count,
            simple,
            ..
        } => {
            // Only the outermost sibling curves; the ones between it and the
            // root run straight, so the row does not turn into a ripple.
            let straight = index != count.saturating_sub(1) || (sx - ex).abs() < 0.001 || simple;
            if ruled || straight {
                return elbow(&e);
            }
            let ctrl = ctrl_x_toward(sx, ex, off);
            format!(
                "M{sx},{sy} L{sx},{mid} {ctrl},{mid} S{ex},{mid} {ex},{} L{ex},{ey}",
                mid + off
            )
        }

        LinkSpec::Child { is_edge, .. } => {
            if ruled || !(is_edge && (sx - ex).abs() > 0.5) {
                return elbow(&e);
            }
            let ctrl = ctrl_x_toward(sx, ex, off);
            format!(
                "M{sx},{sy} L{sx},{mid} {ctrl},{mid} S{ex},{mid} {ex},{} L{ex},{ey}",
                mid + off
            )
        }
    }
}

/// How a card's outline is drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CardFrame {
    /// One rectangle, hairline, optionally rounded.
    Plain,
    /// Two concentric rules, the outer one heavier — how an engraved
    /// cartouche is drawn, and most of what separates a painted pedigree
    /// from a diagram. The value is the inset of the inner rule.
    Cartouche { inner_inset: f64 },
}

/// What a card's outline is stroked with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameStroke {
    /// The neutral border colour, with the sex shown by a separate rule.
    Border,
    /// The sex colour itself, for a frame heavy enough to carry it.
    Gender,
}

/// A rule running down one edge of the card, coloured by the person's sex.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GenderRule {
    pub x_full: f64,
    pub x_compact: f64,
    pub top: f64,
    pub bottom: f64,
    pub width: f64,
}

/// Where a theme puts the things inside a card, and what it sets them in.
///
/// Phase two kept these with the renderer, on the grounds that only one
/// renderer had an opinion about them. A second theme sharing that renderer
/// is what makes them data: a medieval card is larger, so a photograph left
/// at the classic offset would sit against its frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CardStyle {
    pub frame: CardFrame,
    pub frame_stroke: FrameStroke,
    /// Stroke width of the outer rule.
    pub frame_width: f64,

    pub photo_w: f64,
    pub photo_h: f64,
    pub photo_y: f64,
    pub photo_x_full: f64,
    pub photo_x_compact: f64,
    /// Corner radius of the portrait mat. Half the width makes it a
    /// medallion, which is what an engraved pedigree draws.
    pub photo_round: f64,

    pub text_x_full: f64,
    pub text_x_compact: f64,
    pub text_y_full: f64,
    pub text_y_compact: f64,
    pub text_max_width_full: f32,
    /// Baseline step between the given name, the surname and the lifespan.
    pub name_line_step: f64,

    pub surname_font_px: f32,
    pub given_font_px: f32,
    pub date_font_px: f32,
    /// CSS `font-family` for the surname, and for everything else.
    pub surname_font: &'static str,
    pub body_font: &'static str,
    /// `font-weight` of the surname line.
    pub surname_weight: &'static str,

    /// The sex-coded rule, when the theme draws one. A card whose whole
    /// frame carries the colour does not need it.
    pub gender_rule: Option<GenderRule>,

    pub sosa_cx_full: f64,
    pub sosa_cx_compact: f64,
    pub sosa_cy: f64,
    pub sosa_r: f64,

    pub edit_fab_r: f64,
    pub edit_fab_gap: f64,
    /// Baseline nudge that centres the "+" glyph in an empty slot.
    pub slot_plus_baseline: f64,
}

/// How a theme choice is spelled where it is stored and chosen.
///
/// The charts take a whole [`PedigreeTheme`]; this is the name the viewer's
/// preference keeps, so `localStorage` holds `"medieval"` rather than a
/// snapshot of forty numbers that would go stale the moment a theme is
/// adjusted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PedigreeThemeId {
    #[default]
    Classic,
    Medieval,
}

impl PedigreeThemeId {
    /// Every theme, in the order the selector offers them.
    pub const ALL: [Self; 2] = [Self::Classic, Self::Medieval];

    #[must_use]
    pub const fn theme(self) -> &'static PedigreeTheme {
        match self {
            Self::Classic => &PedigreeTheme::CLASSIC,
            Self::Medieval => &PedigreeTheme::MEDIEVAL,
        }
    }

    /// Translation key for the theme's name.
    #[must_use]
    pub const fn label_key(self) -> &'static str {
        match self {
            Self::Classic => "app_settings.pedigree_theme_classic",
            Self::Medieval => "app_settings.pedigree_theme_medieval",
        }
    }

    /// Translation key for the one line describing it in the selector.
    #[must_use]
    pub const fn hint_key(self) -> &'static str {
        match self {
            Self::Classic => "app_settings.pedigree_theme_classic_hint",
            Self::Medieval => "app_settings.pedigree_theme_medieval_hint",
        }
    }
}

/// Everything a theme decides about the shape of the chart.
///
/// Colors are not here: those are CSS variables, redefined under
/// `viewport_class`, and nothing in the layout needs to know about them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PedigreeTheme {
    pub metrics: PedigreeMetrics,
    pub link_style: LinkStyle,
    pub card: CardStyle,
    /// Class set on the pedigree viewport, under which the theme's CSS
    /// variables and canvas ground are defined. Empty for the theme that
    /// uses the application's own palette.
    pub viewport_class: &'static str,
}

impl PedigreeTheme {
    /// What OxidGene has always drawn.
    pub const CLASSIC: Self = Self {
        metrics: PedigreeMetrics::CLASSIC,
        link_style: LinkStyle::Bezier,
        viewport_class: "",
        card: CardStyle {
            frame: CardFrame::Plain,
            frame_stroke: FrameStroke::Border,
            frame_width: 1.0,

            photo_w: 50.0,
            photo_h: 50.0,
            photo_y: 10.0,
            photo_x_full: 10.0,
            photo_x_compact: 20.0,
            photo_round: 0.0,

            text_x_full: 70.0,
            text_x_compact: 10.0,
            text_y_full: 21.0,
            text_y_compact: 81.0,
            text_max_width_full: 105.0,
            name_line_step: 14.0,

            surname_font_px: 11.0,
            given_font_px: 10.0,
            date_font_px: 10.0,
            surname_font: "'Lato',sans-serif",
            body_font: "'Lato',sans-serif",
            surname_weight: "700",

            gender_rule: Some(GenderRule {
                x_full: 9.0,
                x_compact: 19.0,
                top: 10.0,
                bottom: 60.0,
                width: 2.0,
            }),

            sosa_cx_full: 57.5,
            sosa_cx_compact: 67.5,
            sosa_cy: 57.5,
            sosa_r: 7.5,

            edit_fab_r: 14.0,
            edit_fab_gap: 16.0,
            slot_plus_baseline: 8.0,
        },
    };

    /// An engraved pedigree: ruled lines, a double-ruled cartouche around
    /// each person, a portrait in a medallion, and Roman capitals.
    ///
    /// The card is larger than the classic one in every direction, which is
    /// not decoration for its own sake: the second rule and the ring around
    /// the medallion take real room, and taking it from the text instead
    /// would leave the names of a French branch truncated where the classic
    /// theme shows them whole.
    pub const MEDIEVAL: Self = Self {
        metrics: PedigreeMetrics {
            card_w: 210.0,
            card_h: 112.0,
            compact_w: 110.0,
            compact_h: 164.0,
            desc_h: 156.0,

            // Square corners: an engraver had no rounded rectangle.
            border_radius: 0.0,
            padding: 5.0,
            inner_w: 200.0,
            inner_h: 82.0,
            compact_inner_w: 97.0,
            compact_inner_h: 134.0,

            card_bottom_offset: 26.0,
            card_top_offset: 4.0,
            card_top_indent: 5.0,
            bezier_ctrl_offset: 8.0,
            spouse_link_inset: 15.0,

            layout_margin: 60.0,
            sibling_spacing: 225.0,
            sibling_vertical_step: 4.0,
            sibling_min_offset: 6.0,
        },
        link_style: LinkStyle::Ruled,
        viewport_class: "ped-theme-medieval",
        card: CardStyle {
            frame: CardFrame::Cartouche { inner_inset: 4.0 },
            frame_stroke: FrameStroke::Gender,
            frame_width: 2.0,

            photo_w: 56.0,
            photo_h: 56.0,
            photo_y: 13.0,
            photo_x_full: 14.0,
            photo_x_compact: 25.5,
            photo_round: 28.0,

            text_x_full: 82.0,
            text_x_compact: 12.0,
            text_y_full: 30.0,
            text_y_compact: 94.0,
            text_max_width_full: 110.0,
            name_line_step: 17.0,

            surname_font_px: 12.0,
            given_font_px: 11.0,
            date_font_px: 10.0,
            // Cinzel is already loaded for headings, so the theme costs no
            // extra font request. Its Roman capitals are the whole look.
            surname_font: "'Cinzel',Georgia,serif",
            body_font: "Georgia,'Times New Roman',serif",
            surname_weight: "600",

            // The frame carries the sex colour instead of a separate rule.
            gender_rule: None,

            sosa_cx_full: 64.0,
            sosa_cx_compact: 75.5,
            sosa_cy: 63.0,
            sosa_r: 8.0,

            edit_fab_r: 14.0,
            edit_fab_gap: 16.0,
            slot_plus_baseline: 8.0,
        },
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The stored name has to survive a round trip, or a reader's choice
    /// silently reverts on their next visit.
    #[test]
    fn every_theme_name_round_trips_through_storage() {
        for id in PedigreeThemeId::ALL {
            let stored = serde_json::to_string(&id).expect("serialised");
            let read_back: PedigreeThemeId = serde_json::from_str(&stored).expect("parsed");
            assert_eq!(read_back, id, "{id:?} came back as {read_back:?}");
        }
        // The name in storage is the theme's own, not its position in the
        // list — inserting a theme must not repaint everyone's charts.
        assert_eq!(
            serde_json::to_string(&PedigreeThemeId::Medieval).unwrap(),
            "\"medieval\""
        );
    }

    /// A theme the selector offers has to have something to say for itself in
    /// both languages, and its own canvas class if it repaints the ground.
    #[test]
    fn every_theme_is_named_in_both_languages() {
        use crate::i18n::{I18n, Language};

        for id in PedigreeThemeId::ALL {
            for language in [Language::En, Language::Fr] {
                let i18n = I18n(language);
                for key in [id.label_key(), id.hint_key()] {
                    let text = i18n.t(key);
                    assert_ne!(text, key, "{id:?}: {key} is untranslated in {language:?}");
                    assert!(!text.is_empty(), "{id:?}: {key} is empty in {language:?}");
                }
            }
        }
    }

    /// Two themes that painted the same ground under the same class would
    /// silently share a palette.
    #[test]
    fn themes_that_repaint_the_canvas_have_their_own_class() {
        let mut seen: Vec<&str> = Vec::new();
        for id in PedigreeThemeId::ALL {
            let class = id.theme().viewport_class;
            if class.is_empty() {
                continue;
            }
            assert!(!seen.contains(&class), "{class} is claimed by two themes");
            seen.push(class);
        }
    }

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
