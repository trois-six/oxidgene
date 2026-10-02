//! Topbar last-name/first-name search bar with a live suggestion panel.
//!
//! The two fields still submit to [`Route::SearchResults`], and still take a
//! SOSA number in place of a surname. What they add is a panel under the
//! fields, in two levels: first the tree's names completing the field being
//! typed, as the entry forms suggest them, then the matching persons, so the
//! common case — "I know roughly who I am looking for" — never needs the full
//! results page.
//!
//! Lives in its own component so signal updates on each keystroke only
//! re-render this small widget, not the whole page.

use dioxus::prelude::*;
use uuid::Uuid;

use crate::api::{ApiClient, NameScope, PersonSearchParams, PersonSearchSort, SuggestionField};
use crate::components::context_menu::ContextMenuSurface;
use crate::components::search_person::{
    PersonSearchSummary, render_person_search_summary, summary_portraits,
};
use crate::components::suggest_input::{
    picked_text, render_suggest_row, suggest_rows, suggestions_shown, use_value_suggestions,
};
use crate::i18n::use_i18n;
use crate::router::Route;
use crate::ui_observability::use_ui_resource;

/// Suggestions shown at once. Enough to recognise the person, few enough that
/// the panel stays a glance rather than a page; the footer leads to the rest.
const SUGGESTION_LIMIT: u32 = 6;

/// Names completing the field being typed, listed above the persons.
const NAME_SUGGESTION_LIMIT: usize = 5;

/// Keystrokes settle for this long before a request goes out.
const DEBOUNCE_MS: u32 = 200;

/// Shortest input worth a query. One character matches most of a tree.
const MIN_QUERY_LEN: usize = 2;

/// What the current field contents mean.
#[derive(Clone, Debug, PartialEq)]
enum Intent {
    /// Nothing worth querying yet.
    Idle,
    /// A bare number in the surname field, with no given name: the surname
    /// field doubles as a SOSA box, so resolve it as one.
    Sosa(u64),
    /// A name search, as `(surname, given_names)`.
    Name(String, String),
}

/// Classify the field contents.
///
/// Extracted so the rules — what is too short to query, and when a number
/// means a SOSA rather than a name — are testable without a renderer.
fn classify(last: &str, first: &str) -> Intent {
    let last = last.trim();
    let first = first.trim();

    if first.is_empty()
        && !last.is_empty()
        && let Ok(number) = last.parse::<u64>()
    {
        return Intent::Sosa(number);
    }

    if last.chars().count() + first.chars().count() < MIN_QUERY_LEN {
        return Intent::Idle;
    }

    Intent::Name(last.to_owned(), first.to_owned())
}

