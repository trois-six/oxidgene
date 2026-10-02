//! Typeahead search component for finding and linking existing persons.
//!
//! Used in the UI for "Add Spouse", "Add Parents", "Add Child"
//! flows where the user can either create a new person or link to an existing one.
//!
//! Performance: uses the server-side `/persons/search?q=...` endpoint, backed
//! by the `person_search_fts` DB table (SQLite FTS5 / PostgreSQL) with
//! accent-folded matching, instead of downloading the full tree.

use dioxus::prelude::*;
use oxidgene_core::projection::{PersonProfile, ProfileEvent, SearchEntry};
use std::collections::HashMap;

use oxidgene_core::types::{PortraitRef, QualifiedYear, year_from_date};
use oxidgene_core::{DateQualifier, Sex};
use uuid::Uuid;

use crate::api::{ApiClient, CroppedSource};
use crate::components::cropped_image::CroppedImage;
use crate::components::event_icon::EventIcon;
use crate::components::pedigree_chart::lifespan_tooltip;
use crate::i18n::{I18n, use_i18n};
use crate::ui_observability::use_ui_resource;
use oxidgene_core::enums::EventType;

#[derive(Clone)]
pub(crate) struct PersonSearchSummary {
    person_id: Uuid,
    sex: Sex,
    surname: String,
    given_names: String,
    /// Birth (or baptism) and death (or burial) years with their precision:
    /// a year shown alone must not present a guess as a fact.
    birth_year: Option<QualifiedYear>,
    birth_place: Option<String>,
    death_year: Option<QualifiedYear>,
    /// Close relatives, so two people of the same name can be told apart.
    spouse_names: Vec<String>,
    father_name: Option<String>,
    mother_name: Option<String>,
    children_count: u32,
    portrait: Portrait,
}

impl PersonSearchSummary {
    pub(crate) fn person_id(&self) -> Uuid {
        self.person_id
    }

    pub(crate) fn sex(&self) -> Sex {
        self.sex
    }

    pub(crate) fn placeholder(person_id: Uuid, label: String) -> Self {
        Self {
            person_id,
            sex: Sex::Unknown,
            surname: String::new(),
            given_names: label,
            birth_year: None,
            birth_place: None,
            death_year: None,
            spouse_names: Vec::new(),
            father_name: None,
            mother_name: None,
            children_count: 0,
            portrait: Portrait::Unknown,
        }
    }
}

/// What a summary knows of its person's portrait.
#[derive(Clone)]
enum Portrait {
    /// Where it is drawn from, as a search row carries it.
    Known(PortraitRef),
    /// The person has none.
    None,
    /// Not said by what the summary was made from: asked by id.
    Unknown,
}

/// The portraits of `summaries`: those a search row carried resolved
/// straight away, the others asked for by id — one request each way at most.
pub(crate) async fn summary_portraits(
    api: &ApiClient,
    tree_id: Uuid,
    summaries: &[PersonSearchSummary],
) -> HashMap<Uuid, CroppedSource> {
    let refs: Vec<(Uuid, PortraitRef)> = summaries
        .iter()
        .filter_map(|summary| match &summary.portrait {
            Portrait::Known(portrait) => Some((summary.person_id, portrait.clone())),
            _ => None,
        })
        .collect();
    let unknown: Vec<Uuid> = summaries
        .iter()
        .filter(|summary| matches!(summary.portrait, Portrait::Unknown))
        .map(|summary| summary.person_id)
        .collect();
    let mut portraits = if refs.is_empty() {
        HashMap::new()
    } else {
        api.portraits_from_refs(tree_id, &refs).await
    };
    if !unknown.is_empty() {
        portraits.extend(api.portrait_map_for_ids(tree_id, &unknown).await);
    }
    portraits
}

impl From<&SearchEntry> for PersonSearchSummary {
    fn from(entry: &SearchEntry) -> Self {
        Self {
            person_id: entry.person_id,
            sex: entry.sex,
            surname: entry.surname.clone(),
            given_names: entry.given_names.clone(),
            birth_year: entry_year(entry.birth_year.as_deref(), entry.birth_qualifier),
            birth_place: entry.birth_place.clone(),
            death_year: entry_year(entry.death_year.as_deref(), entry.death_qualifier),
            spouse_names: entry.spouse_names.clone(),
            father_name: entry.father_name.clone(),
            mother_name: entry.mother_name.clone(),
            children_count: entry.children_count,
            portrait: entry
                .portrait
                .clone()
                .map_or(Portrait::None, Portrait::Known),
        }
    }
}

