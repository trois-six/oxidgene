//! The person reads of [`ProfileService`] as a viewer sees them
//! (docs/privacy.md §5, §7): projections, pages of projections, pedigrees,
//! search rows. Each takes the request's [`Redaction`]; the owner's is
//! unrestricted and goes straight to the plain read, without a query more.
//!
//! - A person the viewer may not reach by the identifier asked for — absent,
//!   or a placeholder they may not navigate through — is `not_found`.
//! - Lists and searches leave every hidden person out, placeholders
//!   included, in SQL, so pages and counts see only what the viewer sees;
//!   the rows they return are rebuilt from the redacted projections, so no
//!   relative column names a hidden person.
//! - Pedigrees keep their placeholders, as the display of hidden persons
//!   says, and lose their absent persons and the links through them.

use std::collections::HashMap;

use oxidgene_core::OxidGeneError;
use oxidgene_core::projection::{
    Pedigree, PedigreeDelta, PersonProfile, SearchEntry, SearchResult,
};
use oxidgene_core::redaction::Redaction;
use oxidgene_core::search::fold_words;
use oxidgene_core::types::Connection;
use oxidgene_db::repo::{
    PaginationParams, PersonDenormRepo, PersonSearchFilters, PersonSearchSort,
};
use uuid::Uuid;

use super::ProfileService;
use super::builder;

/// The most matches a restricted viewer's search filtered on a relative
/// reads before filtering them on what the viewer sees of that relative.
const RELATIVE_FILTER_CANDIDATES: usize = 10_000;

fn not_found(id: Uuid) -> OxidGeneError {
    OxidGeneError::NotFound {
        entity: "Person",
        id,
    }
}

/// Check that the viewer reaches person `id` by that identifier.
pub fn require_reachable(redaction: &Redaction, id: Uuid) -> Result<(), OxidGeneError> {
    if redaction.public_id(id) == Some(id) {
        Ok(())
    } else {
        Err(not_found(id))
    }
}

/// Check that the viewer sees person `id` in full: `not_found` otherwise.
///
/// # Errors
///
/// [`OxidGeneError::NotFound`] for a person not shown in full.
pub fn require_shown(redaction: &Redaction, id: Uuid) -> Result<(), OxidGeneError> {
    if redaction.shows(id) {
        Ok(())
    } else {
        Err(not_found(id))
    }
}

/// Whether `filters` names a relative.
fn has_relative_filter(filters: &PersonSearchFilters) -> bool {
    [
        &filters.spouse_surname,
        &filters.spouse_given_names,
        &filters.father_surname,
        &filters.father_given_names,
        &filters.mother_surname,
        &filters.mother_given_names,
    ]
    .iter()
    .any(|value| value.as_deref().is_some_and(|v| !v.trim().is_empty()))
}

/// Whether `value` holds `wanted` once both are folded; an absent filter
/// holds.
fn holds(value: Option<&str>, wanted: Option<&str>) -> bool {
    match wanted.map(str::trim).filter(|w| !w.is_empty()) {
        None => true,
        Some(wanted) => value.is_some_and(|value| fold_words(value).contains(&fold_words(wanted))),
    }
}

/// Whether a redacted profile meets the relative filters of `filters`, on
/// the relatives the viewer sees.
fn relatives_match(profile: &PersonProfile, filters: &PersonSearchFilters) -> bool {
    let spouse = filters.spouse_surname.is_some() || filters.spouse_given_names.is_some();
    let spouses_ok = !spouse
        || profile.families_as_spouse.iter().any(|link| {
            holds(
                link.spouse_surname.as_deref(),
                filters.spouse_surname.as_deref(),
            ) && holds(
                link.spouse_given_names.as_deref(),
                filters.spouse_given_names.as_deref(),
            )
        });
    let parents = profile.family_as_child.as_ref();
    let father_ok = holds(
        parents.and_then(|p| p.father_surname.as_deref()),
        filters.father_surname.as_deref(),
    ) && holds(
        parents.and_then(|p| p.father_given_names.as_deref()),
        filters.father_given_names.as_deref(),
    );
    let mother_ok = holds(
        parents.and_then(|p| p.mother_surname.as_deref()),
        filters.mother_surname.as_deref(),
    ) && holds(
        parents.and_then(|p| p.mother_given_names.as_deref()),
        filters.mother_given_names.as_deref(),
    );
    spouses_ok && father_ok && mother_ok
}

/// A page of projections as the viewer sees them. The page was read
/// through the SQL filter of the same rules, so every row should redact to
/// a profile; one that does not — a person the redaction leaves absent —
/// is dropped rather than sent unredacted.
fn redact_page(
    redaction: &Redaction,
    mut page: Connection<PersonProfile>,
) -> Connection<PersonProfile> {
    page.edges
        .retain_mut(|edge| match redaction.profile(&edge.node) {
            Some(profile) => {
                edge.node = profile;
                true
            }
            None => false,
        });
    page
}

