//! Person detail page — shows names, events, notes, citations, and ancestry charts with full CRUD.

use std::collections::HashMap;

use dioxus::prelude::*;
use uuid::Uuid;

use crate::api::ApiClient;
use crate::components::confirm_dialog::ConfirmDialog;
use crate::components::media_gallery::MediaOwner;
use crate::components::merge_dialog::MergeDialog;
use crate::components::person_form::{PersonForm, PersonFormCreateContext};
use crate::components::person_profile::{
    ProfileMediaCard, SHOW_MANUAL_REFRESH, SectionContext, SharedProfile, ancestors_section,
    build_profile, family_section, header_section, media_event_links, notes_section,
    refresh_button, timeline_placeholder, timeline_section, use_ancestor_pedigree,
    use_mini_pedigree, use_sosa_ancestors, use_tree_resource,
};
use crate::components::print::PrintHeading;
use crate::components::topbar_search::TopbarSearch;
use crate::components::tree_cache::{use_track_current_person, use_tree_cache};
use crate::components::tree_icon_sidebar::{TreeIconSidebar, TreeSidebarView};
use crate::i18n::use_i18n;
use crate::router::Route;
use crate::ui_observability::{UiPage, use_traced_resource, use_ui_load_trace};

/// Page rendered at `/trees/:tree_id/persons/:person_id`.
#[component]
pub fn PersonDetail(tree_id: String, person_id: String) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let nav = use_navigator();
    let tree_cache = use_tree_cache();
    let load_trace = use_ui_load_trace(UiPage::PersonDetail);
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
    use_track_current_person(new_tid, new_pid);

    // The router reuses this component instance across navigations (e.g.
    // clicking a relative in the mini pedigree or family section), so the
    // scroll position from the previous person otherwise carries over.
    use_effect(move || {
        person_id_parsed();
        document::eval(
            "document.querySelector('.sub-page-content')?.scrollTo({ top: 0, behavior: 'instant' });",
        );
    });

    // Delete confirmation state.
    let mut confirm_delete = use_signal(|| false);
    // "Merge with…" is open.
    let mut merging = use_signal(|| false);
    let mut delete_error = use_signal(|| None::<String>);

    // Person edit modal (names are managed there — see PersonForm).
    let mut show_edit_person = use_signal(|| false);
    let mut show_create_person = use_signal(|| false);
    let mut media_revision = use_signal(|| 0_u32);

    // ── Resources ────────────────────────────────────────────────────

    // Everything specific to this page, limited to the person's visible
    // family neighborhood and the evidence attached to its events.
    let api_detail = api.clone();
    let detail_resource = use_traced_resource(load_trace.clone(), "person_detail", move || {
        let api = api_detail.clone();
        let _tick = refresh();
        let _media_tick = media_revision();
        let tid = tree_id_parsed();
        let pid = person_id_parsed();
        async move {
            let (Some(tid), Some(pid)) = (tid, pid) else {
                return Err(crate::api::ApiError::invalid_ids(&i18n));
            };
            // Shared, not owned: the render reads the bundle on every pass and
            // it carries every thumbnail as a base64 data URI, so handing out
            // owned copies dominated the page's frame time.
            api.get_person_detail_bundle(tid, pid)
                .await
                .map(std::sync::Arc::new)
        }
    });

    // Fetch notes for this person.
    let api_notes = api.clone();
    let notes_resource = use_traced_resource(load_trace.clone(), "notes", move || {
        let api = api_notes.clone();
        let _tick = refresh();
        let tid = tree_id_parsed();
        let pid = person_id_parsed();
        async move {
            let (Some(tid), Some(pid)) = (tid, pid) else {
                return Err(crate::api::ApiError::invalid_ids(&i18n));
            };
            api.list_notes(tid, Some(pid), None, None, None, None).await
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

    // This person's portrait, reused by the header and mini pedigree.
    let api_photos_map = api.clone();
    let photos_map_resource = use_traced_resource(load_trace.clone(), "portraits", move || {
        let api = api_photos_map.clone();
        let tid = tree_id_parsed();
        let _ = media_revision();
        async move {
            let (Some(tid), Some(person_id)) = (tid, person_id_parsed()) else {
                return HashMap::new();
            };
            api.portrait_map_for_ids(tid, &[person_id]).await
        }
    });

    let ancestor_pedigree_resource = use_ancestor_pedigree(
        load_trace.clone(),
        api.clone(),
        tree_id_parsed,
        person_id_parsed.into(),
        i18n,
    );
    let mini_pedigree = use_mini_pedigree(ancestor_pedigree_resource, photos_map_resource);

    // Everything the sections draw, derived once per load rather than on
    // every render — including the ones caused by opening a dialog.
    let profile = use_memo(move || {
        let Some(Ok(detail)) = &*detail_resource.read() else {
            return None;
        };
        let pid = person_id_parsed()?;
        let pedigree = ancestor_pedigree_resource.read();
        let pedigree = match &*pedigree {
            Some(Ok(Some(pedigree))) => Some(pedigree),
            _ => None,
        };
        Some(SharedProfile::new(build_profile(
            std::sync::Arc::clone(detail),
            pid,
            pedigree,
            &i18n,
        )))
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

    let detail_error = match &*detail_resource.read() {
        Some(Err(error)) => Some(error.to_string()),
        _ => None,
    };
    let profile = profile();
    // Blank while loading — better than flashing a loading label in the
    // breadcrumb and page header.
    let display_name = profile
        .as_ref()
        .map(|profile| profile.name.display_name.clone())
        .unwrap_or_default();

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

    let sosa_ancestors = sosa_ancestors_resource.read().clone().unwrap_or_default();
    let couple_family_id = profile
        .as_ref()
        .and_then(|profile| profile.default_couple_id());
    let ctx = tree_id_parsed().map(|tree_id| SectionContext {
        i18n,
        tree_id,
        sosa_ancestors: &sosa_ancestors,
        media_revision,
    });

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
            PrintHeading {
                tree_name: tree_name_str.clone(),
                title: display_name.clone(),
            }
        }

        div { class: "pd-page-shell",
        TreeIconSidebar {
            active_view: TreeSidebarView::Profile,
            selected_person_id: person_id_parsed(),
            couple_family_id,
            on_profile_view: move |_| {},
            on_couple_view: {
                let tree_id = tree_id.clone();
                move |family_id: Uuid| {
                    nav.push(Route::CoupleDetail {
                        tree_id: tree_id.clone(),
                        family_id: family_id.to_string(),
                    });
                }
            },
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
            on_dictionary: {
                let tree_id = tree_id.clone();
                move |_| {
                    nav.push(Route::Dictionary {
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
                    // This page's person was merged away: show the one kept.
                    on_merged: {
                        let tree_id = tree_id.clone();
                        move |kept: Uuid| {
                            tree_cache.invalidate();
                            nav.replace(Route::PersonDetail {
                                tree_id: tree_id.clone(),
                                person_id: kept.to_string(),
                            });
                        }
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

        // "Merge with…": the wizard, from the search for the other record.
        if let (true, Some(tid), Some(pid)) = (merging(), tree_id_parsed(), person_id_parsed()) {
            MergeDialog {
                tree_id: tid,
                person_id: pid,
                on_close: move |_| merging.set(false),
                on_merged: {
                    let tree_id = tree_id.clone();
                    move |kept: Uuid| {
                        merging.set(false);
                        tree_cache.invalidate();
                        if kept == pid {
                            refresh += 1;
                        } else {
                            nav.replace(Route::PersonDetail {
                                tree_id: tree_id.clone(),
                                person_id: kept.to_string(),
                            });
                        }
                    }
                },
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

        match (profile.as_ref(), ctx.as_ref()) {
            (Some(profile), Some(ctx)) => {
                let photo = photos_map_resource
                    .read()
                    .as_ref()
                    .and_then(|photos| photos.get(&profile.person_id).cloned());
                let is_self = matches!(
                    &*tree_resource.read(),
                    Some(Ok(tree)) if tree.self_person_id == Some(profile.person_id)
                );
                let events: Vec<_> = profile.events.iter().collect();
                let event_links = media_event_links(profile.events.iter(), &i18n);
                let on_navigate = {
                    let tid = tree_id.clone();
                    EventHandler::new(move |pid: Uuid| {
                        nav.push(Route::PersonDetail { tree_id: tid.clone(), person_id: pid.to_string() });
                    })
                };
                rsx! {
                    {header_section(
                        ctx,
                        profile,
                        photo,
                        is_self,
                        {
                            let tree_id = tree_id.clone();
                            EventHandler::new(move |()| {
                                nav.push(Route::Settings { tree_id: tree_id.clone() });
                            })
                        },
                        header_actions(
                            &i18n,
                            move || {
                                confirm_delete.set(true);
                                delete_error.set(None);
                            },
                            move || show_edit_person.set(true),
                            move || merging.set(true),
                            {
                                let tree_id = tree_id.clone();
                                let person_id = person_id.clone();
                                move || {
                                    nav.push(Route::PersonHistory {
                                        tree_id: tree_id.clone(),
                                        person_id: person_id.clone(),
                                    });
                                }
                            },
                            move || refresh += 1,
                        ),
                    )}
                    {notes_section(&i18n, "person.notes_section", notes_resource.read().as_ref())}
                    ProfileMediaCard {
                        tree_id: ctx.tree_id,
                        owner: MediaOwner::Person(profile.person_id),
                        title: i18n.t("media.section"),
                        related_family_ids: profile.union_family_ids(),
                        event_links,
                        preloaded_tiles: Some(profile.profile_tiles(true)),
                        preloaded_bundle: Some(std::sync::Arc::clone(&profile.bundle.gallery)),
                        preloaded_portrait: profile.person.as_ref().map(|person| (
                            person.portrait_media_id,
                            person.portrait_vignette_id,
                        )),
                        preloaded_vignettes: Some(profile.bundle.profile_vignettes.clone()),
                        revision: media_revision(),
                        on_changed: move |()| media_revision += 1,
                    }
                    {family_section(ctx, profile, None)}
                    {timeline_section(ctx, profile, i18n.t("person.events_section"), &events)}
                    {ancestors_section(&i18n, &ancestor_pedigree_resource, mini_pedigree(), on_navigate)}
                }
            }
            _ => rsx! {
                match detail_error.as_deref() {
                    Some(error) => rsx! {
                        div { class: "error-msg", {i18n.t_args("person.load_error", &[("error", error)])} }
                    },
                    None => rsx! {
                        div { class: "loading", {i18n.t("person.loading")} }
                    },
                }
                {timeline_placeholder(&i18n, detail_error.as_deref())}
            },
        }
        } // close sub-page-content
        } // close pd-page-shell
        } // close sub-page
    }
}

/// The person header's Delete, Edit, Merge with…, History and (on the web)
/// Refresh buttons.
fn header_actions(
    i18n: &crate::i18n::I18n,
    mut on_delete: impl FnMut() + 'static,
    mut on_edit: impl FnMut() + 'static,
    mut on_merge: impl FnMut() + 'static,
    mut on_history: impl FnMut() + 'static,
    on_refresh: impl FnMut() + 'static,
) -> Element {
    rsx! {
        button {
            class: "btn btn-danger pd-header-action-btn",
            title: i18n.t("common.delete"),
            aria_label: i18n.t("common.delete"),
            onclick: move |_| on_delete(),
            svg {
                class: "pd-header-action-icon",
                width: "16", height: "16", fill: "none", "viewBox": "0 0 24 24",
                stroke: "currentColor", "strokeWidth": "2",
                path { d: "M3 6h18" }
                path { d: "M8 6V4h8v2" }
                path { d: "M19 6l-1 14H6L5 6" }
            }
        }
        button {
            class: "btn btn-outline pd-header-action-btn",
            title: i18n.t("common.edit"),
            aria_label: i18n.t("common.edit"),
            onclick: move |_| on_edit(),
            svg {
                class: "pd-header-action-icon",
                width: "16", height: "16", fill: "none", "viewBox": "0 0 24 24",
                stroke: "currentColor", "strokeWidth": "2",
                path { d: "M12 20h9" }
                path { d: "M16.5 3.5a2.12 2.12 0 0 1 3 3L7 19l-4 1 1-4Z" }
            }
        }
        button {
            class: "btn btn-outline pd-header-action-btn",
            title: i18n.t("context.merge"),
            aria_label: i18n.t("context.merge"),
            onclick: move |_| on_merge(),
            svg {
                class: "pd-header-action-icon",
                width: "16", height: "16", fill: "none", "viewBox": "0 0 24 24",
                stroke: "currentColor", "strokeWidth": "2",
                path { d: "M6 3v6a6 6 0 0 0 6 6a6 6 0 0 0 6-6V3" }
                path { d: "M12 15v6" }
            }
        }
        button {
            class: "btn btn-outline pd-header-action-btn",
            title: i18n.t("history.button"),
            aria_label: i18n.t("history.button"),
            onclick: move |_| on_history(),
            svg {
                class: "pd-header-action-icon",
                width: "16", height: "16", fill: "none", "viewBox": "0 0 24 24",
                stroke: "currentColor", "strokeWidth": "2",
                path { d: "M3 12a9 9 0 1 0 3-6.7L3 8" }
                path { d: "M3 3v5h5" }
                path { d: "M12 7v5l3 3" }
            }
        }
        if SHOW_MANUAL_REFRESH {
            {refresh_button(i18n, on_refresh)}
        }
    }
}
