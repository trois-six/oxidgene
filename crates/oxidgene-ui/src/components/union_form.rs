//! Modal-based couple/family edit form (spec §16).
//!
//! Body is divided into: Union (events, date/place/note shorthand),
//! Children (with staged detach, applied on Save), Person 1 / Person 2
//! (collapsible, embedding the full person edit fields). Footer holds
//! Delete couple (removes the union only — persons remain in the tree)
//! plus Cancel / Save.

use std::collections::HashSet;

use dioxus::prelude::*;
use uuid::Uuid;

use crate::api::{AddChildBody, ApiClient, CreateEventBody, UpdateEventBody};
use crate::components::person_form::PersonForm;
use crate::components::search_person::SearchPerson;
use crate::i18n::use_i18n;
use crate::utils::{opt_str, resolve_name};
use oxidgene_core::{Calendar, ChildType, DateQualifier, EventType};

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

/// Modal couple/family edit form.
#[component]
pub fn UnionForm(props: UnionFormProps) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let mut refresh = use_signal(|| 0u32);

    let tid = props.tree_id;
    let fid = props.family_id;

    // ── State ──
    let mut save_error = use_signal(|| None::<String>);

    // Marriage event state (primary/first union event).
    let mut marriage_date = use_signal(String::new);
    let mut marriage_place_id = use_signal(String::new);
    let mut marriage_desc = use_signal(String::new);
    let mut marriage_event_id = use_signal(|| None::<Uuid>);
    let mut marriage_loaded = use_signal(|| false);

    // Add union event state.
    let mut show_add_union_event = use_signal(|| false);
    let mut new_union_type = use_signal(|| "Marriage".to_string());
    let mut new_union_date = use_signal(String::new);
    let mut new_union_place = use_signal(String::new);
    let mut new_union_desc = use_signal(String::new);

    // Add child linking mode.
    let mut show_add_child = use_signal(|| false);

    // Person block expand/collapse (collapsed by default).
    let mut show_person1 = use_signal(|| false);
    let mut show_person2 = use_signal(|| false);

    // Staged child detach (applied on Save).
    let mut pending_detach = use_signal(HashSet::<Uuid>::new);
    let mut confirm_detach_id = use_signal(|| None::<Uuid>);

    // Delete couple state.
    let mut show_delete_confirm = use_signal(|| false);
    let mut delete_error = use_signal(|| None::<String>);
    let mut deleting = use_signal(|| false);
    let mut saving = use_signal(|| false);

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

    // All union events (for display).
    let union_events: Vec<oxidgene_core::types::Event> = {
        let data = events_resource.read();
        match &*data {
            Some(Ok(conn)) => conn
                .edges
                .iter()
                .filter(|e| {
                    matches!(
                        e.node.event_type,
                        EventType::Marriage
                            | EventType::Divorce
                            | EventType::Annulment
                            | EventType::Engagement
                            | EventType::MarriageBann
                            | EventType::MarriageContract
                            | EventType::MarriageLicense
                            | EventType::MarriageSettlement
                            | EventType::CivilUnion
                            | EventType::Separation
                            | EventType::DivorceFiled
                            | EventType::Residence
                            | EventType::Census
                            | EventType::Emigration
                            | EventType::Immigration
                            | EventType::Will
                            | EventType::Probate
                            | EventType::Other
                    )
                })
                .map(|e| e.node.clone())
                .collect(),
            _ => vec![],
        }
    };

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

    // Spouses sorted by sort_order — drives the header title and Person 1/2 blocks.
    let spouses_sorted: Vec<oxidgene_core::types::FamilySpouse> = {
        let data = spouses_resource.read();
        match &*data {
            Some(Ok(spouses)) => {
                let mut v = spouses.clone();
                v.sort_by_key(|s| s.sort_order);
                v
            }
            _ => vec![],
        }
    };
    let spouse1 = spouses_sorted.first().cloned();
    let spouse2 = spouses_sorted.get(1).cloned();

    let couple_title: String = match (&spouse1, &spouse2) {
        (Some(s1), Some(s2)) => format!(
            "{} & {}",
            resolve_name(s1.person_id, &name_map_for_display),
            resolve_name(s2.person_id, &name_map_for_display)
        ),
        (Some(s1), None) => resolve_name(s1.person_id, &name_map_for_display),
        _ => i18n.t("union_form.title"),
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
                    date_qualifier: None,
                    date_value2: None,
                    calendar: None,
                    witnesses: None,
                    cause: None,
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
                    date_qualifier: DateQualifier::default(),
                    date_value2: None,
                    calendar: Calendar::default(),
                    witnesses: vec![],
                    cause: None,
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

    // Create new union event handler.
    let api_create_union = api.clone();
    let on_saved_create_union = props.on_saved;
    let on_create_union_event = move |_| {
        let api = api_create_union.clone();
        let evt_type_str = new_union_type();
        let date = new_union_date().trim().to_string();
        let place_str = new_union_place();
        let desc = new_union_desc().trim().to_string();
        spawn(async move {
            let event_type = crate::utils::parse_event_type(&evt_type_str);
            let place_id = if place_str.is_empty() {
                None
            } else {
                place_str.parse::<Uuid>().ok()
            };
            let body = CreateEventBody {
                event_type,
                date_value: opt_str(&date),
                date_sort: None,
                date_qualifier: DateQualifier::default(),
                date_value2: None,
                calendar: Calendar::default(),
                witnesses: vec![],
                cause: None,
                place_id,
                person_id: None,
                family_id: Some(fid),
                description: opt_str(&desc),
            };
            match api.create_event(tid, &body).await {
                Ok(_) => {
                    show_add_union_event.set(false);
                    new_union_date.set(String::new());
                    new_union_place.set(String::new());
                    new_union_desc.set(String::new());
                    save_error.set(None);
                    on_saved_create_union.call(());
                    refresh += 1;
                }
                Err(e) => save_error.set(Some(format!("{e}"))),
            }
        });
    };

    // Delete union event handler.
    let api_del_union = api.clone();
    let on_saved_del_union = props.on_saved;

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

    // Apply staged child detachments, then close.
    let api_save_footer = api.clone();
    let on_saved_footer = props.on_saved;
    let on_close_footer = props.on_close;
    let on_footer_save = move |_| {
        let api = api_save_footer.clone();
        let to_detach: Vec<Uuid> = pending_detach().into_iter().collect();
        spawn(async move {
            if to_detach.is_empty() {
                on_saved_footer.call(());
                on_close_footer.call(());
                return;
            }
            saving.set(true);
            for cid in to_detach {
                if let Err(e) = api.remove_child(tid, fid, cid).await {
                    save_error.set(Some(format!("{e}")));
                    saving.set(false);
                    return;
                }
            }
            saving.set(false);
            on_saved_footer.call(());
            on_close_footer.call(());
        });
    };

    // Delete couple (removes the union only — persons remain in the tree).
    let api_delete_couple = api.clone();
    let on_saved_delete_couple = props.on_saved;
    let on_close_delete_couple = props.on_close;
    let on_confirm_delete_couple = move |_| {
        let api = api_delete_couple.clone();
        spawn(async move {
            deleting.set(true);
            delete_error.set(None);
            match api.delete_family(tid, fid).await {
                Ok(_) => {
                    on_saved_delete_couple.call(());
                    on_close_delete_couple.call(());
                }
                Err(e) => {
                    delete_error.set(Some(format!("{e}")));
                    deleting.set(false);
                }
            }
        });
    };

    // ── Render ──

    rsx! {
        div { class: "modal-backdrop union-form-backdrop",
            onclick: move |_| props.on_close.call(()),

            div {
                class: "union-form-modal",
                onclick: move |evt| evt.stop_propagation(),
                onkeydown: move |e: Event<KeyboardData>| {
                    match e.key() {
                        Key::Escape => props.on_close.call(()),
                        Key::Enter => {
                            document::eval(
                                "var a=document.activeElement;\
                                if(a&&a.tagName==='INPUT'&&a.type!=='button'&&a.type!=='submit'){\
                                    var m=a.closest('.union-form-modal');\
                                    if(!m)return;\
                                    var fs=[...m.querySelectorAll('input:not([type=button]):not([type=submit]),select,textarea')];\
                                    var i=fs.indexOf(a);\
                                    if(i>=0&&i<fs.length-1)fs[i+1].focus();\
                                }"
                            );
                        }
                        _ => {}
                    }
                },

                // Header
                div { class: "union-form-header",
                    div {
                        h2 { "{couple_title}" }
                        span { class: "pf-subtitle", {i18n.t("union_form.subtitle_edit")} }
                    }
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
                    // ── Union block ──
                    div { class: "union-form-section",
                        div { class: "section-header",
                            h3 { style: "font-size: 0.95rem;", {i18n.t("union_form.events")} }
                            button {
                                class: "btn btn-primary btn-sm",
                                onclick: move |_| show_add_union_event.toggle(),
                                if show_add_union_event() { {i18n.t("common.cancel")} } else { {i18n.t("union_form.add_event")} }
                            }
                        }

                        // Existing union events
                        if union_events.is_empty() && marriage_event_id().is_none() {
                            div { class: "empty-state",
                                p { {i18n.t("union_form.no_events")} }
                            }
                        }

                        // Primary union date/place/note shorthand (mapped to the marriage event).
                        if marriage_event_id().is_some() || union_events.is_empty() {
                            div { style: "margin-bottom: 12px; padding: 12px; background: var(--bg-card); border-radius: var(--radius); border: 1px solid var(--border);",
                                div { class: "form-group",
                                    label { {i18n.t("person_form.date")} }
                                    input {
                                        r#type: "text",
                                        placeholder: "{i18n.t(\"union_form.date_placeholder\")}",
                                        value: "{marriage_date}",
                                        oninput: move |e: Event<FormData>| marriage_date.set(e.value()),
                                    }
                                }
                                div { class: "form-group",
                                    label { {i18n.t("person_form.place")} }
                                    select {
                                        value: "{marriage_place_id}",
                                        oninput: move |e: Event<FormData>| marriage_place_id.set(e.value()),
                                        option { value: "", {i18n.t("person_form.no_place")} }
                                        for (pid, pname) in place_options.iter() {
                                            option { value: "{pid}", "{pname}" }
                                        }
                                    }
                                }
                                div { class: "form-group",
                                    label { {i18n.t("person_form.description")} }
                                    input {
                                        r#type: "text",
                                        placeholder: "",
                                        value: "{marriage_desc}",
                                        oninput: move |e: Event<FormData>| marriage_desc.set(e.value()),
                                    }
                                }
                                button {
                                    class: "btn btn-primary",
                                    onclick: on_save_marriage,
                                    if marriage_event_id().is_some() { {i18n.t("union_form.update_marriage")} } else { {i18n.t("union_form.save_marriage")} }
                                }
                            }
                        }

                        // Other union events (not the primary one)
                        for evt in union_events.iter() {
                            if Some(evt.id) != marriage_event_id() {
                                {
                                    let eid = evt.id;
                                    let et = format!("{:?}", evt.event_type);
                                    let date = evt.date_value.clone().unwrap_or_default();
                                    let desc = evt.description.clone().unwrap_or_default();
                                    rsx! {
                                        div { class: "person-form-item",
                                            div { class: "person-form-item-info",
                                                span { class: "badge", "{et}" }
                                                if !date.is_empty() { span { "{date}" } }
                                                if !desc.is_empty() { span { class: "text-muted", "— {desc}" } }
                                            }
                                            div { class: "person-form-item-actions",
                                                button {
                                                    class: "btn btn-danger btn-sm",
                                                    onclick: {
                                                        let api = api_del_union.clone();
                                                        move |_| {
                                                            let api = api.clone();
                                                            spawn(async move {
                                                                match api.delete_event(tid, eid).await {
                                                                    Ok(_) => {
                                                                        on_saved_del_union.call(());
                                                                        refresh += 1;
                                                                    }
                                                                    Err(e) => save_error.set(Some(format!("{e}"))),
                                                                }
                                                            });
                                                        }
                                                    },
                                                    {i18n.t("common.remove")}
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        // Add union event form
                        if show_add_union_event() {
                            div { style: "padding: 12px; background: var(--bg-card); border-radius: var(--radius); border: 1px solid var(--border); margin-top: 8px;",
                                div { class: "form-row",
                                    div { class: "form-group",
                                        label { {i18n.t("person_form.type")} }
                                        select {
                                            value: "{new_union_type}",
                                            oninput: move |e: Event<FormData>| new_union_type.set(e.value()),
                                            optgroup { label: "{i18n.t(\"union_form.core_events\")}",
                                                option { value: "Marriage", {i18n.t("event.type.marriage")} }
                                                option { value: "Divorce", {i18n.t("event.type.divorce")} }
                                                option { value: "Annulment", {i18n.t("event.type.annulment")} }
                                                option { value: "Engagement", {i18n.t("event.type.engagement")} }
                                                option { value: "MarriageBann", {i18n.t("event.type.marriage_bann")} }
                                                option { value: "MarriageContract", {i18n.t("event.type.marriage_contract")} }
                                                option { value: "MarriageLicense", {i18n.t("event.type.marriage_license")} }
                                                option { value: "MarriageSettlement", {i18n.t("event.type.marriage_settlement")} }
                                                option { value: "CivilUnion", {i18n.t("event.type.civil_union")} }
                                                option { value: "Separation", {i18n.t("event.type.separation")} }
                                                option { value: "DivorceFiled", {i18n.t("event.type.divorce_filed")} }
                                            }
                                            optgroup { label: "{i18n.t(\"union_form.optional_events\")}",
                                                option { value: "Residence", {i18n.t("event.type.residence")} }
                                                option { value: "Census", {i18n.t("event.type.census")} }
                                                option { value: "Emigration", {i18n.t("event.type.emigration")} }
                                                option { value: "Immigration", {i18n.t("event.type.immigration")} }
                                                option { value: "Will", {i18n.t("event.type.will")} }
                                                option { value: "Probate", {i18n.t("event.type.probate")} }
                                                option { value: "Other", {i18n.t("event.type.other")} }
                                            }
                                        }
                                    }
                                    div { class: "form-group",
                                        label { {i18n.t("person_form.date")} }
                                        input {
                                            r#type: "text",
                                            placeholder: "{i18n.t(\"union_form.date_placeholder\")}",
                                            value: "{new_union_date}",
                                            oninput: move |e: Event<FormData>| new_union_date.set(e.value()),
                                        }
                                    }
                                }
                                div { class: "form-row",
                                    div { class: "form-group",
                                        label { {i18n.t("person_form.place")} }
                                        select {
                                            value: "{new_union_place}",
                                            oninput: move |e: Event<FormData>| new_union_place.set(e.value()),
                                            option { value: "", {i18n.t("person_form.no_place")} }
                                            for (pid, pname) in place_options.iter() {
                                                option { value: "{pid}", "{pname}" }
                                            }
                                        }
                                    }
                                    div { class: "form-group",
                                        label { {i18n.t("person_form.description")} }
                                        input {
                                            r#type: "text",
                                            value: "{new_union_desc}",
                                            oninput: move |e: Event<FormData>| new_union_desc.set(e.value()),
                                        }
                                    }
                                }
                                button {
                                    class: "btn btn-primary btn-sm",
                                    onclick: on_create_union_event,
                                    {i18n.t("person.create_event")}
                                }
                            }
                        }
                    }

                    // ── Children block ──
                    div { class: "union-form-section",
                        div { class: "section-header",
                            h3 { style: "font-size: 0.95rem;", {i18n.t("union_form.children")} }
                            button {
                                class: "btn btn-primary btn-sm",
                                onclick: move |_| show_add_child.toggle(),
                                if show_add_child() { {i18n.t("common.cancel")} } else { {i18n.t("union_form.add_child")} }
                            }
                        }

                        if show_add_child() {
                            div { class: "linking-panel",
                                p { class: "linking-panel-title", {i18n.t("union_form.link_or_create")} }
                                SearchPerson {
                                    tree_id: tid,
                                    placeholder: i18n.t("union_form.search_child"),
                                    on_select: on_select_child,
                                    on_cancel: move |_| show_add_child.set(false),
                                }
                            }
                        }

                        match &*children_resource.read() {
                            Some(Ok(children)) => rsx! {
                                if children.is_empty() {
                                    div { class: "empty-state",
                                        p { {i18n.t("union_form.no_children")} }
                                    }
                                } else {
                                    for child in children.iter() {
                                        {
                                            let cid = child.person_id;
                                            let ct = format!("{:?}", child.child_type);
                                            let name = resolve_name(cid, &name_map_for_display);
                                            let is_pending = pending_detach().contains(&cid);
                                            let is_confirming = confirm_detach_id() == Some(cid);
                                            rsx! {
                                                if is_confirming {
                                                    div { class: "uf-child-detach-confirm",
                                                        p { {i18n.t_args("union_form.detach_confirm_title", &[("name", &name)])} }
                                                        p { {i18n.t_args("union_form.detach_confirm_message", &[("name", &name)])} }
                                                        div { class: "pf-delete-confirm-actions",
                                                            button {
                                                                class: "btn btn-outline btn-sm",
                                                                r#type: "button",
                                                                onclick: move |_| confirm_detach_id.set(None),
                                                                {i18n.t("common.cancel")}
                                                            }
                                                            button {
                                                                class: "btn btn-danger btn-sm",
                                                                r#type: "button",
                                                                onclick: move |_| {
                                                                    let mut set = pending_detach();
                                                                    set.insert(cid);
                                                                    pending_detach.set(set);
                                                                    confirm_detach_id.set(None);
                                                                },
                                                                {i18n.t("union_form.detach_confirm_button")}
                                                            }
                                                        }
                                                    }
                                                } else {
                                                    div { class: if is_pending { "uf-child-row pending-detach" } else { "uf-child-row" },
                                                        div { class: "uf-child-avatar", "\u{1F464}" }
                                                        div { class: "uf-child-info",
                                                            span { class: "badge", "{ct}" }
                                                            strong { "{name}" }
                                                        }
                                                        if is_pending {
                                                            button {
                                                                class: "btn btn-outline btn-sm",
                                                                r#type: "button",
                                                                onclick: move |_| {
                                                                    let mut set = pending_detach();
                                                                    set.remove(&cid);
                                                                    pending_detach.set(set);
                                                                },
                                                                {i18n.t("union_form.undo_detach")}
                                                            }
                                                        } else {
                                                            button {
                                                                class: "btn btn-danger btn-sm",
                                                                r#type: "button",
                                                                onclick: move |_| confirm_detach_id.set(Some(cid)),
                                                                {i18n.t("union_form.detach_button")}
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
                                div { class: "error-msg", {i18n.t_args("union_form.load_children_error", &[("error", &e.to_string())])} }
                            },
                            None => rsx! {
                                div { class: "loading", {i18n.t("union_form.loading_children")} }
                            },
                        }
                    }

                    // ── Person 1 block ──
                    if let Some(s1) = &spouse1 {
                        {
                            let pid1 = s1.person_id;
                            let name1 = resolve_name(pid1, &name_map_for_display);
                            rsx! {
                                div { class: "uf-person-block",
                                    button {
                                        class: "uf-section-toggle",
                                        r#type: "button",
                                        onclick: move |_| show_person1.toggle(),
                                        div { class: "pf-section-title", {i18n.t_args("union_form.person1", &[("name", &name1)])} }
                                        span { class: if show_person1() { "uf-chevron open" } else { "uf-chevron" }, "\u{276F}" }
                                    }
                                    if show_person1() {
                                        PersonForm {
                                            tree_id: tid,
                                            person_id: Some(pid1),
                                            embedded: true,
                                            on_close: move |_| {},
                                            on_saved: move |_| refresh += 1,
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // ── Person 2 block ──
                    if let Some(s2) = &spouse2 {
                        {
                            let pid2 = s2.person_id;
                            let name2 = resolve_name(pid2, &name_map_for_display);
                            rsx! {
                                div { class: "uf-person-block",
                                    button {
                                        class: "uf-section-toggle",
                                        r#type: "button",
                                        onclick: move |_| show_person2.toggle(),
                                        div { class: "pf-section-title", {i18n.t_args("union_form.person2", &[("name", &name2)])} }
                                        span { class: if show_person2() { "uf-chevron open" } else { "uf-chevron" }, "\u{276F}" }
                                    }
                                    if show_person2() {
                                        PersonForm {
                                            tree_id: tid,
                                            person_id: Some(pid2),
                                            embedded: true,
                                            on_close: move |_| {},
                                            on_saved: move |_| refresh += 1,
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // ── Delete couple ──
                    div { class: "pf-delete-section",
                        if show_delete_confirm() {
                            div { class: "pf-delete-confirm",
                                p { class: "pf-delete-confirm-name", {i18n.t("union_form.delete_confirm_title")} }
                                p { class: "pf-delete-confirm-message", {i18n.t("union_form.delete_confirm_message")} }
                                if let Some(err) = delete_error() { div { class: "error-msg", "{err}" } }
                                div { class: "pf-delete-confirm-actions",
                                    button {
                                        class: "btn btn-outline btn-sm",
                                        r#type: "button",
                                        disabled: deleting(),
                                        onclick: move |_| { show_delete_confirm.set(false); delete_error.set(None); },
                                        {i18n.t("common.cancel")}
                                    }
                                    button {
                                        class: "btn btn-danger btn-sm",
                                        r#type: "button",
                                        disabled: deleting(),
                                        onclick: on_confirm_delete_couple,
                                        if deleting() { {i18n.t("union_form.deleting")} } else { {i18n.t("union_form.delete_confirm_button")} }
                                    }
                                }
                            }
                        } else {
                            hr { class: "pf-delete-divider" }
                            button {
                                class: "pf-delete-person-btn",
                                r#type: "button",
                                onclick: move |_| show_delete_confirm.set(true),
                                {i18n.t("union_form.delete_couple")}
                            }
                        }
                    }
                }

                // ── Fixed footer ──
                div { class: "uf-footer",
                    div { class: "uf-footer-right",
                        button {
                            class: "btn btn-outline",
                            r#type: "button",
                            onclick: move |_| props.on_close.call(()),
                            {i18n.t("common.cancel")}
                        }
                        button {
                            class: "btn btn-primary",
                            r#type: "button",
                            disabled: saving(),
                            onclick: on_footer_save,
                            if saving() { {i18n.t("common.saving")} } else { {i18n.t("common.save")} }
                        }
                    }
                }
            }
        }
    }
}
