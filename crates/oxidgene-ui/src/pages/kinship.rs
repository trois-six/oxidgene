//! Kinship page — every way two persons of a tree are related, each drawn
//! generation by generation from the ancestors they share.

use std::collections::HashMap;

use dioxus::prelude::*;
use oxidgene_core::Sex;
use oxidgene_core::projection::SearchEntry;
use oxidgene_core::types::{Kinship as KinshipReport, KinshipPath, KinshipSegment};
use uuid::Uuid;

use crate::api::{ApiClient, CroppedSource};
use crate::components::search_person::{
    PersonSearchSummary, SearchPerson, render_person_search_summary,
};
use crate::components::tree_cache::{fetch_tree_cached, use_tree_cache};
use crate::components::tree_icon_sidebar::{TreeIconSidebar, TreeSidebarView};
use crate::i18n::{I18n, use_i18n};
use crate::router::Route;
use crate::ui_observability::{UiPage, use_traced_resource, use_ui_load_trace};

/// Page rendered at `/trees/:tree_id/kinship?from=...&to=...`.
#[component]
pub fn Kinship(tree_id: String, from: String, to: String) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let nav = use_navigator();
    let tree_cache = use_tree_cache();
    let load_trace = use_ui_load_trace(UiPage::Kinship);

    // Signals kept in sync with the props: the router reuses this component
    // when only the query changes, e.g. after a swap.
    let tree_id_parsed = use_signal(|| tree_id.parse::<Uuid>().ok());
    let from_parsed = use_signal(|| from.parse::<Uuid>().ok());
    let to_parsed = use_signal(|| to.parse::<Uuid>().ok());
    for (mut signal, raw) in [
        (tree_id_parsed, &tree_id),
        (from_parsed, &from),
        (to_parsed, &to),
    ] {
        let parsed = raw.parse::<Uuid>().ok();
        if parsed != *signal.peek() {
            *signal.write() = parsed;
        }
    }
    let mut picking = use_signal(|| false);

    let api_tree = api.clone();
    let tree_resource = use_traced_resource(load_trace.clone(), "tree", move || {
        let api = api_tree.clone();
        let _generation = tree_cache.generation();
        let tid = tree_id_parsed();
        async move {
            match tid {
                Some(tid) => fetch_tree_cached(&api, &tree_cache, tid).await.ok(),
                None => None,
            }
        }
    });

    // With nobody chosen yet, the question is most often "how is this person
    // related to me": start from the person the tree names as the user, or
    // failing that its SOSA root.
    use_effect(move || {
        if to_parsed().is_some() {
            return;
        }
        let (Some(tid), Some(from)) = (tree_id_parsed(), from_parsed()) else {
            return;
        };
        let Some(Some(tree)) = &*tree_resource.read() else {
            return;
        };
        let default = tree
            .self_person_id
            .into_iter()
            .chain(tree.sosa_root_person_id)
            .find(|&id| id != from);
        if let Some(default) = default {
            nav.replace(Route::Kinship {
                tree_id: tid.to_string(),
                from: from.to_string(),
                to: default.to_string(),
            });
        }
    });

    // The two ends, shown before and while the paths load.
    let api_ends = api.clone();
    let ends_resource = use_traced_resource(load_trace.clone(), "kinship_ends", move || {
        let api = api_ends.clone();
        let (tid, from, to) = (tree_id_parsed(), from_parsed(), to_parsed());
        async move {
            let mut ends = HashMap::new();
            let Some(tid) = tid else { return ends };
            for id in [from, to].into_iter().flatten() {
                if let Ok(profile) = api.get_person_profile(tid, id).await {
                    ends.insert(id, PersonSearchSummary::from(profile));
                }
            }
            ends
        }
    });

    let api_kinship = api.clone();
    let kinship_resource = use_traced_resource(load_trace.clone(), "kinship", move || {
        let api = api_kinship.clone();
        let (tid, from, to) = (tree_id_parsed(), from_parsed(), to_parsed());
        async move {
            match (tid, from, to) {
                (Some(tid), Some(from), Some(to)) if from != to => {
                    Some(api.get_kinship(tid, from, to).await)
                }
                _ => None,
            }
        }
    });

    let api_portraits = api.clone();
    let portraits_resource = use_traced_resource(load_trace, "portraits", move || {
        let api = api_portraits.clone();
        let tid = tree_id_parsed();
        let mut ids: Vec<Uuid> = [from_parsed(), to_parsed()].into_iter().flatten().collect();
        if let Some(Some(Ok(kinship))) = &*kinship_resource.read() {
            ids.extend(kinship.persons.iter().map(|person| person.person_id));
        }
        ids.sort_unstable();
        ids.dedup();
        async move {
            match tid {
                Some(tid) => api.portrait_map_for_ids(tid, &ids).await,
                None => HashMap::new(),
            }
        }
    });

    let tree_name = match &*tree_resource.read() {
        Some(Some(tree)) => tree.name.clone(),
        _ => tree_id_parsed()
            .and_then(|tid| tree_cache.tree(tid))
            .map(|tree| tree.name)
            .unwrap_or_default(),
    };
    let ends = ends_resource.read().clone().unwrap_or_default();
    let portraits = portraits_resource.read().clone().unwrap_or_default();
    let choosing = picking() || to_parsed().is_none();

    let go = {
        let tree_id = tree_id.clone();
        move |from: Uuid, to: Option<Uuid>| {
            nav.replace(Route::Kinship {
                tree_id: tree_id.clone(),
                from: from.to_string(),
                to: to.map(|id| id.to_string()).unwrap_or_default(),
            });
        }
    };
    let on_pick = {
        let go = go.clone();
        move |id: Uuid| {
            picking.set(false);
            if let Some(from) = from_parsed() {
                go(from, Some(id));
            }
        }
    };
    let on_swap = move |_| {
        if let (Some(from), Some(to)) = (from_parsed(), to_parsed()) {
            go(to, Some(from));
        }
    };

    rsx! {
        div { class: "sub-page",
            div { class: "td-topbar",
                nav { class: "td-bc",
                    Link { to: Route::Home {}, class: "td-bc-logo",
                        img {
                            src: crate::components::layout::LOGO_PNG_B64,
                            alt: "OxidGene",
                            class: "td-bc-logo-img",
                        }
                    }
                    if !tree_name.is_empty() {
                        Link {
                            to: Route::TreeDetail { tree_id: tree_id.clone(), person: None },
                            class: "td-bc-link",
                            "{tree_name}"
                        }
                        span { class: "td-bc-sep", "/" }
                    }
                    span { class: "td-bc-current", {i18n.t("kinship.breadcrumb")} }
                }
            }

            div { class: "pd-page-shell",
                TreeIconSidebar {
                    active_view: TreeSidebarView::None,
                    selected_person_id: from_parsed(),
                    show_middle_separator: false,
                    show_add_person: false,
                    on_profile_view: {
                        let tree_id = tree_id.clone();
                        move |pid: Option<Uuid>| {
                            if let Some(pid) = pid {
                                nav.push(Route::PersonDetail {
                                    tree_id: tree_id.clone(),
                                    person_id: pid.to_string(),
                                });
                            }
                        }
                    },
                    on_pedigree_view: {
                        let tree_id = tree_id.clone();
                        move |pid: Option<Uuid>| {
                            nav.push(Route::TreeDetail {
                                tree_id: tree_id.clone(),
                                person: pid.map(|pid| pid.to_string()),
                            });
                        }
                    },
                    on_add_person: move |_| {},
                    on_dictionary: {
                        let tree_id = tree_id.clone();
                        move |_| {
                            nav.push(Route::Dictionary { tree_id: tree_id.clone() });
                        }
                    },
                    on_settings: {
                        let tree_id = tree_id.clone();
                        move |_| {
                            nav.push(Route::Settings { tree_id: tree_id.clone() });
                        }
                    },
                }

                div { class: "sub-page-content kin-content",
                    div { class: "card kin-ends",
                        div { class: "kin-end-slot",
                            div { class: "kin-end-label", {i18n.t("kinship.from")} }
                            {end_row(from_parsed(), &ends, &portraits, &tree_id, &i18n)}
                        }
                        button {
                            class: "btn btn-outline btn-sm kin-swap",
                            title: i18n.t("kinship.swap"),
                            aria_label: i18n.t("kinship.swap"),
                            disabled: to_parsed().is_none(),
                            onclick: on_swap,
                            "\u{21C4}"
                        }
                        div { class: "kin-end-slot",
                            div { class: "kin-end-label",
                                {i18n.t("kinship.to")}
                                if to_parsed().is_some() && !choosing {
                                    button {
                                        class: "btn btn-outline btn-sm kin-change",
                                        onclick: move |_| picking.set(true),
                                        {i18n.t("kinship.change")}
                                    }
                                }
                            }
                            if choosing {
                                if let Some(tid) = tree_id_parsed() {
                                    SearchPerson {
                                        tree_id: tid,
                                        placeholder: i18n.t("kinship.choose"),
                                        on_select: on_pick,
                                        on_cancel: move |_| picking.set(false),
                                    }
                                }
                            } else {
                                {end_row(to_parsed(), &ends, &portraits, &tree_id, &i18n)}
                            }
                        }
                    }

                    {
                        let report = kinship_resource.read();
                        match &*report {
                            _ if to_parsed().is_none() => rsx! {
                                p { class: "text-muted kin-status", {i18n.t("kinship.pick_target")} }
                            },
                            None | Some(None) => rsx! {
                                div { class: "loading kin-status", {i18n.t("kinship.loading")} }
                            },
                            Some(Some(Err(error))) => rsx! {
                                div { class: "error-msg kin-status",
                                    {i18n.t_args("kinship.error", &[("error", &error.to_string())])}
                                }
                            },
                            Some(Some(Ok(report))) => render_report(report, &portraits, &tree_id, &i18n),
                        }
                    }
                }
            }
        }
    }
}

