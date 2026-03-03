//! Tree settings page with navigation sidebar.
//!
//! Provides tree configuration (Tree & Roots), tools stubs,
//! and GEDCOM export functionality.

use dioxus::prelude::*;
use uuid::Uuid;

use crate::api::ApiClient;
use crate::router::Route;

/// Settings page for a tree.
#[component]
pub fn Settings(tree_id: String) -> Element {
    let api = use_context::<ApiClient>();
    let refresh = use_signal(|| 0u32);
    let mut active_section = use_signal(|| "tree-roots".to_string());
    let mut export_loading = use_signal(|| false);
    let mut export_error = use_signal(|| None::<String>);
    let mut export_success = use_signal(|| false);

    let tree_id_parsed = tree_id.parse::<Uuid>().ok();

    // Fetch tree info
    let api_tree = api.clone();
    let tree_resource = use_resource(move || {
        let api = api_tree.clone();
        let _tick = refresh();
        async move {
            let tid = tree_id_parsed?;
            Some(api.get_tree(tid).await)
        }
    });

    let tree_name = match &*tree_resource.read() {
        Some(Some(Ok(tree))) => tree.name.clone(),
        _ => "Loading...".to_string(),
    };

    // Export handler
    let api_export = api.clone();
    let on_export = move |_| {
        let api = api_export.clone();
        export_loading.set(true);
        export_error.set(None);
        export_success.set(false);
        spawn(async move {
            if let Some(tid) = tree_id_parsed {
                match api.export_gedcom(tid).await {
                    Ok(result) => {
                        // Trigger download via JS
                        let gedcom = result.gedcom.replace('\\', "\\\\").replace('`', "\\`");
                        let js = format!(
                            r#"
                            const blob = new Blob([`{gedcom}`], {{ type: 'text/plain' }});
                            const url = URL.createObjectURL(blob);
                            const a = document.createElement('a');
                            a.href = url;
                            a.download = 'export.ged';
                            document.body.appendChild(a);
                            a.click();
                            document.body.removeChild(a);
                            URL.revokeObjectURL(url);
                            "#
                        );
                        document::eval(&js);
                        export_success.set(true);
                        export_loading.set(false);
                    }
                    Err(e) => {
                        export_error.set(Some(format!("{e}")));
                        export_loading.set(false);
                    }
                }
            }
        });
    };

    let sec = active_section();

    rsx! {
        style { {SETTINGS_STYLES} }

        div { class: "page-content",
            // Breadcrumb
            div { class: "settings-breadcrumb",
                Link { to: Route::TreeList {}, "Trees" }
                span { class: "pd-breadcrumb-sep", " / " }
                Link {
                    to: Route::TreeDetail { tree_id: tree_id.clone() },
                    "{tree_name}"
                }
                span { class: "pd-breadcrumb-sep", " / " }
                span { class: "pd-breadcrumb-current", "Settings" }
            }

            div { class: "settings-layout",
                // Left navigation
                nav { class: "settings-nav",
                    div { class: "settings-nav-group",
                        div { class: "settings-nav-group-label", "Settings" }
                        button {
                            class: if sec == "tree-roots" { "settings-nav-item active" } else { "settings-nav-item" },
                            onclick: move |_| active_section.set("tree-roots".to_string()),
                            "Tree & Roots"
                        }
                        button {
                            class: if sec == "privacy" { "settings-nav-item active" } else { "settings-nav-item" },
                            onclick: move |_| active_section.set("privacy".to_string()),
                            "Privacy"
                        }
                        button {
                            class: if sec == "date-display" { "settings-nav-item active" } else { "settings-nav-item" },
                            onclick: move |_| active_section.set("date-display".to_string()),
                            "Date Display"
                        }
                        button {
                            class: if sec == "entry-options" { "settings-nav-item active" } else { "settings-nav-item" },
                            onclick: move |_| active_section.set("entry-options".to_string()),
                            "Entry Options"
                        }
                    }
                    div { class: "settings-nav-group",
                        div { class: "settings-nav-group-label", "Tools" }
                        button {
                            class: if sec == "history" { "settings-nav-item active" } else { "settings-nav-item" },
                            onclick: move |_| active_section.set("history".to_string()),
                            "History"
                        }
                        button {
                            class: if sec == "anomalies" { "settings-nav-item active" } else { "settings-nav-item" },
                            onclick: move |_| active_section.set("anomalies".to_string()),
                            "Anomalies"
                        }
                        button {
                            class: if sec == "duplicates" { "settings-nav-item active" } else { "settings-nav-item" },
                            onclick: move |_| active_section.set("duplicates".to_string()),
                            "Potential Duplicates"
                        }
                    }
                    div { class: "settings-nav-group",
                        div { class: "settings-nav-group-label", "Export" }
                        button {
                            class: if sec == "export" { "settings-nav-item active" } else { "settings-nav-item" },
                            onclick: move |_| active_section.set("export".to_string()),
                            "Export Tree"
                        }
                    }
                }

                // Content area
                div { class: "settings-content",
                    if sec == "tree-roots" {
                        TreeRootsSection {
                            tree_id: tree_id.clone(),
                        }
                    } else if sec == "export" {
                        ExportSection {
                            on_export: on_export,
                            loading: export_loading(),
                            error: export_error(),
                            success: export_success(),
                        }
                    } else {
                        PlaceholderSection { section_name: sec.clone() }
                    }
                }
            }
        }
    }
}

