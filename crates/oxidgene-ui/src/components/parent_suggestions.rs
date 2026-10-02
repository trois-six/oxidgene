//! The persons of the tree a parent being created may already be
//! (`docs/ui-person-edit-modal.md` §12): offered while their name is typed,
//! when the tree suggests existing persons.

use dioxus::prelude::*;
use oxidgene_core::enums::Sex;
use uuid::Uuid;

use crate::api::ApiClient;
use crate::components::person_form::ParentLink;
use crate::components::search_person::{PersonSearchSummary, render_person_search_summary};
use crate::components::tree_cache::use_tree_cache;
use crate::i18n::use_i18n;
use crate::ui_observability::use_ui_resource;

/// How many matching persons the panel offers.
const SUGGESTED: u32 = 5;

/// The tree's persons matching the name typed in the form — but the child,
/// the parents the family already has, and anyone of the other sex — each
/// with a button linking them as the parent instead of creating one. Nothing when
/// the tree does not suggest existing persons, when nothing matches, or once
/// dismissed; the form stays usable either way.
#[component]
pub(crate) fn ParentSuggestions(
    tree_id: Uuid,
    link: ParentLink,
    surname: Signal<String>,
    given_names: Signal<String>,
    /// Fired once an existing person is linked: the form has nothing left
    /// to create.
    on_linked: EventHandler<()>,
) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let enabled = use_tree_cache().suggest_persons();
    let mut dismissed = use_signal(|| false);
    let mut linking = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);

    // Debounced, as every field that asks the backend while one types.
    let mut query = use_signal(String::new);
    use_ui_resource("parent_suggestions_debounce", move || {
        let typed = format!("{} {}", surname(), given_names())
            .trim()
            .to_string();
        async move {
            crate::utils::sleep_ms(300).await;
            query.set(typed);
        }
    });

    // The child and the parents already in the family are no candidates.
    let api_excluded = api.clone();
    let excluded = use_ui_resource("parent_suggestions_excluded", move || {
        let api = api_excluded.clone();
        async move {
            let mut excluded = vec![link.child_id];
            if let Some(fid) = link.family_id
                && let Ok(spouses) = api.list_family_spouses(tree_id, fid).await
            {
                excluded.extend(spouses.into_iter().map(|spouse| spouse.person_id));
            }
            excluded
        }
    });

    let api_search = api.clone();
    let found = use_ui_resource("parent_suggestions", move || {
        let api = api_search.clone();
        let q = query();
        async move {
            if !enabled || q.is_empty() {
                return (Vec::new(), Default::default());
            }
            let entries = api
                .search_persons(tree_id, &q, SUGGESTED, 0)
                .await
                .map(|result| result.entries)
                .unwrap_or_default();
            let portraits = api.entry_portraits(tree_id, &entries).await;
            (entries, portraits)
        }
    });

    let excluded = excluded.read().clone().unwrap_or_default();
    let (entries, portraits) = found.read().clone().unwrap_or_default();
    // A father is no woman, a mother no man; an unknown sex may be either.
    let other_sex = if link.is_father {
        Sex::Female
    } else {
        Sex::Male
    };
    let candidates: Vec<PersonSearchSummary> = entries
        .iter()
        .filter(|entry| !excluded.contains(&entry.person_id) && entry.sex != other_sex)
        .map(PersonSearchSummary::from)
        .collect();
    if !enabled || dismissed() || candidates.is_empty() {
        return rsx! {};
    }

    let pick = move |person_id: Uuid| {
        let api = api.clone();
        linking.set(true);
        error.set(None);
        spawn(async move {
            match link.link(&api, tree_id, person_id, &i18n).await {
                Ok(()) => on_linked.call(()),
                Err(message) => error.set(Some(message)),
            }
            linking.set(false);
        });
    };

    rsx! {
        div { class: "pf-parent-suggestions",
            div { class: "pf-parent-suggestions-head",
                span { {i18n.t("person_form.suggest_title")} }
                button {
                    class: "person-form-close",
                    r#type: "button",
                    title: i18n.t("person_form.suggest_dismiss"),
                    "aria-label": i18n.t("person_form.suggest_dismiss"),
                    onclick: move |_| dismissed.set(true),
                    "\u{00D7}"
                }
            }
            for summary in candidates {
                div { key: "{summary.person_id()}", class: "pf-parent-suggestion",
                    div { class: "person-picker-person",
                        {render_person_search_summary(&summary, portraits.get(&summary.person_id()).cloned(), &i18n)}
                    }
                    button {
                        class: "btn btn-outline btn-sm",
                        r#type: "button",
                        disabled: linking(),
                        onclick: {
                            let mut pick = pick.clone();
                            let person_id = summary.person_id();
                            move |_| pick(person_id)
                        },
                        {i18n.t("person_form.suggest_link")}
                    }
                }
            }
            if let Some(message) = error() {
                div { class: "error-msg", "{message}" }
            }
            p { class: "text-muted pf-parent-suggestions-foot", {i18n.t("person_form.suggest_footer")} }
        }
    }
}
