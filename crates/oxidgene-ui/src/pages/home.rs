//! Home / landing page — tree dashboard.

use chrono::Utc;
use dioxus::prelude::*;
use uuid::Uuid;

use crate::api::{ApiClient, CreateTreeBody};
use crate::components::confirm_dialog::ConfirmDialog;
use crate::router::Route;

/// Dashboard shown at `/`.
#[component]
pub fn Home() -> Element {
    let api = use_context::<ApiClient>();
    let mut refresh_counter = use_signal(|| 0u32);

    let api_res = api.clone();
    let trees_resource = use_resource(move || {
        let api = api_res.clone();
        let _tick = refresh_counter();
        async move { api.list_trees(Some(100), None).await }
    });

    // Create form state.
    let mut show_create = use_signal(|| false);
    let mut new_name = use_signal(String::new);
    let mut new_desc = use_signal(String::new);
    let mut form_error = use_signal(|| None::<String>);

    // Delete confirmation state.
    let mut confirm_delete_id = use_signal(|| None::<Uuid>);
    let mut confirm_delete_name = use_signal(String::new);
    let mut delete_error = use_signal(|| None::<String>);

    let api_create = api.clone();
    let on_create = move |_| {
        let api = api_create.clone();
        let name = new_name().trim().to_string();
        let desc = new_desc().trim().to_string();
        spawn(async move {
            if name.is_empty() {
                form_error.set(Some("Name is required".to_string()));
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
                    show_create.set(false);
                    form_error.set(None);
                    refresh_counter += 1;
                }
                Err(e) => form_error.set(Some(format!("{e}"))),
            }
        });
    };

    let on_confirm_delete = move |_| {
        let api = api.clone();
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
                Err(e) => delete_error.set(Some(format!("{e}"))),
            }
        });
    };

    rsx! {
        // Fixed background gear decorations
        div { class: "gear-bg gear-1" }
        div { class: "gear-bg gear-2" }

        div { class: "home-page",
            div { class: "home-main",

                // ── Page header ──────────────────────────────────────
                div { class: "home-page-header",
                    h1 {
                        "My "
                        span { class: "home-title-accent", "Genealogy Trees" }
                    }
                    p { class: "home-subtitle",
                        "Explore, enrich and share the history of your family lines."
                    }
                }

                // ── Filter / action row ──────────────────────────────
                div { class: "home-filter-row",
                    span { class: "home-filter-label", "Filter:" }
                    button { class: "home-filter-btn home-filter-btn-active", "All" }
                    button { class: "home-filter-btn", "Recent" }
                    button {
                        class: "home-btn-new",
                        onclick: move |_| show_create.set(true),
                        svg {
                            width: "13",
                            height: "13",
                            fill: "none",
                            "viewBox": "0 0 24 24",
                            stroke: "currentColor",
                            "strokeWidth": "2.5",
                            path { d: "M12 5v14M5 12h14" }
                        }
                        "New Tree"
                    }
                }

                // ── Trees grid ───────────────────────────────────────
                match &*trees_resource.read() {
                    Some(Ok(conn)) => rsx! {
                        if conn.edges.is_empty() {
                            div { class: "home-empty",
                                div { class: "home-empty-icon", "🌳" }
                                h3 { "No trees yet" }
                                p { "Create your first genealogy tree to get started." }
                                button {
                                    class: "home-btn-new",
                                    style: "margin-top: 0.5rem;",
                                    onclick: move |_| show_create.set(true),
                                    "New Tree"
                                }
                            }
                        } else {
                            div { class: "trees-grid",
                                for edge in conn.edges.iter() {{
                                    let tree = &edge.node;
                                    let tid = tree.id;
                                    let tid_str = tid.to_string();
                                    let tree_name = tree.name.clone();
                                    let tree_name_del = tree_name.clone();
                                    let desc = tree.description.clone().unwrap_or_default();
                                    let updated_at = tree.updated_at;
                                    rsx! {
                                        TreeCard {
                                            key: "{tid}",
                                            name: tree_name,
                                            description: desc,
                                            updated_at,
                                            tree_id: tid_str,
                                            on_delete: move |_| {
                                                confirm_delete_id.set(Some(tid));
                                                confirm_delete_name.set(tree_name_del.clone());
                                                delete_error.set(None);
                                            },
                                        }
                                    }
                                }}

                                // Add-new card
                                div {
                                    class: "tree-card tree-card-add",
                                    onclick: move |_| show_create.set(true),
                                    div { class: "tree-card-add-icon", "＋" }
                                    div { class: "tree-card-add-text", "Create a new tree" }
                                    div { class: "tree-card-add-sub",
                                        "Start a new family lineage from scratch or import a GEDCOM file"
                                    }
                                }
                            }
                        }
                    },
                    Some(Err(e)) => rsx! {
                        div { class: "error-msg", "Failed to load trees: {e}" }
                    },
                    None => rsx! {
                        div { class: "loading", "Loading trees…" }
                    },
                }
            }
        }

        // ── Create tree modal ─────────────────────────────────────────
        if show_create() {
            div {
                class: "modal-backdrop",
                onclick: move |_| {
                    show_create.set(false);
                    form_error.set(None);
                },
                div {
                    class: "home-create-modal",
                    onclick: move |e: Event<MouseData>| e.stop_propagation(),

                    div { class: "home-create-modal-header",
                        h2 { "New Tree" }
                        button {
                            class: "person-form-close",
                            onclick: move |_| {
                                show_create.set(false);
                                form_error.set(None);
                            },
                            "✕"
                        }
                    }

                    div { class: "home-create-modal-body",
                        if let Some(err) = form_error() {
                            div { class: "error-msg", "{err}" }
                        }
                        div { class: "form-group",
                            label { "Name" }
                            input {
                                r#type: "text",
                                placeholder: "e.g. Martin Family Tree",
                                value: "{new_name}",
                                oninput: move |e: Event<FormData>| new_name.set(e.value()),
                            }
                        }
                        div { class: "form-group",
                            label { "Description (optional)" }
                            textarea {
                                rows: 3,
                                placeholder: "A brief description…",
                                value: "{new_desc}",
                                oninput: move |e: Event<FormData>| new_desc.set(e.value()),
                            }
                        }
                        div { class: "modal-actions",
                            button {
                                class: "btn btn-outline",
                                onclick: move |_| {
                                    show_create.set(false);
                                    form_error.set(None);
                                },
                                "Cancel"
                            }
                            button {
                                class: "btn btn-primary",
                                onclick: on_create,
                                "Create"
                            }
                        }
                    }
                }
            }
        }

        // ── Delete confirmation ───────────────────────────────────────
        if confirm_delete_id().is_some() {
            ConfirmDialog {
                title: "Delete Tree",
                message: format!(
                    "Delete \"{}\"? This action cannot be undone.",
                    confirm_delete_name()
                ),
                confirm_label: "Delete",
                confirm_class: "btn btn-danger",
                error: delete_error(),
                on_confirm: on_confirm_delete,
                on_cancel: move |_| {
                    confirm_delete_id.set(None);
                    delete_error.set(None);
                },
            }
        }

        style { {HOME_STYLES} }
    }
}