/// One of the two ends, as the shared person row.
fn end_row(
    id: Option<Uuid>,
    ends: &HashMap<Uuid, PersonSearchSummary>,
    portraits: &HashMap<Uuid, CroppedSource>,
    tree_id: &str,
    i18n: &I18n,
) -> Element {
    let Some(id) = id else {
        return rsx! {};
    };
    let summary = ends
        .get(&id)
        .cloned()
        .unwrap_or_else(|| PersonSearchSummary::placeholder(id, String::new()));
    person_link(
        &summary,
        portraits.get(&id).cloned(),
        tree_id,
        "kin-end",
        i18n,
    )
}

/// A person as the shared search row, leading to their profile.
fn person_link(
    summary: &PersonSearchSummary,
    portrait: Option<CroppedSource>,
    tree_id: &str,
    extra_class: &str,
    i18n: &I18n,
) -> Element {
    let sex_class = match summary.sex() {
        Sex::Male => "male",
        Sex::Female => "female",
        Sex::Unknown => "",
    };
    rsx! {
        Link {
            class: "search-person-result kin-person {sex_class} {extra_class}",
            to: Route::PersonDetail {
                tree_id: tree_id.to_string(),
                person_id: summary.person_id().to_string(),
            },
            {render_person_search_summary(summary, portrait, i18n)}
        }
    }
}

