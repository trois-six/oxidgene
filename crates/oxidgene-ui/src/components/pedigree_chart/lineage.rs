//! The horizontal charts — the lineage view, Gramps' *Pedigree* view, in
//! OxidGene's cards, and the descendant lineage, the hourglass and the
//! bowtie built the same way (see [`charts`]).
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

mod charts;
pub(super) use charts::{
    bowtie_layout, descendant_lineage_layout, hourglass_layout, lineage_layout,
};

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

/// What a box of a horizontal chart stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum BoxRole {
    /// A position of the ancestor side, its `sosa` a SOSA number.
    Ancestor,
    /// A person of the descendant side, its `sosa` only a key.
    Descendant,
    /// The spouse of a union of the descendant side, under the person.
    Spouse,
}

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
    /// What each entry is, parallel to `entries`.
    pub(super) roles: Vec<BoxRole>,
    pub(super) links: Vec<LineageLink>,
    /// What each entry's box covers, parallel to `entries`, and what each
    /// link covers, parallel to `links` (`None` when unreadable, which
    /// culling always draws): only those near the viewport are drawn.
    pub(super) box_extents: Vec<Area>,
    pub(super) link_extents: Vec<Option<Area>>,
    /// The root's spouses, in the order of their unions.
    pub(super) spouses: Vec<Uuid>,
    pub(super) children: Vec<ChildLink>,
    /// Width of every box.
    pub(super) box_w: f64,
    /// The root's entry.
    pub(super) root_index: usize,
    /// Whether the root is the chart's left edge (lineage, descendant
    /// lineage) rather than in its middle (hourglass, bowtie).
    pub(super) root_at_left: bool,
    /// Whether the button beside the root lists its spouses and children.
    pub(super) lists_family: bool,
    pub(super) root_is_sosa_root: bool,
    pub(super) origin_x: f64,
    pub(super) origin_y: f64,
    pub(super) total_w: f64,
    pub(super) total_h: f64,
}

impl LineageLayout {
    pub(super) fn fit_target(&self) -> FitTarget {
        let (root_x, root_y) = self
            .centres
            .get(self.root_index)
            .copied()
            .unwrap_or_default();
        FitTarget {
            content_cx: self.total_w / 2.0,
            content_cy: self.total_h / 2.0,
            content_w: self.total_w - 2.0 * MARGIN,
            content_h: self.total_h - 2.0 * MARGIN,
            root_cx: self.origin_x + root_x,
            root_cy: self.origin_y + root_y,
            root_at_left: self.root_at_left,
        }
    }

    /// The button beside the root that lists its spouses and children, when
    /// it has any: the view draws neither, only the root's ancestors.
    pub(super) fn family_button(&self) -> Option<(f64, f64)> {
        let (x, y) = *self.centres.get(self.root_index)?;
        (self.lists_family && (!self.spouses.is_empty() || !self.children.is_empty()))
            .then(|| (x - self.box_w / 2.0 - CHILDREN_BUTTON_ROOM / 2.0, y))
    }
}