impl From<PersonProfile> for PersonSearchSummary {
    fn from(profile: PersonProfile) -> Self {
        let primary_name = profile.primary_name.as_ref();
        let child_link = profile.family_as_child.as_ref();
        Self {
            person_id: profile.person_id,
            sex: profile.sex,
            surname: primary_name
                .and_then(|name| name.surname.clone())
                .unwrap_or_default(),
            given_names: primary_name
                .and_then(|name| name.given_names.clone())
                .unwrap_or_default(),
            birth_year: profile_event_year(&profile.birth)
                .or_else(|| profile_event_year(&profile.baptism)),
            birth_place: profile
                .birth
                .as_ref()
                .and_then(|event| event.place_name.clone()),
            death_year: profile_event_year(&profile.death)
                .or_else(|| profile_event_year(&profile.burial)),
            spouse_names: profile
                .families_as_spouse
                .iter()
                .filter_map(|family| family.spouse_display_name.clone())
                .collect(),
            father_name: child_link.and_then(|link| link.father_display_name.clone()),
            mother_name: child_link.and_then(|link| link.mother_display_name.clone()),
            children_count: profile
                .families_as_spouse
                .iter()
                .map(|family| family.children_count)
                .sum(),
            // A profile does not say where its portrait is drawn from.
            portrait: Portrait::Unknown,
        }
    }
}

/// How a result names the person's place among their relatives.
///
/// A spouse identifies someone best, so it wins when there is one; parents are
/// the fallback, which is what distinguishes the children of a large family.
/// Returns `None` when neither is recorded, so the row simply omits the line
/// rather than reserving blank space for it.
fn relation_label(summary: &PersonSearchSummary, i18n: &I18n) -> Option<String> {
    let mut label = if !summary.spouse_names.is_empty() {
        let key = match summary.sex {
            Sex::Male => "search.relation_spouse_male",
            Sex::Female => "search.relation_spouse_female",
            Sex::Unknown => "search.relation_spouse",
        };
        i18n.t_args(key, &[("names", &summary.spouse_names.join(", "))])
    } else if summary.father_name.is_some() || summary.mother_name.is_some() {
        let key = match summary.sex {
            Sex::Male => "search.relation_child_male",
            Sex::Female => "search.relation_child_female",
            Sex::Unknown => "search.relation_child",
        };
        // Only one parent may be recorded; naming the known one beats
        // printing "child of X and —".
        match (&summary.father_name, &summary.mother_name) {
            (Some(father), Some(mother)) => i18n.t_args(
                key,
                &[(
                    "parents",
                    &format!("{father} {} {mother}", i18n.t("common.and")),
                )],
            ),
            (Some(parent), None) | (None, Some(parent)) => i18n.t_args(key, &[("parents", parent)]),
            (None, None) => unreachable!("guarded by the branch condition"),
        }
    } else {
        return None;
    };

    if summary.children_count > 0 {
        label.push_str(" – ");
        label.push_str(&i18n.t_plural("search.relation_children", summary.children_count as usize));
    }
    Some(label)
}

/// A search entry's year, which travels as text beside its precision.
fn entry_year(year: Option<&str>, qualifier: DateQualifier) -> Option<QualifiedYear> {
    year?
        .trim()
        .parse()
        .ok()
        .map(|year| QualifiedYear::new(year, qualifier))
}

/// An event's year with its precision, a range keeping its far end.
fn profile_event_year(event: &Option<ProfileEvent>) -> Option<QualifiedYear> {
    let event = event.as_ref()?;
    Some(QualifiedYear {
        year: year_from_date(event.date_sort, event.date_value.as_deref())?,
        qualifier: event.date_qualifier,
        year2: event
            .date_qualifier
            .needs_second_date()
            .then(|| year_from_date(None, event.date_value2.as_deref()))
            .flatten(),
    })
}

/// Props for [`SearchPerson`].
#[derive(Props, Clone, PartialEq)]
pub struct SearchPersonProps {
    /// Tree ID to search within.
    pub tree_id: Uuid,
    /// Placeholder text for the input.
    #[props(default = String::new())]
    pub placeholder: String,
    /// Called when the user selects a person from the results.
    pub on_select: EventHandler<Uuid>,
    /// Called when the user wants to cancel the search. Without it, no
    /// Cancel button: the caller's own dialog closes the search.
    #[props(default)]
    pub on_cancel: Option<EventHandler<()>>,
}