#[component]
fn TreeRootsSection(tree_id: String) -> Element {
    rsx! {
        div { class: "settings-section",
            div { class: "settings-section-eyebrow", "Settings" }
            h2 { class: "settings-section-title", "Tree & Roots" }
            p { class: "settings-section-subtitle",
                "Configure the root person (SOSA 1) and personal identification."
            }

            div { class: "card", style: "margin-top: 16px;",
                h3 { style: "font-size: 0.95rem; margin-bottom: 12px; color: var(--text-primary);",
                    "Root Person (SOSA 1)"
                }
                p { style: "font-size: 0.82rem; color: var(--text-secondary); margin-bottom: 12px;",
                    "The root person is the starting point of the Sosa-Stradonitz numbering system. "
                    "All ancestors are numbered relative to this person."
                }
                div { class: "settings-placeholder",
                    "Root person selection will be available in a future update."
                }
            }

            div { class: "card", style: "margin-top: 16px;",
                h3 { style: "font-size: 0.95rem; margin-bottom: 12px; color: var(--text-primary);",
                    "Who am I?"
                }
                p { style: "font-size: 0.82rem; color: var(--text-secondary); margin-bottom: 12px;",
                    "Select which person in the tree represents you."
                }
                div { class: "settings-placeholder",
                    "Personal identification will be available in a future update."
                }
            }
        }
    }
}

#[component]
fn ExportSection(
    on_export: EventHandler<MouseEvent>,
    loading: bool,
    error: Option<String>,
    success: bool,
) -> Element {
    rsx! {
        div { class: "settings-section",
            div { class: "settings-section-eyebrow", "Export" }
            h2 { class: "settings-section-title", "Export Tree" }
            p { class: "settings-section-subtitle",
                "Download your tree data in standard genealogy formats."
            }

            div { class: "card", style: "margin-top: 16px;",
                div { style: "display: flex; align-items: center; gap: 16px;",
                    div { style: "flex: 1;",
                        h3 { style: "font-size: 0.95rem; margin-bottom: 4px; color: var(--text-primary);",
                            "GEDCOM 5.5.1"
                        }
                        p { style: "font-size: 0.82rem; color: var(--text-secondary);",
                            "Standard genealogy exchange format (.ged). "
                            "Compatible with most genealogy software."
                        }
                    }
                    button {
                        class: "btn btn-primary",
                        disabled: loading,
                        onclick: on_export,
                        if loading { "Exporting..." } else { "Download .ged" }
                    }
                }
                if let Some(err) = &error {
                    div { class: "error-msg", style: "margin-top: 12px;", "{err}" }
                }
                if success {
                    div { class: "success-msg", style: "margin-top: 12px;",
                        "Export completed successfully."
                    }
                }
            }
        }
    }
}

