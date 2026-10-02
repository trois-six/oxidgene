//! The events panel beside the pedigree: the selected person with the events
//! of their life and of their close family, grouped by year, and the handle
//! that resizes the panel.

use dioxus::prelude::*;
use oxidgene_core::EventType;
use oxidgene_core::types::Event as DomainEvent;
use uuid::Uuid;

use super::{
    EVENT_PANEL_AUTO_COLLAPSE_WIDTH, EVENT_PANEL_KEYBOARD_STEP, EVENT_PANEL_MANUAL_STORAGE_KEY,
    EVENT_PANEL_MAX_RATIO, EVENT_PANEL_MAX_WIDTH, EVENT_PANEL_MIN_WIDTH,
    EVENT_PANEL_RATIO_STORAGE_KEY, SharedPedigree, VIEWPORT_DEFAULT_W,
    WAIT_FOR_EVENT_PANEL_TRANSITION_JS, chart_portrait, format_lifespan,
};
use crate::api::CroppedSource;
use crate::components::cropped_image::CroppedImage;
use crate::components::date_input::format_event_date;
use crate::components::event_icon::EventIcon;
use crate::i18n::{I18n, use_i18n};

/// The events of a life that its family's panel shows too.
const LIFE_EVENTS: [EventType; 4] = [
    EventType::Birth,
    EventType::Death,
    EventType::Baptism,
    EventType::Burial,
];

/// Restores the panel as the reader left it, once: collapsed by hand, or on
/// a screen too narrow for it, and at the width they dragged it to. The
/// returned signal turns true once that is done, so the first fit measures
/// the panel at its restored width.
pub(super) fn use_restored_event_panel(
    mut collapsed: Signal<bool>,
    mut last_viewport_width: Signal<f64>,
) -> Signal<bool> {
    let mut ready = use_signal(|| false);
    use_hook(move || {
        spawn(async move {
            if let Ok(val) = document::eval(&restore_panel_js()).await {
                let manual_collapsed = val
                    .get(0)
                    .and_then(|value| value.as_bool())
                    .unwrap_or(false);
                let width = val
                    .get(1)
                    .and_then(|value| value.as_f64())
                    .unwrap_or(VIEWPORT_DEFAULT_W);
                last_viewport_width.set(width);
                collapsed.set(manual_collapsed || width <= EVENT_PANEL_AUTO_COLLAPSE_WIDTH);
            }
            let _ = document::eval(
                r#"
                await new Promise(requestAnimationFrame);
                await new Promise(requestAnimationFrame);
                document.getElementById('oxidgene-panel-restore-guard')?.remove();
                "#,
            )
            .await;
            ready.set(true);
        });
    });
    ready
}

/// Reapplies the stored width, without the width transition, and answers
/// whether the panel was collapsed by hand and how wide the window is.
fn restore_panel_js() -> String {
    format!(
        r#"
        localStorage.removeItem('oxidgene-ev-panel');
        const transitionGuard = document.createElement('style');
        transitionGuard.id = 'oxidgene-panel-restore-guard';
        transitionGuard.textContent = '.ev-panel {{ transition: none !important; }}';
        document.head.appendChild(transitionGuard);
        const storedRatio = Number.parseFloat(localStorage.getItem('{EVENT_PANEL_RATIO_STORAGE_KEY}'));
        if (Number.isFinite(storedRatio) && storedRatio > 0) {{
            // Only a panel the reader has dragged is proportional; the
            // untouched default stays at the fixed width from the CSS.
            const ratio = Math.min({EVENT_PANEL_MAX_RATIO}, storedRatio);
            const sidebarWidth = document.querySelector('.pedigree-outer > .isb')?.getBoundingClientRect().width || 46;
            document.documentElement.style.setProperty(
                '--evw',
                `calc(${{ratio * 100}}% - ${{ratio * sidebarWidth}}px)`,
            );
        }}
        const width = window.innerWidth || document.documentElement.clientWidth || 1024;
        return [localStorage.getItem('{EVENT_PANEL_MANUAL_STORAGE_KEY}') === 'collapsed', width];
        "#,
    )
}

