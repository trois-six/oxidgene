//! Person detail page — shows names, events, notes, citations, and ancestry charts with full CRUD.

use std::collections::{HashMap, HashSet};

use dioxus::prelude::*;
use oxidgene_cache::types::CachedPedigree;
use oxidgene_core::EventType;
use oxidgene_core::types::Event as DomainEvent;
use uuid::Uuid;

use crate::api::ApiClient;
use crate::components::confirm_dialog::ConfirmDialog;
use crate::components::person_form::{PersonForm, PersonFormCreateContext};
use crate::components::topbar_search::TopbarSearch;
use crate::components::tree_cache::{fetch_tree_cached, use_tree_cache};
use crate::components::tree_icon_sidebar::{TreeIconSidebar, TreeSidebarView};
use crate::i18n::use_i18n;
use crate::router::Route;
use crate::utils::resolve_name;
use oxidgene_core::Sex;

const SHOW_MANUAL_REFRESH: bool = cfg!(target_arch = "wasm32");

/// Indicates the origin of an event relative to the displayed person.
#[derive(Clone, Debug, PartialEq)]
enum EventOrigin {
    /// Event directly attached to this person (birth, death, occupation…).
    Individual,
    /// Event from a conjugal family (marriage, divorce…).
    ConjugalFamily,
    /// Event from a child (birth, death, baptism, burial of a child).
    ChildFamily,
    /// Event from the parental family (parent death, sibling birth…).
    ParentalFamily,
}

/// An event enriched with origin metadata for display purposes.
#[derive(Clone, Debug)]
struct EnrichedEvent {
    event: DomainEvent,
    origin: EventOrigin,
    /// Optional context label (e.g. spouse name, sibling name).
    context: Option<String>,
}

