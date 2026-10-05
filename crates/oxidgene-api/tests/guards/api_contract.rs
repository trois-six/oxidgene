//! The API contract guards: the REST router, its GraphQL twin and
//! docs/api.md describe the same operations.
//!
//! Drift they prevent:
//!
//! - a REST route without its GraphQL operation, or the reverse — the
//!   strict symmetry AGENTS.md requires (`PARITY` below is the declared
//!   mapping, with the few transport exceptions docs/api.md allows);
//! - a route missing from docs/api.md's tables, or a documented route that
//!   no longer exists;
//! - a GraphQL schema change nobody reviewed: the SDL is committed as
//!   docs/schema.graphql.
//!
//! The route list is the OpenAPI document the build script generates from
//! the router (`GET /api/v1/openapi.json`); the GraphQL side is the schema's
//! SDL.
//!
//! Fixing a failure: add the new route to `PARITY` with the GraphQL
//! operations that mirror it (`Query.field`, `Mutation.field`, or a nested
//! `Type.field`) — or, only for a transport exception docs/api.md allows, a
//! `RestOnly` reason; add its row to docs/api.md; after an intended schema
//! change run `just graphql-schema` and commit docs/schema.graphql.

use std::collections::BTreeSet;

use axum::http::Method;

use crate::common::{app_on, ok, setup_db};

/// What mirrors a REST route on the GraphQL side.
enum Twin {
    /// The GraphQL fields, `Root.field` or `Type.field`.
    Gql(&'static [&'static str]),
    /// A transport exception docs/api.md (Surfaces and parity) allows.
    RestOnly(&'static str),
}

use Twin::{Gql, RestOnly};

const BINARY: &str = "direct media read: an HTTP representation with its cache validators (docs/api.md, Surfaces and parity)";