#[component]
pub fn TopbarSearch(
    tree_id: String,
    /// Whether this search bar lives on the person-detail page: results then
    /// navigate back to [`Route::PersonDetail`] instead of the pedigree view.
    #[props(default = false)]
    from_person: bool,
    /// Field state, when the parent has to share it with other controls.
    ///
    /// The results page shows the same surname and given-name fields a second
    /// time in its filter panel, and typing in either place must update both.
    /// Left unset, the component owns its own state and starts empty.
    #[props(default)]
    last: Option<Signal<String>>,
    #[props(default)] first: Option<Signal<String>>,
    /// When set, submitting calls this with `(last, first)` instead of
    /// navigating. The results page searches in place, so it has nowhere to
    /// navigate to; every other mount site leaves this unset.
    #[props(default)]
    on_submit: Option<EventHandler<(String, String)>>,
) -> Element {
    let i18n = use_i18n();
    let nav = use_navigator();
    let api = use_context::<ApiClient>();
    // The hooks run unconditionally — they must, to keep the hook order
    // stable — and are simply unused when the parent supplies its own.
    let local_last = use_signal(String::new);
    let local_first = use_signal(String::new);
    let mut search_last = last.unwrap_or(local_last);
    let mut search_first = first.unwrap_or(local_first);

    // Panel state. `anchor` is the field group's measured bottom-right corner:
    // `.td-topbar` clips its overflow, so the panel cannot be positioned
    // inside it and is placed as a fixed overlay instead.
    let mut open = use_signal(|| false);
    let mut highlight = use_signal(|| None::<usize>);
    let mut anchor = use_signal(|| (0.0_f64, 0.0_f64));
    // The group is kept so the corner can be re-measured: the topbar is a flex
    // row sized from the tree name, so the fields move when the window does.
    let mut anchor_el = use_signal(|| None::<std::rc::Rc<MountedData>>);

    let remeasure = use_callback(move |()| {
        let Some(element) = anchor_el() else {
            return;
        };
        spawn(async move {
            if let Ok(rect) = element.get_client_rect().await {
                anchor.set((
                    rect.origin.x + rect.size.width,
                    rect.origin.y + rect.size.height + 4.0,
                ));
            }
        });
    });

    let tid = Uuid::parse_str(&tree_id).ok();

    // ── Suggestions ──
    let mut debounced = use_signal(|| (String::new(), String::new()));
    let _debounce = use_ui_resource("topbar_search_debounce", move || {
        let raw = (search_last(), search_first());
        async move {
            crate::utils::sleep_ms(DEBOUNCE_MS).await;
            debounced.set(raw);
        }
    });

    // Only for an open panel. On the results page the fields arrive filled
    // from the URL, and looking them up unasked sent the page's own search a
    // second time on every load, for suggestions nobody was shown.
    let api_suggest = api.clone();
    let suggestions = use_ui_resource("topbar_search_suggest", move || {
        let api = api_suggest.clone();
        let (last, first) = debounced();
        let wanted = open();
        async move { suggest_persons(&api, tid.filter(|_| wanted)?, &last, &first).await }
    });

    let (rows, total_count, sosa_number) = match &*suggestions.read() {
        Some(Some((rows, total, sosa))) => (rows.clone(), *total, *sosa),
        _ => (Vec::new(), 0, None),
    };

    let api_portraits = api.clone();
    let portraits_resource = use_ui_resource("topbar_search_portraits", move || {
        let api = api_portraits.clone();
        let rows = match &*suggestions.read() {
            Some(Some((rows, _, _))) => rows.clone(),
            _ => Vec::new(),
        };
        async move {
            let tid = tid?;
            Some(summary_portraits(&api, tid, &rows).await)
        }
    });
    let portraits = match &*portraits_resource.read() {
        Some(Some(map)) => map.clone(),
        _ => Default::default(),
    };

    // ── Names ──
    //
    // What each field last had typed into it; only the field being typed in
    // holds any, so only its names are listed. Empty until the user types, so
    // fields the results page fills in ask for nothing.
    let mut typed_last = use_signal(String::new);
    let mut typed_first = use_signal(String::new);
    // Each field's names count the persons the other field also finds, so a
    // name's count is what the persons below would read once it is picked.
    let with_first = use_memo(move || NameScope {
        given_names: search_first(),
        ..NameScope::default()
    });
    let with_last = use_memo(move || NameScope {
        surname: search_last(),
        ..NameScope::default()
    });
    let last_values = use_value_suggestions(
        tid,
        SuggestionField::FamilyNames,
        typed_last,
        NAME_SUGGESTION_LIMIT,
        with_first,
    );
    let first_values = use_value_suggestions(
        tid,
        SuggestionField::GivenNames,
        typed_first,
        NAME_SUGGESTION_LIMIT,
        with_last,
    );
    let (name_field, names) = typed_names(
        (&typed_last(), &typed_first()),
        last_values.read().as_deref(),
        first_values.read().as_deref(),
    );
    let name_rows = suggest_rows(&names, name_field, &i18n);
    let name_count = name_rows.len();

    // Picking a name completes its field and keeps the panel open, so the
    // persons below narrow down to it.
    let pick_name = use_callback(move |index: usize| {
        let Some(picked) = names.get(index) else {
            return;
        };
        let mut field = match name_field {
            SuggestionField::FamilyNames => search_last,
            _ => search_first,
        };
        let text = picked_text(name_field, &field(), &picked.value);
        field.set(text);
        typed_last.set(String::new());
        typed_first.set(String::new());
        highlight.set(None);
    });

    let panel_visible = open() && name_count + rows.len() > 0;

    // ── Navigation ──
    let go_to_person = use_callback({
        let tree_id = tree_id.clone();
        move |person_id: Uuid| {
            let tree_id = tree_id.clone();
            let person_id = person_id.to_string();
            open.set(false);
            highlight.set(None);
            nav.push(person_route(tree_id, person_id, from_person));
        }
    });

    let show_all = use_callback({
        let tree_id = tree_id.clone();
        let api = api.clone();
        move |()| {
            let last = search_last();
            let first = search_first();
            if last.trim().is_empty() && first.trim().is_empty() {
                return;
            }
            open.set(false);
            highlight.set(None);

            if let Some(on_submit) = on_submit {
                on_submit.call((last, first));
                return;
            }

            let origin = if from_person { "person" } else { "" }.to_string();

            // A bare number is tried as a SOSA-Stradonitz number first — jump
            // straight to that person, falling back to a name search when the
            // tree has no SOSA root or nobody sits at that number.
            let results = Route::SearchResults {
                tree_id: tree_id.clone(),
                last: last.clone(),
                first: first.clone(),
                origin,
            };
            let (Intent::Sosa(number), Some(tid)) = (classify(&last, &first), tid) else {
                nav.push(results);
                return;
            };
            let api = api.clone();
            let tree_id = tree_id.clone();
            spawn(async move {
                let found = api.get_person_by_sosa(tid, number).await;
                nav.push(match found {
                    Ok(person) => person_route(tree_id, person.id.to_string(), from_person),
                    Err(_) => results,
                });
            });
        }
    });

    // ── Keyboard ──
    let keys = PanelKeys {
        open,
        highlight,
        name_count,
        row_ids: rows.iter().map(PersonSearchSummary::person_id).collect(),
        pick_name,
        go_to_person,
        show_all,
    };
    let on_key = use_callback(move |e: Event<KeyboardData>| keys.handle(&e));

    rsx! {
        div {
            class: "td-search-group",
            // Measured rather than assumed: the group sits at the end of a
            // flex row whose other items size themselves from the tree name.
            onmounted: move |e: Event<MountedData>| {
                anchor_el.set(Some(e.data()));
                remeasure.call(());
            },
            input {
                r#type: "text",
                class: "td-search-input",
                placeholder: "{i18n.t(\"tree.search_last\")}",
                value: "{search_last}",
                oninput: move |e: Event<FormData>| {
                    search_last.set(e.value());
                    typed_last.set(e.value());
                    typed_first.set(String::new());
                    open.set(true);
                    highlight.set(None);
                    remeasure.call(());
                },
                // Names complete the field being typed in, not the one left.
                onfocus: move |_| typed_first.set(String::new()),
                onkeydown: move |e| on_key.call(e),
            }
            input {
                r#type: "text",
                class: "td-search-input",
                placeholder: "{i18n.t(\"tree.search_first\")}",
                value: "{search_first}",
                oninput: move |e: Event<FormData>| {
                    search_first.set(e.value());
                    typed_first.set(e.value());
                    typed_last.set(String::new());
                    open.set(true);
                    highlight.set(None);
                    remeasure.call(());
                },
                onfocus: move |_| typed_last.set(String::new()),
                onkeydown: move |e| on_key.call(e),
            }
            button {
                class: "td-search-btn",
                title: "{i18n.t(\"tree.search\")}",
                onclick: move |_| show_all.call(()),
                svg {
                    width: "14",
                    height: "14",
                    fill: "none",
                    "viewBox": "0 0 24 24",
                    stroke: "currentColor",
                    "strokeWidth": "2.5",
                    circle { cx: "11", cy: "11", r: "8" }
                    line { x1: "21", y1: "21", x2: "16.65", y2: "16.65" }
                }
            }

            if panel_visible {
                ContextMenuSurface {
                    x: anchor().0,
                    y: anchor().1,
                    menu_class: "context-menu-anchor-right td-suggest".to_string(),
                    on_close: move |()| {
                        open.set(false);
                        highlight.set(None);
                    },
                    {suggest_panel(SuggestPanel {
                        name_rows: &name_rows,
                        rows: &rows,
                        portraits: &portraits,
                        sosa_number,
                        total_count,
                        highlight,
                        on_name: pick_name,
                        on_person: go_to_person,
                        on_more: show_all,
                    }, &i18n)}
                }
            }
        }
    }
}

