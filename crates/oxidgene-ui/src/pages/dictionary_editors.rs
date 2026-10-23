//! The Dictionary's bulk editors of occupations and places
//! (`docs/ui-dictionary.md` §9.1 and §10.1): an occupation renamed on every
//! event labelled with it, a place renamed, moved, or merged into the place
//! its new name matches. The family-name editor lives with its tab, and the
//! repository editor with the source forms.

use dioxus::prelude::*;
use oxidgene_core::types::Place;
use uuid::Uuid;

use crate::api::{
    ApiClient, DictionaryEntry, MergePlaceBody, PlaceDictionaryEntry, SuggestionField,
    UpdatePlaceBody,
};
use crate::components::merge_fields::{
    MergeCandidate, MergeField, MergeOffer, MergePicks, merged_value, same_name, use_merge_offer,
};
use crate::components::modal::Modal;
use crate::components::suggest_input::ValueInput;
use crate::i18n::{I18n, use_i18n};

/// The header of a bulk editor: its title and its close button.
fn editor_header(title: String, on_close: EventHandler<()>) -> Element {
    rsx! {
        div { class: "dict-edit-header",
            h2 { "{title}" }
            button { class: "person-form-close", onclick: move |_| on_close.call(()), "✕" }
        }
    }
}

/// The occupation editor: renames an occupation on every event labelled
/// with it, which joins the occupation its new label already names.
#[component]
pub fn OccupationEditor(
    tree_id: Uuid,
    entry: DictionaryEntry,
    /// Every listed occupation, to tell a rename from a merge.
    known: Vec<DictionaryEntry>,
    on_close: EventHandler<()>,
    on_saved: EventHandler<()>,
) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let label = use_signal(|| entry.value.clone());
    let mut error = use_signal(|| None::<String>);
    let mut saving = use_signal(|| false);

    let new_value = label().trim().to_string();
    let renaming = !new_value.is_empty() && new_value != entry.value;
    let target = renaming
        .then(|| known.iter().find(|e| e.value == new_value).cloned())
        .flatten();
    let apply = {
        let value = entry.value.clone();
        let new_value = new_value.clone();
        move |_| {
            let (api, value, new_value) = (api.clone(), value.clone(), new_value.clone());
            saving.set(true);
            error.set(None);
            spawn(async move {
                match api.rename_occupation(tree_id, &value, &new_value).await {
                    Ok(_) => on_saved.call(()),
                    Err(e) => {
                        saving.set(false);
                        error.set(Some(e.to_string()));
                    }
                }
            });
        }
    };
    let title = i18n.t("dictionary.occupation.title");

    rsx! {
        Modal { class: "dict-edit-modal", label: title.clone(), busy: saving(), on_close,
            {editor_header(title, on_close)}
            div { class: "dict-particle-body",
                p { class: "dict-particle-intro",
                    {i18n.t_args("dictionary.occupation.intro", &[("name", &entry.value)])}
                }
                div { class: "form-group",
                    label { {i18n.t("dictionary.occupation.label")} }
                    ValueInput {
                        value: label,
                        tree_id,
                        field: SuggestionField::Occupations,
                        on_change: move |()| error.set(None),
                    }
                }
                if renaming {
                    p { class: "dict-particle-scope",
                        {i18n.t_plural("dictionary.occupation.scope", entry.count as usize)}
                    }
                    if let Some(target) = &target {
                        p { class: "field-hint field-hint-warn",
                            {i18n.t_args(
                                &i18n.plural_key("dictionary.occupation.merge", target.count as usize),
                                &[("name", &target.value), ("count", &target.count.to_string())],
                            )}
                        }
                    }
                }
                if let Some(err) = error() {
                    div { class: "error-msg", "{err}" }
                }
            }
            div { class: "modal-actions",
                button { class: "td-btn", onclick: move |_| on_close.call(()), {i18n.t("common.cancel")} }
                button {
                    class: "td-btn td-btn-primary",
                    disabled: !renaming || saving(),
                    onclick: apply,
                    if saving() { {i18n.t("common.saving")} } else { {i18n.t("dictionary.occupation.rename")} }
                }
            }
        }
    }
}

/// A coordinate as typed: empty is none, anything else must be a number.
fn coordinate(text: &str) -> Result<Option<f64>, ()> {
    let text = text.trim().replace(',', ".");
    if text.is_empty() {
        return Ok(None);
    }
    text.parse::<f64>().map(Some).map_err(|_| ())
}

