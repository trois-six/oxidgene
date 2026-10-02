//! Kinship page — every way two persons of a tree are related, each drawn
//! generation by generation from the ancestors they share.

use std::collections::HashMap;

use dioxus::prelude::*;
use oxidgene_core::Sex;
use oxidgene_core::collections::sorted_unique;
use oxidgene_core::projection::SearchEntry;
use oxidgene_core::types::{Kinship as KinshipReport, KinshipPath, KinshipSegment};
use uuid::Uuid;

use crate::api::{ApiClient, ApiError, CroppedSource};
use crate::components::person_picker::PersonPicker;
use crate::components::search_person::{PersonSearchSummary, render_person_search_summary};
use crate::components::tree_page::{ToolPageFrame, use_tree_page};
use crate::i18n::{I18n, use_i18n};
use crate::router::Route;
use crate::ui_observability::{UiPage, use_traced_resource, use_ui_load_trace};

/// The search summaries of the two ends, those that can be read.
async fn load_ends(
    api: &ApiClient,
    tid: Option<Uuid>,
    ends: [Option<Uuid>; 2],
) -> HashMap<Uuid, PersonSearchSummary> {
    let Some(tid) = tid else {
        return HashMap::new();
    };
    // Both at once: neither waits for the other.
    let profiles = futures_util::future::join_all(
        ends.into_iter()
            .flatten()
            .map(|id| async move { api.get_person_profile(tid, id).await.ok() }),
    )
    .await;
    profiles
        .into_iter()
        .flatten()
        .map(|profile| (profile.person_id, PersonSearchSummary::from(profile)))
        .collect()
}

/// Page rendered at `/trees/:tree_id/kinship?from=...&to=...`.
#[component]
pub fn Kinship(tree_id: String, from: String, to: String) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let nav = use_navigator();
    let load_trace = use_ui_load_trace(UiPage::Kinship);

    // Signals kept in sync with the props: the router reuses this component
    // when only the query changes, e.g. after a swap.
    let tree_id_parsed = use_signal(|| tree_id.parse::<Uuid>().ok());
    let from_parsed = use_signal(|| from.parse::<Uuid>().ok());
    let to_parsed = use_signal(|| to.parse::<Uuid>().ok());
    // The path shown under the summary; back to the closest one whenever
    // either person changes.
    let mut selected = use_signal(|| 0_usize);
    for (mut signal, raw) in [
        (tree_id_parsed, &tree_id),
        (from_parsed, &from),
        (to_parsed, &to),
    ] {
        let parsed = raw.parse::<Uuid>().ok();
        if parsed != *signal.peek() {
            *signal.write() = parsed;
            *selected.write() = 0;
        }
    }

    let page = use_tree_page(&tree_id);

    // The two ends, shown before and while the paths load.
    let api_ends = api.clone();
    let ends_resource = use_traced_resource(load_trace.clone(), "kinship_ends", move || {
        let api = api_ends.clone();
        let (tid, from, to) = (tree_id_parsed(), from_parsed(), to_parsed());
        async move { load_ends(&api, tid, [from, to]).await }
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
        // Once, after the result: its rows carry every portrait it draws,
        // the two ends' included. Without a result — the same person twice,
        // or a failure — only the ends, asked by id.
        let wanted = match &*kinship_resource.read() {
            None => None,
            Some(Some(Ok(kinship))) => Some(Ok(kinship.persons.clone())),
            Some(_) => Some(Err(sorted_unique(
                [from_parsed(), to_parsed()].into_iter().flatten(),
            ))),
        };
        async move {
            let (Some(tid), Some(wanted)) = (tid, wanted) else {
                return HashMap::new();
            };
            match wanted {
                Ok(rows) => api.entry_portraits(tid, &rows).await,
                Err(ends) => api.portrait_map_for_ids(tid, &ends).await,
            }
        }
    });

    let ends = ends_resource.read().clone().unwrap_or_default();
    let portraits = portraits_resource.read().clone().unwrap_or_default();

    let go = {
        let tree_id = tree_id.clone();
        move |from: Option<Uuid>, to: Option<Uuid>| {
            let id = |id: Option<Uuid>| id.map(|id| id.to_string()).unwrap_or_default();
            nav.replace(Route::Kinship {
                tree_id: tree_id.clone(),
                from: id(from),
                to: id(to),
            });
        }
    };
    let pick = |end: End| {
        let go = go.clone();
        move |id: Uuid| match end {
            End::From => go(Some(id), to_parsed()),
            End::To => go(from_parsed(), Some(id)),
        }
    };
    let (on_pick_from, on_pick_to) = (pick(End::From), pick(End::To));
    let on_swap = move |_| {
        if let (Some(from), Some(to)) = (from_parsed(), to_parsed()) {
            go(Some(to), Some(from));
        }
    };

    rsx! {
        ToolPageFrame {
            tree_id: tree_id.clone(),
            tree_name: page.name(),
            title: i18n.t("kinship.breadcrumb"),
            selected_person_id: from_parsed(),
            content_class: "kin-content",
            div { class: "card kin-ends",
                {end_slot(EndSlot {
                    label: i18n.t("kinship.from"),
                    id: from_parsed(),
                    on_pick: EventHandler::new(on_pick_from),
                    tree_id: tree_id_parsed(),
                    route_tree_id: &tree_id,
                    ends: &ends,
                    portraits: &portraits,
                    i18n: &i18n,
                })}
                button {
                    class: "btn btn-outline btn-sm kin-swap",
                    title: i18n.t("kinship.swap"),
                    aria_label: i18n.t("kinship.swap"),
                    disabled: from_parsed().is_none() || to_parsed().is_none(),
                    onclick: on_swap,
                    "\u{21C4}"
                }
                {end_slot(EndSlot {
                    label: i18n.t("kinship.to"),
                    id: to_parsed(),
                    on_pick: EventHandler::new(on_pick_to),
                    tree_id: tree_id_parsed(),
                    route_tree_id: &tree_id,
                    ends: &ends,
                    portraits: &portraits,
                    i18n: &i18n,
                })}
            }

            {kinship_status(
                kinship_resource.read().as_ref(),
                (from_parsed(), to_parsed()),
                selected,
                &portraits,
                &tree_id,
                &i18n,
            )}
        }
    }
}

