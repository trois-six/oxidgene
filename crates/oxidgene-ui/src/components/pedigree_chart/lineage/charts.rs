//! The horizontal charts, laid out: the lineage (ancestors to the right),
//! the descendant lineage (descendants to the right), the hourglass
//! (descendants to the left, ancestors to the right) and the bowtie (the
//! father's ancestors to the left, the mother's to the right).
//!
//! Each is built from the same parts. An ancestor side places a person by
//! their SOSA number, a fixed row whatever is known about the others, each
//! child halfway between its parents, as Gramps' Pedigree view does. A
//! descendant side gives each person as much height as their descendants
//! take, their unions' spouses in slim boxes under their card and each
//! union's children in the next column, as Gramps' Descendant Tree does. A
//! side runs to the right or, mirrored, to the left of the root; the
//! [`Chart`] then frames every box and draws the elbow lines.

use super::super::descent::{Tree, unknown_spouse};
use super::*;

/// Key base of the entries a descendant side adds: their `sosa` only keys
/// them, and must not meet an ancestor's number.
const DESCENDANT_KEY_BASE: u64 = 1 << 40;
/// Gap between two spouse boxes.
const SPOUSE_GAP: f64 = 4.0;
/// Gap between a card and the first spouse box under it: room for the
/// pencil the focus card carries at its foot.
const UNDER_CARD_GAP: f64 = 14.0;

/// Which way a side of the chart runs from the root.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::components::pedigree_chart) enum Side {
    Right,
    Left,
}

impl Side {
    fn sign(self) -> f64 {
        match self {
            Self::Right => 1.0,
            Self::Left => -1.0,
        }
    }
}

/// A line from one box to the boxes it leads to: a child to its parents, a
/// union to its children.
struct Relation {
    from: usize,
    to: Vec<usize>,
    non_birth: bool,
}

/// A horizontal chart being built: its boxes, and what joins them.
#[derive(Default)]
pub(super) struct Chart {
    entries: Vec<AncestorEntry>,
    sizes: Vec<BoxSize>,
    centres: Vec<(f64, f64)>,
    roles: Vec<BoxRole>,
    relations: Vec<Relation>,
}

impl Chart {
    fn push(
        &mut self,
        entry: AncestorEntry,
        size: BoxSize,
        centre: (f64, f64),
        role: BoxRole,
    ) -> usize {
        self.entries.push(entry);
        self.sizes.push(size);
        self.centres.push(centre);
        self.roles.push(role);
        self.entries.len() - 1
    }

    fn relate(&mut self, from: usize, to: Vec<usize>, non_birth: bool) {
        if !to.is_empty() {
            self.relations.push(Relation {
                from,
                to,
                non_birth,
            });
        }
    }
}

/// How the finished chart behaves around its root.
pub(super) struct Framing {
    pub(super) root_index: usize,
    /// The root is the chart's left edge (lineage, descendant lineage).
    pub(super) root_at_left: bool,
    /// A button beside the root lists what the chart does not draw of the
    /// root's family (the lineage view).
    pub(super) lists_family: bool,
    pub(super) root_is_sosa_root: bool,
}

/// Half the height of a box of `size`.
fn half_height(size: BoxSize, metrics: &PedigreeMetrics) -> f64 {
    match size {
        BoxSize::Card => metrics.card_h / 2.0,
        BoxSize::Double => DOUBLE_H / 2.0 + metrics.padding,
        BoxSize::Single => SINGLE_H / 2.0 + metrics.padding,
    }
}