/// The field being typed in, and the names completing it: none until one
/// is typed in. A search criterion no record carries would find nobody, so
/// only names the tree holds are listed.
fn typed_names(
    (typed_last, typed_first): (&str, &str),
    last_values: Option<&[crate::api::ValueSuggestion]>,
    first_values: Option<&[crate::api::ValueSuggestion]>,
) -> (SuggestionField, Vec<crate::api::ValueSuggestion>) {
    let (field, values) = if typed_last.is_empty() {
        (SuggestionField::GivenNames, first_values)
    } else {
        (SuggestionField::FamilyNames, last_values)
    };
    if typed_last.is_empty() && typed_first.is_empty() {
        return (field, Vec::new());
    }
    (
        field,
        suggestions_shown(values.unwrap_or_default(), false, true),
    )
}

/// What the keys do in the fields.
///
/// The names and the persons are one list for the arrows, names first.
/// Enter on a highlighted name completes its field, on a highlighted person
/// opens that person; Enter with nothing highlighted keeps the behaviour the
/// bar has always had.
#[derive(Clone)]
struct PanelKeys {
    open: Signal<bool>,
    highlight: Signal<Option<usize>>,
    name_count: usize,
    row_ids: Vec<Uuid>,
    pick_name: Callback<usize>,
    go_to_person: Callback<Uuid>,
    show_all: Callback<()>,
}

