//! Axum router combining REST routes under `/api/v1` and GraphQL at `/graphql`.

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::routing::{delete, get, patch, post, put};

/// Body limit for the Geneanet wizard's calls, its import included.
///
/// These bodies carry the base64 `.gw` and the collected person↔photo mapping:
/// a 10 000-person tree is around 8 MiB encoded, plus a couple more for the
/// mapping. 32 MiB is several times that and does not grow with how many
/// photographs somebody owns — the media themselves are passed as paths, so
/// this number depends on tree size alone. A genealogy file is never sent in
/// one body: it is streamed to an import job, which enforces its own limit.
const GENEANET_BODY_LIMIT: usize = 32 * 1024 * 1024;

/// Body limit for the Geneanet import itself (1 GiB).
///
/// The import body is the preview's plus the fetched-media map, and that map
/// is what outgrows the wizard's allowance on a real tree: every deposit and
/// view a large Geneanet account holds is listed in it. A real tree's import
/// needed more than 32 MiB, so the import keeps the 1 GiB ceiling the other
/// imports had; the media bytes still never travel in it.
const GENEANET_IMPORT_BODY_LIMIT: usize = 1024 * 1024 * 1024;

use tower_http::compression::CompressionLayer;
use tower_http::compression::predicate::{DefaultPredicate, NotForContentType, Predicate};

/// Which responses are worth compressing: tower-http's default (not tiny,
/// not an image other than SVG, not a stream) less the formats that are
/// compressed already or are opaque bytes — an archive, a PDF, a video or a
/// sound, a raw download. Gzipping those spends CPU on every request and
/// gains nothing, or even grows them.
fn compressible() -> impl Predicate {
    DefaultPredicate::new()
        .and(NotForContentType::const_new("application/zip"))
        .and(NotForContentType::const_new("application/gzip"))
        .and(NotForContentType::const_new("application/pdf"))
        .and(NotForContentType::const_new("application/octet-stream"))
        .and(NotForContentType::const_new("video/"))
        .and(NotForContentType::const_new("audio/"))
}

#[cfg(feature = "graphql")]
use crate::graphql::{graphql_handler, graphql_playground};
use crate::rest::citation;
use crate::rest::dictionary;
use crate::rest::event;
use crate::rest::family;
use crate::rest::family_member;
use crate::rest::file_export;
use crate::rest::file_import;
use crate::rest::gedcom;
use crate::rest::geneanet;
use crate::rest::history;
use crate::rest::media;
use crate::rest::media_link;
use crate::rest::note;
use crate::rest::openapi;
use crate::rest::person;
use crate::rest::person_name;
use crate::rest::place;
use crate::rest::profile;
use crate::rest::reference;
use crate::rest::repository;
use crate::rest::source;
use crate::rest::state::AppState;
use crate::rest::tree;
use crate::rest::tree_guard;
use crate::rest::vignette;