/// Frames `chart`: every box placed on its node, the elbow lines drawn, the
/// canvas sized around what is drawn with a margin, and the extents culling
/// tests.
pub(super) fn finish(
    mut chart: Chart,
    framing: Framing,
    root_id: Uuid,
    data: &PedigreeData,
    metrics: &PedigreeMetrics,
) -> LineageLayout {
    let (rect_w, rect_h) = metrics.rect(false);
    let half_w = rect_w / 2.0 + metrics.padding;
    let bounds = chart
        .centres
        .iter()
        .zip(&chart.sizes)
        .fold(None::<Area>, |acc, (&(x, y), &size)| {
            let half_h = half_height(size, metrics);
            let area = Area {
                x0: x - half_w,
                y0: y - half_h,
                x1: x + half_w,
                y1: y + half_h,
            };
            Some(acc.map_or(area, |a| a.union(&area)))
        })
        .unwrap_or(Area {
            x0: 0.0,
            y0: 0.0,
            x1: 0.0,
            y1: 0.0,
        });
    for (entry, &(x, y)) in chart.entries.iter_mut().zip(&chart.centres) {
        // A card is drawn one `padding` inside its box, at the node's corner.
        entry.node.x = x - metrics.padding - rect_w / 2.0;
        entry.node.y = y - metrics.padding - rect_h / 2.0;
    }
    let links: Vec<LineageLink> = chart
        .relations
        .iter()
        .map(|relation| {
            let targets: Vec<(f64, f64)> = relation.to.iter().map(|&t| chart.centres[t]).collect();
            LineageLink {
                path: elbow(chart.centres[relation.from], &targets, rect_w),
                non_birth: relation.non_birth,
            }
        })
        .collect();
    // Every box as large as a card, the badges and the pencil overhanging
    // it: culling may draw too much, never too little.
    let (reach_w, reach_h) = (
        half_w + CARD_OVERHANG,
        metrics.card_h / 2.0 + metrics.padding + CARD_OVERHANG,
    );
    let box_extents = chart
        .centres
        .iter()
        .map(|&(x, y)| Area {
            x0: x - reach_w,
            y0: y - reach_h,
            x1: x + reach_w,
            y1: y + reach_h,
        })
        .collect();
    let link_extents = links.iter().map(|link| path_extent(&link.path)).collect();
    let left_room = MARGIN
        + if framing.lists_family {
            CHILDREN_BUTTON_ROOM
        } else {
            0.0
        };
    LineageLayout {
        entries: chart.entries,
        sizes: chart.sizes,
        centres: chart.centres,
        roles: chart.roles,
        links,
        box_extents,
        link_extents,
        spouses: data.spouses_of(root_id),
        children: children_of(root_id, data),
        box_w: rect_w,
        root_index: framing.root_index,
        root_at_left: framing.root_at_left,
        lists_family: framing.lists_family,
        root_is_sosa_root: framing.root_is_sosa_root,
        origin_x: left_room - bounds.x0,
        origin_y: MARGIN - bounds.y0,
        total_w: bounds.x1 - bounds.x0 + left_room + MARGIN,
        total_h: bounds.y1 - bounds.y0 + 2.0 * MARGIN,
    }
}

/// The centre of a column `generation` steps from the root's, on `side`.
fn column_centre(generation: u32, side: Side, metrics: &PedigreeMetrics) -> f64 {
    let (rect_w, _) = metrics.rect(false);
    side.sign() * column_x(generation, metrics) + metrics.padding + rect_w / 2.0
}

/// Adds the ancestor `entries` placed by `place` (centre and box size for a
/// SOSA number, `None` to leave it out), each child joined to its parents;
/// the root to each parent on its own when `split_root` (the bowtie, whose
/// parents are on either side). Returns the index of the root.
fn add_ancestors(
    chart: &mut Chart,
    entries: Vec<AncestorEntry>,
    place: impl Fn(u64) -> ((f64, f64), BoxSize),
    split_root: bool,
    data: &PedigreeData,
) -> usize {
    let mut index: HashMap<u64, usize> = HashMap::new();
    for entry in entries {
        let (centre, size) = place(entry.sosa);
        let sosa = entry.sosa;
        index.insert(sosa, chart.push(entry, size, centre, BoxRole::Ancestor));
    }
    let mut sosas: Vec<u64> = index.keys().copied().collect();
    sosas.sort_unstable();
    for sosa in sosas {
        let i = index[&sosa];
        let Some(child) = chart.entries[i].node.id else {
            continue;
        };
        let parents: Vec<usize> = [2 * sosa, 2 * sosa + 1]
            .iter()
            .filter_map(|p| index.get(p).copied())
            .collect();
        let non_birth = is_non_birth(child, data);
        if split_root && sosa == 1 {
            for parent in parents {
                chart.relate(i, vec![parent], non_birth);
            }
        } else {
            chart.relate(i, parents, non_birth);
        }
    }
    index.get(&1).copied().unwrap_or(0)
}

