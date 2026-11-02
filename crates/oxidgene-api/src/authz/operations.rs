//! The requirement of every operation (docs/authorization.md §3.2, §10):
//! each REST route by method and template, each GraphQL root field by name.
//!
//! An operation missing here is refused (`403 forbidden`), never let
//! through, and the API contract guard fails until it is declared; the same
//! guard checks that a REST route and its GraphQL twin declare the same
//! requirement, so the two surfaces cannot disagree.

use oxidgene_core::access::Permission as P;

use super::Requirement::{self, Administrator, Open, SignedIn, Tree, TreeCreation, Trees};

/// The requirement of the REST operation `method` on route template
/// `route`, the `/api/v1` prefix included.
pub fn rest(method: &str, route: &str) -> Option<Requirement> {
    let path = route.strip_prefix("/api/v1")?;
    REST.iter()
        .chain(ACCOUNT_REST)
        .find(|(m, p, _)| *m == method && *p == path)
        .map(|(_, _, requirement)| *requirement)
}

/// The requirement of GraphQL root field `field` of the query or the
/// mutation root.
pub fn graphql(field: &str) -> Option<Requirement> {
    GRAPHQL
        .iter()
        .chain(ACCOUNT_GRAPHQL)
        .find(|(f, _)| *f == field)
        .map(|(_, requirement)| *requirement)
}

/// Every declared REST operation, the account ones included: what the
/// guard compares with the router.
pub fn rest_operations() -> impl Iterator<Item = (&'static str, &'static str, Requirement)> {
    REST.iter().chain(ACCOUNT_REST).copied()
}

/// Every declared GraphQL root field.
pub fn graphql_fields() -> impl Iterator<Item = (&'static str, Requirement)> {
    GRAPHQL.iter().chain(ACCOUNT_GRAPHQL).copied()
}

