//! The descent of a family name, unfolded under its Dictionary row, and the
//! switch between it and the alphabetical list of the persons carrying the
//! name. See `docs/ui-dictionary.md` §7.2.

use std::collections::{HashMap, HashSet};

use dioxus::prelude::*;
use oxidgene_core::types::{
    DescentBranch, DescentLine, DescentLineKind, DescentPerson, SurnameDescent,
};
use uuid::Uuid;

use crate::api::{ApiClient, ApiError};
use crate::components::pedigree_chart::format_lifespan;
use crate::components::person_profile::{sex_mark, sosa_ring};
use crate::components::tabs::Tabs;
use crate::i18n::{I18n, use_i18n};
use crate::pages::dictionary::reveal_row;
use crate::router::Route;
use crate::ui_observability::{use_ui_resource, use_ui_trace};

/// A line as its branch shows it: at its depth in the branch, and, for the
/// first line of a descent unfolded in place, the bearer it unfolds.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Shown<'a> {
    line: &'a DescentLine,
    depth: u32,
    opens: Option<Uuid>,
}

/// The lines of a branch with the descents unfolded so far put in place of
/// their `more` lines. An unfolded descent starts with its bearer's own line,
/// already shown above the `more` line, so it is left out; a descent already
/// put in place is not put in again, which no answer asks for but which
/// keeps a loop impossible.
fn spliced<'a>(
    lines: &'a [DescentLine],
    unfolded: &'a HashMap<Uuid, Vec<DescentLine>>,
) -> Vec<Shown<'a>> {
    let mut shown = Vec::new();
    let mut seen = HashSet::new();
    splice(lines, unfolded, 0, None, &mut seen, &mut shown);
    shown
}

fn splice<'a>(
    lines: &'a [DescentLine],
    unfolded: &'a HashMap<Uuid, Vec<DescentLine>>,
    offset: u32,
    mut opens: Option<Uuid>,
    seen: &mut HashSet<Uuid>,
    shown: &mut Vec<Shown<'a>>,
) {
    for line in lines {
        let descent = line
            .unfold_person_id
            .filter(|_| line.kind == DescentLineKind::More)
            .and_then(|id| unfolded.get(&id).map(|lines| (id, lines)));
        if let Some((id, descent)) = descent
            && seen.insert(id)
        {
            // The `more` line sits one step below its bearer.
            let base = offset + line.depth.saturating_sub(1);
            splice(
                descent.get(1..).unwrap_or_default(),
                unfolded,
                base,
                Some(id),
                seen,
                shown,
            );
            continue;
        }
        shown.push(Shown {
            line,
            depth: offset + line.depth,
            opens: opens.take(),
        });
    }
}

/// "Given names SURNAME", or the translated placeholder for a nameless one.
fn display_name(i18n: &I18n, person: &DescentPerson) -> String {
    let surname = person.surname.as_deref().map(str::to_uppercase);
    match (person.given_names.as_deref(), surname) {
        (Some(given), Some(surname)) => format!("{given} {surname}"),
        (Some(given), None) => given.to_string(),
        (None, Some(surname)) => surname,
        (None, None) => i18n.t("common.unnamed"),
    }
}

/// The lifespan as a card draws it, a `†` standing for a death known without
/// a year.
fn lifespan(i18n: &I18n, person: &DescentPerson) -> String {
    let text = format_lifespan(i18n.dates(), person.birth, person.death);
    if !person.deceased || person.death.is_some() {
        return text;
    }
    if text.is_empty() || text.ends_with('-') {
        format!("{text}\u{2020}")
    } else {
        format!("{text} \u{2020}")
    }
}

