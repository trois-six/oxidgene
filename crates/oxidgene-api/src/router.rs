//! Axum router combining all REST routes under `/api/v1`.

use axum::Router;
use axum::routing::{delete, get, post, put};

use crate::rest::family;
use crate::rest::family_member;
use crate::rest::person;
use crate::rest::person_name;
use crate::rest::state::AppState;
use crate::rest::tree;

/// Build the complete API router.
pub fn build_router(state: AppState) -> Router {
    let tree_routes = Router::new()
        .route("/", get(tree::list_trees).post(tree::create_tree))
        .route(
            "/{tree_id}",
            get(tree::get_tree)
                .put(tree::update_tree)
                .delete(tree::delete_tree),
        );

    let person_routes = Router::new()
        .route(
            "/{tree_id}/persons",
            get(person::list_persons).post(person::create_person),
        )
        .route(
            "/{tree_id}/persons/{person_id}",
            get(person::get_person)
                .put(person::update_person)
                .delete(person::delete_person),
        )
        .route(
            "/{tree_id}/persons/{person_id}/ancestors",
            get(person::get_ancestors),
        )
        .route(
            "/{tree_id}/persons/{person_id}/descendants",
            get(person::get_descendants),
        );

    let person_name_routes = Router::new()
        .route(
            "/{tree_id}/persons/{person_id}/names",
            get(person_name::list_person_names).post(person_name::create_person_name),
        )
        .route(
            "/{tree_id}/persons/{person_id}/names/{name_id}",
            put(person_name::update_person_name).delete(person_name::delete_person_name),
        );

    let family_routes = Router::new()
        .route(
            "/{tree_id}/families",
            get(family::list_families).post(family::create_family),
        )
        .route(
            "/{tree_id}/families/{family_id}",
            get(family::get_family)
                .put(family::update_family)
                .delete(family::delete_family),
        );

    let family_member_routes = Router::new()
        .route(
            "/{tree_id}/families/{family_id}/spouses",
            post(family_member::add_spouse),
        )
        .route(
            "/{tree_id}/families/{family_id}/spouses/{spouse_id}",
            delete(family_member::remove_spouse),
        )
        .route(
            "/{tree_id}/families/{family_id}/children",
            post(family_member::add_child),
        )
        .route(
            "/{tree_id}/families/{family_id}/children/{child_id}",
            delete(family_member::remove_child),
        );

    // Nest everything under /api/v1/trees
    Router::new()
        .nest(
            "/api/v1/trees",
            tree_routes
                .merge(person_routes)
                .merge(person_name_routes)
                .merge(family_routes)
                .merge(family_member_routes),
        )
        .with_state(state)
}
