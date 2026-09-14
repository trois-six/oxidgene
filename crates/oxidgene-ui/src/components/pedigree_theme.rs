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
    /// Width of a deepest-ancestor column, in `card_w` units.
    ///
    /// The deepest ancestor row is the widest row of the chart, so it is
    /// packed tighter than the rest. The classic theme can afford half a
    /// column because its compact card is a narrow portrait standing under
    /// landscape cards; a theme whose cards are portrait at every rank has
    /// no such slack and has to buy its top row more room.
    pub compact_separation: f64,
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
        compact_separation: 0.5,
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

    /// The column one card is laid out in, at this depth.
    ///
    /// A card is drawn at `padding` inside its column and is `rect().0` wide,
    /// so `padding + rect().0` must fit here or neighbours overlap. That is
    /// the one relation between these numbers a theme cannot get wrong, and
    /// `no_theme_lets_its_cards_eat_their_neighbours` holds every theme to it.
    #[must_use]
    pub const fn column(&self, is_compact: bool) -> f64 {
        if is_compact {
            self.card_w * self.compact_separation
        } else {
            self.card_w
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
    /// A bombé cartouche, drawn twice: sides that swell outward, a crown
    /// that lifts at the centre, corners rolled like a cut sheet, and a drop
    /// at the foot. `inner_inset` is how far the second rule sits inside the
    /// first.
    ///
    /// This is the shape a *Stammtafel* puts a person in, and the reason the
    /// theme exists — a rectangle with a second rule around it is still a
    /// diagram.
    Cartouche { inner_inset: f64 },
}

/// The outline of a card, as an SVG path.
///
/// `None` for [`CardFrame::Plain`]: the renderer draws that as a `<rect>`,
/// and writing a rectangle as a path would be the same shape at more cost.
#[must_use]
pub fn frame_path(frame: CardFrame, x: f64, y: f64, w: f64, h: f64) -> Option<String> {
    match frame {
        CardFrame::Plain => None,
        CardFrame::Cartouche { .. } => Some(cartouche_path(x, y, w, h)),
    }
}

/// Every extreme of the cartouche touches the box it is handed, so the shape
/// occupies exactly the rectangle the layout reserved and nothing spills into
/// a neighbouring card.
///
/// The outline is asymmetric top to bottom, which is what tells it apart from
/// a rounded rectangle: a crown that rises to a point at the centre, shoulders
/// that fall away to corners rolled back on themselves, sides that pinch below
/// the roll and then swell, and a foot drawn down to a tongue. Every landmark
/// is a fraction of the box, so a compact card is the same shape as a full one
/// rather than a squashed one.
fn cartouche_path(x: f64, y: f64, w: f64, h: f64) -> String {
    // Landmarks as fractions of the box, so a compact card is the same shape
    // as a full one rather than a squashed one.
    let fx = |f: f64| x + w * f;
    let fy = |f: f64| y + h * f;
    let cx = x + w * 0.5;
    let (xw, yh) = (x + w, y + h);

    // Crown: a shallow arch rising to the centre, falling to corners that
    // flare outward. No corner scroll — a scroll drawn small enough to fit a
    // card loops back over the crown and reads as a handle, not a curl.
    let crown_right = format!(
        "M{cx},{y} C{},{} {},{} {},{}",
        fx(0.62),
        fy(0.004),
        fx(0.80),
        fy(0.018),
        fx(0.950),
        fy(0.055)
    );
    let corner_right = format!(
        " C{},{} {xw},{} {xw},{}",
        fx(0.992),
        fy(0.070),
        fy(0.105),
        fy(0.165)
    );
    // The flanks stay full width well past the names, so the taper never
    // crowds a line of text.
    let flank_right = format!(" C{xw},{} {xw},{} {xw},{}", fy(0.40), fy(0.58), fy(0.750));
    // Foot: drawn in to a point at the centre.
    let foot_right = format!(" C{xw},{} {},{} {cx},{yh}", fy(0.865), fx(0.800), fy(0.952));
    let foot_left = format!(
        " C{},{} {x},{} {x},{}",
        fx(0.200),
        fy(0.952),
        fy(0.865),
        fy(0.750)
    );
    let flank_left = format!(" C{x},{} {x},{} {x},{}", fy(0.58), fy(0.40), fy(0.165));
    let corner_left = format!(
        " C{x},{} {},{} {},{}",
        fy(0.105),
        fx(0.008),
        fy(0.070),
        fx(0.050),
        fy(0.055)
    );
    let crown_left = format!(
        " C{},{} {},{} {cx},{y} Z",
        fx(0.20),
        fy(0.018),
        fx(0.38),
        fy(0.004)
    );

    format!(
        "{crown_right}{corner_right}{flank_right}{foot_right}\
         {foot_left}{flank_left}{corner_left}{crown_left}"
    )
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
    /// Whether to paint a ground behind the portrait.
    ///
    /// A portrait keeps its aspect ratio, so it rarely fills its box exactly
    /// and something shows through beside it. A flat card wants that to be
    /// paper white; an engraved card wants its paper-toned fill, and painting
    /// a mat there only draws a shape around the photograph that does not
    /// follow it.
    pub photo_mat: bool,

    pub text_x_full: f64,
    pub text_x_compact: f64,
    pub text_y_full: f64,
    pub text_y_compact: f64,
    pub text_max_width_full: f32,
    /// Widest text a compact card's column holds. Stated rather than derived:
    /// a centred line has whatever its narrower side allows, which is not
    /// what subtracting a left indent from the card width measures.
    pub text_max_width_compact: f32,
    /// SVG `text-anchor` for the name lines. A cartouche centres them under
    /// its medallion; a landscape card sets them beside it.
    pub text_anchor: &'static str,
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
    /// Baseline position of the "+" that marks relations outside the layout.
    pub more_relations_x: f64,
    pub more_relations_y_full: f64,
    pub more_relations_y_compact: f64,
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
    /// variables are defined. Empty for the theme that uses the application's
    /// own palette.
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
            photo_mat: true,

            text_x_full: 70.0,
            text_x_compact: 10.0,
            text_y_full: 21.0,
            text_y_compact: 81.0,
            text_max_width_full: 105.0,
            text_max_width_compact: 72.0,
            text_anchor: "start",
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
            more_relations_x: 10.0,
            more_relations_y_full: 3.0,
            more_relations_y_compact: 3.0,
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
            // Portrait, not landscape. A cartouche is taller than it is wide
            // and carries its names under the portrait rather than beside
            // it, which is the arrangement these plates actually use.
            card_w: 194.0,
            card_h: 190.0,
            // Three quarters of a column rather than the classic half: an
            // escutcheon is portrait at every rank, so halving the top row
            // would have the shields overlap instead of merely standing
            // close. At 0.75 the gap between two crowns up there (37px) is
            // the gap between two cartouches anywhere else (36px).
            compact_w: 145.5,
            compact_h: 188.0,
            compact_separation: 0.75,
            desc_h: 230.0,

            // No corner radius: the shape is a path, and its corners are
            // rolled by the path itself.
            border_radius: 0.0,
            // Wide enough that the swells of two neighbouring cartouches
            // never meet.
            padding: 18.0,
            inner_w: 158.0,
            inner_h: 162.0,
            compact_inner_w: 108.0,
            compact_inner_h: 160.0,

            // Connectors meet the crown and the foot exactly, where the
            // cartouche reaches the edge of its box.
            card_bottom_offset: 18.0,
            card_top_offset: 18.0,
            card_top_indent: 6.0,
            bezier_ctrl_offset: 8.0,
            spouse_link_inset: 18.0,

            layout_margin: 70.0,
            sibling_spacing: 200.0,
            sibling_vertical_step: 4.0,
            sibling_min_offset: 6.0,
        },
        link_style: LinkStyle::Ruled,
        viewport_class: "ped-theme-medieval",
        card: CardStyle {
            frame: CardFrame::Cartouche { inner_inset: 7.0 },
            frame_stroke: FrameStroke::Gender,
            frame_width: 2.2,

            // A medallion centred under the crown, the way a portrait is set
            // into one of these plates.
            photo_w: 66.0,
            photo_h: 66.0,
            photo_y: 30.0,
            photo_x_full: 64.0,
            photo_x_compact: 39.0,
            photo_round: 33.0,
            photo_mat: false,

            text_x_full: 97.0,
            text_x_compact: 72.0,
            // Both ranks stack their names under the same medallion, so
            // both start at the same baseline: far enough below it that a
            // capital clears the photograph, and high enough that the
            // lifespan still lands above the foot, where the cartouche draws
            // in toward its point and a centred line would run past the rule.
            text_y_full: 110.0,
            text_y_compact: 110.0,
            text_max_width_full: 112.0,
            text_max_width_compact: 96.0,
            text_anchor: "middle",
            name_line_step: 18.0,

            surname_font_px: 13.0,
            given_font_px: 11.0,
            date_font_px: 10.0,
            // Cinzel is already loaded for headings, so the theme costs no
            // extra font request. Its Roman capitals are the whole look.
            surname_font: "'Cinzel',Georgia,serif",
            body_font: "Georgia,'Times New Roman',serif",
            surname_weight: "600",

            // The frame carries the sex colour instead of a separate rule.
            gender_rule: None,

            // Pinned to the medallion's lower right, where a plate puts its
            // number.
            sosa_cx_full: 120.0,
            sosa_cx_compact: 95.0,
            sosa_cy: 86.0,
            sosa_r: 9.0,

            edit_fab_r: 14.0,
            edit_fab_gap: 16.0,
            // Left of the cartouche, level with its bottom point and clear of
            // the ruled connector that enters that point at the centre.
            more_relations_x: 30.0,
            more_relations_y_full: 180.0,
            more_relations_y_compact: 178.0,
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
    /// both languages.
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

    /// Two themes that redefine colors under the same class would silently
    /// share a palette.
    #[test]
    fn themes_with_their_own_palette_have_their_own_class() {
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