/// A person's marks, name (a link to their page) and lifespan.
fn person_parts(i18n: I18n, tree_id: Uuid, person: &DescentPerson) -> Element {
    let years = lifespan(&i18n, person);
    rsx! {
        span { class: "pd-person-identity",
            if person.sosa_ancestor {
                {sosa_ring()}
            }
            {sex_mark(person.sex)}
            Link {
                to: Route::PersonDetail {
                    tree_id: tree_id.to_string(),
                    person_id: person.person_id.to_string(),
                },
                class: "dict-descent-name",
                {display_name(&i18n, person)}
            }
        }
        if !years.is_empty() {
            span { class: "dict-accordion-dates", "{years}" }
        }
    }
}

/// The union written on a line: `&` and its year, then the spouse.
fn union_parts(i18n: I18n, tree_id: Uuid, line: &DescentLine) -> Element {
    if line.marriage.is_none() && line.spouse.is_none() {
        return rsx! {};
    }
    let year = line.marriage.map(|year| year.wide()).unwrap_or_default();
    rsx! {
        span { class: "dict-descent-union",
            span { title: "{i18n.t(\"dictionary.descent.married\")}", "&{year}" }
            if let Some(spouse) = &line.spouse {
                {person_parts(i18n, tree_id, spouse)}
            }
        }
    }
}

/// What the descent panel works with.
#[derive(Clone, Copy)]
struct LineContext {
    i18n: I18n,
    tree_id: Uuid,
    branch: u32,
    root_has_parents: bool,
    unfolding: Signal<HashSet<Uuid>>,
    failed: Signal<HashSet<Uuid>>,
    on_unfold: EventHandler<Uuid>,
}

/// The `«` link before a root whose parents do not bear the name.
fn parents_link(ctx: LineContext, person: &DescentPerson) -> Element {
    let label = ctx.i18n.t_args(
        "dictionary.descent.parents_of",
        &[("name", &display_name(&ctx.i18n, person))],
    );
    rsx! {
        Link {
            to: Route::TreeDetail {
                tree_id: ctx.tree_id.to_string(),
                person: Some(person.person_id.to_string()),
            },
            class: "dict-descent-up",
            title: "{label}",
            "aria-label": "{label}",
            "\u{00AB}"
        }
    }
}

/// The `more` line of a folded descent: a button unfolding it.
fn more_button(ctx: LineContext, line: &DescentLine) -> Element {
    let Some(person) = line.unfold_person_id else {
        return rsx! {};
    };
    let i18n = ctx.i18n;
    let busy = ctx.unfolding.read().contains(&person);
    let label = if busy {
        i18n.t("common.loading")
    } else if ctx.failed.read().contains(&person) {
        i18n.t("dictionary.descent.retry")
    } else if line.count == 0 {
        i18n.t("dictionary.descent.show_rest")
    } else {
        i18n.t_plural("dictionary.descent.show_more", line.count as usize)
    };
    let on_unfold = ctx.on_unfold;
    rsx! {
        button {
            r#type: "button",
            class: "dict-descent-more",
            disabled: busy,
            onclick: move |_| on_unfold.call(person),
            "{label}"
        }
    }
}

fn render_line(ctx: LineContext, shown: Shown, first: bool) -> Element {
    let Shown { line, depth, opens } = shown;
    let i18n = ctx.i18n;
    let class = match line.kind {
        DescentLineKind::Union => "dict-descent-line is-repeat",
        DescentLineKind::Others | DescentLineKind::Reference => "dict-descent-line is-aside",
        _ => "dict-descent-line",
    };
    let id = opens.map(|person| format!("descent-{person}"));
    let body = match line.kind {
        DescentLineKind::Person | DescentLineKind::Union => {
            let Some(person) = &line.person else {
                return rsx! {};
            };
            rsx! {
                if first && ctx.root_has_parents {
                    {parents_link(ctx, person)}
                }
                {person_parts(i18n, ctx.tree_id, person)}
                {union_parts(i18n, ctx.tree_id, line)}
            }
        }
        DescentLineKind::Others => rsx! {
            span { "aria-hidden": "true", "\u{2026}" }
            span { class: "dict-descent-note",
                {i18n.t_plural("dictionary.descent.others", line.count as usize)}
            }
        },
        DescentLineKind::Reference => {
            let Some(person) = &line.person else {
                return rsx! {};
            };
            let note = match line.branch {
                Some(branch) if branch != ctx.branch => i18n.t_args(
                    "dictionary.descent.see_branch",
                    &[("number", &branch.to_string())],
                ),
                _ => i18n.t("dictionary.descent.see_elsewhere"),
            };
            rsx! {
                {person_parts(i18n, ctx.tree_id, person)}
                span { class: "dict-descent-note", "{note}" }
            }
        }
        DescentLineKind::More => more_button(ctx, line),
    };
    rsx! {
        div {
            class,
            id,
            tabindex: opens.map(|_| "-1"),
            style: "--depth: {depth}",
            {body}
        }
    }
}

