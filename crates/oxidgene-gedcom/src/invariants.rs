//! The invariants every stored row holds, applied to an import's result.
//!
//! The schema refuses a row that breaks one of them (docs/data-model.md): a
//! person twice a spouse or a child of one family, the same witness twice
//! under one relation, two primary names, an event owned by both a person
//! and a family, a note, a citation or a media link attached to no record or
//! to several. A file may say any of these, and one such row must not fail
//! the import of the whole file: [`ImportResult::enforce_invariants`] makes
//! the result valid — it keeps the first of what repeats, the most specific
//! owner of what names several, and leaves out what is attached to nothing —
//! and says what it changed in the import's warnings, one line per rule.

use std::collections::HashSet;

use uuid::Uuid;

use crate::ImportResult;

impl ImportResult {
    /// Make every record of the result one the schema accepts, adding a
    /// warning per rule that changed something. A valid result is left as it
    /// is, so calling it twice changes nothing more.
    pub fn enforce_invariants(&mut self) {
        let mut changes = Changes::default();
        self.keep_one_link_per_family_member(&mut changes);
        self.keep_one_witness_per_relation(&mut changes);
        self.keep_one_primary_name(&mut changes);
        self.keep_one_portrait(&mut changes);
        self.keep_one_event_owner(&mut changes);
        self.keep_one_target(&mut changes);
        self.warnings.extend(changes.warnings());
    }

    fn keep_one_link_per_family_member(&mut self, changes: &mut Changes) {
        let mut seen: HashSet<(Uuid, Uuid)> = HashSet::new();
        let before = self.family_spouses.len();
        self.family_spouses
            .retain(|link| seen.insert((link.family_id, link.person_id)));
        changes.repeated_spouses = before - self.family_spouses.len();

        seen.clear();
        let before = self.family_children.len();
        self.family_children
            .retain(|link| seen.insert((link.family_id, link.person_id)));
        changes.repeated_children = before - self.family_children.len();
    }

    fn keep_one_witness_per_relation(&mut self, changes: &mut Changes) {
        let mut seen: HashSet<(Uuid, Uuid, String)> = HashSet::new();
        let before = self.event_witnesses.len();
        self.event_witnesses.retain(|witness| {
            seen.insert((
                witness.event_id,
                witness.person_id,
                witness.relation.clone().unwrap_or_default(),
            ))
        });
        changes.repeated_witnesses = before - self.event_witnesses.len();
    }

    fn keep_one_primary_name(&mut self, changes: &mut Changes) {
        let mut with_primary: HashSet<Uuid> = HashSet::new();
        for name in &mut self.person_names {
            if name.is_primary && !with_primary.insert(name.person_id) {
                name.is_primary = false;
                changes.demoted_names += 1;
            }
        }
    }

    fn keep_one_portrait(&mut self, changes: &mut Changes) {
        for person in &mut self.persons {
            // The crop is the more specific of the two.
            if person.portrait_vignette_id.is_some() && person.portrait_media_id.take().is_some() {
                changes.double_portraits += 1;
            }
        }
    }

    fn keep_one_event_owner(&mut self, changes: &mut Changes) {
        for event in &mut self.events {
            if event.person_id.is_some() && event.family_id.take().is_some() {
                changes.double_owner_events += 1;
            }
            if event.person_id.is_none() && event.related_family_id.take().is_some() {
                changes.misplaced_related_families += 1;
            }
        }
    }

    /// A note, a citation and a media link are attached to one record: the
    /// event when one is named, as the most specific, then the person, the
    /// family and the others.
    fn keep_one_target(&mut self, changes: &mut Changes) {
        self.citations.retain_mut(|citation| {
            let kept = first_set(&mut [
                &mut citation.event_id,
                &mut citation.person_id,
                &mut citation.family_id,
            ]);
            changes.count_target(kept, Kind::Citation)
        });
        self.media_links.retain_mut(|link| {
            let kept = first_set(&mut [
                &mut link.event_id,
                &mut link.person_id,
                &mut link.family_id,
                &mut link.source_id,
            ]);
            changes.count_target(kept, Kind::MediaLink)
        });
        self.notes.retain_mut(|note| {
            let kept = first_set(&mut [
                &mut note.media_id,
                &mut note.event_id,
                &mut note.person_id,
                &mut note.family_id,
                &mut note.source_id,
                &mut note.repository_id,
            ]);
            changes.count_target(kept, Kind::Note)
        });
    }
}

/// How many of `targets` were set, after clearing all but the first set one.
fn first_set(targets: &mut [&mut Option<Uuid>]) -> usize {
    let mut set = 0;
    for target in targets {
        if target.is_some() {
            set += 1;
            if set > 1 {
                **target = None;
            }
        }
    }
    set
}