/// A person record as the viewer sees it: `not_found` unless they reach it
/// by its identifier; a placeholder keeps its sex and privacy only.
pub fn person_record(
    redaction: &Redaction,
    person: oxidgene_core::types::Person,
) -> Result<oxidgene_core::types::Person, OxidGeneError> {
    if redaction.is_unrestricted() {
        return Ok(person);
    }
    require_reachable(redaction, person.id)?;
    let id = person.id;
    seen_person(redaction, person).ok_or_else(|| not_found(id))
}

/// A related person's record as the viewer sees it: none when absent; a
/// placeholder, keeping its sex and privacy only, under the identifier the
/// viewer may know when hidden.
pub fn seen_person(
    redaction: &Redaction,
    mut person: oxidgene_core::types::Person,
) -> Option<oxidgene_core::types::Person> {
    if redaction.is_unrestricted() {
        return Some(person);
    }
    let id = redaction.public_id(person.id)?;
    if redaction.shows(person.id) {
        if redaction.withholds_tags() {
            person.tags.clear();
        }
        return Some(person);
    }
    Some(oxidgene_core::types::Person {
        placeholder: redaction.placeholder(person.id),
        id,
        portrait_media_id: None,
        portrait_vignette_id: None,
        tags: Vec::new(),
        ..person
    })
}

/// A tree's settings as the viewer reads them: its SOSA root and "Who am
/// I?" person named only by the identifier the viewer may know of them.
pub fn redact_tree(redaction: &Redaction, tree: &mut oxidgene_core::types::Tree) {
    if redaction.is_unrestricted() {
        return;
    }
    tree.sosa_root_person_id = tree
        .sosa_root_person_id
        .and_then(|id| redaction.public_id(id));
    tree.self_person_id = tree.self_person_id.and_then(|id| redaction.public_id(id));
}

/// The SOSA number the viewer may see of person `id`: none for a hidden one.
pub fn sosa_shown(redaction: &Redaction, id: Uuid, number: Option<u64>) -> Option<u64> {
    number.filter(|_| redaction.shows(id))
}

impl ProfileService {
    /// Person `person_id`'s projection as the viewer sees it.
    pub async fn person_as(
        &self,
        redaction: &Redaction,
        tree_id: Uuid,
        person_id: Uuid,
    ) -> Result<PersonProfile, OxidGeneError> {
        if redaction.is_unrestricted() {
            return self.get_or_build_person(tree_id, person_id).await;
        }
        require_reachable(redaction, person_id)?;
        let profile = self.get_or_build_person(tree_id, person_id).await?;
        redaction
            .profile(&profile)
            .ok_or_else(|| not_found(person_id))
    }

    /// One page of the projections the viewer sees in full.
    pub async fn persons_page_as(
        &self,
        redaction: &Redaction,
        tree_id: Uuid,
        params: &PaginationParams,
    ) -> Result<Connection<PersonProfile>, OxidGeneError> {
        if redaction.is_unrestricted() {
            return self.persons_page(tree_id, params).await;
        }
        self.ensure_materialized(tree_id).await?;
        let unseen = oxidgene_db::repo::Unseen::new(tree_id, redaction.rules());
        let page = PersonDenormRepo::list_page(self.reader(), tree_id, params, unseen).await?;
        Ok(redact_page(redaction, page))
    }

    /// The pedigree around `root` as the viewer sees it.
    pub async fn pedigree_as(
        &self,
        redaction: &Redaction,
        tree_id: Uuid,
        root: Uuid,
        ancestor_depth: u32,
        descendant_depth: u32,
    ) -> Result<Pedigree, OxidGeneError> {
        if !redaction.is_unrestricted() {
            require_reachable(redaction, root)?;
        }
        let mut pedigree = self
            .get_or_build_pedigree(tree_id, root, ancestor_depth, descendant_depth)
            .await?;
        redaction.pedigree(&mut pedigree);
        Ok(pedigree)
    }

    /// What a pedigree gains when expanded, as the viewer sees it.
    pub fn redact_delta(redaction: &Redaction, delta: &mut PedigreeDelta) {
        if redaction.is_unrestricted() {
            return;
        }
        delta.new_nodes = std::mem::take(&mut delta.new_nodes)
            .into_iter()
            .filter_map(|node| redaction.node(node))
            .collect();
        delta.new_edges.retain_mut(|edge| {
            match (
                redaction.public_id(edge.parent_id),
                redaction.public_id(edge.child_id),
            ) {
                (Some(parent), Some(child)) => {
                    edge.parent_id = parent;
                    edge.child_id = child;
                    true
                }
                _ => false,
            }
        });
    }

