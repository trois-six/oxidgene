//! The source and repository editors, opened from the Dictionary: a source's
//! bibliographic fields with, behind "More details", its agency and the
//! repositories holding it; and a repository's name, address and contact
//! details. See `docs/ui-dictionary.md`.
//!
//! A source's repository links are their own writes, made the moment one is
//! added or removed — like an event's witnesses — while the fields above wait
//! for Save.

use dioxus::prelude::*;
use oxidgene_core::SourceMediaType;
use oxidgene_core::types::{Repository, Source, SourceRepository};
use uuid::Uuid;

use crate::api::{
    AddSourceRepositoryBody, ApiClient, ApiError, CreateNoteBody, CreateRepositoryBody,
    UpdateNoteBody, UpdateRepositoryBody, UpdateSourceBody,
};
use crate::components::modal::Modal;
use crate::components::person_form::{DeleteSection, MoreDetails};
use crate::i18n::{I18n, use_i18n};
use crate::ui_observability::use_ui_resource;
use crate::utils::opt_str;

/// The value of the repository picker's "new repository" choice.
const NEW_REPOSITORY: &str = "new";

/// What a medium is called.
pub(crate) fn medium_label(i18n: &I18n, medium: SourceMediaType) -> String {
    i18n.t(&format!("media.medium.{}", medium.as_str()))
}

/// "Name — call number · medium": a link as a row reads it.
fn link_label(i18n: &I18n, name: &str, link: &SourceRepository) -> String {
    let detail = [
        link.call_number.clone(),
        link.media_type.map(|m| medium_label(i18n, m)),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" \u{00B7} ");
    if detail.is_empty() {
        name.to_string()
    } else {
        format!("{name} \u{2014} {detail}")
    }
}

/// The text of a field, `None` when blank.
fn field(text: &str) -> Option<String> {
    opt_str(text.trim())
}

/// What the source editor loads: the source, its links, and every
/// repository of the tree for the picker.
#[derive(Clone, PartialEq)]
struct SourceState {
    source: Source,
    links: Vec<SourceRepository>,
    repositories: Vec<Repository>,
}

async fn load_source_state(
    api: &ApiClient,
    tree_id: Uuid,
    source_id: Uuid,
) -> Result<SourceState, ApiError> {
    Ok(SourceState {
        source: api.get_source(tree_id, source_id).await?,
        links: api.source_repositories(tree_id, source_id).await?,
        repositories: api.list_all_repositories(tree_id).await?,
    })
}