/// The relationship found between the two ends, or where the search
/// stands: waiting for both, the same person twice, loading, failed.
fn kinship_status(
    report: Option<&Option<Result<KinshipReport, ApiError>>>,
    ends: (Option<Uuid>, Option<Uuid>),
    selected: Signal<usize>,
    portraits: &HashMap<Uuid, CroppedSource>,
    tree_id: &str,
    i18n: &I18n,
) -> Element {
    match report {
        _ if ends.0.is_none() || ends.1.is_none() => rsx! {
            p { class: "text-muted kin-status", {i18n.t("kinship.pick_target")} }
        },
        _ if ends.0 == ends.1 => rsx! {
            p { class: "text-muted kin-status", {i18n.t("kinship.same_person")} }
        },
        None | Some(None) => rsx! {
            div { class: "loading kin-status", {i18n.t("kinship.loading")} }
        },
        Some(Some(Err(error))) => rsx! {
            div { class: "error-msg kin-status",
                {i18n.t_args("kinship.error", &[("error", &error.to_string())])}
            }
        },
        Some(Some(Ok(report))) => render_report(report, selected, portraits, tree_id, i18n),
    }
}

/// One of the two persons.
#[derive(Clone, Copy, PartialEq, Eq)]
enum End {
    From,
    To,
}

/// What one of the two ends needs to draw itself.
struct EndSlot<'a> {
    label: String,
    id: Option<Uuid>,
    on_pick: EventHandler<Uuid>,
    tree_id: Option<Uuid>,
    route_tree_id: &'a str,
    ends: &'a HashMap<Uuid, PersonSearchSummary>,
    portraits: &'a HashMap<Uuid, CroppedSource>,
    i18n: &'a I18n,
}