/// A typeahead search input that queries the server-side search index.
///
/// Keystroke input is debounced by 200 ms before the search request fires.
/// At most 20 results are fetched per query.
#[component]
pub fn SearchPerson(props: SearchPersonProps) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let mut query = use_signal(String::new);
    let tree_id = props.tree_id;

    let placeholder = if props.placeholder.is_empty() {
        i18n.t("search.placeholder")
    } else {
        props.placeholder.clone()
    };

    // Debounce: update the committed query after a short delay.
    let mut debounced_query = use_signal(String::new);
    let _debounce_task = use_ui_resource("search_debounce", move || {
        let raw = query();
        async move {
            crate::utils::sleep_ms(200).await;
            debounced_query.set(raw);
        }
    });

    // Server-side search: fires when debounced_query changes.
    let api_search = api.clone();
    let search_resource = use_ui_resource("search_people", move || {
        let api = api_search.clone();
        let q = debounced_query();
        async move {
            if q.is_empty() {
                // Empty query: return first 20 persons (no filter).
                return api.search_persons(tree_id, "", 20, 0).await;
            }
            api.search_persons(tree_id, &q, 20, 0).await
        }
    });

    let api_portraits = api.clone();
    let search_for_portraits = search_resource;
    let portraits_resource = use_ui_resource("search_portraits", move || {
        let api = api_portraits.clone();
        let entries = search_for_portraits
            .read()
            .as_ref()
            .and_then(|result| result.as_ref().ok())
            .map(|result| result.entries.clone())
            .unwrap_or_default();
        async move { api.entry_portraits(tree_id, &entries).await }
    });

    let results: Vec<SearchEntry> = {
        let data = search_resource.read();
        match &*data {
            Some(Ok(sr)) => sr.entries.clone(),
            _ => vec![],
        }
    };

    let is_loading = search_resource.read().is_none();
    let portraits = {
        let data = portraits_resource.read();
        match &*data {
            Some(urls) => urls.clone(),
            _ => Default::default(),
        }
    };

    rsx! {
        div { class: "search-person",
            div { class: "search-person-input-row",
                input {
                    r#type: "text",
                    placeholder: "{placeholder}",
                    value: "{query}",
                    oninput: move |e: Event<FormData>| query.set(e.value()),
                }
                if let Some(on_cancel) = props.on_cancel {
                    button {
                        class: "btn btn-outline btn-sm",
                        onclick: move |_| on_cancel.call(()),
                        {i18n.t("common.cancel")}
                    }
                }
            }

            if is_loading {
                div { class: "loading", {i18n.t("search.loading")} }
            } else if results.is_empty() {
                div { class: "text-muted", style: "padding: 8px;",
                    {i18n.t("search.no_match")}
                }
            } else {
                div { class: "search-person-results",
                    for entry in results.iter() {
                        {render_search_entry(
                            entry,
                            props.on_select,
                            portraits.get(&entry.person_id).cloned(),
                            &i18n,
                        )}
                    }
                }
            }
        }
    }
}

/// Render a single search result row.
fn render_search_entry(
    entry: &SearchEntry,
    on_select: EventHandler<Uuid>,
    portrait: Option<CroppedSource>,
    i18n: &I18n,
) -> Element {
    let summary = PersonSearchSummary::from(entry);
    let rid = summary.person_id;
    let sex_class = match summary.sex {
        Sex::Male => "male",
        Sex::Female => "female",
        Sex::Unknown => "",
    };

    rsx! {
        button {
            class: "search-person-result {sex_class}",
            onclick: move |_| on_select.call(rid),
            {render_person_search_summary(&summary, portrait, i18n)}
        }
    }
}

pub(crate) fn render_person_search_summary(
    summary: &PersonSearchSummary,
    portrait: Option<CroppedSource>,
    i18n: &I18n,
) -> Element {
    let given = &summary.given_names;
    let surname = &summary.surname;
    let relation = relation_label(summary, i18n);
    let portrait = portrait.unwrap_or_else(|| CroppedSource::silhouette(summary.sex));

    rsx! {
        div { class: "sp-result-photo",
            CroppedImage {
                class: "sp-result-portrait",
                image: portrait,
                alt: String::new(),
                fallback: CroppedSource::silhouette(summary.sex),
            }
        }
        div { class: "sp-result-info",
            div { class: "sp-result-name",
                if !surname.is_empty() {
                    span { class: "sp-surname", "{surname}" }
                }
                span { class: "sp-given", " {given}" }
                if surname.is_empty() && given.is_empty() {
                    span { class: "sp-given", "?" }
                }
            }
            div {
                class: "sp-result-dates",
                // The marks (`ca`, `<`, `..`) spelled out, as on a pedigree card.
                title: lifespan_tooltip(i18n, summary.birth_year, summary.death_year),
                if let Some(ref birth_year) = summary.birth_year {
                    span { class: "sp-birth",
                        EventIcon { event_type: EventType::Birth, bare: true }
                        " {birth_year}"
                    }
                }
                if let Some(ref death_year) = summary.death_year {
                    span { class: "sp-death",
                        EventIcon { event_type: EventType::Death, bare: true }
                        " {death_year}"
                    }
                }
            }
            if let Some(relation) = relation {
                div { class: "sp-result-rel", "{relation}" }
            }
            if let Some(ref birth_place) = summary.birth_place {
                div { class: "sp-result-meta",
                    span { class: "sp-place", "{birth_place}" }
                }
            }
        }
    }
}