/// Edits a source.
#[component]
pub fn SourceEditor(
    tree_id: Uuid,
    source_id: Uuid,
    on_close: EventHandler<()>,
    on_saved: EventHandler<()>,
) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let mut title = use_signal(String::new);
    let mut author = use_signal(String::new);
    let mut publisher = use_signal(String::new);
    let mut abbreviation = use_signal(String::new);
    let mut agency = use_signal(String::new);
    let mut more = use_signal(|| false);
    let mut loaded = use_signal(|| false);
    let mut saving = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);

    let api_load = api.clone();
    let mut state = use_ui_resource("source_editor", move || {
        let api = api_load.clone();
        async move { load_source_state(&api, tree_id, source_id).await }
    });

    if !loaded()
        && let Some(Ok(loaded_state)) = &*state.read()
    {
        let source = &loaded_state.source;
        title.set(source.title.clone());
        author.set(source.author.clone().unwrap_or_default());
        publisher.set(source.publisher.clone().unwrap_or_default());
        abbreviation.set(source.abbreviation.clone().unwrap_or_default());
        agency.set(source.agency.clone().unwrap_or_default());
        // Open from the start when it holds something.
        more.set(source.agency.is_some() || !loaded_state.links.is_empty());
        loaded.set(true);
    }

    let save = move |_| {
        let api = api.clone();
        let body = UpdateSourceBody {
            title: Some(title().trim().to_string()),
            author: Some(field(&author())),
            publisher: Some(field(&publisher())),
            abbreviation: Some(field(&abbreviation())),
            agency: Some(field(&agency())),
        };
        if title().trim().is_empty() {
            error.set(Some(i18n.t("dictionary.source.title_required")));
            return;
        }
        saving.set(true);
        error.set(None);
        spawn(async move {
            match api.update_source(tree_id, source_id, &body).await {
                Ok(_) => on_saved.call(()),
                Err(e) => {
                    saving.set(false);
                    error.set(Some(e.to_string()));
                }
            }
        });
    };

    let loaded_state = state.read().as_ref().and_then(|s| s.as_ref().ok()).cloned();
    rsx! {
        Modal {
            class: "dict-edit-modal is-wide",
            label: i18n.t("dictionary.source.title"),
            busy: saving(),
            on_close,
            div { class: "dict-edit-header",
                h2 { {i18n.t("dictionary.source.title")} }
                button { class: "person-form-close", onclick: move |_| on_close.call(()), "✕" }
            }
            match &loaded_state {
                None => rsx! { div { class: "loading", {i18n.t("common.loading")} } },
                Some(loaded_state) => rsx! {
                    div { class: "form-group",
                        label { {i18n.t("dictionary.source.field_title")} }
                        input {
                            r#type: "text",
                            value: "{title}",
                            oninput: move |e: Event<FormData>| title.set(e.value()),
                        }
                    }
                    div { class: "form-row",
                        div { class: "form-group",
                            label { {i18n.t("dictionary.source.author")} }
                            input {
                                r#type: "text",
                                value: "{author}",
                                oninput: move |e: Event<FormData>| author.set(e.value()),
                            }
                        }
                        div { class: "form-group",
                            label { {i18n.t("dictionary.source.abbreviation")} }
                            input {
                                r#type: "text",
                                value: "{abbreviation}",
                                oninput: move |e: Event<FormData>| abbreviation.set(e.value()),
                            }
                        }
                    }
                    div { class: "form-group",
                        label { {i18n.t("dictionary.source.publisher")} }
                        textarea {
                            rows: 2,
                            value: "{publisher}",
                            oninput: move |e: Event<FormData>| publisher.set(e.value()),
                        }
                    }
                    MoreDetails { open: more,
                        div { class: "form-group",
                            label { {i18n.t("person_form.agency")} }
                            input {
                                r#type: "text",
                                value: "{agency}",
                                oninput: move |e: Event<FormData>| agency.set(e.value()),
                            }
                        }
                        SourceRepositories {
                            tree_id,
                            source_id,
                            links: loaded_state.links.clone(),
                            repositories: loaded_state.repositories.clone(),
                            on_changed: move |()| state.restart(),
                        }
                    }
                },
            }
            if let Some(err) = error() {
                div { class: "error-msg", "{err}" }
            }
            div { class: "modal-actions",
                button { class: "td-btn", onclick: move |_| on_close.call(()), {i18n.t("common.cancel")} }
                button {
                    class: "td-btn td-btn-primary",
                    disabled: saving() || !loaded(),
                    onclick: save,
                    if saving() { {i18n.t("common.saving")} } else { {i18n.t("common.save")} }
                }
            }
        }
    }
}

