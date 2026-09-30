//! The ancestor wheel and the fan chart.
//!
//! Both put the root at the centre and each generation of ancestors on a
//! ring around it, split into one segment per SOSA position: the wheel over a
//! full circle, the fan over its upper half with the root at the base. The
//! father's side takes the first half of every arc, the mother's the second,
//! so the two lines never cross.
//!
//! Angles are in degrees, clockwise from twelve o'clock, which is how SVG's
//! `rotate()` turns as well. Everything here is pure geometry and text
//! fitting, computed once per layout; the component at the end only places
//! what it decided.

use super::ancestors::{
    AncestorEntry, ancestor_tooltip, collect_ancestors, generation_of, index_in_generation,
    mark_colour, svg_title,
};
use super::*;

/// The part of a circle a chart spreads its generations over.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct ChartArc {
    /// Where the father's side begins.
    pub(super) start: f64,
    /// How far the generations reach around the root.
    pub(super) sweep: f64,
    /// Radius of the root's own disc.
    pub(super) root_radius: f64,
    /// Whether the root's disc is a whole circle or only its upper half.
    pub(super) full_circle: bool,
}

impl ChartArc {
    /// Father's side on the left, from six o'clock round through nine to
    /// twelve; the mother's on the right.
    pub(super) const WHEEL: Self = Self {
        start: 180.0,
        sweep: 360.0,
        root_radius: 72.0,
        full_circle: true,
    };

    /// The upper half circle, from nine o'clock to three: father's side on
    /// the left. The root sits on the base, in a half disc large enough for
    /// three lines of text.
    pub(super) const FAN: Self = Self {
        start: 270.0,
        sweep: 180.0,
        root_radius: 96.0,
        full_circle: false,
    };
}

/// Radial depth of a ring whose names run along the arc.
const TANGENTIAL_RING: f64 = 64.0;
/// Radial depth of a ring whose names run along the radius: a name needs
/// more room lengthwise than a ring of three stacked lines does.
const RADIAL_RING: f64 = 132.0;
/// Below this chord, a straight line of text no longer fits across a
/// segment and the ring turns its names to follow the radius.
const MIN_TANGENTIAL_CHORD: f64 = 110.0;
/// The narrowest a segment may be at its inner edge: one line of the
/// smallest type, with a little air. Deep generations push their ring
/// outwards until it holds, so an eighth generation is still legible.
pub(super) const MIN_SEGMENT_ARC: f64 = 15.0;
/// Air between a line of text and the edge of its segment.
const LABEL_PAD: f64 = 5.0;
/// Room around the chart, so strokes at its rim are not cut off.
const CHART_MARGIN: f64 = 24.0;

const TANGENTIAL_LINE: f64 = 15.0;
const RADIAL_LINE: f64 = 13.0;

/// The SVG point at radius `r`, `deg` degrees clockwise from twelve o'clock.
pub(super) fn polar(r: f64, deg: f64) -> (f64, f64) {
    let a = deg.to_radians();
    (r * a.sin(), -r * a.cos())
}

/// Where the segment of a SOSA number starts and ends on `arc`.
pub(super) fn segment_angles(sosa: u64, arc: ChartArc) -> (f64, f64) {
    let span = arc.sweep / (1u64 << generation_of(sosa)) as f64;
    let start = arc.start + index_in_generation(sosa) as f64 * span;
    (start, start + span)
}

/// Which way a ring's names run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LabelFlow {
    /// Across the segment, like a line of print, turned to follow the arc.
    Tangential,
    /// Along the radius, from the centre out (or in, on the left half, so
    /// that no name is read upside down).
    Radial,
}

/// One generation's ring.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Ring {
    pub(super) r_in: f64,
    pub(super) r_out: f64,
    pub(super) flow: LabelFlow,
}

/// The chord a segment of `span_deg` subtends at radius `r`.
fn chord(r: f64, span_deg: f64) -> f64 {
    2.0 * r * (span_deg.min(180.0).to_radians() / 2.0).sin()
}

/// The ring of every generation from 1 to `generations`, innermost first.
///
/// Rings touch: each starts where the previous one ends. The first rings
/// write their names across, until a segment gets too narrow for that; from
/// there on names follow the radius. A ring whose segments would be thinner
/// than [`MIN_SEGMENT_ARC`] at its inner edge starts further out instead, the
/// ring inside it widening to meet it — the extra room only lengthens the
/// names that ring can hold.
pub(super) fn ring_radii(arc: ChartArc, generations: u32) -> Vec<Ring> {
    let mut rings: Vec<Ring> = Vec::new();
    let mut r = arc.root_radius;
    let mut flow = LabelFlow::Tangential;
    for generation in 1..=generations {
        let span = arc.sweep / f64::from(1u32 << generation.min(31));
        if flow == LabelFlow::Tangential && chord(r, span) < MIN_TANGENTIAL_CHORD {
            flow = LabelFlow::Radial;
        }
        let needed = MIN_SEGMENT_ARC / span.to_radians();
        if needed > r {
            if let Some(previous) = rings.last_mut() {
                previous.r_out = needed;
            }
            r = needed;
        }
        let depth = match flow {
            LabelFlow::Tangential => TANGENTIAL_RING,
            LabelFlow::Radial => RADIAL_RING,
        };
        rings.push(Ring {
            r_in: r,
            r_out: r + depth,
            flow,
        });
        r += depth;
    }
    rings
}

