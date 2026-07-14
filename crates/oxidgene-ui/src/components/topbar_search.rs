//! Topbar last-name/first-name search bar, navigating to [`Route::SearchResults`].
//!
//! Lives in its own component so signal updates on each keystroke only
//! re-render this small widget, not the whole page.

use dioxus::prelude::*;
use uuid::Uuid;

use crate::api::ApiClient;
use crate::i18n::use_i18n;
use crate::router::Route;

#[component]
pub fn TopbarSearch(
    tree_id: String,
    /// Whether this search bar lives on the person-detail page: results then
    /// navigate back to [`Route::PersonDetail`] instead of the pedigree view.
    #[props(default = false)]
    from_person: bool,
) -> Element {
    let i18n = use_i18n();
    let nav = use_navigator();
    let api = use_context::<ApiClient>();
    let mut search_last = use_signal(String::new);
    let mut search_first = use_signal(String::new);

    let do_search = {
        let tree_id = tree_id.clone();
        let api = api.clone();
        move || {
            let last = search_last();
            let first = search_first();
            let last_trim = last.trim();
            let first_trim = first.trim();
            if last_trim.is_empty() && first_trim.is_empty() {
                return;
            }
            let origin = if from_person {
                "person".to_string()
            } else {
                String::new()
            };

            // If a SOSA root is set for the tree and the search is
            // family-name-only *and* numeric, try it as a SOSA-Stradonitz
            // number first — jump straight to the matching person, falling
            // back to a normal name search if no person exists at that
            // number (or the tree has no SOSA root).
            if first_trim.is_empty()
                && let Ok(number) = last_trim.parse::<u64>()
                && let Ok(tid) = Uuid::parse_str(&tree_id)
            {
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
    };

    let do_search2 = do_search.clone();
    let do_search3 = do_search.clone();
    let on_search_enter = move |e: Event<KeyboardData>| {
        if e.key() == Key::Enter {
            do_search();
        }
    };
    let on_search_enter2 = move |e: Event<KeyboardData>| {
        if e.key() == Key::Enter {
            do_search2();
        }
    };
    let on_search_btn = move |_| {
        do_search3();
    };

    rsx! {
        div { class: "td-search-group",
            input {
                r#type: "text",
                class: "td-search-input",
                placeholder: "{i18n.t(\"tree.search_last\")}",
                value: "{search_last}",
                oninput: move |e: Event<FormData>| search_last.set(e.value()),
                onkeydown: on_search_enter,
            }
            input {
                r#type: "text",
                class: "td-search-input",
                placeholder: "{i18n.t(\"tree.search_first\")}",
                value: "{search_first}",
                oninput: move |e: Event<FormData>| search_first.set(e.value()),
                onkeydown: on_search_enter2,
            }
            button {
                class: "td-search-btn",
                title: "{i18n.t(\"tree.search\")}",
                onclick: on_search_btn,
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
        }
    }
}