/// Build the complete API router.
pub fn build_router(state: AppState) -> Router {
    let tree_routes = Router::new()
        .route("/", get(tree::list_trees).post(tree::create_tree))
        // Names no tree, so the tree guard lets it through; the handler
        // checks each tree it is asked about.
        .route("/recent-persons", get(tree::recent_persons))
        .route(
            "/{tree_id}",
            get(tree::get_tree)
                .put(tree::update_tree)
                .delete(tree::delete_tree),
        )
        .route("/{tree_id}/duplicate", post(tree::duplicate_tree));

    let person_routes = Router::new()
        .route(
            "/{tree_id}/persons",
            get(person::list_persons).post(person::create_person),
        )
        .route("/{tree_id}/persons/search", get(person::search_persons))
        .route(
            "/{tree_id}/persons/recently-modified",
            get(person::list_recently_modified),
        )
        .route("/{tree_id}/portraits", get(person::list_portraits))
        .route(
            "/{tree_id}/portrait-images",
            post(person::load_portrait_images),
        )
        .route(
            "/{tree_id}/persons/{person_id}/portrait",
            put(person::set_person_portrait),
        )
        .route(
            "/{tree_id}/persons/sosa/{number}",
            get(person::get_person_by_sosa),
        )
        .route(
            "/{tree_id}/persons/{person_id}",
            get(person::get_person)
                .put(person::update_person)
                .delete(person::delete_person),
        )
        .route(
            "/{tree_id}/persons/{person_id}/homonyms",
            get(person::list_homonyms),
        )
        .route(
            "/{tree_id}/persons/{person_id}/distinct",
            post(person::mark_persons_distinct),
        )
        .route(
            "/{tree_id}/persons/{person_id}/merge",
            post(person::merge_persons),
        )
        .route(
            "/{tree_id}/persons/{person_id}/ancestors",
            get(person::get_ancestors),
        )
        .route(
            "/{tree_id}/persons/{person_id}/descendants",
            get(person::get_descendants),
        )
        .route(
            "/{tree_id}/persons/{person_id}/kinship/{other_person_id}",
            get(person::get_kinship),
        );

    let person_name_routes = Router::new()
        .route(
            "/{tree_id}/relation-labels",
            post(person_name::relation_labels),
        )
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
        )
        .route(
            "/{tree_id}/families/{family_id}/detail-bundle",
            get(family::get_couple_detail_bundle),
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
        )
        .route(
            "/{tree_id}/events/{event_id}/witnesses",
            get(event::list_witnesses).post(event::add_witness),
        )
        .route(
            "/{tree_id}/events/{event_id}/witnesses/{witness_id}",
            delete(event::remove_witness),
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
        )
        .route(
            "/{tree_id}/sources/{source_id}/repositories",
            get(repository::list_source_repositories).post(repository::add_source_repository),
        )
        .route(
            "/{tree_id}/sources/{source_id}/repositories/{link_id}",
            put(repository::update_source_repository).delete(repository::remove_source_repository),
        )
        .route(
            "/{tree_id}/repositories",
            get(repository::list_repositories).post(repository::create_repository),
        )
        .route(
            "/{tree_id}/repositories/{repository_id}",
            get(repository::get_repository)
                .put(repository::update_repository)
                .delete(repository::delete_repository),
        )
        .route(
            "/{tree_id}/repositories/{repository_id}/sources",
            get(repository::list_repository_sources),
        );

    let citation_routes = Router::new()
        .route(
            "/{tree_id}/citations",
            get(citation::list_citations).post(citation::create_citation),
        )
        .route(
            "/{tree_id}/citations/{citation_id}",
            put(citation::update_citation).delete(citation::delete_citation),
        );

    let media_routes = Router::new()
        .route("/{tree_id}/gallery-bundle", post(media::gallery_bundle))
        .route("/{tree_id}/image-data", post(media::image_data))
        .route(
            "/{tree_id}/media",
            get(media::list_media).post(media::create_media),
        )
        // Declared before `/{media_id}` so `upload` is matched as the literal
        // segment it is rather than parsed as a UUID and rejected.
        .route(
            "/{tree_id}/media/upload",
            post(media::upload_media)
                // Only this route lifts the body limit, and only to the
                // upload ceiling — every other endpoint keeps Axum's default.
                .layer(DefaultBodyLimit::max(media::UPLOAD_BODY_LIMIT)),
        )
        .route(
            "/{tree_id}/media/{media_id}/deletion-status",
            get(media::media_deletion_status),
        )
        .route(
            "/{tree_id}/media/{media_id}",
            get(media::get_media)
                .put(media::update_media)
                .delete(media::delete_media),
        )
        .route("/{tree_id}/media/{media_id}/tags", post(media::add_tag))
        .route(
            "/{tree_id}/media/{media_id}/tags/{tag}",
            delete(media::remove_tag),
        )
        // Before `/{media_id}`, same reason as `upload`.
        .route("/{tree_id}/media/document", post(media::create_document))
        .route("/{tree_id}/media/facets", get(media::list_media_facets))
        .route(
            "/{tree_id}/media/{media_id}/pages",
            get(media::list_pages).put(media::reorder_pages),
        )
        .route(
            "/{tree_id}/media/{media_id}/pages/{page_id}",
            delete(media::delete_page),
        )
        .route(
            "/{tree_id}/media/{media_id}/file",
            get(media::download_media),
        )
        .route(
            "/{tree_id}/media/{media_id}/download",
            get(media::download_attachment),
        )
        .route(
            "/{tree_id}/media/{media_id}/archive",
            get(media::download_archive),
        )
        .route(
            "/{tree_id}/media/{media_id}/thumbnail",
            get(media::download_thumbnail),
        )
        .route(
            "/{tree_id}/media/{media_id}/vignettes",
            get(vignette::list_media_vignettes).post(vignette::create_vignette),
        );

    let vignette_routes = Router::new()
        .route("/{tree_id}/vignettes", get(vignette::list_vignettes))
        .route(
            "/{tree_id}/vignettes/{vignette_id}",
            get(vignette::get_vignette)
                .put(vignette::update_vignette)
                .delete(vignette::delete_vignette),
        )
        .route(
            "/{tree_id}/vignettes/{vignette_id}/image",
            get(vignette::vignette_image),
        );

    let media_link_routes = Router::new()
        .route(
            "/{tree_id}/media-links",
            get(media_link::list_media_links).post(media_link::create_media_link),
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

    let dictionary_routes = Router::new()
        .route(
            "/{tree_id}/dictionary/family-names",
            get(dictionary::family_names),
        )
        .route(
            "/{tree_id}/dictionary/family-names/usage",
            get(dictionary::family_name_usage),
        )
        .route(
            "/{tree_id}/dictionary/family-names/particle",
            patch(dictionary::set_family_name_particle),
        )
        .route(
            "/{tree_id}/dictionary/family-names/rename",
            patch(dictionary::rename_family_name),
        )
        .route(
            "/{tree_id}/dictionary/occupations",
            get(dictionary::occupations),
        )
        .route(
            "/{tree_id}/dictionary/occupations/usage",
            get(dictionary::occupation_usage),
        )
        .route("/{tree_id}/dictionary/sources", get(dictionary::sources))
        .route(
            "/{tree_id}/dictionary/sources/groups",
            get(dictionary::source_groups),
        )
        .route(
            "/{tree_id}/dictionary/sources/{source_id}/usage",
            get(dictionary::source_usage),
        )
        .route("/{tree_id}/dictionary/places", get(dictionary::places))
        .route(
            "/{tree_id}/dictionary/places/{place_id}/usage",
            get(dictionary::place_usage),
        )
        .route(
            "/{tree_id}/statistics",
            get(crate::rest::statistics::statistics),
        )
        .route(
            "/{tree_id}/statistics/growth",
            get(crate::rest::statistics::growth),
        )
        .route(
            "/{tree_id}/ancestry-completeness",
            get(crate::rest::tools::ancestry_completeness),
        )
        .route(
            "/{tree_id}/anomalies",
            get(crate::rest::tools::tree_anomalies),
        )
        .route(
            "/{tree_id}/duplicates",
            get(crate::rest::tools::potential_duplicates),
        )
        .route(
            "/{tree_id}/unlocated-places",
            get(crate::rest::tools::unlocated_places),
        )
        .route(
            "/{tree_id}/suggestions/{field}",
            get(crate::rest::suggestion::suggest),
        );

    let profile_routes = Router::new()
        .route(
            "/{tree_id}/persons/{person_id}/detail-bundle",
            get(profile::get_person_detail_bundle),
        )
        .route(
            "/{tree_id}/profiles",
            get(profile::get_person_profiles).delete(profile::drop_tree_profiles),
        )
        .route(
            "/{tree_id}/profiles/rebuild",
            post(profile::rebuild_tree_profiles),
        )
        .route(
            "/{tree_id}/profiles/rebuild/{person_id}",
            post(profile::rebuild_person_profile),
        )
        // Declared after the fixed `rebuild` segment so it wins.
        .route(
            "/{tree_id}/profiles/{person_id}",
            get(profile::get_person_profile),
        )
        .route("/{tree_id}/pedigree", get(profile::get_default_pedigree))
        .route(
            "/{tree_id}/pedigree/{root_person_id}",
            get(profile::get_pedigree),
        )
        .route("/{tree_id}/pedigrees", post(profile::load_pedigrees))
        .route(
            "/{tree_id}/pedigree/{root_person_id}/expand",
            get(profile::expand_pedigree),
        );

    let history_routes = Router::new()
        .route("/{tree_id}/audit", get(history::list_audit))
        .route("/{tree_id}/audit/{entry_id}", get(history::get_audit_entry))
        .route(
            "/{tree_id}/audit/{entry_id}/changes",
            get(history::list_audit_changes),
        )
        .route(
            "/{tree_id}/history/{record_type}/{record_id}",
            get(history::list_versions),
        )
        // Declared before `/{version}` so `revert` is not parsed as a number.
        .route(
            "/{tree_id}/history/{record_type}/{record_id}/revert",
            post(history::revert_record),
        )
        .route(
            "/{tree_id}/history/{record_type}/{record_id}/{version}",
            get(history::get_version),
        );

    let import_export_routes = Router::new()
        .route("/{tree_id}/export-jobs", post(file_export::start))
        // A fixed segment, which the router matches before the `{job_id}`
        // beside it.
        .route(
            "/{tree_id}/export-jobs/downloadable",
            get(file_export::downloadable),
        )
        .route("/{tree_id}/export-jobs/{job_id}", get(file_export::status))
        .route(
            "/{tree_id}/export-jobs/{job_id}/download",
            get(file_export::download),
        )
        .route(
            "/{tree_id}/import-jobs",
            post(file_import::start)
                .layer(DefaultBodyLimit::max(file_import::FILE_IMPORT_BODY_LIMIT)),
        )
        .route("/{tree_id}/import-jobs/{job_id}", get(file_import::status))
        // GEDZIP is the archive form of the same export, so it rides on
        // `gedcom/export?format=gedzip` rather than a route of its own.
        .route(
            "/{tree_id}/gedcom/export",
            get(gedcom::export_gedcom_handler),
        )
        // The wizard's import body is its preview body plus the fetched-media
        // map, which is what a large account fills: see
        // `GENEANET_IMPORT_BODY_LIMIT`.
        .route(
            "/{tree_id}/geneanet/import",
            post(geneanet::import_handler).layer(DefaultBodyLimit::max(GENEANET_IMPORT_BODY_LIMIT)),
        );

    // The wizard's first steps run before a tree has been chosen — indeed
    // before the user has decided whether to create one — so they cannot sit
    // under the tree-scoped nest.
    let geneanet_routes = Router::new()
        .route("/archives", post(geneanet::index_archives_handler))
        .route("/preview", post(geneanet::preview_handler))
        .route("/plan", post(geneanet::plan_handler))
        .route("/session/encode", post(geneanet::encode_session_handler))
        .route(
            "/session/release",
            post(geneanet::release_session_media_handler),
        )
        .route(
            "/session/decode",
            post(geneanet::decode_session_handler)
                // Saved sessions contain media bytes, not only metadata paths.
                .layer(DefaultBodyLimit::disable()),
        )
        .layer(DefaultBodyLimit::max(GENEANET_BODY_LIMIT));

    let geneweb_routes = Router::new()
        .route("/inspect", post(geneanet::inspect_geneweb_handler))
        .layer(DefaultBodyLimit::max(GENEANET_BODY_LIMIT));

    // Static reference content (occupation sheets, given-name meanings, the
    // place dictionary) — not tied to a tree, so kept out of the `/trees` nest.
    let reference_routes = Router::new()
        .route("/basemap", get(reference::basemap))
        .route("/{lang}/places", get(reference::places))
        .route("/{lang}/occupations", get(reference::occupation))
        .route("/{lang}/occupations/bundle", post(reference::occupations))
        .route("/{lang}/given-names", get(reference::given_name))
        .route("/{lang}/given-names/bundle", post(reference::given_names));

    #[cfg(feature = "graphql")]
    let schema = crate::graphql::build_schema_with_local_file_access(
        state.connections(),
        state.profiles.clone(),
        state.purge.clone(),
        state.media.clone(),
        state.work_dir.clone(),
        state.local_file_access,
    );

    #[cfg(feature = "graphql")]
    let graphql_body_limit = if state.local_file_access.require().is_ok() {
        // The desktop-only session operation carries an archive in base64.
        DefaultBodyLimit::disable()
    } else {
        DefaultBodyLimit::max(2 * 1024 * 1024)
    };

    let rest_router = Router::new()
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
                .merge(vignette_routes)
                .merge(note_routes)
                .merge(dictionary_routes)
                .merge(profile_routes)
                .merge(history_routes)
                .merge(import_export_routes)
                // Applied to the whole nest so every tree-scoped route gets
                // the same check, including any added later.
                .layer(axum::middleware::from_fn_with_state(
                    state.clone(),
                    tree_guard::require_live_tree,
                )),
        )
        .nest("/api/v1/geneanet", geneanet_routes)
        .nest("/api/v1/geneweb", geneweb_routes)
        .nest("/api/v1/reference", reference_routes)
        .route("/api/v1/openapi.json", get(openapi::spec))
        .fallback(crate::rest::error::unknown_route)
        // Inside the compression layer, so it reads the response as written.
        .layer(axum::middleware::map_response(
            crate::rest::error::envelope_rejections,
        ))
        .with_state(state);

    #[cfg(feature = "graphql")]
    let rest_router = rest_router.merge(
        Router::new()
            .route("/graphql", post(graphql_handler).get(graphql_playground))
            .layer(graphql_body_limit)
            .with_state(schema),
    );

    // Outermost, so GraphQL answers are compressed exactly like REST ones.
    rest_router
        .layer(CompressionLayer::new().compress_when(compressible()))
        .layer(axum::middleware::map_response(
            crate::access::security_headers,
        ))
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::Response;

    use super::*;

    fn compresses(content_type: &str) -> bool {
        let response = Response::builder()
            .header("content-type", content_type)
            .body(Body::from(vec![b'x'; 4096]))
            .unwrap();
        compressible().should_compress(&response)
    }

    #[test]
    fn text_formats_are_compressed_and_packed_or_opaque_ones_are_not() {
        for content_type in [
            "application/json",
            "text/plain; charset=utf-8",
            "image/svg+xml",
        ] {
            assert!(compresses(content_type), "{content_type}");
        }
        for content_type in [
            "application/zip",
            "application/pdf",
            "application/octet-stream",
            "video/mp4",
            "audio/mpeg",
            "image/jpeg",
        ] {
            assert!(!compresses(content_type), "{content_type}");
        }
    }
}