/// `x,y` for a path, rounded so the markup stays short.
fn pt((x, y): (f64, f64)) -> String {
    format!("{x:.2},{y:.2}")
}

/// The outline of the ring sector between two radii and two angles.
pub(super) fn sector_path(r_in: f64, r_out: f64, a0: f64, a1: f64) -> String {
    let large = u8::from(a1 - a0 > 180.0);
    format!(
        "M{} A{r_out:.2},{r_out:.2} 0 {large} 1 {} L{} A{r_in:.2},{r_in:.2} 0 {large} 0 {} Z",
        pt(polar(r_out, a0)),
        pt(polar(r_out, a1)),
        pt(polar(r_in, a1)),
        pt(polar(r_in, a0)),
    )
}

/// An arc of radius `r` from `a0` to `a1`, for the SOSA band along a
/// segment's inner edge.
fn arc_path(r: f64, a0: f64, a1: f64) -> String {
    let large = u8::from(a1 - a0 > 180.0);
    format!(
        "M{} A{r:.2},{r:.2} 0 {large} 1 {}",
        pt(polar(r, a0)),
        pt(polar(r, a1))
    )
}

/// The root's disc: a whole circle for the wheel, the upper half for the fan.
fn root_path(arc: ChartArc) -> String {
    let r = arc.root_radius;
    if arc.full_circle {
        format!(
            "M{r:.2},0 A{r:.2},{r:.2} 0 1 0 {:.2},0 A{r:.2},{r:.2} 0 1 0 {r:.2},0 Z",
            -r
        )
    } else {
        format!("M{:.2},0 A{r:.2},{r:.2} 0 0 1 {r:.2},0 Z", -r)
    }
}

/// What a line of a label is, which sets its type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LineRole {
    Given,
    Surname,
    /// Surname and given names on one line, when a segment holds only one
    /// or two lines.
    Name,
    Dates,
}

/// One line of a label, already fitted to the room it has.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct LabelLine {
    pub(super) text: String,
    pub(super) role: LineRole,
    /// Baseline, in the label's own rotated frame.
    pub(super) y: f64,
    pub(super) font_px: f32,
    /// Width to compress a lifespan into when even its narrow form
    /// overruns: dropping characters off a date would change what it says.
    pub(super) squeeze: Option<f32>,
}

/// A segment's text: where it is anchored, how it is turned, what it says.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct SegmentLabel {
    pub(super) x: f64,
    pub(super) y: f64,
    pub(super) rotate: f64,
    pub(super) lines: Vec<LabelLine>,
}

/// The name pieces a label is made of, as a card writes them.
struct LabelText<'a> {
    given: &'a str,
    surname: String,
    birth: Option<QualifiedYear>,
    death: Option<QualifiedYear>,
}

impl<'a> LabelText<'a> {
    fn of(node: &'a LayoutNode) -> Self {
        Self {
            given: node.label_given.split(',').next().unwrap_or("").trim(),
            surname: node
                .label_surname
                .split(',')
                .next()
                .unwrap_or("")
                .trim()
                .to_uppercase(),
            birth: node.birth_year,
            death: node.death_year,
        }
    }

    /// The lines to write when `room` of them fit, before fitting widths.
    fn lines(&self, room: usize) -> Vec<(LineRole, String)> {
        let name = format!("{} {}", self.surname, self.given)
            .trim()
            .to_string();
        let dates = format_lifespan(self.birth, self.death);
        let planned = match room {
            0 => Vec::new(),
            1 => vec![(LineRole::Name, name)],
            2 => vec![(LineRole::Name, name), (LineRole::Dates, dates)],
            _ => vec![
                (LineRole::Given, self.given.to_string()),
                (LineRole::Surname, self.surname.clone()),
                (LineRole::Dates, dates),
            ],
        };
        planned
            .into_iter()
            .filter(|(_, text)| !text.is_empty())
            .collect()
    }
}

/// Type size of each line, a point smaller in radial rings, whose lines
/// stack across a narrow segment.
fn font_px(role: LineRole, flow: LabelFlow) -> f32 {
    let base = match role {
        LineRole::Surname => 12.0,
        LineRole::Given | LineRole::Name => 11.0,
        LineRole::Dates => 10.0,
    };
    match flow {
        LabelFlow::Tangential => base,
        LabelFlow::Radial => base - 1.0,
    }
}

/// Fits one planned line into `max_width`: names are truncated with an
/// ellipsis, a lifespan drops to its narrow form and is compressed if even
/// that overruns.
fn fit_line(
    role: LineRole,
    text: &str,
    text_of: &LabelText,
    max_width: f32,
    font: f32,
) -> (String, Option<f32>) {
    if role != LineRole::Dates {
        return (truncate_text_to_fit(text, max_width, font), None);
    }
    let dates = fit_lifespan(text_of.birth, text_of.death, max_width, font);
    let squeeze =
        (crate::utils::estimate_text_width_px(&dates, font) > max_width).then_some(max_width);
    (dates, squeeze)
}