/// The ancestors of the lineage view: the root's column first, one column
/// per generation to its right, rows as [`row_centre`] places them.
fn lineage_ancestors(
    chart: &mut Chart,
    entries: Vec<AncestorEntry>,
    depth: u32,
    data: &PedigreeData,
    metrics: &PedigreeMetrics,
) -> usize {
    let pitch = leaf_pitch(depth, metrics);
    add_ancestors(
        chart,
        entries,
        |sosa| {
            let generation = generation_of(sosa);
            (
                (
                    column_centre(generation, Side::Right, metrics),
                    row_centre(sosa, depth, pitch),
                ),
                box_size(row_room(generation, depth, pitch), metrics),
            )
        },
        false,
        data,
    )
}

/// How tall a person of the descendant side is drawn, and a spouse box.
struct DescendantPitch {
    person: f64,
    size: BoxSize,
    spouse: f64,
}

impl DescendantPitch {
    /// Full cards while the side stays within the height every column holds
    /// cards at, slim boxes beyond.
    fn of(leaves: f64, metrics: &PedigreeMetrics) -> Self {
        let (person, size) = if leaves * metrics.card_h <= ALL_CARDS_MAX_HEIGHT {
            (metrics.card_h, BoxSize::Card)
        } else {
            (DOUBLE_H + 2.0 * SPOUSE_GAP, BoxSize::Double)
        };
        Self {
            person,
            size,
            spouse: SINGLE_H + SPOUSE_GAP,
        }
    }

    /// A person's own block: their box and their spouses' under it.
    fn block(&self, unions: usize) -> f64 {
        if unions == 0 {
            self.person
        } else {
            self.person + UNDER_CARD_GAP - SPOUSE_GAP + unions as f64 * self.spouse
        }
    }
}

/// How tall each person's part of the descendant side is: their block, or
/// the children of their unions stacked, whichever is taller.
fn subtree_heights(tree: &Tree, pitch: &DescendantPitch) -> Vec<f64> {
    let mut heights = vec![0.0; tree.people.len()];
    for i in (0..tree.people.len()).rev() {
        let person = &tree.people[i];
        let children: f64 = person
            .unions
            .iter()
            .flat_map(|u| &u.children)
            .map(|&c| heights[c])
            .sum();
        heights[i] = pitch.block(person.unions.len()).max(children);
    }
    heights
}

/// Where each person's block starts, from the top of the side: the root at
/// 0, each person's children stacked in the order of their unions and
/// centred on their part, which their own block is centred on too.
fn subtree_tops(tree: &Tree, heights: &[f64]) -> Vec<f64> {
    let mut tops = vec![0.0; tree.people.len()];
    for (i, person) in tree.people.iter().enumerate() {
        let children: Vec<usize> = person
            .unions
            .iter()
            .flat_map(|u| u.children.clone())
            .collect();
        let total: f64 = children.iter().map(|&c| heights[c]).sum();
        let mut cursor = tops[i] + (heights[i] - total) / 2.0;
        for child in children {
            tops[child] = cursor;
            cursor += heights[child];
        }
    }
    tops
}

/// What a descendant side needs of the tree.
pub(super) struct Descendants<'a> {
    pub(super) data: &'a PedigreeData,
    pub(super) sosa_root_id: Option<Uuid>,
    pub(super) sosa_ancestors: &'a HashSet<Uuid>,
    pub(super) metrics: &'a PedigreeMetrics,
}