    /// Search rows of the persons of `ids` the viewer sees in full, in the
    /// order asked, rebuilt from their redacted projections.
    pub async fn search_entries_as(
        &self,
        redaction: &Redaction,
        tree_id: Uuid,
        ids: &[Uuid],
    ) -> Result<Vec<SearchEntry>, OxidGeneError> {
        if redaction.is_unrestricted() {
            return self.search_entries(tree_id, ids).await;
        }
        let shown: Vec<Uuid> = ids
            .iter()
            .copied()
            .filter(|id| redaction.shows(*id))
            .collect();
        let profiles = self.redacted_profiles(redaction, tree_id, &shown).await?;
        let mut by_id: HashMap<Uuid, SearchEntry> = profiles
            .iter()
            .map(|profile| (profile.person_id, builder::build_search_entry(profile)))
            .collect();
        let mut entries: Vec<SearchEntry> =
            shown.iter().filter_map(|id| by_id.remove(id)).collect();
        crate::service::portrait::with_portraits(self.reader(), tree_id, &mut entries).await?;
        for entry in &mut entries {
            if entry
                .portrait
                .as_ref()
                .is_some_and(|p| redaction.portrait_withheld(p.media_id, p.vignette_id))
            {
                entry.portrait = None;
            }
            if redaction.withholds_tags() {
                entry.tags.clear();
            }
        }
        Ok(entries)
    }

    /// The rows of the placeholders among `ids` — the persons who appear
    /// to the viewer but are not shown in full — each under the identifier
    /// the viewer may know it by, with its sex and why it is one, no name:
    /// what a kinship path names besides the persons shown.
    pub async fn placeholder_entries_as(
        &self,
        redaction: &Redaction,
        tree_id: Uuid,
        ids: &[Uuid],
    ) -> Result<Vec<SearchEntry>, OxidGeneError> {
        let stubs: Vec<Uuid> = ids
            .iter()
            .copied()
            .filter(|id| !redaction.shows(*id) && redaction.appears(*id))
            .collect();
        let mut entries = Vec::with_capacity(stubs.len());
        for chunk in stubs.chunks(1_000) {
            for profile in PersonDenormRepo::get_many(self.reader(), tree_id, chunk).await? {
                if let Some(profile) = redaction.profile(&profile) {
                    entries.push(builder::build_search_entry(&profile));
                }
            }
        }
        Ok(entries)
    }

    /// The redacted projections of `ids`, every one shown in full.
    async fn redacted_profiles(
        &self,
        redaction: &Redaction,
        tree_id: Uuid,
        ids: &[Uuid],
    ) -> Result<Vec<PersonProfile>, OxidGeneError> {
        let mut profiles = Vec::with_capacity(ids.len());
        for chunk in ids.chunks(1_000) {
            for profile in PersonDenormRepo::get_many(self.reader(), tree_id, chunk).await? {
                if let Some(profile) = redaction.profile(&profile) {
                    profiles.push(profile);
                }
            }
        }
        Ok(profiles)
    }

