//! The part of a tree a scoped export keeps (docs/tags.md §2.3).
//!
//! The scope names persons; the cut keeps them and the records they reach,
//! and no record that points at one left out. It is the semantics of
//! `oxidged`'s `Dataset::subset`, applied to the records the exporter takes:
//! the kept persons, every family one of them belongs to as a spouse or a
//! child (with the links of the kept persons only), the events of those
//! persons and families, the citations on them and the sources they cite,
//! the repositories of those sources, the media linked to any kept record
//! (with the parent document of a page), the places of the events and media
//! kept, and the notes, vignettes and kept GEDCOM of the records kept.

use std::collections::HashSet;

use oxidgene_core::types::KeptEntityKind;
use uuid::Uuid;

use super::TreeRecords;

/// Cuts `records` down to the persons `persons` names and what they reach.
///
/// The names of `self_person` (the tree's "Who am I?") stay even when the
/// scope leaves that person out: the submitter's name falls back to them, so
/// a scoped export names the same submitter as the whole-tree one. Their
/// names write no record, since only kept persons get an `INDI`.
pub(super) fn cut_to(records: &mut TreeRecords, persons: &[Uuid], self_person: Option<Uuid>) {
    let persons: HashSet<Uuid> = persons.iter().copied().collect();
    let families = kept_families(records, &persons);
    let events = kept_events(records, &persons, &families);
    let sources = kept_sources(records, &persons, &families, &events);
    let media = kept_media(records, &persons, &families, &events, &sources);
    let repositories = kept_repositories(records, &sources);
    let places = kept_places(records, &events, &media);

    records.persons.retain(|p| persons.contains(&p.id));
    records
        .person_names
        .retain(|n| persons.contains(&n.person_id) || Some(n.person_id) == self_person);
    records
        .family_spouses
        .retain(|l| families.contains(&l.family_id) && persons.contains(&l.person_id));
    records
        .family_children
        .retain(|l| families.contains(&l.family_id) && persons.contains(&l.person_id));
    records.families.retain(|f| families.contains(&f.id));
    records.events.retain(|e| events.contains(&e.id));
    records
        .event_witnesses
        .retain(|w| events.contains(&w.event_id) && persons.contains(&w.person_id));
    records.places.retain(|p| places.contains(&p.id));
    records
        .citations
        .retain(|c| citation_kept(c, &persons, &families, &events));
    records.sources.retain(|s| sources.contains(&s.id));
    records
        .source_repositories
        .retain(|l| sources.contains(&l.source_id) && repositories.contains(&l.repository_id));
    records
        .repositories
        .retain(|r| repositories.contains(&r.id));
    records.media.retain(|m| media.contains(&m.id));
    records
        .media_links
        .retain(|l| media_link_kept(l, &persons, &families, &events, &sources));
    records.vignettes.retain(|v| {
        media.contains(&v.media_id) && (is(&persons, v.person_id) || is(&events, v.event_id))
    });
    records.notes.retain(|n| {
        is(&persons, n.person_id)
            || is(&families, n.family_id)
            || is(&events, n.event_id)
            || is(&sources, n.source_id)
            || is(&media, n.media_id)
            || is(&repositories, n.repository_id)
    });
    let kept = Kept {
        ids: persons
            .iter()
            .chain(&families)
            .chain(&events)
            .chain(&sources)
            .chain(&repositories)
            .chain(&media)
            .copied()
            .collect(),
        shared_groups: records
            .notes
            .iter()
            .filter_map(|n| n.shared_group)
            .collect(),
    };
    records
        .gedcom_kept
        .retain(|k| kept.keeps(k.entity_kind, k.entity_id));
}

/// Whether `id` names one of `set`.
fn is(set: &HashSet<Uuid>, id: Option<Uuid>) -> bool {
    id.is_some_and(|id| set.contains(&id))
}

/// The families a kept person belongs to as a spouse or a child.
fn kept_families(records: &TreeRecords, persons: &HashSet<Uuid>) -> HashSet<Uuid> {
    records
        .family_spouses
        .iter()
        .filter(|l| persons.contains(&l.person_id))
        .map(|l| l.family_id)
        .chain(
            records
                .family_children
                .iter()
                .filter(|l| persons.contains(&l.person_id))
                .map(|l| l.family_id),
        )
        .collect()
}