fn render_branch(
    ctx: LineContext,
    branch: &DescentBranch,
    unfolded: &HashMap<Uuid, Vec<DescentLine>>,
) -> Element {
    let shown = spliced(&branch.lines, unfolded);
    rsx! {
        li { class: "dict-descent-branch",
            span { class: "dict-descent-number", "{branch.number}." }
            div { class: "dict-descent-lines",
                for (index , line) in shown.into_iter().enumerate() {
                    {render_line(ctx, line, index == 0)}
                }
            }
        }
    }
}

/// The later pages of branches, and whether one is on its way or failed.
#[derive(Clone, Copy)]
struct Pages {
    branches: Signal<Vec<DescentBranch>>,
    busy: Signal<bool>,
    failed: Signal<bool>,
}

/// The descents unfolded in place, by bearer, and those on their way or
/// failed.
#[derive(Clone, Copy)]
struct Unfolding {
    lines: Signal<HashMap<Uuid, Vec<DescentLine>>>,
    busy: Signal<HashSet<Uuid>>,
    failed: Signal<HashSet<Uuid>>,
}

/// An unfolded family-name row: the switch between the descent and the
/// alphabetical list, and the view chosen.
#[component]
pub(crate) fn FamilyNameUnfolded(
    tree_id: Uuid,
    value: String,
    /// The alphabetical list rather than the branches, for every row.
    alphabetical: Signal<bool>,
    /// The alphabetical list, rendered by the page.
    usage: Element,
) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    // Asked for the first time the branches show, then kept.
    let mut wanted = use_signal(|| false);
    if !alphabetical() && !*wanted.peek() {
        wanted.set(true);
    }
    let first_api = api.clone();
    let first_value = value.clone();
    let mut first = use_ui_resource("family_name_descent", move || {
        let api = first_api.clone();
        let value = first_value.clone();
        let wanted = wanted();
        async move {
            if !wanted {
                return None;
            }
            Some(
                api.dictionary_family_name_descent(tree_id, &value, 0, None)
                    .await,
            )
        }
    });
    let branch_count = match &*first.read() {
        Some(Some(Ok(descent))) => Some(descent.branch_count),
        _ => None,
    };
    let branches_label = match branch_count {
        Some(count) => i18n.t_args(
            "dictionary.descent.branches_count",
            &[("count", &count.to_string())],
        ),
        None => i18n.t("dictionary.descent.branches"),
    };
    let body = if alphabetical() {
        usage
    } else {
        rsx! {
            DescentView { tree_id, value: value.clone(), first, on_retry: move |()| first.restart() }
        }
    };
    rsx! {
        div { class: "dict-accordion",
            div { class: "dict-descent-views",
                span { {i18n.t("dictionary.descent.display_by")} }
                Tabs {
                    tabs: vec![(false, branches_label), (true, i18n.t("dictionary.descent.alphabetical"))],
                    current: Some(alphabetical()),
                    on_select: move |choice| alphabetical.set(choice),
                    class: "dict-descent-tabs".to_string(),
                }
            }
            {body}
        }
    }
}