impl PanelKeys {
    fn handle(&self, e: &Event<KeyboardData>) {
        let (mut open, mut highlight) = (self.open, self.highlight);
        let row_count = self.name_count + self.row_ids.len();
        match e.key() {
            Key::Enter => self.enter(highlight().filter(|&index| open() && index < row_count)),
            Key::Escape => {
                open.set(false);
                highlight.set(None);
            }
            // A closed panel has nothing loaded yet: the first press opens
            // it, which fetches the suggestions, and the next ones move
            // through them.
            Key::ArrowDown if row_count == 0 => {
                e.prevent_default();
                open.set(true);
            }
            Key::ArrowDown | Key::ArrowUp if row_count > 0 => {
                e.prevent_default();
                open.set(true);
                let down = e.key() == Key::ArrowDown;
                highlight.set(Some(step_highlight(highlight(), row_count, down)));
            }
            _ => {}
        }
    }

    /// Enter, on the row `highlighted` if any.
    fn enter(&self, highlighted: Option<usize>) {
        match highlighted {
            Some(index) if index < self.name_count => self.pick_name.call(index),
            Some(index) => self
                .go_to_person
                .call(self.row_ids[index - self.name_count]),
            None => self.show_all.call(()),
        }
    }
}

/// The persons the fields find: the one at a SOSA number — the same lookup
/// Enter performs, so the panel previews exactly where Enter lands — or the
/// best matches of a name search; with how many there are, and the number.
async fn suggest_persons(
    api: &ApiClient,
    tid: Uuid,
    last: &str,
    first: &str,
) -> Option<(Vec<PersonSearchSummary>, usize, Option<u64>)> {
    match classify(last, first) {
        Intent::Idle => None,
        Intent::Sosa(number) => {
            let person = api.get_person_by_sosa(tid, number).await.ok()?;
            let profile = api.get_person_profile(tid, person.id).await.ok()?;
            Some((vec![PersonSearchSummary::from(profile)], 1, Some(number)))
        }
        Intent::Name(last, first) => {
            let params = PersonSearchParams {
                limit: SUGGESTION_LIMIT,
                surname: Some(last).filter(|last| !last.is_empty()),
                given_names: Some(first).filter(|first| !first.is_empty()),
                sort: PersonSearchSort::Relevance,
                ..Default::default()
            };
            let result = api.search_persons_filtered(tid, &params).await.ok()?;
            let summaries = result
                .entries
                .iter()
                .map(PersonSearchSummary::from)
                .collect();
            Some((summaries, result.total_count, None))
        }
    }
}

/// Where a person found leads: their profile from the person page, else
/// the pedigree around them.
fn person_route(tree_id: String, person_id: String, from_person: bool) -> Route {
    if from_person {
        Route::PersonDetail { tree_id, person_id }
    } else {
        Route::TreeDetail {
            tree_id,
            person: Some(person_id),
        }
    }
}

/// The row an arrow key moves the highlight to among `count`, going round.
fn step_highlight(current: Option<usize>, count: usize, down: bool) -> usize {
    match (current, down) {
        (Some(index), true) if index + 1 < count => index + 1,
        (_, true) => 0,
        (Some(0) | None, false) => count - 1,
        (Some(index), false) => index - 1,
    }
}

