//! The lineage view: Gramps' *Pedigree* view, in OxidGene's cards.
//!
//! The root stands on the left and its ancestors extend to the right, one
//! column per generation, each child joined to its father above and its
//! mother below by elbow lines. As in Gramps, a person keeps a fixed row
//! whatever is known about the others: the last column is divided evenly,
//! and every child sits halfway between its two parents. Descendants are not
//! drawn — Gramps reaches them through a button beside the active person that
//! lists their children, and so does this view.
//!
//! Columns with room for them hold the theme's own cards. Deep charts would
//! be tens of thousands of pixels tall at that size, so once the last column
//! runs out of room, its rows tighten to a slim box — two lines, then one —
//! the way Gramps' boxes shrink with the generations.

use super::ancestors::{
    AncestorEntry, ancestor_tooltip, collect_ancestors, generation_of, index_in_generation,
    mark_colour, svg_title,
};
use super::*;
use crate::components::context_menu::ContextMenuSurface;

/// Horizontal room between two columns, where the elbow lines run.
const COLUMN_GAP: f64 = 48.0;
/// Row pitch of the last column once it no longer holds full cards.
pub(super) const SLIM_PITCH: f64 = 30.0;
/// Tallest the chart may be while every column still holds full cards.
const ALL_CARDS_MAX_HEIGHT: f64 = 1600.0;
/// Heights of the two slim boxes.
const DOUBLE_H: f64 = 44.0;
const SINGLE_H: f64 = 24.0;
/// Room left of the root for the button listing its children.
const CHILDREN_BUTTON_ROOM: f64 = 44.0;
const CHILDREN_BUTTON_R: f64 = 12.0;
const MARGIN: f64 = 24.0;

/// How much of a person one row has room for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum BoxSize {
    /// The theme's own card: portrait, names, lifespan.
    Card,
    /// Name, then lifespan.
    Double,
    /// The name alone; the lifespan is in the hover text.
    Single,
}

/// Row pitch of the last column: a full card's height while the whole chart
/// stays within [`ALL_CARDS_MAX_HEIGHT`], a slim box's beyond.
pub(super) fn leaf_pitch(depth: u32, metrics: &PedigreeMetrics) -> f64 {
    let leaves = f64::from(1u32 << depth.min(31));
    if leaves * metrics.card_h <= ALL_CARDS_MAX_HEIGHT {
        metrics.card_h
    } else {
        SLIM_PITCH
    }
}

/// The height one row of `generation` has, in a chart `depth` generations
/// deep: twice the next column's, so each child sits between its parents.
pub(super) fn row_room(generation: u32, depth: u32, pitch: f64) -> f64 {
    pitch * f64::from(1u32 << (depth.saturating_sub(generation)).min(31))
}

/// The largest box a row of `room` holds.
pub(super) fn box_size(room: f64, metrics: &PedigreeMetrics) -> BoxSize {
    if room >= metrics.card_h {
        BoxSize::Card
    } else if room >= DOUBLE_H + 6.0 {
        BoxSize::Double
    } else {
        BoxSize::Single
    }
}

/// Where the row of a SOSA number is centred, from the top of the chart.
pub(super) fn row_centre(sosa: u64, depth: u32, pitch: f64) -> f64 {
    let room = row_room(generation_of(sosa), depth, pitch);
    room * (index_in_generation(sosa) as f64 + 0.5)
}

/// Left edge of a generation's column.
pub(super) fn column_x(generation: u32, metrics: &PedigreeMetrics) -> f64 {
    f64::from(generation) * (metrics.card_w + COLUMN_GAP)
}

/// One of the root's children, for the button that lists them.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct ChildLink {
    pub(super) id: Uuid,
    /// Set in bold, as Gramps does, when they have children of their own.
    pub(super) has_children: bool,
}

