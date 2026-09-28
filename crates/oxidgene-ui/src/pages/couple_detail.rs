//! Couple page — both spouses of a family side by side, with what they share
//! drawn once across the two columns.

use std::collections::HashMap;
use std::sync::Arc;

use dioxus::prelude::*;
use oxidgene_core::Sex;
use oxidgene_core::projection::Pedigree;
use oxidgene_core::types::{FamilySpouse, Note};
use uuid::Uuid;

use crate::api::{ApiClient, ApiError, PersonDetailBundle};
use crate::components::media_gallery::MediaOwner;
use crate::components::person_form::{PersonForm, PersonFormCreateContext};
use crate::components::person_profile::{
    EnrichedEvent, EventOrigin, Profile, ProfileMediaCard, SHOW_MANUAL_REFRESH, SectionContext,
    SharedProfile, ancestors_section, build_profile, children_list, couple_sides, family_section,
    header_section, media_event_links, notes_section, refresh_button, timeline_section, union_line,
    use_ancestor_pedigree, use_mini_pedigree, use_sosa_ancestors, use_tree_resource,
};
use crate::components::topbar_search::TopbarSearch;
use crate::components::tree_cache::{use_track_current_person, use_tree_cache};
use crate::components::tree_icon_sidebar::{TreeIconSidebar, TreeSidebarView};
use crate::components::union_form::UnionForm;
use crate::i18n::{I18n, use_i18n};
use crate::router::Route;
use crate::ui_observability::{UiLoadTrace, UiPage, use_traced_resource, use_ui_load_trace};

/// The family's spouses and each one's detail bundle, loaded together.
struct CoupleData {
    spouses: Vec<FamilySpouse>,
    bundles: HashMap<Uuid, Arc<PersonDetailBundle>>,
}

impl CoupleData {
    fn sex_of(&self, person_id: Uuid) -> Sex {
        self.bundles
            .get(&person_id)
            .and_then(|bundle| bundle.persons.iter().find(|p| p.id == person_id))
            .map_or(Sex::Unknown, |person| person.sex)
    }
}