/// The repositories holding a source: one row per link, removable, and a
/// row adding one — picking a repository of the tree or naming a new one.
#[component]
fn SourceRepositories(
    tree_id: Uuid,
    source_id: Uuid,
    links: Vec<SourceRepository>,
    repositories: Vec<Repository>,
    on_changed: EventHandler<()>,
) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let mut choice = use_signal(String::new);
    let mut new_name = use_signal(String::new);
    let mut call_number = use_signal(String::new);
    let mut medium = use_signal(|| None::<SourceMediaType>);
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);

    let mut sorted = repositories.clone();
    sorted.sort_by_cached_key(|r| r.name.to_lowercase());
    let name_of = |id: Uuid| {
        repositories
            .iter()
            .find(|r| r.id == id)
            .map(|r| r.name.clone())
            .unwrap_or_default()
    };

    let api_add = api.clone();
    let add = move |_| {
        let api = api_add.clone();
        let picked = choice();
        let name = new_name().trim().to_string();
        let body_call = field(&call_number());
        let body_medium = medium();
        spawn(async move {
            busy.set(true);
            error.set(None);
            let result = add_link(
                &api,
                tree_id,
                source_id,
                &picked,
                &name,
                body_call,
                body_medium,
            )
            .await;
            busy.set(false);
            match result {
                Ok(()) => {
                    choice.set(String::new());
                    new_name.set(String::new());
                    call_number.set(String::new());
                    medium.set(None);
                    on_changed.call(());
                }
                Err(e) => error.set(Some(e.to_string())),
            }
        });
    };
    let can_add = !busy()
        && match choice().as_str() {
            "" => false,
            NEW_REPOSITORY => !new_name().trim().is_empty(),
            _ => true,
        };

    rsx! {
        div { class: "pf-ns-block",
            label { class: "pf-ns-label", {i18n.t("dictionary.source.repositories")} }
            if links.is_empty() {
                p { class: "pf-ns-hint", {i18n.t("dictionary.source.no_repositories")} }
            }
            for link in links.iter() {
                {
                    let link_id = link.id;
                    let label = link_label(&i18n, &name_of(link.repository_id), link);
                    let api = api.clone();
                    rsx! {
                        div { key: "{link_id}", class: "pf-witness-row",
                            span { class: "pf-witness-name", "{label}" }
                            button {
                                class: "pf-row-btn is-danger",
                                r#type: "button",
                                disabled: busy(),
                                onclick: move |_| {
                                    let api = api.clone();
                                    spawn(async move {
                                        busy.set(true);
                                        let result = api.remove_source_repository(tree_id, source_id, link_id).await;
                                        busy.set(false);
                                        match result {
                                            Ok(()) => on_changed.call(()),
                                            Err(e) => error.set(Some(e.to_string())),
                                        }
                                    });
                                },
                                {i18n.t("common.remove")}
                            }
                        }
                    }
                }
            }
            div { class: "pf-witness-add",
                div { class: "form-row",
                    div { class: "form-group",
                        label { {i18n.t("dictionary.source.repository")} }
                        select {
                            value: "{choice}",
                            oninput: move |e: Event<FormData>| choice.set(e.value()),
                            option { value: "", {i18n.t("dictionary.source.pick_repository")} }
                            for repository in sorted.iter() {
                                option { key: "{repository.id}", value: "{repository.id}", "{repository.name}" }
                            }
                            option { value: NEW_REPOSITORY, {i18n.t("dictionary.source.new_repository")} }
                        }
                    }
                    if choice() == NEW_REPOSITORY {
                        div { class: "form-group",
                            label { {i18n.t("dictionary.repository.name")} }
                            input {
                                r#type: "text",
                                value: "{new_name}",
                                oninput: move |e: Event<FormData>| new_name.set(e.value()),
                            }
                        }
                    }
                }
                div { class: "form-row",
                    div { class: "form-group",
                        label { {i18n.t("dictionary.source.call_number")} }
                        input {
                            r#type: "text",
                            value: "{call_number}",
                            oninput: move |e: Event<FormData>| call_number.set(e.value()),
                        }
                    }
                    div { class: "form-group",
                        label { {i18n.t("dictionary.source.medium")} }
                        select {
                            onchange: move |e: Event<FormData>| medium.set(SourceMediaType::parse(&e.value())),
                            option { value: "", selected: medium().is_none(), "\u{2014}" }
                            for m in SourceMediaType::all() {
                                option {
                                    key: "{m.as_str()}",
                                    value: "{m.as_str()}",
                                    selected: medium() == Some(*m),
                                    {medium_label(&i18n, *m)}
                                }
                            }
                        }
                    }
                }
                if let Some(err) = error() {
                    div { class: "error-msg", "{err}" }
                }
                button {
                    class: "pf-add-btn",
                    r#type: "button",
                    disabled: !can_add,
                    onclick: add,
                    {i18n.t("dictionary.source.add_repository")}
                }
            }
        }
    }
}

/// Links source `source_id` to the repository `picked` names — creating a
/// repository called `name` for the "new repository" choice.
async fn add_link(
    api: &ApiClient,
    tree_id: Uuid,
    source_id: Uuid,
    picked: &str,
    name: &str,
    call_number: Option<String>,
    media_type: Option<SourceMediaType>,
) -> Result<(), ApiError> {
    let repository_id = match picked.parse::<Uuid>() {
        Ok(id) => id,
        Err(_) => {
            api.create_repository(
                tree_id,
                &CreateRepositoryBody {
                    name: name.to_string(),
                    ..Default::default()
                },
            )
            .await?
            .id
        }
    };
    api.add_source_repository(
        tree_id,
        source_id,
        &AddSourceRepositoryBody {
            repository_id,
            call_number,
            media_type,
        },
    )
    .await
    .map(|_| ())
}

