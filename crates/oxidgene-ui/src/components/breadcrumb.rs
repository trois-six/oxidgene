//! The breadcrumb at the head of every tree page: the history buttons, the
//! logo home, the tree, then where the page is.

use dioxus::prelude::*;

use crate::components::history_nav::HistoryNav;
use crate::router::Route;

/// The back and forward buttons unless the navbar above carries them, the
/// logo leading home, then the tree named `tree_name` when there is one — a
/// link to its pedigree unless `linked` is false, on the pedigree itself —
/// then `children`, the page's own crumbs.
#[component]
pub fn TreeBreadcrumb(
    #[props(default)] tree_id: String,
    #[props(default)] tree_name: String,
    #[props(default = true)] linked: bool,
    children: Element,
) -> Element {
    let route = use_route::<Route>();
    rsx! {
        nav { class: "td-bc",
            if !route.shows_navbar() {
                HistoryNav {}
            }
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