fn render_report(
    report: &KinshipReport,
    portraits: &HashMap<Uuid, CroppedSource>,
    tree_id: &str,
    i18n: &I18n,
) -> Element {
    if report.paths.is_empty() {
        return rsx! {
            p { class: "text-muted kin-status", {i18n.t("kinship.none")} }
        };
    }
    let persons: HashMap<Uuid, &SearchEntry> = report
        .persons
        .iter()
        .map(|person| (person.person_id, person))
        .collect();
    let view = PathView {
        persons: &persons,
        portraits,
        tree_id,
        from: report.from_person_id,
        to: report.to_person_id,
        i18n,
    };
    rsx! {
        p { class: "kin-status",
            {i18n.t_plural("kinship.found", report.paths.len())}
            if report.truncated {
                " — "
                {i18n.t("kinship.truncated")}
            }
        }
        for (index, path) in report.paths.iter().enumerate() {
            {view.path(index, path)}
        }
    }
}

/// What drawing a path needs besides the path.
struct PathView<'a> {
    persons: &'a HashMap<Uuid, &'a SearchEntry>,
    portraits: &'a HashMap<Uuid, CroppedSource>,
    tree_id: &'a str,
    from: Uuid,
    to: Uuid,
    i18n: &'a I18n,
}

impl PathView<'_> {
    fn sex(&self, id: Option<Uuid>) -> Sex {
        id.and_then(|id| self.persons.get(&id))
            .map_or(Sex::Unknown, |person| person.sex)
    }

    fn name(&self, id: Uuid) -> String {
        self.persons
            .get(&id)
            .map(|person| person.display_name.clone())
            .unwrap_or_default()
    }

    fn path(&self, index: usize, path: &KinshipPath) -> Element {
        let i18n = self.i18n;
        let (title, chain, half) = match path.segments.as_slice() {
            [segment] => (
                relation_label(
                    segment.from_line.len(),
                    segment.to_line.len(),
                    self.sex(Some(self.to)),
                    segment.half,
                    i18n,
                )
                .unwrap_or_default(),
                None,
                segment.half && (segment.from_line.len(), segment.to_line.len()) != (1, 1),
            ),
            segments => (
                i18n.t("kinship.by_marriage"),
                Some(self.chain(segments)),
                false,
            ),
        };

        // Generations are counted from the first person, across unions: a
        // spouse stands on the same generation as the person they married.
        let mut start = 0_i64;
        let mut blocks = Vec::new();
        for (position, segment) in path.segments.iter().enumerate() {
            blocks.push(self.segment(position, segment, start));
            start += segment.from_line.len() as i64 - segment.to_line.len() as i64;
        }

        rsx! {
            section { class: "card kin-path",
                div { class: "kin-path-hd",
                    span { class: "kin-path-num", "{index + 1}" }
                    h3 { class: "kin-path-title", "{title}" }
                    if half {
                        span { class: "kin-path-note", {i18n.t("kinship.half_note")} }
                    }
                }
                if let Some(chain) = chain {
                    p { class: "kin-chain", "{chain}" }
                    p { class: "kin-chain-hint",
                        {i18n.t_args("kinship.chain_hint", &[("name", &self.name(self.from))])}
                    }
                }
                div {
                    class: "kin-path-body",
                    title: i18n.t_args("kinship.generation_hint", &[("name", &self.name(self.from))]),
                    for block in blocks {
                        {block}
                    }
                }
            }
        }
    }

    /// A relationship by marriage, step by step from the first person: each
    /// segment's relation, and the spouse at every union.
    fn chain(&self, segments: &[KinshipSegment]) -> String {
        let mut steps = Vec::new();
        for (position, segment) in segments.iter().enumerate() {
            if position > 0 {
                steps.push(sexed("spouse", self.sex(segment.first_person()), self.i18n));
            }
            if let Some(label) = relation_label(
                segment.from_line.len(),
                segment.to_line.len(),
                self.sex(segment.last_person()),
                segment.half,
                self.i18n,
            ) {
                steps.push(label);
            }
        }
        steps.join(" \u{203A} ")
    }

    /// One segment as a grid: the ancestors on top, then one row per
    /// generation, the first person's line on the left and the other's on
    /// the right.
    fn segment(&self, position: usize, segment: &KinshipSegment, start: i64) -> Element {
        let (left, right) = (&segment.from_line, &segment.to_line);
        let single = left.is_empty() || right.is_empty();
        let top = start + left.len() as i64;
        let rows = left.len().max(right.len());
        let class = if single {
            "kin-seg kin-seg-single"
        } else {
            "kin-seg"
        };

        rsx! {
            if position > 0 {
                div { class: "kin-union", "\u{26AD} " {self.i18n.t("kinship.union")} }
            }
            div { class: "{class}",
                div { class: "kin-gen", "{generation_label(top)}" }
                div { class: "kin-top",
                    for &id in &segment.ancestor_ids {
                        {self.cell(id)}
                    }
                }
                for row in 0..rows {
                    div { class: "kin-gen", "{generation_label(top - row as i64 - 1)}" }
                    if !left.is_empty() {
                        div { class: "kin-cell",
                            if let Some(&id) = left.get(row) {
                                {self.cell(id)}
                            }
                        }
                    }
                    if !right.is_empty() {
                        div { class: "kin-cell",
                            if let Some(&id) = right.get(row) {
                                {self.cell(id)}
                            }
                        }
                    }
                }
            }
        }
    }

    fn cell(&self, id: Uuid) -> Element {
        let summary = match self.persons.get(&id) {
            Some(entry) => PersonSearchSummary::from(*entry),
            None => PersonSearchSummary::placeholder(id, self.i18n.t("common.unknown")),
        };
        let end = if id == self.from || id == self.to {
            "kin-end"
        } else {
            ""
        };
        person_link(
            &summary,
            self.portraits.get(&id).cloned(),
            self.tree_id,
            end,
            self.i18n,
        )
    }
}

