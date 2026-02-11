use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::enums::{NameType, Sex};

/// A person in a genealogical tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Person {
    pub id: Uuid,
    pub tree_id: Uuid,
    pub sex: Sex,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

/// A name for a person (a person can have multiple names).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonName {
    pub id: Uuid,
    pub person_id: Uuid,
    pub name_type: NameType,
    pub given_names: Option<String>,
    pub surname: Option<String>,
    pub prefix: Option<String>,
    pub suffix: Option<String>,
    pub nickname: Option<String>,
    pub is_primary: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl PersonName {
    /// Returns a display-friendly full name.
    pub fn display_name(&self) -> String {
        let mut parts = Vec::new();
        if let Some(ref prefix) = self.prefix {
            parts.push(prefix.as_str());
        }
        if let Some(ref given) = self.given_names {
            parts.push(given.as_str());
        }
        if let Some(ref surname) = self.surname {
            parts.push(surname.as_str());
        }
        if let Some(ref suffix) = self.suffix {
            parts.push(suffix.as_str());
        }
        parts.join(" ")
    }
}

/// An entry in the ancestry closure table for optimized traversal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonAncestry {
    pub id: Uuid,
    pub tree_id: Uuid,
    pub ancestor_id: Uuid,
    pub descendant_id: Uuid,
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
            prefix: Some("Dr.".to_string()),
            suffix: Some("Jr.".to_string()),
            nickname: None,
            is_primary: true,
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
            prefix: None,
            suffix: None,
            nickname: None,
            is_primary: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        assert_eq!(name.display_name(), "Dupont");
    }
}