/// An elbow line from a child to its parents.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct LineageLink {
    pub(super) path: String,
    /// Drawn dashed when the child is not the parents' birth child —
    /// adopted, fostered — as Gramps draws a non-birth relationship.
    pub(super) non_birth: bool,
}

/// The lineage view, laid out.
pub(super) struct LineageLayout {
    pub(super) entries: Vec<AncestorEntry>,
    /// Size of each entry's box, parallel to `entries`.
    pub(super) sizes: Vec<BoxSize>,
    /// Centre of each entry's box, parallel to `entries`.
    pub(super) centres: Vec<(f64, f64)>,
    pub(super) links: Vec<LineageLink>,
    pub(super) children: Vec<ChildLink>,
    /// Width of every box.
    pub(super) box_w: f64,
    pub(super) root_is_sosa_root: bool,
    pub(super) origin_x: f64,
    pub(super) origin_y: f64,
    pub(super) total_w: f64,
    pub(super) total_h: f64,
}

impl LineageLayout {
    pub(super) fn fit_target(&self) -> FitTarget {
        let (root_x, root_y) = self.centres.first().copied().unwrap_or_default();
        FitTarget {
            content_cx: self.total_w / 2.0,
            content_cy: self.total_h / 2.0,
            content_w: self.total_w - 2.0 * MARGIN,
            content_h: self.total_h - 2.0 * MARGIN,
            root_cx: self.origin_x + root_x,
            root_cy: self.origin_y + root_y,
        }
    }

    /// The button beside the root that lists its children, when it has any.
    pub(super) fn children_button(&self) -> Option<(f64, f64)> {
        let (x, y) = *self.centres.first()?;
        (!self.children.is_empty()).then(|| (x - self.box_w / 2.0 - CHILDREN_BUTTON_ROOM / 2.0, y))
    }
}

/// The root's children, in the order of its unions and of their births.
fn children_of(root_id: Uuid, data: &PedigreeData) -> Vec<ChildLink> {
    let has_children = |pid: Uuid| {
        data.families_as_spouse.get(&pid).is_some_and(|families| {
            families.iter().any(|fid| {
                data.children_by_family
                    .get(fid)
                    .is_some_and(|c| !c.is_empty())
            })
        })
    };
    let mut seen = HashSet::new();
    data.families_as_spouse
        .get(&root_id)
        .into_iter()
        .flatten()
        .flat_map(|fid| {
            let mut children = data
                .children_by_family
                .get(fid)
                .cloned()
                .unwrap_or_default();
            children.sort_by_key(|child| child.sort_order);
            children
        })
        .filter(|child| seen.insert(child.person_id))
        .map(|child| ChildLink {
            id: child.person_id,
            has_children: has_children(child.person_id),
        })
        .collect()
}

/// Whether `child` is not the birth child of the family it descends through.
fn is_non_birth(child: Uuid, data: &PedigreeData) -> bool {
    let Some(fid) = data.families_as_child.get(&child).and_then(|f| f.first()) else {
        return false;
    };
    data.children_by_family
        .get(fid)
        .and_then(|children| children.iter().find(|c| c.person_id == child))
        .is_some_and(|c| c.child_type != ChildType::Biological)
}

/// The elbow from the child at `child` to whichever of its parents' rows are
/// drawn, `parents` being their centres.
fn elbow(child: (f64, f64), parents: &[(f64, f64)], box_w: f64) -> String {
    let right = child.0 + box_w / 2.0;
    let left = parents.first().map_or(right, |p| p.0 - box_w / 2.0);
    let mid = (right + left) / 2.0;
    let top = parents.iter().map(|p| p.1).fold(child.1, f64::min);
    let bottom = parents.iter().map(|p| p.1).fold(child.1, f64::max);
    let mut d = format!(
        "M{right:.2},{:.2} L{mid:.2},{:.2} M{mid:.2},{top:.2} L{mid:.2},{bottom:.2}",
        child.1, child.1
    );
    for parent in parents {
        d.push_str(&format!(
            " M{mid:.2},{:.2} L{left:.2},{:.2}",
            parent.1, parent.1
        ));
    }
    d
}