/// One of the two ends: the shared person picker, the person drawn as the
/// shared row leading to their profile.
fn end_slot(slot: EndSlot<'_>) -> Element {
    let i18n = slot.i18n;
    let on_pick = slot.on_pick;
    let row = slot.id.map(|id| {
        let summary = slot
            .ends
            .get(&id)
            .cloned()
            .unwrap_or_else(|| PersonSearchSummary::placeholder(id, String::new()));
        person_link(
            &summary,
            slot.portraits.get(&id).cloned(),
            slot.route_tree_id,
            "",
            i18n,
        )
    });
    rsx! {
        div { class: "kin-end-slot",
            div { class: "kin-end-label", "{slot.label}" }
            if let Some(tid) = slot.tree_id {
                PersonPicker {
                    tree_id: tid,
                    selected: row,
                    search_placeholder: i18n.t("kinship.choose"),
                    change_label: i18n.t("kinship.change"),
                    on_change: move |person: Option<Uuid>| {
                        if let Some(person) = person {
                            on_pick.call(person);
                        }
                    },
                }
            }
        }
    }
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
    mut selected: Signal<usize>,
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
    let current = selected().min(report.paths.len() - 1);
    let generations_hint = i18n.t_args(
        "kinship.generations_hint",
        &[("from", &view.name(view.from)), ("to", &view.name(view.to))],
    );
    rsx! {
        p { class: "kin-status",
            {i18n.t_plural("kinship.found", report.paths.len())}
            if report.truncated {
                " — "
                {i18n.t("kinship.truncated")}
            }
        }
        if report.paths.len() > 1 {
            nav { class: "card kin-summary", aria_label: i18n.t("kinship.summary"),
                for (index, path) in report.paths.iter().enumerate() {
                    {
                        let heading = view.heading(path);
                        let (up, down) = path.segments.iter().fold((0, 0), |(up, down), segment| {
                            (up + segment.from_line.len(), down + segment.to_line.len())
                        });
                        let generations = i18n.t_args(
                            "kinship.generations",
                            &[("up", &up.to_string()), ("down", &down.to_string())],
                        );
                        let unions = path.segments.len() - 1;
                        let via = view.via(path);
                        rsx! {
                            button {
                                class: if index == current { "kin-summary-row active" } else { "kin-summary-row" },
                                aria_pressed: index == current,
                                onclick: move |_| selected.set(index),
                                span { class: "kin-path-num", "{index + 1}" }
                                span { class: "kin-summary-title", "{heading.title}" }
                                span { class: "kin-summary-gen", title: "{generations_hint}",
                                    "{generations}"
                                    if unions > 0 {
                                        " · "
                                        {i18n.t_plural("kinship.unions", unions)}
                                    }
                                }
                                if let Some(via) = via {
                                    span { class: "kin-summary-via", "{via}" }
                                }
                            }
                        }
                    }
                }
            }
        }
        {view.path(current, &report.paths[current])}
    }
}

/// How a path is titled.
struct Heading {
    title: String,
    /// The relations step by step, for a path through unions.
    chain: Option<String>,
    /// Related through one parent only, when no word says so already.
    half: bool,
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

    fn heading(&self, path: &KinshipPath) -> Heading {
        match path.segments.as_slice() {
            [segment] => Heading {
                title: relation_label(
                    segment.from_line.len(),
                    segment.to_line.len(),
                    self.sex(Some(self.to)),
                    segment.half,
                    self.i18n,
                )
                .unwrap_or_default(),
                chain: None,
                half: segment.half && (segment.from_line.len(), segment.to_line.len()) != (1, 1),
            },
            segments => Heading {
                title: self.i18n.t("kinship.by_marriage"),
                chain: Some(self.chain(segments)),
                half: false,
            },
        }
    }

    /// The common ancestors of a blood relationship, by name.
    fn via(&self, path: &KinshipPath) -> Option<String> {
        let [segment] = path.segments.as_slice() else {
            return None;
        };
        if segment.from_line.is_empty() || segment.to_line.is_empty() {
            // A direct line: the ancestor is one of the two persons.
            return None;
        }
        let names: Vec<String> = segment
            .ancestor_ids
            .iter()
            .map(|&id| self.name(id))
            .filter(|name| !name.is_empty())
            .collect();
        (!names.is_empty()).then(|| names.join(&format!(" {} ", self.i18n.t("common.and"))))
    }

    fn path(&self, index: usize, path: &KinshipPath) -> Element {
        let i18n = self.i18n;
        let Heading { title, chain, half } = self.heading(path);

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
        relation_label(up, down, sex, false, &I18n::new(Language::En)).unwrap()
    }

    fn fr(up: usize, down: usize, sex: Sex) -> String {
        relation_label(up, down, sex, false, &I18n::new(Language::Fr)).unwrap()
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
        let en = I18n::new(Language::En);
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