#[cfg(test)]
mod relation_tests {
    use super::*;
    use crate::i18n::Language;

    fn summary(sex: Sex) -> PersonSearchSummary {
        PersonSearchSummary {
            person_id: Uuid::now_v7(),
            sex,
            surname: "Branch A".into(),
            given_names: "Child One".into(),
            birth_year: None,
            birth_place: None,
            death_year: None,
            spouse_names: Vec::new(),
            father_name: None,
            mother_name: None,
            children_count: 0,
            portrait: Portrait::None,
        }
    }

    #[test]
    fn a_year_keeps_its_precision() {
        assert_eq!(
            entry_year(Some("1849"), DateQualifier::About).map(|y| y.to_string()),
            Some("ca 1849".to_string())
        );
        assert_eq!(
            entry_year(Some("1917"), DateQualifier::Before).map(|y| y.to_string()),
            Some("< 1917".to_string())
        );
        assert_eq!(
            entry_year(Some("1850"), DateQualifier::Exact).map(|y| y.to_string()),
            Some("1850".to_string())
        );
        assert_eq!(entry_year(None, DateQualifier::About), None);
    }

    #[test]
    fn a_spouse_identifies_someone_better_than_their_parents() {
        let en = I18n::new(Language::english());
        let mut s = summary(Sex::Male);
        s.father_name = Some("Parent One".into());
        s.mother_name = Some("Parent Two".into());
        assert_eq!(
            relation_label(&s, &en).as_deref(),
            Some("son of Parent One and Parent Two")
        );

        s.spouse_names = vec!["Spouse One".into()];
        assert_eq!(
            relation_label(&s, &en).as_deref(),
            Some("married to Spouse One"),
            "a spouse takes precedence over the parents"
        );
    }

    #[test]
    fn a_single_known_parent_is_named_alone() {
        // "son of Parent One and —" would be worse than naming the one parent
        // the record actually has.
        let en = I18n::new(Language::english());
        let mut s = summary(Sex::Female);
        s.mother_name = Some("Parent Two".into());
        assert_eq!(
            relation_label(&s, &en).as_deref(),
            Some("daughter of Parent Two")
        );
    }

    #[test]
    fn sex_picks_the_wording_and_unknown_stays_neutral() {
        let fr = I18n::new(Language::try_from_code("fr").unwrap());
        let mut s = summary(Sex::Unknown);
        s.father_name = Some("Parent One".into());
        assert_eq!(
            relation_label(&s, &fr).as_deref(),
            Some("enfant de Parent One")
        );

        s.sex = Sex::Female;
        assert_eq!(
            relation_label(&s, &fr).as_deref(),
            Some("fille de Parent One")
        );
    }

    #[test]
    fn several_spouses_are_all_named() {
        let en = I18n::new(Language::english());
        let mut s = summary(Sex::Male);
        s.spouse_names = vec!["Spouse One".into(), "Spouse Two".into()];
        assert_eq!(
            relation_label(&s, &en).as_deref(),
            Some("married to Spouse One, Spouse Two")
        );
    }

    #[test]
    fn the_children_count_is_pluralised_and_omitted_at_zero() {
        let en = I18n::new(Language::english());
        let mut s = summary(Sex::Male);
        s.spouse_names = vec!["Spouse One".into()];
        assert_eq!(
            relation_label(&s, &en).as_deref(),
            Some("married to Spouse One")
        );

        s.children_count = 1;
        assert_eq!(
            relation_label(&s, &en).as_deref(),
            Some("married to Spouse One – 1 child")
        );

        s.children_count = 3;
        assert_eq!(
            relation_label(&s, &en).as_deref(),
            Some("married to Spouse One – 3 children")
        );
    }

    #[test]
    fn nothing_recorded_draws_no_line() {
        // `None` rather than an empty string: the row omits the element
        // instead of reserving a blank line for it.
        assert!(relation_label(&summary(Sex::Male), &I18n::new(Language::english())).is_none());
    }
}
