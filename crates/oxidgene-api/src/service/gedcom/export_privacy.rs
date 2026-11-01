//! The persons an export leaves out for privacy (docs/privacy.md §8): the
//! *Leave out contemporary and private persons* option of both builds, and
//! the owner's *Leave out excluded persons* of the web build.
//!
//! The rule is `oxidgene_core::privacy`'s, applied to the records the export
//! read, with the tree's settings and the current year. A person left out is
//! written as the tree's display of hidden persons says: in `hidden` mode
//! not at all, the links through them dropped as a scoped export drops them;
//! otherwise as a placeholder keeping their sex and family links, without a
//! name (or with their initials in `initials` mode, never for an excluded
//! person), an event, a note, a citation, a medium or a tag, and marked
//! `RESN privacy` (GEDCOM 7: `CONFIDENTIAL`).
//!
//! What a hidden person reaches goes with them (§3.2): a family with a
//! hidden spouse, or private itself, keeps the links between its members
//! but loses its events, notes, citations and media; a medium private
//! itself, or showing a hidden person, stays out, with its pages; a source
//! only hidden records cite, a repository only such sources reach, and a
//! place only hidden events and media use are left out too.

use std::collections::HashSet;

use chrono::{Datelike, Utc};
use oxidgene_core::enums::{HiddenDisplay, NameType, Privacy, TreeDefaultPrivacy};
use oxidgene_core::privacy::{Withheld, derive_records, withheld};
use oxidgene_core::types::{KeptEntityKind, PersonName, Tree};
use uuid::Uuid;

use super::TreeRecords;

/// The privacy options of one export.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ExportPrivacy {
    /// Leave out the persons the default privacy, their own privacy and the
    /// contemporary rule hide (§3, §4).
    pub leave_out_private: bool,
    /// Leave out the persons excluded from sharing (§6), web build only.
    pub leave_out_excluded: bool,
}

impl ExportPrivacy {
    /// Whether the export leaves anyone out.
    #[must_use]
    pub fn applies(self) -> bool {
        self.leave_out_private || self.leave_out_excluded
    }

    /// The options as an export's audit entry names them: absent when none
    /// applies.
    #[must_use]
    pub fn details(self) -> Option<String> {
        let names: Vec<&str> = [
            (self.leave_out_private, "leave_out_private"),
            (self.leave_out_excluded, "leave_out_excluded"),
        ]
        .into_iter()
        .filter_map(|(on, name)| on.then_some(name))
        .collect();
        (!names.is_empty()).then(|| names.join(","))
    }
}

/// Whether `privacy` resolves to private in a tree whose default is
/// `default`.
fn private(privacy: Privacy, default: TreeDefaultPrivacy) -> bool {
    default.resolve(privacy) == TreeDefaultPrivacy::Private
}

/// The first letter of `text`, capitalized, followed by a period.
fn initial(text: Option<&str>) -> Option<String> {
    let first = text?.trim().chars().next()?;
    Some(format!("{}.", first.to_uppercase()))
}

/// The name an `initials` placeholder carries: the initials of the first
/// given name and of the surname of the person's primary name (`J. D.`).
fn initials_of(names: &[PersonName], person: Uuid) -> Option<PersonName> {
    let primary = names
        .iter()
        .filter(|name| name.person_id == person)
        .min_by_key(|name| (!name.is_primary, name.sort_order))?;
    let given = initial(
        primary
            .given_names
            .as_deref()
            .and_then(|given| given.split_whitespace().next()),
    );
    let surname = initial(primary.surname.as_deref());
    (given.is_some() || surname.is_some()).then(|| PersonName {
        given_names: given,
        surname,
        surname_prefix: None,
        prefix: None,
        suffix: None,
        nickname: None,
        name_type: NameType::Birth,
        is_primary: true,
        sort_order: 0,
        ..primary.clone()
    })
}

/// Who the options leave out, and why.
struct Hidden {
    /// Every person left out.
    persons: HashSet<Uuid>,
    /// Those left out because they are excluded: never shown with initials.
    excluded: HashSet<Uuid>,
}