/// A repository as its editor holds it, with its first note.
#[derive(Clone, PartialEq, Default)]
struct RepositoryState {
    repository: Option<Repository>,
    note_id: Option<Uuid>,
    note: String,
}

async fn load_repository_state(
    api: &ApiClient,
    tree_id: Uuid,
    repository: Option<Repository>,
) -> Result<RepositoryState, ApiError> {
    let Some(repository) = repository else {
        return Ok(RepositoryState::default());
    };
    let note = api
        .list_repository_notes(tree_id, repository.id)
        .await?
        .into_iter()
        .next();
    Ok(RepositoryState {
        note_id: note.as_ref().map(|n| n.id),
        note: note.map(|n| n.text).unwrap_or_default(),
        repository: Some(repository),
    })
}

/// The fields of the repository editor.
#[derive(Clone)]
struct RepositoryFields {
    name: String,
    address: Option<String>,
    phone: Option<String>,
    email: Option<String>,
    website: Option<String>,
    note: String,
}

/// Writes `fields` — creating the repository when `state` holds none — and
/// reconciles its first note.
async fn save_repository(
    api: &ApiClient,
    tree_id: Uuid,
    state: &RepositoryState,
    fields: RepositoryFields,
) -> Result<(), ApiError> {
    let saved = match &state.repository {
        Some(repository) => {
            api.update_repository(
                tree_id,
                repository.id,
                &UpdateRepositoryBody {
                    name: fields.name,
                    address: fields.address,
                    phone: fields.phone,
                    email: fields.email,
                    website: fields.website,
                },
            )
            .await?
        }
        None => {
            api.create_repository(
                tree_id,
                &CreateRepositoryBody {
                    name: fields.name,
                    address: fields.address,
                    phone: fields.phone,
                    email: fields.email,
                    website: fields.website,
                },
            )
            .await?
        }
    };
    let note = fields.note.trim();
    match (state.note_id, note.is_empty()) {
        (Some(id), true) => api.delete_note(tree_id, id).await?,
        (Some(id), false) if note != state.note.trim() => {
            api.update_note(
                tree_id,
                id,
                &UpdateNoteBody {
                    text: Some(note.to_string()),
                },
            )
            .await?;
        }
        (None, false) => {
            api.create_note(
                tree_id,
                &CreateNoteBody {
                    text: note.to_string(),
                    person_id: None,
                    event_id: None,
                    family_id: None,
                    source_id: None,
                    media_id: None,
                    repository_id: Some(saved.id),
                },
            )
            .await?;
        }
        _ => {}
    }
    Ok(())
}