/// A generation relative to the first person: `+2`, `0`, `−1`.
fn generation_label(generation: i64) -> String {
    match generation {
        0 => "0".to_string(),
        g if g > 0 => format!("+{g}"),
        g => format!("\u{2212}{}", -g),
    }
}

/// The key of a relation worded for `sex`.
fn sexed(key: &str, sex: Sex, i18n: &I18n) -> String {
    i18n.t(&format!("kinship.rel.{key}_{}", sex_suffix(sex)))
}

/// What the person at the end of a segment is to the person at its start,
/// given how many generations the segment climbs (`up`) and then descends
/// (`down`). `None` for a segment that is one person.
///
/// Close relations have a word of their own; beyond them the label says how
/// many generations separate the two, which every language can put the same
/// way.
pub(crate) fn relation_label(
    up: usize,
    down: usize,
    sex: Sex,
    half: bool,
    i18n: &I18n,
) -> Option<String> {
    let counted =
        |key: &str, n: usize| i18n.t_args(&format!("kinship.rel.{key}"), &[("n", &n.to_string())]);
    let label = match (up, down) {
        (0, 0) => return None,
        (0, 1) => sexed("child", sex, i18n),
        (0, 2) => sexed("grandchild", sex, i18n),
        (0, 3) => sexed("great_grandchild", sex, i18n),
        (0, n) => counted("descendant", n),
        (1, 0) => sexed("parent", sex, i18n),
        (2, 0) => sexed("grandparent", sex, i18n),
        (3, 0) => sexed("great_grandparent", sex, i18n),
        (n, 0) => counted("ancestor", n),
        (1, 1) if half => sexed("half_sibling", sex, i18n),
        (1, 1) => sexed("sibling", sex, i18n),
        (2, 1) => sexed("uncle", sex, i18n),
        (3, 1) => sexed("great_uncle", sex, i18n),
        (n, 1) => counted("ancestor_sibling", n - 1),
        (1, 2) => sexed("nephew", sex, i18n),
        (1, 3) => sexed("grand_nephew", sex, i18n),
        (1, n) => counted("sibling_descendant", n - 1),
        (up, down) => {
            let cousin = match up.min(down) - 1 {
                1 => sexed("cousin_1", sex, i18n),
                2 => sexed("cousin_2", sex, i18n),
                degree => i18n.t_args(
                    &format!("kinship.rel.cousin_n_{}", sex_suffix(sex)),
                    &[("n", &degree.to_string())],
                ),
            };
            match up.abs_diff(down) {
                0 => cousin,
                removed => format!(
                    "{cousin} \u{2013} {}",
                    i18n.t_plural("kinship.rel.removed", removed)
                ),
            }
        }
    };
    Some(label)
}