/// Adds the descendants of `tree`'s root on `side`, the root's block centred
/// on `root_y`; the root itself is `root_index` when the chart already has
/// it (the hourglass), or added here. Each union's spouse sits in a slim box
/// under the person's card, and a line joins it to the union's children.
/// Returns the index of the root.
fn add_descendants(
    chart: &mut Chart,
    tree: &Tree,
    side: Side,
    root_y: f64,
    root_index: Option<usize>,
    from: &Descendants<'_>,
) -> usize {
    let weights = tree.weights();
    let pitch = DescendantPitch::of(weights[0], from.metrics);
    let heights = subtree_heights(tree, &pitch);
    let tops = subtree_tops(tree, &heights);
    let block_top = |i: usize| {
        let block = pitch.block(tree.people[i].unions.len());
        tops[i] + (heights[i] - block) / 2.0
    };
    // The root's card centre sits on `root_y`.
    let shift = root_y - (block_top(0) + pitch.person / 2.0);
    let node_of = |id: Uuid| {
        PersonNode::from_data(id, from.data, from.sosa_root_id, from.sosa_ancestors)
            .card_at(id, 0.0, 0.0)
    };
    let mut placed = vec![0usize; tree.people.len()];
    for (i, person) in tree.people.iter().enumerate() {
        let x = column_centre(person.depth, side, from.metrics);
        let top = block_top(i) + shift;
        placed[i] = match (i, root_index) {
            (0, Some(root)) => root,
            _ => {
                let key = DESCENDANT_KEY_BASE + chart.entries.len() as u64;
                let entry = AncestorEntry {
                    sosa: key,
                    node: node_of(person.id),
                };
                let size = if i == 0 { BoxSize::Card } else { pitch.size };
                chart.push(
                    entry,
                    size,
                    (x, top + pitch.person / 2.0),
                    BoxRole::Descendant,
                )
            }
        };
    }
    for (i, person) in tree.people.iter().enumerate() {
        let x = column_centre(person.depth, side, from.metrics);
        let mut y = block_top(i) + shift + pitch.person + UNDER_CARD_GAP;
        for union in &person.unions {
            let node = union.spouse.map_or_else(unknown_spouse, node_of);
            let key = DESCENDANT_KEY_BASE + chart.entries.len() as u64;
            let entry = AncestorEntry { sosa: key, node };
            let spouse = chart.push(
                entry,
                BoxSize::Single,
                (x, y + SINGLE_H / 2.0),
                BoxRole::Spouse,
            );
            let children = union.children.iter().map(|&c| placed[c]).collect();
            chart.relate(spouse, children, false);
            y += pitch.spouse;
        }
    }
    placed[0]
}

/// Lays out the lineage view of `root_id`'s ancestors, `generations` deep.
pub(in crate::components::pedigree_chart) fn lineage_layout(
    root_id: Uuid,
    data: &PedigreeData,
    generations: usize,
    sosa_root_id: Option<Uuid>,
    sosa_ancestors: &HashSet<Uuid>,
    theme: &PedigreeTheme,
) -> LineageLayout {
    let metrics = &theme.metrics;
    let mut chart = Chart::default();
    let entries = collect_ancestors(root_id, data, generations, sosa_root_id, sosa_ancestors);
    let root_index = lineage_ancestors(&mut chart, entries, generations as u32, data, metrics);
    let framing = Framing {
        root_index,
        root_at_left: true,
        lists_family: true,
        root_is_sosa_root: sosa_root_id.is_some() && sosa_root_id == Some(root_id),
    };
    finish(chart, framing, root_id, data, metrics)
}