/// The panel, its resize handle when it is open, and its toggle. Toggling it
/// asks the chart for a fit once the panel has finished moving.
#[component]
pub(super) fn EventPanel(
    data: SharedPedigree,
    selected: Uuid,
    tree_id: String,
    collapsed: Signal<bool>,
    needs_fit: Signal<bool>,
) -> Element {
    let i18n = use_i18n();
    rsx! {
        if !collapsed() {
            EventPanelResizeHandle {}
        }
        div {
            class: if collapsed() { "ev-panel ev-panel-collapsed" } else { "ev-panel" },
            button {
                class: "evp-toggle",
                title: if collapsed() { i18n.t("pedigree.events") } else { i18n.t("pedigree.hide_events") },
                onclick: move |_| toggle_panel(collapsed, needs_fit),
                if collapsed() { "\u{203A}" } else { "\u{2039}" }
            }
            if !collapsed() {
                EventPanelBody { data, selected, tree_id }
            }
        }
    }
}

/// Opens or closes the panel, remembering the reader chose it, and refits
/// the chart once the panel has moved.
fn toggle_panel(mut collapsed: Signal<bool>, mut needs_fit: Signal<bool>) {
    let now_collapsed = !collapsed();
    collapsed.set(now_collapsed);
    let state = if now_collapsed { "collapsed" } else { "open" };
    document::eval(&format!(
        "localStorage.setItem('{EVENT_PANEL_MANUAL_STORAGE_KEY}', '{state}')",
    ));
    spawn(async move {
        let _ = document::eval(WAIT_FOR_EVENT_PANEL_TRANSITION_JS).await;
        needs_fit.set(true);
    });
}

/// The separator the reader drags, or moves with the arrow keys, to resize
/// the panel.
#[component]
fn EventPanelResizeHandle() -> Element {
    let i18n = use_i18n();
    rsx! {
        div {
            class: "evp-resize-handle",
            role: "separator",
            tabindex: "0",
            "aria-orientation": "vertical",
            "aria-label": i18n.t("pedigree.resize_events"),
            title: i18n.t("pedigree.resize_events"),
            onpointerdown: move |evt| {
                document::eval(&drag_panel_js(evt.client_coordinates().x));
            },
            onkeydown: move |evt| {
                let delta = match evt.key() {
                    Key::ArrowLeft => EVENT_PANEL_KEYBOARD_STEP,
                    Key::ArrowRight => -EVENT_PANEL_KEYBOARD_STEP,
                    _ => return,
                };
                evt.prevent_default();
                document::eval(&step_panel_js(delta));
            },
        }
    }
}

/// Follows a drag of the resize handle started at `start_x`, then stores the
/// width as a ratio of the room beside the sidebar.
fn drag_panel_js(start_x: f64) -> String {
    format!(
        r#"
        const outer = document.querySelector('.pedigree-outer');
        const panel = document.querySelector('.ev-panel:not(.ev-panel-collapsed)');
        if (!outer || !panel || window.innerWidth <= {EVENT_PANEL_AUTO_COLLAPSE_WIDTH}) return;

        const sidebarWidth = outer.querySelector(':scope > .isb')?.getBoundingClientRect().width || 46;
        const availableWidth = Math.max(1, outer.getBoundingClientRect().width - sidebarWidth);
        const maxWidth = Math.max(
            {EVENT_PANEL_MIN_WIDTH},
            Math.min({EVENT_PANEL_MAX_WIDTH}, availableWidth * {EVENT_PANEL_MAX_RATIO}),
        );
        const startWidth = panel.getBoundingClientRect().width;
        const startX = {start_x};

        let width = startWidth;
        const move = (event) => {{
            const requested = startWidth + startX - event.clientX;
            width = Math.min(maxWidth, Math.max({EVENT_PANEL_MIN_WIDTH}, requested));
            document.documentElement.style.setProperty('--evw', `${{width}}px`);
        }};
        const finish = () => {{
            window.removeEventListener('pointermove', move);
            window.removeEventListener('pointerup', finish);
            window.removeEventListener('pointercancel', finish);
            outer.classList.remove('pedigree-is-resizing');
            document.body.style.removeProperty('cursor');
            document.body.style.removeProperty('user-select');

            // Store and re-apply the width as a ratio so it
            // follows later window resizes.
            const ratio = width / availableWidth;
            document.documentElement.style.setProperty(
                '--evw',
                `calc(${{ratio * 100}}% - ${{ratio * sidebarWidth}}px)`,
            );
            localStorage.setItem('{EVENT_PANEL_RATIO_STORAGE_KEY}', String(ratio));
            document.querySelector('.pedigree-resize-fit-trigger')?.click();
        }};

        outer.classList.add('pedigree-is-resizing');
        document.body.style.cursor = 'col-resize';
        document.body.style.userSelect = 'none';
        window.addEventListener('pointermove', move);
        window.addEventListener('pointerup', finish);
        window.addEventListener('pointercancel', finish);
        "#,
    )
}