fn sex_suffix(sex: Sex) -> &'static str {
    match sex {
        Sex::Male => "m",
        Sex::Female => "f",
        Sex::Unknown => "u",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Language;

    fn en(up: usize, down: usize, sex: Sex) -> String {
        relation_label(up, down, sex, false, &I18n(Language::En)).unwrap()
    }

    fn fr(up: usize, down: usize, sex: Sex) -> String {
        relation_label(up, down, sex, false, &I18n(Language::Fr)).unwrap()
    }

    #[test]
    fn direct_lines_are_named_then_counted() {
        assert_eq!(en(1, 0, Sex::Female), "Mother");
        assert_eq!(en(3, 0, Sex::Male), "Great-grandfather");
        assert_eq!(en(5, 0, Sex::Male), "Ancestor, 5 generations up");
        assert_eq!(en(0, 2, Sex::Unknown), "Grandchild");
        assert_eq!(fr(0, 1, Sex::Female), "Fille");
    }

    #[test]
    fn collaterals_follow_the_shorter_side() {
        assert_eq!(en(2, 1, Sex::Female), "Aunt");
        assert_eq!(en(1, 2, Sex::Male), "Nephew");
        assert_eq!(
            en(4, 1, Sex::Male),
            "Sibling of an ancestor 3 generations up"
        );
        assert_eq!(fr(3, 1, Sex::Male), "Grand-oncle");
    }

    #[test]
    fn cousins_carry_their_degree_and_the_generations_between_them() {
        assert_eq!(en(2, 2, Sex::Female), "First cousin");
        assert_eq!(fr(2, 2, Sex::Female), "Cousine germaine");
        assert_eq!(fr(3, 3, Sex::Male), "Cousin issu de germain");
        assert_eq!(fr(4, 4, Sex::Female), "Cousine au 3e degré");
        assert_eq!(
            en(2, 3, Sex::Male),
            "First cousin \u{2013} 1 generation apart"
        );
        assert_eq!(
            en(4, 2, Sex::Male),
            "First cousin \u{2013} 2 generations apart"
        );
    }

    #[test]
    fn half_siblings_have_their_own_word() {
        let en = I18n(Language::En);
        assert_eq!(
            relation_label(1, 1, Sex::Male, true, &en).as_deref(),
            Some("Half-brother")
        );
        assert_eq!(relation_label(0, 0, Sex::Male, false, &en), None);
    }

    #[test]
    fn generations_are_signed() {
        assert_eq!(generation_label(2), "+2");
        assert_eq!(generation_label(0), "0");
        assert_eq!(generation_label(-1), "\u{2212}1");
    }
}