/// Lays out the descendant lineage of `root_id`, `generations` deep: Gramps'
/// Descendant Tree, the root on the left and a column per generation to its
/// right.
pub(in crate::components::pedigree_chart) fn descendant_lineage_layout(
    root_id: Uuid,
    data: &PedigreeData,
    generations: usize,
    sosa_root_id: Option<Uuid>,
    sosa_ancestors: &HashSet<Uuid>,
    theme: &PedigreeTheme,
) -> LineageLayout {
    let metrics = &theme.metrics;
    let tree = Tree::collect(root_id, data, generations as u32);
    let mut chart = Chart::default();
    let from = Descendants {
        data,
        sosa_root_id,
        sosa_ancestors,
        metrics,
    };
    let root_index = add_descendants(&mut chart, &tree, Side::Right, 0.0, None, &from);
    let framing = Framing {
        root_index,
        root_at_left: true,
        lists_family: false,
        root_is_sosa_root: false,
    };
    finish(chart, framing, root_id, data, metrics)
}

/// Lays out the hourglass of `root_id`: its ancestors to the right,
/// `ancestor_generations` deep, as the lineage view draws them, and its
/// descendants to the left, `descendant_generations` deep, as the
/// descendant lineage draws them mirrored — webtrees' horizontal hourglass.
/// The ancestor side carries no direct-ancestor badge (everyone there is
/// one); the descendant side keeps it, tracing the line to the SOSA 1.
pub(in crate::components::pedigree_chart) fn hourglass_layout(
    root_id: Uuid,
    data: &PedigreeData,
    ancestor_generations: usize,
    descendant_generations: usize,
    sosa_root_id: Option<Uuid>,
    sosa_ancestors: &HashSet<Uuid>,
    theme: &PedigreeTheme,
) -> LineageLayout {
    let metrics = &theme.metrics;
    let mut chart = Chart::default();
    let entries = collect_ancestors(
        root_id,
        data,
        ancestor_generations,
        sosa_root_id,
        &HashSet::new(),
    );
    let root_index = lineage_ancestors(
        &mut chart,
        entries,
        ancestor_generations as u32,
        data,
        metrics,
    );
    let root_y = chart.centres[root_index].1;
    let tree = Tree::collect(root_id, data, descendant_generations as u32);
    let from = Descendants {
        data,
        sosa_root_id,
        sosa_ancestors,
        metrics,
    };
    add_descendants(
        &mut chart,
        &tree,
        Side::Left,
        root_y,
        Some(root_index),
        &from,
    );
    let framing = Framing {
        root_index,
        root_at_left: false,
        lists_family: false,
        root_is_sosa_root: sosa_root_id.is_some() && sosa_root_id == Some(root_id),
    };
    finish(chart, framing, root_id, data, metrics)
}

/// Lays out the bowtie of `root_id`, `generations` deep: the root in the
/// middle, its father's ancestors to the left and its mother's to the
/// right, each side a lineage of its own — to compare the two branches.
pub(in crate::components::pedigree_chart) fn bowtie_layout(
    root_id: Uuid,
    data: &PedigreeData,
    generations: usize,
    sosa_root_id: Option<Uuid>,
    sosa_ancestors: &HashSet<Uuid>,
    theme: &PedigreeTheme,
) -> LineageLayout {
    let metrics = &theme.metrics;
    let depth = generations as u32;
    // Each side holds half the last generation.
    let pitch = leaf_pitch(depth.saturating_sub(1), metrics);
    let side_depth = depth.saturating_sub(1);
    let mut chart = Chart::default();
    let entries = collect_ancestors(root_id, data, generations, sosa_root_id, sosa_ancestors);
    let root_index = add_ancestors(
        &mut chart,
        entries,
        |sosa| bowtie_place(sosa, side_depth, pitch, metrics),
        true,
        data,
    );
    let framing = Framing {
        root_index,
        root_at_left: false,
        lists_family: false,
        root_is_sosa_root: sosa_root_id.is_some() && sosa_root_id == Some(root_id),
    };
    finish(chart, framing, root_id, data, metrics)
}

