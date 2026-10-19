use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::enums::Confidence;
use crate::types::{Event, TagRef};

/// A citation linking a source to a person, event, or family.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Citation {
    pub id: Uuid,
    pub source_id: Uuid,
    pub person_id: Option<Uuid>,
    pub event_id: Option<Uuid>,
    pub family_id: Option<Uuid>,
    /// Page or location within the source.
    pub page: Option<String>,
    /// How reliable the cited evidence is; `None` when nobody assessed it.
    pub confidence: Option<Confidence>,
    /// Extracted text from the source.
    pub text: Option<String>,
    /// The tags the citation carries, sorted by folded label (docs/data-model.md, RecordTag).
    #[serde(default)]
    pub tags: Vec<TagRef>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Citation {
    /// The one person this citation is about: the principal of the
    /// individual event it is attached to, or else the person it is attached
    /// to; `event` is the event of `event_id`, when loaded. A family event
    /// has two principals and a family none, so a citation of either is
    /// about no one person.
    pub fn cited_person_id(&self, event: Option<&Event>) -> Option<Uuid> {
        match (self.event_id, event) {
            (Some(id), Some(event)) if event.id == id && event.family_id.is_none() => {
                event.person_id
            }
            (Some(_), _) => None,
            (None, _) if self.family_id.is_some() => None,
            (None, _) => self.person_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enums::{Calendar, DateQualifier, EventType};

    fn citation(person: Option<Uuid>, event: Option<Uuid>, family: Option<Uuid>) -> Citation {
        let now = Utc::now();
        Citation {
            id: Uuid::nil(),
            source_id: Uuid::nil(),
            person_id: person,
            event_id: event,
            family_id: family,
            page: None,
            confidence: None,
            text: None,
            tags: Vec::new(),
            created_at: now,
            updated_at: now,
        }
    }

    fn event(id: Uuid, person: Option<Uuid>, family: Option<Uuid>) -> Event {
        let now = Utc::now();
        Event {
            classification: None,
            tags: Vec::new(),
            id,
            tree_id: Uuid::nil(),
            event_type: EventType::Birth,
            date_value: None,
            date_sort: None,
            date_qualifier: DateQualifier::default(),
            date_value2: None,
            calendar: Calendar::default(),
            cause: None,
            age: None,
            agency: None,
            spouse_ages: Vec::new(),
            place_id: None,
            person_id: person,
            family_id: family,
            related_family_id: None,
            description: None,
            created_at: now,
            updated_at: now,
            deleted_at: None,
        }
    }

    #[test]
    fn a_citation_is_about_its_person_or_its_individual_event_s_principal() {
        let (person, principal, family, event_id) = (
            Uuid::from_u128(1),
            Uuid::from_u128(2),
            Uuid::from_u128(3),
            Uuid::from_u128(4),
        );
        assert_eq!(
            citation(Some(person), None, None).cited_person_id(None),
            Some(person)
        );
        let individual = event(event_id, Some(principal), None);
        assert_eq!(
            citation(None, Some(event_id), None).cited_person_id(Some(&individual)),
            Some(principal)
        );
        // A family event, a family, an event not loaded or another one: no
        // one person.
        let shared = event(event_id, None, Some(family));
        assert_eq!(
            citation(None, Some(event_id), None).cited_person_id(Some(&shared)),
            None
        );
        assert_eq!(
            citation(None, None, Some(family)).cited_person_id(None),
            None
        );
        assert_eq!(
            citation(None, Some(event_id), None).cited_person_id(None),
            None
        );
        let other = event(Uuid::from_u128(5), Some(principal), None);
        assert_eq!(
            citation(None, Some(event_id), None).cited_person_id(Some(&other)),
            None
        );
    }
}