type FirstPage = Resource<Option<Result<SurnameDescent, ApiError>>>;

/// The branches of a family name, as they arrive.
#[component]
fn DescentView(
    tree_id: Uuid,
    value: String,
    first: FirstPage,
    on_retry: EventHandler<()>,
) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let trace = use_ui_trace();
    let pages = Pages {
        branches: use_signal(Vec::new),
        busy: use_signal(|| false),
        failed: use_signal(|| false),
    };
    let unfolding = Unfolding {
        lines: use_signal(HashMap::new),
        busy: use_signal(HashSet::new),
        failed: use_signal(HashSet::new),
    };
    let snapshot = first.read();
    let descent = match &*snapshot {
        Some(Some(Ok(descent))) => descent,
        Some(Some(Err(_))) => {
            return rsx! {
                div { class: "error-msg", {i18n.t("dictionary.descent.error")} }
                button {
                    r#type: "button",
                    class: "dict-descent-more",
                    onclick: move |_| on_retry.call(()),
                    {i18n.t("dictionary.descent.retry")}
                }
            };
        }
        _ => {
            return rsx! {
                div { class: "dict-accordion-empty", {i18n.t("common.loading")} }
            };
        }
    };

    let unfold_api = api.clone();
    let unfold_value = value.clone();
    let unfold_trace = trace.clone();
    let on_unfold = EventHandler::new(move |person: Uuid| {
        let Unfolding {
            mut lines,
            mut busy,
            mut failed,
        } = unfolding;
        if !busy.write().insert(person) {
            return;
        }
        failed.write().remove(&person);
        let api = unfold_api.clone();
        let value = unfold_value.clone();
        let trace = unfold_trace.clone();
        spawn(async move {
            let answer = trace
                .resource(
                    "family_name_descent_unfold",
                    api.dictionary_family_name_descent(tree_id, &value, 0, Some(person)),
                )
                .await;
            busy.write().remove(&person);
            match answer.map(|mut descent| descent.branches.pop()) {
                Ok(Some(branch)) => {
                    lines.write().insert(person, branch.lines);
                    reveal_row(&format!("descent-{person}"));
                }
                _ => {
                    failed.write().insert(person);
                }
            }
        });
    });

    let shown_count = descent.branches.len() + pages.branches.read().len();
    let more_branches = (shown_count as u32) < descent.branch_count;
    let next_offset = descent.offset as usize + shown_count;
    let on_more_branches = move |_| {
        let Pages {
            mut branches,
            mut busy,
            mut failed,
        } = pages;
        if busy() {
            return;
        }
        busy.set(true);
        failed.set(false);
        let api = api.clone();
        let value = value.clone();
        let trace = trace.clone();
        let offset = u32::try_from(next_offset).unwrap_or(u32::MAX);
        spawn(async move {
            let answer = trace
                .resource(
                    "family_name_descent_page",
                    api.dictionary_family_name_descent(tree_id, &value, offset, None),
                )
                .await;
            busy.set(false);
            match answer {
                Ok(descent) => branches.write().extend(descent.branches),
                Err(_) => failed.set(true),
            }
        });
    };

    let unfolded = unfolding.lines.read();
    let later = pages.branches.read();
    let context = |branch: &DescentBranch| LineContext {
        i18n,
        tree_id,
        branch: branch.number,
        root_has_parents: branch.root_has_parents,
        unfolding: unfolding.busy,
        failed: unfolding.failed,
        on_unfold,
    };
    let more_label = if (pages.busy)() {
        i18n.t("common.loading")
    } else if (pages.failed)() {
        i18n.t("dictionary.descent.retry")
    } else {
        i18n.t("dictionary.descent.more_branches")
    };
    rsx! {
        if descent.bearer_count == 0 {
            div { class: "dict-accordion-empty",
                if descent.by_marriage.is_empty() {
                    {i18n.t("dictionary.usage_empty")}
                } else {
                    {i18n.t("dictionary.descent.no_bearer")}
                }
            }
        }
        ol { class: "dict-descent-branches",
            for branch in descent.branches.iter().chain(later.iter()) {
                {render_branch(context(branch), branch, &unfolded)}
            }
        }
        if more_branches {
            button {
                r#type: "button",
                class: "dict-descent-more",
                disabled: (pages.busy)(),
                onclick: on_more_branches,
                "{more_label}"
            }
        }
        if !descent.by_marriage.is_empty() {
            div { class: "dict-descent-apart",
                h4 { {i18n.t("dictionary.descent.by_marriage")} }
                for person in descent.by_marriage.iter() {
                    div { key: "{person.person_id}", class: "dict-descent-line",
                        {person_parts(i18n, tree_id, person)}
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxidgene_core::enums::Sex;

    fn person(id: Uuid) -> DescentPerson {
        DescentPerson {
            person_id: id,
            sex: Sex::Unknown,
            given_names: None,
            surname: None,
            birth: None,
            death: None,
            deceased: false,
            sosa_ancestor: false,
        }
    }

    fn line(kind: DescentLineKind, depth: u32, id: Uuid) -> DescentLine {
        let mut line = DescentLine::new(kind, depth);
        if kind == DescentLineKind::More {
            line.unfold_person_id = Some(id);
        } else {
            line.person = Some(person(id));
        }
        line
    }

    fn depths(shown: &[Shown]) -> Vec<(DescentLineKind, u32, bool)> {
        shown
            .iter()
            .map(|s| (s.line.kind, s.depth, s.opens.is_some()))
            .collect()
    }

    #[test]
    fn an_unfolded_descent_takes_the_place_of_its_more_line() {
        let (root, son, grandson) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
        let lines = vec![
            line(DescentLineKind::Person, 0, root),
            line(DescentLineKind::Person, 1, son),
            line(DescentLineKind::More, 2, son),
        ];
        let mut unfolded = HashMap::new();
        // The son's descent, from the son at depth 0.
        unfolded.insert(
            son,
            vec![
                line(DescentLineKind::Person, 0, son),
                line(DescentLineKind::Person, 1, grandson),
                line(DescentLineKind::More, 2, grandson),
            ],
        );
        let shown = spliced(&lines, &unfolded);
        assert_eq!(
            depths(&shown),
            [
                (DescentLineKind::Person, 0, false),
                (DescentLineKind::Person, 1, false),
                (DescentLineKind::Person, 2, true),
                (DescentLineKind::More, 3, false),
            ]
        );
        assert_eq!(shown[2].opens, Some(son));
    }

    #[test]
    fn a_descent_is_put_in_place_once() {
        let son = Uuid::now_v7();
        let lines = vec![
            line(DescentLineKind::Person, 0, son),
            line(DescentLineKind::More, 1, son),
        ];
        let mut unfolded = HashMap::new();
        unfolded.insert(
            son,
            vec![
                line(DescentLineKind::Person, 0, son),
                line(DescentLineKind::More, 1, son),
            ],
        );
        let shown = spliced(&lines, &unfolded);
        assert_eq!(
            depths(&shown),
            [
                (DescentLineKind::Person, 0, false),
                (DescentLineKind::More, 1, true),
            ]
        );
    }

    #[test]
    fn a_death_without_a_year_shows_a_dagger() {
        let i18n = I18n::new(crate::i18n::Language::english());
        let mut someone = person(Uuid::now_v7());
        someone.deceased = true;
        assert_eq!(lifespan(&i18n, &someone), "\u{2020}");
        someone.birth = Some(oxidgene_core::types::QualifiedYear::new(
            1646,
            oxidgene_core::enums::DateQualifier::Exact,
        ));
        assert_eq!(lifespan(&i18n, &someone), "1646-\u{2020}");
    }
}