/// Widens the panel by `delta` pixels, narrows it when negative, within its
/// bounds, and stores the width as a ratio.
fn step_panel_js(delta: f64) -> String {
    format!(
        r#"
        const outer = document.querySelector('.pedigree-outer');
        const panel = document.querySelector('.ev-panel:not(.ev-panel-collapsed)');
        if (!outer || !panel || window.innerWidth <= {EVENT_PANEL_AUTO_COLLAPSE_WIDTH}) return;
        const sidebarWidth = outer.querySelector(':scope > .isb')?.getBoundingClientRect().width || 46;
        const availableWidth = Math.max(1, outer.getBoundingClientRect().width - sidebarWidth);
        const maxWidth = Math.max(
            {EVENT_PANEL_MIN_WIDTH},
            Math.min({EVENT_PANEL_MAX_WIDTH}, availableWidth * {EVENT_PANEL_MAX_RATIO}),
        );
        const width = Math.min(
            maxWidth,
            Math.max({EVENT_PANEL_MIN_WIDTH}, panel.getBoundingClientRect().width + {delta}),
        );
        const ratio = width / availableWidth;
        document.documentElement.style.setProperty(
            '--evw',
            `calc(${{ratio * 100}}% - ${{ratio * sidebarWidth}}px)`,
        );
        localStorage.setItem('{EVENT_PANEL_RATIO_STORAGE_KEY}', String(ratio));
        document.querySelector('.pedigree-resize-fit-trigger')?.click();
        "#,
    )
}

/// The open panel: who is selected, and their events by year.
#[component]
fn EventPanelBody(data: SharedPedigree, selected: Uuid, tree_id: String) -> Element {
    let i18n = use_i18n();
    // The same resolver every other surface uses, so the no-name fallback is
    // the translated one rather than a hardcoded "Unknown".
    let full_name = data.display_name(selected, &i18n);
    let silhouette = CroppedSource::silhouette(data.sex_of(selected));
    let portrait = chart_portrait(selected).unwrap_or_else(|| silhouette.clone());
    // The same lifespan the card draws, rather than the old "n. 1620" / "d.
    // 1691" abbreviations: the panel sits beside the card showing the very
    // same person, and two spellings of one life read as two different facts.
    // The events below keep their own full-text dates.
    // Always the wide form here: this is HTML that wraps, so unlike the card
    // it never has to give up a range's far end.
    let dates = format_lifespan(
        i18n.dates(),
        data.qualified_birth_year(selected),
        data.qualified_death_year(selected),
    );
    let groups = events_by_year(&selected_person_events(&data, selected), &i18n);
    rsx! {
        div { class: "evp-hd", {i18n.t("pedigree.events")} }
        div { class: "evp-person",
            div { class: "evp-av",
                CroppedImage {
                    image: portrait,
                    alt: String::new(),
                    fallback: silhouette,
                }
            }
            div { class: "evp-name",
                strong { "{full_name}" }
                if !dates.is_empty() {
                    span { "{dates}" }
                }
            }
        }
        div { class: "evp-list",
            if groups.is_empty() {
                div { class: "evp-empty", {i18n.t("person_form.no_other_events")} }
            }
            for (gi, (year, events)) in groups.into_iter().enumerate() {
                div { key: "evg-{gi}", class: "ev-year-group",
                    div { class: "ev-year-header", "{year}" }
                    for (ei, event) in events.into_iter().enumerate() {
                        PedigreeEventRow {
                            key: "ev-{gi}-{ei}",
                            data: data.clone(),
                            event,
                            selected,
                            tree_id: tree_id.clone(),
                        }
                    }
                }
            }
        }
    }
}

