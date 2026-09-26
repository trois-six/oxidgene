//! Topbar last-name/first-name search bar with a live suggestion panel.
//!
//! The two fields still submit to [`Route::SearchResults`], and still take a
//! SOSA number in place of a surname. What they add is a panel of matching
//! persons under the fields, so the common case — "I know roughly who I am
//! looking for" — never needs the full results page.
//!
//! Lives in its own component so signal updates on each keystroke only
//! re-render this small widget, not the whole page.

use dioxus::prelude::*;
use uuid::Uuid;

use crate::api::{ApiClient, PersonSearchParams, PersonSearchSort};
use crate::components::context_menu::ContextMenuSurface;
use crate::components::search_person::{PersonSearchSummary, render_person_search_summary};
use crate::i18n::use_i18n;
use crate::router::Route;
use crate::ui_observability::use_ui_resource;

/// Suggestions shown at once. Enough to recognise the person, few enough that
/// the panel stays a glance rather than a page; the footer leads to the rest.
const SUGGESTION_LIMIT: u32 = 6;

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
        if let Some(element) = anchor_el() {
            spawn(async move {
                if let Ok(rect) = element.get_client_rect().await {
                    anchor.set((
                        rect.origin.x + rect.size.width,
                        rect.origin.y + rect.size.height + 4.0,
                    ));
                }
            });
        }
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
        async move {
            let tid = tid.filter(|_| wanted)?;
            match classify(&last, &first) {
                Intent::Idle => None,
                // Resolving the number is the same lookup Enter already
                // performs, so the panel previews exactly where Enter lands.
                Intent::Sosa(number) => {
                    let person = api.get_person_by_sosa(tid, number).await.ok()?;
                    let profile = api.get_person_profile(tid, person.id).await.ok()?;
                    Some((vec![PersonSearchSummary::from(profile)], 1, Some(number)))
                }
                Intent::Name(last, first) => {
                    let params = PersonSearchParams {
                        limit: SUGGESTION_LIMIT,
                        surname: (!last.is_empty()).then_some(last),
                        given_names: (!first.is_empty()).then_some(first),
                        sort: PersonSearchSort::Relevance,
                        ..Default::default()
                    };
                    let result = api.search_persons_filtered(tid, &params).await.ok()?;
                    let summaries = result
                        .entries
                        .iter()
                        .map(PersonSearchSummary::from)
                        .collect::<Vec<_>>();
                    Some((summaries, result.total_count, None))
                }
            }
        }
    });

    let (rows, total_count, sosa_number) = match &*suggestions.read() {
        Some(Some((rows, total, sosa))) => (rows.clone(), *total, *sosa),
        _ => (Vec::new(), 0, None),
    };

    let person_ids: Vec<Uuid> = rows.iter().map(PersonSearchSummary::person_id).collect();
    let api_portraits = api.clone();
    let portraits_resource = use_ui_resource("topbar_search_portraits", move || {
        let api = api_portraits.clone();
        let person_ids = person_ids.clone();
        async move {
            let tid = tid?;
            Some(api.portrait_map_for_ids(tid, &person_ids).await)
        }
    });
    let portraits = match &*portraits_resource.read() {
        Some(Some(map)) => map.clone(),
        _ => Default::default(),
    };

    let panel_visible = open() && !rows.is_empty();

    // ── Navigation ──
    let go_to_person = use_callback({
        let tree_id = tree_id.clone();
        move |person_id: Uuid| {
            let tree_id = tree_id.clone();
            let person_id = person_id.to_string();
            open.set(false);
            highlight.set(None);
            if from_person {
                nav.push(Route::PersonDetail { tree_id, person_id });
            } else {
                nav.push(Route::TreeDetail {
                    tree_id,
                    person: Some(person_id),
                });
            }
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

            let origin = if from_person {
                "person".to_string()
            } else {
                String::new()
            };

            // A bare number is tried as a SOSA-Stradonitz number first — jump
            // straight to that person, falling back to a name search when the
            // tree has no SOSA root or nobody sits at that number.
            if let (Intent::Sosa(number), Some(tid)) = (classify(&last, &first), tid) {
                let api = api.clone();
                let tree_id = tree_id.clone();
                spawn(async move {
                    match api.get_person_by_sosa(tid, number).await {
                        Ok(person) => {
                            let person_id = person.id.to_string();
                            if from_person {
                                nav.push(Route::PersonDetail { tree_id, person_id });
                            } else {
                                nav.push(Route::TreeDetail {
                                    tree_id,
                                    person: Some(person_id),
                                });
                            }
                        }
                        Err(_) => {
                            nav.push(Route::SearchResults {
                                tree_id,
                                last,
                                first,
                                origin,
                            });
                        }
                    }
                });
                return;
            }

            nav.push(Route::SearchResults {
                tree_id: tree_id.clone(),
                last,
                first,
                origin,
            });
        }
    });

    // ── Keyboard ──
    //
    // Enter on a highlighted suggestion opens that person; Enter with nothing
    // highlighted keeps the behaviour the bar has always had.
    let row_count = rows.len();
    let on_key = use_callback(move |e: Event<KeyboardData>| match e.key() {
        Key::Enter => {
            let highlighted = highlight()
                .filter(|_| open())
                .and_then(|index| match &*suggestions.read() {
                    Some(Some((rows, _, _))) => rows.get(index).map(PersonSearchSummary::person_id),
                    _ => None,
                });
            match highlighted {
                Some(id) => go_to_person.call(id),
                None => show_all.call(()),
            }
        }
        Key::Escape => {
            open.set(false);
            highlight.set(None);
        }
        // A closed panel has nothing loaded yet: the first press opens it,
        // which fetches the suggestions, and the next ones move through them.
        Key::ArrowDown if row_count == 0 => {
            e.prevent_default();
            open.set(true);
        }
        Key::ArrowDown => {
            e.prevent_default();
            open.set(true);
            highlight.set(Some(match highlight() {
                Some(index) if index + 1 < row_count => index + 1,
                Some(_) => 0,
                None => 0,
            }));
        }
        Key::ArrowUp if row_count > 0 => {
            e.prevent_default();
            open.set(true);
            highlight.set(Some(match highlight() {
                Some(0) | None => row_count - 1,
                Some(index) => index - 1,
            }));
        }
        _ => {}
    });

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
                    open.set(true);
                    highlight.set(None);
                    remeasure.call(());
                },
                onkeydown: move |e| on_key.call(e),
            }
            input {
                r#type: "text",
                class: "td-search-input",
                placeholder: "{i18n.t(\"tree.search_first\")}",
                value: "{search_first}",
                oninput: move |e: Event<FormData>| {
                    search_first.set(e.value());
                    open.set(true);
                    highlight.set(None);
                    remeasure.call(());
                },
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
                    for (index, row) in rows.iter().enumerate() {
                        button {
                            key: "{row.person_id()}",
                            class: if highlight() == Some(index) {
                                "search-person-result td-suggest-row is-active"
                            } else {
                                "search-person-result td-suggest-row"
                            },
                            onclick: {
                                let id = row.person_id();
                                move |_| go_to_person.call(id)
                            },
                            onmouseenter: move |_| highlight.set(Some(index)),
                            {render_person_search_summary(
                                row,
                                portraits.get(&row.person_id()).cloned(),
                                &i18n,
                            )}
                            if let Some(number) = sosa_number {
                                span { class: "td-suggest-sosa",
                                    {i18n.t_args("search.sosa_badge", &[("number", &number.to_string())])}
                                }
                            }
                        }
                    }
                    // Only worth offering when there is more to see than the
                    // panel already shows.
                    if total_count > rows.len() {
                        button {
                            class: "td-suggest-more",
                            onclick: move |_| show_all.call(()),
                            {i18n.t_args("search.see_all_results", &[("count", &total_count.to_string())])}
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