/// What the suggestion panel lists and does.
struct SuggestPanel<'a> {
    name_rows: &'a [crate::components::suggest_input::SuggestRow],
    rows: &'a [PersonSearchSummary],
    portraits: &'a std::collections::HashMap<Uuid, crate::api::CroppedSource>,
    sosa_number: Option<u64>,
    total_count: usize,
    highlight: Signal<Option<usize>>,
    on_name: Callback<usize>,
    on_person: Callback<Uuid>,
    on_more: Callback<()>,
}

/// The names completing the field, then the persons, then a way to all the
/// results when there are more; one highlight runs through the lot.
fn suggest_panel(panel: SuggestPanel<'_>, i18n: &crate::i18n::I18n) -> Element {
    let SuggestPanel {
        name_rows,
        rows,
        portraits,
        sosa_number,
        total_count,
        mut highlight,
        on_name,
        on_person,
        on_more,
    } = panel;
    let name_count = name_rows.len();
    let class = |index: usize, base: &str| {
        let active = if highlight() == Some(index) {
            " is-active"
        } else {
            ""
        };
        format!("{base}{active}")
    };
    rsx! {
        if name_count > 0 {
            div { class: "td-suggest-names",
                for (index, row) in name_rows.iter().enumerate() {
                    button {
                        key: "{index}",
                        r#type: "button",
                        class: class(index, "context-menu-item td-suggest-row suggest-input-row"),
                        // Keep the focus in the field, to go on typing.
                        onmousedown: move |e: Event<MouseData>| e.prevent_default(),
                        onclick: move |_| on_name.call(index),
                        onmouseenter: move |_| highlight.set(Some(index)),
                        {render_suggest_row(row, i18n)}
                    }
                }
            }
        }
        for (index, row) in rows.iter().enumerate().map(|(i, row)| (name_count + i, row)) {
            button {
                key: "{row.person_id()}",
                class: class(index, "search-person-result td-suggest-row"),
                onclick: {
                    let id = row.person_id();
                    move |_| on_person.call(id)
                },
                onmouseenter: move |_| highlight.set(Some(index)),
                {render_person_search_summary(row, portraits.get(&row.person_id()).cloned(), i18n)}
                if let Some(number) = sosa_number {
                    span { class: "td-suggest-sosa",
                        {i18n.t_args("search.sosa_badge", &[("number", &number.to_string())])}
                    }
                }
            }
        }
        // Only worth offering when there is more to see than the panel
        // already shows.
        if total_count > rows.len() {
            button {
                class: "td-suggest-more",
                onclick: move |_| on_more.call(()),
                {i18n.t_args("search.see_all_results", &[("count", &total_count.to_string())])}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_number_without_a_given_name_is_a_sosa() {
        // The surname field doubles as a SOSA box, which is what its
        // placeholder advertises.
        assert_eq!(classify("8", ""), Intent::Sosa(8));
        assert_eq!(classify("  12  ", " "), Intent::Sosa(12));
    }

    #[test]
    fn a_number_beside_a_given_name_is_a_name() {
        // Someone typing a given name is searching names, whatever the other
        // field holds.
        assert_eq!(
            classify("8", "Pierre"),
            Intent::Name("8".into(), "Pierre".into())
        );
    }

    #[test]
    fn too_little_input_queries_nothing() {
        // One character matches most of a tree, so it is not worth a request.
        assert_eq!(classify("", ""), Intent::Idle);
        assert_eq!(classify("e", ""), Intent::Idle);
        assert_eq!(classify("", "p"), Intent::Idle);
        assert_eq!(classify("er", ""), Intent::Name("er".into(), String::new()));
        assert_eq!(
            classify("e", "p"),
            Intent::Name("e".into(), "p".into()),
            "the two fields count together"
        );
    }

    #[test]
    fn accents_count_as_one_character_each() {
        // Counted in `char`s, not bytes: "ér" is two characters and three
        // bytes, and must not be treated as long enough on byte length alone.
        assert_eq!(classify("é", ""), Intent::Idle);
        assert_eq!(classify("ér", ""), Intent::Name("ér".into(), String::new()));
    }
}