/// Page rendered at `/trees/:tree_id/persons/:person_id`.
#[component]
pub fn PersonDetail(tree_id: String, person_id: String) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let nav = use_navigator();
    let tree_cache = use_tree_cache();
    let mut refresh = use_signal(|| 0u32);

    // Reactive IDs: signals kept in sync with the props so resources re-run
    // when navigating to a different person (the router reuses this component
    // instance instead of remounting it, e.g. clicking through to a parent).
    let mut tree_id_parsed = use_signal(|| tree_id.parse::<Uuid>().ok());
    let new_tid = tree_id.parse::<Uuid>().ok();
    if new_tid != *tree_id_parsed.peek() {
        *tree_id_parsed.write() = new_tid;
    }

    let mut person_id_parsed = use_signal(|| person_id.parse::<Uuid>().ok());
    let new_pid = person_id.parse::<Uuid>().ok();
    if new_pid != *person_id_parsed.peek() {
        *person_id_parsed.write() = new_pid;
    }

    // Delete confirmation state.
    let mut confirm_delete = use_signal(|| false);
    let mut delete_error = use_signal(|| None::<String>);

    // Person edit modal (names are managed there — see PersonForm).
    let mut show_edit_person = use_signal(|| false);
    let mut show_create_person = use_signal(|| false);

    // ── Resources ────────────────────────────────────────────────────

    // Fetch person.
    let api_person = api.clone();
    let person_resource = use_resource(move || {
        let api = api_person.clone();
        let _tick = refresh();
        let tid = tree_id_parsed();
        let pid = person_id_parsed();
        async move {
            let (Some(tid), Some(pid)) = (tid, pid) else {
                return Err(crate::api::ApiError::Api {
                    status: 400,
                    body: i18n.t("common.invalid_ids"),
                });
            };
            api.get_person(tid, pid).await
        }
    });

    // Fetch person names.
    let api_names = api.clone();
    let names_resource = use_resource(move || {
        let api = api_names.clone();
        let _tick = refresh();
        let tid = tree_id_parsed();
        let pid = person_id_parsed();
        async move {
            let (Some(tid), Some(pid)) = (tid, pid) else {
                return Err(crate::api::ApiError::Api {
                    status: 400,
                    body: i18n.t("common.invalid_ids"),
                });
            };
            api.list_person_names(tid, pid).await
        }
    });

    // Fetch person events.
    let api_events = api.clone();
    let events_resource = use_resource(move || {
        let api = api_events.clone();
        let _tick = refresh();
        let tid = tree_id_parsed();
        let pid = person_id_parsed();
        async move {
            let (Some(tid), Some(pid)) = (tid, pid) else {
                return Err(crate::api::ApiError::Api {
                    status: 400,
                    body: i18n.t("common.invalid_ids"),
                });
            };
            api.list_events(tid, Some(100), None, None, Some(pid), None)
                .await
        }
    });

    // Fetch places in tree (for place picker in events).
    let api_places = api.clone();
    let places_resource = use_resource(move || {
        let api = api_places.clone();
        let _tick = refresh();
        let tid = tree_id_parsed();
        async move {
            let Some(tid) = tid else {
                return Err(crate::api::ApiError::Api {
                    status: 400,
                    body: i18n.t("common.invalid_ids"),
                });
            };
            api.list_all_places(tid).await
        }
    });

    // Fetch notes for this person.
    let api_notes = api.clone();
    let notes_resource = use_resource(move || {
        let api = api_notes.clone();
        let _tick = refresh();
        let tid = tree_id_parsed();
        let pid = person_id_parsed();
        async move {
            let (Some(tid), Some(pid)) = (tid, pid) else {
                return Err(crate::api::ApiError::Api {
                    status: 400,
                    body: i18n.t("common.invalid_ids"),
                });
            };
            api.list_notes(tid, Some(pid), None, None, None).await
        }
    });

    // Fetch all citations in the tree (unfiltered): the backend already
    // builds the full source/citation set internally regardless of filters,
    // so fetching once and filtering client-side (by person_id and by the
    // relevant event ids) avoids issuing one request per event.
    let api_citations = api.clone();
    let citations_resource = use_resource(move || {
        let api = api_citations.clone();
        let _tick = refresh();
        let tid = tree_id_parsed();
        async move {
            let Some(tid) = tid else {
                return Err(crate::api::ApiError::Api {
                    status: 400,
                    body: i18n.t("common.invalid_ids"),
                });
            };
            api.list_citations(tid, None, None, None, None).await
        }
    });

    // Fetch all sources in the tree, for rendering citation source text.
    let api_sources = api.clone();
    let sources_resource = use_resource(move || {
        let api = api_sources.clone();
        let _tick = refresh();
        let tid = tree_id_parsed();
        async move {
            let Some(tid) = tid else {
                return Err(crate::api::ApiError::Api {
                    status: 400,
                    body: i18n.t("common.invalid_ids"),
                });
            };
            api.list_all_sources(tid).await
        }
    });

    // Fetch tree snapshot for enriched events (direct API call).
    let api_snap = api.clone();
    let snapshot_resource = use_resource(move || {
        let api = api_snap.clone();
        let _tick = refresh();
        let _gen = tree_cache.generation();
        let tid = tree_id_parsed();
        async move {
            let Some(tid) = tid else {
                return Err(crate::api::ApiError::Api {
                    status: 400,
                    body: i18n.t("common.invalid_tree_id"),
                });
            };
            api.get_tree_snapshot(tid).await
        }
    });

    // Name lookup for *all* persons in the tree — used by family connections
    // (parents, spouses, children, siblings) and ancestry charts. Derived
    // from `snapshot_resource` rather than issuing a second, duplicate
    // `get_tree_snapshot` call.
    let all_names = use_memo(move || match &*snapshot_resource.read() {
        Some(Ok(snapshot)) => {
            let mut name_map: HashMap<Uuid, Vec<oxidgene_core::types::PersonName>> = HashMap::new();
            for pn in &snapshot.names {
                name_map.entry(pn.person_id).or_default().push(pn.clone());
            }
            Some(name_map)
        }
        _ => None,
    });

    // Fetch tree info (for breadcrumb, cache-backed).
    let api_tree = api.clone();
    let tree_resource = use_resource(move || {
        let api = api_tree.clone();
        let _tick = refresh();
        let _gen = tree_cache.generation();
        let tid = tree_id_parsed();
        async move {
            let Some(tid) = tid else {
                return Err(crate::api::ApiError::Api {
                    status: 400,
                    body: i18n.t("common.invalid_tree_id"),
                });
            };
            fetch_tree_cached(&api, &tree_cache, tid).await
        }
    });

    // Fetch SOSA ancestor IDs from the closure table (same query as the tree
    // view) — used to show the green SOSA badge in the family narrative.
    let api_sosa = api.clone();
    let sosa_ancestors_resource = use_resource(move || {
        let api = api_sosa.clone();
        let tid = tree_id_parsed();
        let _gen = tree_cache.generation();
        let sosa_root = match &*tree_resource.read() {
            Some(Ok(tree)) => tree.sosa_root_person_id,
            _ => None,
        };
        async move {
            let (Some(tid), Some(sosa_id)) = (tid, sosa_root) else {
                return HashSet::new();
            };
            match api.get_ancestors(tid, sosa_id, None).await {
                Ok(entries) => entries.into_iter().map(|a| a.ancestor_id).collect(),
                Err(_) => HashSet::new(),
            }
        }
    });

    // Tree-wide photo map (person_id -> file_path), used both for this
    // person's header avatar and for the mini pedigrees below — a single
    // `list_media_links_for_tree` call instead of two duplicate ones.
    let api_photos_map = api.clone();
    let photos_map_resource = use_resource(move || {
        let api = api_photos_map.clone();
        let tid = tree_id_parsed();
        async move {
            let Some(tid) = tid else {
                return HashMap::new();
            };
            api.list_media_links_for_tree(tid)
                .await
                .map(|rows| {
                    rows.into_iter()
                        .filter(|r| r.entity_type == "person")
                        .map(|r| (r.entity_id, r.file_path))
                        .collect()
                })
                .unwrap_or_default()
        }
    });

    // This person's portrait photo, derived from `photos_map_resource`
    // rather than issuing a second `list_media_links_for_tree` call.
    let photo = use_memo(move || {
        let pid = person_id_parsed()?;
        photos_map_resource.read().as_ref()?.get(&pid).cloned()
    });

    // Small static pedigree window (self + parents + grandparents), for the
    // Ancestors section.
    let api_anc_ped = api.clone();
    let ancestor_pedigree_resource = use_resource(move || {
        let api = api_anc_ped.clone();
        let tid = tree_id_parsed();
        let pid = person_id_parsed();
        async move {
            let (Some(tid), Some(pid)) = (tid, pid) else {
                return Err(crate::api::ApiError::Api {
                    status: 400,
                    body: i18n.t("common.invalid_ids"),
                });
            };
            api.get_pedigree(tid, pid, 2, 0).await.map(Some)
        }
    });

    // Resolve the name synchronously from the cache while the resource is
    // pending, so the breadcrumb never flashes a loading label.
    let tree_name_str = match &*tree_resource.read() {
        Some(Ok(tree)) => tree.name.clone(),
        _ => tree_id_parsed()
            .and_then(|tid| tree_cache.tree(tid))
            .map(|t| t.name)
            .unwrap_or_default(),
    };

    // Derive display name from loaded names.
    let display_name = match &*names_resource.read() {
        Some(Ok(names)) => {
            let primary = names.iter().find(|n| n.is_primary).or(names.first());
            match primary {
                Some(name) => {
                    let dn = name.display_name();
                    if dn.is_empty() {
                        i18n.t("common.unnamed")
                    } else {
                        dn
                    }
                }
                None => i18n.t("common.unnamed"),
            }
        }
        // Blank while loading — better than flashing a loading label
        // in the breadcrumb and page header.
        _ => String::new(),
    };

    // Alternate names shown under the header name (Geneanet-style), e.g.
    // "(Given Surname)". Excludes whichever name was picked as display_name
    // above, and de-duplicates identical given/surname combinations.
    let alt_names: Vec<String> = match &*names_resource.read() {
        Some(Ok(names)) => {
            let primary = names.iter().find(|n| n.is_primary).or(names.first());
            let primary_id = primary.map(|n| n.id);
            let mut seen: std::collections::HashSet<(String, String)> =
                std::collections::HashSet::new();
            if let Some(p) = primary {
                seen.insert((
                    p.given_names.clone().unwrap_or_default(),
                    p.surname.clone().unwrap_or_default(),
                ));
            }
            names
                .iter()
                .filter(|n| Some(n.id) != primary_id)
                .filter_map(|n| {
                    let key = (
                        n.given_names.clone().unwrap_or_default(),
                        n.surname.clone().unwrap_or_default(),
                    );
                    if !seen.insert(key) {
                        return None;
                    }
                    let dn = n.display_name();
                    if dn.is_empty() { None } else { Some(dn) }
                })
                .collect()
        }
        _ => Vec::new(),
    };

    // Helper: resolve place_id to place name.
    let place_name = |place_id: Uuid| -> String {
        let places_data = places_resource.read();
        match &*places_data {
            Some(Ok(places)) => places
                .iter()
                .find(|p| p.id == place_id)
                .map(|p| p.name.clone())
                .unwrap_or_else(|| place_id.to_string()[..8].to_string()),
            _ => place_id.to_string()[..8].to_string(),
        }
    };

    // One clause of the birth/death vitals sentence — kept structured (rather
    // than a flat formatted string) so the date/age can be rendered in bold.
    enum VitalClause {
        Born { date: String, place: Option<String> },
        Died { date: String, place: Option<String> },
        Age(AgeSpan),
        Occupation(String),
    }

    // Birth/death vitals clauses shown under the header name, e.g.
    // "Born on **10 December 1700** in Paris — **43 years old**."
    let vital_clauses: Vec<VitalClause> = match &*events_resource.read() {
        Some(Ok(conn)) => {
            let birth = conn
                .edges
                .iter()
                .map(|e| &e.node)
                .find(|e| e.event_type == EventType::Birth);
            let death = conn
                .edges
                .iter()
                .map(|e| &e.node)
                .find(|e| e.event_type == EventType::Death);

            let mut clauses = Vec::new();
            if let Some(b) = birth {
                clauses.push(VitalClause::Born {
                    date: b.date_value.clone().unwrap_or_default(),
                    place: b.place_id.map(&place_name),
                });
            }
            if let Some(d) = death {
                clauses.push(VitalClause::Died {
                    date: d.date_value.clone().unwrap_or_default(),
                    place: d.place_id.map(&place_name),
                });
            }
            if let Some(birth_date) = birth.and_then(|e| e.date_sort) {
                // Only fall back to "today" when the person has no death event at
                // all (still alive). If a death event exists but its date is
                // unrecorded, the age at death is unknown — don't guess it as the
                // current date, which would wildly inflate the age shown.
                let end_date = match death {
                    Some(d) => d.date_sort,
                    None => Some(chrono::Local::now().date_naive()),
                };
                if let Some(end_date) = end_date {
                    clauses.push(VitalClause::Age(age_span(birth_date, end_date)));
                }
            }

            // Occupation(s), shown on its own line below the birth/death
            // vitals (mirrors the "Profession" line on Geneanet person
            // pages). A person can have several OCCU events (career
            // changes) — list them all rather than picking just one.
            let occupations: Vec<String> = conn
                .edges
                .iter()
                .map(|e| &e.node)
                .filter(|e| e.event_type == EventType::Occupation)
                .filter_map(|e| e.description.clone())
                .filter(|title| !title.is_empty())
                .collect();
            if !occupations.is_empty() {
                clauses.push(VitalClause::Occupation(occupations.join(", ")));
            }

            clauses
        }
        _ => Vec::new(),
    };

    // Index of family-level events (marriage, divorce…) keyed by family_id,
    // used to describe unions in the family narrative below.
    let events_by_family: HashMap<Uuid, Vec<DomainEvent>> = match &*snapshot_resource.read() {
        Some(Ok(snapshot)) => {
            let mut map: HashMap<Uuid, Vec<DomainEvent>> = HashMap::new();
            for e in snapshot.events.iter() {
                if e.deleted_at.is_none()
                    && let Some(fid) = e.family_id
                {
                    map.entry(fid).or_default().push(e.clone());
                }
            }
            map
        }
        _ => HashMap::new(),
    };

    // Resolve a family's marriage date/place and divorce date, earliest first.
    let union_marriage_divorce = |fid: Uuid| -> (Option<String>, Option<String>, Option<String>) {
        let mut marriage_date = None;
        let mut marriage_place = None;
        let mut divorce_date = None;
        if let Some(events) = events_by_family.get(&fid) {
            let mut sorted: Vec<&DomainEvent> = events.iter().collect();
            sorted.sort_by_key(|e| e.date_sort);
            for e in sorted {
                match e.event_type {
                    EventType::Marriage if marriage_date.is_none() => {
                        marriage_date = e.date_value.clone();
                        marriage_place = e.place_id.map(&place_name);
                    }
                    EventType::Divorce if divorce_date.is_none() => {
                        divorce_date = e.date_value.clone();
                    }
                    _ => {}
                }
            }
        }
        (marriage_date, marriage_place, divorce_date)
    };

    // This person's sex, used to word the family narrative ("Son of…",
    // "Daughter of…", "Married"/"In a relationship"…).
    let person_sex: Option<Sex> = match &*person_resource.read() {
        Some(Ok(person)) => Some(person.sex),
        _ => None,
    };

    // ── Handlers ─────────────────────────────────────────────────────

    // Delete person handler.
    let tree_id_nav = tree_id.clone();
    let api_del = api.clone();
    let on_confirm_delete = move |_| {
        let api = api_del.clone();
        let Some(tid) = tree_id_parsed() else { return };
        let Some(pid) = person_id_parsed() else {
            return;
        };
        let tree_id_nav = tree_id_nav.clone();
        spawn(async move {
            match api.delete_person(tid, pid).await {
                Ok(_) => {
                    nav.push(Route::TreeDetail {
                        tree_id: tree_id_nav,
                        person: None,
                    });
                }
                Err(e) => {
                    delete_error.set(Some(format!("{e}")));
                }
            }
        });
    };

    // ── Render ────────────────────────────────────────────────────────

    // One of this person's own unions: partner(s), this person's role in it,
    // marriage/divorce info, and the children born into it.
    struct UnionGroup {
        partner_ids: Vec<Uuid>,
        role: oxidgene_core::SpouseRole,
        marriage_date: Option<String>,
        marriage_place: Option<String>,
        divorce_date: Option<String>,
        child_ids: Vec<Uuid>,
    }

    // A group of half-siblings sharing one parent with this person, born from
    // that parent's union with someone other than this person's other parent.
    struct SiblingGroup {
        common_parent_id: Uuid,
        other_parent_id: Option<Uuid>,
        child_ids: Vec<Uuid>,
    }

    // Build the family narrative data for the current person: parents, own
    // unions (grouped one per family, not flattened), full siblings, and
    // half-siblings (grouped by which parent they share).
    let family_data = {
        let pid = person_id_parsed();
        match (&*snapshot_resource.read(), pid) {
            (Some(Ok(snapshot)), Some(pid)) => {
                let all_spouses = snapshot
                    .spouses
                    .iter()
                    .map(|s| (s.family_id, s.clone()))
                    .collect::<Vec<_>>();
                let all_children = snapshot
                    .children
                    .iter()
                    .map(|c| (c.family_id, c.clone()))
                    .collect::<Vec<_>>();

                // ── This person's own unions ──
                let spouse_family_ids: Vec<Uuid> = all_spouses
                    .iter()
                    .filter(|(_fid, s)| s.person_id == pid)
                    .map(|(fid, _)| *fid)
                    .collect();

                let unions: Vec<UnionGroup> = spouse_family_ids
                    .iter()
                    .map(|fid| {
                        let role = all_spouses
                            .iter()
                            .find(|(f, s)| f == fid && s.person_id == pid)
                            .map(|(_, s)| s.role)
                            .unwrap_or(oxidgene_core::SpouseRole::Partner);
                        let partner_ids: Vec<Uuid> = all_spouses
                            .iter()
                            .filter(|(f, s)| f == fid && s.person_id != pid)
                            .map(|(_, s)| s.person_id)
                            .collect();
                        let child_ids: Vec<Uuid> = all_children
                            .iter()
                            .filter(|(f, _)| f == fid)
                            .map(|(_, c)| c.person_id)
                            .collect();
                        let (marriage_date, marriage_place, divorce_date) =
                            union_marriage_divorce(*fid);
                        UnionGroup {
                            partner_ids,
                            role,
                            marriage_date,
                            marriage_place,
                            divorce_date,
                            child_ids,
                        }
                    })
                    .collect();

                // ── Parents & full siblings (from the family this person is a child in) ──
                let child_family_ids: Vec<Uuid> = all_children
                    .iter()
                    .filter(|(_fid, c)| c.person_id == pid)
                    .map(|(fid, _)| *fid)
                    .collect();

                let mut parent_ids: Vec<Uuid> = Vec::new();
                let mut full_sibling_ids: Vec<Uuid> = Vec::new();
                for fid in &child_family_ids {
                    for (f, s) in all_spouses.iter() {
                        if f == fid {
                            parent_ids.push(s.person_id);
                        }
                    }
                    for (f, c) in all_children.iter() {
                        if f == fid && c.person_id != pid {
                            full_sibling_ids.push(c.person_id);
                        }
                    }
                }
                if parent_ids.is_empty()
                    && let Some(Ok(Some(pedigree))) = &*ancestor_pedigree_resource.read()
                {
                    let mut pedigree_parent_ids = pedigree
                        .edges
                        .iter()
                        .filter(|edge| edge.child_id == pid)
                        .map(|edge| edge.parent_id)
                        .collect::<Vec<_>>();
                    pedigree_parent_ids.sort_by_key(|parent_id| {
                        pedigree
                            .persons
                            .get(parent_id)
                            .map(|person| match person.sex {
                                Sex::Male => 0,
                                Sex::Female => 1,
                                Sex::Unknown => 2,
                            })
                            .unwrap_or(2)
                    });
                    pedigree_parent_ids.dedup();
                    parent_ids = pedigree_parent_ids;
                }

                // ── Half-siblings: each parent's *other* unions ──
                let mut half_sibling_groups: Vec<SiblingGroup> = Vec::new();
                for parent_id in parent_ids.iter().filter(|_| !child_family_ids.is_empty()) {
                    let other_family_ids: Vec<Uuid> = all_spouses
                        .iter()
                        .filter(|(fid, s)| {
                            s.person_id == *parent_id && !child_family_ids.contains(fid)
                        })
                        .map(|(fid, _)| *fid)
                        .collect();
                    for fid in &other_family_ids {
                        let other_parent_id = all_spouses
                            .iter()
                            .find(|(f, s)| f == fid && s.person_id != *parent_id)
                            .map(|(_, s)| s.person_id);
                        let child_ids: Vec<Uuid> = all_children
                            .iter()
                            .filter(|(f, _)| f == fid)
                            .map(|(_, c)| c.person_id)
                            .collect();
                        if !child_ids.is_empty() {
                            half_sibling_groups.push(SiblingGroup {
                                common_parent_id: *parent_id,
                                other_parent_id,
                                child_ids,
                            });
                        }
                    }
                }

                Some((parent_ids, unions, full_sibling_ids, half_sibling_groups))
            }
            _ => None,
        }
    };

    // Helper to resolve person name from all_names or names_resource
    let resolve_person_name = |pid: Uuid| -> String {
        if let Some(name_map) = &*all_names.read() {
            return resolve_name(pid, name_map);
        }
        i18n.t("common.unknown")
    };

    // Tree-wide sex + lifespan lookups, used to decorate every person
    // mentioned in the family narrative (sex glyph + "birth-death" suffix,
    // matching the format shown on the pedigree cards).
    let (person_sex_map, person_lifespan_map): (HashMap<Uuid, Sex>, HashMap<Uuid, String>) =
        match &*snapshot_resource.read() {
            Some(Ok(snapshot)) => {
                let sex_map: HashMap<Uuid, Sex> =
                    snapshot.persons.iter().map(|p| (p.id, p.sex)).collect();

                let mut birth_years: HashMap<Uuid, i32> = HashMap::new();
                let mut death_years: HashMap<Uuid, i32> = HashMap::new();
                for e in &snapshot.events {
                    let Some(pid) = e.person_id else { continue };
                    let Some(year) = e
                        .date_value
                        .as_deref()
                        .and_then(crate::components::pedigree_chart::extract_year)
                    else {
                        continue;
                    };
                    match e.event_type {
                        EventType::Birth => {
                            birth_years.entry(pid).or_insert(year);
                        }
                        EventType::Death => {
                            death_years.entry(pid).or_insert(year);
                        }
                        _ => {}
                    }
                }
                let lifespan_map: HashMap<Uuid, String> = sex_map
                    .keys()
                    .filter_map(|pid| {
                        let lifespan = crate::components::pedigree_chart::format_lifespan(
                            birth_years.get(pid).copied(),
                            death_years.get(pid).copied(),
                        );
                        (!lifespan.is_empty()).then_some((*pid, lifespan))
                    })
                    .collect();
                (sex_map, lifespan_map)
            }
            _ => (HashMap::new(), HashMap::new()),
        };

    let sosa_ancestors: HashSet<Uuid> = sosa_ancestors_resource.read().clone().unwrap_or_default();

    // Renders "[SOSA mark] [sex glyph] Name [years]", linked to the
    // person's own page — used throughout the family narrative below.
    let person_chip = |pid: Uuid| -> Element {
        let name = resolve_person_name(pid);
        let sex = person_sex_map.get(&pid).copied().unwrap_or(Sex::Unknown);
        let sex_glyph = match sex {
            Sex::Male => "\u{2642}",
            Sex::Female => "\u{2640}",
            Sex::Unknown => "?",
        };
        let sex_class = match sex {
            Sex::Male => "pd-sex-glyph male",
            Sex::Female => "pd-sex-glyph female",
            Sex::Unknown => "pd-sex-glyph",
        };
        let lifespan = person_lifespan_map.get(&pid).cloned().unwrap_or_default();
        let is_sosa = sosa_ancestors.contains(&pid);
        let tid = tree_id.clone();
        rsx! {
            span { class: "pd-person-chip",
                if is_sosa {
                    svg { class: "pd-sosa-mark", "viewBox": "0 0 10 10", width: "10", height: "10",
                        circle { cx: "5", cy: "5", r: "5", fill: "rgb(149,196,23)" }
                        circle { cx: "5", cy: "5", r: "3", fill: "#fff" }
                        circle { cx: "5", cy: "5", r: "1.8", fill: "rgb(149,196,23)" }
                    }
                }
                span { class: sex_class, "{sex_glyph}" }
                Link {
                    to: Route::PersonDetail { tree_id: tid, person_id: pid.to_string() },
                    class: "pd-person-link",
                    "{name}"
                }
                if !lifespan.is_empty() {
                    span { class: "pd-person-years", "{lifespan}" }
                }
            }
        }
    };

    // ── Build enriched event list ───────────────────────────────────
    //
    // Combines three sources:
    //   1. Individual events (birth, death, occupation…)
    //   2. Conjugal family events (marriage, divorce…)
    //   3. Parental family events (parent death, sibling birth…)
    let enriched_events: Vec<EnrichedEvent> = {
        let snap_data = snapshot_resource.read();
        let pid = person_id_parsed();

        match (&*snap_data, pid) {
            (Some(Ok(snapshot)), Some(pid)) => {
                let all_spouses = snapshot
                    .spouses
                    .iter()
                    .map(|s| (s.family_id, s.clone()))
                    .collect::<Vec<_>>();
                let all_children = snapshot
                    .children
                    .iter()
                    .map(|c| (c.family_id, c.clone()))
                    .collect::<Vec<_>>();

                // Index events by person_id and family_id.
                let mut events_by_person: HashMap<Uuid, Vec<&DomainEvent>> = HashMap::new();
                let mut events_by_family: HashMap<Uuid, Vec<&DomainEvent>> = HashMap::new();
                for e in snapshot.events.iter() {
                    if e.deleted_at.is_some() {
                        continue;
                    }
                    if let Some(epid) = e.person_id {
                        events_by_person.entry(epid).or_default().push(e);
                    }
                    if let Some(fid) = e.family_id {
                        events_by_family.entry(fid).or_default().push(e);
                    }
                }

                // Derive family IDs (same logic as family_connections).
                let spouse_family_ids: Vec<Uuid> = all_spouses
                    .iter()
                    .filter(|(_fid, s)| s.person_id == pid)
                    .map(|(fid, _)| *fid)
                    .collect();
                let child_family_ids: Vec<Uuid> = all_children
                    .iter()
                    .filter(|(_fid, c)| c.person_id == pid)
                    .map(|(fid, _)| *fid)
                    .collect();

                let mut result: Vec<EnrichedEvent> = Vec::new();
                let mut seen_ids: std::collections::HashSet<Uuid> =
                    std::collections::HashSet::new();

                // 1. Individual events.
                if let Some(person_events) = events_by_person.get(&pid) {
                    for &e in person_events {
                        if seen_ids.insert(e.id) {
                            result.push(EnrichedEvent {
                                event: e.clone(),
                                origin: EventOrigin::Individual,
                                context: None,
                            });
                        }
                    }
                }

                // 2. Conjugal family events (from families where person is spouse).
                for fid in &spouse_family_ids {
                    // Find partner name for context.
                    let partner_name = all_spouses
                        .iter()
                        .find(|(f, s)| f == fid && s.person_id != pid)
                        .map(|(_, s)| resolve_person_name(s.person_id));

                    if let Some(fam_events) = events_by_family.get(fid) {
                        for &e in fam_events {
                            if seen_ids.insert(e.id) {
                                result.push(EnrichedEvent {
                                    event: e.clone(),
                                    origin: EventOrigin::ConjugalFamily,
                                    context: partner_name.clone(),
                                });
                            }
                        }
                    }

                    // Major individual events of children (birth, death, baptism, burial).
                    for (f, c) in all_children.iter() {
                        if *f != *fid {
                            continue;
                        }
                        let child_name = resolve_person_name(c.person_id);
                        if let Some(child_events) = events_by_person.get(&c.person_id) {
                            for &e in child_events {
                                if (e.event_type == EventType::Birth
                                    || e.event_type == EventType::Death
                                    || e.event_type == EventType::Baptism
                                    || e.event_type == EventType::Burial)
                                    && seen_ids.insert(e.id)
                                {
                                    result.push(EnrichedEvent {
                                        event: e.clone(),
                                        origin: EventOrigin::ChildFamily,
                                        context: Some(child_name.clone()),
                                    });
                                }
                            }
                        }
                    }
                }

                // 3. Parental family events (from families where person is child).
                for fid in &child_family_ids {
                    // Family-level events of parental family.
                    if let Some(fam_events) = events_by_family.get(fid) {
                        for &e in fam_events {
                            if seen_ids.insert(e.id) {
                                result.push(EnrichedEvent {
                                    event: e.clone(),
                                    origin: EventOrigin::ParentalFamily,
                                    context: None,
                                });
                            }
                        }
                    }

                    // Major individual events of parents (death, burial).
                    for (f, s) in all_spouses.iter() {
                        if f != fid {
                            continue;
                        }
                        let parent_name = resolve_person_name(s.person_id);
                        if let Some(parent_events) = events_by_person.get(&s.person_id) {
                            for &e in parent_events {
                                if (e.event_type == EventType::Death
                                    || e.event_type == EventType::Burial)
                                    && seen_ids.insert(e.id)
                                {
                                    result.push(EnrichedEvent {
                                        event: e.clone(),
                                        origin: EventOrigin::ParentalFamily,
                                        context: Some(parent_name.clone()),
                                    });
                                }
                            }
                        }
                    }

                    // Major individual events of siblings (birth, death, baptism, burial).
                    for (f, c) in all_children.iter() {
                        if f != fid || c.person_id == pid {
                            continue;
                        }
                        let sib_name = resolve_person_name(c.person_id);
                        if let Some(sib_events) = events_by_person.get(&c.person_id) {
                            for &e in sib_events {
                                if (e.event_type == EventType::Birth
                                    || e.event_type == EventType::Death
                                    || e.event_type == EventType::Baptism
                                    || e.event_type == EventType::Burial)
                                    && seen_ids.insert(e.id)
                                {
                                    result.push(EnrichedEvent {
                                        event: e.clone(),
                                        origin: EventOrigin::ParentalFamily,
                                        context: Some(sib_name.clone()),
                                    });
                                }
                            }
                        }
                    }
                }

                // Sort by date.
                result.sort_by_key(|a| a.event.date_sort);
                result
            }
            _ => Vec::new(),
        }
    };

    // ── Build per-event source citations ──────────────────────────────
    //
    // One entry per event, listing its citations ("Source title — page"),
    // rendered directly under that event in the timeline instead of a
    // separate "Sources" section.
    let citations_by_event: HashMap<Uuid, Vec<String>> = {
        match (&*citations_resource.read(), &*sources_resource.read()) {
            (Some(Ok(citations)), Some(Ok(sources))) => {
                let source_by_id: HashMap<Uuid, &oxidgene_core::types::Source> =
                    sources.iter().map(|s| (s.id, s)).collect();

                let mut result: HashMap<Uuid, Vec<String>> = HashMap::new();
                for citation in citations {
                    let Some(eid) = citation.event_id else {
                        continue;
                    };
                    let Some(source) = source_by_id.get(&citation.source_id) else {
                        continue;
                    };
                    let text = match &citation.page {
                        Some(page) if !page.is_empty() => {
                            format!("{} \u{2014} {page}", source.title)
                        }
                        _ => source.title.clone(),
                    };
                    result.entry(eid).or_default().push(text);
                }
                result
            }
            _ => HashMap::new(),
        }
    };

    rsx! {
        div { class: "sub-page",
        // Breadcrumb
        div { class: "td-topbar",
            nav { class: "td-bc",
                Link { to: Route::Home {}, class: "td-bc-logo",
                    img {
                        src: crate::components::layout::LOGO_PNG_B64,
                        alt: "OxidGene",
                        class: "td-bc-logo-img",
                    }
                }
                if !tree_name_str.is_empty() {
                    Link {
                        to: Route::TreeDetail { tree_id: tree_id.clone(), person: None },
                        class: "td-bc-link",
                        "{tree_name_str}"
                    }
                    span { class: "td-bc-sep", "/" }
                }
                span { class: "td-bc-current", "{display_name}" }
            }
            TopbarSearch { tree_id: tree_id.clone(), from_person: true }
        }

        div { class: "pd-page-shell",
        TreeIconSidebar {
            active_view: TreeSidebarView::Profile,
            selected_person_id: person_id_parsed(),
            on_profile_view: move |_| {},
            on_pedigree_view: {
                let tree_id = tree_id.clone();
                let person_id = person_id.clone();
                move |_| {
                    nav.push(Route::TreeDetail {
                        tree_id: tree_id.clone(),
                        person: Some(person_id.clone()),
                    });
                }
            },
            on_add_person: move |_| show_create_person.set(true),
            on_settings: {
                let tree_id = tree_id.clone();
                move |_| {
                    nav.push(Route::Settings {
                        tree_id: tree_id.clone(),
                    });
                }
            },
        }

        div { class: "sub-page-content pd-content",

        // Person edit modal (civil status, names, birth/death — see PersonForm).
        if show_edit_person() {
            if let Some(tid) = tree_id_parsed() {
                PersonForm {
                    tree_id: tid,
                    person_id: person_id_parsed(),
                    on_close: move |_| show_edit_person.set(false),
                    on_saved: move |_| refresh += 1,
                }
            }
        }

        if show_create_person() {
            if let Some(tid) = tree_id_parsed() {
                PersonForm {
                    tree_id: tid,
                    create_context: PersonFormCreateContext::Standalone,
                    on_close: move |_| show_create_person.set(false),
                    on_saved: move |_| {
                        tree_cache.invalidate();
                        refresh += 1;
                    },
                }
            }
        }

        // Delete person confirmation dialog
        if confirm_delete() {
            ConfirmDialog {
                title: i18n.t("confirm.delete_person.title"),
                message: i18n.t_args("confirm.delete_person.message_name", &[("name", &display_name)]),
                confirm_label: i18n.t("common.delete"),
                confirm_class: "btn btn-danger",
                error: delete_error(),
                on_confirm: move |_| on_confirm_delete(()),
                on_cancel: move |_| {
                    confirm_delete.set(false);
                    delete_error.set(None);
                },
            }
        }

        // Person header
        match &*person_resource.read() {
            Some(Ok(person)) => {
                let person_sex = person.sex;
                let sex_symbol = match person_sex {
                    Sex::Male => "\u{2642}",
                    Sex::Female => "\u{2640}",
                    Sex::Unknown => "?",
                };
                let avatar_src = match &*photo.read() {
                    Some(url) => url.clone(),
                    None => crate::components::pedigree_chart::default_portrait(person_sex).to_string(),
                };
                rsx! {
                    div { class: "card page-header",
                        div { class: "pd-header-left",
                            img { class: "pd-avatar", alt: "", src: "{avatar_src}" }
                            div { class: "pd-header-main",
                                div { class: "pd-header-top",
                                    h1 { "{display_name}" }
                                }
                                if !alt_names.is_empty() {
                                    p { class: "pd-alt-names",
                                        for n in alt_names.iter() {
                                            span { key: "{n}", "({n})" }
                                        }
                                    }
                                }
                                if !vital_clauses.is_empty() {
                                    p { class: "pd-vitals",
                                        span { class: "pd-sex-mark", "{sex_symbol}" }
                                        for (i, clause) in vital_clauses.iter().enumerate() {
                                            if i > 0 {
                                                match clause {
                                                    VitalClause::Died { .. }
                                                    | VitalClause::Occupation(_) => rsx! { br {} },
                                                    _ => rsx! { " \u{2014} " },
                                                }
                                            }
                                            {
                                                match clause {
                                                    VitalClause::Born { date, place } => {
                                                        let place_clause = place
                                                            .as_ref()
                                                            .map(|p| format!(" {}", i18n.t_args("person.vitals.in_place", &[("place", p)])))
                                                            .unwrap_or_default();
                                                        rsx! {
                                                            "{i18n.t(\"person.vitals.born_prefix\")} "
                                                            b { "{date}" }
                                                            "{place_clause}"
                                                        }
                                                    }
                                                    VitalClause::Died { date, place } => {
                                                        let place_clause = place
                                                            .as_ref()
                                                            .map(|p| format!(" {}", i18n.t_args("person.vitals.in_place", &[("place", p)])))
                                                            .unwrap_or_default();
                                                        if date.is_empty() {
                                                            rsx! {
                                                                b { "{i18n.t(\"person.vitals.died_no_date\")}" }
                                                                "{place_clause}"
                                                            }
                                                        } else {
                                                            rsx! {
                                                                "{i18n.t(\"person.vitals.died_prefix\")} "
                                                                b { "{date}" }
                                                                "{place_clause}"
                                                            }
                                                        }
                                                    }
                                                    VitalClause::Age(age) => {
                                                        let (key, n) = match age {
                                                            AgeSpan::Days(n) => ("person.vitals.age_days", *n),
                                                            AgeSpan::Months(n) => ("person.vitals.age_months", *n),
                                                            AgeSpan::Years(n) => ("person.vitals.age", *n),
                                                        };
                                                        let label = i18n
                                                            .t_plural(key, n as usize)
                                                            .replace("{n}", &n.to_string());
                                                        rsx! { b { "{label}" } }
                                                    }
                                                    VitalClause::Occupation(title) => {
                                                        rsx! { "{title}" }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        div { class: "pd-header-actions",
                            div { class: "pd-header-sosa",
                                if let Some(sosa) = person.sosa_number {
                                    span { class: "badge pd-sosa-badge",
                                        "SOSA {sosa}"
                                    }
                                }
                            }
                            div { class: "pd-header-buttons",
                                button {
                                    class: "btn btn-danger",
                                    onclick: move |_| {
                                        confirm_delete.set(true);
                                        delete_error.set(None);
                                    },
                                    {i18n.t("common.delete")}
                                }
                                button {
                                    class: "btn btn-outline",
                                    onclick: move |_| show_edit_person.set(true),
                                    {i18n.t("common.edit")}
                                }
                                if SHOW_MANUAL_REFRESH {
                                    button {
                                        class: "btn btn-outline",
                                        onclick: move |_| refresh += 1,
                                        {i18n.t("person.refresh")}
                                    }
                                }
                            }
                        }
                    }
                }
            },
            Some(Err(e)) => rsx! {
                div { class: "error-msg", {i18n.t_args("person.load_error", &[("error", &e.to_string())])} }
            },
            None => rsx! {
                div { class: "loading", {i18n.t("person.loading")} }
            },
        }

        // ── Notes section ────────────────────────────────────────────
        match &*notes_resource.read() {
            Some(Ok(notes)) if !notes.is_empty() => rsx! {
                div { class: "card", style: "margin-bottom: 24px;",
                    h2 { style: "font-size: 1.1rem; margin-bottom: 12px;", {i18n.t("person.notes_section")} }

                    div {
                        for note in notes.iter() {
                            div {
                                style: "margin-bottom: 12px; padding: 12px; border: 1px solid var(--color-border); border-radius: var(--radius);",
                                p { style: "margin: 0; white-space: pre-wrap;", "{note.text}" }
                            }
                        }
                    }
                }
            },
            Some(Err(e)) => rsx! {
                div { class: "error-msg", {i18n.t_args("person.load_notes_error", &[("error", &e.to_string())])} }
            },
            _ => rsx! {},
        }

        // ── Family section (narrative) ────────────────────────────────
        if let Some((parent_ids, unions, full_sibling_ids, half_sibling_groups)) = &family_data {
            div { class: "card", style: "margin-bottom: 24px;",
                h2 { style: "font-size: 1.1rem; margin-bottom: 12px;", {i18n.t("person.family_connections")} }

                if !parent_ids.is_empty() {
                    p { class: "pd-family-prose",
                        {
                            let key = match (parent_ids.len() >= 2, person_sex) {
                                (true, Some(Sex::Male)) => "person.family.son_of_two",
                                (true, Some(Sex::Female)) => "person.family.daughter_of_two",
                                (true, _) => "person.family.child_of_two",
                                (false, Some(Sex::Male)) => "person.family.son_of_one",
                                (false, Some(Sex::Female)) => "person.family.daughter_of_one",
                                (false, _) => "person.family.child_of_one",
                            };
                            let template = i18n.t(key);
                            if parent_ids.len() >= 2 {
                                let p1 = parent_ids[0];
                                let p2 = parent_ids[1];
                                let (pre, rest) =
                                    template.split_once("{p1}").unwrap_or((template.as_str(), ""));
                                let (mid, post) = rest.split_once("{p2}").unwrap_or((rest, ""));
                                let (pre, mid, post) =
                                    (pre.to_string(), mid.to_string(), post.to_string());
                                rsx! {
                                    "{pre}"
                                    {person_chip(p1)}
                                    "{mid}"
                                    {person_chip(p2)}
                                    "{post}"
                                }
                            } else {
                                let p1 = parent_ids[0];
                                let (pre, post) =
                                    template.split_once("{p1}").unwrap_or((template.as_str(), ""));
                                let (pre, post) = (pre.to_string(), post.to_string());
                                rsx! {
                                    "{pre}"
                                    {person_chip(p1)}
                                    "{post}"
                                }
                            }
                        }
                    }
                }

                for (idx, union) in unions.iter().enumerate() {
                    div { key: "{idx}", class: "pd-union",
                        p { class: "pd-union-line",
                            {
                                // Everything up to "with" is plain text; the
                                // partner name(s) need real links, so the
                                // "with {partner}" template is split around
                                // its placeholder instead of substituted.
                                let verb = if union.role == oxidgene_core::SpouseRole::Partner {
                                    i18n.t("person.family.in_relationship")
                                } else {
                                    i18n.t("person.family.married")
                                };
                                let mut prefix = verb;
                                if let Some(date) = &union.marriage_date {
                                    prefix.push(' ');
                                    prefix.push_str(&i18n.t_args("person.family.on_date", &[("date", date)]));
                                }
                                if let Some(place) = &union.marriage_place {
                                    prefix.push(' ');
                                    prefix.push_str(&i18n.t_args("person.family.in_place", &[("place", place)]));
                                }
                                prefix.push_str(", ");
                                let with_template = i18n.t("person.family.with_person");
                                let (with_pre, with_post) = with_template
                                    .split_once("{partner}")
                                    .unwrap_or((with_template.as_str(), ""));
                                prefix.push_str(with_pre);

                                let mut suffix = with_post.to_string();
                                if let Some(ddate) = &union.divorce_date {
                                    suffix.push_str(", ");
                                    suffix.push_str(&i18n.t_args("person.family.divorced_on", &[("date", ddate)]));
                                }
                                if union.child_ids.is_empty() {
                                    suffix.push('.');
                                } else {
                                    suffix.push_str(", ");
                                    suffix.push_str(&i18n.t("person.family.and_had"));
                                }

                                let and_word = i18n.t("common.and");
                                let partner_ids = union.partner_ids.clone();
                                rsx! {
                                    "{prefix}"
                                    for (i, pid) in partner_ids.iter().enumerate() {
                                        if i > 0 {
                                            " {and_word} "
                                        }
                                        {person_chip(*pid)}
                                    }
                                    "{suffix}"
                                }
                            }
                        }
                        if !union.child_ids.is_empty() {
                            ul { class: "pd-children",
                                for cid in union.child_ids.iter() {
                                    { let cid = *cid; rsx! {
                                        li { {person_chip(cid)} }
                                    }}
                                }
                            }
                        }
                    }
                }

                if !full_sibling_ids.is_empty() {
                    div { class: "pd-fc-section",
                        h3 { class: "pd-fc-label", {i18n.t("person.siblings")} }
                        ul { class: "pd-children",
                            for sid in full_sibling_ids.iter() {
                                { let sid = *sid; rsx! {
                                    li { {person_chip(sid)} }
                                }}
                            }
                        }
                    }
                }

                if !half_sibling_groups.is_empty() {
                    div { class: "pd-fc-section",
                        h3 { class: "pd-fc-label", {i18n.t("person.half_siblings")} }
                        for (idx, group) in half_sibling_groups.iter().enumerate() {
                            div { key: "{idx}", class: "pd-sib-group",
                                p { class: "pd-sib-group-head",
                                    {
                                        let side_template = i18n.t("person.family.side_of");
                                        let (side_pre, side_post) = side_template
                                            .split_once("{parent}")
                                            .unwrap_or((side_template.as_str(), ""));
                                        let with_template = i18n.t("person.family.with_person");
                                        let (with_pre, with_post) = with_template
                                            .split_once("{partner}")
                                            .unwrap_or((with_template.as_str(), ""));
                                        let unknown_label = i18n.t("person.family.unknown_person");
                                        let common_parent = group.common_parent_id;
                                        let other_parent = group.other_parent_id;
                                        rsx! {
                                            "{side_pre}"
                                            {person_chip(common_parent)}
                                            "{side_post}, {with_pre}"
                                            if let Some(pid) = other_parent {
                                                {person_chip(pid)}
                                            } else {
                                                "{unknown_label}"
                                            }
                                            "{with_post}"
                                        }
                                    }
                                }
                                ul { class: "pd-children",
                                    for cid in group.child_ids.iter() {
                                        { let cid = *cid; rsx! {
                                            li { {person_chip(cid)} }
                                        }}
                                    }
                                }
                            }
                        }
                    }
                }

                if parent_ids.is_empty() && unions.is_empty() && full_sibling_ids.is_empty() && half_sibling_groups.is_empty() {
                    div { class: "empty-state",
                        p { {i18n.t("person.no_family_connections")} }
                    }
                }
            }
        }

        // ── Events section ───────────────────────────────────────────
        div { class: "card", style: "margin-bottom: 24px;",
            div { class: "section-header",
                h2 { style: "font-size: 1.1rem;", {i18n.t("person.events_section")} }
            }

            match &*events_resource.read() {
                Some(Ok(_conn)) => rsx! {
                    if enriched_events.is_empty() {
                        div { class: "empty-state",
                            p { {i18n.t("person.no_events")} }
                        }
                    } else {
                        ul { class: "pd-timeline",
                            for ee in enriched_events.iter() {
                                {
                                    let event = &ee.event;
                                    let eid = event.id;
                                    let event_type_key = format!("event.type.{}", event.event_type);
                                    let event_type_label = i18n.t(&event_type_key);
                                    let desc = event.description.clone().unwrap_or_default();
                                    let place_display = event.place_id.map(&place_name);

                                    // Origin label.
                                    let origin_label = match &ee.origin {
                                        EventOrigin::Individual => i18n.t("person.origin_individual"),
                                        EventOrigin::ConjugalFamily => i18n.t("person.origin_conjugal"),
                                        EventOrigin::ChildFamily => i18n.t("person.origin_child"),
                                        EventOrigin::ParentalFamily => i18n.t("person.origin_parental"),
                                    };
                                    let origin_display = if let Some(ref ctx) = ee.context {
                                        format!("{origin_label} ({ctx})")
                                    } else {
                                        origin_label
                                    };

                                    let is_direct = matches!(
                                        ee.origin,
                                        EventOrigin::Individual | EventOrigin::ConjugalFamily
                                    );
                                    let li_class = if is_direct { "pd-ev-direct" } else { "" };
                                    let event_sources = citations_by_event.get(&eid);

                                    rsx! {
                                        li { key: "{eid}", class: "{li_class}",
                                            span { class: "pd-ev-date",
                                                {event.date_value.as_deref().unwrap_or("--")}
                                            }
                                            div { class: "pd-ev-body",
                                                div { class: "pd-ev-row",
                                                    div {
                                                        span { class: "badge", "{event_type_label}" }
                                                        if let Some(place) = &place_display {
                                                            " \u{2014} {place}"
                                                        }
                                                        if !desc.is_empty() {
                                                            span { class: "text-muted", " \u{2014} {desc}" }
                                                        }
                                                    }
                                                }
                                                div { class: "pd-ev-origin", "{origin_display}" }
                                                if let Some(sources) = event_sources {
                                                    div { class: "pd-ev-sources",
                                                        {i18n.t("person.sources_section")}
                                                        ": {sources.join(\"; \")}"
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                },
                Some(Err(e)) => rsx! {
                    div { class: "error-msg", {i18n.t_args("person.load_events_error", &[("error", &e.to_string())])} }
                },
                None => rsx! {
                    div { class: "loading", {i18n.t("person.loading_events")} }
                },
            }
        }

        // ── Ancestors section ─────────────────────────────────────────
        div { class: "card",
            div { class: "section-header",
                h2 { style: "font-size: 1.1rem;", {i18n.t("person.ancestors")} }
            }

            {
                let tid = tree_id.clone();
                let on_navigate = EventHandler::new(move |pid: Uuid| {
                    nav.push(Route::PersonDetail { tree_id: tid.clone(), person_id: pid.to_string() });
                });
                render_mini_pedigree(
                    &ancestor_pedigree_resource,
                    &photos_map_resource,
                    2,
                    0,
                    on_navigate,
                    &i18n,
                )
            }
        }
        } // close sub-page-content
        } // close pd-page-shell
        } // close sub-page
    }
}

/// Whole years between two dates, matching the usual "age" definition
/// (doesn't count the current year until the birthday has passed).
fn age_in_years(birth: chrono::NaiveDate, end: chrono::NaiveDate) -> i32 {
    use chrono::Datelike;
    let mut age = end.year() - birth.year();
    if (end.month(), end.day()) < (birth.month(), birth.day()) {
        age -= 1;
    }
    age.max(0)
}

/// Whole months between two dates (doesn't count the current month until the
/// day-of-month has passed) — used by `age_span` to pick a display unit.
fn months_between(birth: chrono::NaiveDate, end: chrono::NaiveDate) -> i32 {
    use chrono::Datelike;
    let mut months = (end.year() - birth.year()) * 12 + end.month() as i32 - birth.month() as i32;
    if end.day() < birth.day() {
        months -= 1;
    }
    months.max(0)
}

/// A person's age at `end`, expressed in the coarsest unit that keeps it
/// meaningful: days for infants under one month old, months for children
/// under one year old, years otherwise.
enum AgeSpan {
    Days(i32),
    Months(i32),
    Years(i32),
}

fn age_span(birth: chrono::NaiveDate, end: chrono::NaiveDate) -> AgeSpan {
    let months = months_between(birth, end);
    if months < 1 {
        AgeSpan::Days((end - birth).num_days().max(0) as i32)
    } else if months < 12 {
        AgeSpan::Months(months)
    } else {
        AgeSpan::Years(age_in_years(birth, end))
    }
}

// ── Helper: static mini-pedigree rendering ────────────────────────────

/// Renders a small static (no pan/zoom/drag) pedigree fragment — used for
/// both the Ancestors (parents + grandparents) and Descendants (children +
/// grandchildren) sections, depending on the levels passed in.
fn render_mini_pedigree(
    pedigree_resource: &Resource<Result<Option<CachedPedigree>, crate::api::ApiError>>,
    photos_resource: &Resource<HashMap<Uuid, String>>,
    ancestor_levels: usize,
    descendant_levels: usize,
    on_navigate: EventHandler<Uuid>,
    i18n: &crate::i18n::I18n,
) -> Element {
    let ped_data = pedigree_resource.read();
    let cached = match &*ped_data {
        Some(Ok(Some(c))) => c,
        Some(Ok(None)) | None => {
            return rsx! {
                div { class: "loading", {i18n.t("person.loading_ancestry")} }
            };
        }
        Some(Err(e)) => {
            return rsx! {
                div { class: "error-msg", {i18n.t_args("person.load_ancestry_error", &[("error", &e.to_string())])} }
            };
        }
    };

    let mut data = crate::components::pedigree_chart::PedigreeData::from_cached_pedigree(cached);
    data.photos = photos_resource.read().clone().unwrap_or_default();
    let root_person_id = cached.root_person_id;

    rsx! {
        crate::components::pedigree_chart::MiniPedigree {
            root_person_id: root_person_id,
            data: data,
            ancestor_levels: ancestor_levels,
            descendant_levels: descendant_levels,
            on_person_navigate: on_navigate,
        }
    }
}