    /// A person search as the viewer sees it: hidden persons left out of
    /// the matches and the count, a relative filter matching only the
    /// relatives the viewer sees, tags filters refused to a viewer who sees
    /// no tags.
    #[expect(
        clippy::too_many_arguments,
        reason = "the search's own arguments, and the viewer's redaction"
    )]
    pub async fn search_as(
        &self,
        redaction: &Redaction,
        tree_id: Uuid,
        query: &str,
        filters: &PersonSearchFilters,
        sort: PersonSearchSort,
        limit: usize,
        offset: usize,
    ) -> Result<SearchResult, OxidGeneError> {
        if redaction.is_unrestricted() {
            return self
                .search_filtered(tree_id, query, filters, sort, limit, offset)
                .await;
        }
        if redaction.withholds_tags() && !filters.tags.is_empty() {
            return Ok(SearchResult {
                entries: Vec::new(),
                total_count: 0,
            });
        }
        let mut filters = filters.clone();
        filters.unseen = redaction.rules();
        if !has_relative_filter(&filters) {
            let page = self
                .search_filtered(tree_id, query, &filters, sort, limit, offset)
                .await?;
            let ids: Vec<Uuid> = page.entries.iter().map(|e| e.person_id).collect();
            return Ok(SearchResult {
                entries: self.search_entries_as(redaction, tree_id, &ids).await?,
                total_count: page.total_count,
            });
        }
        // The stored relative columns name every relative: read the matches
        // without them, then keep those whose visible relatives match.
        let mut unfiltered = filters.clone();
        for field in [
            &mut unfiltered.spouse_surname,
            &mut unfiltered.spouse_given_names,
            &mut unfiltered.father_surname,
            &mut unfiltered.father_given_names,
            &mut unfiltered.mother_surname,
            &mut unfiltered.mother_given_names,
        ] {
            *field = None;
        }
        let candidates = self
            .search_ids_as(tree_id, query, &unfiltered, sort)
            .await?;
        let profiles = self
            .redacted_profiles(redaction, tree_id, &candidates)
            .await?;
        let matching: HashMap<Uuid, bool> = profiles
            .iter()
            .map(|profile| (profile.person_id, relatives_match(profile, &filters)))
            .collect();
        let kept: Vec<Uuid> = candidates
            .into_iter()
            .filter(|id| matching.get(id).copied().unwrap_or(false))
            .collect();
        let page: Vec<Uuid> = kept
            .iter()
            .skip(offset)
            .take(limit.min(super::service::SEARCH_MAX_LIMIT))
            .copied()
            .collect();
        Ok(SearchResult {
            entries: self.search_entries_as(redaction, tree_id, &page).await?,
            total_count: kept.len(),
        })
    }

    /// The ids of every match of a search, in its order, at most
    /// [`RELATIVE_FILTER_CANDIDATES`].
    async fn search_ids_as(
        &self,
        tree_id: Uuid,
        query: &str,
        filters: &PersonSearchFilters,
        sort: PersonSearchSort,
    ) -> Result<Vec<Uuid>, OxidGeneError> {
        let mut ids = Vec::new();
        let step = super::service::SEARCH_MAX_LIMIT;
        while ids.len() < RELATIVE_FILTER_CANDIDATES {
            let page = self
                .search_filtered(tree_id, query, filters, sort, step, ids.len())
                .await?;
            let got = page.entries.len();
            ids.extend(page.entries.into_iter().map(|e| e.person_id));
            if got < step {
                break;
            }
        }
        Ok(ids)
    }

    /// The homonyms of person `person_id` the viewer sees in full.
    pub async fn homonyms_as(
        &self,
        redaction: &Redaction,
        tree_id: Uuid,
        person_id: Uuid,
    ) -> Result<Vec<SearchEntry>, OxidGeneError> {
        if redaction.is_unrestricted() {
            return self.homonyms(tree_id, person_id).await;
        }
        require_reachable(redaction, person_id)?;
        if !redaction.shows(person_id) {
            return Ok(Vec::new());
        }
        let rows = self.homonyms(tree_id, person_id).await?;
        let ids: Vec<Uuid> = rows.iter().map(|e| e.person_id).collect();
        self.search_entries_as(redaction, tree_id, &ids).await
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use chrono::Utc;
    use oxidgene_core::enums::{NameType, Sex, TreeDefaultPrivacy};
    use oxidgene_core::projection::ProfileName;
    use oxidgene_core::redaction::{RedactionInputs, Viewing};
    use oxidgene_core::types::{Edge, PageInfo, PrivacySettings};

    use super::*;

    fn profile(given: &str) -> PersonProfile {
        PersonProfile {
            placeholder: None,
            tags: Vec::new(),
            person_id: Uuid::now_v7(),
            tree_id: Uuid::nil(),
            sex: Sex::Unknown,
            primary_name: Some(ProfileName {
                name_id: Uuid::now_v7(),
                name_type: NameType::Birth,
                display_name: format!("{given} BRANCH_A"),
                given_names: Some(given.to_string()),
                surname: Some("BRANCH_A".to_string()),
            }),
            other_names: Vec::new(),
            birth: None,
            death: None,
            baptism: None,
            burial: None,
            other_events: Vec::new(),
            families_as_spouse: Vec::new(),
            family_as_child: None,
            primary_media: None,
            media_count: 0,
            citation_count: 0,
            note_count: 0,
            updated_at: Utc::now(),
            built_at: Utc::now(),
        }
    }

    #[test]
    fn a_row_the_redaction_leaves_absent_is_dropped_from_the_page() {
        let (kept, excluded) = (profile("child_1"), profile("child_2"));
        let excluded_set = HashSet::from([excluded.person_id]);
        let editor = Viewing {
            read_private: true,
            read_confidential: true,
            read_tags: true,
            exclusions_apply: true,
        };
        let redaction = Redaction::build(
            editor,
            TreeDefaultPrivacy::Public,
            &PrivacySettings::default(),
            &RedactionInputs {
                excluded: Some(&excluded_set),
                ..RedactionInputs::default()
            },
            2026,
        );
        let page = Connection {
            edges: [&kept, &excluded]
                .into_iter()
                .map(|node| Edge {
                    cursor: node.person_id.to_string(),
                    node: node.clone(),
                })
                .collect(),
            page_info: PageInfo {
                has_next_page: false,
                end_cursor: None,
            },
            total_count: 2,
        };
        let page = redact_page(&redaction, page);
        let ids: Vec<Uuid> = page.edges.iter().map(|edge| edge.node.person_id).collect();
        assert_eq!(ids, [kept.person_id]);
    }
}