/// A place's coordinates as the merge compares them, `"lat, lon"`, or empty.
fn coordinates_text(latitude: Option<f64>, longitude: Option<f64>) -> String {
    match (latitude, longitude) {
        (Some(lat), Some(lon)) => format!("{lat}, {lon}"),
        _ => String::new(),
    }
}

/// The coordinates a merge keeps: the picked place's, else the other's.
fn merged_coordinates(
    edited: (Option<f64>, Option<f64>),
    other: &Place,
    from_edited: bool,
) -> (Option<f64>, Option<f64>) {
    let theirs = (other.latitude, other.longitude);
    let has = |c: (Option<f64>, Option<f64>)| c.0.is_some() && c.1.is_some();
    match (from_edited, has(edited), has(theirs)) {
        (true, true, _) | (false, true, false) => edited,
        _ => theirs,
    }
}

/// The places of `known` other than `place` whose name matches `name`, as
/// merge candidates named with their use count.
fn place_candidates(
    i18n: I18n,
    known: &[PlaceDictionaryEntry],
    place: &Place,
    name: &str,
) -> Vec<MergeCandidate> {
    known
        .iter()
        .filter(|e| e.place.id != place.id && same_name(&e.place.name, name))
        .map(|e| MergeCandidate {
            id: e.place.id,
            name: e.place.name.clone(),
            option: i18n.t_args(
                &i18n.plural_key("dictionary.merge.option", e.count as usize),
                &[("name", &e.place.name), ("count", &e.count.to_string())],
            ),
        })
        .collect()
}

/// The fields a place merge compares: the name, and the coordinates as a
/// pair, the edited place's as typed (none while they do not parse).
fn place_fields(
    i18n: I18n,
    name: &str,
    coordinates: (Result<Option<f64>, ()>, Result<Option<f64>, ()>),
    target: &Place,
) -> Vec<MergeField> {
    let edited = match coordinates {
        (Ok(lat), Ok(lon)) => coordinates_text(lat, lon),
        _ => String::new(),
    };
    vec![
        MergeField {
            key: "name",
            label: i18n.t("dictionary.place.name"),
            values: [name.trim().to_string(), target.name.clone()],
        },
        MergeField {
            key: "coordinates",
            label: i18n.t("dictionary.place.coordinates"),
            values: [edited, coordinates_text(target.latitude, target.longitude)],
        },
    ]
}

/// The merge of the edited place, its coordinates `edited`, into `target`,
/// with the fields picked.
fn place_merge(
    target: &Place,
    fields: &[MergeField],
    picks: &MergePicks,
    edited: (Option<f64>, Option<f64>),
) -> MergePlaceBody {
    let from_edited = picks.get("coordinates").copied().unwrap_or(1) == 0;
    let (latitude, longitude) = merged_coordinates(edited, target, from_edited);
    MergePlaceBody {
        into: target.id,
        name: merged_value(&fields[0], picks),
        latitude,
        longitude,
    }
}

