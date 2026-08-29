//! Axum router combining REST routes under `/api/v1` and GraphQL at `/graphql`.

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::routing::{delete, get, patch, post, put};

/// Body limit for the Geneanet wizard's calls and the imports beside them.
///
/// These bodies carry the base64 `.gw` and the collected person↔photo mapping:
/// a 10 000-person tree is around 8 MiB encoded, plus a couple more for the
/// mapping. 32 MiB is several times that and does not grow with how many
/// photographs somebody owns — the media themselves are passed as paths, so
/// this number depends on tree size alone.
const GENEANET_BODY_LIMIT: usize = 32 * 1024 * 1024;

/// Body limit for importing a genealogy file — `.ged` or `.gw` (1 GiB).
///
/// This one tracks the size of a file somebody hands us rather than the size
/// of their tree, and the two part company badly at the top end: Geneanet
/// accepts a **zipped** GEDCOM of 350 MB, so an unzipped file a user drops on
/// us is not unusual for being larger still. 1 GiB is comfortably past that
/// and past anything a tree's text alone plausibly reaches. The JSON-wrapped
/// GEDCOM import pays a further ~1.4× for the string escaping, which is the
/// one path here still bounded by tree size rather than by this.
const IMPORT_BODY_LIMIT: usize = 1024 * 1024 * 1024;

/// Body limit for a GEDZIP import (1 GiB).
///
/// Unlike every other body here, this one is mostly photographs: a `.gdz` is a
/// tree's genealogy plus its entire media library in one file. Real ones run
/// to hundreds of MiB — an export of a tree with a decade of scans behind it
/// lands around 650. The direct compatibility endpoint still takes one body;
/// the UI sends large files through `import-jobs`, which streams them to disk.
///
/// It overrides [`IMPORT_BODY_LIMIT`] on its own route rather than inheriting
/// it, so the two have to be kept in step by hand; the assertion below is
/// there because raising the text limit alone leaves the archive — the larger
/// file of the two, by definition — refused at a ceiling nobody remembered.
/// Anything past this requires a resumable/chunked-upload protocol.
const GEDZIP_BODY_LIMIT: usize = 1024 * 1024 * 1024;

/// A `.gdz` is a `.ged` with the album added, so its ceiling cannot be the
/// lower of the two.
const _: () = assert!(GEDZIP_BODY_LIMIT >= IMPORT_BODY_LIMIT);
use tower_http::compression::CompressionLayer;

#[cfg(feature = "graphql")]
use crate::graphql::{build_schema, graphql_handler, graphql_playground};
use crate::rest::citation;
use crate::rest::dictionary;
use crate::rest::event;
use crate::rest::family;
use crate::rest::family_member;
use crate::rest::file_import;
use crate::rest::gedcom;
use crate::rest::geneanet;
use crate::rest::geneweb;
use crate::rest::media;
use crate::rest::media_link;
use crate::rest::note;
use crate::rest::person;
use crate::rest::person_name;
use crate::rest::place;
use crate::rest::profile;
use crate::rest::reference;
use crate::rest::snapshot;
use crate::rest::source;
use crate::rest::state::AppState;
use crate::rest::tree;
use crate::rest::tree_guard;
use crate::rest::vignette;