/// Every REST operation, as the router declares it, with its twin.
const PARITY: &[(&str, &str, Twin)] = &[
    (
        "POST",
        "/api/v1/geneanet/archives",
        Gql(&["Query.indexGeneanetArchives"]),
    ),
    (
        "POST",
        "/api/v1/geneanet/plan",
        Gql(&["Query.geneanetPlan"]),
    ),
    (
        "POST",
        "/api/v1/geneanet/preview",
        Gql(&["Query.geneanetPreview"]),
    ),
    (
        "POST",
        "/api/v1/geneanet/session/decode",
        Gql(&["Mutation.decodeGeneanetSession"]),
    ),
    (
        "POST",
        "/api/v1/geneanet/session/encode",
        Gql(&["Mutation.encodeGeneanetSession"]),
    ),
    (
        "POST",
        "/api/v1/geneanet/session/release",
        Gql(&["Mutation.releaseGeneanetSessionMedia"]),
    ),
    (
        "POST",
        "/api/v1/geneweb/inspect",
        Gql(&["Query.inspectGeneweb"]),
    ),
    (
        "GET",
        "/api/v1/openapi.json",
        RestOnly("describes the REST surface; GraphQL has introspection"),
    ),
    ("GET", "/api/v1/reference/basemap", Gql(&["Query.basemap"])),
    (
        "GET",
        "/api/v1/reference/{lang}/given-names",
        Gql(&["Query.givenNameReference"]),
    ),
    (
        "POST",
        "/api/v1/reference/{lang}/given-names/bundle",
        Gql(&["Query.givenNameReferences"]),
    ),
    (
        "GET",
        "/api/v1/reference/{lang}/occupations",
        Gql(&["Query.occupationReference"]),
    ),
    (
        "POST",
        "/api/v1/reference/{lang}/occupations/bundle",
        Gql(&["Query.occupationReferences"]),
    ),
    (
        "GET",
        "/api/v1/reference/{lang}/places",
        Gql(&["Query.placeSuggestions"]),
    ),
    ("GET", "/api/v1/trees", Gql(&["Query.trees"])),
    ("POST", "/api/v1/trees", Gql(&["Mutation.createTree"])),
    (
        "GET",
        "/api/v1/trees/recent-persons",
        Gql(&["Query.recentPersonsOfTrees"]),
    ),
    (
        "DELETE",
        "/api/v1/trees/{tree_id}",
        Gql(&["Mutation.deleteTree"]),
    ),
    ("GET", "/api/v1/trees/{tree_id}", Gql(&["Query.tree"])),
    (
        "PUT",
        "/api/v1/trees/{tree_id}",
        Gql(&["Mutation.updateTree"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/ancestry-completeness",
        Gql(&["Query.ancestryCompleteness"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/anomalies",
        Gql(&["Query.treeAnomalies"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/audit",
        Gql(&["Query.auditEntries"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/audit/{entry_id}",
        Gql(&["Query.auditEntry"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/audit/{entry_id}/changes",
        Gql(&["Query.auditEntryChanges"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/citations",
        Gql(&["Query.citations"]),
    ),
    (
        "POST",
        "/api/v1/trees/{tree_id}/citations",
        Gql(&["Mutation.createCitation"]),
    ),
    (
        "DELETE",
        "/api/v1/trees/{tree_id}/citations/{citation_id}",
        Gql(&["Mutation.deleteCitation"]),
    ),
    (
        "PUT",
        "/api/v1/trees/{tree_id}/citations/{citation_id}",
        Gql(&["Mutation.updateCitation"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/dictionary/family-names",
        Gql(&["Query.dictionaryFamilyNames"]),
    ),
    (
        "PATCH",
        "/api/v1/trees/{tree_id}/dictionary/family-names/particle",
        Gql(&["Mutation.setFamilyNameParticle"]),
    ),
    (
        "PATCH",
        "/api/v1/trees/{tree_id}/dictionary/family-names/rename",
        Gql(&["Mutation.renameFamilyName"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/dictionary/family-names/usage",
        Gql(&["Query.familyNameUsage"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/dictionary/occupations",
        Gql(&["Query.dictionaryOccupations"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/dictionary/occupations/usage",
        Gql(&["Query.occupationUsage"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/dictionary/places",
        Gql(&["Query.dictionaryPlaces"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/dictionary/places/{place_id}/usage",
        Gql(&["Query.placeUsage"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/dictionary/sources",
        Gql(&["Query.dictionarySources"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/dictionary/sources/groups",
        Gql(&["Query.dictionarySourceDrill"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/dictionary/sources/{source_id}/usage",
        Gql(&["Query.sourceUsage"]),
    ),
    (
        "POST",
        "/api/v1/trees/{tree_id}/duplicate",
        Gql(&["Mutation.duplicateTree"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/duplicates",
        Gql(&["Query.potentialDuplicates"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/events",
        Gql(&["Query.events"]),
    ),
    (
        "POST",
        "/api/v1/trees/{tree_id}/events",
        Gql(&["Mutation.createEvent"]),
    ),
    (
        "DELETE",
        "/api/v1/trees/{tree_id}/events/{event_id}",
        Gql(&["Mutation.deleteEvent"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/events/{event_id}",
        Gql(&["Query.event"]),
    ),
    (
        "PUT",
        "/api/v1/trees/{tree_id}/events/{event_id}",
        Gql(&["Mutation.updateEvent"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/events/{event_id}/witnesses",
        Gql(&["GqlEvent.witnesses"]),
    ),
    (
        "POST",
        "/api/v1/trees/{tree_id}/events/{event_id}/witnesses",
        Gql(&["Mutation.addEventWitness"]),
    ),
    (
        "DELETE",
        "/api/v1/trees/{tree_id}/events/{event_id}/witnesses/{witness_id}",
        Gql(&["Mutation.removeEventWitness"]),
    ),
    (
        "POST",
        "/api/v1/trees/{tree_id}/export-jobs",
        Gql(&["Mutation.startExportJob"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/export-jobs/downloadable",
        Gql(&["Query.downloadableExport"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/export-jobs/{job_id}",
        Gql(&["Query.exportJobStatus"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/export-jobs/{job_id}/download",
        RestOnly("binary export artifact download (docs/api.md, Surfaces and parity)"),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/families",
        Gql(&["Query.families"]),
    ),
    (
        "POST",
        "/api/v1/trees/{tree_id}/families",
        Gql(&["Mutation.createFamily"]),
    ),
    (
        "DELETE",
        "/api/v1/trees/{tree_id}/families/{family_id}",
        Gql(&["Mutation.deleteFamily"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/families/{family_id}",
        Gql(&["Query.family"]),
    ),
    (
        "PUT",
        "/api/v1/trees/{tree_id}/families/{family_id}",
        Gql(&["Mutation.updateFamily"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/families/{family_id}/children",
        Gql(&["GqlFamily.children"]),
    ),
    (
        "POST",
        "/api/v1/trees/{tree_id}/families/{family_id}/children",
        Gql(&["Mutation.addChild"]),
    ),
    (
        "DELETE",
        "/api/v1/trees/{tree_id}/families/{family_id}/children/{child_id}",
        Gql(&["Mutation.removeChild"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/families/{family_id}/detail-bundle",
        Gql(&["Query.coupleDetailBundle"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/families/{family_id}/spouses",
        Gql(&["GqlFamily.spouses"]),
    ),
    (
        "POST",
        "/api/v1/trees/{tree_id}/families/{family_id}/spouses",
        Gql(&["Mutation.addSpouse"]),
    ),
    (
        "DELETE",
        "/api/v1/trees/{tree_id}/families/{family_id}/spouses/{spouse_id}",
        Gql(&["Mutation.removeSpouse"]),
    ),
    (
        "POST",
        "/api/v1/trees/{tree_id}/gallery-bundle",
        Gql(&["Query.galleryBundle"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/gedcom/export",
        Gql(&["Query.exportGedcom"]),
    ),
    (
        "POST",
        "/api/v1/trees/{tree_id}/geneanet/import",
        Gql(&["Mutation.importGeneanet"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/history/{record_type}/{record_id}",
        Gql(&["Query.recordVersions"]),
    ),
    (
        "POST",
        "/api/v1/trees/{tree_id}/history/{record_type}/{record_id}/revert",
        Gql(&["Mutation.revertRecord"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/history/{record_type}/{record_id}/{version}",
        Gql(&["Query.recordVersion"]),
    ),
    (
        "POST",
        "/api/v1/trees/{tree_id}/image-data",
        Gql(&["Query.imageData"]),
    ),
    (
        "POST",
        "/api/v1/trees/{tree_id}/import-jobs",
        RestOnly(
            "streamed binary upload of an import source (docs/api.md, Surfaces and parity); GraphQL polls the job with importJobStatus",
        ),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/import-jobs/{job_id}",
        Gql(&["Query.importJobStatus"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/media",
        Gql(&["Query.mediaList"]),
    ),
    (
        "POST",
        "/api/v1/trees/{tree_id}/media",
        Gql(&["Mutation.uploadMedia"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/media-links",
        Gql(&[
            "Query.treeMediaLinks",
            "Query.entityMedia",
            "Query.mediaLinks",
        ]),
    ),
    (
        "POST",
        "/api/v1/trees/{tree_id}/media-links",
        Gql(&["Mutation.createMediaLink"]),
    ),
    (
        "DELETE",
        "/api/v1/trees/{tree_id}/media-links/{link_id}",
        Gql(&["Mutation.deleteMediaLink"]),
    ),
    (
        "POST",
        "/api/v1/trees/{tree_id}/media/document",
        Gql(&["Mutation.createMediaDocument"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/media/facets",
        Gql(&["Query.mediaFacets"]),
    ),
    (
        "POST",
        "/api/v1/trees/{tree_id}/media/upload",
        Gql(&["Mutation.uploadMediaFile"]),
    ),
    (
        "DELETE",
        "/api/v1/trees/{tree_id}/media/{media_id}",
        Gql(&["Mutation.deleteMedia"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/media/{media_id}",
        Gql(&["Query.media"]),
    ),
    (
        "PUT",
        "/api/v1/trees/{tree_id}/media/{media_id}",
        Gql(&["Mutation.updateMedia"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/media/{media_id}/archive",
        Gql(&["Query.mediaArchive"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/media/{media_id}/deletion-status",
        Gql(&["Query.canDeleteMedia"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/media/{media_id}/download",
        Gql(&["Query.mediaDownload"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/media/{media_id}/file",
        RestOnly(BINARY),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/media/{media_id}/pages",
        Gql(&["Query.mediaPages"]),
    ),
    (
        "PUT",
        "/api/v1/trees/{tree_id}/media/{media_id}/pages",
        Gql(&["Mutation.reorderMediaPages"]),
    ),
    (
        "DELETE",
        "/api/v1/trees/{tree_id}/media/{media_id}/pages/{page_id}",
        Gql(&["Mutation.deleteMediaPage"]),
    ),
    (
        "POST",
        "/api/v1/trees/{tree_id}/media/{media_id}/tags",
        Gql(&["Mutation.addMediaTag"]),
    ),
    (
        "DELETE",
        "/api/v1/trees/{tree_id}/media/{media_id}/tags/{tag}",
        Gql(&["Mutation.removeMediaTag"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/media/{media_id}/thumbnail",
        RestOnly(BINARY),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/media/{media_id}/vignettes",
        Gql(&["Query.mediaVignettes"]),
    ),
    (
        "POST",
        "/api/v1/trees/{tree_id}/media/{media_id}/vignettes",
        Gql(&["Mutation.createVignette"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/notes",
        Gql(&["Query.notes"]),
    ),
    (
        "POST",
        "/api/v1/trees/{tree_id}/notes",
        Gql(&["Mutation.createNote"]),
    ),
    (
        "DELETE",
        "/api/v1/trees/{tree_id}/notes/{note_id}",
        Gql(&["Mutation.deleteNote"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/notes/{note_id}",
        Gql(&["Query.note"]),
    ),
    (
        "PUT",
        "/api/v1/trees/{tree_id}/notes/{note_id}",
        Gql(&["Mutation.updateNote"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/pedigree",
        Gql(&["Query.pedigree"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/pedigree/{root_person_id}",
        Gql(&["Query.pedigree"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/pedigree/{root_person_id}/expand",
        Gql(&["Query.expandPedigree"]),
    ),
    (
        "POST",
        "/api/v1/trees/{tree_id}/pedigrees",
        Gql(&["Query.pedigrees"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/persons",
        Gql(&["Query.persons"]),
    ),
    (
        "POST",
        "/api/v1/trees/{tree_id}/persons",
        Gql(&["Mutation.createPerson"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/persons/recently-modified",
        Gql(&["Query.recentlyModifiedPersons"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/persons/search",
        Gql(&["Query.searchPersons"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/persons/sosa/{number}",
        Gql(&["Query.personBySosa"]),
    ),
    (
        "DELETE",
        "/api/v1/trees/{tree_id}/persons/{person_id}",
        Gql(&["Mutation.deletePerson"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/persons/{person_id}",
        Gql(&["Query.person"]),
    ),
    (
        "PUT",
        "/api/v1/trees/{tree_id}/persons/{person_id}",
        Gql(&["Mutation.updatePerson"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/persons/{person_id}/ancestors",
        Gql(&["Query.ancestors"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/persons/{person_id}/descendants",
        Gql(&["Query.descendants"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/persons/{person_id}/detail-bundle",
        Gql(&["Query.personDetailBundle"]),
    ),
    (
        "POST",
        "/api/v1/trees/{tree_id}/persons/{person_id}/distinct",
        Gql(&["Mutation.markPersonsDistinct"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/persons/{person_id}/homonyms",
        Gql(&["Query.personHomonyms"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/persons/{person_id}/kinship/{other_person_id}",
        Gql(&["Query.kinship"]),
    ),
    (
        "POST",
        "/api/v1/trees/{tree_id}/persons/{person_id}/merge",
        Gql(&["Mutation.mergePersons"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/persons/{person_id}/names",
        Gql(&["GqlPerson.names"]),
    ),
    (
        "POST",
        "/api/v1/trees/{tree_id}/persons/{person_id}/names",
        Gql(&["Mutation.addPersonName"]),
    ),
    (
        "DELETE",
        "/api/v1/trees/{tree_id}/persons/{person_id}/names/{name_id}",
        Gql(&["Mutation.deletePersonName"]),
    ),
    (
        "PUT",
        "/api/v1/trees/{tree_id}/persons/{person_id}/names/{name_id}",
        Gql(&["Mutation.updatePersonName"]),
    ),
    (
        "PUT",
        "/api/v1/trees/{tree_id}/persons/{person_id}/portrait",
        Gql(&["Mutation.setPersonPortrait"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/places",
        Gql(&["Query.places"]),
    ),
    (
        "POST",
        "/api/v1/trees/{tree_id}/places",
        Gql(&["Mutation.createPlace"]),
    ),
    (
        "DELETE",
        "/api/v1/trees/{tree_id}/places/{place_id}",
        Gql(&["Mutation.deletePlace"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/places/{place_id}",
        Gql(&["Query.place"]),
    ),
    (
        "PUT",
        "/api/v1/trees/{tree_id}/places/{place_id}",
        Gql(&["Mutation.updatePlace"]),
    ),
    (
        "POST",
        "/api/v1/trees/{tree_id}/portrait-images",
        Gql(&["Query.portraitImages"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/portraits",
        Gql(&["Query.portraits"]),
    ),
    (
        "DELETE",
        "/api/v1/trees/{tree_id}/profiles",
        Gql(&["Mutation.dropTreeProfiles"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/profiles",
        Gql(&["Query.personProfiles"]),
    ),
    (
        "POST",
        "/api/v1/trees/{tree_id}/profiles/rebuild",
        Gql(&["Mutation.rebuildTreeProfiles"]),
    ),
    (
        "POST",
        "/api/v1/trees/{tree_id}/profiles/rebuild/{person_id}",
        Gql(&["Mutation.rebuildPersonProfile"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/profiles/{person_id}",
        Gql(&["Query.personProfile"]),
    ),
    (
        "POST",
        "/api/v1/trees/{tree_id}/relation-labels",
        Gql(&["Query.relationLabels"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/repositories",
        Gql(&["Query.repositories"]),
    ),
    (
        "POST",
        "/api/v1/trees/{tree_id}/repositories",
        Gql(&["Mutation.createRepository"]),
    ),
    (
        "DELETE",
        "/api/v1/trees/{tree_id}/repositories/{repository_id}",
        Gql(&["Mutation.deleteRepository"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/repositories/{repository_id}",
        Gql(&["Query.repository"]),
    ),
    (
        "PUT",
        "/api/v1/trees/{tree_id}/repositories/{repository_id}",
        Gql(&["Mutation.updateRepository"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/repositories/{repository_id}/sources",
        Gql(&["GqlRepository.sources"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/sources",
        Gql(&["Query.sources"]),
    ),
    (
        "POST",
        "/api/v1/trees/{tree_id}/sources",
        Gql(&["Mutation.createSource"]),
    ),
    (
        "DELETE",
        "/api/v1/trees/{tree_id}/sources/{source_id}",
        Gql(&["Mutation.deleteSource"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/sources/{source_id}",
        Gql(&["Query.source"]),
    ),
    (
        "PUT",
        "/api/v1/trees/{tree_id}/sources/{source_id}",
        Gql(&["Mutation.updateSource"]),
    ),
    (
        "POST",
        "/api/v1/trees/{tree_id}/sources/{source_id}/archive-target",
        Gql(&["GqlSource.archiveTarget"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/sources/{source_id}/repositories",
        Gql(&["GqlSource.repositories"]),
    ),
    (
        "POST",
        "/api/v1/trees/{tree_id}/sources/{source_id}/repositories",
        Gql(&["Mutation.addSourceRepository"]),
    ),
    (
        "DELETE",
        "/api/v1/trees/{tree_id}/sources/{source_id}/repositories/{link_id}",
        Gql(&["Mutation.removeSourceRepository"]),
    ),
    (
        "PUT",
        "/api/v1/trees/{tree_id}/sources/{source_id}/repositories/{link_id}",
        Gql(&["Mutation.updateSourceRepository"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/statistics",
        Gql(&["Query.treeStatistics"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/statistics/growth",
        Gql(&["Query.treeGrowth"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/suggestions/{field}",
        Gql(&["Query.valueSuggestions"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/unlocated-places",
        Gql(&["Query.unlocatedPlaces"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/vignettes",
        Gql(&["Query.vignettes"]),
    ),
    (
        "DELETE",
        "/api/v1/trees/{tree_id}/vignettes/{vignette_id}",
        Gql(&["Mutation.deleteVignette"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/vignettes/{vignette_id}",
        Gql(&["Query.vignette"]),
    ),
    (
        "PUT",
        "/api/v1/trees/{tree_id}/vignettes/{vignette_id}",
        Gql(&["Mutation.updateVignette"]),
    ),
    (
        "GET",
        "/api/v1/trees/{tree_id}/vignettes/{vignette_id}/image",
        RestOnly(BINARY),
    ),
];

/// The `(METHOD, path)` of every operation of the OpenAPI document the
/// router serves.
async fn router_operations() -> BTreeSet<(String, String)> {
    let app = app_on(setup_db().await);
    let spec = ok(&app, Method::GET, "/api/v1/openapi.json", None).await;
    let mut operations = BTreeSet::new();
    for (path, methods) in spec["paths"].as_object().expect("paths") {
        for method in methods.as_object().expect("operations").keys() {
            operations.insert((method.to_uppercase(), path.clone()));
        }
    }
    assert!(
        operations.len() > 100,
        "the OpenAPI document lists the router"
    );
    operations
}

fn declared_operations() -> BTreeSet<(String, String)> {
    PARITY
        .iter()
        .map(|(method, path, _)| (method.to_string(), path.to_string()))
        .collect()
}

#[tokio::test]
async fn every_route_is_in_the_parity_table() {
    let routes = router_operations().await;
    let declared = declared_operations();
    assert_eq!(declared.len(), PARITY.len(), "a route is declared twice");
    let undeclared: Vec<_> = routes.difference(&declared).collect();
    let gone: Vec<_> = declared.difference(&routes).collect();
    assert!(
        undeclared.is_empty(),
        "routes missing from PARITY (add their GraphQL twin): {undeclared:#?}"
    );
    assert!(
        gone.is_empty(),
        "PARITY entries the router lacks: {gone:#?}"
    );
    for (method, path, twin) in PARITY {
        let named = match twin {
            RestOnly(reason) => !reason.is_empty(),
            Gql(operations) => !operations.is_empty(),
        };
        assert!(
            named,
            "{method} {path}: name its GraphQL twin or its reason"
        );
    }
}

/// The `(METHOD, path)` rows of docs/api.md's REST tables, `/api/v1`
/// prefixed and without their query strings.
fn documented_operations() -> BTreeSet<(String, String)> {
    let doc = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/api.md"))
        .expect("docs/api.md");
    let mut rows = BTreeSet::new();
    for line in doc.lines() {
        let cells: Vec<&str> = line.split('|').map(str::trim).collect();
        let (Some(method), Some(path)) = (cells.get(1), cells.get(2)) else {
            continue;
        };
        let method = method.trim_matches('`');
        if !["GET", "POST", "PUT", "PATCH", "DELETE"].contains(&method) {
            continue;
        }
        let path = path.trim_matches('`');
        let path = path.split('?').next().unwrap_or(path);
        rows.insert((method.to_string(), format!("/api/v1{path}")));
    }
    rows
}

/// A path with its parameter names erased: docs may name them for reading.
fn shape(path: &str) -> String {
    path.split('/')
        .map(|segment| {
            if segment.starts_with('{') {
                "{}"
            } else {
                segment
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

#[tokio::test]
async fn api_md_documents_exactly_the_router() {
    let routes: BTreeSet<_> = router_operations()
        .await
        .into_iter()
        .map(|(m, p)| (m, shape(&p)))
        .collect();
    let documented: BTreeSet<_> = documented_operations()
        .into_iter()
        .map(|(m, p)| (m, shape(&p)))
        .collect();
    let undocumented: Vec<_> = routes.difference(&documented).collect();
    let stale: Vec<_> = documented.difference(&routes).collect();
    assert!(
        undocumented.is_empty(),
        "routes without a row in docs/api.md: {undocumented:#?}"
    );
    assert!(
        stale.is_empty(),
        "docs/api.md rows the router lacks: {stale:#?}"
    );
}

#[cfg(feature = "graphql")]
mod graphql_side {
    use std::collections::{BTreeMap, BTreeSet};

    use async_graphql::{EmptySubscription, Schema};
    use oxidgene_api::graphql::{mutation::MutationRoot, query::QueryRoot};

    use super::{Gql, PARITY, RestOnly};

    /// Root GraphQL fields with no REST route, with why. Empty: the
    /// contract has none.
    const GRAPHQL_ONLY: &[(&str, &str)] = &[];

    const SNAPSHOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/schema.graphql");

    fn sdl() -> String {
        Schema::build(QueryRoot, MutationRoot, EmptySubscription)
            .finish()
            .sdl()
    }

    /// The fields of every object type of `sdl`, the roots as `Query` and
    /// `Mutation`.
    fn fields(sdl: &str) -> BTreeMap<String, BTreeSet<String>> {
        let mut reader = SdlReader::default();
        for line in sdl.lines() {
            reader.read(line.trim());
        }
        reader.types
    }

    /// A line-by-line reader of the SDL async-graphql prints.
    #[derive(Default)]
    struct SdlReader {
        types: BTreeMap<String, BTreeSet<String>>,
        current: Option<String>,
        description: bool,
        depth: usize,
    }

    impl SdlReader {
        fn read(&mut self, line: &str) {
            if self.in_description(line) {
                return;
            }
            if let Some(name) = object_type(line) {
                self.current = Some(name);
                return;
            }
            if line == "}" {
                self.current = None;
                return;
            }
            // A long argument list spans several lines: only a line that
            // starts outside one names a field.
            let at_field = self.depth == 0;
            self.depth += line.matches('(').count();
            self.depth -= line.matches(')').count().min(self.depth);
            if let (Some(ty), true, Some(field)) = (&self.current, at_field, field_name(line)) {
                self.types.entry(ty.clone()).or_default().insert(field);
            }
        }

        /// Whether `line` belongs to a description; a `"""` line opens or
        /// closes one, unless it opens and closes it on the same line.
        fn in_description(&mut self, line: &str) -> bool {
            if line.starts_with("\"\"\"") {
                if !(line.len() > 3 && line.ends_with("\"\"\"")) {
                    self.description = !self.description;
                }
                return true;
            }
            self.description || line.starts_with('"')
        }
    }

    /// The object type a `type X {` line opens, the roots as `Query` and
    /// `Mutation`.
    fn object_type(line: &str) -> Option<String> {
        let name = line.strip_prefix("type ")?.split([' ', '{']).next()?;
        Some(match name {
            "QueryRoot" => "Query".to_string(),
            "MutationRoot" => "Mutation".to_string(),
            other => other.to_string(),
        })
    }

    fn field_name(line: &str) -> Option<String> {
        let field: String = line
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        (!field.is_empty()).then_some(field)
    }

    #[test]
    fn every_graphql_operation_mirrors_a_route() {
        let types = fields(&sdl());
        let mut problems = Vec::new();
        let mut referenced = BTreeSet::new();
        for (method, path, twin) in PARITY {
            let Gql(operations) = twin else {
                continue;
            };
            for operation in *operations {
                let (ty, field) = operation.split_once('.').expect("Type.field");
                if !types.get(ty).is_some_and(|f| f.contains(field)) {
                    problems.push(format!("{method} {path}: {operation} is not in the schema"));
                }
                referenced.insert(operation.to_string());
            }
        }
        for (ty, root) in [("Query", "Query"), ("Mutation", "Mutation")] {
            for field in types.get(ty).into_iter().flatten() {
                let name = format!("{root}.{field}");
                if !referenced.contains(&name) && !GRAPHQL_ONLY.iter().any(|(f, _)| *f == name) {
                    problems.push(format!("{name} mirrors no REST route"));
                }
            }
        }
        let exceptions = PARITY
            .iter()
            .filter(|(_, _, twin)| matches!(twin, RestOnly(_)))
            .count();
        assert!(
            exceptions <= 8,
            "REST-only exceptions keep growing ({exceptions})"
        );
        assert!(problems.is_empty(), "{}", problems.join("\n"));
    }

    /// The SDL equals docs/schema.graphql; `OXIDGENE_BLESS=1` (or
    /// `just graphql-schema`) rewrites the file instead.
    #[test]
    fn the_schema_matches_its_snapshot() {
        let current = sdl();
        if std::env::var_os("OXIDGENE_BLESS").is_some() {
            std::fs::write(SNAPSHOT, &current).expect("writes docs/schema.graphql");
            return;
        }
        let committed = std::fs::read_to_string(SNAPSHOT).unwrap_or_default();
        assert!(
            committed == current,
            "the GraphQL schema changed: review it, then run `just graphql-schema` and commit docs/schema.graphql"
        );
    }
}