#[component]
fn PlaceholderSection(section_name: String) -> Element {
    let display_name = match section_name.as_str() {
        "privacy" => "Privacy",
        "date-display" => "Date Display",
        "entry-options" => "Entry Options",
        "history" => "History",
        "anomalies" => "Anomalies",
        "duplicates" => "Potential Duplicates",
        _ => &section_name,
    };

    let group = match section_name.as_str() {
        "privacy" | "date-display" | "entry-options" => "Settings",
        "history" | "anomalies" | "duplicates" => "Tools",
        _ => "Settings",
    };

    rsx! {
        div { class: "settings-section",
            div { class: "settings-section-eyebrow", "{group}" }
            h2 { class: "settings-section-title", "{display_name}" }

            div { class: "card", style: "margin-top: 16px;",
                div { class: "empty-state",
                    h3 { "Coming soon" }
                    p { "This section is under development." }
                }
            }
        }
    }
}

const SETTINGS_STYLES: &str = r#"
    .settings-breadcrumb {
        display: flex;
        align-items: center;
        gap: 6px;
        font-size: 0.85rem;
        margin-bottom: 20px;
    }
    .settings-breadcrumb a {
        color: var(--text-secondary);
        text-decoration: none;
        transition: color 0.15s;
    }
    .settings-breadcrumb a:hover { color: var(--orange); }

    .settings-layout {
        display: flex;
        gap: 24px;
        min-height: 0;
    }

    .settings-nav {
        width: 200px;
        min-width: 200px;
        flex-shrink: 0;
    }

    .settings-nav-group {
        margin-bottom: 20px;
    }

    .settings-nav-group-label {
        font-size: 0.68rem;
        font-weight: 700;
        color: var(--orange);
        text-transform: uppercase;
        letter-spacing: 0.5px;
        margin-bottom: 6px;
        padding: 0 8px;
    }

    .settings-nav-item {
        display: block;
        width: 100%;
        padding: 6px 8px;
        text-align: left;
        background: none;
        border: none;
        border-radius: 5px;
        font-size: 0.85rem;
        color: var(--text-secondary);
        cursor: pointer;
        transition: background 0.12s, color 0.12s;
        font-family: var(--font-sans);
    }
    .settings-nav-item:hover {
        background: var(--bg-card-hover);
        color: var(--text-primary);
    }
    .settings-nav-item.active {
        background: var(--sel-bg);
        color: var(--text-primary);
        font-weight: 600;
    }

    .settings-content {
        flex: 1;
        min-width: 0;
        max-width: 860px;
    }

    .settings-section-eyebrow {
        font-size: 0.68rem;
        font-weight: 700;
        color: var(--orange);
        text-transform: uppercase;
        letter-spacing: 0.5px;
        margin-bottom: 4px;
    }

    .settings-section-title {
        font-family: var(--font-heading);
        font-size: 1.2rem;
        font-weight: 600;
        color: var(--text-primary);
        margin-bottom: 4px;
    }

    .settings-section-subtitle {
        font-size: 0.85rem;
        color: var(--text-secondary);
    }

    .settings-placeholder {
        padding: 16px;
        text-align: center;
        color: var(--text-muted);
        font-size: 0.85rem;
        font-style: italic;
    }

    @media (max-width: 768px) {
        .settings-layout {
            flex-direction: column;
        }
        .settings-nav {
            width: 100%;
            min-width: 0;
            display: flex;
            flex-wrap: wrap;
            gap: 4px;
        }
        .settings-nav-group {
            display: flex;
            flex-wrap: wrap;
            gap: 4px;
            margin-bottom: 8px;
        }
        .settings-nav-group-label {
            width: 100%;
        }
    }
"#;