fn hidden_persons(
    records: &TreeRecords,
    tree: &Tree,
    options: ExportPrivacy,
    excluded: &HashSet<Uuid>,
    current_year: i32,
) -> Hidden {
    let visibility = derive_records(
        &records.persons,
        &records.events,
        &records.family_spouses,
        &records.family_children,
    );
    let rule = tree.privacy.contemporary_rule();
    let mut hidden = Hidden {
        persons: HashSet::new(),
        excluded: HashSet::new(),
    };
    for person in &records.persons {
        let contemporary = visibility
            .get(&person.id)
            .is_some_and(|row| row.is_contemporary(rule, current_year));
        let is_excluded = options.leave_out_excluded && excluded.contains(&person.id);
        let reason = withheld(
            is_excluded,
            person.privacy,
            tree.default_privacy,
            contemporary,
        );
        match reason {
            Some(Withheld::Excluded) => {
                hidden.excluded.insert(person.id);
                hidden.persons.insert(person.id);
            }
            Some(_) if options.leave_out_private => {
                hidden.persons.insert(person.id);
            }
            _ => {}
        }
    }
    hidden
}

/// Cut `records` down as `options` say, for tree `tree`: the persons left
/// out removed or made placeholders, and what only they reach left out.
/// `excluded` is the tree's union of exclusions, read when
/// `leave_out_excluded` is set.
pub(super) fn apply(
    records: &mut TreeRecords,
    tree: &Tree,
    options: ExportPrivacy,
    excluded: &HashSet<Uuid>,
) {
    if !options.applies() {
        return;
    }
    let current_year = Utc::now().year();
    let hidden = hidden_persons(records, tree, options, excluded, current_year);
    let display = tree.privacy.hidden_display;
    let places_before = places_used(records);
    let cited_before: HashSet<Uuid> = records.citations.iter().map(|c| c.source_id).collect();
    strip_persons(records, &hidden.persons, &hidden.excluded, display);
    strip_families(records, &hidden.persons, options, tree.default_privacy);
    strip_media(records, &hidden.persons, options, tree.default_privacy);
    if display == HiddenDisplay::Hidden {
        remove_persons(records, &hidden.persons);
    } else {
        records.withheld_persons = hidden.persons.iter().copied().collect();
    }
    drop_orphans(records, &places_before, &cited_before);
}

/// The places events and media use.
fn places_used(records: &TreeRecords) -> HashSet<Uuid> {
    records
        .events
        .iter()
        .filter_map(|event| event.place_id)
        .chain(records.media.iter().filter_map(|medium| medium.place_id))
        .collect()
}

/// `hidden` mode: the placeholders `persons` go altogether, with the links
/// through them, so a child whose parent is hidden shows one parent; a
/// family left without a member goes too.
fn remove_persons(records: &mut TreeRecords, persons: &HashSet<Uuid>) {
    if persons.is_empty() {
        return;
    }
    let had_members: HashSet<Uuid> = records
        .family_spouses
        .iter()
        .map(|link| link.family_id)
        .chain(records.family_children.iter().map(|link| link.family_id))
        .collect();
    records
        .persons
        .retain(|person| !persons.contains(&person.id));
    records
        .family_spouses
        .retain(|link| !persons.contains(&link.person_id));
    records
        .family_children
        .retain(|link| !persons.contains(&link.person_id));
    records
        .event_witnesses
        .retain(|witness| !persons.contains(&witness.person_id));
    let has_members: HashSet<Uuid> = records
        .family_spouses
        .iter()
        .map(|link| link.family_id)
        .chain(records.family_children.iter().map(|link| link.family_id))
        .collect();
    let emptied: HashSet<Uuid> = had_members.difference(&has_members).copied().collect();
    if emptied.is_empty() {
        return;
    }
    let on = |id: Option<Uuid>| id.is_some_and(|id| emptied.contains(&id));
    let events: HashSet<Uuid> = records
        .events
        .iter()
        .filter(|event| on(event.family_id))
        .map(|event| event.id)
        .collect();
    drop_events(records, &events);
    records
        .families
        .retain(|family| !emptied.contains(&family.id));
    records.notes.retain(|note| !on(note.family_id));
    records.citations.retain(|cite| !on(cite.family_id));
    records.media_links.retain(|link| !on(link.family_id));
    records.gedcom_kept.retain(|kept| {
        !(kept.entity_kind == KeptEntityKind::Family && emptied.contains(&kept.entity_id))
    });
}