/// The place editor: renames a place and sets its coordinates, which every
/// event and media at it shows; when its name matches another place of the
/// tree, it can merge into it, choosing field by field what the merged
/// place keeps.
#[component]
pub fn PlaceEditor(
    tree_id: Uuid,
    place: Place,
    /// Every place of the tree with its use count, to find the one a name
    /// matches.
    known: Vec<PlaceDictionaryEntry>,
    on_close: EventHandler<()>,
    on_saved: EventHandler<()>,
) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let name = use_signal(|| place.name.clone());
    let mut latitude = use_signal(|| place.latitude.map(|v| v.to_string()).unwrap_or_default());
    let mut longitude = use_signal(|| place.longitude.map(|v| v.to_string()).unwrap_or_default());
    let offer = use_merge_offer();
    let mut error = use_signal(|| None::<String>);
    let mut saving = use_signal(|| false);

    let candidates = place_candidates(i18n, &known, &place, &name());
    let target: Option<PlaceDictionaryEntry> = offer
        .target(&candidates)
        .and_then(|candidate| known.iter().find(|e| e.place.id == candidate.id))
        .cloned();
    let merging = offer.merging(target.is_some(), name().trim() != place.name.trim());
    let coordinates = (coordinate(&latitude()), coordinate(&longitude()));
    let fields = target
        .as_ref()
        .map(|target| place_fields(i18n, &name(), coordinates, &target.place))
        .unwrap_or_default();
    let warning = target.as_ref().map(|target| {
        i18n.t_args(
            &i18n.plural_key("dictionary.place.exists", target.count as usize),
            &[
                ("name", &target.place.name),
                ("count", &target.count.to_string()),
            ],
        )
    });

    let save = {
        let id = place.id;
        let target = target.clone().filter(|_| merging);
        let fields = fields.clone();
        move |_| {
            let (Ok(lat), Ok(lon)) = coordinates else {
                error.set(Some(i18n.t("dictionary.place.coordinates_invalid")));
                return;
            };
            if name().trim().is_empty() {
                error.set(Some(i18n.t("dictionary.place.name_required")));
                return;
            }
            let api = api.clone();
            let merge = target
                .as_ref()
                .map(|target| place_merge(&target.place, &fields, &offer.picks.read(), (lat, lon)));
            let update = UpdatePlaceBody {
                name: Some(name().trim().to_string()),
                latitude: Some(lat),
                longitude: Some(lon),
            };
            saving.set(true);
            error.set(None);
            spawn(async move {
                let result = match merge {
                    Some(body) => api.merge_place(tree_id, id, &body).await,
                    None => api.update_place(tree_id, id, &update).await,
                };
                match result {
                    Ok(_) => on_saved.call(()),
                    Err(e) => {
                        saving.set(false);
                        error.set(Some(e.to_string()));
                    }
                }
            });
        }
    };
    let title = i18n.t("dictionary.place.title");

    rsx! {
        Modal { class: "dict-edit-modal", label: title.clone(), busy: saving(), on_close,
            {editor_header(title, on_close)}
            div { class: "dict-particle-body",
                p { class: "dict-particle-intro", {i18n.t("dictionary.place.intro")} }
                div { class: "form-group",
                    label { {i18n.t("dictionary.place.name")} }
                    ValueInput {
                        value: name,
                        tree_id,
                        field: SuggestionField::Places,
                        tree_only: true,
                        on_change: move |()| {
                            offer.reset();
                            error.set(None);
                        },
                    }
                }
                div { class: "form-row",
                    div { class: "form-group",
                        label { {i18n.t("dictionary.place.latitude")} }
                        input {
                            r#type: "text",
                            inputmode: "decimal",
                            value: "{latitude}",
                            oninput: move |e: Event<FormData>| latitude.set(e.value()),
                        }
                    }
                    div { class: "form-group",
                        label { {i18n.t("dictionary.place.longitude")} }
                        input {
                            r#type: "text",
                            inputmode: "decimal",
                            value: "{longitude}",
                            oninput: move |e: Event<FormData>| longitude.set(e.value()),
                        }
                    }
                }
                if let Some(warning) = warning {
                    MergeOffer { state: offer, warning, candidates, merging, fields }
                }
                if let Some(err) = error() {
                    div { class: "error-msg", "{err}" }
                }
            }
            div { class: "modal-actions",
                button { class: "td-btn", onclick: move |_| on_close.call(()), {i18n.t("common.cancel")} }
                button {
                    class: "td-btn td-btn-primary",
                    disabled: saving(),
                    onclick: save,
                    if saving() {
                        {i18n.t("common.saving")}
                    } else if merging {
                        {i18n.t("dictionary.merge.apply")}
                    } else {
                        {i18n.t("common.save")}
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn place(latitude: Option<f64>, longitude: Option<f64>) -> Place {
        Place {
            id: Uuid::nil(),
            tree_id: Uuid::nil(),
            name: "Northfield".to_string(),
            latitude,
            longitude,
            tags: Vec::new(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn a_coordinate_is_a_number_or_nothing() {
        assert_eq!(coordinate(" 47,25 "), Ok(Some(47.25)));
        assert_eq!(coordinate(""), Ok(None));
        assert_eq!(coordinate("north"), Err(()));
    }

    #[test]
    fn a_merge_keeps_the_coordinates_picked_or_the_only_ones() {
        let other = place(Some(1.0), Some(2.0));
        let edited = (Some(3.0), Some(4.0));
        assert_eq!(
            merged_coordinates(edited, &other, false),
            (Some(1.0), Some(2.0))
        );
        assert_eq!(
            merged_coordinates(edited, &other, true),
            (Some(3.0), Some(4.0))
        );
        // A place without coordinates takes the other's, whichever is picked.
        assert_eq!(
            merged_coordinates((None, None), &other, true),
            (Some(1.0), Some(2.0))
        );
        assert_eq!(
            merged_coordinates(edited, &place(None, None), false),
            (Some(3.0), Some(4.0))
        );
    }
}
