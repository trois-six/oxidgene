//! The merge of a record into another of its kind — a place, a repository —
//! from its editor (`docs/ui-dictionary.md` §9.1 and §19): the record its
//! name matches, offered as the one to merge into, and the choice, field by
//! field, of which record's value the merged record keeps.

use std::collections::HashMap;

use dioxus::prelude::*;
use oxidgene_core::search::fold_words;
use uuid::Uuid;

use crate::i18n::use_i18n;

/// One field the merge compares: its label and the two records' values, the
/// edited record's first.
#[derive(Debug, Clone, PartialEq)]
pub struct MergeField {
    pub key: &'static str,
    pub label: String,
    pub values: [String; 2],
}

impl MergeField {
    /// Whether the user chooses this field: both records fill it, and not
    /// alike.
    fn is_choice(&self) -> bool {
        let [edited, other] = self.values.each_ref().map(|value| value.trim());
        !edited.is_empty() && !other.is_empty() && edited != other
    }

    /// Whether the two records hold it differently, one of them perhaps not
    /// at all.
    fn differs(&self) -> bool {
        self.values[0].trim() != self.values[1].trim()
    }
}

/// The record each field is taken from, by field key: `0` the edited
/// record, `1` the one it merges into, which a field not picked keeps.
pub type MergePicks = HashMap<&'static str, usize>;

/// The value the merged record keeps for `field`: the picked record's, or
/// the other's when the picked record leaves it empty.
pub fn merged_value(field: &MergeField, picks: &MergePicks) -> String {
    let picked = picks.get(field.key).copied().unwrap_or(1).min(1);
    let value = field.values[picked].trim();
    if value.is_empty() {
        field.values[1 - picked].trim().to_string()
    } else {
        value.to_string()
    }
}

/// Whether two names name the same record: alike once trimmed, ignoring
/// case and accents, as a typed place or repository is looked up.
pub fn same_name(a: &str, b: &str) -> bool {
    let (a, b) = (a.trim(), b.trim());
    !a.is_empty() && fold_words(a) == fold_words(b)
}

/// A record the edited record's name matches, offered to merge into.
#[derive(Debug, Clone, PartialEq)]
pub struct MergeCandidate {
    pub id: Uuid,
    pub name: String,
    /// How the choice among several names it: its name and what tells it
    /// apart.
    pub option: String,
}

/// What an editor's merge offer holds: whether to merge, into which
/// candidate, and the record each field is taken from.
#[derive(Clone, Copy, PartialEq)]
pub struct MergeOfferState {
    choice: Signal<Option<bool>>,
    chosen: Signal<Option<Uuid>>,
    pub picks: Signal<MergePicks>,
}

/// The state of an editor's merge offer, nothing chosen yet.
pub fn use_merge_offer() -> MergeOfferState {
    MergeOfferState {
        choice: use_signal(|| None),
        chosen: use_signal(|| None),
        picks: use_signal(MergePicks::new),
    }
}

impl MergeOfferState {
    /// The candidate merged into: the one chosen, else the first.
    pub fn target<'a>(&self, candidates: &'a [MergeCandidate]) -> Option<&'a MergeCandidate> {
        (self.chosen)()
            .and_then(|id| candidates.iter().find(|candidate| candidate.id == id))
            .or(candidates.first())
    }

    /// Whether the edit merges into a candidate: proposed when the name was
    /// changed into the candidate's, left to the user when the two already
    /// shared it, as two places may share a name and be two places.
    pub fn merging(&self, has_target: bool, renamed: bool) -> bool {
        has_target && (self.choice)().unwrap_or(renamed)
    }

    /// Forgets the choices, which belong to the name they were made for.
    pub fn reset(mut self) {
        self.choice.set(None);
        self.chosen.set(None);
    }
}

