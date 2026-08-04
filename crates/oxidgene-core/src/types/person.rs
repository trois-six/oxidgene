use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::enums::{NameType, Privacy, Sex};

/// A person in a genealogical tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Person {
    pub id: Uuid,
    pub tree_id: Uuid,
    pub sex: Sex,
    /// Per-person privacy override (§7 of the person edit modal spec).
    pub privacy: Privacy,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

/// A name for a person (a person can have multiple names).
///
/// One row is one complete name the person bore, not one name *piece*: the
/// pieces below only mean anything relative to each other, which is why a
/// birth name and a married name are two rows rather than shared columns.
/// Multiple given names ("Jean Baptiste Marie") stay in a single
/// `given_names` string — they are one name, not three.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PersonName {
    pub id: Uuid,
    pub person_id: Uuid,
    pub name_type: NameType,
    pub given_names: Option<String>,
    /// The surname root, without its particle — "Cruz" in "de la Cruz".
    pub surname: Option<String>,
    /// GEDCOM `SPFX`: the particle preceding the surname ("de la", "van der").
    ///
    /// Split out so surnames can be filed under their root; see
    /// [`crate::types::split_surname_particle`].
    pub surname_prefix: Option<String>,
    pub prefix: Option<String>,
    pub suffix: Option<String>,
    pub nickname: Option<String>,
    pub is_primary: bool,
    /// Display order among a person's secondary names; the primary name always
    /// comes first regardless.
    pub sort_order: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl PersonName {
    /// Returns a display-friendly full name, particle included.
    pub fn display_name(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(ref prefix) = self.prefix {
            parts.push(prefix.clone());
        }
        if let Some(ref given) = self.given_names {
            parts.push(given.clone());
        }
        if let Some(ref surname) = self.surname {
            parts.push(crate::types::join_surname_particle(
                self.surname_prefix.as_deref(),
                surname,
            ));
        }
        if let Some(ref suffix) = self.suffix {
            parts.push(suffix.clone());
        }
        parts.join(" ")
    }

    /// Returns the surname as written, particle included — "de la Cruz".
    ///
    /// Use this anywhere a surname is shown to the user; [`Self::surname`] on
    /// its own is the filing root, not a display value.
    pub fn full_surname(&self) -> Option<String> {
        self.surname
            .as_deref()
            .map(|root| crate::types::join_surname_particle(self.surname_prefix.as_deref(), root))
    }
}

/// One person reached by an ancestor or descendant traversal, with the number
/// of generations separating them from the person the walk started at.
///
/// `depth` is the *shortest* distance: with pedigree implex the same ancestor
/// is reachable by several paths, and this reports the closest one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AncestryLink {
    pub person_id: Uuid,
    pub depth: i32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display_name_full() {
        let name = PersonName {
            id: Uuid::nil(),
            person_id: Uuid::nil(),
            name_type: NameType::Birth,
            given_names: Some("Jean-Pierre".to_string()),
            surname: Some("Dupont".to_string()),
            surname_prefix: None,
            prefix: Some("Dr.".to_string()),
            suffix: Some("Jr.".to_string()),
            nickname: None,
            is_primary: true,
            sort_order: 0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        assert_eq!(name.display_name(), "Dr. Jean-Pierre Dupont Jr.");
    }

    #[test]
    fn test_display_name_minimal() {
        let name = PersonName {
            id: Uuid::nil(),
            person_id: Uuid::nil(),
            name_type: NameType::Birth,
            given_names: None,
            surname: Some("Dupont".to_string()),
            surname_prefix: None,
            prefix: None,
            suffix: None,
            nickname: None,
            is_primary: true,
            sort_order: 0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        assert_eq!(name.display_name(), "Dupont");
    }

    #[test]
    fn test_display_name_reattaches_the_surname_particle() {
        let name = PersonName {
            id: Uuid::nil(),
            person_id: Uuid::nil(),
            name_type: NameType::Birth,
            given_names: Some("Lois".to_string()),
            surname: Some("Cruz".to_string()),
            surname_prefix: Some("de la".to_string()),
            prefix: None,
            suffix: None,
            nickname: None,
            is_primary: true,
            sort_order: 0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        assert_eq!(name.display_name(), "Lois de la Cruz");
        assert_eq!(name.full_surname().as_deref(), Some("de la Cruz"));
    }
}
