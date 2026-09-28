//! Tools page: one tab per tool a tree's author checks or works it with.
//! See `docs/ui-tools.md`.
//!
//! Each tab is its own component, mounted only while it is shown, so a tab
//! asks the server for its data when it is opened and never for another's.

use dioxus::prelude::*;
use oxidgene_core::calendar::to_jdn;
use oxidgene_core::enums::{Calendar, DateQualifier};
use uuid::Uuid;

use crate::api::ApiClient;
use crate::components::date_input::{DateInput, DateParts};
use crate::components::tree_cache::{fetch_tree_cached, use_current_person, use_tree_cache};
use crate::components::tree_icon_sidebar::{TreeIconSidebar, TreeSidebarView};
use crate::i18n::{I18n, use_i18n};
use crate::prefs::{store, stored};
use crate::router::Route;
use crate::ui_observability::{UiPage, use_traced_resource, use_ui_load_trace};

const TAB_STORAGE_KEY: &str = "oxidgene-tools-tab";

/// The page's tabs, one per tool.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ToolsTab {
    Converter,
}

impl ToolsTab {
    const ALL: [Self; 1] = [Self::Converter];

    /// The tab's name in storage and in its label's i18n key.
    fn key(self) -> &'static str {
        match self {
            Self::Converter => "converter",
        }
    }

    fn parse(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tab| tab.key() == key)
    }
}

#[component]
pub fn Tools(tree_id: String) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let nav = use_navigator();
    let tree_cache = use_tree_cache();
    let load_trace = use_ui_load_trace(UiPage::Tools);
    let tid = tree_id.parse::<Uuid>().ok();

    let mut tab = use_signal(|| ToolsTab::ALL[0]);
    use_effect(move || {
        spawn(async move {
            if let Some(stored_tab) = stored(TAB_STORAGE_KEY)
                .await
                .as_deref()
                .and_then(ToolsTab::parse)
            {
                tab.set(stored_tab);
            }
        });
    });
    let mut choose_tab = move |value: ToolsTab| {
        tab.set(value);
        store(TAB_STORAGE_KEY, value.key());
    };

    let tree = use_traced_resource(load_trace, "tree", move || {
        let api = api.clone();
        let _gen = tree_cache.generation();
        async move { fetch_tree_cached(&api, &tree_cache, tid?).await.ok() }
    });
    // The person last shown in this tree, else its SOSA root.
    let current_person = use_current_person();
    let selected_person_id = tid.and_then(|tid| current_person.get(tid)).or_else(|| {
        tree.read()
            .as_ref()
            .and_then(|tree| tree.as_ref())
            .and_then(|tree| tree.sosa_root_person_id)
    });
    let tree_name = tree
        .read()
        .as_ref()
        .and_then(|t| t.as_ref().map(|t| t.name.clone()))
        .unwrap_or_default();

    rsx! {
        div { class: "sub-page tools-page",
            div { class: "td-topbar",
                nav { class: "td-bc",
                    Link { to: Route::Home {}, class: "td-bc-logo",
                        img {
                            src: crate::components::layout::LOGO_PNG_B64,
                            alt: "OxidGene",
                            class: "td-bc-logo-img",
                        }
                    }
                    if !tree_name.is_empty() {
                        Link {
                            to: Route::TreeDetail { tree_id: tree_id.clone(), person: None },
                            class: "td-bc-link",
                            "{tree_name}"
                        }
                        span { class: "td-bc-sep", "/" }
                    }
                    span { class: "td-bc-current", {i18n.t("tools.breadcrumb")} }
                }
            }

            div { class: "pd-page-shell",
                TreeIconSidebar {
                    active_view: TreeSidebarView::None,
                    selected_person_id,
                    show_middle_separator: false,
                    show_add_person: false,
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
                    on_add_person: move |_| {},
                    on_dictionary: {
                        let tree_id = tree_id.clone();
                        move |_| {
                            nav.push(Route::Dictionary { tree_id: tree_id.clone() });
                        }
                    },
                    on_settings: {
                        let tree_id = tree_id.clone();
                        move |_| {
                            nav.push(Route::Settings { tree_id: tree_id.clone() });
                        }
                    },
                }

                div { class: "sub-page-content tools-content",
                    div { class: "dict-tabs stats-tabs", role: "tablist",
                        for choice in ToolsTab::ALL {
                            button {
                                key: "{choice.key()}",
                                role: "tab",
                                "aria-selected": tab() == choice,
                                class: if tab() == choice { "dict-tab active" } else { "dict-tab" },
                                onclick: move |_| choose_tab(choice),
                                {i18n.t(&format!("tools.tab.{}", choice.key()))}
                            }
                        }
                    }
                    match tab() {
                        ToolsTab::Converter => rsx! { DateConverter {} },
                    }
                }
            }
        }
    }
}