/// Lays `planned` lines out one `step` apart, centred on the anchor, each
/// fitted to the width `width_at(offset)` gives for its offset.
fn stack_lines(
    planned: Vec<(LineRole, String)>,
    text_of: &LabelText,
    flow: LabelFlow,
    step: f64,
    width_at: impl Fn(f64) -> f64,
) -> Vec<LabelLine> {
    let count = planned.len();
    planned
        .into_iter()
        .enumerate()
        .filter_map(|(k, (role, text))| {
            let offset = (k as f64 - (count as f64 - 1.0) / 2.0) * step;
            let font = font_px(role, flow);
            let width = width_at(offset).max(0.0) as f32;
            let (text, squeeze) = fit_line(role, &text, text_of, width, font);
            (!text.is_empty()).then(|| LabelLine {
                text,
                role,
                y: offset + f64::from(font) * 0.35,
                font_px: font,
                squeeze,
            })
        })
        .collect()
}

/// Whether text turned to face `deg` reads upright, rather than upside down.
fn upright(deg: f64) -> bool {
    deg.to_radians().cos() >= -1e-9
}

/// The half-length a straight line of text may have at `distance` from the
/// centre, square to the radius, inside a ring of outer radius `r_out` and
/// between two radii `span` degrees apart.
fn tangential_half_width(distance: f64, r_out: f64, span: f64) -> f64 {
    // The glyphs' far side reaches further out than the line's centre.
    let reach = (r_out * r_out - (distance + 6.0).powi(2)).max(0.0).sqrt();
    let wedge = if span >= 180.0 {
        f64::INFINITY
    } else {
        (distance - 6.0).max(0.0) * (span.to_radians() / 2.0).tan()
    };
    reach.min(wedge) - LABEL_PAD
}

/// A label written across its segment, square to the radius through the
/// segment's middle, and turned over on the lower half so it reads upright.
fn tangential_label(node: &LayoutNode, ring: Ring, a0: f64, a1: f64) -> SegmentLabel {
    let mid = (a0 + a1) / 2.0;
    let span = a1 - a0;
    let r_mid = (ring.r_in + ring.r_out) / 2.0;
    let is_upright = upright(mid);
    // In the label's frame +y points to the centre when upright, away from it
    // when turned over.
    let inward = if is_upright { 1.0 } else { -1.0 };
    let room = ((ring.r_out - ring.r_in) / TANGENTIAL_LINE).floor() as usize;
    let text = LabelText::of(node);
    let lines = stack_lines(
        text.lines(room.min(3)),
        &text,
        LabelFlow::Tangential,
        TANGENTIAL_LINE,
        |offset| 2.0 * tangential_half_width(r_mid - offset * inward, ring.r_out, span),
    );
    let (x, y) = polar(r_mid, mid);
    SegmentLabel {
        x,
        y,
        rotate: if is_upright { mid } else { mid + 180.0 },
        lines,
    }
}

/// A label written along the radius through the segment's middle: outwards
/// on the right half, inwards on the left, so every name reads left to right.
fn radial_label(node: &LayoutNode, ring: Ring, a0: f64, a1: f64) -> SegmentLabel {
    let mid = (a0 + a1) / 2.0;
    let r_mid = (ring.r_in + ring.r_out) / 2.0;
    let across = ring.r_in * (a1 - a0).to_radians();
    let room = ((across - 2.0) / RADIAL_LINE).floor().max(1.0) as usize;
    let length = ring.r_out - ring.r_in - 2.0 * LABEL_PAD;
    let text = LabelText::of(node);
    let lines = stack_lines(
        text.lines(room.min(3)),
        &text,
        LabelFlow::Radial,
        RADIAL_LINE,
        |_| length,
    );
    let (x, y) = polar(r_mid, mid);
    let right_half = mid.to_radians().sin() >= -1e-9;
    SegmentLabel {
        x,
        y,
        rotate: if right_half { mid - 90.0 } else { mid + 90.0 },
        lines,
    }
}

/// The root's label, level and centred in its disc (or half disc).
fn root_label(node: &LayoutNode, arc: ChartArc) -> SegmentLabel {
    let r = arc.root_radius;
    let cy = if arc.full_circle { 0.0 } else { -r * 0.42 };
    let text = LabelText::of(node);
    let lines = stack_lines(
        text.lines(3),
        &text,
        LabelFlow::Tangential,
        TANGENTIAL_LINE,
        |offset| {
            let from_centre = (cy + offset).abs() + 7.0;
            2.0 * ((r * r - from_centre * from_centre).max(0.0).sqrt() - LABEL_PAD)
        },
    );
    SegmentLabel {
        x: 0.0,
        y: cy,
        rotate: 0.0,
        lines,
    }
}

/// A segment of one ancestor, or of a parent's empty slot.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Segment {
    /// Index into [`CircularLayout::entries`].
    pub(super) entry: usize,
    pub(super) start: f64,
    pub(super) end: f64,
    pub(super) ring: Ring,
    pub(super) path: String,
    /// The line along the inner edge that carries the SOSA or self mark.
    pub(super) band: String,
    pub(super) label: SegmentLabel,
    /// Where an empty slot's "+" goes.
    pub(super) centroid: (f64, f64),
}

