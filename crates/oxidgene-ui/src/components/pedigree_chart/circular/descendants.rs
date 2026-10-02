//! The descendant wheel and fan.
//!
//! The root at the centre, its children on the first ring, their children on
//! the next, and so on, `generations` deep. Between two generations a thin
//! ring holds the unions: one segment per couple, spanning the children it
//! had and naming the spouse, as Gramps' descendant fan does — so the
//! children of each union stay together and it is plain whose they are.
//!
//! A person's arc is shared among their unions, and a union's among its
//! children, in proportion to how many descendants each holds within the
//! depth drawn (a person or a childless union counts one). Equal shares
//! would starve a large family and leave an empty one wide.

use super::*;
use crate::components::pedigree_chart::descent::{Tree, union_weight, unknown_spouse};

/// Radial depth of a ring of unions: one line of a spouse's name.
const UNION_RING: f64 = 20.0;
/// Type size of a spouse's name, before any reduction.
const UNION_FONT: f32 = 9.0;

/// Where each person and each union sits on the arc.
struct Angles {
    people: Vec<(f64, f64)>,
    /// Per person, per union, parallel to [`Person::unions`].
    unions: Vec<Vec<(f64, f64)>>,
}

/// Shares `arc` out, from the root outwards, in proportion to the weights.
///
/// Siblings follow each other clockwise, the eldest first — except in the
/// fan opening downwards, where clockwise runs right to left: there the
/// order is turned round so that siblings read left to right, as in the
/// tree view.
fn angles(tree: &Tree, weights: &[f64], arc: ChartArc) -> Angles {
    let reversed = arc.disc == Disc::Lower;
    let mut people = vec![(arc.start, arc.start + arc.sweep); tree.people.len()];
    let mut unions = vec![Vec::new(); tree.people.len()];
    for (i, person) in tree.people.iter().enumerate() {
        let (a0, a1) = people[i];
        let spans: Vec<f64> = person
            .unions
            .iter()
            .map(|union| (a1 - a0) * union_weight(union, weights) / weights[i])
            .collect();
        let mut placed = vec![(0.0, 0.0); spans.len()];
        let mut cursor = a0;
        for k in in_order(spans.len(), reversed) {
            placed[k] = (cursor, cursor + spans[k]);
            cursor += spans[k];
            let union = &person.unions[k];
            let total: f64 = union.children.iter().map(|&c| weights[c]).sum();
            let mut child_cursor = placed[k].0;
            for c in in_order(union.children.len(), reversed) {
                let child = union.children[c];
                let child_span = spans[k] * weights[child] / total;
                people[child] = (child_cursor, child_cursor + child_span);
                child_cursor += child_span;
            }
        }
        unions[i] = placed;
    }
    Angles { people, unions }
}

/// `0..len`, backwards when `reversed`.
fn in_order(len: usize, reversed: bool) -> Box<dyn Iterator<Item = usize>> {
    if reversed {
        Box::new((0..len).rev())
    } else {
        Box::new(0..len)
    }
}

/// The rings of a descendant chart, innermost first: for each generation
/// with anyone in it, the ring of its parents' unions and then its own.
struct DescendantRings {
    /// Per depth from 0: the ring of that depth's unions.
    unions: Vec<Ring>,
    /// Per depth from 1: the ring of that depth's people (index `depth - 1`).
    people: Vec<Ring>,
}

/// Lays the rings out. A generation's ring is as deep as its narrowest
/// person needs: written across if every one of them has room for a straight
/// line, along the radius otherwise.
fn descendant_rings(tree: &Tree, angles: &Angles, arc: ChartArc) -> DescendantRings {
    let deepest = tree.people.iter().map(|p| p.depth).max().unwrap_or(0);
    let mut rings = DescendantRings {
        unions: Vec::new(),
        people: Vec::new(),
    };
    let mut r = arc.root_radius;
    for depth in 1..=deepest {
        rings.unions.push(Ring {
            r_in: r,
            r_out: r + UNION_RING,
            flow: LabelFlow::Tangential,
        });
        r += UNION_RING;
        let all_across = tree
            .people
            .iter()
            .zip(&angles.people)
            .filter(|(person, _)| person.depth == depth)
            .all(|(_, (a0, a1))| chord(r, a1 - a0) >= MIN_TANGENTIAL_CHORD);
        let (flow, ring_depth) = if all_across {
            (LabelFlow::Tangential, TANGENTIAL_RING)
        } else {
            (LabelFlow::Radial, RADIAL_RING)
        };
        rings.people.push(Ring {
            r_in: r,
            r_out: r + ring_depth,
            flow,
        });
        r += ring_depth;
    }
    // A union of the outermost generation has no children drawn — a
    // childless couple, or a root alone with its spouse — and still needs
    // its band, outside the last ring of persons.
    if tree
        .people
        .iter()
        .any(|person| person.depth == deepest && !person.unions.is_empty())
    {
        rings.unions.push(Ring {
            r_in: r,
            r_out: r + UNION_RING,
            flow: LabelFlow::Tangential,
        });
    }
    rings
}