/// The root's children, as [`PedigreeData::children_of`] orders them, each
/// marked when they have children of their own.
fn children_of(root_id: Uuid, data: &PedigreeData) -> Vec<ChildLink> {
    data.children_of(root_id)
        .into_iter()
        .map(|id| ChildLink {
            id,
            has_children: !data.children_of(id).is_empty(),
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

/// The elbow from the box at `child` to the boxes at `parents`, all in one
/// column to its right or, on a side mirrored to the left, to its left: a
/// child to its parents, a union to its children.
fn elbow(child: (f64, f64), parents: &[(f64, f64)], box_w: f64) -> String {
    let side = parents
        .first()
        .map_or(1.0, |p| if p.0 < child.0 { -1.0 } else { 1.0 });
    let right = child.0 + side * box_w / 2.0;
    let left = parents.first().map_or(right, |p| p.0 - side * box_w / 2.0);
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
            text { class: "fan-slot-plus no-print", x: "{x + w / 2.0:.2}", y: "{y + h / 2.0 + 5.0:.2}", "+" }
        }
    }
}

/// A union's spouse under the person's card, in the descendant charts: a
/// slim box with the union sign, which makes the spouse the focus; an
/// unknown spouse only names itself on hover.
fn render_spouse_box(
    entry: &AncestorEntry,
    centre: (f64, f64),
    box_w: f64,
    theme: &PedigreeTheme,
    actions: BoxActions,
    i18n: &I18n,
) -> Element {
    let (x, y) = (centre.0 - box_w / 2.0, centre.1 - SINGLE_H / 2.0);
    let node = &entry.node;
    let spouse = node.id;
    let (name, title) = match spouse {
        Some(_) => (
            slim_text(node, BoxSize::Single, (box_w - 30.0) as f32).name,
            ancestor_tooltip(entry, false, i18n),
        ),
        None => ("?".to_string(), i18n.t("couple.unknown_spouse")),
    };
    let BoxActions {
        mut selected_person_id,
        on_person_navigate,
        on_person_click,
        ..
    } = actions;
    rsx! {
        g {
            key: "lu-{entry.sosa}",
            class: if spouse.is_some() { "lineage-spouse" } else { "lineage-spouse lineage-spouse-unknown" },
            onclick: move |_| {
                if let Some(pid) = spouse {
                    selected_person_id.set(pid);
                    on_person_navigate.call(pid);
                }
            },
            oncontextmenu: move |evt: Event<MouseData>| {
                evt.prevent_default();
                evt.stop_propagation();
                if let Some(pid) = spouse {
                    selected_person_id.set(pid);
                    let coords = evt.client_coordinates();
                    on_person_click.call((pid, coords.x, coords.y));
                }
            },
            rect {
                class: "lineage-spouse-rect",
                x: "{x:.2}", y: "{y:.2}", width: "{box_w:.2}", height: "{SINGLE_H}",
                rx: "{theme.metrics.border_radius}",
                dangerous_inner_html: "{svg_title(&title)}",
            }
            text {
                class: "ped-card-name-text",
                x: "{x + 8.0:.2}", y: "{centre.1 + 4.0:.2}",
                style: "font-size:{SLIM_DATE_PX}px;font-family:{theme.card.body_font};fill:var(--pn-text);pointer-events:none",
                "\u{26AD} {name}"
            }
        }
    }
}

/// The button beside the root listing its spouses and children.
fn render_family_button(
    at: (f64, f64),
    label: String,
    on_family_menu: EventHandler<(f64, f64)>,
) -> Element {
    let (x, y) = at;
    rsx! {
        g {
            class: "lineage-children no-print",
            transform: "translate({x:.2},{y:.2})",
            role: "button",
            "aria-label": "{label}",
            onclick: move |evt: Event<MouseData>| {
                evt.stop_propagation();
                let coords = evt.client_coordinates();
                on_family_menu.call((coords.x, coords.y));
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
    on_family_menu: EventHandler<(f64, f64)>,
    theme: &'static PedigreeTheme,
    transform: Signal<ViewportTransform>,
    viewport: Signal<ViewportRect>,
    animating: Signal<bool>,
) -> Element {
    let i18n = use_i18n();
    // Only the boxes and links near the viewport are drawn, as in every
    // view; they are placed from the layout's origin.
    let region = use_culled_region(transform, viewport, animating)
        .translated(-layout.origin_x, -layout.origin_y);
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
                    for (i, link) in layout.links.iter().enumerate().filter(|(i, _)| in_region(&region, layout.link_extents[*i].as_ref())) {
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
                    for (i, entry) in layout.entries.iter().enumerate().filter(|(i, _)| region.intersects(&layout.box_extents[*i])) {
                        {
                            let numbered = layout.root_is_sosa_root && layout.roles[i] == BoxRole::Ancestor;
                            match (layout.roles[i], layout.sizes[i]) {
                                (BoxRole::Spouse, _) => render_spouse_box(
                                    entry,
                                    layout.centres[i],
                                    layout.box_w,
                                    theme,
                                    actions,
                                    &i18n,
                                ),
                                (_, BoxSize::Card) => render_pedigree_card(
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
                                (_, size) => render_slim_box(
                                    entry,
                                    size,
                                    layout.centres[i],
                                    layout.box_w,
                                    svg_title(&ancestor_tooltip(entry, numbered, &i18n)),
                                    theme,
                                    actions,
                                ),
                            }
                        }
                    }
                    if let Some(at) = layout.family_button() {
                        {render_family_button(at, i18n.t("pedigree.jump_to_family"), on_family_menu)}
                    }
                }
            }
        }
    }
}

/// The root's spouses and children, listed from the button beside it —
/// the view draws only ancestors, so this is how it reaches them. Picking one
/// makes them the focus. Drawn outside the pannable canvas, whose transform
/// would otherwise carry a fixed-position menu with it.
#[component]
pub(super) fn LineageFamilyMenu(
    data: SharedPedigree,
    root_person_id: Uuid,
    x: f64,
    y: f64,
    on_pick: EventHandler<Uuid>,
    on_close: EventHandler<()>,
) -> Element {
    let i18n = use_i18n();
    let spouses = data.spouses_of(root_person_id);
    let children = children_of(root_person_id, &data);
    let label = |id: Uuid| {
        let name = data.display_name(id, &i18n);
        let dates = format_lifespan(data.qualified_birth_year(id), data.qualified_death_year(id));
        if dates.is_empty() {
            name
        } else {
            format!("{name}  {dates}")
        }
    };
    rsx! {
        ContextMenuSurface {
            x,
            y,
            on_close,
            if !spouses.is_empty() {
                div { class: "context-menu-header", {i18n.t("pedigree.spouses")} }
            }
            for spouse in spouses {
                button {
                    key: "s-{spouse}",
                    class: "context-menu-item",
                    onclick: move |_| on_pick.call(spouse),
                    {label(spouse)}
                }
            }
            if !children.is_empty() {
                div { class: "context-menu-header", {i18n.t("pedigree.children")} }
            }
            for child in children {
                button {
                    key: "c-{child.id}",
                    class: if child.has_children { "context-menu-item lineage-child-with-children" } else { "context-menu-item" },
                    onclick: move |_| on_pick.call(child.id),
                    {label(child.id)}
                }
            }
        }
    }
}

/// A miniature of a horizontal chart for the view picker in the settings:
/// `boxes` as centres (the root's flagged), joined by `links`.
fn swatch_of(boxes: &[((f64, f64), bool)], links: &[String], box_w: f64) -> Element {
    const BOX_H: f64 = 9.0;
    rsx! {
        svg {
            class: "ped-theme-swatch ped-view-swatch",
            "viewBox": "0 0 120 68",
            "preserveAspectRatio": "xMidYMid meet",
            "aria-hidden": "true",
            rect { x: "0", y: "0", width: "120", height: "68", style: "fill:var(--pn-swatch-bg,transparent)" }
            for (i, d) in links.iter().enumerate() {
                path { key: "l{i}", class: "pedigree-connector-path", d: "{d}" }
            }
            for (i, ((x, y), root)) in boxes.iter().enumerate() {
                rect {
                    key: "b{i}",
                    x: "{x - box_w / 2.0}", y: "{y - BOX_H / 2.0}", width: "{box_w}", height: "{BOX_H}", rx: "1.5",
                    style: if *root { "fill:var(--pn-root-bg);stroke:var(--pn-border)" } else { "fill:var(--pn-bg);stroke:var(--pn-border)" },
                }
            }
        }
    }
}

/// The lineage view's miniature, its rows placed by [`row_centre`] and
/// joined by [`elbow`]: the root on the left, parents and grandparents in
/// columns to its right.
pub(super) fn swatch() -> Element {
    const DEPTH: u32 = 2;
    const PITCH: f64 = 15.0;
    const BOX_W: f64 = 26.0;
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
    let boxes: Vec<((f64, f64), bool)> = (1u64..8).map(|sosa| (centre(sosa), sosa == 1)).collect();
    swatch_of(&boxes, &links, BOX_W)
}

/// The descendant lineage's miniature: the root on the left, its children
/// in the next column, some of theirs in the last.
pub(super) fn descendant_lineage_swatch() -> Element {
    const BOX_W: f64 = 26.0;
    let (root, children, grandchildren) = (
        (23.0, 34.0),
        [(63.0, 14.0), (63.0, 34.0), (63.0, 54.0)],
        [(103.0, 8.0), (103.0, 20.0), (103.0, 54.0)],
    );
    let links = [
        elbow(root, &children, BOX_W),
        elbow(children[0], &grandchildren[..2], BOX_W),
        elbow(children[2], &grandchildren[2..], BOX_W),
    ];
    let mut boxes = vec![(root, true)];
    boxes.extend(children.iter().chain(&grandchildren).map(|&c| (c, false)));
    swatch_of(&boxes, &links, BOX_W)
}

/// The hourglass's miniature: children on the left of the root, parents on
/// its right.
pub(super) fn hourglass_swatch() -> Element {
    const BOX_W: f64 = 26.0;
    let root = (60.0, 34.0);
    let (children, parents) = ([(20.0, 18.0), (20.0, 50.0)], [(100.0, 18.0), (100.0, 50.0)]);
    let links = [elbow(root, &children, BOX_W), elbow(root, &parents, BOX_W)];
    let mut boxes = vec![(root, true)];
    boxes.extend(children.iter().chain(&parents).map(|&c| (c, false)));
    swatch_of(&boxes, &links, BOX_W)
}

/// The bowtie's miniature: the father's line on the left of the root, the
/// mother's on its right.
pub(super) fn bowtie_swatch() -> Element {
    const BOX_W: f64 = 22.0;
    let (root, father, mother) = ((60.0, 34.0), (34.0, 34.0), (86.0, 34.0));
    let (fathers, mothers) = ([(11.0, 18.0), (11.0, 50.0)], [(109.0, 18.0), (109.0, 50.0)]);
    let links = [
        elbow(root, &[father], BOX_W),
        elbow(root, &[mother], BOX_W),
        elbow(father, &fathers, BOX_W),
        elbow(mother, &mothers, BOX_W),
    ];
    let mut boxes = vec![(root, true), (father, false), (mother, false)];
    boxes.extend(fathers.iter().chain(&mothers).map(|&c| (c, false)));
    swatch_of(&boxes, &links, BOX_W)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::pedigree_chart::geometry_golden_tests::{Fixture, id};
    use oxidgene_core::types::FamilySpouse;

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
        assert_eq!(layout.spouses, vec![id(20)]);
        let (bx, by) = layout.family_button().expect("a button");
        let (rx, ry) = layout.centres[0];
        assert!(bx < rx - layout.box_w / 2.0, "left of the root");
        assert!((by - ry).abs() < EPS);
        assert!(
            bx - CHILDREN_BUTTON_R + layout.origin_x >= 0.0,
            "inside the canvas"
        );
        // A person with neither spouse nor children gets no button; a spouse
        // alone is enough for one.
        let childless = lineage_layout(
            id(21),
            &data,
            1,
            None,
            &HashSet::new(),
            &PedigreeTheme::CLASSIC,
        );
        assert!(childless.family_button().is_none());
        let mut married = family_fixture();
        married
            .families_as_spouse
            .entry(id(21))
            .or_default()
            .push(id(104));
        married.spouses_by_family.insert(
            id(104),
            married.spouses_by_family[&id(102)]
                .iter()
                .map(|link| FamilySpouse {
                    family_id: id(104),
                    person_id: if link.person_id == id(1) {
                        id(21)
                    } else {
                        id(23)
                    },
                    ..link.clone()
                })
                .collect(),
        );
        let spouse_only = lineage_layout(
            id(21),
            &married,
            1,
            None,
            &HashSet::new(),
            &PedigreeTheme::CLASSIC,
        );
        assert_eq!(spouse_only.spouses, vec![id(23)]);
        assert!(spouse_only.children.is_empty());
        assert!(spouse_only.family_button().is_some());
    }

    /// Culling draws a box when its extent meets the region: every extent
    /// must hold the card drawn there, and every link have one.
    #[test]
    fn every_box_and_link_has_an_extent_that_holds_it() {
        let data = family_fixture();
        let theme = &PedigreeTheme::CLASSIC;
        let layout = lineage_layout(id(1), &data, 4, None, &HashSet::new(), theme);
        let (rect_w, rect_h) = theme.metrics.rect(false);
        let pad = theme.metrics.padding;
        assert_eq!(layout.box_extents.len(), layout.entries.len());
        for (entry, extent) in layout.entries.iter().zip(&layout.box_extents) {
            let card = Area {
                x0: entry.node.x,
                y0: entry.node.y,
                x1: entry.node.x + rect_w + 2.0 * pad,
                y1: entry.node.y + rect_h + 2.0 * pad,
            };
            assert!(extent.contains(&card), "sosa {}", entry.sosa);
        }
        assert_eq!(layout.link_extents.len(), layout.links.len());
        assert!(layout.link_extents.iter().all(Option::is_some));
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