/// Individual tree card in the grid.
#[component]
fn TreeCard(
    name: String,
    description: String,
    updated_at: chrono::DateTime<Utc>,
    tree_id: String,
    on_delete: EventHandler<()>,
) -> Element {
    let now = Utc::now();
    let diff = now.signed_duration_since(updated_at);
    let time_ago = if diff.num_days() == 0 {
        "Modified today".to_string()
    } else if diff.num_days() == 1 {
        "Modified 1 day ago".to_string()
    } else if diff.num_days() < 30 {
        format!("Modified {} days ago", diff.num_days())
    } else if diff.num_days() < 365 {
        let w = diff.num_weeks();
        format!("Modified {} week{} ago", w, if w == 1 { "" } else { "s" })
    } else {
        let m = diff.num_days() / 30;
        format!("Modified {} month{} ago", m, if m == 1 { "" } else { "s" })
    };

    rsx! {
        div { class: "tree-card",
            // ── Visual header ──────────────────────────────────────
            div { class: "tree-card-visual",
                svg {
                    "viewBox": "0 0 300 140",
                    xmlns: "http://www.w3.org/2000/svg",
                    width: "100%",
                    height: "100%",
                    // Background
                    rect { fill: "#080a0f", width: "300", height: "140" }
                    // Soft glow
                    rect {
                        x: "90", y: "10", width: "120", height: "120", rx: "60",
                        fill: "#e07820", "fillOpacity": "0.07"
                    }
                    // Trunk
                    rect { x: "147", y: "90", width: "6", height: "35", rx: "3", fill: "#3a4458" }
                    // Main branches
                    line { x1: "150", y1: "90", x2: "100", y2: "60", stroke: "#3a4458", "strokeWidth": "2.5" }
                    line { x1: "150", y1: "90", x2: "200", y2: "60", stroke: "#3a4458", "strokeWidth": "2.5" }
                    // Sub-branches
                    line { x1: "100", y1: "60", x2: "75",  y2: "40", stroke: "#3a4458", "strokeWidth": "1.5" }
                    line { x1: "100", y1: "60", x2: "120", y2: "38", stroke: "#3a4458", "strokeWidth": "1.5" }
                    line { x1: "200", y1: "60", x2: "225", y2: "40", stroke: "#3a4458", "strokeWidth": "1.5" }
                    line { x1: "200", y1: "60", x2: "180", y2: "38", stroke: "#3a4458", "strokeWidth": "1.5" }
                    // Root node (orange)
                    circle { cx: "150", cy: "93", r: "7",  fill: "#e07820", "fillOpacity": "0.9" }
                    // Gen-1 nodes (orange)
                    circle { cx: "100", cy: "60", r: "6",  fill: "#e07820", "fillOpacity": "0.8" }
                    circle { cx: "200", cy: "60", r: "6",  fill: "#e07820", "fillOpacity": "0.8" }
                    // Gen-2 nodes (green)
                    circle { cx: "75",  cy: "40", r: "5",  fill: "#5aab3c", "fillOpacity": "0.9" }
                    circle { cx: "120", cy: "38", r: "5",  fill: "#5aab3c", "fillOpacity": "0.9" }
                    circle { cx: "225", cy: "40", r: "5",  fill: "#5aab3c", "fillOpacity": "0.9" }
                    circle { cx: "180", cy: "38", r: "5",  fill: "#5aab3c", "fillOpacity": "0.9" }
                }
            }

            // ── Card body ──────────────────────────────────────────
            div { class: "tree-card-body",
                div { class: "tree-card-name", "{name}" }
                if !description.is_empty() {
                    div { class: "tree-card-desc", "{description}" }
                }
                div { class: "tree-card-footer",
                    span { class: "tree-last-update", "{time_ago}" }
                    div { class: "tree-card-actions",
                        button {
                            class: "btn-card-delete",
                            title: "Delete tree",
                            onclick: move |e: Event<MouseData>| {
                                e.stop_propagation();
                                on_delete.call(());
                            },
                            "✕"
                        }
                        Link {
                            to: Route::TreeDetail { tree_id: tree_id.clone() },
                            class: "btn-open",
                            "Open"
                        }
                    }
                }
            }
        }
    }
}