/// Build the complete API router.
pub fn build_router(state: AppState) -> Router {
    let tree_routes = Router::new()
        .route("/", get(tree::list_trees).post(tree::create_tree))
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
        .route("/{tree_id}/portraits", get(person::list_portraits))
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
        .route(
            "/{tree_id}/media/{media_id}/tags",
            post(media::add_tag).delete(media::remove_tag),
        )
        // Before `/{media_id}`, same reason as `upload`.
        .route("/{tree_id}/media/document", post(media::create_document))
        .route(
            "/{tree_id}/media/{media_id}/pages",
            get(media::list_pages).put(media::reorder_pages),
        )
        .route(
            "/{tree_id}/media/{media_id}/pages/{page_id}",
            delete(media::detach_page),
        )
        .route(
            "/{tree_id}/media/{media_id}/file",
            get(media::download_media),
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

    let snapshot_routes = Router::new().route("/{tree_id}/snapshot", get(snapshot::tree_snapshot));

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
        );

    let profile_routes = Router::new()
        .route("/{tree_id}/profiles", get(profile::get_person_profiles))
        .route(
            "/{tree_id}/profiles/rebuild",
            post(profile::rebuild_tree_profiles),
        )
        .route(
            "/{tree_id}/profiles/rebuild/{person_id}",
            post(profile::rebuild_person_profile),
        )
        .route(
            "/{tree_id}/profiles/drop",
            post(profile::drop_tree_profiles),
        )
        // Declared after the fixed `rebuild` / `drop` segments so those win.
        .route(
            "/{tree_id}/profiles/{person_id}",
            get(profile::get_person_profile),
        )
        .route(
            "/{tree_id}/pedigree/{root_person_id}",
            get(profile::get_pedigree),
        )
        .route(
            "/{tree_id}/pedigree/{root_person_id}/expand",
            patch(profile::expand_pedigree),
        );

    let import_export_routes = Router::new()
        .route(
            "/{tree_id}/import-jobs",
            post(file_import::start)
                .layer(DefaultBodyLimit::max(file_import::FILE_IMPORT_BODY_LIMIT)),
        )
        .route("/{tree_id}/import-jobs/{job_id}", get(file_import::status))
        .route(
            "/{tree_id}/gedcom/import",
            post(gedcom::import_gedcom_handler),
        )
        .route(
            "/{tree_id}/gedcom/export",
            get(gedcom::export_gedcom_handler),
        )
        // GEDZIP is `.gdz` in and `.gdz` out — the archive form of the same
        // export, so it rides on `gedcom/export?format=gedzip` rather than a
        // route of its own. Only the import needs one, because it takes raw
        // bytes where the GEDCOM import takes JSON.
        .route(
            "/{tree_id}/gedzip/import",
            post(gedcom::import_gedzip_handler)
                // The archive carries a tree's whole photo album, so this is
                // the one import whose size tracks how much media somebody has
                // rather than how many people. Set well above the group limit
                // below, which this inner layer overrides.
                .layer(DefaultBodyLimit::max(GEDZIP_BODY_LIMIT)),
        )
        // GeneWeb is import-only — `.gw` is a format OxidGene reads, not writes.
        .route(
            "/{tree_id}/geneweb/import",
            post(geneweb::import_geneweb_handler),
        )
        .route("/{tree_id}/geneanet/import", post(geneanet::import_handler))
        // Sized for the file, not for the tree: a `.ged` or `.gw` big enough
        // to be interesting is nothing like small, and the wizard's bodies —
        // which bundle the base64 `.gw` with the collected mapping, several
        // times a bare import — fit inside the same allowance with room over.
        .layer(DefaultBodyLimit::max(IMPORT_BODY_LIMIT));

    // The wizard's first steps run before a tree has been chosen — indeed
    // before the user has decided whether to create one — so they cannot sit
    // under the tree-scoped nest.
    let geneanet_routes = Router::new()
        .route("/archives", post(geneanet::index_archives_handler))
        .route("/preview", post(geneanet::preview_handler))
        .route("/plan", post(geneanet::plan_handler))
        .route("/session/encode", post(geneanet::encode_session_handler))
        .route("/session/decode", post(geneanet::decode_session_handler))
        .route(
            "/import/{progress_id}",
            get(geneanet::import_progress_handler),
        )
        .layer(DefaultBodyLimit::max(GENEANET_BODY_LIMIT));

    let geneweb_routes = Router::new()
        .route("/inspect", post(geneanet::inspect_geneweb_handler))
        .layer(DefaultBodyLimit::max(GENEANET_BODY_LIMIT));

    // Static reference content (occupation sheets, given-name meanings) —
    // not tied to a tree, so kept out of the `/trees` nest.
    let reference_routes = Router::new()
        .route("/{lang}/occupations", get(reference::occupation))
        .route("/{lang}/given-names", get(reference::given_name));

    #[cfg(feature = "graphql")]
    let schema = build_schema(
        state.db.clone(),
        state.profiles.clone(),
        state.purge.clone(),
        state.media.clone(),
        state.imports.clone(),
    );

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
                .merge(snapshot_routes)
                .merge(dictionary_routes)
                .merge(profile_routes)
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
        .layer(CompressionLayer::new())
        .with_state(state);

    #[cfg(feature = "graphql")]
    {
        let graphql_routes = Router::new()
            .route("/graphql", post(graphql_handler).get(graphql_playground))
            .with_state(schema);
        rest_router.merge(graphql_routes)
    }

    #[cfg(not(feature = "graphql"))]
    rest_router
}