/// The elbow lines of every drawn child to its drawn parents.
fn collect_links(
    entries: &[AncestorEntry],
    centres: &[(f64, f64)],
    box_w: f64,
    data: &PedigreeData,
) -> Vec<LineageLink> {
    let position: HashMap<u64, usize> = entries
        .iter()
        .enumerate()
        .map(|(i, e)| (e.sosa, i))
        .collect();
    entries
        .iter()
        .enumerate()
        .filter_map(|(i, entry)| {
            let parents: Vec<(f64, f64)> = [2 * entry.sosa, 2 * entry.sosa + 1]
                .iter()
                .filter_map(|sosa| position.get(sosa).map(|&p| centres[p]))
                .collect();
            let child = entry.node.id?;
            (!parents.is_empty()).then(|| LineageLink {
                path: elbow(centres[i], &parents, box_w),
                non_birth: is_non_birth(child, data),
            })
        })
        .collect()
}

/// Lays out the lineage view of `root_id`'s ancestors, `generations` deep.
pub(super) fn lineage_layout(
    root_id: Uuid,
    data: &PedigreeData,
    generations: usize,
    sosa_root_id: Option<Uuid>,
    sosa_ancestors: &HashSet<Uuid>,
    theme: &PedigreeTheme,
) -> LineageLayout {
    let metrics = &theme.metrics;
    let depth = generations as u32;
    let pitch = leaf_pitch(depth, metrics);
    let (rect_w, _) = metrics.rect(false);
    let mut entries = collect_ancestors(root_id, data, generations, sosa_root_id, sosa_ancestors);
    let mut sizes = Vec::with_capacity(entries.len());
    let mut centres = Vec::with_capacity(entries.len());
    for entry in &mut entries {
        let generation = generation_of(entry.sosa);
        let size = box_size(row_room(generation, depth, pitch), metrics);
        let centre = (
            column_x(generation, metrics) + metrics.padding + rect_w / 2.0,
            row_centre(entry.sosa, depth, pitch),
        );
        // A card is drawn one `padding` inside its box, at the node's corner.
        entry.node.x = centre.0 - metrics.padding - rect_w / 2.0;
        entry.node.y = centre.1 - metrics.padding - metrics.rect(false).1 / 2.0;
        sizes.push(size);
        centres.push(centre);
    }
    let links = collect_links(&entries, &centres, rect_w, data);
    let columns_w = column_x(depth, metrics) + metrics.card_w;
    let rows_h = row_room(0, depth, pitch);
    LineageLayout {
        entries,
        sizes,
        centres,
        links,
        children: children_of(root_id, data),
        box_w: rect_w,
        root_is_sosa_root: sosa_root_id.is_some() && sosa_root_id == Some(root_id),
        origin_x: MARGIN + CHILDREN_BUTTON_ROOM,
        origin_y: MARGIN,
        total_w: columns_w + CHILDREN_BUTTON_ROOM + 2.0 * MARGIN,
        total_h: rows_h + 2.0 * MARGIN,
    }
}

/// A computed lineage layout, shared with the canvas that draws it.
/// Equality is identity, as for [`SharedLayout`].
#[derive(Clone)]
pub(super) struct SharedLineage(pub(super) Rc<LineageLayout>);

impl PartialEq for SharedLineage {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl std::ops::Deref for SharedLineage {
    type Target = LineageLayout;

