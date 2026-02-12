//! Axum router combining REST routes under `/api/v1` and GraphQL at `/graphql`.

use axum::Router;
use axum::routing::{delete, get, post, put};

use crate::graphql::{build_schema, graphql_handler, graphql_playground};
use crate::rest::citation;
use crate::rest::event;
use crate::rest::family;
use crate::rest::family_member;
use crate::rest::gedcom;
use crate::rest::media;
use crate::rest::media_link;
use crate::rest::note;
use crate::rest::person;
use crate::rest::person_name;
use crate::rest::place;
use crate::rest::source;
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
            get(family_member::list_spouses).post(family_member::add_spouse),
        )
        .route(
            "/{tree_id}/families/{family_id}/spouses/{spouse_id}",
            delete(family_member::remove_spouse),
        )
        .route(
            "/{tree_id}/families/{family_id}/children",
            get(family_member::list_children).post(family_member::add_child),
        )
        .route(
            "/{tree_id}/families/{family_id}/children/{child_id}",
            delete(family_member::remove_child),
        );

    let event_routes = Router::new()
        .route(
            "/{tree_id}/events",
            get(event::list_events).post(event::create_event),
        )
        .route(
            "/{tree_id}/events/{event_id}",
            get(event::get_event)
                .put(event::update_event)
                .delete(event::delete_event),
        );

    let place_routes = Router::new()
        .route(
            "/{tree_id}/places",
            get(place::list_places).post(place::create_place),
        )
        .route(
            "/{tree_id}/places/{place_id}",
            get(place::get_place)
                .put(place::update_place)
                .delete(place::delete_place),
        );

    let source_routes = Router::new()
        .route(
            "/{tree_id}/sources",
            get(source::list_sources).post(source::create_source),
        )
        .route(
            "/{tree_id}/sources/{source_id}",
            get(source::get_source)
                .put(source::update_source)
                .delete(source::delete_source),
        );

    let citation_routes = Router::new()
        .route("/{tree_id}/citations", post(citation::create_citation))
        .route(
            "/{tree_id}/citations/{citation_id}",
            put(citation::update_citation).delete(citation::delete_citation),
        );

    let media_routes = Router::new()
        .route(
            "/{tree_id}/media",
            get(media::list_media).post(media::create_media),
        )
        .route(
            "/{tree_id}/media/{media_id}",
            get(media::get_media)
                .put(media::update_media)
                .delete(media::delete_media),
        );

    let media_link_routes = Router::new()
        .route(
            "/{tree_id}/media-links",
            post(media_link::create_media_link),
        )
        .route(
            "/{tree_id}/media-links/{link_id}",
            delete(media_link::delete_media_link),
        );

    let note_routes = Router::new()
        .route(
            "/{tree_id}/notes",
            get(note::list_notes).post(note::create_note),
        )
        .route(
            "/{tree_id}/notes/{note_id}",
            get(note::get_note)
                .put(note::update_note)
                .delete(note::delete_note),
        );

    let gedcom_routes = Router::new()
        .route("/{tree_id}/import", post(gedcom::import_gedcom_handler))
        .route("/{tree_id}/export", get(gedcom::export_gedcom_handler));

    // Build GraphQL schema
    let schema = build_schema(state.db.clone());

    let graphql_routes = Router::new()
        .route("/graphql", post(graphql_handler).get(graphql_playground))
        .with_state(schema);

    // Nest REST under /api/v1/trees, GraphQL at /graphql
    Router::new()
        .nest(
            "/api/v1/trees",
            tree_routes
                .merge(person_routes)
                .merge(person_name_routes)
                .merge(family_routes)
                .merge(family_member_routes)
                .merge(event_routes)
                .merge(place_routes)
                .merge(source_routes)
                .merge(citation_routes)
                .merge(media_routes)
                .merge(media_link_routes)
                .merge(note_routes)
                .merge(gedcom_routes),
        )
        .with_state(state)
        .merge(graphql_routes)
}
