//! What every page of a tree shares: the tree it loads, the person its
//! sidebar acts on, and its frame — topbar, breadcrumb, printed heading,
//! left icon sidebar and scrollable content.

use dioxus::prelude::*;
use oxidgene_core::types::Tree;
use uuid::Uuid;

use crate::api::{ApiClient, ApiError};
use crate::components::breadcrumb::TreeBreadcrumb;
use crate::components::print::PrintHeading;
use crate::components::tree_cache::{fetch_tree_cached, use_current_person, use_tree_cache};
use crate::components::tree_icon_sidebar::ToolPageSidebar;
use crate::ui_observability::use_ui_resource;
use crate::utils::use_synced;

/// The tree a page is about, as [`use_tree_page`] loads it.
#[derive(Clone)]
pub struct TreePage {
    /// The route's tree id, when it is one.
    pub tree_id: Option<Uuid>,
    /// The tree's metadata as fetched, for the pages that read its state.
    pub resource: Resource<Option<Result<Tree, ApiError>>>,
    /// The tree as loaded or, while it loads, as cached: a breadcrumb never
    /// flashes a loading label.
    pub tree: Option<Tree>,
    /// The person the sidebar's profile and pedigree buttons act on: the
    /// one last shown in this tree, else its SOSA root.
    pub selected_person_id: Option<Uuid>,
}

impl TreePage {
    /// The tree's name, empty until it is known.
    pub fn name(&self) -> String {
        self.tree
            .as_ref()
            .map(|tree| tree.name.clone())
            .unwrap_or_default()
    }
}

/// Loads the tree named by the route's `tree_id` through the tree cache,
/// again whenever the cache is invalidated or the route names another tree
/// — the router reuses a page's instance across navigations.
pub fn use_tree_page(tree_id: &str) -> TreePage {
    let api = use_context::<ApiClient>();
    let tree_cache = use_tree_cache();
    let current_person = use_current_person();
    let tid = use_synced(tree_id.parse::<Uuid>().ok());
    let resource = use_ui_resource("tree", move || {
        let api = api.clone();
        let _generation = tree_cache.generation();
        let tid = tid();
        async move { Some(fetch_tree_cached(&api, &tree_cache, tid?).await) }
    });
    let tree = {
        let loaded = resource.read();
        let loaded = loaded
            .as_ref()
            .and_then(Option::as_ref)
            .and_then(|tree| tree.as_ref().ok());
        tree_cache.loaded_or_cached(tid(), loaded)
    };
    let selected_person_id = tid()
        .and_then(|tid| current_person.get(tid))
        .or_else(|| tree.as_ref().and_then(|tree| tree.sosa_root_person_id));
    TreePage {
        tree_id: tid(),
        resource,
        tree,
        selected_person_id,
    }
}

/// The frame of a page of a tree: the topbar with the breadcrumb (the logo,
/// the tree, `crumbs`, then `title`), the page's own `topbar` controls and
/// the heading it prints under, then the left icon sidebar beside the
/// scrollable `children`.
///
/// The sidebar is the tool pages' one, acting on `selected_person_id`,
/// unless the page brings its own `sidebar`.
#[component]
pub fn ToolPageFrame(
    tree_id: String,
    tree_name: String,
    /// The breadcrumb's last crumb, and the printed title unless
    /// `print_title` names another.
    title: String,
    /// Crumbs between the tree and `title`.
    #[props(default = VNode::empty())]
    crumbs: Element,
    /// Controls of the topbar, after the breadcrumb.
    #[props(default = VNode::empty())]
    topbar: Element,
    print_title: Option<String>,
    /// Whether the page prints; one that does not has no printed heading.
    #[props(default = true)]
    printed: bool,
    selected_person_id: Option<Uuid>,
    sidebar: Option<Element>,
    /// Classes added to the page and to its content area.
    #[props(default)]
    page_class: String,
    #[props(default)] content_class: String,
    children: Element,
) -> Element {
    let print_title = print_title.unwrap_or_else(|| title.clone());
    rsx! {
        div { class: "sub-page {page_class}",
            div { class: "td-topbar",
                TreeBreadcrumb {
                    tree_id: tree_id.clone(),
                    tree_name: tree_name.clone(),
                    {crumbs}
                    span { class: "td-bc-current", "{title}" }
                }
                {topbar}
                if printed {
                    PrintHeading { tree_name, title: print_title }
                }
            }
            div { class: "pd-page-shell",
                if let Some(sidebar) = sidebar {
                    {sidebar}
                } else {
                    ToolPageSidebar { tree_id, selected_person_id }
                }
                div { class: "sub-page-content {content_class}", {children} }
            }
        }
    }
}