/// A wheel or a fan, laid out.
pub(super) struct CircularLayout {
    pub(super) entries: Vec<AncestorEntry>,
    pub(super) segments: Vec<Segment>,
    pub(super) root_path: String,
    pub(super) root_label: Option<SegmentLabel>,
    /// Whether the chart's root is the tree's SOSA 1, which makes every
    /// position its SOSA number.
    pub(super) root_is_sosa_root: bool,
    /// Where the centre sits on the canvas.
    pub(super) origin_x: f64,
    pub(super) origin_y: f64,
    pub(super) total_w: f64,
    pub(super) total_h: f64,
}

impl CircularLayout {
    /// The whole canvas, margin included: a fit that framed the rim exactly
    /// would leave it touching the edges of a viewport as tall as the chart.
    pub(super) fn fit_target(&self) -> FitTarget {
        let (root_dx, root_dy) = self.root_label.as_ref().map_or((0.0, 0.0), |l| (l.x, l.y));
        FitTarget {
            content_cx: self.total_w / 2.0,
            content_cy: self.total_h / 2.0,
            content_w: self.total_w,
            content_h: self.total_h,
            root_cx: self.origin_x + root_dx,
            root_cy: self.origin_y + root_dy,
        }
    }
}

/// Builds one segment from its SOSA position.
fn segment(index: usize, entry: &AncestorEntry, arc: ChartArc, rings: &[Ring]) -> Segment {
    let ring = rings[generation_of(entry.sosa) as usize - 1];
    let (start, end) = segment_angles(entry.sosa, arc);
    let label = match ring.flow {
        LabelFlow::Tangential => tangential_label(&entry.node, ring, start, end),
        LabelFlow::Radial => radial_label(&entry.node, ring, start, end),
    };
    Segment {
        entry: index,
        start,
        end,
        ring,
        path: sector_path(ring.r_in, ring.r_out, start, end),
        band: arc_path(ring.r_in + 2.0, start, end),
        label,
        centroid: polar((ring.r_in + ring.r_out) / 2.0, (start + end) / 2.0),
    }
}

/// Lays out the wheel or the fan of `root_id`'s ancestors, `generations`
/// generations back.
pub(super) fn circular_layout(
    arc: ChartArc,
    root_id: Uuid,
    data: &PedigreeData,
    generations: usize,
    sosa_root_id: Option<Uuid>,
    sosa_ancestors: &HashSet<Uuid>,
) -> CircularLayout {
    let entries = collect_ancestors(root_id, data, generations, sosa_root_id, sosa_ancestors);
    let rings = ring_radii(arc, generations as u32);
    let segments = entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry.sosa > 1)
        .map(|(i, entry)| segment(i, entry, arc, &rings))
        .collect();
    let root_label = entries
        .first()
        .filter(|entry| entry.sosa == 1)
        .map(|entry| root_label(&entry.node, arc));
    let radius = rings.last().map_or(arc.root_radius, |ring| ring.r_out);
    let total_w = 2.0 * (radius + CHART_MARGIN);
    let (total_h, origin_y) = if arc.full_circle {
        (total_w, radius + CHART_MARGIN)
    } else {
        (radius + 2.0 * CHART_MARGIN, radius + CHART_MARGIN)
    };
    CircularLayout {
        entries,
        segments,
        root_path: root_path(arc),
        root_label,
        root_is_sosa_root: sosa_root_id.is_some() && sosa_root_id == Some(root_id),
        origin_x: radius + CHART_MARGIN,
        origin_y,
        total_w,
        total_h,
    }
}

/// A computed circular layout, shared with the canvas that draws it.
/// Equality is identity, as for [`SharedLayout`].
#[derive(Clone)]
pub(super) struct SharedCircular(pub(super) Rc<CircularLayout>);

impl PartialEq for SharedCircular {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl std::ops::Deref for SharedCircular {
    type Target = CircularLayout;

    fn deref(&self) -> &CircularLayout {
        &self.0
    }
}

/// The class that tints a segment by its person's sex.
fn sex_class(sex: Sex) -> &'static str {
    match sex {
        Sex::Male => "fan-seg fan-seg-male",
        Sex::Female => "fan-seg fan-seg-female",
        Sex::Unknown => "fan-seg",
    }
}

/// The text of a label, turned into place.
fn render_label(label: &SegmentLabel, theme: &PedigreeTheme, fill: &str, muted: &str) -> Element {
    let card = &theme.card;
    rsx! {
        g {
            class: "fan-seg-label",
            transform: "translate({label.x:.2},{label.y:.2}) rotate({label.rotate:.2})",
            for (k, line) in label.lines.iter().enumerate() {
                {
                    let (family, weight, colour) = match line.role {
                        LineRole::Surname => (card.surname_font, card.surname_weight, fill),
                        LineRole::Name => (card.body_font, "600", fill),
                        LineRole::Given => (card.body_font, "400", fill),
                        LineRole::Dates => (card.body_font, "400", muted),
                    };
                    rsx! {
                        text {
                            key: "{k}",
                            x: "0",
                            y: "{line.y:.2}",
                            style: "font-size:{line.font_px}px;font-family:{family};font-weight:{weight};fill:{colour};text-anchor:middle",
                            "textLength": line.squeeze.map(|w| w.to_string()),
                            "lengthAdjust": line.squeeze.map(|_| "spacingAndGlyphs"),
                            "{line.text}"
                        }
                    }
                }
            }
        }
    }
}