/// The events of a kept person or of a kept family.
fn kept_events(
    records: &TreeRecords,
    persons: &HashSet<Uuid>,
    families: &HashSet<Uuid>,
) -> HashSet<Uuid> {
    records
        .events
        .iter()
        .filter(|e| is(persons, e.person_id) || is(families, e.family_id))
        .map(|e| e.id)
        .collect()
}

/// Whether a citation sits on a kept person, family or event.
fn citation_kept(
    citation: &oxidgene_core::types::Citation,
    persons: &HashSet<Uuid>,
    families: &HashSet<Uuid>,
    events: &HashSet<Uuid>,
) -> bool {
    is(persons, citation.person_id)
        || is(families, citation.family_id)
        || is(events, citation.event_id)
}

/// The sources a kept record cites.
fn kept_sources(
    records: &TreeRecords,
    persons: &HashSet<Uuid>,
    families: &HashSet<Uuid>,
    events: &HashSet<Uuid>,
) -> HashSet<Uuid> {
    records
        .citations
        .iter()
        .filter(|c| citation_kept(c, persons, families, events))
        .map(|c| c.source_id)
        .collect()
}

/// The media linked to a kept person, family, event or source, with the
/// pages of those documents and the document of each page kept.
fn kept_media(
    records: &TreeRecords,
    persons: &HashSet<Uuid>,
    families: &HashSet<Uuid>,
    events: &HashSet<Uuid>,
    sources: &HashSet<Uuid>,
) -> HashSet<Uuid> {
    let mut media: HashSet<Uuid> = records
        .media_links
        .iter()
        .filter(|l| media_link_kept(l, persons, families, events, sources))
        .map(|l| l.media_id)
        .collect();
    loop {
        let before = media.len();
        for medium in &records.media {
            let Some(parent) = medium.parent_media_id else {
                continue;
            };
            if media.contains(&medium.id) {
                media.insert(parent);
            } else if media.contains(&parent) {
                media.insert(medium.id);
            }
        }
        if media.len() == before {
            return media;
        }
    }
}

/// Whether a media link sits on a kept person, family, event or source.
fn media_link_kept(
    link: &oxidgene_core::types::MediaLink,
    persons: &HashSet<Uuid>,
    families: &HashSet<Uuid>,
    events: &HashSet<Uuid>,
    sources: &HashSet<Uuid>,
) -> bool {
    is(persons, link.person_id)
        || is(families, link.family_id)
        || is(events, link.event_id)
        || is(sources, link.source_id)
}

/// The repositories of the kept sources.
fn kept_repositories(records: &TreeRecords, sources: &HashSet<Uuid>) -> HashSet<Uuid> {
    records
        .source_repositories
        .iter()
        .filter(|l| sources.contains(&l.source_id))
        .map(|l| l.repository_id)
        .collect()
}

/// The places of the kept events and of the kept media.
fn kept_places(
    records: &TreeRecords,
    events: &HashSet<Uuid>,
    media: &HashSet<Uuid>,
) -> HashSet<Uuid> {
    records
        .events
        .iter()
        .filter(|e| events.contains(&e.id))
        .filter_map(|e| e.place_id)
        .chain(
            records
                .media
                .iter()
                .filter(|m| media.contains(&m.id))
                .filter_map(|m| m.place_id),
        )
        .collect()
}

/// The entities whose kept GEDCOM an export keeps.
struct Kept {
    /// The ids of the kept persons, families, events, sources, repositories
    /// and media.
    ids: HashSet<Uuid>,
    /// The shared groups of the kept notes.
    shared_groups: HashSet<Uuid>,
}

impl Kept {
    /// Whether a kept GEDCOM row of `kind` for `entity` belongs to the
    /// export. The submitter's rows stay; those of a record OxidGene has no
    /// entity for are left out, as they may point at records left out.
    fn keeps(&self, kind: KeptEntityKind, entity: Uuid) -> bool {
        match kind {
            KeptEntityKind::Submitter => true,
            KeptEntityKind::Record => false,
            KeptEntityKind::Note => self.shared_groups.contains(&entity),
            _ => self.ids.contains(&entity),
        }
    }
}
