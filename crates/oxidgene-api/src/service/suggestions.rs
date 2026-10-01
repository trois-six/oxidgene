//! Suggestions for the free-text fields of the entry forms: the values a
//! tree already holds, then, for occupations and given names, the terms the
//! reference sheets answer to. See `docs/api.md` (value suggestions).

use std::collections::{HashMap, HashSet};

use oxidgene_core::OxidGeneError;
use oxidgene_db::repo::{DictionaryRepo, PersonSearchRepo};
use sea_orm::ConnectionTrait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::reference::{ReferenceKind, ReferenceLang, has_sheet, starts_a_word, suggest_terms};
use oxidgene_core::search::fold_words;

pub const DEFAULT_VALUE_SUGGESTIONS: usize = 10;
pub const MAX_VALUE_SUGGESTIONS: usize = 50;

/// The field a suggestion is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SuggestionField {
    FamilyNames,
    /// One given name, not the whole field: the form completes the word
    /// being typed.
    GivenNames,
    Occupations,
    /// Source titles.
    Sources,
}

impl SuggestionField {
    fn reference(self) -> Option<ReferenceKind> {
        match self {
            Self::GivenNames => Some(ReferenceKind::GivenNames),
            Self::Occupations => Some(ReferenceKind::Occupations),
            Self::FamilyNames | Self::Sources => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ValueSuggestion {
    pub value: String,
    /// Persons carrying the value, or citations of the source; 0 for a term
    /// only a reference sheet knows.
    pub count: i64,
    /// Whether a reference sheet answers to the value itself.
    pub reference: bool,
}

/// The persons a name suggestion counts, when not the whole tree: those the
/// person search's `surname` and `given_names` filters find. A search form
/// passes what its other name field holds, so each suggestion counts the
/// persons its search would then find.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct NameScope {
    pub surname: Option<String>,
    pub given_names: Option<String>,
}

impl NameScope {
    fn is_empty(&self) -> bool {
        [&self.surname, &self.given_names]
            .iter()
            .all(|value| value.as_deref().is_none_or(|v| v.trim().is_empty()))
    }
}

pub const SCOPE_NAMES_ONLY: &str = "surname and given_names only scope name suggestions";

/// Up to `limit` suggestions with a word starting with `query`, ignoring
/// case, accents and punctuation: the tree's values first — those starting
/// with the query, then the most used — then the reference terms the tree
/// does not hold yet. A `scope` counts and lists only the values of the
/// persons it finds, and adds no reference term.
pub async fn suggest(
    db: &impl ConnectionTrait,
    tree_id: Uuid,
    field: SuggestionField,
    language: &str,
    query: &str,
    limit: Option<usize>,
    scope: &NameScope,
) -> Result<Vec<ValueSuggestion>, OxidGeneError> {
    let (lang, limit) = validated(field, language, limit, scope)?;
    let key = fold_words(query);
    if key.is_empty() {
        return Ok(Vec::new());
    }
    if !scope.is_empty() {
        let names = PersonSearchRepo::primary_names(
            db,
            tree_id,
            scope.surname.as_deref(),
            scope.given_names.as_deref(),
        )
        .await?;
        let values = scoped_values(names, field);
        return Ok(rank(values, field.reference(), false, lang, &key, limit));
    }
    let values = tree_values(db, tree_id, field).await?;
    Ok(rank(values, field.reference(), true, lang, &key, limit))
}

/// The language and the limit of a request, refusing an unknown language, a
/// limit out of range, and a scope on a field other than names.
fn validated(
    field: SuggestionField,
    language: &str,
    limit: Option<usize>,
    scope: &NameScope,
) -> Result<(ReferenceLang, usize), OxidGeneError> {
    let lang = crate::reference::language(language)?;
    let limit = limit.unwrap_or(DEFAULT_VALUE_SUGGESTIONS);
    if !(1..=MAX_VALUE_SUGGESTIONS).contains(&limit) {
        return Err(OxidGeneError::Validation(format!(
            "limit must be between 1 and {MAX_VALUE_SUGGESTIONS}"
        )));
    }
    let names = matches!(
        field,
        SuggestionField::FamilyNames | SuggestionField::GivenNames
    );
    if !scope.is_empty() && !names {
        return Err(OxidGeneError::Validation(SCOPE_NAMES_ONLY.to_string()));
    }
    Ok((lang, limit))
}

/// The tree's values of `field`, each with its number of uses.
async fn tree_values(
    db: &impl ConnectionTrait,
    tree_id: Uuid,
    field: SuggestionField,
) -> Result<Vec<(String, i64)>, OxidGeneError> {
    Ok(match field {
        SuggestionField::FamilyNames => entries(DictionaryRepo::family_names(db, tree_id).await?),
        SuggestionField::GivenNames => entries(DictionaryRepo::given_names(db, tree_id).await?),
        SuggestionField::Occupations => entries(DictionaryRepo::occupations(db, tree_id).await?),
        SuggestionField::Sources => {
            // Two sources may share a title: the field offers it once.
            let mut titles: Vec<(String, i64)> = Vec::new();
            let mut at: HashMap<String, usize> = HashMap::new();
            for (source, count) in DictionaryRepo::sources_with_usage(db, tree_id).await? {
                let title = source.title.trim();
                if title.is_empty() {
                    continue;
                }
                match at.get(title) {
                    Some(&index) => titles[index].1 += count,
                    None => {
                        at.insert(title.to_string(), titles.len());
                        titles.push((title.to_string(), count));
                    }
                }
            }
            titles
        }
    })
}

/// Each surname, or each given name, of `names` with the number of persons
/// carrying it; `names` holds one `(surname, given names)` per person.
fn scoped_values(names: Vec<(String, String)>, field: SuggestionField) -> Vec<(String, i64)> {
    let mut counts: HashMap<String, i64> = HashMap::new();
    for (surname, given_names) in names {
        let words: HashSet<&str> = match field {
            SuggestionField::GivenNames => given_names.split_whitespace().collect(),
            _ => [surname.trim()]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect(),
        };
        for word in words {
            *counts.entry(word.to_string()).or_default() += 1;
        }
    }
    counts.into_iter().collect()
}

fn entries(entries: Vec<oxidgene_db::repo::DictionaryValueEntry>) -> Vec<(String, i64)> {
    entries.into_iter().map(|e| (e.value, e.count)).collect()
}

/// `with_terms` fills the rest of the list with the reference terms.
fn rank(
    values: Vec<(String, i64)>,
    reference: Option<ReferenceKind>,
    with_terms: bool,
    lang: ReferenceLang,
    key: &str,
    limit: usize,
) -> Vec<ValueSuggestion> {
    let mut found: Vec<(String, String, i64)> = values
        .into_iter()
        .map(|(value, count)| (fold_words(&value), value, count))
        .filter(|(folded, _, _)| starts_a_word(folded, key))
        .collect();
    found.sort_by(|(fa, a, ca), (fb, b, cb)| {
        (!fa.starts_with(key), -ca, fa, a).cmp(&(!fb.starts_with(key), -cb, fb, b))
    });
    found.truncate(limit);

    let mut held: HashSet<String> = HashSet::new();
    let mut out: Vec<ValueSuggestion> = found
        .into_iter()
        .map(|(folded, value, count)| {
            held.insert(folded);
            ValueSuggestion {
                reference: reference.is_some_and(|kind| has_sheet(kind, lang, &value)),
                value,
                count,
            }
        })
        .collect();
    if let Some(kind) = reference.filter(|_| with_terms) {
        let room = limit - out.len();
        if room > 0 {
            // Asked for more, since some may already be the tree's.
            out.extend(
                suggest_terms(kind, lang, key, limit)
                    .into_iter()
                    .filter(|term| held.insert(fold_words(term)))
                    .take(room)
                    .map(|value| ValueSuggestion {
                        value,
                        count: 0,
                        reference: true,
                    }),
            );
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(list: &[(&str, i64)]) -> Vec<(String, i64)> {
        list.iter().map(|(v, c)| (v.to_string(), *c)).collect()
    }

    #[test]
    fn tree_values_starting_with_the_query_come_first_then_the_most_used() {
        let found = rank(
            values(&[
                ("LE MARCHAND", 9),
                ("MARTIN", 2),
                ("Marchal", 5),
                ("DUPONT", 7),
            ]),
            None,
            true,
            ReferenceLang::Fr,
            "mar",
            10,
        );
        let got: Vec<&str> = found.iter().map(|s| s.value.as_str()).collect();
        assert_eq!(got, ["Marchal", "MARTIN", "LE MARCHAND"]);
        assert!(found.iter().all(|s| !s.reference));
    }

    #[test]
    fn reference_terms_follow_the_tree_values_they_do_not_repeat() {
        let found = rank(
            values(&[("Laboureur", 3), ("Labourier de terre", 1)]),
            Some(ReferenceKind::Occupations),
            true,
            ReferenceLang::Fr,
            &fold_words("labou"),
            4,
        );
        assert_eq!(found.len(), 4);
        assert_eq!(found[0].value, "Laboureur");
        assert!(found[0].reference);
        assert_eq!(found[0].count, 3);
        assert!(!found[1].reference);
        assert!(found[2..].iter().all(|s| s.reference && s.count == 0));
        let folded: HashSet<String> = found.iter().map(|s| fold_words(&s.value)).collect();
        assert_eq!(folded.len(), found.len());
    }

    #[test]
    fn a_full_list_of_tree_values_leaves_no_room_for_the_sheets() {
        let found = rank(
            values(&[("Jean", 3)]),
            Some(ReferenceKind::GivenNames),
            true,
            ReferenceLang::Fr,
            "jea",
            1,
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].value, "Jean");
    }

    #[test]
    fn a_scope_counts_each_name_once_per_person() {
        let names = vec![
            ("NAME_A".to_string(), "Given_a Given_b".to_string()),
            ("NAME_A".to_string(), "Given_a Given_a".to_string()),
            (String::new(), "Given_b".to_string()),
        ];
        let mut given = scoped_values(names.clone(), SuggestionField::GivenNames);
        given.sort();
        assert_eq!(given, values(&[("Given_a", 2), ("Given_b", 2)]));
        assert_eq!(
            scoped_values(names, SuggestionField::FamilyNames),
            values(&[("NAME_A", 2)])
        );
    }

    #[test]
    fn a_scoped_list_adds_no_reference_term() {
        let found = rank(
            values(&[("Jean", 1)]),
            Some(ReferenceKind::GivenNames),
            false,
            ReferenceLang::Fr,
            "jea",
            10,
        );
        assert_eq!(found.len(), 1);
        assert!(found[0].reference);
    }
}