/// What a segment answers to, as a card does.
#[derive(Clone, Copy)]
struct SegmentActions {
    selected_person_id: Signal<Uuid>,
    on_person_navigate: EventHandler<Uuid>,
    on_person_click: EventHandler<(Uuid, f64, f64)>,
    on_empty_slot: EventHandler<(Uuid, bool)>,
}

/// One ancestor's segment: clicking it re-roots the chart on them, a right
/// click opens the action picker.
fn render_person_segment(
    segment: &Segment,
    entry: &AncestorEntry,
    pid: Uuid,
    title: String,
    theme: &PedigreeTheme,
    actions: SegmentActions,
) -> Element {
    let SegmentActions {
        mut selected_person_id,
        on_person_navigate,
        on_person_click,
        ..
    } = actions;
    let band = mark_colour(&entry.node);
    rsx! {
        g {
            key: "fs-{entry.sosa}",
            class: sex_class(entry.node.sex),
            onclick: move |_| { selected_person_id.set(pid); on_person_navigate.call(pid); },
            oncontextmenu: move |evt: Event<MouseData>| {
                evt.prevent_default();
                evt.stop_propagation();
                selected_person_id.set(pid);
                let coords = evt.client_coordinates();
                on_person_click.call((pid, coords.x, coords.y));
            },
            path { class: "fan-seg-shape", d: "{segment.path}", dangerous_inner_html: "{title}" }
            if let Some(colour) = band {
                path { class: "fan-seg-band", d: "{segment.band}", style: "stroke:{colour}" }
            }
            {render_label(&segment.label, theme, "var(--pn-text)", "var(--pn-text-muted)")}
        }
    }
}

/// A missing parent's segment: dashed, with a "+" that adds them.
fn render_empty_segment(
    segment: &Segment,
    entry: &AncestorEntry,
    title: String,
    actions: SegmentActions,
) -> Element {
    let on_empty_slot = actions.on_empty_slot;
    let is_father = entry.node.is_father;
    let child = entry.node.child_of;
    let (cx, cy) = segment.centroid;
    // A "+" only where the segment is wide enough to show one; the whole
    // segment stays clickable either way.
    let show_plus = segment.ring.r_in * (segment.end - segment.start).to_radians() >= 18.0;
    rsx! {
        g {
            key: "fs-{entry.sosa}",
            class: "fan-slot",
            onclick: move |_| {
                if let Some(child) = child {
                    on_empty_slot.call((child, is_father));
                }
            },
            path { class: "fan-slot-shape", d: "{segment.path}", dangerous_inner_html: "{title}" }
            if show_plus {
                text { class: "fan-slot-plus", x: "{cx:.2}", y: "{cy + 6.0:.2}", "+" }
            }
        }
    }
}

/// The root's disc. Clicking it opens the action picker: the chart is
/// already rooted on this person, so there is nowhere to navigate to.
fn render_root(
    layout: &CircularLayout,
    theme: &PedigreeTheme,
    title: String,
    actions: SegmentActions,
) -> Element {
    let Some(entry) = layout.entries.first().filter(|entry| entry.sosa == 1) else {
        return rsx! {};
    };
    let Some(pid) = entry.node.id else {
        return rsx! {};
    };
    let SegmentActions {
        mut selected_person_id,
        on_person_click,
        ..
    } = actions;
    let pick = move |evt: Event<MouseData>| {
        evt.prevent_default();
        evt.stop_propagation();
        selected_person_id.set(pid);
        let coords = evt.client_coordinates();
        on_person_click.call((pid, coords.x, coords.y));
    };
    let band = mark_colour(&entry.node);
    rsx! {
        g {
            class: "fan-seg fan-root",
            onclick: pick,
            oncontextmenu: pick,
            path { class: "fan-seg-shape", d: "{layout.root_path}", dangerous_inner_html: "{title}" }
            if let Some(colour) = band {
                path { class: "fan-root-ring", d: "{layout.root_path}", style: "stroke:{colour}" }
            }
            if let Some(label) = &layout.root_label {
                {render_label(label, theme, "var(--white)", "var(--white)")}
            }
        }
    }
}