/// A spouse's name across a union's band, square to the radius, turned over
/// on the lower half. Written smaller when the band is too short for it,
/// as a narrow segment's label is, for zooming in on.
fn union_label(node: &LayoutNode, ring: Ring, a0: f64, a1: f64) -> SegmentLabel {
    let mid = (a0 + a1) / 2.0;
    let r_mid = (ring.r_in + ring.r_out) / 2.0;
    let (x, y) = polar(r_mid, mid);
    let rotate = if upright(mid) { mid } else { mid + 180.0 };
    // Only the name is written across a union's band, never a lifespan.
    let text = LabelText::of(node, DateStyle::DEFAULT);
    let name = format!("{} {}", text.surname, text.given)
        .trim()
        .to_string();
    let available = 2.0 * tangential_half_width(r_mid, ring.r_out, a1 - a0);
    let width = f64::from(crate::utils::estimate_text_width_px(&name, UNION_FONT));
    if name.is_empty() || available <= 0.0 {
        return SegmentLabel {
            x,
            y,
            rotate,
            lines: Vec::new(),
            scale: 1.0,
        };
    }
    let scale = (available / width).min(1.0);
    let line = LabelLine {
        text: name,
        role: LineRole::Name,
        y: f64::from(UNION_FONT) * 0.35,
        font_px: UNION_FONT,
        squeeze: None,
    }
    .scaled(scale);
    SegmentLabel {
        x,
        y,
        rotate,
        lines: vec![line],
        scale,
    }
}

/// A person's segment: a label across if the segment has room for a
/// straight line, along the radius otherwise.
fn person_segment(
    entry: usize,
    node: &LayoutNode,
    ring: Ring,
    (a0, a1): (f64, f64),
    dates: DateStyle,
) -> Segment {
    let label = if chord(ring.r_in, a1 - a0) >= MIN_TANGENTIAL_CHORD {
        tangential_label(node, ring, (a0, a1), dates)
    } else {
        radial_label(node, ring, (a0, a1), dates)
    };
    Segment {
        entry,
        start: a0,
        end: a1,
        ring,
        path: sector_path(ring.r_in, ring.r_out, a0, a1),
        band: arc_path(ring.r_in + 2.0, a0, a1),
        label,
        centroid: polar((ring.r_in + ring.r_out) / 2.0, (a0 + a1) / 2.0),
        union: false,
        extent: sector_extent(ring.r_in, ring.r_out, a0, a1),
    }
}

/// A union's segment in its thin ring, the entry being the spouse.
fn union_segment(entry: usize, node: &LayoutNode, ring: Ring, (a0, a1): (f64, f64)) -> Segment {
    Segment {
        label: union_label(node, ring, a0, a1),
        union: true,
        band: String::new(),
        ..person_segment(entry, node, ring, (a0, a1), DateStyle::DEFAULT)
    }
}

