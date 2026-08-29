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

use crate::api::{AddChildBody, ApiClient};
use crate::components::date_input::{DateInput, DateParts, format_event_date};
use crate::components::media_gallery::MediaOwner;
use crate::components::media_manager_modal::MediaManagerModal;
use crate::components::person_form::{
    DeleteSection, EventEditor, EventOwner, FormSection, NotesSource, PersonForm,
    create_event_body, focus_next_field_js, render_add_toggle, render_notes_source_fields,
    render_place_select, save_notes_source, update_event_body,
};
use crate::components::search_person::SearchPerson;
use crate::i18n::use_i18n;
use crate::utils::{
    child_type_label_key, event_type_label_key, opt_str, parse_privacy, resolve_name,
};
use oxidgene_core::{ChildType, EventType};

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
    // A couple has a privacy of its own: a living pair's marriage is a fact
    // about two living people, and withholding both their person records does
    // not withhold the union that names them.
    let mut privacy_val = use_signal(|| "Default".to_string());
    let mut privacy_loaded = use_signal(|| false);
    let open_privacy = use_signal(|| true);

    // ── State ──
    let mut save_error = use_signal(|| None::<String>);

    // Marriage event state (primary/first union event).
    let mut marriage_parts = use_signal(DateParts::default);
    let mut marriage_place_id = use_signal(String::new);
    let mut marriage_desc = use_signal(String::new);
    let mut marriage_event_id = use_signal(|| None::<Uuid>);
    let mut marriage_loaded = use_signal(|| false);

    // Which union event row is expanded into its full editor.
    let mut open_union_event = use_signal(|| None::<Uuid>);

    // Add union event state.
    let mut show_add_union_event = use_signal(|| false);
    let mut new_union_type = use_signal(|| "Marriage".to_string());
    let mut new_union_parts = use_signal(DateParts::default);
    let mut new_union_notes = use_signal(String::new);
    let mut new_union_source = use_signal(String::new);
    let mut new_union_place = use_signal(String::new);
    let mut new_union_desc = use_signal(String::new);

    // Add child linking mode.
    let mut show_add_child = use_signal(|| false);

    // Section fold state. The union's own blocks open with the form; the two
    // person blocks stay closed, since each mounts a whole PersonForm with its
    // own fetches — opening both by default would load the couple twice over.
    let open_union = use_signal(|| true);
    let open_children = use_signal(|| true);
    let mut media_manager_open = use_signal(|| false);
    let show_person1 = use_signal(|| false);
    let show_person2 = use_signal(|| false);

    // Staged child detach (applied on Save).
    let mut pending_detach = use_signal(HashSet::<Uuid>::new);
    let mut confirm_detach_id = use_signal(|| None::<Uuid>);

    // Delete couple state (the confirmation itself lives in DeleteSection).
    let mut delete_error = use_signal(|| None::<String>);
    let mut deleting = use_signal(|| false);
    let mut saving = use_signal(|| false);

    // ── Resources ──

    // Spouses
    let api_spouses = api.clone();
    // Seeded once from the stored row: re-seeding on every render would fight
    // the user's own clicks.
    let family_resource = use_resource({
        let api = api.clone();
        move || {
            let api = api.clone();
            async move { api.get_family(tid, fid).await.ok() }
        }
    });
    if !privacy_loaded()
        && let Some(Some(family)) = &*family_resource.read_unchecked()
    {
        privacy_val.set(format!("{:?}", family.privacy));
        privacy_loaded.set(true);
    }

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
        // Every page: an event may sit on any place in the tree, and a place
        // missing from this list has no name to show.
        async move { api.list_all_places(tid).await }
    });

    // Display names for the people this modal actually shows: the two spouses
    // and the children. It used to list the first 500 persons of the tree and
    // then request the names of every one of them — hundreds of sequential
    // round trips to render seven names, which left the map empty (and every
    // name reading "Unnamed") for as long as it ran, and missed anyone past
    // the 500th outright.
    let api_names_res = api.clone();
    let names_resource = use_resource(move || {
        let api = api_names_res.clone();
        let _tick = refresh();
        async move {
            let mut ids: Vec<Uuid> = Vec::new();
            if let Ok(spouses) = api.list_family_spouses(tid, fid).await {
                ids.extend(spouses.iter().map(|s| s.person_id));
            }
            if let Ok(children) = api.list_family_children(tid, fid).await {
                ids.extend(children.iter().map(|c| c.person_id));
            }

            let mut name_map: std::collections::HashMap<
                Uuid,
                Vec<oxidgene_core::types::PersonName>,
            > = std::collections::HashMap::new();
            for id in ids {
                if let Ok(names) = api.list_person_names(tid, id).await {
                    name_map.insert(id, names);
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
                marriage_parts.set(DateParts::from_fields(
                    ev.calendar,
                    ev.date_qualifier,
                    ev.date_value.as_deref(),
                    ev.date_value2.as_deref(),
                ));
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

    // The union's events as (id, label) pairs — what the media gallery offers
    // when asking which event a certificate documents.
    let union_event_choices: Vec<(Uuid, String)> = union_events
        .iter()
        .map(|ev| {
            let kind = i18n.t(event_type_label_key(ev.event_type));
            let date = format_event_date(&i18n, ev);
            let label = if date.is_empty() {
                kind
            } else {
                format!("{kind} — {date}")
            };
            (ev.id, label)
        })
        .collect();

    // Place options.
    let place_options: Vec<(String, String)> = {
        let data = places_resource.read();
        match &*data {
            Some(Ok(places)) => places
                .iter()
                .map(|p| (p.id.to_string(), p.name.clone()))
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
            resolve_name(s1.person_id, &name_map_for_display, &i18n),
            resolve_name(s2.person_id, &name_map_for_display, &i18n)
        ),
        (Some(s1), None) => resolve_name(s1.person_id, &name_map_for_display, &i18n),
        _ => i18n.t("union_form.title"),
    };

    // ── Handlers ──

    // Save marriage event.
    let api_save_marriage = api.clone();
    let on_saved_marriage = props.on_saved;
    let on_save_marriage = move |_| {
        let api = api_save_marriage.clone();
        let parts = marriage_parts();
        let place_str = marriage_place_id();
        let desc = marriage_desc().trim().to_string();
        let existing_id = marriage_event_id();
        spawn(async move {
            if let Some(key) = parts.validate() {
                save_error.set(Some(i18n.t(key)));
                return;
            }
            if let Some(eid) = existing_id {
                let body = update_event_body(
                    Some(EventType::Marriage),
                    &parts,
                    &place_str,
                    Some(opt_str(&desc)),
                );
                match api.update_event(tid, eid, &body).await {
                    Ok(_) => {
                        save_error.set(None);
                        on_saved_marriage.call(());
                        refresh += 1;
                    }
                    Err(e) => save_error.set(Some(format!("{e}"))),
                }
            } else {
                let body = create_event_body(
                    EventType::Marriage,
                    &parts,
                    &place_str,
                    EventOwner::Family(fid),
                    opt_str(&desc),
                    None,
                );
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
        let parts = new_union_parts();
        let place_str = new_union_place();
        let desc = new_union_desc().trim().to_string();
        let notes = new_union_notes().trim().to_string();
        let source = new_union_source();
        spawn(async move {
            if let Some(key) = parts.validate() {
                save_error.set(Some(i18n.t(key)));
                return;
            }
            let body = create_event_body(
                crate::utils::parse_event_type(&evt_type_str),
                &parts,
                &place_str,
                EventOwner::Family(fid),
                opt_str(&desc),
                None,
            );
            match api.create_event(tid, &body).await {
                Ok(new_event) => {
                    // Family events carry no person: their notes and source
                    // are reached through the event itself.
                    let _ = save_notes_source(
                        &api,
                        tid,
                        None,
                        Some(new_event.id),
                        &notes,
                        &source,
                        &NotesSource::default(),
                    )
                    .await;
                    show_add_union_event.set(false);
                    new_union_parts.set(DateParts::default());
                    new_union_place.set(String::new());
                    new_union_desc.set(String::new());
                    new_union_notes.set(String::new());
                    new_union_source.set(String::new());
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
        div { class: "modal-backdrop",
            // Dismiss on press (not click): a click fires on the common ancestor of
            // mousedown/mouseup, so selecting text then releasing outside would close.
            onmousedown: move |_| props.on_close.call(()),

            div {
                class: "union-form-modal",
                onmousedown: move |evt: Event<MouseData>| evt.stop_propagation(),
                onkeydown: move |e: Event<KeyboardData>| {
                    match e.key() {
                        Key::Escape => props.on_close.call(()),
                        Key::Enter => {
                            document::eval(&focus_next_field_js("union-form-modal"));
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
                    div { class: "uf-header-actions",
                        button {
                            class: "pf-confirm-btn",
                            r#type: "button",
                            onclick: move |_| media_manager_open.set(true),
                            {i18n.t("media.manager_title")}
                        }
                        button {
                            class: "person-form-close",
                            onclick: move |_| props.on_close.call(()),
                            "x"
                        }
                    }
                }

                if let Some(err) = save_error() {
                    div { class: "error-msg", style: "margin: 0 16px;", "{err}" }
                }

                div { class: "union-form-body",
                    // ── Person 1 / Person 2 blocks ──
                    // Each mounts a whole PersonForm with its own fetches, so
                    // both stay closed by default — opening them eagerly would
                    // load the couple twice over.
                    for (spouse , key , open) in [
                        (&spouse1, "union_form.person1", show_person1),
                        (&spouse2, "union_form.person2", show_person2),
                    ] {
                        if let Some(spouse) = spouse {
                            {
                                let spouse_id = spouse.person_id;
                                let name = resolve_name(spouse_id, &name_map_for_display, &i18n);
                                rsx! {
                                    FormSection { title: i18n.t_args(key, &[("name", &name)]), open,
                                        PersonForm {
                                            tree_id: tid,
                                            person_id: Some(spouse_id),
                                            embedded: true,
                                            on_close: move |_| {},
                                            on_saved: move |_| refresh += 1,
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // ── Union block ──
                    FormSection {
                        title: i18n.t("union_form.events"),
                        open: open_union,
                        action: render_add_toggle(
                            i18n.t("union_form.add_event"),
                            i18n.t("common.cancel"),
                            show_add_union_event,
                        ),
                        // Existing union events
                        if union_events.is_empty() && marriage_event_id().is_none() {
                            div { class: "empty-state",
                                p { {i18n.t("union_form.no_events")} }
                            }
                        }

                        // Primary union date/place/note shorthand (mapped to the marriage event).
                        if marriage_event_id().is_some() || union_events.is_empty() {
                            div { class: "pf-subform",
                                div { class: "form-group",
                                    label { {i18n.t("person_form.date")} }
                                    DateInput { parts: marriage_parts, i18n, on_change: move |()| {} }
                                }
                                {render_place_select(&i18n, marriage_place_id, &place_options, || {})}
                                div { class: "form-group",
                                    label { {i18n.t("person_form.description")} }
                                    input {
                                        r#type: "text",
                                        value: "{marriage_desc}",
                                        oninput: move |e: Event<FormData>| marriage_desc.set(e.value()),
                                    }
                                }
                                button {
                                    class: "pf-confirm-btn",
                                    r#type: "button",
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
                                    let et = i18n.t(event_type_label_key(evt.event_type));
                                    let date = format_event_date(&i18n, evt);
                                    let desc = evt.description.clone().unwrap_or_default();
                                    let open = open_union_event() == Some(eid);
                                    rsx! {
                                        div {
                                            class: if open { "person-form-item pf-ns-open" } else { "person-form-item" },
                                            div { class: "person-form-item-info",
                                                span { class: "badge", "{et}" }
                                                if !desc.is_empty() { span { "{desc}" } }
                                                if !date.is_empty() { span { class: "text-muted", "{date}" } }
                                            }
                                            div { class: "person-form-item-actions",
                                                button {
                                                    class: if open { "pf-row-btn is-active" } else { "pf-row-btn" },
                                                    r#type: "button",
                                                    onclick: move |_| open_union_event.set(if open { None } else { Some(eid) }),
                                                    {i18n.t("common.edit")}
                                                }
                                                button {
                                                    class: "pf-row-btn is-danger",
                                                    r#type: "button",
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
                                        if open {
                                            EventEditor {
                                                tree_id: tid,
                                                person_id: None,
                                                event: evt.clone(),
                                                description_label: i18n.t("person_form.description"),
                                                place_options: place_options.clone(),
                                                on_saved: move |_| { on_saved_del_union.call(()); refresh += 1; },
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        // Add union event form
                        if show_add_union_event() {
                            div { class: "pf-subform",
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
                                        DateInput { parts: new_union_parts, i18n, on_change: move |()| {} }
                                    }
                                }
                                div { class: "form-row",
                                    {render_place_select(&i18n, new_union_place, &place_options, || {})}
                                    div { class: "form-group",
                                        label { {i18n.t("person_form.description")} }
                                        input {
                                            r#type: "text",
                                            value: "{new_union_desc}",
                                            oninput: move |e: Event<FormData>| new_union_desc.set(e.value()),
                                        }
                                    }
                                }
                                {render_notes_source_fields(&i18n, new_union_notes, new_union_source, || {})}
                                button {
                                    class: "pf-confirm-btn",
                                    r#type: "button",
                                    onclick: on_create_union_event,
                                    {i18n.t("person.create_event")}
                                }
                            }
                        }
                    }

                    // ── Children block ──
                    FormSection {
                        title: i18n.t("union_form.children"),
                        open: open_children,
                        action: render_add_toggle(
                            i18n.t("union_form.add_child"),
                            i18n.t("common.cancel"),
                            show_add_child,
                        ),
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
                                            let ct = i18n.t(child_type_label_key(child.child_type));
                                            let name = resolve_name(cid, &name_map_for_display, &i18n);
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

                    // ── Privacy ──
                    FormSection { title: i18n.t("union_form.privacy"), open: open_privacy,
                        {crate::components::person_form::render_choice_group(
                            &[
                                ("Default", i18n.t("privacy.default")),
                                ("Public",  i18n.t("privacy.public")),
                                ("Private", i18n.t("privacy.private")),
                            ],
                            privacy_val,
                            {
                                let api = api.clone();
                                move || {
                                    let api = api.clone();
                                    let privacy = parse_privacy(&privacy_val());
                                    spawn(async move {
                                        let _ = api
                                            .update_family_privacy(tid, fid, privacy)
                                            .await;
                                    });
                                }
                            },
                        )}
                        p { class: "pf-ns-hint", {i18n.t("privacy.not_enforced_yet")} }
                    }

                    // ── Delete couple ──
                    // No section header and no rule above it, as in person_form:
                    // the button already says what it does.
                    DeleteSection {
                        button_label: i18n.t("union_form.delete_couple"),
                        title: i18n.t("union_form.delete_confirm_title"),
                        message: i18n.t("union_form.delete_confirm_message"),
                        confirm_label: i18n.t("union_form.delete_confirm_button"),
                        busy_label: i18n.t("union_form.deleting"),
                        deleting: deleting(),
                        error: delete_error(),
                        on_confirm: on_confirm_delete_couple,
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

            if media_manager_open() {
                MediaManagerModal {
                    tree_id: tid,
                    owner: MediaOwner::Family(fid),
                    events: union_event_choices.clone(),
                    on_changed: move |()| props.on_saved.call(()),
                    on_close: move |()| media_manager_open.set(false),
                }
            }
        }
    }
}
