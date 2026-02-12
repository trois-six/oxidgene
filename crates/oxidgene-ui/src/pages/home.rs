//! Home / landing page.

use dioxus::prelude::*;

use crate::router::Route;

/// Welcome page shown at `/`.
#[component]
pub fn Home() -> Element {
    rsx! {
        div { class: "home-page",
            div { class: "card home-hero",
                h1 { "Welcome to OxidGene" }
                p { class: "text-muted",
                    "A multiplatform genealogy application built with Rust."
                }

                div { class: "home-actions",
                    Link { to: Route::TreeList {},
                        button { class: "btn btn-primary", "Browse Trees" }
                    }
                }
            }

            div { class: "home-features",
                div { class: "card feature-card",
                    h3 { "Genealogy Trees" }
                    p { class: "text-muted",
                        "Create and manage family trees with full CRUD support for persons, families, events, and more."
                    }
                }
                div { class: "card feature-card",
                    h3 { "GEDCOM Support" }
                    p { class: "text-muted",
                        "Import and export GEDCOM 5.5.1 files to interoperate with other genealogy software."
                    }
                }
                div { class: "card feature-card",
                    h3 { "Source Citations" }
                    p { class: "text-muted",
                        "Attach sources, citations, and media to persons, events, and families."
                    }
                }
            }
        }

        style { {HOME_STYLES} }
    }
}

const HOME_STYLES: &str = r#"
    .home-hero {
        text-align: center;
        padding: 48px 24px;
        margin-bottom: 32px;
    }

    .home-hero h1 {
        font-size: 2rem;
        margin-bottom: 12px;
    }

    .home-hero p {
        font-size: 1.1rem;
        margin-bottom: 24px;
    }

    .home-actions {
        display: flex;
        justify-content: center;
        gap: 12px;
    }

    .home-features {
        display: grid;
        grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
        gap: 16px;
    }

    .feature-card {
        text-align: center;
        padding: 32px 24px;
    }

    .feature-card h3 {
        margin-bottom: 8px;
        font-weight: 600;
    }
"#;