/// Lays out the descendant wheel or fan of `root_id`, `generations` deep.
///
/// The entries are the root, then each person and spouse as placed, their
/// `sosa` only a running number that keys them: it means nothing in a
/// descendant chart. The SOSA and self marks are kept, so the line leading
/// to the tree's SOSA 1 stands out among the descendants.
pub(in crate::components::pedigree_chart) fn descendant_circular_layout(
    arc: ChartArc,
    root_id: Uuid,
    data: &PedigreeData,
    generations: usize,
    sosa_root_id: Option<Uuid>,
    sosa_ancestors: &HashSet<Uuid>,
    dates: DateStyle,
) -> CircularLayout {
    let tree = Tree::collect(root_id, data, generations as u32);
    let weights = tree.weights();
    let angles = angles(&tree, &weights, arc);
    let rings = descendant_rings(&tree, &angles, arc);
    let node_of = |id: Uuid| {
        PersonNode::from_data(id, data, sosa_root_id, sosa_ancestors).card_at(id, 0.0, 0.0)
    };

    let mut entries = Vec::new();
    let mut segments = Vec::new();
    let push_entry = |node: LayoutNode, entries: &mut Vec<AncestorEntry>| {
        entries.push(AncestorEntry {
            sosa: entries.len() as u64 + 1,
            node,
        });
        entries.len() - 1
    };
    for (i, person) in tree.people.iter().enumerate() {
        let node = node_of(person.id);
        let entry = push_entry(node.clone(), &mut entries);
        if person.depth > 0 {
            let ring = rings.people[person.depth as usize - 1];
            segments.push(person_segment(entry, &node, ring, angles.people[i], dates));
        }
        for (union, span) in person.unions.iter().zip(&angles.unions[i]) {
            let spouse = union.spouse.map_or_else(unknown_spouse, node_of);
            let entry = push_entry(spouse.clone(), &mut entries);
            let ring = rings.unions[person.depth as usize];
            segments.push(union_segment(entry, &spouse, ring, *span));
        }
    }

    let root_label = entries
        .first()
        .map(|entry| root_label(&entry.node, arc, dates));
    let radius = rings
        .people
        .last()
        .into_iter()
        .chain(rings.unions.last())
        .map(|ring| ring.r_out)
        .fold(arc.root_radius, f64::max);
    let max_zoom = max_zoom_for(&segments);
    CircularLayout {
        entries,
        segments,
        root_path: root_path(arc),
        root_label,
        max_zoom,
        ..CircularLayout::framed(arc, radius)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::pedigree_chart::geometry_golden_tests::{Fixture, id};

    const EPS: f64 = 1e-9;

    /// The root (1) with two unions: to 2, with children 10 and 11 — 10
    /// having two children (20, 21) with 3 — and to an unknown spouse, with
    /// child 12.
    fn family() -> PedigreeData {
        let mut f = Fixture::default();
        f.person(1, Sex::Male, "Root", "Branch_A")
            .person(2, Sex::Female, "Spouse_1", "Branch_B")
            .person(3, Sex::Female, "Spouse_2", "Branch_C")
            .person(10, Sex::Male, "Child_1", "Branch_A")
            .person(11, Sex::Female, "Child_2", "Branch_A")
            .person(12, Sex::Male, "Child_3", "Branch_A")
            .person(20, Sex::Male, "Grandchild_1", "Branch_A")
            .person(21, Sex::Female, "Grandchild_2", "Branch_A");
        f.family(100, &[1, 2], &[10, 11]);
        f.family(101, &[1], &[12]);
        f.family(102, &[10, 3], &[20, 21]);
        f.build()
    }

    fn person_span(layout: &CircularLayout, n: u128) -> (f64, f64) {
        let s = layout
            .segments
            .iter()
            .find(|s| !s.union && layout.entries[s.entry].node.id == Some(id(n)))
            .expect("segment");
        (s.start, s.end)
    }

    /// Each union spans its children, each person their share: a person's
    /// arc is proportional to the descendants it holds.
    #[test]
    fn arcs_are_shared_by_descendants_and_unions_span_their_children() {
        let data = family();
        let arc = ChartArc::DESCENDANT_WHEEL;
        let layout = descendant_circular_layout(
            arc,
            id(1),
            &data,
            2,
            None,
            &HashSet::new(),
            DateStyle::DEFAULT,
        );
        // Leaves: 20, 21 (under 10), 11, 12 — four.
        let quarter = arc.sweep / 4.0;
        let (a0, a1) = person_span(&layout, 10);
        assert!(
            (a1 - a0 - 2.0 * quarter).abs() < EPS,
            "Child_1 holds two leaves"
        );
        let (b0, b1) = person_span(&layout, 11);
        assert!((b1 - b0 - quarter).abs() < EPS);
        assert!((b0 - a1).abs() < EPS, "siblings side by side");
        let (c0, _) = person_span(&layout, 12);
        assert!((c0 - b1).abs() < EPS, "the second union's child follows");
        let unions: Vec<&Segment> = layout.segments.iter().filter(|s| s.union).collect();
        // The root's two unions and Child_1's.
        assert_eq!(unions.len(), 3);
        let first = unions[0];
        assert!((first.start - a0).abs() < EPS && (first.end - b1).abs() < EPS);
        assert_eq!(layout.entries[first.entry].node.id, Some(id(2)));
        assert_eq!(
            layout.entries[unions[1].entry].node.id, None,
            "unknown spouse"
        );
        // Grandchildren inside their parent's arc.
        let (g0, _) = person_span(&layout, 20);
        let (_, g1) = person_span(&layout, 21);
        assert!((g0 - a0).abs() < EPS && (g1 - a1).abs() < EPS);
    }

    /// Unions sit on a thin ring between the generations they join, and the
    /// depth limits what is drawn: no union beyond the last generation.
    #[test]
    fn unions_ring_between_generations_and_the_depth_limits_the_chart() {
        let data = family();
        let layout = descendant_circular_layout(
            ChartArc::DESCENDANT_FAN,
            id(1),
            &data,
            1,
            None,
            &HashSet::new(),
            DateStyle::DEFAULT,
        );
        let people: Vec<&Segment> = layout.segments.iter().filter(|s| !s.union).collect();
        assert_eq!(people.len(), 3, "the three children only");
        let root_union = layout.segments.iter().find(|s| s.union).expect("a union");
        assert!((root_union.ring.r_in - ChartArc::DESCENDANT_FAN.root_radius).abs() < EPS);
        assert!((people[0].ring.r_in - root_union.ring.r_out).abs() < EPS);
        assert_eq!(
            layout.segments.iter().filter(|s| s.union).count(),
            2,
            "none around the children"
        );
        // The fan hangs below its root.
        assert!((layout.origin_y - CHART_MARGIN).abs() < EPS);
        for s in &layout.segments {
            assert!(
                s.start >= 90.0 - EPS && s.end <= 270.0 + EPS,
                "below the root"
            );
        }
    }

    /// A union of the outermost generation drawn — here the root's own,
    /// childless — gets its band outside the persons' rings. It had none,
    /// and the chart panicked on an out-of-bounds ring.
    #[test]
    fn a_childless_union_of_the_last_generation_gets_its_band() {
        let mut f = Fixture::default();
        f.person(1, Sex::Male, "Root", "Branch_A")
            .person(2, Sex::Female, "Spouse_1", "Branch_B");
        f.family(100, &[1, 2], &[]);
        let data = f.build();
        for arc in [ChartArc::DESCENDANT_WHEEL, ChartArc::DESCENDANT_FAN] {
            let layout = descendant_circular_layout(
                arc,
                id(1),
                &data,
                3,
                None,
                &HashSet::new(),
                DateStyle::DEFAULT,
            );
            let union = layout.segments.iter().find(|s| s.union).expect("the union");
            assert!((union.ring.r_in - arc.root_radius).abs() < EPS);
            assert_eq!(layout.entries[union.entry].node.id, Some(id(2)));
        }
    }

    /// In the fan opening downwards, siblings read left to right: the eldest
    /// sits nearest nine o'clock, at the end of the clockwise arc.
    #[test]
    fn the_downward_fan_puts_the_eldest_on_the_left() {
        let data = family();
        let layout = descendant_circular_layout(
            ChartArc::DESCENDANT_FAN,
            id(1),
            &data,
            1,
            None,
            &HashSet::new(),
            DateStyle::DEFAULT,
        );
        let (eldest, _) = person_span(&layout, 10);
        let (second, _) = person_span(&layout, 11);
        let (third, _) = person_span(&layout, 12);
        assert!(
            eldest > second && second > third,
            "{eldest} {second} {third}"
        );
        let wheel = descendant_circular_layout(
            ChartArc::DESCENDANT_WHEEL,
            id(1),
            &data,
            1,
            None,
            &HashSet::new(),
            DateStyle::DEFAULT,
        );
        assert!(
            person_span(&wheel, 10).0 < person_span(&wheel, 11).0,
            "clockwise in the wheel"
        );
    }

    /// Bad data where a person is their own descendant does not loop.
    #[test]
    fn a_loop_in_the_data_ends() {
        let mut f = Fixture::default();
        f.person(1, Sex::Male, "Root", "Branch_A")
            .person(2, Sex::Male, "Child_1", "Branch_A");
        f.family(100, &[1], &[2]);
        f.family(101, &[2], &[1]);
        let data = f.build();
        let layout = descendant_circular_layout(
            ChartArc::DESCENDANT_WHEEL,
            id(1),
            &data,
            10,
            None,
            &HashSet::new(),
            DateStyle::DEFAULT,
        );
        assert_eq!(layout.segments.iter().filter(|s| !s.union).count(), 1);
    }

    /// A person with nobody after them is a root disc alone.
    #[test]
    fn a_person_without_descendants_is_the_root_alone() {
        let data = family();
        let layout = descendant_circular_layout(
            ChartArc::DESCENDANT_WHEEL,
            id(20),
            &data,
            3,
            None,
            &HashSet::new(),
            DateStyle::DEFAULT,
        );
        assert!(layout.segments.is_empty());
        assert!(layout.root_label.is_some());
    }
}