/// A tab's title, with its explanation under it.
fn heading(i18n: &I18n, key: &str) -> Element {
    rsx! {
        h2 { class: "stats-section-title", {i18n.t(&format!("tools.{key}.title"))} }
        p { class: "tools-intro", {i18n.t(&format!("tools.{key}.intro"))} }
    }
}

// ── Date converter ──────────────────────────────────────────────────────

/// The calendars a date can be entered in, in the order the date input
/// offers them.
const CALENDARS: [(Calendar, &str); 4] = [
    (Calendar::Gregorian, "calendar.gregorian"),
    (Calendar::Julian, "calendar.julian"),
    (Calendar::Hebrew, "calendar.hebrew"),
    (Calendar::FrenchRepublican, "calendar.french_republican"),
];

/// The weekday of a single, complete date, Monday being 1: the same in every
/// calendar, since they all count the same days.
fn weekday(parts: &DateParts) -> Option<u8> {
    let resolved = parts.resolved();
    if resolved.needs_second_date() || resolved.qualifier != DateQualifier::Exact {
        return None;
    }
    let jdn = to_jdn(
        resolved.calendar,
        resolved.year?,
        resolved.month?,
        resolved.day?,
    )?;
    // Julian Day 0 fell on a Monday.
    Some(jdn.rem_euclid(7) as u8 + 1)
}

/// A date entered in one calendar, shown in every calendar the application
/// records dates in (`docs/ui-tools.md` §7).
#[component]
fn DateConverter() -> Element {
    let i18n = use_i18n();
    let parts = use_signal(DateParts::default);
    let entered = parts();
    let usable = !entered.is_empty() && entered.validate().is_none();
    let day_of_week = usable.then(|| weekday(&entered)).flatten();

    rsx! {
        section { class: "stats-section",
            {heading(&i18n, "converter")}
            div { class: "stats-card tools-converter-input",
                DateInput { parts, i18n, on_change: move |_| {} }
            }
            div { class: "stats-tiles tools-converter-results",
                for (calendar, key) in CALENDARS {
                    {
                        let converted = usable.then(|| entered.expressed_in(calendar)).flatten();
                        let text = match (usable, converted) {
                            (false, _) => "–".to_string(),
                            (true, Some(date)) => date.literal(&i18n),
                            (true, None) => i18n.t("tools.converter.unexpressible"),
                        };
                        rsx! {
                            div {
                                key: "{key}",
                                class: if calendar == entered.calendar { "stats-tile tools-converter-result active" } else { "stats-tile tools-converter-result" },
                                span { class: "stats-tile-label", {i18n.t(key)} }
                                span { class: "tools-converter-date", "{text}" }
                                if calendar == entered.calendar && usable {
                                    span { class: "stats-tile-detail", {i18n.t("tools.converter.entered")} }
                                }
                            }
                        }
                    }
                }
            }
            if let Some(day) = day_of_week {
                p { class: "tools-converter-weekday",
                    {i18n.t_args("tools.converter.weekday", &[("day", &i18n.t(&format!("stats.weekday.{day}")))])}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_complete_date_has_a_weekday_in_every_calendar() {
        // 14 July 1789 was a Tuesday; 25 messidor an II a Sunday
        // (13 July 1794).
        let gregorian = DateParts {
            year: Some(1789),
            month: Some(7),
            day: Some(14),
            ..Default::default()
        };
        assert_eq!(weekday(&gregorian), Some(2));
        let republican = DateParts {
            calendar: Calendar::FrenchRepublican,
            year: Some(2),
            month: Some(10),
            day: Some(25),
            ..Default::default()
        };
        assert_eq!(weekday(&republican), Some(7));
    }

    #[test]
    fn a_partial_or_uncertain_date_has_no_weekday() {
        let year_only = DateParts {
            year: Some(1789),
            ..Default::default()
        };
        assert_eq!(weekday(&year_only), None);
        let about = DateParts {
            qualifier: DateQualifier::About,
            year: Some(1789),
            month: Some(7),
            day: Some(14),
            ..Default::default()
        };
        assert_eq!(weekday(&about), None);
    }
}
