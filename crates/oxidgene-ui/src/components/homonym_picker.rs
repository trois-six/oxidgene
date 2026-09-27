//! Asks whether a person is one of the others bearing their name.
//!
//! Raised after a person is created or edited, and on the receipt of a
//! Geneanet import for the people it created from identifications outside the
//! tree. The choices are the homonyms, drawn as the quick-search suggestion
//! rows, plus "a different person". Picking a homonym merges the person into
//! them; keeping them apart records that they differ from every homonym
//! listed, so the question is not asked again for those pairs.

use dioxus::prelude::*;
use oxidgene_core::Sex;
use oxidgene_core::projection::SearchEntry;
use uuid::Uuid;

use crate::api::ApiClient;
use crate::components::search_person::{PersonSearchSummary, render_person_search_summary};
use crate::i18n::use_i18n;
use crate::ui_observability::use_ui_resource;

/// What the user decided about a person's homonyms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HomonymDecision {
    /// Recorded as different from every homonym listed.
    Distinct,
    /// Merged into this homonym, who is kept.
    Merged(Uuid),
}

#[derive(Props, Clone, PartialEq)]
pub struct HomonymPickerProps {
    pub tree_id: Uuid,
    /// The person the question is about.
    pub person_id: Uuid,
    /// The other persons bearing their name, as the homonyms endpoint lists
    /// them.
    pub homonyms: Vec<SearchEntry>,
    /// Word "keep apart" as keeping a new person: the import's case, where
    /// the person exists only because of the identification that named them.
    #[props(default)]
    pub new_person: bool,
    /// When set, offer to answer later instead; the question then comes back
    /// the next time the person is saved.
    #[props(default)]
    pub on_later: Option<EventHandler<()>>,
    pub on_decided: EventHandler<HomonymDecision>,
}

