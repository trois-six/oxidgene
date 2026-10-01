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
use crate::components::modal::Modal;
use crate::components::search_person::{PersonSearchSummary, render_person_search_summary};
use crate::i18n::use_i18n;
use crate::ui_observability::{UiCommand, trace_ui_action, use_ui_resource};

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
    /// The homonym chosen when the picker opens, so its button merges into
    /// them. Unset, it opens on keeping the person apart. A caller that
    /// already asks the user which record to keep — the duplicates tool —
    /// passes that record, merging being its question.
    #[props(default)]
    pub preselected: Option<Uuid>,
    pub on_decided: EventHandler<HomonymDecision>,
}

/// Merges `person_id` into `target`, or without one records them distinct
/// from every homonym; the key of the error message on failure.
async fn decide(
    api: &ApiClient,
    tree_id: Uuid,
    person_id: Uuid,
    target: Option<Uuid>,
    homonym_ids: &[Uuid],
) -> Result<HomonymDecision, &'static str> {
    match target {
        Some(kept) => trace_ui_action(
            UiCommand::Merge,
            api.merge_persons(tree_id, kept, person_id, &Default::default()),
        )
        .await
        .map(|_| HomonymDecision::Merged(kept))
        .map_err(|_| "homonym.merge_failed"),
        None => trace_ui_action(
            UiCommand::MarkDistinct,
            api.mark_persons_distinct(tree_id, person_id, homonym_ids),
        )
        .await
        .map(|()| HomonymDecision::Distinct)
        .map_err(|_| "homonym.distinct_failed"),
    }
}

/// A homonym row's class: by sex, and marked when chosen.
fn row_class(sex: Sex, active: bool) -> String {
    let sex = match sex {
        Sex::Male => " male",
        Sex::Female => " female",
        Sex::Unknown => "",
    };
    let active = if active { " is-active" } else { "" };
    format!("search-person-result td-suggest-row{sex}{active}")
}

/// A drop-down of the homonyms, and the button that acts on the choice.
#[component]
pub fn HomonymPicker(props: HomonymPickerProps) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let mut open = use_signal(|| false);
    let preselected = props
        .preselected
        .filter(|id| props.homonyms.iter().any(|e| e.person_id == *id));
    let mut chosen = use_signal(|| preselected);
    // Props are not reactive: a caller that changes the pre-selection (the
    // record to keep) moves the choice along with it.
    let mut last_preselected = use_signal(|| preselected);
    if *last_preselected.peek() != preselected {
        last_preselected.set(preselected);
        chosen.set(preselected);
    }
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);

    let tree_id = props.tree_id;
    let person_id = props.person_id;
    let homonym_ids: Vec<Uuid> = props.homonyms.iter().map(|e| e.person_id).collect();
    // Only a homonym offered can be merged into: a choice the list no longer
    // holds reads as keeping the person apart, never as a merge into
    // someone not shown.
    let valid_ids = homonym_ids.clone();
    let chosen_now = move || chosen().filter(|id| valid_ids.contains(id));

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
    let selected = chosen_now().and_then(|id| rows.iter().find(|row| row.person_id() == id));
    let merging = selected.is_some();

    let [separate_label, separate_hint, keep_label] = if props.new_person {
        [
            "homonym.option_new",
            "homonym.option_new_hint",
            "homonym.keep_new",
        ]
    } else {
        [
            "homonym.option_distinct",
            "homonym.option_distinct_hint",
            "homonym.keep_distinct",
        ]
    }
    .map(|key| i18n.t(key));

    let on_decided = props.on_decided;
    let on_confirm = move |_| {
        let api = api.clone();
        let homonym_ids = homonym_ids.clone();
        let target = chosen_now();
        spawn(async move {
            busy.set(true);
            error.set(None);
            let outcome = decide(&api, tree_id, person_id, target, &homonym_ids).await;
            busy.set(false);
            match outcome {
                Ok(decision) => on_decided.call(decision),
                Err(key) => error.set(Some(i18n.t(key))),
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
                onkeydown: move |e: Event<KeyboardData>| close_list_on_escape(&e, open),
                button {
                    class: "search-person-result homonym-select-trigger",
                    "aria-haspopup": "listbox",
                    "aria-expanded": open().to_string(),
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
                            "aria-selected": chosen().is_none().to_string(),
                            onclick: move |_| {
                                chosen.set(None);
                                open.set(false);
                            },
                            {separate_option}
                        }
                        for row in rows.iter() {
                            button {
                                key: "{row.person_id()}",
                                class: row_class(row.sex(), chosen() == Some(row.person_id())),
                                role: "option",
                                "aria-selected": (chosen() == Some(row.person_id())).to_string(),
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

            if merging {
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
                    class: if merging { "btn btn-danger" } else { "btn btn-primary" },
                    disabled: busy(),
                    onclick: on_confirm,
                    if busy() {
                        span { class: "btn-spinner" }
                    }
                    if merging {
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
        Modal {
            class: "modal-card homonym-card",
            label: i18n.t("homonym.title"),
            close_on_backdrop: false,
            on_close: on_later,
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

/// Escape closes the open list first, and only then the dialog around it.
fn close_list_on_escape(e: &KeyboardEvent, mut open: Signal<bool>) {
    if e.key() == Key::Escape && open() {
        e.stop_propagation();
        open.set(false);
    }
}
