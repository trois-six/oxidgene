//! The persons a viewer sees as placeholders (docs/privacy.md §5,
//! docs/authorization.md §6.5 "The boundary"): what stands for their name,
//! and whether they may be opened.
//!
//! The server marks each placeholder it sends — a pedigree node or family
//! member, a person record of a page bundle, a kinship row — with its
//! [`Placeholder`]; a person shown in full carries none, so the desktop and
//! an owner never meet one. Every view asks this module, through the data it
//! already loaded, rather than deciding on its own:
//!
//! - **the name**: a *Not shared* relative has none and shows the label *Not
//!   shared*; a *Private* person shows the initials the server sent, else the
//!   label *Private* ([`label`], [`label_names`]);
//! - **navigation**: a *Not shared* relative is never opened, re-rooted on or
//!   offered a menu — its identifier is a pseudonym of the request; a
//!   *Private* one keeps the navigation the tree allows ([`navigable`],
//!   [`Placeholders`]).

use std::collections::HashMap;

pub use oxidgene_core::projection::Placeholder;
use oxidgene_core::types::{Person, PersonName};
use uuid::Uuid;

use crate::i18n::I18n;

/// The translation key of the label standing for the name of a person with
/// `placeholder`, who has a name of their own or not (`named`); `None` when
/// the person's own name, or the usual fallback, is shown.
pub fn label_key(placeholder: Option<Placeholder>, named: bool) -> Option<&'static str> {
    match placeholder {
        // No name ever comes with one: the label stands whatever is there.
        Some(Placeholder::NotShared) => Some("person.not_shared"),
        // The `initials` mode names them by their initials.
        Some(Placeholder::Private) if !named => Some("person.private"),
        _ => None,
    }
}

/// The label standing for the name of a person with `placeholder`, in the
/// reader's language; `None` when their own name is shown.
pub fn label(placeholder: Option<Placeholder>, named: bool, i18n: &I18n) -> Option<String> {
    label_key(placeholder, named).map(|key| i18n.t(key))
}

/// The name a person with `placeholder` shows: the label, or `name`.
pub fn shown_name(placeholder: Option<Placeholder>, name: String, i18n: &I18n) -> String {
    label(placeholder, !name.trim().is_empty(), i18n).unwrap_or(name)
}

/// Whether a person with `placeholder` may be opened, chosen as a chart's
/// focus or offered a menu: never a *Not shared* relative.
pub fn navigable(placeholder: Option<Placeholder>) -> bool {
    placeholder != Some(Placeholder::NotShared)
}

/// What a view loaded that says which of its persons are placeholders.
pub trait Placeholders {
    /// The placeholder `person_id` is, if they are one.
    fn placeholder(&self, person_id: Uuid) -> Option<Placeholder>;

    /// Whether `person_id` may be opened (see [`navigable`]).
    fn navigable(&self, person_id: Uuid) -> bool {
        navigable(self.placeholder(person_id))
    }
}

/// The person records of a pedigree or a page bundle.
impl Placeholders for HashMap<Uuid, Person> {
    fn placeholder(&self, person_id: Uuid) -> Option<Placeholder> {
        self.get(&person_id).and_then(|person| person.placeholder)
    }
}

/// The placeholders of a view, by person.
impl Placeholders for HashMap<Uuid, Placeholder> {
    fn placeholder(&self, person_id: Uuid) -> Option<Placeholder> {
        self.get(&person_id).copied()
    }
}

/// Writes into `names` the label of every person of `placeholders` whose
/// name it stands for, as their one primary name: the name resolution of
/// every view then reads it like any other, in the reader's language.
pub fn label_names(
    names: &mut HashMap<Uuid, Vec<PersonName>>,
    placeholders: impl IntoIterator<Item = (Uuid, Placeholder)>,
    i18n: &I18n,
) {
    for (person_id, placeholder) in placeholders {
        let named = names
            .get(&person_id)
            .and_then(|names| PersonName::primary(names.as_slice()))
            .is_some_and(|name| !name.display_name().is_empty());
        if let Some(label) = label(Some(placeholder), named, i18n) {
            names.insert(person_id, vec![label_name(person_id, label)]);
        }
    }
}

/// A primary name made of `label` alone.
fn label_name(person_id: Uuid, label: String) -> PersonName {
    let epoch = chrono::DateTime::<chrono::Utc>::default();
    PersonName {
        id: Uuid::nil(),
        person_id,
        name_type: oxidgene_core::NameType::Birth,
        given_names: Some(label),
        surname: None,
        surname_prefix: None,
        prefix: None,
        suffix: None,
        nickname: None,
        is_primary: true,
        sort_order: 0,
        created_at: epoch,
        updated_at: epoch,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Language;

    fn named(person_id: Uuid, given: &str, surname: &str) -> PersonName {
        PersonName {
            given_names: Some(given.to_string()),
            surname: Some(surname.to_string()),
            ..label_name(person_id, String::new())
        }
    }

    #[test]
    fn a_relative_not_shared_is_labelled_whatever_it_carries() {
        assert_eq!(
            label_key(Some(Placeholder::NotShared), false),
            Some("person.not_shared")
        );
        assert_eq!(
            label_key(Some(Placeholder::NotShared), true),
            Some("person.not_shared")
        );
    }

    #[test]
    fn a_private_person_shows_their_initials_else_the_label() {
        assert_eq!(
            label_key(Some(Placeholder::Private), false),
            Some("person.private")
        );
        assert_eq!(label_key(Some(Placeholder::Private), true), None);
        // A person shown in full keeps their name, or the usual fallback.
        assert_eq!(label_key(None, false), None);
        assert_eq!(label_key(None, true), None);
    }

    #[test]
    fn only_a_relative_not_shared_is_never_opened() {
        assert!(!navigable(Some(Placeholder::NotShared)));
        assert!(navigable(Some(Placeholder::Private)));
        assert!(navigable(None));
        let (outside, private, shown) =
            (Uuid::from_u128(1), Uuid::from_u128(2), Uuid::from_u128(3));
        let marks = HashMap::from([
            (outside, Placeholder::NotShared),
            (private, Placeholder::Private),
        ]);
        assert!(!marks.navigable(outside));
        assert!(marks.navigable(private));
        assert!(marks.navigable(shown));
    }

    #[test]
    fn the_labels_are_written_as_names_in_the_reader_s_language() {
        let i18n = I18n::new(Language::english());
        let (outside, private, initials) =
            (Uuid::from_u128(1), Uuid::from_u128(2), Uuid::from_u128(3));
        let mut names = HashMap::from([(initials, vec![named(initials, "A.", "F.")])]);
        label_names(
            &mut names,
            [
                (outside, Placeholder::NotShared),
                (private, Placeholder::Private),
                (initials, Placeholder::Private),
            ],
            &i18n,
        );
        let shown = |id| crate::utils::resolve_name(id, &names, &i18n);
        assert_eq!(shown(outside), "Not shared");
        assert_eq!(shown(private), "Private");
        assert_eq!(shown(initials), "A. F.");
        assert_eq!(
            shown_name(Some(Placeholder::NotShared), String::new(), &i18n),
            "Not shared"
        );
        assert_eq!(shown_name(None, "Aldric".into(), &i18n), "Aldric");
    }
}