/// A drop-down of the homonyms, and the button that acts on the choice.
#[component]
pub fn HomonymPicker(props: HomonymPickerProps) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let mut open = use_signal(|| false);
    let mut chosen = use_signal(|| None::<Uuid>);
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);

    let tree_id = props.tree_id;
    let person_id = props.person_id;
    let homonym_ids: Vec<Uuid> = props.homonyms.iter().map(|e| e.person_id).collect();

    let api_portraits = api.clone();
    let portrait_ids = homonym_ids.clone();
    let portraits_resource = use_ui_resource("homonym_portraits", move || {
        let api = api_portraits.clone();
        let ids = portrait_ids.clone();
        async move { api.portrait_map_for_ids(tree_id, &ids).await }
    });
    let portraits = portraits_resource.read().clone().unwrap_or_default();

    let rows: Vec<PersonSearchSummary> = props
        .homonyms
        .iter()
        .map(PersonSearchSummary::from)
        .collect();
    let selected = chosen().and_then(|id| rows.iter().find(|row| row.person_id() == id));

    let (separate_label, separate_hint, keep_label) = if props.new_person {
        (
            i18n.t("homonym.option_new"),
            i18n.t("homonym.option_new_hint"),
            i18n.t("homonym.keep_new"),
        )
    } else {
        (
            i18n.t("homonym.option_distinct"),
            i18n.t("homonym.option_distinct_hint"),
            i18n.t("homonym.keep_distinct"),
        )
    };

    let on_decided = props.on_decided;
    let on_confirm = move |_| {
        let api = api.clone();
        let homonym_ids = homonym_ids.clone();
        let target = chosen();
        spawn(async move {
            busy.set(true);
            error.set(None);
            let outcome = match target {
                Some(kept) => api
                    .merge_persons(tree_id, kept, person_id)
                    .await
                    .map(|_| HomonymDecision::Merged(kept))
                    .map_err(|_| i18n.t("homonym.merge_failed")),
                None => api
                    .mark_persons_distinct(tree_id, person_id, &homonym_ids)
                    .await
                    .map(|()| HomonymDecision::Distinct)
                    .map_err(|_| i18n.t("homonym.distinct_failed")),
            };
            busy.set(false);
            match outcome {
                Ok(decision) => on_decided.call(decision),
                Err(message) => error.set(Some(message)),
            }
        });
    };

    let separate_option = rsx! {
        div { class: "sp-result-photo",
            span { class: "homonym-separate-icon", "+" }
        }
        div { class: "sp-result-info",
            div { class: "sp-result-name", "{separate_label}" }
            div { class: "sp-result-rel", "{separate_hint}" }
        }
    };

    rsx! {
        div { class: "homonym-picker",
            div {
                class: "homonym-select",
                onkeydown: move |e: Event<KeyboardData>| {
                    if e.key() == Key::Escape {
                        open.set(false);
                    }
                },
                button {
                    class: "search-person-result homonym-select-trigger",
                    "aria-haspopup": "listbox",
                    "aria-expanded": if open() { "true" } else { "false" },
                    disabled: busy(),
                    onclick: move |_| open.toggle(),
                    match selected {
                        Some(row) => render_person_search_summary(
                            row,
                            portraits.get(&row.person_id()).cloned(),
                            &i18n,
                        ),
                        None => separate_option.clone(),
                    }
                    span { class: "homonym-select-caret", "aria-hidden": "true", "\u{25BE}" }
                }
                if open() {
                    div { class: "homonym-select-list", role: "listbox",
                        button {
                            class: if chosen().is_none() {
                                "search-person-result td-suggest-row is-active"
                            } else {
                                "search-person-result td-suggest-row"
                            },
                            role: "option",
                            "aria-selected": if chosen().is_none() { "true" } else { "false" },
                            onclick: move |_| {
                                chosen.set(None);
                                open.set(false);
                            },
                            {separate_option}
                        }
                        for row in rows.iter() {
                            button {
                                key: "{row.person_id()}",
                                class: {
                                    let sex = match row.sex() {
                                        Sex::Male => " male",
                                        Sex::Female => " female",
                                        Sex::Unknown => "",
                                    };
                                    let active = if chosen() == Some(row.person_id()) {
                                        " is-active"
                                    } else {
                                        ""
                                    };
                                    format!("search-person-result td-suggest-row{sex}{active}")
                                },
                                role: "option",
                                "aria-selected": if chosen() == Some(row.person_id()) { "true" } else { "false" },
                                onclick: {
                                    let id = row.person_id();
                                    move |_| {
                                        chosen.set(Some(id));
                                        open.set(false);
                                    }
                                },
                                {render_person_search_summary(
                                    row,
                                    portraits.get(&row.person_id()).cloned(),
                                    &i18n,
                                )}
                            }
                        }
                    }
                }
            }

            if chosen().is_some() {
                p { class: "homonym-warning", {i18n.t("homonym.merge_warning")} }
            }
            if let Some(message) = error() {
                div { class: "error-msg", "{message}" }
            }

            div { class: "modal-actions",
                if let Some(on_later) = props.on_later {
                    button {
                        class: "btn btn-outline",
                        disabled: busy(),
                        onclick: move |_| on_later.call(()),
                        {i18n.t("homonym.later")}
                    }
                }
                button {
                    class: if chosen().is_some() { "btn btn-danger" } else { "btn btn-primary" },
                    disabled: busy(),
                    onclick: on_confirm,
                    if busy() {
                        span { class: "btn-spinner" }
                    }
                    if chosen().is_some() {
                        {i18n.t("homonym.merge")}
                    } else {
                        "{keep_label}"
                    }
                }
            }
        }
    }
}

/// The picker in a dialog, for a person that has just been saved.
#[component]
pub fn HomonymDialog(
    tree_id: Uuid,
    person_id: Uuid,
    /// How the person is called, as the form saved it.
    person_name: String,
    homonyms: Vec<SearchEntry>,
    on_later: EventHandler<()>,
    on_decided: EventHandler<HomonymDecision>,
) -> Element {
    let i18n = use_i18n();
    let key = i18n.plural_key("homonym.message", homonyms.len());
    let message = i18n.t_args(
        &key,
        &[
            ("name", &person_name),
            ("count", &homonyms.len().to_string()),
        ],
    );

    rsx! {
        // No dismissal on the backdrop: leaving without an answer is what
        // "decide later" is for, and a stray press must not stand for it.
        div { class: "modal-backdrop",
            div {
                class: "modal-card homonym-card",
                role: "dialog",
                "aria-modal": "true",
                h3 { {i18n.t("homonym.title")} }
                p { "{message}" }
                HomonymPicker {
                    tree_id,
                    person_id,
                    homonyms,
                    on_later,
                    on_decided,
                }
            }
        }
    }
}