const HOME_STYLES: &str = r#"
    /* ── Gear background decorations ────────────────────────────── */

    .gear-bg {
        position: fixed;
        border-radius: 50%;
        border: 2px solid #2a3040;
        opacity: 0.12;
        pointer-events: none;
        z-index: 0;
    }

    .gear-bg::before {
        content: '';
        position: absolute;
        inset: -12px;
        border-radius: 50%;
        border: 2px dashed #3a4458;
        opacity: 0.5;
        animation: gear-spin 60s linear infinite;
    }

    .gear-1 {
        width: 320px;
        height: 320px;
        top: -80px;
        right: -80px;
        animation: gear-spin 90s linear infinite;
    }

    .gear-2 {
        width: 200px;
        height: 200px;
        bottom: 80px;
        left: -60px;
        animation: gear-spin 70s linear infinite reverse;
    }

    @keyframes gear-spin {
        to { transform: rotate(360deg); }
    }

    /* ── Home page wrapper ───────────────────────────────────────── */

    .home-page {
        flex: 1;
        overflow-y: auto;
        position: relative;
        z-index: 1;
    }

    .home-main {
        max-width: 1280px;
        margin: 0 auto;
        padding: 3rem 2.5rem 5rem;
    }

    /* ── Page header ─────────────────────────────────────────────── */

    .home-page-header {
        margin-bottom: 2.5rem;
        animation: home-fade-up 0.6s ease both;
    }

    .home-page-header h1 {
        font-family: var(--font-heading);
        font-size: 2rem;
        font-weight: 700;
        color: var(--text-primary);
        letter-spacing: 0.03em;
    }

    .home-title-accent {
        background: linear-gradient(135deg, var(--orange) 0%, var(--orange-light) 100%);
        -webkit-background-clip: text;
        -webkit-text-fill-color: transparent;
        background-clip: text;
    }

    .home-subtitle {
        margin-top: 0.4rem;
        color: var(--text-secondary);
        font-size: 0.95rem;
        font-weight: 300;
    }

    /* ── Filter / action row ─────────────────────────────────────── */

    .home-filter-row {
        display: flex;
        align-items: center;
        gap: 0.75rem;
        margin-bottom: 2rem;
        flex-wrap: wrap;
        animation: home-fade-up 0.6s 0.08s ease both;
    }

    .home-filter-label {
        font-size: 0.78rem;
        color: var(--text-muted);
        text-transform: uppercase;
        letter-spacing: 0.08em;
        margin-right: 0.25rem;
    }

    .home-filter-btn {
        background: var(--bg-panel);
        border: 1px solid var(--border);
        border-radius: 20px;
        padding: 0.35rem 1rem;
        color: var(--text-secondary);
        font-size: 0.82rem;
        cursor: pointer;
        transition: border-color 0.2s, color 0.2s, background 0.2s;
        font-family: var(--font-sans);
    }

    .home-filter-btn:hover {
        border-color: var(--orange);
        color: var(--text-primary);
    }

    .home-filter-btn-active {
        background: rgba(224, 120, 32, 0.12);
        border-color: var(--orange);
        color: var(--orange-light);
    }

    .home-btn-new {
        margin-left: auto;
        display: inline-flex;
        align-items: center;
        gap: 0.5rem;
        background: linear-gradient(135deg, var(--orange) 0%, var(--orange-light) 100%);
        border: none;
        border-radius: 8px;
        padding: 0.5rem 1.1rem;
        color: #fff;
        font-family: var(--font-heading);
        font-size: 0.8rem;
        font-weight: 600;
        letter-spacing: 0.05em;
        cursor: pointer;
        transition: opacity 0.2s, transform 0.15s;
        box-shadow: 0 2px 12px rgba(224, 120, 32, 0.35);
        text-decoration: none;
    }

    .home-btn-new:hover {
        opacity: 0.9;
        transform: translateY(-1px);
    }

    /* ── Trees grid ──────────────────────────────────────────────── */

    .trees-grid {
        display: grid;
        grid-template-columns: repeat(auto-fill, minmax(300px, 1fr));
        gap: 1.5rem;
        animation: home-fade-up 0.6s 0.15s ease both;
    }

    /* ── Tree card ───────────────────────────────────────────────── */

    .tree-card {
        background: var(--bg-card);
        border: 1px solid var(--border);
        border-radius: 16px;
        overflow: hidden;
        cursor: pointer;
        transition: transform 0.25s, border-color 0.25s, box-shadow 0.25s, background 0.25s;
        position: relative;
    }

    .tree-card:hover {
        transform: translateY(-4px);
        border-color: var(--orange);
        box-shadow: 0 8px 40px rgba(224, 120, 32, 0.18), 0 2px 12px rgba(0, 0, 0, 0.5);
        background: var(--bg-card-hover);
    }

    .tree-card-visual {
        height: 140px;
        position: relative;
        overflow: hidden;
        background: #0d1018;
    }

    .tree-card-visual svg {
        display: block;
        width: 100%;
        height: 100%;
    }

    .tree-card-body {
        padding: 1.25rem 1.4rem 1.4rem;
    }

    .tree-card-name {
        font-family: var(--font-heading);
        font-size: 1.05rem;
        font-weight: 600;
        color: var(--text-primary);
        margin-bottom: 0.3rem;
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
    }

    .tree-card-desc {
        font-size: 0.82rem;
        color: var(--text-secondary);
        margin-bottom: 0.75rem;
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
    }

    .tree-card-footer {
        display: flex;
        align-items: center;
        justify-content: space-between;
        border-top: 1px solid var(--border);
        padding-top: 1rem;
        margin-top: 0.75rem;
    }

    .tree-last-update {
        font-size: 0.75rem;
        color: var(--text-muted);
    }

    .tree-card-actions {
        display: flex;
        align-items: center;
        gap: 0.5rem;
    }

    .btn-open {
        background: linear-gradient(135deg, var(--orange), var(--orange-light));
        border: none;
        border-radius: 7px;
        padding: 0.4rem 1rem;
        color: #fff;
        font-family: var(--font-heading);
        font-size: 0.72rem;
        font-weight: 600;
        letter-spacing: 0.05em;
        cursor: pointer;
        transition: opacity 0.2s;
        text-decoration: none;
        display: inline-block;
    }

    .btn-open:hover {
        opacity: 0.85;
    }

    .btn-card-delete {
        width: 28px;
        height: 28px;
        display: flex;
        align-items: center;
        justify-content: center;
        background: none;
        border: 1px solid var(--border);
        border-radius: 6px;
        cursor: pointer;
        font-size: 0.7rem;
        color: var(--text-muted);
        transition: background 0.15s, border-color 0.15s, color 0.15s;
        padding: 0;
        line-height: 1;
    }

    .btn-card-delete:hover {
        background: rgba(224, 82, 82, 0.1);
        border-color: var(--color-danger);
        color: var(--color-danger);
    }

    /* ── Add-new card ─────────────────────────────────────────────── */

    .tree-card-add {
        border-style: dashed;
        border-color: var(--border);
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        min-height: 280px;
        gap: 1rem;
        background: transparent;
    }

    .tree-card-add:hover {
        border-color: var(--green);
        background: rgba(90, 171, 60, 0.04);
        box-shadow: 0 0 30px rgba(90, 171, 60, 0.08);
        transform: translateY(-2px);
    }

    .tree-card-add-icon {
        width: 56px;
        height: 56px;
        border-radius: 50%;
        background: rgba(90, 171, 60, 0.1);
        border: 1px solid var(--green);
        display: flex;
        align-items: center;
        justify-content: center;
        font-size: 1.6rem;
        color: var(--green-light);
        transition: background 0.2s;
    }

    .tree-card-add:hover .tree-card-add-icon {
        background: rgba(90, 171, 60, 0.2);
    }

    .tree-card-add-text {
        font-family: var(--font-heading);
        font-size: 0.9rem;
        color: var(--green-light);
        letter-spacing: 0.04em;
    }

    .tree-card-add-sub {
        font-size: 0.78rem;
        color: var(--text-muted);
        text-align: center;
        padding: 0 1.5rem;
    }

    /* ── Empty state ─────────────────────────────────────────────── */

    .home-empty {
        text-align: center;
        padding: 5rem 2rem;
        display: flex;
        flex-direction: column;
        align-items: center;
        gap: 1rem;
    }

    .home-empty-icon {
        font-size: 3.5rem;
    }

    .home-empty h3 {
        font-family: var(--font-heading);
        font-size: 1.2rem;
        color: var(--text-primary);
    }

    .home-empty p {
        color: var(--text-secondary);
        font-size: 0.9rem;
    }

    /* ── Create tree modal ───────────────────────────────────────── */

    .home-create-modal {
        background: var(--bg-panel);
        border: 1px solid var(--border);
        border-radius: 16px;
        min-width: 360px;
        max-width: 480px;
        width: 95vw;
        box-shadow: 0 20px 60px rgba(0, 0, 0, 0.8);
    }

    .home-create-modal-header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        padding: 1.25rem 1.5rem;
        border-bottom: 1px solid var(--border);
    }

    .home-create-modal-header h2 {
        font-family: var(--font-heading);
        font-size: 1.1rem;
        font-weight: 600;
        color: var(--text-primary);
        margin: 0;
    }

    .home-create-modal-body {
        padding: 1.5rem;
    }

    /* ── Animations ──────────────────────────────────────────────── */

    @keyframes home-fade-up {
        from { opacity: 0; transform: translateY(20px); }
        to   { opacity: 1; transform: translateY(0); }
    }

    /* ── Responsive ──────────────────────────────────────────────── */

    @media (max-width: 640px) {
        .home-main { padding: 2rem 1rem 4rem; }
        .trees-grid { grid-template-columns: 1fr; }
        .home-btn-new { margin-left: 0; }
        .home-page-header h1 { font-size: 1.5rem; }
    }
"#;
