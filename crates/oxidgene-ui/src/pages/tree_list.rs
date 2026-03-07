//! Tree list page — shows all genealogy trees with create / edit / delete.

use dioxus::prelude::*;
use uuid::Uuid;

use crate::api::{ApiClient, CreateTreeBody, UpdateTreeBody};
use crate::components::confirm_dialog::ConfirmDialog;
use crate::i18n::use_i18n;
use crate::router::Route;

/// Page rendered at `/trees`.
#[component]
pub fn TreeList() -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let mut refresh_counter = use_signal(|| 0u32);

    // Fetch trees whenever refresh_counter changes.
    let api_res = api.clone();
    let trees_resource = use_resource(move || {
        let api = api_res.clone();
        let _tick = refresh_counter();
        async move { api.list_trees(Some(100), None).await }
    });

    // New-tree form state.
    let mut new_name = use_signal(String::new);
    let mut new_desc = use_signal(String::new);
    let mut show_form = use_signal(|| false);
    let mut form_error = use_signal(|| None::<String>);

    // Edit state: which tree is being edited inline.
    let mut editing_id = use_signal(|| None::<Uuid>);
    let mut edit_name = use_signal(String::new);
    let mut edit_desc = use_signal(String::new);
    let mut edit_error = use_signal(|| None::<String>);

    // Delete confirmation state.
    let mut confirm_delete_id = use_signal(|| None::<Uuid>);
    let mut confirm_delete_name = use_signal(String::new);
    let mut delete_error = use_signal(|| None::<String>);

    // Import state.
    let mut importing_tree_id = use_signal(|| None::<Uuid>);
    let mut import_error = use_signal(|| None::<String>);

    // Create tree handler.
    let api_create = api.clone();
    let on_create = move |_| {
        let api = api_create.clone();
        let name = new_name().trim().to_string();
        let desc = new_desc().trim().to_string();
        spawn(async move {
            if name.is_empty() {
                form_error.set(Some(i18n.t("tree.form.name_required")));
                return;
            }
            let body = CreateTreeBody {
                name,
                description: if desc.is_empty() { None } else { Some(desc) },
            };
            match api.create_tree(&body).await {
                Ok(_) => {
                    new_name.set(String::new());
                    new_desc.set(String::new());
                    show_form.set(false);
                    form_error.set(None);
                    refresh_counter += 1;
                }
                Err(e) => {
                    form_error.set(Some(format!("{e}")));
                }
            }
        });
    };

    // Save edit handler.
    let api_edit = api.clone();
    let on_save_edit = move |_| {
        let api = api_edit.clone();
        let Some(id) = editing_id() else { return };
        let name = edit_name().trim().to_string();
        let desc = edit_desc().trim().to_string();
        spawn(async move {
            if name.is_empty() {
                edit_error.set(Some(i18n.t("tree.form.name_required")));
                return;
            }
            let body = UpdateTreeBody {
                name: Some(name),
                description: Some(if desc.is_empty() { None } else { Some(desc) }),
            };
            match api.update_tree(id, &body).await {
                Ok(_) => {
                    editing_id.set(None);
                    edit_error.set(None);
                    refresh_counter += 1;
                }
                Err(e) => {
                    edit_error.set(Some(format!("{e}")));
                }
            }
        });
    };

    // Confirm delete handler.
    let api_del = api.clone();
    let on_confirm_delete = move |_| {
        let api = api_del.clone();
        let Some(id) = confirm_delete_id() else {
            return;
        };
        spawn(async move {
            match api.delete_tree(id).await {
                Ok(_) => {
                    confirm_delete_id.set(None);
                    delete_error.set(None);
                    refresh_counter += 1;
                }
                Err(e) => {
                    delete_error.set(Some(format!("{e}")));
                }
            }
        });
    };

    rsx! {
        div { class: "page-content",
        div { class: "page-header",
            h1 { {i18n.t("tree_list.title")} }
            button {
                class: "btn btn-primary",
                onclick: move |_| show_form.toggle(),
                if show_form() { {i18n.t("common.cancel")} } else { {i18n.t("home.new_tree")} }
            }
        }

        // Create tree form
        if show_form() {
            div { class: "card", style: "margin-bottom: 24px;",
                h3 { style: "margin-bottom: 16px;", {i18n.t("tree_list.create_title")} }

                if let Some(err) = form_error() {
                    div { class: "error-msg", "{err}" }
                }

                div { class: "form-group",
                    label { {i18n.t("tree.form.name_label")} }
                    input {
                        r#type: "text",
                        placeholder: "{i18n.t(\"tree.form.name_placeholder\")}",
                        value: "{new_name}",
                        oninput: move |e: Event<FormData>| new_name.set(e.value()),
                    }
                }
                div { class: "form-group",
                    label { {i18n.t("tree.form.description_label")} }
                    textarea {
                        rows: 3,
                        placeholder: "{i18n.t(\"tree.form.description_placeholder\")}",
                        value: "{new_desc}",
                        oninput: move |e: Event<FormData>| new_desc.set(e.value()),
                    }
                }
                button { class: "btn btn-primary", onclick: on_create, {i18n.t("common.create")} }
            }
        }

        // Import error
        if let Some(err) = import_error() {
            div { class: "error-msg", style: "margin-bottom: 12px;", "{err}" }
        }

        // Delete confirmation dialog
        if confirm_delete_id().is_some() {
            ConfirmDialog {
                title: i18n.t("confirm.delete_tree.title"),
                message: i18n.t_args("confirm.delete_tree.message_name", &[("name", &confirm_delete_name())]),
                confirm_label: i18n.t("common.delete"),
                confirm_class: "btn btn-danger",
                error: delete_error(),
                on_confirm: on_confirm_delete,
                on_cancel: move |_| {
                    confirm_delete_id.set(None);
                    delete_error.set(None);
                },
            }
        }

        // Trees list
        match &*trees_resource.read() {
            Some(Ok(conn)) => rsx! {
                if conn.edges.is_empty() {
                    div { class: "empty-state",
                        h3 { {i18n.t("tree_list.no_trees")} }
                        p { {i18n.t("tree_list.no_trees_hint")} }
                    }
                } else {
                    div { class: "card",
                        div { class: "table-wrapper",
                            table {
                                thead {
                                    tr {
                                        th { {i18n.t("tree.form.name_label")} }
                                        th { {i18n.t("tree.form.description_label")} }
                                        th { {i18n.t("tree_list.created")} }
                                        th { style: "width: 200px;", {i18n.t("tree_list.actions")} }
                                    }
                                }
                                tbody {
                                    for edge in conn.edges.iter() {
                                        {
                                            let tree = &edge.node;
                                            let tid = tree.id;
                                            let tid_str = tid.to_string();
                                            let is_editing = editing_id() == Some(tid);
                                            let tree_name = tree.name.clone();
                                            let tree_name_del = tree_name.clone();
                                            let tree_desc = tree.description.clone().unwrap_or_default();
                                            if is_editing {
                                                rsx! {
                                                    tr {
                                                        td { colspan: 4,
                                                            div { style: "display: flex; flex-direction: column; gap: 8px;",
                                                                if let Some(err) = edit_error() {
                                                                    div { class: "error-msg", "{err}" }
                                                                }
                                                                div { class: "form-group", style: "margin-bottom: 0;",
                                                                    label { {i18n.t("tree.form.name_label")} }
                                                                    input {
                                                                        r#type: "text",
                                                                        value: "{edit_name}",
                                                                        oninput: move |e: Event<FormData>| edit_name.set(e.value()),
                                                                    }
                                                                }
                                                                div { class: "form-group", style: "margin-bottom: 0;",
                                                                    label { {i18n.t("tree.form.description_label")} }
                                                                    input {
                                                                        r#type: "text",
                                                                        value: "{edit_desc}",
                                                                        oninput: move |e: Event<FormData>| edit_desc.set(e.value()),
                                                                    }
                                                                }
                                                                div { style: "display: flex; gap: 8px;",
                                                                    button {
                                                                        class: "btn btn-primary",
                                                                        onclick: on_save_edit.clone(),
                                                                        {i18n.t("common.save")}
                                                                    }
                                                                    button {
                                                                        class: "btn btn-outline",
                                                                        onclick: move |_| {
                                                                            editing_id.set(None);
                                                                            edit_error.set(None);
                                                                        },
                                                                        {i18n.t("common.cancel")}
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            } else {
                                                rsx! {
                                                    tr {
                                                        td {
                                                            Link {
                                                                to: Route::TreeDetail { tree_id: tid_str, person: None },
                                                                "{tree.name}"
                                                            }
                                                        }
                                                        td { class: "text-muted",
                                                            {tree.description.as_deref().unwrap_or("--")}
                                                        }
                                                        td { class: "text-muted",
                                                            {tree.created_at.format("%Y-%m-%d").to_string()}
                                                        }
                                                        td {
                                                            div { style: "display: flex; gap: 4px;",
                                                                button {
                                                                    class: "btn btn-outline btn-sm",
                                                                    onclick: move |_| {
                                                                        editing_id.set(Some(tid));
                                                                        edit_name.set(tree_name.clone());
                                                                        edit_desc.set(tree_desc.clone());
                                                                        edit_error.set(None);
                                                                    },
                                                                    {i18n.t("common.edit")}
                                                                }
                                                                button {
                                                                    class: "btn btn-outline btn-sm",
                                                                    disabled: importing_tree_id() == Some(tid),
                                                                    onclick: {
                                                                        let api = api.clone();
                                                                        move |_| {
                                                                            let api = api.clone();
                                                                            spawn(async move {
                                                                                let file = rfd::AsyncFileDialog::new()
                                                                                    .add_filter("GEDCOM", &["ged"])
                                                                                    .add_filter("All files", &["*"])
                                                                                    .set_title("Select a GEDCOM file")
                                                                                    .pick_file()
                                                                                    .await;
                                                                                let Some(file) = file else { return };
                                                                                importing_tree_id.set(Some(tid));
                                                                                import_error.set(None);
                                                                                let path = file.path().to_path_buf();
                                                                                let gedcom = match tokio::fs::read_to_string(&path).await {
                                                                                    Ok(c) => c,
                                                                                    Err(e) => {
                                                                                        import_error.set(Some(format!("Failed to read: {e}")));
                                                                                        importing_tree_id.set(None);
                                                                                        return;
                                                                                    }
                                                                                };
                                                                                match api.import_gedcom(tid, &gedcom).await {
                                                                                    Ok(_) => {
                                                                                        importing_tree_id.set(None);
                                                                                        refresh_counter += 1;
                                                                                    }
                                                                                    Err(e) => {
                                                                                        import_error.set(Some(format!("{e}")));
                                                                                        importing_tree_id.set(None);
                                                                                    }
                                                                                }
                                                                            });
                                                                        }
                                                                    },
                                                                    if importing_tree_id() == Some(tid) { {i18n.t("common.importing")} } else { {i18n.t("common.import")} }
                                                                }
                                                                button {
                                                                    class: "btn btn-danger btn-sm",
                                                                    onclick: move |_| {
                                                                        confirm_delete_id.set(Some(tid));
                                                                        confirm_delete_name.set(tree_name_del.clone());
                                                                        delete_error.set(None);
                                                                    },
                                                                    {i18n.t("common.delete")}
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            },
            Some(Err(e)) => rsx! {
                div { class: "error-msg", "{e}" }
            },
            None => rsx! {
                div { class: "loading", {i18n.t("common.loading")} }
            },
        }
        } // .page-content
    }
}
