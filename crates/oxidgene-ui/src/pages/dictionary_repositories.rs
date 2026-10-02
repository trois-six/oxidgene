//! The Dictionary's Repositories tab: every archive, library or office of
//! the tree holding sources, each row unfolding into the sources it holds,
//! editable, and a button adding one. See `docs/ui-dictionary.md`.

use dioxus::prelude::*;
use oxidgene_core::types::Repository;
use uuid::Uuid;

use crate::api::{ApiClient, HeldSource};
use crate::components::source_forms::{RepositoryEditor, medium_label};
use crate::i18n::{I18n, use_i18n};
use crate::ui_observability::use_ui_resource;

/// Which editor is open: a repository's, or the one creating a repository.
#[derive(Clone, PartialEq)]
enum Editing {
    New,
    Existing(Repository),
}

/// The repository's address, phone and website on one line.
fn repository_meta(repository: &Repository) -> String {
    let first_address_line = repository
        .address
        .as_deref()
        .and_then(|a| a.lines().map(str::trim).find(|l| !l.is_empty()))
        .map(str::to_string);
    [
        first_address_line,
        repository.phone.clone(),
        repository.website.clone(),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" \u{00B7} ")
}

/// "Title — call number · medium": a held source as the unfolded row reads
/// it.
fn held_label(i18n: &I18n, held: &HeldSource) -> String {
    let detail = [
        held.link.call_number.clone(),
        held.link.media_type.map(|m| medium_label(i18n, m)),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" \u{00B7} ");
    if detail.is_empty() {
        held.source.title.clone()
    } else {
        format!("{} \u{2014} {detail}", held.source.title)
    }
}

#[component]
pub fn DictionaryRepositories(tree_id: Uuid) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let mut quick = use_signal(String::new);
    let mut expanded = use_signal(|| None::<Uuid>);
    let mut editing = use_signal(|| None::<Editing>);

    let api_list = api.clone();
    let mut repositories = use_ui_resource("repositories", move || {
        let api = api_list.clone();
        async move { api.list_all_repositories(tree_id).await }
    });
    let api_held = api.clone();
    let held = use_ui_resource("repository_sources", move || {
        let api = api_held.clone();
        let open = expanded();
        async move {
            let Some(id) = open else {
                return (None, Vec::new());
            };
            (
                Some(id),
                api.repository_sources(tree_id, id)
                    .await
                    .unwrap_or_default(),
            )
        }
    });

    let filter = quick().trim().to_lowercase();
    let listed: Option<Vec<Repository>> = match &*repositories.read() {
        Some(Ok(list)) => {
            let mut list: Vec<Repository> = list
                .iter()
                .filter(|r| filter.is_empty() || r.name.to_lowercase().contains(&filter))
                .cloned()
                .collect();
            list.sort_by_cached_key(|r| r.name.to_lowercase());
            Some(list)
        }
        _ => None,
    };
    let failed = matches!(&*repositories.read(), Some(Err(_)));

    rsx! {
        div { class: "dict-filter-row",
            input {
                r#type: "text",
                class: "dict-filter-input",
                placeholder: "{i18n.t(\"dictionary.filter_placeholder\")}",
                value: "{quick}",
                oninput: move |e: Event<FormData>| quick.set(e.value()),
            }
            button {
                class: "pf-add-btn",
                r#type: "button",
                onclick: move |_| editing.set(Some(Editing::New)),
                {i18n.t("dictionary.repository.add")}
            }
        }
        match listed {
            None if failed => rsx! { div { class: "empty-state", {i18n.t("dictionary.error")} } },
            None => rsx! { div { class: "empty-state", {i18n.t("dictionary.loading")} } },
            Some(list) if list.is_empty() => rsx! {
                div { class: "empty-state", {i18n.t("dictionary.no_entries_repositories")} }
            },
            Some(list) => rsx! {
                div { class: "dict-src-summary", {i18n.t_plural("dictionary.count", list.len())} }
                div { class: "dict-list",
                    for repository in list.into_iter() {
                        {
                            let id = repository.id;
                            let is_open = expanded() == Some(id);
                            let meta = repository_meta(&repository);
                            let edited = repository.clone();
                            rsx! {
                                div { key: "{id}",
                                    div {
                                        class: "dict-row",
                                        onclick: move |_| expanded.set((!is_open).then_some(id)),
                                        div { class: "dict-row-main",
                                            span { class: "dict-row-value", "{repository.name}" }
                                            if !meta.is_empty() {
                                                span { class: "dict-row-meta", "{meta}" }
                                            }
                                        }
                                        button {
                                            class: "dict-row-action",
                                            title: "{i18n.t(\"common.edit\")}",
                                            onclick: move |e: Event<MouseData>| {
                                                e.stop_propagation();
                                                editing.set(Some(Editing::Existing(edited.clone())));
                                            },
                                            "\u{270E}"
                                        }
                                        button {
                                            class: "dict-row-action",
                                            title: "{i18n.t(\"dictionary.repository.view_sources\")}",
                                            if is_open { "\u{25B2}" } else { "\u{25BC}" }
                                        }
                                    }
                                    if is_open {
                                        {render_held(i18n, id, &held.read())}
                                    }
                                }
                            }
                        }
                    }
                }
            },
        }
        if let Some(edit) = editing() {
            RepositoryEditor {
                key: "{edit_key(&edit)}",
                tree_id,
                repository: match edit {
                    Editing::New => None,
                    Editing::Existing(repository) => Some(repository),
                },
                on_close: move |()| editing.set(None),
                on_saved: move |()| {
                    editing.set(None);
                    repositories.restart();
                },
            }
        }
    }
}

/// The key the editor is mounted under, so another repository's opens fresh.
fn edit_key(edit: &Editing) -> String {
    match edit {
        Editing::New => "new".to_string(),
        Editing::Existing(repository) => repository.id.to_string(),
    }
}

/// The sources an unfolded repository holds — once they were read for it.
fn render_held(i18n: I18n, id: Uuid, held: &Option<(Option<Uuid>, Vec<HeldSource>)>) -> Element {
    match held {
        Some((Some(for_id), list)) if *for_id == id && !list.is_empty() => rsx! {
            div { class: "dict-accordion",
                for entry in list.iter() {
                    div { key: "{entry.link.id}", class: "dict-accordion-item",
                        span { class: "dict-accordion-name", {held_label(&i18n, entry)} }
                    }
                }
            }
        },
        Some((Some(for_id), _)) if *for_id == id => rsx! {
            div { class: "dict-accordion",
                div { class: "dict-accordion-empty", {i18n.t("dictionary.repository.no_sources")} }
            }
        },
        _ => rsx! {
            div { class: "dict-accordion",
                div { class: "dict-accordion-empty", {i18n.t("common.loading")} }
            }
        },
    }
}