/// The wheel or the fan, drawn.
///
/// A component of its own so that it redraws only when the layout does, as
/// the tree view's canvas. It draws every segment: even ten generations are
/// 2,046 of them, and the chart is compact enough that most are on screen
/// at a fitting zoom anyway.
#[component]
pub(super) fn CircularCanvas(
    layout: SharedCircular,
    selected_person_id: Signal<Uuid>,
    on_person_navigate: EventHandler<Uuid>,
    on_person_click: EventHandler<(Uuid, f64, f64)>,
    on_empty_slot: EventHandler<(Uuid, bool)>,
    theme: &'static PedigreeTheme,
) -> Element {
    let i18n = use_i18n();
    let actions = SegmentActions {
        selected_person_id,
        on_person_navigate,
        on_person_click,
        on_empty_slot,
    };
    let add_parent = svg_title(&i18n.t("linking.add_parent"));
    let root_title = layout
        .entries
        .first()
        .map(|entry| svg_title(&ancestor_tooltip(entry, layout.root_is_sosa_root, &i18n)))
        .unwrap_or_default();

    rsx! {
        div {
            class: "pedigree-tree",
            style: "position: relative; width: {layout.total_w}px; height: {layout.total_h}px;",
            svg {
                class: "fan-chart",
                "viewBox": "0 0 {layout.total_w} {layout.total_h}",
                width: "{layout.total_w}",
                height: "{layout.total_h}",
                style: "display: block; overflow: visible;",
                g { transform: "translate({layout.origin_x},{layout.origin_y})",
                    for segment in layout.segments.iter() {
                        {
                            let entry = &layout.entries[segment.entry];
                            match entry.node.id {
                                Some(pid) => render_person_segment(
                                    segment,
                                    entry,
                                    pid,
                                    svg_title(&ancestor_tooltip(entry, layout.root_is_sosa_root, &i18n)),
                                    theme,
                                    actions,
                                ),
                                None => render_empty_segment(segment, entry, add_parent.clone(), actions),
                            }
                        }
                    }
                    {render_root(&layout, theme, root_title, actions)}
                }
            }
        }
    }
}