/// The tree operations, served by every build.
const REST: &[(&str, &str, Requirement)] = &[
    ("GET", "/openapi.json", Open),
    ("GET", "/gedcom/export-destinations", Open),
    ("GET", "/reference/basemap", Open),
    ("GET", "/reference/{lang}/given-names", Open),
    ("POST", "/reference/{lang}/given-names/bundle", Open),
    ("GET", "/reference/{lang}/occupations", Open),
    ("POST", "/reference/{lang}/occupations/bundle", Open),
    ("GET", "/reference/{lang}/places", Open),
    // The Geneanet wizard's first steps read what the user sends, before a
    // tree is chosen.
    ("POST", "/geneanet/archives", SignedIn),
    ("POST", "/geneanet/plan", SignedIn),
    ("POST", "/geneanet/preview", SignedIn),
    ("POST", "/geneanet/session/decode", SignedIn),
    ("POST", "/geneanet/session/encode", SignedIn),
    ("POST", "/geneanet/session/release", SignedIn),
    ("POST", "/geneweb/inspect", SignedIn),
    // The tree list is the viewer's memberships.
    ("GET", "/trees", SignedIn),
    ("POST", "/trees", TreeCreation),
    ("GET", "/trees/recent-persons", Trees(P::Read)),
    ("GET", "/trees/{tree_id}", Tree(P::Read)),
    ("PUT", "/trees/{tree_id}", Tree(P::SettingsManage)),
    ("DELETE", "/trees/{tree_id}", Tree(P::TreeDelete)),
    ("POST", "/trees/{tree_id}/duplicate", Tree(P::TreeDuplicate)),
    // Tools.
    (
        "GET",
        "/trees/{tree_id}/ancestry-completeness",
        Tree(P::Tools),
    ),
    ("GET", "/trees/{tree_id}/anomalies", Tree(P::Tools)),
    ("GET", "/trees/{tree_id}/duplicates", Tree(P::Tools)),
    ("GET", "/trees/{tree_id}/unlocated-places", Tree(P::Tools)),
    // History.
    ("GET", "/trees/{tree_id}/audit", Tree(P::HistoryRead)),
    (
        "GET",
        "/trees/{tree_id}/audit/{entry_id}",
        Tree(P::HistoryRead),
    ),
    (
        "GET",
        "/trees/{tree_id}/audit/{entry_id}/changes",
        Tree(P::HistoryRead),
    ),
    (
        "GET",
        "/trees/{tree_id}/history/{record_type}/{record_id}",
        Tree(P::HistoryRead),
    ),
    (
        "GET",
        "/trees/{tree_id}/history/{record_type}/{record_id}/{version}",
        Tree(P::HistoryRead),
    ),
    (
        "POST",
        "/trees/{tree_id}/history/{record_type}/{record_id}/revert",
        Tree(P::HistoryRestore),
    ),
    // Citations.
    ("GET", "/trees/{tree_id}/citations", Tree(P::Read)),
    ("POST", "/trees/{tree_id}/citations", Tree(P::Create)),
    (
        "PUT",
        "/trees/{tree_id}/citations/{citation_id}",
        Tree(P::Edit),
    ),
    (
        "DELETE",
        "/trees/{tree_id}/citations/{citation_id}",
        Tree(P::Delete),
    ),
    // The dictionary: reads, and renames that reach every record.
    (
        "GET",
        "/trees/{tree_id}/dictionary/family-names",
        Tree(P::Read),
    ),
    (
        "GET",
        "/trees/{tree_id}/dictionary/family-names/descent",
        Tree(P::Read),
    ),
    (
        "GET",
        "/trees/{tree_id}/dictionary/family-names/usage",
        Tree(P::Read),
    ),
    (
        "PATCH",
        "/trees/{tree_id}/dictionary/family-names/particle",
        Tree(P::Merge),
    ),
    (
        "PATCH",
        "/trees/{tree_id}/dictionary/family-names/rename",
        Tree(P::Merge),
    ),
    (
        "GET",
        "/trees/{tree_id}/dictionary/occupations",
        Tree(P::Read),
    ),
    (
        "GET",
        "/trees/{tree_id}/dictionary/occupations/usage",
        Tree(P::Read),
    ),
    (
        "PATCH",
        "/trees/{tree_id}/dictionary/occupations/rename",
        Tree(P::Merge),
    ),
    ("GET", "/trees/{tree_id}/dictionary/places", Tree(P::Read)),
    (
        "GET",
        "/trees/{tree_id}/dictionary/places/{place_id}/usage",
        Tree(P::Read),
    ),
    ("GET", "/trees/{tree_id}/dictionary/sources", Tree(P::Read)),
    (
        "GET",
        "/trees/{tree_id}/dictionary/sources/groups",
        Tree(P::Read),
    ),
    (
        "GET",
        "/trees/{tree_id}/dictionary/sources/{source_id}/usage",
        Tree(P::Read),
    ),
    // Events and their witnesses.
    ("GET", "/trees/{tree_id}/events", Tree(P::Read)),
    ("POST", "/trees/{tree_id}/events", Tree(P::Create)),
    ("GET", "/trees/{tree_id}/events/{event_id}", Tree(P::Read)),
    ("PUT", "/trees/{tree_id}/events/{event_id}", Tree(P::Edit)),
    (
        "DELETE",
        "/trees/{tree_id}/events/{event_id}",
        Tree(P::Delete),
    ),
    (
        "GET",
        "/trees/{tree_id}/events/{event_id}/witnesses",
        Tree(P::Read),
    ),
    (
        "POST",
        "/trees/{tree_id}/events/{event_id}/witnesses",
        Tree(P::Create),
    ),
    (
        "DELETE",
        "/trees/{tree_id}/events/{event_id}/witnesses/{witness_id}",
        Tree(P::Delete),
    ),
    // Imports and exports.
    ("POST", "/trees/{tree_id}/export-jobs", Tree(P::Export)),
    (
        "GET",
        "/trees/{tree_id}/export-jobs/downloadable",
        Tree(P::Export),
    ),
    (
        "GET",
        "/trees/{tree_id}/export-jobs/{job_id}",
        Tree(P::Export),
    ),
    (
        "POST",
        "/trees/{tree_id}/export-jobs/{job_id}/cancel",
        Tree(P::Export),
    ),
    (
        "GET",
        "/trees/{tree_id}/export-jobs/{job_id}/download",
        Tree(P::Export),
    ),
    ("GET", "/trees/{tree_id}/gedcom/export", Tree(P::Export)),
    ("POST", "/trees/{tree_id}/import-jobs", Tree(P::Import)),
    (
        "GET",
        "/trees/{tree_id}/import-jobs/{job_id}",
        Tree(P::Import),
    ),
    (
        "POST",
        "/trees/{tree_id}/import-jobs/{job_id}/cancel",
        Tree(P::Import),
    ),
    ("POST", "/trees/{tree_id}/geneanet/import", Tree(P::Import)),
    // Families and their members.
    ("GET", "/trees/{tree_id}/families", Tree(P::Read)),
    ("POST", "/trees/{tree_id}/families", Tree(P::Create)),
    (
        "GET",
        "/trees/{tree_id}/families/{family_id}",
        Tree(P::Read),
    ),
    (
        "PUT",
        "/trees/{tree_id}/families/{family_id}",
        Tree(P::Edit),
    ),
    (
        "DELETE",
        "/trees/{tree_id}/families/{family_id}",
        Tree(P::Delete),
    ),
    (
        "GET",
        "/trees/{tree_id}/families/{family_id}/detail-bundle",
        Tree(P::Read),
    ),
    (
        "GET",
        "/trees/{tree_id}/families/{family_id}/children",
        Tree(P::Read),
    ),
    (
        "POST",
        "/trees/{tree_id}/families/{family_id}/children",
        Tree(P::Create),
    ),
    (
        "DELETE",
        "/trees/{tree_id}/families/{family_id}/children/{child_id}",
        Tree(P::Delete),
    ),
    (
        "GET",
        "/trees/{tree_id}/families/{family_id}/spouses",
        Tree(P::Read),
    ),
    (
        "POST",
        "/trees/{tree_id}/families/{family_id}/spouses",
        Tree(P::Create),
    ),
    (
        "DELETE",
        "/trees/{tree_id}/families/{family_id}/spouses/{spouse_id}",
        Tree(P::Delete),
    ),
    // Media, documents, their pages and links.
    ("POST", "/trees/{tree_id}/gallery-bundle", Tree(P::Read)),
    ("GET", "/trees/{tree_id}/media", Tree(P::Read)),
    ("POST", "/trees/{tree_id}/media", Tree(P::Create)),
    ("POST", "/trees/{tree_id}/media/upload", Tree(P::Create)),
    ("POST", "/trees/{tree_id}/media/document", Tree(P::Create)),
    ("GET", "/trees/{tree_id}/media/facets", Tree(P::Read)),
    ("GET", "/trees/{tree_id}/media/{media_id}", Tree(P::Read)),
    ("PUT", "/trees/{tree_id}/media/{media_id}", Tree(P::Edit)),
    (
        "DELETE",
        "/trees/{tree_id}/media/{media_id}",
        Tree(P::Delete),
    ),
    (
        "GET",
        "/trees/{tree_id}/media/{media_id}/deletion-status",
        Tree(P::Read),
    ),
    (
        "GET",
        "/trees/{tree_id}/media/{media_id}/pages",
        Tree(P::Read),
    ),
    (
        "PUT",
        "/trees/{tree_id}/media/{media_id}/pages",
        Tree(P::Edit),
    ),
    (
        "DELETE",
        "/trees/{tree_id}/media/{media_id}/pages/{page_id}",
        Tree(P::Delete),
    ),
    (
        "GET",
        "/trees/{tree_id}/media/{media_id}/viewer-bundle",
        Tree(P::Read),
    ),
    (
        "GET",
        "/trees/{tree_id}/media/{media_id}/file",
        Tree(P::Read),
    ),
    (
        "GET",
        "/trees/{tree_id}/media/{media_id}/download",
        Tree(P::Read),
    ),
    (
        "GET",
        "/trees/{tree_id}/media/{media_id}/archive",
        Tree(P::Read),
    ),
    (
        "GET",
        "/trees/{tree_id}/media/{media_id}/thumbnail",
        Tree(P::Read),
    ),
    (
        "GET",
        "/trees/{tree_id}/media/{media_id}/card",
        Tree(P::Read),
    ),
    (
        "GET",
        "/trees/{tree_id}/media/{media_id}/vignettes",
        Tree(P::Read),
    ),
    (
        "POST",
        "/trees/{tree_id}/media/{media_id}/vignettes",
        Tree(P::Create),
    ),
    ("GET", "/trees/{tree_id}/media-links", Tree(P::Read)),
    ("POST", "/trees/{tree_id}/media-links", Tree(P::Create)),
    (
        "DELETE",
        "/trees/{tree_id}/media-links/{link_id}",
        Tree(P::Delete),
    ),
    ("GET", "/trees/{tree_id}/vignettes", Tree(P::Read)),
    (
        "GET",
        "/trees/{tree_id}/vignettes/{vignette_id}",
        Tree(P::Read),
    ),
    (
        "PUT",
        "/trees/{tree_id}/vignettes/{vignette_id}",
        Tree(P::Edit),
    ),
    (
        "DELETE",
        "/trees/{tree_id}/vignettes/{vignette_id}",
        Tree(P::Delete),
    ),
    (
        "GET",
        "/trees/{tree_id}/vignettes/{vignette_id}/image",
        Tree(P::Read),
    ),
    (
        "GET",
        "/trees/{tree_id}/vignettes/{vignette_id}/card",
        Tree(P::Read),
    ),
    // Tags: the vocabulary, the tags of records, bulk tagging.
    ("GET", "/trees/{tree_id}/tags", Tree(P::ReadTags)),
    ("POST", "/trees/{tree_id}/tags", Tree(P::Create)),
    ("PUT", "/trees/{tree_id}/tags/{tag_id}", Tree(P::Edit)),
    ("DELETE", "/trees/{tree_id}/tags/{tag_id}", Tree(P::Delete)),
    (
        "POST",
        "/trees/{tree_id}/tags/{tag_id}/merge",
        Tree(P::Merge),
    ),
    (
        "POST",
        "/trees/{tree_id}/tags/{tag_id}/bulk",
        Tree(P::Merge),
    ),
    // The bulk target's preview, which only bulk tagging uses.
    ("POST", "/trees/{tree_id}/tags/target", Tree(P::Merge)),
    (
        "GET",
        "/trees/{tree_id}/tags/{tag_id}/records",
        Tree(P::ReadTags),
    ),
    (
        "PUT",
        "/trees/{tree_id}/tags/{tag_id}/records/{kind}/{record_id}",
        Tree(P::Edit),
    ),
    (
        "DELETE",
        "/trees/{tree_id}/tags/{tag_id}/records/{kind}/{record_id}",
        Tree(P::Edit),
    ),
    // Notes.
    ("GET", "/trees/{tree_id}/notes", Tree(P::Read)),
    ("POST", "/trees/{tree_id}/notes", Tree(P::Create)),
    ("GET", "/trees/{tree_id}/notes/{note_id}", Tree(P::Read)),
    ("PUT", "/trees/{tree_id}/notes/{note_id}", Tree(P::Edit)),
    (
        "DELETE",
        "/trees/{tree_id}/notes/{note_id}",
        Tree(P::Delete),
    ),
    // Charts and profiles.
    ("GET", "/trees/{tree_id}/pedigree", Tree(P::Read)),
    (
        "GET",
        "/trees/{tree_id}/pedigree/{root_person_id}",
        Tree(P::Read),
    ),
    (
        "GET",
        "/trees/{tree_id}/pedigree/{root_person_id}/expand",
        Tree(P::Read),
    ),
    ("POST", "/trees/{tree_id}/pedigrees", Tree(P::Read)),
    ("GET", "/trees/{tree_id}/profiles", Tree(P::Read)),
    (
        "GET",
        "/trees/{tree_id}/profiles/{person_id}",
        Tree(P::Read),
    ),
    // Projection maintenance: derived data, rebuilt for the whole tree.
    (
        "DELETE",
        "/trees/{tree_id}/profiles",
        Tree(P::SettingsManage),
    ),
    (
        "POST",
        "/trees/{tree_id}/profiles/rebuild",
        Tree(P::SettingsManage),
    ),
    (
        "POST",
        "/trees/{tree_id}/profiles/rebuild/{person_id}",
        Tree(P::SettingsManage),
    ),
    ("POST", "/trees/{tree_id}/relation-labels", Tree(P::Read)),
    ("POST", "/trees/{tree_id}/portrait-images", Tree(P::Read)),
    ("GET", "/trees/{tree_id}/portraits", Tree(P::Read)),
    // Persons and their names.
    ("GET", "/trees/{tree_id}/persons", Tree(P::Read)),
    ("POST", "/trees/{tree_id}/persons", Tree(P::Create)),
    (
        "GET",
        "/trees/{tree_id}/persons/recently-modified",
        Tree(P::Read),
    ),
    ("GET", "/trees/{tree_id}/persons/search", Tree(P::Read)),
    (
        "GET",
        "/trees/{tree_id}/persons/sosa/{number}",
        Tree(P::Read),
    ),
    ("GET", "/trees/{tree_id}/persons/{person_id}", Tree(P::Read)),
    ("PUT", "/trees/{tree_id}/persons/{person_id}", Tree(P::Edit)),
    (
        "DELETE",
        "/trees/{tree_id}/persons/{person_id}",
        Tree(P::Delete),
    ),
    (
        "GET",
        "/trees/{tree_id}/persons/{person_id}/ancestors",
        Tree(P::Read),
    ),
    (
        "GET",
        "/trees/{tree_id}/persons/{person_id}/descendants",
        Tree(P::Read),
    ),
    (
        "GET",
        "/trees/{tree_id}/persons/{person_id}/detail-bundle",
        Tree(P::Read),
    ),
    (
        "GET",
        "/trees/{tree_id}/persons/{person_id}/homonyms",
        Tree(P::Read),
    ),
    (
        "GET",
        "/trees/{tree_id}/persons/{person_id}/kinship/{other_person_id}",
        Tree(P::Read),
    ),
    (
        "POST",
        "/trees/{tree_id}/persons/{person_id}/distinct",
        Tree(P::Edit),
    ),
    (
        "POST",
        "/trees/{tree_id}/persons/{person_id}/merge",
        Tree(P::Merge),
    ),
    (
        "GET",
        "/trees/{tree_id}/persons/{person_id}/names",
        Tree(P::Read),
    ),
    (
        "POST",
        "/trees/{tree_id}/persons/{person_id}/names",
        Tree(P::Create),
    ),
    (
        "PUT",
        "/trees/{tree_id}/persons/{person_id}/names/{name_id}",
        Tree(P::Edit),
    ),
    (
        "DELETE",
        "/trees/{tree_id}/persons/{person_id}/names/{name_id}",
        Tree(P::Delete),
    ),
    (
        "PUT",
        "/trees/{tree_id}/persons/{person_id}/portrait",
        Tree(P::Edit),
    ),
    // Places.
    ("GET", "/trees/{tree_id}/places", Tree(P::Read)),
    ("POST", "/trees/{tree_id}/places", Tree(P::Create)),
    ("GET", "/trees/{tree_id}/places/{place_id}", Tree(P::Read)),
    ("PUT", "/trees/{tree_id}/places/{place_id}", Tree(P::Edit)),
    (
        "DELETE",
        "/trees/{tree_id}/places/{place_id}",
        Tree(P::Delete),
    ),
    (
        "POST",
        "/trees/{tree_id}/places/{place_id}/merge",
        Tree(P::Merge),
    ),
    // Repositories.
    ("GET", "/trees/{tree_id}/repositories", Tree(P::Read)),
    ("POST", "/trees/{tree_id}/repositories", Tree(P::Create)),
    (
        "GET",
        "/trees/{tree_id}/repositories/{repository_id}",
        Tree(P::Read),
    ),
    (
        "PUT",
        "/trees/{tree_id}/repositories/{repository_id}",
        Tree(P::Edit),
    ),
    (
        "DELETE",
        "/trees/{tree_id}/repositories/{repository_id}",
        Tree(P::Delete),
    ),
    (
        "POST",
        "/trees/{tree_id}/repositories/{repository_id}/merge",
        Tree(P::Merge),
    ),
    (
        "GET",
        "/trees/{tree_id}/repositories/{repository_id}/sources",
        Tree(P::Read),
    ),
    // Sources and their repositories.
    ("GET", "/trees/{tree_id}/sources", Tree(P::Read)),
    ("POST", "/trees/{tree_id}/sources", Tree(P::Create)),
    ("GET", "/trees/{tree_id}/sources/{source_id}", Tree(P::Read)),
    ("PUT", "/trees/{tree_id}/sources/{source_id}", Tree(P::Edit)),
    (
        "DELETE",
        "/trees/{tree_id}/sources/{source_id}",
        Tree(P::Delete),
    ),
    (
        "POST",
        "/trees/{tree_id}/sources/{source_id}/archive-target",
        Tree(P::Read),
    ),
    (
        "GET",
        "/trees/{tree_id}/sources/{source_id}/repositories",
        Tree(P::Read),
    ),
    (
        "POST",
        "/trees/{tree_id}/sources/{source_id}/repositories",
        Tree(P::Create),
    ),
    (
        "PUT",
        "/trees/{tree_id}/sources/{source_id}/repositories/{link_id}",
        Tree(P::Edit),
    ),
    (
        "DELETE",
        "/trees/{tree_id}/sources/{source_id}/repositories/{link_id}",
        Tree(P::Delete),
    ),
    // Statistics and value suggestions.
    ("GET", "/trees/{tree_id}/statistics", Tree(P::Read)),
    ("GET", "/trees/{tree_id}/statistics/growth", Tree(P::Read)),
    ("GET", "/trees/{tree_id}/suggestions/{field}", Tree(P::Read)),
];