#[derive(Clone, Copy)]
enum Kind {
    Citation,
    MediaLink,
    Note,
}

/// What [`ImportResult::enforce_invariants`] changed, by rule.
#[derive(Default)]
struct Changes {
    repeated_spouses: usize,
    repeated_children: usize,
    repeated_witnesses: usize,
    demoted_names: usize,
    double_portraits: usize,
    double_owner_events: usize,
    misplaced_related_families: usize,
    /// Citations, media links and notes attached to no record, left out.
    unattached: [usize; 3],
    /// Citations, media links and notes that named several records.
    several_targets: [usize; 3],
}

impl Changes {
    /// Count a row that had `set` targets; whether it is kept.
    fn count_target(&mut self, set: usize, kind: Kind) -> bool {
        match set {
            0 => {
                self.unattached[kind as usize] += 1;
                false
            }
            1 => true,
            _ => {
                self.several_targets[kind as usize] += 1;
                true
            }
        }
    }

    fn warnings(&self) -> Vec<String> {
        const KINDS: [&str; 3] = ["citation(s)", "media link(s)", "note(s)"];
        let mut lines = vec![
            (
                self.repeated_spouses,
                "spouse link(s) repeated a person already a spouse of the family: left out",
            ),
            (
                self.repeated_children,
                "child link(s) repeated a person already a child of the family: left out",
            ),
            (
                self.repeated_witnesses,
                "witness link(s) repeated a person already a witness of the event in the same relation: left out",
            ),
            (
                self.demoted_names,
                "name(s) marked primary beside a person's first primary name: kept as secondary names",
            ),
            (
                self.double_portraits,
                "person(s) had both a whole picture and a crop as portrait: the crop was kept",
            ),
            (
                self.double_owner_events,
                "event(s) belonged to both a person and a family: kept as the person's",
            ),
            (
                self.misplaced_related_families,
                "event(s) without an individual owner named a related family: the link was left out",
            ),
        ]
        .into_iter()
        .filter(|(count, _)| *count > 0)
        .map(|(count, what)| format!("{count} {what}"))
        .collect::<Vec<_>>();
        for (index, kind) in KINDS.iter().enumerate() {
            if self.unattached[index] > 0 {
                lines.push(format!(
                    "{} {kind} attached to no record: left out",
                    self.unattached[index]
                ));
            }
            if self.several_targets[index] > 0 {
                lines.push(format!(
                    "{} {kind} attached to several records: kept on the most specific one",
                    self.several_targets[index]
                ));
            }
        }
        lines
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use oxidgene_core::types::{
        Citation, Event, EventWitness, FamilyChild, FamilySpouse, MediaLink, Note, Person,
        PersonName,
    };
    use oxidgene_core::{
        Calendar, ChildType, DateQualifier, EventType, NameType, Privacy, Sex, SpouseRole,
    };

    use super::*;

    fn id() -> Uuid {
        Uuid::now_v7()
    }

    fn name(person_id: Uuid, is_primary: bool, sort_order: i32) -> PersonName {
        PersonName {
            id: id(),
            person_id,
            name_type: NameType::Birth,
            given_names: Some("Sample".to_string()),
            surname: Some("Fixture".to_string()),
            surname_prefix: None,
            prefix: None,
            suffix: None,
            nickname: None,
            is_primary,
            sort_order,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn event(person_id: Option<Uuid>, family_id: Option<Uuid>, related: Option<Uuid>) -> Event {
        Event {
            id: id(),
            tree_id: Uuid::nil(),
            event_type: EventType::Other,
            date_value: None,
            date_sort: None,
            date_qualifier: DateQualifier::Exact,
            date_value2: None,
            calendar: Calendar::Gregorian,
            cause: None,
            age: None,
            agency: None,
            classification: None,
            spouse_ages: Vec::new(),
            place_id: None,
            person_id,
            family_id,
            related_family_id: related,
            description: None,
            tags: Vec::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            deleted_at: None,
        }
    }

    fn citation(person: Option<Uuid>, event: Option<Uuid>, family: Option<Uuid>) -> Citation {
        Citation {
            id: id(),
            source_id: id(),
            person_id: person,
            event_id: event,
            family_id: family,
            page: None,
            confidence: None,
            text: None,
            tags: Vec::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn note(person: Option<Uuid>, event: Option<Uuid>) -> Note {
        Note {
            tags: Vec::new(),
            id: id(),
            tree_id: Uuid::nil(),
            text: "Sample note".to_string(),
            person_id: person,
            event_id: event,
            family_id: None,
            source_id: None,
            media_id: None,
            repository_id: None,
            shared_group: None,
            confidential: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            deleted_at: None,
        }
    }

    #[test]
    fn repeated_links_are_kept_once_with_a_warning_each() {
        let (family, person, event_id) = (id(), id(), id());
        let spouse = |role| FamilySpouse {
            id: id(),
            family_id: family,
            person_id: person,
            role,
            sort_order: 0,
        };
        let child = |child_type| FamilyChild {
            id: id(),
            family_id: family,
            person_id: person,
            child_type,
            sort_order: 0,
        };
        let witness = |relation: Option<&str>| EventWitness {
            id: id(),
            event_id,
            person_id: person,
            relation: relation.map(str::to_string),
            sort_order: 0,
        };
        let mut result = ImportResult {
            family_spouses: vec![spouse(SpouseRole::Husband), spouse(SpouseRole::Partner)],
            family_children: vec![child(ChildType::Biological), child(ChildType::Adopted)],
            event_witnesses: vec![
                witness(Some("Godfather")),
                witness(Some("Witness")),
                witness(Some("Godfather")),
                witness(None),
                witness(None),
            ],
            ..ImportResult::default()
        };

        result.enforce_invariants();

        assert_eq!(result.family_spouses.len(), 1);
        assert_eq!(result.family_spouses[0].role, SpouseRole::Husband);
        assert_eq!(result.family_children.len(), 1);
        assert_eq!(result.family_children[0].child_type, ChildType::Biological);
        let relations: Vec<_> = result
            .event_witnesses
            .iter()
            .map(|w| w.relation.as_deref())
            .collect();
        assert_eq!(relations, [Some("Godfather"), Some("Witness"), None]);
        assert_eq!(result.warnings.len(), 3, "{:?}", result.warnings);

        let warnings = result.warnings.len();
        result.enforce_invariants();
        assert_eq!(
            result.warnings.len(),
            warnings,
            "a valid result is left alone"
        );
    }

    #[test]
    fn a_person_keeps_one_primary_name_and_one_portrait() {
        let person = id();
        let mut result = ImportResult {
            persons: vec![Person {
                id: person,
                tree_id: Uuid::nil(),
                sex: Sex::Unknown,
                privacy: Privacy::Default,
                portrait_media_id: Some(id()),
                portrait_vignette_id: Some(id()),
                tags: Vec::new(),
                placeholder: None,
                created_at: Utc::now(),
                updated_at: Utc::now(),
                deleted_at: None,
            }],
            person_names: vec![
                name(person, false, 0),
                name(person, true, 1),
                name(person, true, 2),
            ],
            ..ImportResult::default()
        };

        result.enforce_invariants();

        let primary: Vec<_> = result.person_names.iter().map(|n| n.is_primary).collect();
        assert_eq!(primary, [false, true, false]);
        assert!(result.persons[0].portrait_media_id.is_none());
        assert!(result.persons[0].portrait_vignette_id.is_some());
        assert_eq!(result.warnings.len(), 2, "{:?}", result.warnings);
    }

    #[test]
    fn an_event_has_one_owner_and_a_row_one_target() {
        let (person, family, event_id) = (id(), id(), id());
        let mut result = ImportResult {
            events: vec![
                event(Some(person), Some(family), Some(family)),
                event(None, Some(family), Some(family)),
                event(None, None, None),
            ],
            citations: vec![
                citation(Some(person), Some(event_id), Some(family)),
                citation(None, None, Some(family)),
                citation(None, None, None),
            ],
            notes: vec![note(Some(person), Some(event_id)), note(None, None)],
            media_links: vec![MediaLink {
                id: id(),
                media_id: id(),
                person_id: Some(person),
                event_id: None,
                source_id: Some(id()),
                family_id: Some(family),
                sort_order: 0,
            }],
            ..ImportResult::default()
        };

        result.enforce_invariants();

        let owners: Vec<_> = result
            .events
            .iter()
            .map(|e| (e.person_id, e.family_id, e.related_family_id))
            .collect();
        assert_eq!(
            owners,
            [
                (Some(person), None, Some(family)),
                (None, Some(family), None),
                (None, None, None),
            ]
        );
        let cited: Vec<_> = result
            .citations
            .iter()
            .map(|c| (c.person_id, c.event_id, c.family_id))
            .collect();
        assert_eq!(
            cited,
            [(None, Some(event_id), None), (None, None, Some(family))]
        );
        assert_eq!(result.notes.len(), 1);
        assert_eq!(
            (result.notes[0].person_id, result.notes[0].event_id),
            (None, Some(event_id))
        );
        let link = &result.media_links[0];
        assert_eq!(
            (link.person_id, link.family_id, link.source_id),
            (Some(person), None, None)
        );
    }
}