/// Make `stubs` placeholders: their sex and family links, nothing else, and
/// the initials of their name in `initials` mode unless they are excluded.
fn strip_persons(
    records: &mut TreeRecords,
    stubs: &HashSet<Uuid>,
    excluded: &HashSet<Uuid>,
    display: HiddenDisplay,
) {
    if stubs.is_empty() {
        return;
    }
    let initials: Vec<PersonName> = if display == HiddenDisplay::Initials {
        stubs
            .iter()
            .filter(|id| !excluded.contains(id))
            .filter_map(|&id| initials_of(&records.person_names, id))
            .collect()
    } else {
        Vec::new()
    };
    records
        .person_names
        .retain(|name| !stubs.contains(&name.person_id));
    records.person_names.extend(initials);
    for person in records.persons.iter_mut().filter(|p| stubs.contains(&p.id)) {
        person.tags.clear();
        person.portrait_media_id = None;
        person.portrait_vignette_id = None;
    }
    let events: HashSet<Uuid> = records
        .events
        .iter()
        .filter(|event| event.person_id.is_some_and(|id| stubs.contains(&id)))
        .map(|event| event.id)
        .collect();
    drop_events(records, &events);
    records
        .notes
        .retain(|note| !note.person_id.is_some_and(|id| stubs.contains(&id)));
    records
        .citations
        .retain(|cite| !cite.person_id.is_some_and(|id| stubs.contains(&id)));
    records.gedcom_kept.retain(|kept| {
        !(kept.entity_kind == KeptEntityKind::Person && stubs.contains(&kept.entity_id))
    });
}

/// Leave out `events` and what hangs from them: their witnesses, notes,
/// citations and media links.
fn drop_events(records: &mut TreeRecords, events: &HashSet<Uuid>) {
    if events.is_empty() {
        return;
    }
    let on = |id: Option<Uuid>| id.is_some_and(|id| events.contains(&id));
    records.events.retain(|event| !events.contains(&event.id));
    records
        .event_witnesses
        .retain(|witness| !events.contains(&witness.event_id));
    records.notes.retain(|note| !on(note.event_id));
    records.citations.retain(|cite| !on(cite.event_id));
    records.media_links.retain(|link| !on(link.event_id));
    records.vignettes.retain(|crop| !on(crop.event_id));
}

/// The families a placeholder is a spouse of, and those private
/// themselves: their links stay, their events, notes, citations, media and
/// tags go.
fn strip_families(
    records: &mut TreeRecords,
    stubs: &HashSet<Uuid>,
    options: ExportPrivacy,
    default: TreeDefaultPrivacy,
) {
    let mut hidden: HashSet<Uuid> = records
        .family_spouses
        .iter()
        .filter(|link| stubs.contains(&link.person_id))
        .map(|link| link.family_id)
        .collect();
    if options.leave_out_private {
        hidden.extend(
            records
                .families
                .iter()
                .filter(|family| private(family.privacy, default))
                .map(|family| family.id),
        );
    }
    if hidden.is_empty() {
        return;
    }
    let on = |id: Option<Uuid>| id.is_some_and(|id| hidden.contains(&id));
    let events: HashSet<Uuid> = records
        .events
        .iter()
        .filter(|event| on(event.family_id))
        .map(|event| event.id)
        .collect();
    drop_events(records, &events);
    for family in records
        .families
        .iter_mut()
        .filter(|f| hidden.contains(&f.id))
    {
        family.tags.clear();
    }
    records.notes.retain(|note| !on(note.family_id));
    records.citations.retain(|cite| !on(cite.family_id));
    records.media_links.retain(|link| !on(link.family_id));
    records.gedcom_kept.retain(|kept| {
        !(kept.entity_kind == KeptEntityKind::Family && hidden.contains(&kept.entity_id))
    });
}