/// The operations of the web build's accounts, memberships, sharing and exclusions,
/// which only a build with the `auth` feature serves. A member reads their own row of
/// the members, updates their own "Who am I?" and leaves; the owner manages
/// the others: the service narrows `read` to that (§10).
const ACCOUNT_REST: &[(&str, &str, Requirement)] = &[
    ("GET", "/me", SignedIn),
    ("PUT", "/me", SignedIn),
    ("DELETE", "/me", SignedIn),
    ("GET", "/me/export", SignedIn),
    ("PUT", "/me/avatar", SignedIn),
    ("DELETE", "/me/avatar", SignedIn),
    ("GET", "/me/sessions", SignedIn),
    ("DELETE", "/me/sessions/{session_id}", SignedIn),
    // The invitations addressed to the user: the service matches their
    // verified address.
    ("GET", "/me/invitations", SignedIn),
    ("POST", "/me/invitations/{invitation_id}/accept", SignedIn),
    ("POST", "/me/invitations/{invitation_id}/decline", SignedIn),
    // Notifications (Messaging §2, §6): the user's own.
    ("GET", "/me/notifications", SignedIn),
    ("POST", "/me/notifications/read", SignedIn),
    ("GET", "/me/notification-preferences", SignedIn),
    ("PUT", "/me/notification-preferences", SignedIn),
    ("GET", "/me/updates", SignedIn),
    // A mail's one-click unsubscribe: its signed token is the credential.
    ("POST", "/mail/unsubscribe", Open),
    ("GET", "/users/{user_id}", SignedIn),
    ("GET", "/users/{user_id}/avatar", SignedIn),
    ("GET", "/admin/users", Administrator),
    ("GET", "/admin/users/{user_id}", Administrator),
    ("PUT", "/admin/users/{user_id}", Administrator),
    ("DELETE", "/admin/users/{user_id}", Administrator),
    ("PUT", "/admin/users/{user_id}/identity", Administrator),
    ("GET", "/admin/trees", Administrator),
    ("PUT", "/admin/trees/{tree_id}/owner", Administrator),
    ("GET", "/admin/access-log", Administrator),
    ("GET", "/trees/{tree_id}/members", Tree(P::Read)),
    // Sharing: the owner's alone (§6).
    ("POST", "/trees/{tree_id}/members", Tree(P::MembersManage)),
    (
        "GET",
        "/trees/{tree_id}/share-candidates",
        Tree(P::MembersManage),
    ),
    (
        "GET",
        "/trees/{tree_id}/invitations",
        Tree(P::MembersManage),
    ),
    (
        "DELETE",
        "/trees/{tree_id}/invitations/{invitation_id}",
        Tree(P::MembersManage),
    ),
    (
        "POST",
        "/trees/{tree_id}/invitations/{invitation_id}/resend",
        Tree(P::MembersManage),
    ),
    (
        "GET",
        "/trees/{tree_id}/share-links",
        Tree(P::MembersManage),
    ),
    (
        "POST",
        "/trees/{tree_id}/share-links",
        Tree(P::MembersManage),
    ),
    (
        "PUT",
        "/trees/{tree_id}/share-links/{link_id}",
        Tree(P::MembersManage),
    ),
    (
        "DELETE",
        "/trees/{tree_id}/share-links/{link_id}",
        Tree(P::MembersManage),
    ),
    (
        "POST",
        "/trees/{tree_id}/access-scopes/preview",
        Tree(P::MembersManage),
    ),
    // A tree setting, read by the owner alone: its root may be a person
    // Visitors do not see.
    (
        "GET",
        "/trees/{tree_id}/visitor-scope",
        Tree(P::SettingsManage),
    ),
    ("PUT", "/trees/{tree_id}/members/{user_id}", Tree(P::Read)),
    (
        "DELETE",
        "/trees/{tree_id}/members/{user_id}",
        Tree(P::Read),
    ),
    (
        "POST",
        "/trees/{tree_id}/ownership",
        Tree(P::OwnershipTransfer),
    ),
    ("GET", "/trees/{tree_id}/access-log", Tree(P::AccessLogRead)),
    // The exclusions from sharing: the owner's alone (Privacy §6).
    (
        "GET",
        "/trees/{tree_id}/exclusions",
        Tree(P::ExclusionsManage),
    ),
    (
        "POST",
        "/trees/{tree_id}/exclusions",
        Tree(P::ExclusionsManage),
    ),
    (
        "DELETE",
        "/trees/{tree_id}/exclusions/{exclusion_id}",
        Tree(P::ExclusionsManage),
    ),
];