/// Where the bowtie places SOSA `sosa`: the root in the middle, level with
/// its parents; the father's side (the first half of every generation) to
/// the left, the mother's to the right, each laid out as a lineage of
/// `side_depth` generations above the parent.
fn bowtie_place(
    sosa: u64,
    side_depth: u32,
    pitch: f64,
    metrics: &PedigreeMetrics,
) -> ((f64, f64), BoxSize) {
    let generation = generation_of(sosa);
    let side_rows = |g: u32| row_room(g, side_depth + 1, pitch);
    if generation == 0 {
        return (
            (column_centre(0, Side::Right, metrics), side_rows(1) / 2.0),
            BoxSize::Card,
        );
    }
    let half = 1u64 << (generation - 1);
    let index = index_in_generation(sosa);
    let (side, local) = if index < half {
        (Side::Left, index)
    } else {
        (Side::Right, index - half)
    };
    let room = side_rows(generation);
    (
        (
            column_centre(generation, side, metrics),
            room * (local as f64 + 0.5),
        ),
        box_size(room, metrics),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::pedigree_chart::geometry_golden_tests::{Fixture, id};

    const EPS: f64 = 1e-6;

    /// Three generations up and down around the root (1): parents 2 and 3,
    /// grandparents 4 and 5 on the father's side; the root married to 20
    /// with children 21 and 22, 22 married to 23 with child 24, and a
    /// second union of the root with no spouse recorded, child 25.
    fn family() -> PedigreeData {
        let mut f = Fixture::default();
        f.person(1, Sex::Male, "Root", "Branch_A")
            .person(2, Sex::Male, "Father_1", "Branch_A")
            .person(3, Sex::Female, "Mother_1", "Branch_B")
            .person(4, Sex::Male, "Grandfather_1", "Branch_A")
            .person(5, Sex::Female, "Grandmother_1", "Branch_C")
            .person(20, Sex::Female, "Spouse_1", "Branch_D")
            .person(21, Sex::Male, "Child_1", "Branch_A")
            .person(22, Sex::Female, "Child_2", "Branch_A")
            .person(23, Sex::Male, "Spouse_2", "Branch_E")
            .person(24, Sex::Male, "Grandchild_1", "Branch_E")
            .person(25, Sex::Female, "Child_3", "Branch_A");
        f.family(100, &[2, 3], &[1]);
        f.family(101, &[4, 5], &[2]);
        f.family(102, &[1, 20], &[21, 22]);
        f.family(103, &[22, 23], &[24]);
        f.family(104, &[1], &[25]);
        f.build()
    }

    fn index_of(layout: &LineageLayout, n: u128, role: BoxRole) -> usize {
        (0..layout.entries.len())
            .find(|&i| layout.entries[i].node.id == Some(id(n)) && layout.roles[i] == role)
            .unwrap_or_else(|| panic!("{n} as {role:?}"))
    }

    /// No two boxes of one column overlap, whatever their size.
    fn assert_no_overlap(layout: &LineageLayout, metrics: &PedigreeMetrics) {
        for i in 0..layout.entries.len() {
            for j in i + 1..layout.entries.len() {
                let ((xi, yi), (xj, yj)) = (layout.centres[i], layout.centres[j]);
                if (xi - xj).abs() > EPS {
                    continue;
                }
                let gap = (yi - yj).abs();
                let need = half_height(layout.sizes[i], metrics) - metrics.padding
                    + half_height(layout.sizes[j], metrics)
                    - metrics.padding;
                assert!(
                    gap + EPS >= need,
                    "boxes {i} and {j} overlap: {gap} < {need}"
                );
            }
        }
    }

    /// Gramps' Descendant Tree: the root on the left, each generation a
    /// column to its right, each union's spouse under the person and its
    /// children joined to that spouse box.
    #[test]
    fn the_descendant_lineage_runs_right_with_spouses_under_each_person() {
        let data = family();
        let theme = &PedigreeTheme::CLASSIC;
        let layout = descendant_lineage_layout(id(1), &data, 2, None, &HashSet::new(), theme);
        let root = index_of(&layout, 1, BoxRole::Descendant);
        assert_eq!(layout.root_index, root);
        assert!(layout.root_at_left);
        let child = index_of(&layout, 22, BoxRole::Descendant);
        let grandchild = index_of(&layout, 24, BoxRole::Descendant);
        let (root_x, _) = layout.centres[root];
        assert!(layout.centres[child].0 > root_x);
        assert!(layout.centres[grandchild].0 > layout.centres[child].0);
        // The spouse sits under the root's card, in its column.
        let spouse = index_of(&layout, 20, BoxRole::Spouse);
        assert!((layout.centres[spouse].0 - root_x).abs() < EPS);
        assert!(layout.centres[spouse].1 > layout.centres[root].1);
        // The unknown spouse of the second union has a box too.
        let unknown = layout
            .roles
            .iter()
            .zip(&layout.entries)
            .filter(|(role, e)| **role == BoxRole::Spouse && e.node.id.is_none())
            .count();
        assert_eq!(unknown, 1);
        // Spouse boxes lead to the children; nothing else does.
        assert_eq!(
            layout.links.len(),
            3,
            "two unions of the root, one of Child_2"
        );
        assert_no_overlap(&layout, &theme.metrics);
        // The depth limits it.
        let shallow = descendant_lineage_layout(id(1), &data, 1, None, &HashSet::new(), theme);
        assert!(shallow.entries.iter().all(|e| e.node.id != Some(id(24))));
    }

    /// webtrees' horizontal hourglass: the root once, its ancestors to the
    /// right as in the lineage, its descendants to the left, level with it.
    #[test]
    fn the_hourglass_puts_descendants_left_and_ancestors_right() {
        let data = family();
        let theme = &PedigreeTheme::CLASSIC;
        let layout = hourglass_layout(id(1), &data, 2, 2, Some(id(1)), &HashSet::new(), theme);
        let roots = layout
            .entries
            .iter()
            .filter(|e| e.node.id == Some(id(1)))
            .count();
        assert_eq!(roots, 1, "the root is drawn once");
        let root = layout.root_index;
        let (root_x, root_y) = layout.centres[root];
        assert!(!layout.root_at_left);
        let father = index_of(&layout, 2, BoxRole::Ancestor);
        let child = index_of(&layout, 21, BoxRole::Descendant);
        assert!(layout.centres[father].0 > root_x);
        assert!(layout.centres[child].0 < root_x);
        let spouse = index_of(&layout, 20, BoxRole::Spouse);
        assert!(
            (layout.centres[spouse].0 - root_x).abs() < EPS,
            "under the root"
        );
        // The descendant side is centred on the root's row.
        let lineage = lineage_layout(id(1), &data, 2, Some(id(1)), &HashSet::new(), theme);
        assert!((root_y - lineage.centres[lineage.root_index].1).abs() < EPS);
        assert_no_overlap(&layout, &theme.metrics);
    }

    /// The bowtie: the root in the middle, level with its parents, the
    /// father's line to the left and the mother's to the right, each side
    /// a lineage of its own.
    #[test]
    fn the_bowtie_splits_the_lines_either_side_of_the_root() {
        let data = family();
        let theme = &PedigreeTheme::CLASSIC;
        let layout = bowtie_layout(id(1), &data, 3, None, &HashSet::new(), theme);
        let root = layout.root_index;
        let (root_x, root_y) = layout.centres[root];
        let father = index_of(&layout, 2, BoxRole::Ancestor);
        let mother = index_of(&layout, 3, BoxRole::Ancestor);
        let grandfather = index_of(&layout, 4, BoxRole::Ancestor);
        assert!(layout.centres[father].0 < root_x && layout.centres[mother].0 > root_x);
        assert!((layout.centres[father].1 - root_y).abs() < EPS);
        assert!((layout.centres[mother].1 - root_y).abs() < EPS);
        assert!(layout.centres[grandfather].0 < layout.centres[father].0);
        // The two sides mirror each other.
        assert!(
            (root_x - layout.centres[father].0 - (layout.centres[mother].0 - root_x)).abs() < EPS
        );
        // Root to each parent, each parent to theirs, the grandparents to
        // theirs: missing ones are empty slots, still joined.
        assert_eq!(layout.links.len(), 6);
        assert_no_overlap(&layout, &theme.metrics);
    }
}
