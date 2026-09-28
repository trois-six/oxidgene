//! Suggestions for the free-text fields of the entry forms: the values a
//! tree already holds, then, for occupations and given names, the terms the
//! reference sheets answer to. See `docs/api.md` (value suggestions).

use std::collections::HashSet;

use oxidgene_core::OxidGeneError;
use oxidgene_db::repo::DictionaryRepo;
use sea_orm::ConnectionTrait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::reference::{
    ReferenceKind, ReferenceLang, UNSUPPORTED_LANGUAGE, has_sheet, normalize_key, starts_a_word,
    suggest_terms,
};

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

/// Up to `limit` suggestions with a word starting with `query`, ignoring
/// case, accents and punctuation: the tree's values first — those starting
/// with the query, then the most used — then the reference terms the tree
/// does not hold yet.
pub async fn suggest(
    db: &impl ConnectionTrait,
    tree_id: Uuid,
    field: SuggestionField,
    language: &str,
    query: &str,
    limit: Option<usize>,
) -> Result<Vec<ValueSuggestion>, OxidGeneError> {
    let lang = ReferenceLang::from_code(language)
        .ok_or_else(|| OxidGeneError::Validation(UNSUPPORTED_LANGUAGE.to_string()))?;
    let limit = limit.unwrap_or(DEFAULT_VALUE_SUGGESTIONS);
    if !(1..=MAX_VALUE_SUGGESTIONS).contains(&limit) {
        return Err(OxidGeneError::Validation(format!(
            "limit must be between 1 and {MAX_VALUE_SUGGESTIONS}"
        )));
    }
    let key = normalize_key(query);
    if key.is_empty() {
        return Ok(Vec::new());
    }

    let values: Vec<(String, i64)> = match field {
        SuggestionField::FamilyNames => entries(DictionaryRepo::family_names(db, tree_id).await?),
        SuggestionField::GivenNames => entries(DictionaryRepo::given_names(db, tree_id).await?),
        SuggestionField::Occupations => entries(DictionaryRepo::occupations(db, tree_id).await?),
        SuggestionField::Sources => {
            // Two sources may share a title: the field offers it once.
            let mut titles: Vec<(String, i64)> = Vec::new();
            for (source, count) in DictionaryRepo::sources_with_usage(db, tree_id).await? {
                let title = source.title.trim().to_string();
                match titles.iter_mut().find(|(t, _)| *t == title) {
                    Some((_, total)) => *total += count,
                    None if !title.is_empty() => titles.push((title, count)),
                    None => {}
                }
            }
            titles
        }
    };
    Ok(rank(values, field.reference(), lang, &key, limit))
}

fn entries(entries: Vec<oxidgene_db::repo::DictionaryValueEntry>) -> Vec<(String, i64)> {
    entries.into_iter().map(|e| (e.value, e.count)).collect()
}

fn rank(
    values: Vec<(String, i64)>,
    reference: Option<ReferenceKind>,
    lang: ReferenceLang,
    key: &str,
    limit: usize,
) -> Vec<ValueSuggestion> {
    let mut found: Vec<(String, String, i64)> = values
        .into_iter()
        .map(|(value, count)| (normalize_key(&value), value, count))
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
    if let Some(kind) = reference {
        let room = limit - out.len();
        if room > 0 {
            // Asked for more, since some may already be the tree's.
            out.extend(
                suggest_terms(kind, lang, key, limit)
                    .into_iter()
                    .filter(|term| held.insert(normalize_key(term)))
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
            ReferenceLang::Fr,
            &normalize_key("labou"),
            4,
        );
        assert_eq!(found.len(), 4);
        assert_eq!(found[0].value, "Laboureur");
        assert!(found[0].reference);
        assert_eq!(found[0].count, 3);
        assert!(!found[1].reference);
        assert!(found[2..].iter().all(|s| s.reference && s.count == 0));
        let folded: HashSet<String> = found.iter().map(|s| normalize_key(&s.value)).collect();
        assert_eq!(folded.len(), found.len());
    }

    #[test]
    fn a_full_list_of_tree_values_leaves_no_room_for_the_sheets() {
        let found = rank(
            values(&[("Jean", 3)]),
            Some(ReferenceKind::GivenNames),
            ReferenceLang::Fr,
            "jea",
            1,
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].value, "Jean");
    }
}