/// One event of the panel: what, whose when it is a relative's, when and
/// where. Its person's own events and their couples' are marked direct;
/// the rest is family context.
#[component]
fn PedigreeEventRow(
    data: SharedPedigree,
    event: DomainEvent,
    selected: Uuid,
    tree_id: String,
) -> Element {
    let i18n = use_i18n();
    let nav = use_navigator();
    let label = i18n.t(row_label_key(event.event_type));
    let full_label = match event_context(&data, &event, selected, &i18n) {
        Some(context) => format!("{label} ({context})"),
        None => label,
    };
    let date = format_event_date(&i18n, &event);
    let place = event
        .place_id
        .and_then(|pid| data.place_name(pid).map(String::from))
        .or_else(|| event.description.clone())
        .unwrap_or_default();
    let is_direct = event.person_id == Some(selected)
        || event.family_id.is_some_and(|fid| {
            data.families_as_spouse
                .get(&selected)
                .is_some_and(|own| own.contains(&fid))
        });
    let item_class = if is_direct {
        "ev-item ev-item-clickable ev-item-direct"
    } else {
        "ev-item ev-item-clickable"
    };
    rsx! {
        div {
            class: "{item_class}",
            onclick: move |_| {
                nav.push(crate::router::Route::PersonDetail {
                    tree_id: tree_id.clone(),
                    person_id: selected.to_string(),
                });
            },
            EventIcon { event_type: event.event_type }
            div { class: "ev-info",
                div { class: "ev-type", "{full_label}" }
                if !date.is_empty() {
                    div { class: "ev-date", "{date}" }
                }
                if !place.is_empty() {
                    div { class: "ev-place", "{place}" }
                }
            }
        }
    }
}

/// An event's name in the narrow panel: the marriage formalities by their
/// short names, every other type by its own.
fn row_label_key(event_type: EventType) -> &'static str {
    match event_type {
        EventType::MarriageBann => "event.short.banns",
        EventType::MarriageContract => "event.short.contract",
        EventType::MarriageLicense => "event.short.license",
        EventType::MarriageSettlement => "event.short.settlement",
        other => crate::utils::event_type_label_key(other),
    }
}

/// Whose event it is, when not the selected person's own: the relative's
/// name, or for a couple's event the partner's.
fn event_context(
    data: &SharedPedigree,
    event: &DomainEvent,
    selected: Uuid,
    i18n: &I18n,
) -> Option<String> {
    match (event.person_id, event.family_id) {
        (Some(pid), _) if pid != selected => Some(data.display_name(pid, i18n)),
        // Family event (marriage, divorce…) — show partner name.
        (None, Some(fid)) => data
            .spouses_by_family
            .get(&fid)?
            .iter()
            .find(|s| s.person_id != selected)
            .map(|s| data.display_name(s.person_id, i18n)),
        _ => None,
    }
}

/// The events the panel shows for `selected`, once each, by date: their
/// own; their couples' and the life events of their children; their
/// parents' family's, the deaths and burials of their parents, and the life
/// events of their siblings.
fn selected_person_events(data: &SharedPedigree, selected: Uuid) -> Vec<DomainEvent> {
    let own = |pid: Uuid| data.events_by_person.get(&pid).into_iter().flatten();
    let of_family = |fid: &Uuid| data.events_by_family.get(fid).into_iter().flatten();
    let children = |fid: &Uuid| data.children_by_family.get(fid).into_iter().flatten();
    let life = |e: &&DomainEvent| LIFE_EVENTS.contains(&e.event_type);
    let end = |e: &&DomainEvent| matches!(e.event_type, EventType::Death | EventType::Burial);

    let mut events: Vec<DomainEvent> = own(selected).cloned().collect();
    for fid in data.families_as_spouse.get(&selected).into_iter().flatten() {
        events.extend(of_family(fid).cloned());
        for child in children(fid) {
            events.extend(own(child.person_id).filter(life).cloned());
        }
    }
    for fid in data.families_as_child.get(&selected).into_iter().flatten() {
        events.extend(of_family(fid).cloned());
        for parent in data.spouses_by_family.get(fid).into_iter().flatten() {
            events.extend(own(parent.person_id).filter(end).cloned());
        }
        for sibling in children(fid).filter(|c| c.person_id != selected) {
            events.extend(own(sibling.person_id).filter(life).cloned());
        }
    }
    events.sort_by_key(|a| a.id);
    events.dedup_by_key(|e| e.id);
    events.sort_by_key(|a| a.date_sort);
    events
}

/// `events`, already in date order, in runs of one year each; the undated
/// under their own heading.
fn events_by_year(events: &[DomainEvent], i18n: &I18n) -> Vec<(String, Vec<DomainEvent>)> {
    let mut groups: Vec<(String, Vec<DomainEvent>)> = Vec::new();
    for event in events {
        let year = event
            .year()
            .map_or_else(|| i18n.t("pedigree.events_undated"), |y| y.to_string());
        match groups.last_mut() {
            Some((last, run)) if *last == year => run.push(event.clone()),
            _ => groups.push((year, vec![event.clone()])),
        }
    }
    groups
}