/// Creates a repository (`repository` absent) or edits one.
#[component]
pub fn RepositoryEditor(
    tree_id: Uuid,
    repository: Option<Repository>,
    on_close: EventHandler<()>,
    on_saved: EventHandler<()>,
) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let seed = repository.clone();
    let mut name = use_signal(|| seed.as_ref().map(|r| r.name.clone()).unwrap_or_default());
    let mut address = use_signal(|| {
        seed.as_ref()
            .and_then(|r| r.address.clone())
            .unwrap_or_default()
    });
    let mut phone = use_signal(|| {
        seed.as_ref()
            .and_then(|r| r.phone.clone())
            .unwrap_or_default()
    });
    let mut email = use_signal(|| {
        seed.as_ref()
            .and_then(|r| r.email.clone())
            .unwrap_or_default()
    });
    let mut website = use_signal(|| {
        seed.as_ref()
            .and_then(|r| r.website.clone())
            .unwrap_or_default()
    });
    let mut note = use_signal(String::new);
    let mut note_loaded = use_signal(|| false);
    let mut saving = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);

    let api_load = api.clone();
    let loading = repository.clone();
    let state = use_ui_resource("repository_editor", move || {
        let api = api_load.clone();
        let repository = loading.clone();
        async move { load_repository_state(&api, tree_id, repository).await }
    });
    if !note_loaded()
        && let Some(Ok(loaded)) = &*state.read()
    {
        note.set(loaded.note.clone());
        note_loaded.set(true);
    }

    let api_save = api.clone();
    let save = move |_| {
        let api = api_save.clone();
        let current = match &*state.read() {
            Some(Ok(current)) => current.clone(),
            _ => return,
        };
        if name().trim().is_empty() {
            error.set(Some(i18n.t("dictionary.repository.name_required")));
            return;
        }
        let fields = RepositoryFields {
            name: name().trim().to_string(),
            address: field(&address()),
            phone: field(&phone()),
            email: field(&email()),
            website: field(&website()),
            note: note(),
        };
        saving.set(true);
        error.set(None);
        spawn(async move {
            match save_repository(&api, tree_id, &current, fields).await {
                Ok(()) => on_saved.call(()),
                Err(e) => {
                    saving.set(false);
                    error.set(Some(e.to_string()));
                }
            }
        });
    };

    let api_delete = api.clone();
    let existing_id = repository.as_ref().map(|r| r.id);
    let title_key = if existing_id.is_some() {
        "dictionary.repository.edit_title"
    } else {
        "dictionary.repository.new_title"
    };
    let repository_name = repository
        .as_ref()
        .map(|r| r.name.clone())
        .unwrap_or_default();
    rsx! {
        Modal {
            class: "dict-edit-modal",
            label: i18n.t(title_key),
            busy: saving(),
            on_close,
            div { class: "dict-edit-header",
                h2 { {i18n.t(title_key)} }
                button { class: "person-form-close", onclick: move |_| on_close.call(()), "✕" }
            }
            div { class: "form-group",
                label { {i18n.t("dictionary.repository.name")} }
                input {
                    r#type: "text",
                    value: "{name}",
                    oninput: move |e: Event<FormData>| name.set(e.value()),
                }
            }
            div { class: "form-group",
                label { {i18n.t("dictionary.repository.address")} }
                textarea {
                    rows: 3,
                    value: "{address}",
                    oninput: move |e: Event<FormData>| address.set(e.value()),
                }
            }
            div { class: "form-row",
                div { class: "form-group",
                    label { {i18n.t("dictionary.repository.phone")} }
                    input {
                        r#type: "tel",
                        value: "{phone}",
                        oninput: move |e: Event<FormData>| phone.set(e.value()),
                    }
                }
                div { class: "form-group",
                    label { {i18n.t("dictionary.repository.email")} }
                    input {
                        r#type: "email",
                        value: "{email}",
                        oninput: move |e: Event<FormData>| email.set(e.value()),
                    }
                }
            }
            div { class: "form-group",
                label { {i18n.t("dictionary.repository.website")} }
                input {
                    r#type: "url",
                    value: "{website}",
                    oninput: move |e: Event<FormData>| website.set(e.value()),
                }
            }
            div { class: "form-group",
                label { {i18n.t("person_form.notes")} }
                textarea {
                    rows: 3,
                    value: "{note}",
                    oninput: move |e: Event<FormData>| note.set(e.value()),
                }
            }
            if let Some(err) = error() {
                div { class: "error-msg", "{err}" }
            }
            div { class: "modal-actions",
                button { class: "td-btn", onclick: move |_| on_close.call(()), {i18n.t("common.cancel")} }
                button {
                    class: "td-btn td-btn-primary",
                    disabled: saving() || !note_loaded(),
                    onclick: save,
                    if saving() { {i18n.t("common.saving")} } else { {i18n.t("common.save")} }
                }
            }
            if let Some(id) = existing_id {
                DeleteSection {
                    button_label: i18n.t("dictionary.repository.delete"),
                    title: repository_name.clone(),
                    message: i18n.t("dictionary.repository.delete_message"),
                    confirm_label: i18n.t("common.delete"),
                    busy_label: i18n.t("common.deleting"),
                    deleting: saving(),
                    error: None,
                    on_confirm: move |()| {
                        let api = api_delete.clone();
                        saving.set(true);
                        spawn(async move {
                            match api.delete_repository(tree_id, id).await {
                                Ok(()) => on_saved.call(()),
                                Err(e) => {
                                    saving.set(false);
                                    error.set(Some(e.to_string()));
                                }
                            }
                        });
                    },
                }
            }
        }
    }
}