/// Leave out the media private themselves, and those showing a placeholder
/// — linked to them or cropped on them — with the pages of each.
fn strip_media(
    records: &mut TreeRecords,
    stubs: &HashSet<Uuid>,
    options: ExportPrivacy,
    default: TreeDefaultPrivacy,
) {
    let mut hidden: HashSet<Uuid> = records
        .media_links
        .iter()
        .filter(|link| link.person_id.is_some_and(|id| stubs.contains(&id)))
        .map(|link| link.media_id)
        .chain(
            records
                .vignettes
                .iter()
                .filter(|crop| crop.person_id.is_some_and(|id| stubs.contains(&id)))
                .map(|crop| crop.media_id),
        )
        .collect();
    if options.leave_out_private {
        hidden.extend(
            records
                .media
                .iter()
                .filter(|medium| private(medium.privacy, default))
                .map(|medium| medium.id),
        );
    }
    // A page follows its document, and a document its pages.
    let documents: HashSet<Uuid> = records
        .media
        .iter()
        .filter(|medium| hidden.contains(&medium.id))
        .filter_map(|medium| medium.parent_media_id)
        .collect();
    hidden.extend(documents);
    let pages: Vec<Uuid> = records
        .media
        .iter()
        .filter(|medium| {
            medium
                .parent_media_id
                .is_some_and(|parent| hidden.contains(&parent))
        })
        .map(|medium| medium.id)
        .collect();
    hidden.extend(pages);
    if hidden.is_empty() {
        return;
    }
    records.media.retain(|medium| !hidden.contains(&medium.id));
    records
        .media_links
        .retain(|link| !hidden.contains(&link.media_id));
    records
        .vignettes
        .retain(|crop| !hidden.contains(&crop.media_id));
    records
        .notes
        .retain(|note| !note.media_id.is_some_and(|id| hidden.contains(&id)));
    for person in &mut records.persons {
        if person
            .portrait_media_id
            .is_some_and(|id| hidden.contains(&id))
        {
            person.portrait_media_id = None;
            person.portrait_vignette_id = None;
        }
    }
    records.gedcom_kept.retain(|kept| {
        !(kept.entity_kind == KeptEntityKind::Media && hidden.contains(&kept.entity_id))
    });
}

/// Leave out the places only left-out events and media used, the sources
/// only left-out records cited, and the repositories only such sources
/// reached (§3.2).
fn drop_orphans(
    records: &mut TreeRecords,
    places_before: &HashSet<Uuid>,
    cited_before: &HashSet<Uuid>,
) {
    let places_after = places_used(records);
    records
        .places
        .retain(|place| !places_before.contains(&place.id) || places_after.contains(&place.id));
    let cited_after: HashSet<Uuid> = records.citations.iter().map(|c| c.source_id).collect();
    let gone: HashSet<Uuid> = cited_before.difference(&cited_after).copied().collect();
    if gone.is_empty() {
        return;
    }
    let held_before: HashSet<Uuid> = records
        .source_repositories
        .iter()
        .map(|link| link.repository_id)
        .collect();
    records.sources.retain(|source| !gone.contains(&source.id));
    records
        .source_repositories
        .retain(|link| !gone.contains(&link.source_id));
    records
        .notes
        .retain(|note| !note.source_id.is_some_and(|id| gone.contains(&id)));
    records
        .media_links
        .retain(|link| !link.source_id.is_some_and(|id| gone.contains(&id)));
    let held_after: HashSet<Uuid> = records
        .source_repositories
        .iter()
        .map(|link| link.repository_id)
        .collect();
    let unheld: HashSet<Uuid> = held_before.difference(&held_after).copied().collect();
    records
        .repositories
        .retain(|repository| !unheld.contains(&repository.id));
    records
        .notes
        .retain(|note| !note.repository_id.is_some_and(|id| unheld.contains(&id)));
    records.gedcom_kept.retain(|kept| {
        !(kept.entity_kind == KeptEntityKind::Source && gone.contains(&kept.entity_id)
            || kept.entity_kind == KeptEntityKind::Repository && unheld.contains(&kept.entity_id))
    });
}