/// The GraphQL root fields of the tree operations.
const GRAPHQL: &[(&str, Requirement)] = &[
    ("exportDestinations", Open),
    ("basemap", Open),
    ("givenNameReference", Open),
    ("givenNameReferences", Open),
    ("occupationReference", Open),
    ("occupationReferences", Open),
    ("placeSuggestions", Open),
    ("indexGeneanetArchives", SignedIn),
    ("geneanetPlan", SignedIn),
    ("geneanetPreview", SignedIn),
    ("decodeGeneanetSession", SignedIn),
    ("encodeGeneanetSession", SignedIn),
    ("releaseGeneanetSessionMedia", SignedIn),
    ("inspectGeneweb", SignedIn),
    ("trees", SignedIn),
    ("createTree", TreeCreation),
    ("recentPersonsOfTrees", Trees(P::Read)),
    ("tree", Tree(P::Read)),
    ("updateTree", Tree(P::SettingsManage)),
    ("deleteTree", Tree(P::TreeDelete)),
    ("duplicateTree", Tree(P::TreeDuplicate)),
    ("ancestryCompleteness", Tree(P::Tools)),
    ("treeAnomalies", Tree(P::Tools)),
    ("potentialDuplicates", Tree(P::Tools)),
    ("unlocatedPlaces", Tree(P::Tools)),
    ("auditEntries", Tree(P::HistoryRead)),
    ("auditEntry", Tree(P::HistoryRead)),
    ("auditEntryChanges", Tree(P::HistoryRead)),
    ("recordVersions", Tree(P::HistoryRead)),
    ("recordVersion", Tree(P::HistoryRead)),
    ("revertRecord", Tree(P::HistoryRestore)),
    ("citations", Tree(P::Read)),
    ("createCitation", Tree(P::Create)),
    ("updateCitation", Tree(P::Edit)),
    ("deleteCitation", Tree(P::Delete)),
    ("dictionaryFamilyNames", Tree(P::Read)),
    ("familyNameDescent", Tree(P::Read)),
    ("familyNameUsage", Tree(P::Read)),
    ("setFamilyNameParticle", Tree(P::Merge)),
    ("renameFamilyName", Tree(P::Merge)),
    ("dictionaryOccupations", Tree(P::Read)),
    ("occupationUsage", Tree(P::Read)),
    ("renameOccupation", Tree(P::Merge)),
    ("dictionaryPlaces", Tree(P::Read)),
    ("placeUsage", Tree(P::Read)),
    ("dictionarySources", Tree(P::Read)),
    ("dictionarySourceDrill", Tree(P::Read)),
    ("sourceUsage", Tree(P::Read)),
    ("events", Tree(P::Read)),
    ("event", Tree(P::Read)),
    ("createEvent", Tree(P::Create)),
    ("updateEvent", Tree(P::Edit)),
    ("deleteEvent", Tree(P::Delete)),
    ("addEventWitness", Tree(P::Create)),
    ("removeEventWitness", Tree(P::Delete)),
    ("startExportJob", Tree(P::Export)),
    ("downloadableExport", Tree(P::Export)),
    ("exportJobStatus", Tree(P::Export)),
    ("cancelExportJob", Tree(P::Export)),
    ("exportGedcom", Tree(P::Export)),
    ("importJobStatus", Tree(P::Import)),
    ("cancelImportJob", Tree(P::Import)),
    ("importGeneanet", Tree(P::Import)),
    ("families", Tree(P::Read)),
    ("family", Tree(P::Read)),
    ("createFamily", Tree(P::Create)),
    ("updateFamily", Tree(P::Edit)),
    ("deleteFamily", Tree(P::Delete)),
    ("coupleDetailBundle", Tree(P::Read)),
    ("addChild", Tree(P::Create)),
    ("removeChild", Tree(P::Delete)),
    ("addSpouse", Tree(P::Create)),
    ("removeSpouse", Tree(P::Delete)),
    ("galleryBundle", Tree(P::Read)),
    ("mediaList", Tree(P::Read)),
    ("uploadMedia", Tree(P::Create)),
    ("uploadMediaFile", Tree(P::Create)),
    ("createMediaDocument", Tree(P::Create)),
    ("mediaFacets", Tree(P::Read)),
    ("media", Tree(P::Read)),
    ("updateMedia", Tree(P::Edit)),
    ("deleteMedia", Tree(P::Delete)),
    ("canDeleteMedia", Tree(P::Read)),
    ("mediaPages", Tree(P::Read)),
    ("reorderMediaPages", Tree(P::Edit)),
    ("deleteMediaPage", Tree(P::Delete)),
    ("mediaViewerBundle", Tree(P::Read)),
    ("mediaDownload", Tree(P::Read)),
    ("mediaArchive", Tree(P::Read)),
    ("mediaVignettes", Tree(P::Read)),
    ("createVignette", Tree(P::Create)),
    ("treeMediaLinks", Tree(P::Read)),
    ("entityMedia", Tree(P::Read)),
    ("mediaLinks", Tree(P::Read)),
    ("createMediaLink", Tree(P::Create)),
    ("deleteMediaLink", Tree(P::Delete)),
    ("vignettes", Tree(P::Read)),
    ("vignette", Tree(P::Read)),
    ("updateVignette", Tree(P::Edit)),
    ("deleteVignette", Tree(P::Delete)),
    ("tags", Tree(P::ReadTags)),
    ("createTag", Tree(P::Create)),
    ("updateTag", Tree(P::Edit)),
    ("deleteTag", Tree(P::Delete)),
    ("mergeTag", Tree(P::Merge)),
    ("bulkTag", Tree(P::Merge)),
    ("tagTarget", Tree(P::Merge)),
    ("taggedRecords", Tree(P::ReadTags)),
    ("tagRecord", Tree(P::Edit)),
    ("untagRecord", Tree(P::Edit)),
    ("notes", Tree(P::Read)),
    ("note", Tree(P::Read)),
    ("createNote", Tree(P::Create)),
    ("updateNote", Tree(P::Edit)),
    ("deleteNote", Tree(P::Delete)),
    ("pedigree", Tree(P::Read)),
    ("expandPedigree", Tree(P::Read)),
    ("pedigrees", Tree(P::Read)),
    ("personProfiles", Tree(P::Read)),
    ("personProfile", Tree(P::Read)),
    ("dropTreeProfiles", Tree(P::SettingsManage)),
    ("rebuildTreeProfiles", Tree(P::SettingsManage)),
    ("rebuildPersonProfile", Tree(P::SettingsManage)),
    ("relationLabels", Tree(P::Read)),
    ("portraitImages", Tree(P::Read)),
    ("portraits", Tree(P::Read)),
    ("persons", Tree(P::Read)),
    ("person", Tree(P::Read)),
    ("createPerson", Tree(P::Create)),
    ("updatePerson", Tree(P::Edit)),
    ("deletePerson", Tree(P::Delete)),
    ("recentlyModifiedPersons", Tree(P::Read)),
    ("searchPersons", Tree(P::Read)),
    ("personBySosa", Tree(P::Read)),
    ("ancestors", Tree(P::Read)),
    ("descendants", Tree(P::Read)),
    ("personDetailBundle", Tree(P::Read)),
    ("personHomonyms", Tree(P::Read)),
    ("kinship", Tree(P::Read)),
    ("markPersonsDistinct", Tree(P::Edit)),
    ("mergePersons", Tree(P::Merge)),
    ("addPersonName", Tree(P::Create)),
    ("updatePersonName", Tree(P::Edit)),
    ("deletePersonName", Tree(P::Delete)),
    ("setPersonPortrait", Tree(P::Edit)),
    ("places", Tree(P::Read)),
    ("place", Tree(P::Read)),
    ("createPlace", Tree(P::Create)),
    ("updatePlace", Tree(P::Edit)),
    ("deletePlace", Tree(P::Delete)),
    ("mergePlace", Tree(P::Merge)),
    ("repositories", Tree(P::Read)),
    ("repository", Tree(P::Read)),
    ("createRepository", Tree(P::Create)),
    ("updateRepository", Tree(P::Edit)),
    ("deleteRepository", Tree(P::Delete)),
    ("mergeRepository", Tree(P::Merge)),
    ("sources", Tree(P::Read)),
    ("source", Tree(P::Read)),
    ("createSource", Tree(P::Create)),
    ("updateSource", Tree(P::Edit)),
    ("deleteSource", Tree(P::Delete)),
    ("addSourceRepository", Tree(P::Create)),
    ("updateSourceRepository", Tree(P::Edit)),
    ("removeSourceRepository", Tree(P::Delete)),
    ("treeStatistics", Tree(P::Read)),
    ("treeGrowth", Tree(P::Read)),
    ("valueSuggestions", Tree(P::Read)),
];

