//! Modal-based union/family edit form.
//!
//! Allows editing a family's marriage event (date, place, description),
//! managing spouse roles, viewing and removing children, and adding
//! new children via search or creation.

use dioxus::prelude::*;
use uuid::Uuid;

use crate::api::{
    AddChildBody, AddSpouseBody, ApiClient, CreateEventBody, CreatePersonBody, UpdateEventBody,
};
use crate::components::search_person::SearchPerson;
use crate::utils::{opt_str, resolve_name};
use oxidgene_core::{ChildType, EventType, Sex, SpouseRole};

// ── Props ────────────────────────────────────────────────────────────────

#[derive(Props, Clone, PartialEq)]
pub struct UnionFormProps {
    /// Tree ID.
    pub tree_id: Uuid,
    /// Family ID to edit.
    pub family_id: Uuid,
    /// Called when the form is closed.
    pub on_close: EventHandler<()>,
    /// Called when data is saved (so parent can refresh).
    pub on_saved: EventHandler<()>,
}

// ── Component ────────────────────────────────────────────────────────────

/// Modal union/family edit form.
#[component]
pub fn UnionForm(props: UnionFormProps) -> Element {
    let api = use_context::<ApiClient>();
    let mut refresh = use_signal(|| 0u32);

    let tid = props.tree_id;
    let fid = props.family_id;

    // ── State ──
    let mut save_error = use_signal(|| None::<String>);

    // Marriage event state.
    let mut marriage_date = use_signal(String::new);
    let mut marriage_place_id = use_signal(String::new);
    let mut marriage_desc = use_signal(String::new);
    let mut marriage_event_id = use_signal(|| None::<Uuid>);
    let mut marriage_loaded = use_signal(|| false);

    // Add child linking mode.
    let mut show_add_child = use_signal(|| false);
    let mut show_add_spouse = use_signal(|| false);

    // ── Resources ──

    // Spouses
    let api_spouses = api.clone();
    let spouses_resource = use_resource(move || {
        let api = api_spouses.clone();
        let _tick = refresh();
        async move { api.list_family_spouses(tid, fid).await }
    });

    // Children
    let api_children = api.clone();
    let children_resource = use_resource(move || {
        let api = api_children.clone();
        let _tick = refresh();
        async move { api.list_family_children(tid, fid).await }
    });

    // Events (for marriage)
    let api_events = api.clone();
    let events_resource = use_resource(move || {
        let api = api_events.clone();
        let _tick = refresh();
        async move {
            api.list_events(tid, Some(100), None, None, None, Some(fid))
                .await
        }
    });

    // Places (for picker)
    let api_places = api.clone();
    let places_resource = use_resource(move || {
        let api = api_places.clone();
        let _tick = refresh();
        async move { api.list_places(tid, Some(200), None, None).await }
    });

    // All persons + names (for display names)
    let api_persons = api.clone();
    let _persons_resource = use_resource(move || {
        let api = api_persons.clone();
        let _tick = refresh();
        async move { api.list_persons(tid, Some(500), None).await }
    });

    let api_names_res = api.clone();
    let names_resource = use_resource(move || {
        let api = api_names_res.clone();
        let _tick = refresh();
        async move {
            let persons = api.list_persons(tid, Some(500), None).await?;
            let mut name_map: std::collections::HashMap<
                Uuid,
                Vec<oxidgene_core::types::PersonName>,
            > = std::collections::HashMap::new();
            for edge in &persons.edges {
                if let Ok(names) = api.list_person_names(tid, edge.node.id).await {
                    name_map.insert(edge.node.id, names);
                }
            }
            Ok::<_, crate::api::ApiError>(name_map)
        }
    });

    // ── Populate marriage fields ──
    if !marriage_loaded()
        && let Some(Ok(conn)) = &*events_resource.read()
    {
        for edge in &conn.edges {
            let ev = &edge.node;
            if matches!(
                ev.event_type,
                EventType::Marriage
                    | EventType::Engagement
                    | EventType::MarriageBann
                    | EventType::MarriageContract
                    | EventType::MarriageLicense
                    | EventType::MarriageSettlement
            ) {
                marriage_event_id.set(Some(ev.id));
                marriage_date.set(ev.date_value.clone().unwrap_or_default());
                marriage_place_id.set(ev.place_id.map(|id| id.to_string()).unwrap_or_default());
                marriage_desc.set(ev.description.clone().unwrap_or_default());
                break;
            }
        }
        marriage_loaded.set(true);
    }

    // Place options.
    let place_options: Vec<(String, String)> = {
        let data = places_resource.read();
        match &*data {
            Some(Ok(conn)) => conn
                .edges
                .iter()
                .map(|e| (e.node.id.to_string(), e.node.name.clone()))
                .collect(),
            _ => vec![],
        }
    };

    // Person name resolver from loaded data.
    let name_map_for_display: std::collections::HashMap<
        Uuid,
        Vec<oxidgene_core::types::PersonName>,
    > = {
        let data = names_resource.read();
        match &*data {
            Some(Ok(map)) => map.clone(),
            _ => std::collections::HashMap::new(),
        }
    };

    // ── Handlers ──

    // Save marriage event.
    let api_save_marriage = api.clone();
    let on_saved_marriage = props.on_saved;
    let on_save_marriage = move |_| {
        let api = api_save_marriage.clone();
        let date = marriage_date().trim().to_string();
        let place_str = marriage_place_id();
        let desc = marriage_desc().trim().to_string();
        let existing_id = marriage_event_id();
        spawn(async move {
            let place_id = if place_str.is_empty() {
                None
            } else {
                place_str.parse::<Uuid>().ok()
            };
            if let Some(eid) = existing_id {
                let body = UpdateEventBody {
                    event_type: Some(EventType::Marriage),
                    date_value: Some(opt_str(&date)),
                    date_sort: None,
                    place_id: Some(place_id),
                    description: Some(opt_str(&desc)),
                };
                match api.update_event(tid, eid, &body).await {
                    Ok(_) => {
                        save_error.set(None);
                        on_saved_marriage.call(());
                        refresh += 1;
                    }
                    Err(e) => save_error.set(Some(format!("{e}"))),
                }
            } else {
                let body = CreateEventBody {
                    event_type: EventType::Marriage,
                    date_value: opt_str(&date),
                    date_sort: None,
                    place_id,
                    person_id: None,
                    family_id: Some(fid),
                    description: opt_str(&desc),
                };
                match api.create_event(tid, &body).await {
                    Ok(ev) => {
                        marriage_event_id.set(Some(ev.id));
                        save_error.set(None);
                        on_saved_marriage.call(());
                        refresh += 1;
                    }
                    Err(e) => save_error.set(Some(format!("{e}"))),
                }
            }
        });
    };

    // Remove spouse handler — cloned per iteration in rsx.
    let api_rm_spouse = api.clone();
    let on_saved_rm_spouse = props.on_saved;

    // Remove child handler — cloned per iteration in rsx.
    let api_rm_child = api.clone();
    let on_saved_rm_child = props.on_saved;

    // Add child by linking existing person.
    let api_add_child_link = api.clone();
    let on_saved_add_child = props.on_saved;
    let on_select_child = move |person_id: Uuid| {
        let api = api_add_child_link.clone();
        spawn(async move {
            let body = AddChildBody {
                person_id,
                child_type: ChildType::Biological,
                sort_order: 0,
            };
            match api.add_child(tid, fid, &body).await {
                Ok(_) => {
                    show_add_child.set(false);
                    save_error.set(None);
                    on_saved_add_child.call(());
                    refresh += 1;
                }
                Err(e) => save_error.set(Some(format!("{e}"))),
            }
        });
    };

    // Add child by creating new person.
    let api_add_child_new = api.clone();
    let on_saved_add_child_new = props.on_saved;
    let on_create_new_child = move |_| {
        let api = api_add_child_new.clone();
        spawn(async move {
            match api
                .create_person(tid, &CreatePersonBody { sex: Sex::Unknown })
                .await
            {
                Ok(new_person) => {
                    let body = AddChildBody {
                        person_id: new_person.id,
                        child_type: ChildType::Biological,
                        sort_order: 0,
                    };
                    let _ = api.add_child(tid, fid, &body).await;
                    show_add_child.set(false);
                    save_error.set(None);
                    on_saved_add_child_new.call(());
                    refresh += 1;
                }
                Err(e) => save_error.set(Some(format!("{e}"))),
            }
        });
    };

    // Add spouse by linking existing person.
    let api_add_spouse_link = api.clone();
    let on_saved_add_spouse = props.on_saved;
    let on_select_spouse = move |person_id: Uuid| {
        let api = api_add_spouse_link.clone();
        spawn(async move {
            let body = AddSpouseBody {
                person_id,
                role: SpouseRole::Partner,
                sort_order: 0,
            };
            match api.add_spouse(tid, fid, &body).await {
                Ok(_) => {
                    show_add_spouse.set(false);
                    save_error.set(None);
                    on_saved_add_spouse.call(());
                    refresh += 1;
                }
                Err(e) => save_error.set(Some(format!("{e}"))),
            }
        });
    };

    // Add spouse by creating new person.
    let api_add_spouse_new = api.clone();
    let on_saved_add_spouse_new = props.on_saved;
    let on_create_new_spouse = move |_| {
        let api = api_add_spouse_new.clone();
        spawn(async move {
            match api
                .create_person(tid, &CreatePersonBody { sex: Sex::Unknown })
                .await
            {
                Ok(new_person) => {
                    let body = AddSpouseBody {
                        person_id: new_person.id,
                        role: SpouseRole::Partner,
                        sort_order: 0,
                    };
                    let _ = api.add_spouse(tid, fid, &body).await;
                    show_add_spouse.set(false);
                    save_error.set(None);
                    on_saved_add_spouse_new.call(());
                    refresh += 1;
                }
                Err(e) => save_error.set(Some(format!("{e}"))),
            }
        });
    };

    // ── Render ──

    rsx! {
        div { class: "modal-backdrop union-form-backdrop",
            onclick: move |_| props.on_close.call(()),

            div { class: "union-form-modal",
                onclick: move |evt| evt.stop_propagation(),

                // Header
                div { class: "union-form-header",
                    h2 { "Edit Union" }
                    button {
                        class: "person-form-close",
                        onclick: move |_| props.on_close.call(()),
                        "x"
                    }
                }

                if let Some(err) = save_error() {
                    div { class: "error-msg", style: "margin: 0 16px;", "{err}" }
                }

                div { class: "union-form-body",
                    // ── Spouses section ──
                    div { class: "union-form-section",
                        div { class: "section-header",
                            h3 { style: "font-size: 0.95rem;", "Spouses" }
                            button {
                                class: "btn btn-primary btn-sm",
                                onclick: move |_| {
                                    show_add_spouse.toggle();
                                    show_add_child.set(false);
                                },
                                if show_add_spouse() { "Cancel" } else { "Add Spouse" }
                            }
                        }

                        if show_add_spouse() {
                            div { class: "linking-panel",
                                p { class: "linking-panel-title", "Link existing person or create new:" }
                                SearchPerson {
                                    tree_id: tid,
                                    placeholder: "Search for spouse...",
                                    on_select: on_select_spouse,
                                    on_cancel: move |_| show_add_spouse.set(false),
                                }
                                div { class: "linking-panel-or", "— or —" }
                                button {
                                    class: "btn btn-outline",
                                    onclick: on_create_new_spouse,
                                    "Create New Person"
                                }
                            }
                        }

                        match &*spouses_resource.read() {
                            Some(Ok(spouses)) => rsx! {
                                if spouses.is_empty() {
                                    div { class: "empty-state",
                                        p { "No spouses in this union." }
                                    }
                                } else {
                                    for spouse in spouses.iter() {
                                        {
                                            let sid = spouse.person_id;
                                            let role = format!("{:?}", spouse.role);
                                            let name = resolve_name(sid, &name_map_for_display);
                                            rsx! {
                                                div { class: "person-form-item",
                                                    div { class: "person-form-item-info",
                                                        span { class: "badge", "{role}" }
                                                        strong { "{name}" }
                                                    }
                                                    div { class: "person-form-item-actions",
                                                        button {
                                                            class: "btn btn-danger btn-sm",
                                                            onclick: {
                                                                let api = api_rm_spouse.clone();
                                                                move |_| {
                                                                    let api = api.clone();
                                                                    spawn(async move {
                                                                        match api.remove_spouse(tid, fid, sid).await {
                                                                            Ok(_) => {
                                                                                on_saved_rm_spouse.call(());
                                                                                refresh += 1;
                                                                            }
                                                                            Err(e) => save_error.set(Some(format!("{e}"))),
                                                                        }
                                                                    });
                                                                }
                                                            },
                                                            "Remove"
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            },
                            Some(Err(e)) => rsx! {
                                div { class: "error-msg", "Failed to load spouses: {e}" }
                            },
                            None => rsx! {
                                div { class: "loading", "Loading spouses..." }
                            },
                        }
                    }

                    // ── Marriage event section ──
                    div { class: "union-form-section",
                        h3 { style: "font-size: 0.95rem; margin-bottom: 12px;", "Marriage / Union Event" }
                        div { class: "form-group",
                            label { "Date" }
                            input {
                                r#type: "text",
                                placeholder: "e.g. 15 Jun 1920",
                                value: "{marriage_date}",
                                oninput: move |e: Event<FormData>| marriage_date.set(e.value()),
                            }
                        }
                        div { class: "form-group",
                            label { "Place" }
                            select {
                                value: "{marriage_place_id}",
                                oninput: move |e: Event<FormData>| marriage_place_id.set(e.value()),
                                option { value: "", "-- No place --" }
                                for (pid, pname) in place_options.iter() {
                                    option { value: "{pid}", "{pname}" }
                                }
                            }
                        }
                        div { class: "form-group",
                            label { "Description" }
                            input {
                                r#type: "text",
                                placeholder: "Optional description",
                                value: "{marriage_desc}",
                                oninput: move |e: Event<FormData>| marriage_desc.set(e.value()),
                            }
                        }
                        button {
                            class: "btn btn-primary",
                            onclick: on_save_marriage,
                            if marriage_event_id().is_some() { "Update Marriage" } else { "Save Marriage" }
                        }
                    }

                    // ── Children section ──
                    div { class: "union-form-section",
                        div { class: "section-header",
                            h3 { style: "font-size: 0.95rem;", "Children" }
                            button {
                                class: "btn btn-primary btn-sm",
                                onclick: move |_| {
                                    show_add_child.toggle();
                                    show_add_spouse.set(false);
                                },
                                if show_add_child() { "Cancel" } else { "Add Child" }
                            }
                        }

                        if show_add_child() {
                            div { class: "linking-panel",
                                p { class: "linking-panel-title", "Link existing person or create new:" }
                                SearchPerson {
                                    tree_id: tid,
                                    placeholder: "Search for child...",
                                    on_select: on_select_child,
                                    on_cancel: move |_| show_add_child.set(false),
                                }
                                div { class: "linking-panel-or", "— or —" }
                                button {
                                    class: "btn btn-outline",
                                    onclick: on_create_new_child,
                                    "Create New Person"
                                }
                            }
                        }

                        match &*children_resource.read() {
                            Some(Ok(children)) => rsx! {
                                if children.is_empty() {
                                    div { class: "empty-state",
                                        p { "No children in this union." }
                                    }
                                } else {
                                    for child in children.iter() {
                                        {
                                            let cid = child.person_id;
                                            let ct = format!("{:?}", child.child_type);
                                            let name = resolve_name(cid, &name_map_for_display);
                                            rsx! {
                                                div { class: "person-form-item",
                                                    div { class: "person-form-item-info",
                                                        span { class: "badge", "{ct}" }
                                                        strong { "{name}" }
                                                    }
                                                    div { class: "person-form-item-actions",
                                                        button {
                                                            class: "btn btn-danger btn-sm",
                                                            onclick: {
                                                                let api = api_rm_child.clone();
                                                                move |_| {
                                                                    let api = api.clone();
                                                                    spawn(async move {
                                                                        match api.remove_child(tid, fid, cid).await {
                                                                            Ok(_) => {
                                                                                on_saved_rm_child.call(());
                                                                                refresh += 1;
                                                                            }
                                                                            Err(e) => save_error.set(Some(format!("{e}"))),
                                                                        }
                                                                    });
                                                                }
                                                            },
                                                            "Remove"
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            },
                            Some(Err(e)) => rsx! {
                                div { class: "error-msg", "Failed to load children: {e}" }
                            },
                            None => rsx! {
                                div { class: "loading", "Loading children..." }
                            },
                        }
                    }
                }
            }
        }
    }
}