    fn deref(&self) -> &LineageLayout {
        &self.0
    }
}

/// What a slim box answers to, as a card does.
#[derive(Clone, Copy)]
struct BoxActions {
    selected_person_id: Signal<Uuid>,
    on_person_navigate: EventHandler<Uuid>,
    on_person_click: EventHandler<(Uuid, f64, f64)>,
    on_empty_slot: EventHandler<(Uuid, bool)>,
}

/// The text of a slim box: the name, and the lifespan when it has two lines.
struct SlimText {
    name: String,
    dates: Option<String>,
}

/// Fits a slim box's text to `width`.
fn slim_text(node: &LayoutNode, size: BoxSize, width: f32) -> SlimText {
    let surname = node
        .label_surname
        .split(',')
        .next()
        .unwrap_or("")
        .trim()
        .to_uppercase();
    let given = node.label_given.split(',').next().unwrap_or("").trim();
    let name = format!("{surname} {given}").trim().to_string();
    SlimText {
        name: truncate_text_to_fit(&name, width, SLIM_NAME_PX),
        dates: (size == BoxSize::Double)
            .then(|| fit_lifespan(node.birth_year, node.death_year, width, SLIM_DATE_PX))
            .filter(|dates| !dates.is_empty()),
    }
}

const SLIM_NAME_PX: f32 = 11.0;
const SLIM_DATE_PX: f32 = 10.0;

/// A slim box: a frame with the sex rule down its left side and the SOSA
/// mark on its right, the name and, room permitting, the lifespan.
fn render_slim_box(
    entry: &AncestorEntry,
    size: BoxSize,
    centre: (f64, f64),
    box_w: f64,
    title: String,
    theme: &PedigreeTheme,
    actions: BoxActions,
) -> Element {
    let h = if size == BoxSize::Double {
        DOUBLE_H
    } else {
        SINGLE_H
    };
    let (x, y) = (centre.0 - box_w / 2.0, centre.1 - h / 2.0);
    let node = &entry.node;
    let key = format!("ls-{}", entry.sosa);
    let Some(pid) = node.id else {
        return render_slim_slot(entry, &key, (x, y, box_w, h), actions);
    };
    let BoxActions {
        mut selected_person_id,
        on_person_navigate,
        on_person_click,
        ..
    } = actions;
    let text = slim_text(node, size, (box_w - 26.0) as f32);
    let name_y = if text.dates.is_some() {
        y + 18.0
    } else {
        y + h / 2.0 + 4.0
    };
    let card = &theme.card;
    let mark = mark_colour(node);
    rsx! {
        g {
            key: "{key}",
            class: "ped-card",
            style: "cursor:pointer",
            onclick: move |_| { selected_person_id.set(pid); on_person_navigate.call(pid); },
            oncontextmenu: move |evt: Event<MouseData>| {
                evt.prevent_default();
                evt.stop_propagation();
                selected_person_id.set(pid);
                let coords = evt.client_coordinates();
                on_person_click.call((pid, coords.x, coords.y));
            },
            rect {
                class: "ped-card-rect",
                x: "{x:.2}", y: "{y:.2}", width: "{box_w:.2}", height: "{h}",
                rx: "{theme.metrics.border_radius}",
                style: "fill:var(--pn-bg);stroke:var(--pn-border);stroke-width:1",
                dangerous_inner_html: "{title}",
            }
            path {
                d: "M{x + 4.0:.2},{y + 4.0:.2} L{x + 4.0:.2},{y + h - 4.0:.2}",
                style: "stroke:{gender_stroke(node.sex)};stroke-width:3;fill:none;pointer-events:none",
            }
            if let Some(colour) = mark {
                circle { cx: "{x + box_w - 9.0:.2}", cy: "{centre.1:.2}", r: "4", style: "fill:{colour};pointer-events:none" }
            }
            text {
                class: "ped-card-name-text",
                x: "{x + 12.0:.2}", y: "{name_y:.2}",
                style: "font-size:{SLIM_NAME_PX}px;font-family:{card.body_font};font-weight:600;fill:var(--pn-text);pointer-events:none",
                "{text.name}"
            }
            if let Some(dates) = text.dates {
                text {
                    class: "ped-card-name-text",
                    x: "{x + 12.0:.2}", y: "{y + 34.0:.2}",
                    style: "font-size:{SLIM_DATE_PX}px;font-family:{card.body_font};fill:var(--pn-text-muted);pointer-events:none",
                    "{dates}"
                }
            }
        }
    }
}

/// A missing parent's slim box: dashed, with a "+" that adds them.
fn render_slim_slot(
    entry: &AncestorEntry,
    key: &str,
    (x, y, w, h): (f64, f64, f64, f64),
    actions: BoxActions,
) -> Element {
    let is_father = entry.node.is_father;
    let child = entry.node.child_of;
    let on_empty_slot = actions.on_empty_slot;
    rsx! {
        g {
            key: "{key}",
            class: "fan-slot",
            onclick: move |_| {
                if let Some(child) = child {
                    on_empty_slot.call((child, is_father));
                }
            },
            rect { class: "fan-slot-shape", x: "{x:.2}", y: "{y:.2}", width: "{w:.2}", height: "{h}", rx: "3" }
            text { class: "fan-slot-plus", x: "{x + w / 2.0:.2}", y: "{y + h / 2.0 + 5.0:.2}", "+" }
        }
    }
}

/// The button beside the root listing its children.
fn render_children_button(
    at: (f64, f64),
    label: String,
    on_children_menu: EventHandler<(f64, f64)>,
) -> Element {
    let (x, y) = at;
    rsx! {
        g {
            class: "lineage-children",
            transform: "translate({x:.2},{y:.2})",
            role: "button",
            "aria-label": "{label}",
            onclick: move |evt: Event<MouseData>| {
                evt.stop_propagation();
                let coords = evt.client_coordinates();
                on_children_menu.call((coords.x, coords.y));
            },
            circle { r: "{CHILDREN_BUTTON_R}", dangerous_inner_html: "{svg_title(&label)}" }
            text { x: "-1", y: "5", "\u{2039}" }
        }
    }
}

/// The lineage view, drawn.
#[component]
pub(super) fn LineageCanvas(
    layout: SharedLineage,
    root_person_id: Uuid,
    selected_person_id: Signal<Uuid>,
    on_person_navigate: EventHandler<Uuid>,
    on_person_click: EventHandler<(Uuid, f64, f64)>,
    on_empty_slot: EventHandler<(Uuid, bool)>,
    on_children_menu: EventHandler<(f64, f64)>,
    theme: &'static PedigreeTheme,
) -> Element {
    let i18n = use_i18n();
    let actions = BoxActions {
        selected_person_id,
        on_person_navigate,
        on_person_click,
        on_empty_slot,
    };
    let ruled = theme.link_style == crate::components::pedigree_theme::LinkStyle::Ruled;
    rsx! {
        div {
            class: "pedigree-tree",
            style: "position: relative; width: {layout.total_w}px; height: {layout.total_h}px;",
            svg {
                class: "lineage-chart",
                "viewBox": "0 0 {layout.total_w} {layout.total_h}",
                width: "{layout.total_w}",
                height: "{layout.total_h}",
                style: "display: block; overflow: visible;",
                g { transform: "translate({layout.origin_x},{layout.origin_y})",
                    for (i, link) in layout.links.iter().enumerate() {
                        path {
                            key: "ll-{i}",
                            d: "{link.path}",
                            class: if link.non_birth { "pedigree-connector-path lineage-link-non-birth" } else { "pedigree-connector-path" },
                            fill: "none",
                        }
                        if ruled && !link.non_birth {
                            path { key: "llc-{i}", d: "{link.path}", class: "pedigree-connector-core", fill: "none" }
                        }
                    }
                    for (i, entry) in layout.entries.iter().enumerate() {
                        {
                            match layout.sizes[i] {
                                BoxSize::Card => render_pedigree_card(
                                    &entry.node,
                                    i,
                                    "ln",
                                    root_person_id,
                                    selected_person_id,
                                    on_person_navigate,
                                    on_person_click,
                                    on_empty_slot,
                                    true,
                                    i18n,
                                    theme,
                                    None,
                                ),
                                size => render_slim_box(
                                    entry,
                                    size,
                                    layout.centres[i],
                                    layout.box_w,
                                    svg_title(&ancestor_tooltip(entry, layout.root_is_sosa_root, &i18n)),
                                    theme,
                                    actions,
                                ),
                            }
                        }
                    }
                    if let Some(at) = layout.children_button() {
                        {render_children_button(at, i18n.t("pedigree.jump_to_child"), on_children_menu)}
                    }
                }
            }
        }
    }
}

/// The root's children, listed from the button beside it; picking one makes
/// them the focus. Drawn outside the pannable canvas, whose transform would
/// otherwise carry a fixed-position menu with it.
#[component]
pub(super) fn LineageChildrenMenu(
    data: SharedPedigree,
    root_person_id: Uuid,
    x: f64,
    y: f64,
    on_pick: EventHandler<Uuid>,
    on_close: EventHandler<()>,
) -> Element {
    let i18n = use_i18n();
    let children = children_of(root_person_id, &data);
    rsx! {
        ContextMenuSurface {
            x,
            y,
            on_close,
            div { class: "context-menu-header", {i18n.t("pedigree.children")} }
            for child in children {
                {
                    let name = data.display_name(child.id, &i18n);
                    let dates = format_lifespan(data.qualified_birth_year(child.id), data.qualified_death_year(child.id));
                    let label = if dates.is_empty() { name } else { format!("{name}  {dates}") };
                    rsx! {
                        button {
                            key: "{child.id}",
                            class: if child.has_children { "context-menu-item lineage-child-with-children" } else { "context-menu-item" },
                            onclick: move |_| on_pick.call(child.id),
                            "{label}"
                        }
                    }
                }
            }
        }
    }
}

/// A three-generation miniature of the view for the picker in the settings,
/// its rows placed by [`row_centre`] and joined by [`elbow`]: the root on
/// the left, parents and grandparents in columns to its right.
pub(super) fn swatch() -> Element {
    const DEPTH: u32 = 2;
    const PITCH: f64 = 15.0;
    const BOX_W: f64 = 26.0;
    const BOX_H: f64 = 9.0;
    const STEP: f64 = 40.0;
    let centre = |sosa: u64| {
        (
            10.0 + f64::from(generation_of(sosa)) * STEP + BOX_W / 2.0,
            4.0 + row_centre(sosa, DEPTH, PITCH),
        )
    };
    let links: Vec<String> = (1u64..4)
        .map(|sosa| {
            elbow(
                centre(sosa),
                &[centre(2 * sosa), centre(2 * sosa + 1)],
                BOX_W,
            )
        })
        .collect();
    rsx! {
        svg {
            class: "ped-theme-swatch ped-view-swatch",
            "viewBox": "0 0 120 68",
            "preserveAspectRatio": "xMidYMid meet",
            "aria-hidden": "true",
            rect { x: "0", y: "0", width: "120", height: "68", style: "fill:var(--pn-swatch-bg,transparent)" }
            for (i, d) in links.into_iter().enumerate() {
                path { key: "l{i}", class: "pedigree-connector-path", d: "{d}" }
            }
            for sosa in 1u64..8 {
                {
                    let (x, y) = centre(sosa);
                    rsx! {
                        rect {
                            key: "{sosa}",
                            x: "{x - BOX_W / 2.0}", y: "{y - BOX_H / 2.0}", width: "{BOX_W}", height: "{BOX_H}", rx: "1.5",
                            style: if sosa == 1 { "fill:var(--pn-root-bg);stroke:var(--pn-border)" } else { "fill:var(--pn-bg);stroke:var(--pn-border)" },
                        }
                    }
                }
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
    fn every_child_sits_halfway_between_its_parents() {
        for depth in 1..=10u32 {
            for pitch in [SLIM_PITCH, 96.0] {
                for sosa in 1u64..(1 << depth) {
                    let mid = (row_centre(2 * sosa, depth, pitch)
                        + row_centre(2 * sosa + 1, depth, pitch))
                        / 2.0;
                    assert!(
                        (row_centre(sosa, depth, pitch) - mid).abs() < EPS,
                        "{sosa} at depth {depth}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_father_is_above_the_mother_and_the_root_in_the_middle() {
        let depth = 3;
        let pitch = 96.0;
        assert!(row_centre(2, depth, pitch) < row_centre(3, depth, pitch));
        assert!((row_centre(1, depth, pitch) - row_room(0, depth, pitch) / 2.0).abs() < EPS);
        // The last column is divided evenly.
        for sosa in 8u64..15 {
            assert!(
                (row_centre(sosa + 1, depth, pitch) - row_centre(sosa, depth, pitch) - pitch).abs()
                    < EPS
            );
        }
    }

    #[test]
    fn columns_step_by_a_card_and_the_gap() {
        let metrics = PedigreeTheme::CLASSIC.metrics;
        assert_eq!(column_x(0, &metrics), 0.0);
        assert!((column_x(3, &metrics) - 3.0 * (metrics.card_w + COLUMN_GAP)).abs() < EPS);
    }

    #[test]
    fn shallow_charts_hold_cards_and_deep_ones_tighten_their_last_columns() {
        for theme in [&PedigreeTheme::CLASSIC, &PedigreeTheme::MEDIEVAL] {
            let m = &theme.metrics;
            // Three generations always fit as cards.
            let pitch = leaf_pitch(3, m);
            assert_eq!(pitch, m.card_h);
            for g in 0..=3 {
                assert_eq!(box_size(row_room(g, 3, pitch), m), BoxSize::Card);
            }
            // Eight generations: slim boxes at the far end, cards near the root.
            let pitch = leaf_pitch(8, m);
            assert_eq!(pitch, SLIM_PITCH);
            assert_eq!(box_size(row_room(8, 8, pitch), m), BoxSize::Single);
            assert_eq!(box_size(row_room(7, 8, pitch), m), BoxSize::Double);
            assert_eq!(box_size(row_room(0, 8, pitch), m), BoxSize::Card);
            // No box is ever taller than its row.
            for g in 0..=8 {
                let room = row_room(g, 8, pitch);
                let h = match box_size(room, m) {
                    BoxSize::Card => m.card_h,
                    BoxSize::Double => DOUBLE_H,
                    BoxSize::Single => SINGLE_H,
                };
                assert!(h <= room + EPS, "{g}: {h} in {room}");
            }
        }
    }

    fn family_fixture() -> PedigreeData {
        let mut f = Fixture::default();
        f.person(1, Sex::Male, "Root", "Branch_A")
            .person(2, Sex::Male, "Father_1", "Branch_A")
            .person(3, Sex::Female, "Mother_1", "Branch_B")
            .person(4, Sex::Male, "Grandfather_1", "Branch_A")
            .person(20, Sex::Female, "Spouse_1", "Branch_C")
            .person(21, Sex::Male, "Child_1", "Branch_A")
            .person(22, Sex::Female, "Child_2", "Branch_A")
            .person(23, Sex::Female, "Spouse_2", "Branch_D")
            .person(24, Sex::Male, "Grandchild_1", "Branch_A");
        f.family(100, &[2, 3], &[1]);
        f.family(101, &[4], &[2]);
        f.family(102, &[1, 20], &[21, 22]);
        f.family(103, &[22, 23], &[24]);
        f.build()
    }

    #[test]
    fn the_layout_places_each_ancestor_by_sosa_and_links_it_to_its_parents() {
        let data = family_fixture();
        let theme = &PedigreeTheme::CLASSIC;
        let layout = lineage_layout(id(1), &data, 2, None, &HashSet::new(), theme);
        let sosas: Vec<u64> = layout.entries.iter().map(|e| e.sosa).collect();
        // Root, both parents, the father's father and his missing wife, and
        // the mother's two missing parents.
        assert_eq!(sosas, vec![1, 2, 3, 4, 5, 6, 7]);
        let pitch = leaf_pitch(2, &theme.metrics);
        for (entry, centre) in layout.entries.iter().zip(&layout.centres) {
            let g = generation_of(entry.sosa);
            assert!((centre.1 - row_centre(entry.sosa, 2, pitch)).abs() < EPS);
            let rect_w = theme.metrics.rect(false).0;
            assert!(
                (centre.0 - column_x(g, &theme.metrics) - theme.metrics.padding - rect_w / 2.0)
                    .abs()
                    < EPS
            );
        }
        // Links from the root and from both parents (the mother's are to
        // empty slots, still drawn so the "+" is attached to her).
        assert_eq!(layout.links.len(), 3);
        assert!(layout.links.iter().all(|l| !l.non_birth));
    }

    #[test]
    fn the_root_lists_its_children_and_marks_those_with_children() {
        let data = family_fixture();
        let layout = lineage_layout(
            id(1),
            &data,
            1,
            None,
            &HashSet::new(),
            &PedigreeTheme::CLASSIC,
        );
        assert_eq!(
            layout.children,
            vec![
                ChildLink {
                    id: id(21),
                    has_children: false
                },
                ChildLink {
                    id: id(22),
                    has_children: true
                },
            ]
        );
        let (bx, by) = layout.children_button().expect("a button");
        let (rx, ry) = layout.centres[0];
        assert!(bx < rx - layout.box_w / 2.0, "left of the root");
        assert!((by - ry).abs() < EPS);
        assert!(
            bx - CHILDREN_BUTTON_R + layout.origin_x >= 0.0,
            "inside the canvas"
        );
        // A person with no children gets no button.
        let childless = lineage_layout(
            id(21),
            &data,
            1,
            None,
            &HashSet::new(),
            &PedigreeTheme::CLASSIC,
        );
        assert!(childless.children_button().is_none());
    }

    #[test]
    fn an_adopted_child_is_linked_by_a_dashed_line() {
        let mut data = family_fixture();
        let fid = id(100);
        if let Some(children) = data.children_by_family.get_mut(&fid) {
            children[0].child_type = ChildType::Adopted;
        }
        let layout = lineage_layout(
            id(1),
            &data,
            1,
            None,
            &HashSet::new(),
            &PedigreeTheme::CLASSIC,
        );
        assert_eq!(layout.links.len(), 1);
        assert!(layout.links[0].non_birth);
    }

    #[test]
    fn an_elbow_runs_right_then_splits_to_both_parents() {
        let d = elbow((100.0, 50.0), &[(300.0, 20.0), (300.0, 80.0)], 100.0);
        assert_eq!(
            d,
            "M150.00,50.00 L200.00,50.00 M200.00,20.00 L200.00,80.00 M200.00,20.00 L250.00,20.00 M200.00,80.00 L250.00,80.00"
        );
    }

    #[test]
    fn a_slim_box_keeps_the_precision_marks_and_fits_its_width() {
        let mut node = LayoutNode {
            id: Some(id(1)),
            x: 0.0,
            y: 0.0,
            sex: Sex::Female,
            label_surname: "Longbranch_of_the_valley".to_string(),
            label_given: "Marie-Madeleine-Antoinette".to_string(),
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
        };
        node.birth_year = Some(QualifiedYear {
            year: 1849,
            qualifier: DateQualifier::About,
            year2: None,
        });
        let text = slim_text(&node, BoxSize::Double, 150.0);
        assert!(crate::utils::estimate_text_width_px(&text.name, SLIM_NAME_PX) <= 150.0);
        assert!(text.name.starts_with("LONGBRANCH"));
        assert_eq!(text.dates.as_deref(), Some("ca 1849-"));
        assert_eq!(slim_text(&node, BoxSize::Single, 150.0).dates, None);
    }
}