/// An editor's offer to merge into the record its name matches: the
/// `warning` naming it, a choice among several `candidates`, the box that
/// decides, and while merging the fields to pick ([`MergeFields`]).
#[component]
pub fn MergeOffer(
    state: MergeOfferState,
    warning: String,
    candidates: Vec<MergeCandidate>,
    merging: bool,
    fields: Vec<MergeField>,
) -> Element {
    let i18n = use_i18n();
    let MergeOfferState {
        mut choice,
        mut chosen,
        picks,
    } = state;
    let Some(target) = state.target(&candidates).cloned() else {
        return rsx! {};
    };
    rsx! {
        p { class: "field-hint field-hint-warn", "{warning}" }
        if candidates.len() > 1 {
            div { class: "form-group",
                label { {i18n.t("dictionary.merge.into")} }
                select {
                    value: "{target.id}",
                    onchange: move |e: Event<FormData>| chosen.set(e.value().parse().ok()),
                    for candidate in candidates.iter() {
                        option { key: "{candidate.id}", value: "{candidate.id}", "{candidate.option}" }
                    }
                }
            }
        }
        label { class: "dict-merge-toggle",
            input {
                r#type: "checkbox",
                checked: merging,
                onchange: move |e: Event<FormData>| choice.set(Some(e.checked())),
            }
            {i18n.t_args("dictionary.merge.toggle", &[("name", &target.name)])}
        }
        if merging {
            MergeFields {
                columns: [i18n.t("dictionary.merge.this_one"), target.name.clone()],
                fields,
                picks,
            }
        }
    }
}

/// The fields the two records hold differently, as a table: a radio on
/// each record's value when both fill it, the one value otherwise, which
/// the merged record keeps. `columns` names the two records, the edited
/// one first.
#[component]
pub fn MergeFields(
    columns: [String; 2],
    fields: Vec<MergeField>,
    picks: Signal<MergePicks>,
) -> Element {
    let i18n = use_i18n();
    let differing: Vec<MergeField> = fields.into_iter().filter(MergeField::differs).collect();
    if differing.is_empty() {
        return rsx! {
            p { class: "dict-particle-hint", {i18n.t("dictionary.merge.same_details")} }
        };
    }
    rsx! {
        p { class: "dict-particle-hint", {i18n.t("dictionary.merge.pick_hint")} }
        table { class: "merge-compare-table",
            thead {
                tr {
                    th {}
                    for column in columns.iter() {
                        th { key: "{column}", "{column}" }
                    }
                }
            }
            tbody {
                for field in differing {
                    tr { key: "{field.key}", class: "merge-differs",
                        th { "{field.label}" }
                        for (index , value) in field.values.iter().enumerate() {
                            td { key: "{index}",
                                if field.is_choice() {
                                    label { class: "merge-pick",
                                        input {
                                            r#type: "radio",
                                            name: "merge-field-{field.key}",
                                            checked: picks.read().get(field.key).copied().unwrap_or(1) == index,
                                            onchange: {
                                                let key = field.key;
                                                move |_| {
                                                    picks.write().insert(key, index);
                                                }
                                            },
                                        }
                                        span { "{value}" }
                                    }
                                } else if value.trim().is_empty() {
                                    "\u{2014}"
                                } else {
                                    "{value}"
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(edited: &str, other: &str) -> MergeField {
        MergeField {
            key: "name",
            label: "Name".to_string(),
            values: [edited.to_string(), other.to_string()],
        }
    }

    #[test]
    fn a_field_not_picked_keeps_the_record_merged_into() {
        let picks = MergePicks::new();
        assert_eq!(merged_value(&field("Alpha", "Beta"), &picks), "Beta");
        let picks = MergePicks::from([("name", 0)]);
        assert_eq!(merged_value(&field("Alpha", "Beta"), &picks), "Alpha");
    }

    #[test]
    fn a_field_only_one_record_fills_is_no_choice_and_keeps_that_value() {
        let one_sided = field("", "Beta");
        assert!(!one_sided.is_choice());
        assert!(one_sided.differs());
        let picks = MergePicks::from([("name", 0)]);
        assert_eq!(merged_value(&one_sided, &picks), "Beta");
        assert_eq!(
            merged_value(&field("Alpha ", "  "), &MergePicks::new()),
            "Alpha"
        );
    }

    #[test]
    fn names_match_ignoring_case_accents_and_spaces() {
        assert!(same_name("Ville-Fictive ", "VILLE-FICTIVE"));
        assert!(same_name("Écluse", "ecluse"));
        assert!(!same_name("Alpha", "Alphaville"));
        assert!(!same_name(" ", ""));
    }
}