/// A three-generation miniature of `arc`, from the chart's own rings and
/// segments, for the view picker in the settings: segments tinted father's
/// side, mother's side, the root in its accent. No names.
pub(super) fn swatch(arc: ChartArc) -> Element {
    const GENERATIONS: u32 = 3;
    let rings = ring_radii(arc, GENERATIONS);
    let radius = rings.last().map_or(arc.root_radius, |ring| ring.r_out) + 6.0;
    let (y0, height) = if arc.full_circle {
        (-radius, 2.0 * radius)
    } else {
        (-radius, radius + 6.0)
    };
    let segments: Vec<(u64, String)> = (2u64..(1 << (GENERATIONS + 1)))
        .map(|sosa| {
            let ring = rings[generation_of(sosa) as usize - 1];
            let (a0, a1) = segment_angles(sosa, arc);
            (sosa, sector_path(ring.r_in, ring.r_out, a0, a1))
        })
        .collect();
    let root = root_path(arc);
    rsx! {
        svg {
            class: "ped-theme-swatch ped-view-swatch",
            "viewBox": "{-radius} {y0} {2.0 * radius} {height}",
            "preserveAspectRatio": "xMidYMid meet",
            "aria-hidden": "true",
            rect {
                x: "{-radius}", y: "{y0}", width: "{2.0 * radius}", height: "{height}",
                style: "fill:var(--pn-swatch-bg,transparent)",
            }
            for (sosa, d) in segments {
                g {
                    key: "{sosa}",
                    class: if sosa % 2 == 0 { "fan-seg-male" } else { "fan-seg-female" },
                    path { class: "fan-seg-shape", d: "{d}" }
                }
            }
            g { class: "fan-root",
                path { class: "fan-seg-shape", d: "{root}" }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::pedigree_chart::geometry_golden_tests::{Fixture, id};

    const EPS: f64 = 1e-9;

    #[test]
    fn the_wheel_gives_the_father_the_left_half_and_the_mother_the_right() {
        assert_eq!(segment_angles(2, ChartArc::WHEEL), (180.0, 360.0));
        assert_eq!(segment_angles(3, ChartArc::WHEEL), (360.0, 540.0));
        // Grandparents: father's father low left, mother's mother low right.
        assert_eq!(segment_angles(4, ChartArc::WHEEL), (180.0, 270.0));
        assert_eq!(segment_angles(5, ChartArc::WHEEL), (270.0, 360.0));
        assert_eq!(segment_angles(6, ChartArc::WHEEL), (360.0, 450.0));
        assert_eq!(segment_angles(7, ChartArc::WHEEL), (450.0, 540.0));
    }

    #[test]
    fn the_fan_spreads_the_same_order_over_the_upper_half() {
        assert_eq!(segment_angles(2, ChartArc::FAN), (270.0, 360.0));
        assert_eq!(segment_angles(3, ChartArc::FAN), (360.0, 450.0));
        assert_eq!(segment_angles(8, ChartArc::FAN), (270.0, 292.5));
        assert_eq!(segment_angles(15, ChartArc::FAN), (427.5, 450.0));
    }

    /// Every generation's segments tile the arc: no gap, no overlap, and
    /// each one inside its child's, which is what keeps a line together.
    #[test]
    fn each_generation_tiles_the_arc_inside_its_children() {
        for arc in [ChartArc::WHEEL, ChartArc::FAN] {
            for generation in 1..=10u32 {
                let first = 1u64 << generation;
                let mut expected_start = arc.start;
                for sosa in first..2 * first {
                    let (a0, a1) = segment_angles(sosa, arc);
                    assert!((a0 - expected_start).abs() < EPS, "{sosa}: gap before it");
                    assert!((a1 - a0 - arc.sweep / first as f64).abs() < EPS);
                    expected_start = a1;
                    if sosa > 1 && generation > 1 {
                        let (c0, c1) = segment_angles(sosa / 2, arc);
                        assert!(c0 - EPS <= a0 && a1 <= c1 + EPS, "{sosa} outside its child");
                    }
                }
                assert!((expected_start - arc.start - arc.sweep).abs() < EPS);
            }
        }
    }

    #[test]
    fn rings_touch_and_stay_wide_enough_ten_generations_out() {
        for arc in [ChartArc::WHEEL, ChartArc::FAN] {
            let rings = ring_radii(arc, 10);
            assert_eq!(rings.len(), 10);
            assert!((rings[0].r_in - arc.root_radius).abs() < EPS);
            for (g, pair) in rings.windows(2).enumerate() {
                assert!(
                    (pair[0].r_out - pair[1].r_in).abs() < EPS,
                    "gap after ring {}",
                    g + 1
                );
            }
            for (g, ring) in rings.iter().enumerate() {
                let span = arc.sweep / f64::from(1u32 << (g + 1));
                assert!(
                    ring.r_in * span.to_radians() >= MIN_SEGMENT_ARC - EPS,
                    "{arc:?}: generation {} is {:.1}px wide",
                    g + 1,
                    ring.r_in * span.to_radians()
                );
            }
            // Names turn to follow the radius once, and never turn back.
            let first_radial = rings
                .iter()
                .position(|r| r.flow == LabelFlow::Radial)
                .expect("some radial ring");
            assert!(first_radial >= 1, "{arc:?}: parents are written across");
            assert!(
                rings[first_radial..]
                    .iter()
                    .all(|r| r.flow == LabelFlow::Radial)
            );
        }
    }

    #[test]
    fn a_sector_path_starts_on_the_outer_rim_and_closes_on_the_inner() {
        let d = sector_path(10.0, 20.0, 0.0, 90.0);
        assert_eq!(
            d,
            "M0.00,-20.00 A20.00,20.00 0 0 1 20.00,-0.00 L10.00,-0.00 A10.00,10.00 0 0 0 0.00,-10.00 Z"
        );
    }

    fn named(given: &str, surname: &str) -> LayoutNode {
        LayoutNode {
            id: Some(id(1)),
            x: 0.0,
            y: 0.0,
            sex: Sex::Male,
            label_surname: surname.to_string(),
            label_given: given.to_string(),
            birth_year: None,
            death_year: None,
            photo_url: None,
            sosa_badge: SosaBadge::None,
            is_self: false,
            is_compact: false,
            child_of: None,
            is_father: false,
            is_sibling: false,
            has_more_relations: false,
        }
    }

    /// No line of any label, in any generation, overruns what its segment
    /// has room for — checked against the width estimator the fitting uses.
    #[test]
    fn labels_fit_their_segments_eight_generations_out() {
        let long = named(
            "Marie-Madeleine-Antoinette",
            "Branch_with_a_very_long_surname",
        );
        for arc in [ChartArc::WHEEL, ChartArc::FAN] {
            let rings = ring_radii(arc, 8);
            for sosa in [2u64, 3, 5, 12, 27, 50, 101, 200, 255, 256, 511] {
                let ring = rings[generation_of(sosa) as usize - 1];
                let (a0, a1) = segment_angles(sosa, arc);
                let label = match ring.flow {
                    LabelFlow::Tangential => tangential_label(&long, ring, a0, a1),
                    LabelFlow::Radial => radial_label(&long, ring, a0, a1),
                };
                assert!(!label.lines.is_empty(), "{arc:?} {sosa}: nothing written");
                let room = match ring.flow {
                    LabelFlow::Tangential => ring.r_out - ring.r_in,
                    LabelFlow::Radial => ring.r_in * (a1 - a0).to_radians(),
                };
                let step = match ring.flow {
                    LabelFlow::Tangential => TANGENTIAL_LINE,
                    LabelFlow::Radial => RADIAL_LINE,
                };
                assert!(
                    label.lines.len() as f64 * step <= room + 2.0,
                    "{arc:?} {sosa}: lines overflow across"
                );
                for line in &label.lines {
                    let width = crate::utils::estimate_text_width_px(&line.text, line.font_px);
                    let limit = match ring.flow {
                        LabelFlow::Tangential => ring.r_out * 2.0,
                        LabelFlow::Radial => ring.r_out - ring.r_in,
                    };
                    assert!(
                        f64::from(width) <= limit,
                        "{arc:?} {sosa}: {:?} too long",
                        line.text
                    );
                    if ring.flow == LabelFlow::Radial {
                        assert!(f64::from(width) <= ring.r_out - ring.r_in - 2.0 * LABEL_PAD + 0.5);
                    }
                }
            }
        }
    }

    #[test]
    fn no_label_is_upside_down() {
        let node = named("Given_1", "Branch_A");
        for arc in [ChartArc::WHEEL, ChartArc::FAN] {
            let rings = ring_radii(arc, 6);
            for sosa in 2u64..128 {
                let ring = rings[generation_of(sosa) as usize - 1];
                let (a0, a1) = segment_angles(sosa, arc);
                let label = match ring.flow {
                    LabelFlow::Tangential => tangential_label(&node, ring, a0, a1),
                    LabelFlow::Radial => radial_label(&node, ring, a0, a1),
                };
                // Text runs along the rotated x axis; upright means that axis
                // never points left of straight down.
                let dx = label.rotate.to_radians().cos();
                assert!(dx >= -1e-9, "{arc:?} {sosa}: rotated {}", label.rotate);
            }
        }
    }

    #[test]
    fn a_narrow_segment_keeps_the_name_and_drops_lines_it_cannot_hold() {
        let node = named("Given_1", "Branch_A");
        let rings = ring_radii(ChartArc::FAN, 8);
        let sosa = 256; // first of the eighth generation
        let ring = rings[7];
        let (a0, a1) = segment_angles(sosa, ChartArc::FAN);
        let label = radial_label(&node, ring, a0, a1);
        assert_eq!(label.lines.len(), 1);
        assert_eq!(label.lines[0].role, LineRole::Name);
        assert!(label.lines[0].text.starts_with("BRANCH_A"));
    }

    #[test]
    fn a_lifespan_keeps_its_precision_marks() {
        let mut node = named("Given_1", "Branch_A");
        node.birth_year = Some(QualifiedYear {
            year: 1849,
            qualifier: DateQualifier::About,
            year2: None,
        });
        node.death_year = Some(QualifiedYear {
            year: 1917,
            qualifier: DateQualifier::Before,
            year2: None,
        });
        let rings = ring_radii(ChartArc::WHEEL, 3);
        let (a0, a1) = segment_angles(2, ChartArc::WHEEL);
        let label = tangential_label(&node, rings[0], a0, a1);
        let dates = label
            .lines
            .iter()
            .find(|l| l.role == LineRole::Dates)
            .expect("dates line");
        assert_eq!(dates.text, "ca 1849-< 1917");
    }

    /// A full ancestry, `generations` deep: person `n` is SOSA `n`.
    fn full_ancestry(generations: u32) -> PedigreeData {
        let mut f = Fixture::default();
        let last = 1u128 << (generations + 1);
        for n in 1..last {
            let sex = if n % 2 == 0 { Sex::Male } else { Sex::Female };
            f.person(n, sex, &format!("Given_{n}"), &format!("Branch_{n}"));
        }
        for n in 1..(1u128 << generations) {
            f.family(1_000_000 + n, &[2 * n, 2 * n + 1], &[n]);
        }
        f.build()
    }

    #[test]
    fn every_ancestor_lands_on_the_segment_of_its_sosa_number() {
        let data = full_ancestry(6);
        let layout = circular_layout(ChartArc::WHEEL, id(1), &data, 6, None, &HashSet::new());
        assert_eq!(layout.entries.len(), 127);
        assert_eq!(layout.segments.len(), 126);
        for segment in &layout.segments {
            let entry = &layout.entries[segment.entry];
            assert_eq!(entry.node.id, Some(id(u128::from(entry.sosa))));
            assert_eq!(
                (segment.start, segment.end),
                segment_angles(entry.sosa, ChartArc::WHEEL)
            );
        }
        // The chart is square around its centre.
        assert!((layout.total_w - layout.total_h).abs() < EPS);
        assert!((layout.origin_x - layout.total_w / 2.0).abs() < EPS);
    }

    #[test]
    fn a_missing_parent_gets_one_empty_slot_and_nothing_above_it() {
        let mut f = Fixture::default();
        f.person(1, Sex::Male, "Root", "Branch_A")
            .person(3, Sex::Female, "Mother_1", "Branch_B");
        f.family(100, &[3], &[1]);
        let data = f.build();
        let layout = circular_layout(ChartArc::FAN, id(1), &data, 4, None, &HashSet::new());
        let sosas: Vec<(u64, bool)> = layout
            .entries
            .iter()
            .map(|e| (e.sosa, e.node.id.is_some()))
            .collect();
        // Root, the empty father, the mother, then the mother's two empty parents.
        assert_eq!(
            sosas,
            vec![(1, true), (2, false), (3, true), (6, false), (7, false)]
        );
        let father = &layout.entries[1].node;
        assert_eq!((father.child_of, father.is_father), (Some(id(1)), true));
        // The fan sits on its base: nothing is drawn below the root.
        assert!((layout.origin_y - layout.total_h + CHART_MARGIN).abs() < EPS);
    }

    #[test]
    fn the_depth_limits_the_rings() {
        let data = full_ancestry(6);
        let layout = circular_layout(ChartArc::WHEEL, id(1), &data, 3, None, &HashSet::new());
        assert_eq!(layout.entries.iter().map(|e| e.sosa).max(), Some(15));
        let none = circular_layout(ChartArc::WHEEL, id(1), &data, 0, None, &HashSet::new());
        assert_eq!(none.entries.len(), 1);
        assert!(none.segments.is_empty());
        assert!(none.root_label.is_some());
    }
}
