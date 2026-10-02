//! The breadcrumb at the head of every tree page: the logo home, the tree,
//! then where the page is.

use dioxus::prelude::*;

use crate::router::Route;

/// The logo leading home, then the tree named `tree_name` when there is one
/// — a link to its pedigree unless `linked` is false, on the pedigree itself
/// — then `children`, the page's own crumbs.
#[component]
pub fn TreeBreadcrumb(
    #[props(default)] tree_id: String,
    #[props(default)] tree_name: String,
    #[props(default = true)] linked: bool,
    children: Element,
) -> Element {
    rsx! {
        nav { class: "td-bc",
            Link { to: Route::Home {}, class: "td-bc-logo",
                img {
                    src: crate::components::layout::logo_data_url(),
                    alt: "OxidGene",
                    class: "td-bc-logo-img",
                }
            }
            if !tree_name.is_empty() {
                if linked {
                    Link {
                        to: Route::TreeDetail { tree_id, person: None },
                        class: "td-bc-link",
                        "{tree_name}"
                    }
                } else {
                    span { class: "td-bc-link", "{tree_name}" }
                }
                span { class: "td-bc-sep", "/" }
            }
            {children}
        }
    }
}