/// The GraphQL root fields of the accounts and memberships.
const ACCOUNT_GRAPHQL: &[(&str, Requirement)] = &[
    ("me", SignedIn),
    ("updateMe", SignedIn),
    ("deleteMyAccount", SignedIn),
    ("myAccountData", SignedIn),
    ("uploadMyAvatar", SignedIn),
    ("deleteMyAvatar", SignedIn),
    ("mySessions", SignedIn),
    ("endMySession", SignedIn),
    ("myInvitations", SignedIn),
    ("acceptInvitation", SignedIn),
    ("declineInvitation", SignedIn),
    ("notifications", SignedIn),
    ("markNotificationsRead", SignedIn),
    ("notificationPreferences", SignedIn),
    ("updateNotificationPreferences", SignedIn),
    ("updates", SignedIn),
    ("unsubscribe", Open),
    ("userProfile", SignedIn),
    ("users", Administrator),
    ("user", Administrator),
    ("updateUser", Administrator),
    ("eraseUser", Administrator),
    ("rebindUserIdentity", Administrator),
    ("adminTrees", Administrator),
    ("assignTreeOwner", Administrator),
    ("accessEvents", Administrator),
    ("treeMembers", Tree(P::Read)),
    ("shareTree", Tree(P::MembersManage)),
    ("shareCandidates", Tree(P::MembersManage)),
    ("treeInvitations", Tree(P::MembersManage)),
    ("resendTreeInvitation", Tree(P::MembersManage)),
    ("revokeTreeInvitation", Tree(P::MembersManage)),
    ("shareLinks", Tree(P::MembersManage)),
    ("createShareLink", Tree(P::MembersManage)),
    ("updateShareLink", Tree(P::MembersManage)),
    ("revokeShareLink", Tree(P::MembersManage)),
    ("accessScopePreview", Tree(P::MembersManage)),
    ("visitorScope", Tree(P::SettingsManage)),
    ("updateTreeMember", Tree(P::Read)),
    ("removeTreeMember", Tree(P::Read)),
    ("transferTreeOwnership", Tree(P::OwnershipTransfer)),
    ("treeAccessEvents", Tree(P::AccessLogRead)),
    ("exclusions", Tree(P::ExclusionsManage)),
    ("createExclusion", Tree(P::ExclusionsManage)),
    ("deleteExclusion", Tree(P::ExclusionsManage)),
];

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn each_operation_is_declared_once() {
        let mut rest = HashSet::new();
        for (method, path, _) in rest_operations() {
            assert!(rest.insert((method, path)), "{method} {path} twice");
        }
        let mut graphql = HashSet::new();
        for (field, _) in graphql_fields() {
            assert!(graphql.insert(field), "{field} twice");
        }
    }

    #[test]
    fn a_route_is_found_with_its_prefix_only() {
        assert_eq!(rest("GET", "/api/v1/trees/{tree_id}"), Some(Tree(P::Read)));
        assert_eq!(rest("GET", "/trees/{tree_id}"), None);
        assert_eq!(rest("PATCH", "/api/v1/trees/{tree_id}"), None);
    }
}