/// Page rendered at `/trees/:tree_id/couples/:family_id`.
#[component]
pub fn CoupleDetail(tree_id: String, family_id: String) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let nav = use_navigator();
    let tree_cache = use_tree_cache();
    let load_trace = use_ui_load_trace(UiPage::CoupleDetail);
    let mut refresh = use_signal(|| 0u32);
    let mut media_revision = use_signal(|| 0_u32);
    let mut show_edit_couple = use_signal(|| false);
    let mut show_create_person = use_signal(|| false);

    // Reactive IDs, as on the person page: the router reuses this instance
    // when the spouse selectors move to another couple.
    let mut tree_id_parsed = use_signal(|| tree_id.parse::<Uuid>().ok());
    let new_tid = tree_id.parse::<Uuid>().ok();
    if new_tid != *tree_id_parsed.peek() {
        *tree_id_parsed.write() = new_tid;
    }
    let mut family_id_parsed = use_signal(|| family_id.parse::<Uuid>().ok());
    let new_fid = family_id.parse::<Uuid>().ok();
    if new_fid != *family_id_parsed.peek() {
        *family_id_parsed.write() = new_fid;
    }

    use_effect(move || {
        family_id_parsed();
        document::eval(
            "document.querySelector('.sub-page-content')?.scrollTo({ top: 0, behavior: 'instant' });",
        );
    });

    // ── Resources ────────────────────────────────────────────────────

    // The family (which fails once the couple is deleted), its spouses, and
    // both spouses' bundles, fetched concurrently.
    let api_couple = api.clone();
    let couple_resource = use_traced_resource(load_trace.clone(), "couple", move || {
        let api = api_couple.clone();
        let _tick = refresh();
        let _media_tick = media_revision();
        let (tid, fid) = (tree_id_parsed(), family_id_parsed());
        async move {
            let (Some(tid), Some(fid)) = (tid, fid) else {
                return Err(ApiError::Api {
                    status: 400,
                    body: i18n.t("common.invalid_ids"),
                });
            };
            let (_family, spouses) = futures_util::future::try_join(
                api.get_family(tid, fid),
                api.list_family_spouses(tid, fid),
            )
            .await?;
            let bundles = futures_util::future::try_join_all(spouses.iter().map(|spouse| {
                let api = api.clone();
                let pid = spouse.person_id;
                async move {
                    api.get_person_detail_bundle(tid, pid)
                        .await
                        .map(|bundle| (pid, Arc::new(bundle)))
                }
            }))
            .await?;
            Ok(Arc::new(CoupleData {
                spouses,
                bundles: bundles.into_iter().collect(),
            }))
        }
    });

    let sides = use_memo(move || match &*couple_resource.read() {
        Some(Ok(data)) => couple_sides(&data.spouses, |id| data.sex_of(id)),
        _ => (None, None),
    });
    let left_id = use_memo(move || sides().0);
    let right_id = use_memo(move || sides().1);

    // Deleting the couple from its edit modal leaves nothing to show: go back
    // to the tree, on whichever spouse was on screen.
    let mut last_person = use_signal(|| None::<Uuid>);
    use_effect(move || {
        if let Some(person) = left_id().or(right_id()) {
            last_person.set(Some(person));
        }
    });
    use_effect({
        let tree_id = tree_id.clone();
        move || {
            if let Some(Err(ApiError::Api { status: 404, .. })) = &*couple_resource.read()
                && let Some(person) = *last_person.peek()
            {
                nav.replace(Route::TreeDetail {
                    tree_id: tree_id.clone(),
                    person: Some(person.to_string()),
                });
            }
        }
    });

    let tree_resource = use_tree_resource(
        load_trace.clone(),
        api.clone(),
        tree_id_parsed,
        refresh,
        i18n,
    );
    let sosa_ancestors_resource = use_sosa_ancestors(
        load_trace.clone(),
        api.clone(),
        tree_id_parsed,
        tree_resource,
    );

    let api_photos = api.clone();
    let photos_resource = use_traced_resource(load_trace.clone(), "portraits", move || {
        let api = api_photos.clone();
        let tid = tree_id_parsed();
        let _ = media_revision();
        let ids: Vec<Uuid> = [left_id(), right_id()].into_iter().flatten().collect();
        async move {
            match tid {
                Some(tid) if !ids.is_empty() => api.portrait_map_for_ids(tid, &ids).await,
                _ => HashMap::new(),
            }
        }
    });

    let left_notes = use_notes(
        load_trace.clone(),
        &api,
        tree_id_parsed,
        refresh,
        NotesOf::Person(left_id),
    );
    let right_notes = use_notes(
        load_trace.clone(),
        &api,
        tree_id_parsed,
        refresh,
        NotesOf::Person(right_id),
    );
    let couple_notes = use_notes(
        load_trace.clone(),
        &api,
        tree_id_parsed,
        refresh,
        NotesOf::Family(family_id_parsed),
    );

    let left_pedigree = use_ancestor_pedigree(
        load_trace.clone(),
        api.clone(),
        tree_id_parsed,
        left_id.into(),
        i18n,
    );
    let right_pedigree = use_ancestor_pedigree(
        load_trace.clone(),
        api.clone(),
        tree_id_parsed,
        right_id.into(),
        i18n,
    );
    let left_mini = use_mini_pedigree(left_pedigree, photos_resource);
    let right_mini = use_mini_pedigree(right_pedigree, photos_resource);

    let left_profile = use_side_profile(couple_resource, left_id, left_pedigree, i18n);
    let right_profile = use_side_profile(couple_resource, right_id, right_pedigree, i18n);

    // ── Render ────────────────────────────────────────────────────────

    let tree_name_str = match &*tree_resource.read() {
        Some(Ok(tree)) => tree.name.clone(),
        _ => tree_id_parsed()
            .and_then(|tid| tree_cache.tree(tid))
            .map(|t| t.name)
            .unwrap_or_default(),
    };
    let self_person_id = match &*tree_resource.read() {
        Some(Ok(tree)) => tree.self_person_id,
        _ => None,
    };

    let (loaded, load_error) = match &*couple_resource.read() {
        Some(Ok(_)) => (true, None),
        Some(Err(error)) => (false, Some(error.to_string())),
        None => (false, None),
    };
    let left = left_profile();
    let right = right_profile();
    let unknown = i18n.t("couple.unknown_spouse");
    let side_name = |profile: &Option<SharedProfile>| {
        profile
            .as_ref()
            .map_or_else(|| unknown.clone(), |p| p.name.display_name.clone())
    };
    let title = if loaded {
        i18n.t_args(
            "couple.title",
            &[("left", &side_name(&left)), ("right", &side_name(&right))],
        )
    } else {
        String::new()
    };

    let sosa_ancestors = sosa_ancestors_resource.read().clone().unwrap_or_default();
    let ctx = tree_id_parsed().map(|tree_id| SectionContext {
        i18n,
        tree_id,
        sosa_ancestors: &sosa_ancestors,
        media_revision,
    });
    let selected_person_id = left_id().or(right_id());
    use_track_current_person(tree_id_parsed(), selected_person_id);
    let photo_of = |person_id: Uuid| {
        photos_resource
            .read()
            .as_ref()
            .and_then(|photos| photos.get(&person_id).cloned())
    };

    rsx! {
        div { class: "sub-page",
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
                span { class: "td-bc-current", "{title}" }
            }
            TopbarSearch { tree_id: tree_id.clone(), from_person: true }
        }

        div { class: "pd-page-shell",
        TreeIconSidebar {
            active_view: TreeSidebarView::Couple,
            selected_person_id,
            couple_family_id: family_id_parsed(),
            on_couple_view: move |_| {},
            on_profile_view: {
                let tree_id = tree_id.clone();
                move |pid: Option<Uuid>| {
                    if let Some(pid) = pid {
                        nav.push(Route::PersonDetail {
                            tree_id: tree_id.clone(),
                            person_id: pid.to_string(),
                        });
                    }
                }
            },
            on_pedigree_view: {
                let tree_id = tree_id.clone();
                move |pid: Option<Uuid>| {
                    nav.push(Route::TreeDetail {
                        tree_id: tree_id.clone(),
                        person: pid.map(|pid| pid.to_string()),
                    });
                }
            },
            on_add_person: move |_| show_create_person.set(true),
            on_settings: {
                let tree_id = tree_id.clone();
                move |_| {
                    nav.push(Route::Settings { tree_id: tree_id.clone() });
                }
            },
            on_dictionary: {
                let tree_id = tree_id.clone();
                move |_| {
                    nav.push(Route::Dictionary { tree_id: tree_id.clone() });
                }
            },
        }

        div { class: "sub-page-content pd-content cp-content",

        // The existing couple edit modal: both spouses, the union's events,
        // its children (detachable), its media — and deleting the couple.
        if show_edit_couple() {
            if let (Some(tid), Some(fid)) = (tree_id_parsed(), family_id_parsed()) {
                UnionForm {
                    tree_id: tid,
                    family_id: fid,
                    on_close: move |_| show_edit_couple.set(false),
                    on_saved: move |_| {
                        tree_cache.invalidate();
                        refresh += 1;
                    },
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

        match (loaded, ctx.as_ref(), family_id_parsed()) {
            (true, Some(ctx), Some(fid)) => {
                let anchor = left.as_ref().or(right.as_ref());
                let union = anchor.and_then(|profile| {
                    profile
                        .family
                        .unions
                        .iter()
                        .find(|union| union.family_id == fid)
                        .map(|union| (profile, union))
                });
                // What the couple shares: the union's own events and its
                // children's, told once rather than from each spouse's side.
                let couple_events: Vec<EnrichedEvent> = anchor
                    .map(|profile| {
                        profile
                            .events
                            .iter()
                            .filter(|entry| entry.union_id == Some(fid))
                            .map(|entry| {
                                let mut entry = entry.clone();
                                if entry.origin == EventOrigin::ConjugalFamily {
                                    entry.context = None;
                                }
                                entry
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let couple_event_refs: Vec<&EnrichedEvent> = couple_events.iter().collect();
                let has_person_notes = shows_notes(&left_notes) || shows_notes(&right_notes);
                let open_profile = |profile: &Profile| {
                    let tree_id = tree_id.clone();
                    let person_id = profile.person_id;
                    open_profile_button(&i18n, move || {
                        nav.push(Route::PersonDetail {
                            tree_id: tree_id.clone(),
                            person_id: person_id.to_string(),
                        });
                    })
                };
                let on_self_badge = {
                    let tree_id = tree_id.clone();
                    EventHandler::new(move |()| {
                        nav.push(Route::Settings { tree_id: tree_id.clone() });
                    })
                };
                let on_navigate = {
                    let tree_id = tree_id.clone();
                    EventHandler::new(move |pid: Uuid| {
                        nav.push(Route::PersonDetail { tree_id: tree_id.clone(), person_id: pid.to_string() });
                    })
                };
                let columns = [
                    (left_id(), &left, left_notes, left_pedigree, left_mini()),
                    (right_id(), &right, right_notes, right_pedigree, right_mini()),
                ];

                rsx! {
                    div { class: "cp-actions",
                        button {
                            class: "btn btn-outline pd-header-action-btn",
                            title: i18n.t("couple.edit"),
                            aria_label: i18n.t("couple.edit"),
                            onclick: move |_| show_edit_couple.set(true),
                            svg {
                                class: "pd-header-action-icon",
                                width: "16", height: "16", fill: "none", "viewBox": "0 0 24 24",
                                stroke: "currentColor", "strokeWidth": "2",
                                path { d: "M12 20h9" }
                                path { d: "M16.5 3.5a2.12 2.12 0 0 1 3 3L7 19l-4 1 1-4Z" }
                            }
                            span { class: "pd-header-action-label", {i18n.t("couple.edit")} }
                        }
                        if SHOW_MANUAL_REFRESH {
                            {refresh_button(&i18n, move || refresh += 1)}
                        }
                    }

                    div { class: "card cp-bar",
                        {spouse_select(&i18n, nav, &tree_id, fid, right.as_deref(), side_name(&left))}
                        span { class: "cp-ring", aria_hidden: "true", "\u{26AD}" }
                        {spouse_select(&i18n, nav, &tree_id, fid, left.as_deref(), side_name(&right))}
                    }

                    div { class: "cp-grid",
                        // ── Identity ──
                        for (person_id, profile, ..) in columns.iter() {
                            div { class: "cp-cell",
                                match (person_id, profile) {
                                    (Some(_), Some(profile)) => header_section(
                                        ctx,
                                        profile,
                                        photo_of(profile.person_id),
                                        self_person_id == Some(profile.person_id),
                                        on_self_badge,
                                        open_profile(profile),
                                    ),
                                    (Some(_), None) => rsx! {
                                        div { class: "card page-header loading", {i18n.t("couple.loading")} }
                                    },
                                    (None, _) => rsx! {
                                        div { class: "card page-header cp-unknown",
                                            p { class: "text-muted", "{unknown}" }
                                        }
                                    },
                                }
                            }
                        }

                        // ── Notes ──
                        if shows_notes(&couple_notes) {
                            div { class: "cp-span",
                                {notes_section(&i18n, "couple.notes_section", couple_notes.read().as_ref().and_then(Option::as_ref))}
                            }
                        }
                        if has_person_notes {
                            for (_, _, notes, ..) in columns.iter() {
                                div { class: "cp-cell",
                                    {notes_section(&i18n, "person.notes_section", notes.read().as_ref().and_then(Option::as_ref))}
                                }
                            }
                        }

                        // ── Media ──
                        div { class: "cp-span",
                            ProfileMediaCard {
                                tree_id: ctx.tree_id,
                                owner: MediaOwner::Family(fid),
                                title: i18n.t("couple.media_section"),
                                event_links: media_event_links(couple_events.iter(), &i18n),
                                revision: media_revision(),
                                on_changed: move |()| media_revision += 1,
                            }
                        }
                        for (_, profile, ..) in columns.iter() {
                            div { class: "cp-cell",
                                if let Some(profile) = profile {
                                    ProfileMediaCard {
                                        key: "{profile.person_id}",
                                        tree_id: ctx.tree_id,
                                        owner: MediaOwner::Person(profile.person_id),
                                        title: i18n.t("media.section"),
                                        event_links: media_event_links(
                                            profile.events.iter().filter(|entry| entry.union_id != Some(fid)),
                                            &i18n,
                                        ),
                                        preloaded_tiles: Some(profile.profile_tiles(false)),
                                        preloaded_bundle: Some(Arc::clone(&profile.bundle.gallery)),
                                        preloaded_portrait: profile.person.as_ref().map(|person| (
                                            person.portrait_media_id,
                                            person.portrait_vignette_id,
                                        )),
                                        preloaded_vignettes: Some(profile.bundle.profile_vignettes.clone()),
                                        revision: media_revision(),
                                        on_changed: move |()| media_revision += 1,
                                    }
                                }
                            }
                        }

                        // ── Family ──
                        if let Some((profile, union)) = union {
                            div { class: "cp-span",
                                div { class: "card pd-family-card",
                                    h2 { style: "font-size: 1.1rem; margin-bottom: 12px;", {i18n.t("couple.union_section")} }
                                    p { class: "pd-union-line", {union_line(ctx, profile, union, false)} }
                                    {children_list(ctx, profile, &union.child_ids)}
                                }
                            }
                        }
                        for (_, profile, ..) in columns.iter() {
                            div { class: "cp-cell",
                                if let Some(profile) = profile {
                                    {family_section(ctx, profile, Some(fid))}
                                }
                            }
                        }

                        // ── Events ──
                        if let Some(profile) = anchor {
                            div { class: "cp-span",
                                {timeline_section(ctx, profile, i18n.t("couple.events_section"), &couple_event_refs)}
                            }
                        }
                        for (_, profile, ..) in columns.iter() {
                            div { class: "cp-cell",
                                if let Some(profile) = profile {
                                    {
                                        let own: Vec<&EnrichedEvent> = profile
                                            .events
                                            .iter()
                                            .filter(|entry| entry.union_id != Some(fid))
                                            .collect();
                                        timeline_section(ctx, profile, i18n.t("person.events_section"), &own)
                                    }
                                }
                            }
                        }

                        // ── Ancestors ──
                        for (person_id, _, _, pedigree, mini) in columns.iter() {
                            div { class: "cp-cell",
                                if person_id.is_some() {
                                    {ancestors_section(&i18n, pedigree, mini.clone(), on_navigate)}
                                }
                            }
                        }
                    }
                }
            }
            _ => match load_error {
                Some(error) => rsx! {
                    div { class: "error-msg", {i18n.t_args("couple.load_error", &[("error", &error)])} }
                },
                None => rsx! {
                    div { class: "loading", {i18n.t("couple.loading")} }
                },
            },
        }
        } // close sub-page-content
        } // close pd-page-shell
        } // close sub-page
    }
}

/// Whose notes a notes resource loads.
enum NotesOf {
    Person(Memo<Option<Uuid>>),
    Family(Signal<Option<Uuid>>),
}

/// One notes list — `None` while there is no one to load it for.
fn use_notes(
    load_trace: UiLoadTrace,
    api: &ApiClient,
    tree_id: Signal<Option<Uuid>>,
    refresh: Signal<u32>,
    of: NotesOf,
) -> Resource<Option<Result<Vec<Note>, ApiError>>> {
    let api = api.clone();
    use_traced_resource(load_trace, "notes", move || {
        let api = api.clone();
        let _tick = refresh();
        let tid = tree_id();
        let (person, family) = match &of {
            NotesOf::Person(person) => (person(), None),
            NotesOf::Family(family) => (None, family()),
        };
        async move {
            let tid = tid?;
            if person.is_none() && family.is_none() {
                return None;
            }
            Some(api.list_notes(tid, person, None, family, None, None).await)
        }
    })
}

/// Whether a notes card has anything to say: notes, or why they failed.
fn shows_notes(notes: &Resource<Option<Result<Vec<Note>, ApiError>>>) -> bool {
    match &*notes.read() {
        Some(Some(Ok(notes))) => !notes.is_empty(),
        Some(Some(Err(_))) => true,
        _ => false,
    }
}

/// One spouse's profile, derived from the couple's load and their pedigree.
fn use_side_profile(
    couple: Resource<Result<Arc<CoupleData>, ApiError>>,
    person_id: Memo<Option<Uuid>>,
    pedigree: Resource<Result<Option<Pedigree>, ApiError>>,
    i18n: I18n,
) -> Memo<Option<SharedProfile>> {
    use_memo(move || {
        let pid = person_id()?;
        let data = couple.read();
        let Some(Ok(data)) = &*data else {
            return None;
        };
        let bundle = Arc::clone(data.bundles.get(&pid)?);
        let pedigree = pedigree.read();
        let pedigree = match &*pedigree {
            Some(Ok(Some(pedigree))) => Some(pedigree),
            _ => None,
        };
        Some(SharedProfile::new(build_profile(
            bundle, pid, pedigree, &i18n,
        )))
    })
}

/// A spouse selector: the couples of `owner` — the spouse on the other side —
/// each named by the partner it would put on this side. Choosing one opens
/// that couple. Without an owner (an unknown spouse) it only names `current`.
fn spouse_select(
    i18n: &I18n,
    nav: dioxus::router::Navigator,
    tree_id: &str,
    family_id: Uuid,
    owner: Option<&Profile>,
    current: String,
) -> Element {
    let options: Vec<(Uuid, String)> = match owner {
        Some(owner) => owner
            .couples()
            .map(|union| {
                let names = union
                    .partner_ids
                    .iter()
                    .map(|pid| owner.name_of(*pid, i18n))
                    .collect::<Vec<_>>()
                    .join(" & ");
                let label = match &union.marriage_date {
                    Some(date) => format!("{names} \u{2014} {date}"),
                    None => names,
                };
                (union.family_id, label)
            })
            .collect(),
        None => Vec::new(),
    };
    let options = if options.iter().any(|(id, _)| *id == family_id) {
        options
    } else {
        vec![(family_id, current)]
    };
    let aria_label = owner
        .map(|owner| i18n.t_args("couple.spouses_of", &[("name", &owner.name.display_name)]))
        .unwrap_or_default();
    let tree_id = tree_id.to_string();
    let single = options.len() < 2;
    rsx! {
        select {
            class: "cp-select",
            aria_label: "{aria_label}",
            disabled: single,
            onchange: move |evt| {
                if let Ok(family_id) = evt.value().parse::<Uuid>() {
                    nav.replace(Route::CoupleDetail {
                        tree_id: tree_id.clone(),
                        family_id: family_id.to_string(),
                    });
                }
            },
            for (id, label) in options {
                option { key: "{id}", value: "{id}", selected: id == family_id, "{label}" }
            }
        }
    }
}

/// A spouse header's link to their own profile.
fn open_profile_button(i18n: &I18n, mut on_open: impl FnMut() + 'static) -> Element {
    rsx! {
        button {
            class: "btn btn-outline pd-header-action-btn",
            title: i18n.t("couple.open_profile"),
            aria_label: i18n.t("couple.open_profile"),
            onclick: move |_| on_open(),
            svg {
                class: "pd-header-action-icon",
                width: "16", height: "16", fill: "none", "viewBox": "0 0 24 24",
                stroke: "currentColor", "strokeWidth": "2",
                circle { cx: "12", cy: "8", r: "4" }
                path { d: "M4 21v-1a6 6 0 0 1 12 0v1" }
            }
            span { class: "pd-header-action-label", {i18n.t("couple.open_profile")} }
        }
    }
}
