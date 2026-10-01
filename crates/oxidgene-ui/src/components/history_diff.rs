//! Side-by-side comparison of two versions of a record, field by field, and
//! the wording of an audit entry. Shared by the person history page and the
//! tree's audit log. See `docs/ui-person-history.md`.

use dioxus::prelude::*;
use oxidgene_core::history::{
    AuditAction, AuditEntity, AuditEntry, CitationSnapshot, EventSnapshot, NameSnapshot,
    NoteSnapshot, PersonSnapshot, PlaceSnapshot, RecordSnapshot, RecordVersion, SourceSnapshot,
    TreeSnapshot, align,
};
use oxidgene_core::types::join_surname_particle;
use oxidgene_core::{Confidence, SpouseRole, TreeDefaultPrivacy};
use uuid::Uuid;

use crate::components::date_input::format_date;
use crate::i18n::I18n;
use crate::utils::{child_type_label_key, event_type_label_key, name_type_label_key};

// ── Model ───────────────────────────────────────────────────────────────

/// How an item of a record differs between the two versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Removed,
    Changed,
    Unchanged,
}

/// One field: its label and its value in each version, empty when absent.
#[derive(Debug, Clone, PartialEq)]
pub struct DiffRow {
    pub label: String,
    pub before: String,
    pub after: String,
    /// Decided on the values underneath the wording: two versions naming the
    /// same place are the same even if the place was renamed between them.
    pub changed: bool,
}

impl DiffRow {
    pub fn differs(&self) -> bool {
        self.changed
    }
}

/// One item of a record — a name, an event, a note — and its fields.
#[derive(Debug, Clone, PartialEq)]
pub struct DiffGroup {
    pub title: String,
    pub kind: ChangeKind,
    pub rows: Vec<DiffRow>,
}

/// A part of a record: identity, names, events…
#[derive(Debug, Clone, PartialEq)]
pub struct DiffSection {
    pub title: String,
    pub groups: Vec<DiffGroup>,
}

impl DiffSection {
    pub fn differs(&self) -> bool {
        self.groups.iter().any(|g| g.kind != ChangeKind::Unchanged)
    }
}

/// Collects the rows of one group, deciding its kind from what it holds.
struct GroupBuilder {
    title: String,
    presence: (bool, bool),
    rows: Vec<DiffRow>,
}

impl GroupBuilder {
    fn new(title: String, before: bool, after: bool) -> Self {
        Self {
            title,
            presence: (before, after),
            rows: Vec::new(),
        }
    }

    fn row(&mut self, label: String, before: Option<String>, after: Option<String>) {
        let clean = |value: Option<String>| value.map(|v| v.trim().to_string()).unwrap_or_default();
        let (before, after) = (clean(before), clean(after));
        if before.is_empty() && after.is_empty() {
            return;
        }
        let changed = before != after;
        self.rows.push(DiffRow {
            label,
            before,
            after,
            changed,
        });
    }

    /// A row whose values name other records: each side reads through its
    /// own version's labels, and the row differs only when the records named
    /// differ — never because one of them was renamed in between.
    fn reference_row<K: PartialEq>(
        &mut self,
        label: String,
        before: Option<(String, K)>,
        after: Option<(String, K)>,
    ) {
        let (before_text, before_key) = before.map_or((None, None), |(t, k)| (Some(t), Some(k)));
        let (after_text, after_key) = after.map_or((None, None), |(t, k)| (Some(t), Some(k)));
        let pushed = self.rows.len();
        self.row(label, before_text, after_text);
        if let Some(row) = self.rows.get_mut(pushed) {
            row.changed = before_key != after_key;
        }
    }

    fn finish(self) -> DiffGroup {
        let kind = match self.presence {
            (false, true) => ChangeKind::Added,
            (true, false) => ChangeKind::Removed,
            _ if self.rows.iter().any(DiffRow::differs) => ChangeKind::Changed,
            _ => ChangeKind::Unchanged,
        };
        DiffGroup {
            title: self.title,
            kind,
            rows: self.rows,
        }
    }
}

/// What the comparison reads a version's references through.
struct Side<'a> {
    version: Option<&'a RecordVersion>,
}

impl Side<'_> {
    fn label(&self, i18n: &I18n, id: Uuid) -> String {
        self.version
            .and_then(|v| v.label(id))
            .map(str::to_string)
            .unwrap_or_else(|| i18n.t("history.unknown_record"))
    }
}

/// Compare `before` (absent for a record's first version) with `after`.
///
/// A deleted state has no content: compared against, everything the other
/// side holds reads as added; compared, there is nothing to list — the
/// deletion banner says it all.
pub fn diff_versions(
    i18n: &I18n,
    before: Option<&RecordVersion>,
    after: &RecordVersion,
) -> Vec<DiffSection> {
    let old = Side { version: before };
    let new = Side {
        version: Some(after),
    };
    let previous = before.and_then(|v| v.snapshot.as_ref());
    match &after.snapshot {
        None => Vec::new(),
        Some(RecordSnapshot::Person(person)) => {
            let previous = previous.and_then(|s| match s {
                RecordSnapshot::Person(p) => Some(p),
                _ => None,
            });
            person_sections(i18n, (&old, previous), (&new, person))
        }
        Some(RecordSnapshot::Place(place)) => {
            let previous = previous.and_then(|s| match s {
                RecordSnapshot::Place(p) => Some(p),
                _ => None,
            });
            vec![place_section(i18n, previous, place)]
        }
        Some(RecordSnapshot::Source(source)) => {
            let previous = previous.and_then(|s| match s {
                RecordSnapshot::Source(s) => Some(s),
                _ => None,
            });
            vec![source_section(i18n, previous, source)]
        }
        Some(RecordSnapshot::Tree(tree)) => {
            let previous = previous.and_then(|s| match s {
                RecordSnapshot::Tree(t) => Some(t),
                _ => None,
            });
            vec![tree_section(i18n, (&old, previous), (&new, tree))]
        }
    }
}

fn person_sections(
    i18n: &I18n,
    (old, before): (&Side, Option<&PersonSnapshot>),
    (new, after): (&Side, &PersonSnapshot),
) -> Vec<DiffSection> {
    let mut sections = Vec::new();

    let mut identity = GroupBuilder::new(String::new(), before.is_some(), true);
    identity.row(
        i18n.t("history.field.sex"),
        before.map(|p| i18n.t(&format!("sex.{}", p.sex))),
        Some(i18n.t(&format!("sex.{}", after.sex))),
    );
    identity.row(
        i18n.t("history.field.privacy"),
        before.map(|p| i18n.t(&format!("privacy.{}", p.privacy))),
        Some(i18n.t(&format!("privacy.{}", after.privacy))),
    );
    sections.push(DiffSection {
        title: i18n.t("history.section.identity"),
        groups: vec![identity.finish()],
    });

    let empty = Vec::new();
    let names = align(before.map_or(&empty, |p| &p.names), &after.names)
        .into_iter()
        .map(|(b, a)| name_group(i18n, b, a))
        .collect();
    sections.push(DiffSection {
        title: i18n.t("history.section.names"),
        groups: names,
    });

    let events: Vec<EventSnapshot> = Vec::new();
    sections.push(DiffSection {
        title: i18n.t("history.section.events"),
        groups: align(before.map_or(&events, |p| &p.events), &after.events)
            .into_iter()
            .map(|(b, a)| event_group(i18n, (old, b), (new, a), None))
            .collect(),
    });

    let notes: Vec<NoteSnapshot> = Vec::new();
    sections.push(DiffSection {
        title: i18n.t("history.section.notes"),
        groups: align(before.map_or(&notes, |p| &p.notes), &after.notes)
            .into_iter()
            .map(|(b, a)| note_group(i18n, b, a))
            .collect(),
    });

    let citations: Vec<CitationSnapshot> = Vec::new();
    sections.push(DiffSection {
        title: i18n.t("history.section.sources"),
        groups: align(
            before.map_or(&citations, |p| &p.citations),
            &after.citations,
        )
        .into_iter()
        .map(|(b, a)| citation_group(i18n, (old, b), (new, a)))
        .collect(),
    });

    let parents = Vec::new();
    sections.push(DiffSection {
        title: i18n.t("history.section.parents"),
        groups: align(before.map_or(&parents, |p| &p.parents), &after.parents)
            .into_iter()
            .map(|(b, a)| {
                let title = a
                    .or(b)
                    .map(|link| {
                        new.version
                            .and_then(|v| v.label(link.family_id))
                            .or_else(|| old.version.and_then(|v| v.label(link.family_id)))
                            .map(str::to_string)
                            .unwrap_or_else(|| i18n.t("history.unknown_parents"))
                    })
                    .unwrap_or_default();
                let mut group = GroupBuilder::new(title, b.is_some(), a.is_some());
                group.row(
                    i18n.t("history.field.child_type"),
                    b.map(|l| i18n.t(child_type_label_key(l.child_type))),
                    a.map(|l| i18n.t(child_type_label_key(l.child_type))),
                );
                group.finish()
            })
            .collect(),
    });

    let unions = Vec::new();
    let mut union_groups = Vec::new();
    for (b, a) in align(before.map_or(&unions, |p| &p.unions), &after.unions) {
        let partners = |side: &Side, union: &oxidgene_core::history::UnionSnapshot| {
            union
                .spouses
                .iter()
                .map(|s| format!("{} ({})", side.label(i18n, s.person_id), role(i18n, s.role)))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let title = a
            .map(|u| partners(new, u))
            .or_else(|| b.map(|u| partners(old, u)))
            .unwrap_or_default();
        let mut group = GroupBuilder::new(title.clone(), b.is_some(), a.is_some());
        let spouse_keys = |u: &oxidgene_core::history::UnionSnapshot| {
            u.spouses
                .iter()
                .map(|s| (s.person_id, s.role))
                .collect::<Vec<_>>()
        };
        group.reference_row(
            i18n.t("history.field.spouses"),
            b.map(|u| (partners(old, u), spouse_keys(u))),
            a.map(|u| (partners(new, u), spouse_keys(u))),
        );
        let child_keys = |u: &oxidgene_core::history::UnionSnapshot| {
            u.children.iter().map(|c| c.person_id).collect::<Vec<_>>()
        };
        group.reference_row(
            i18n.t("history.field.children"),
            b.map(|u| {
                (
                    join(u.children.iter().map(|c| old.label(i18n, c.person_id))),
                    child_keys(u),
                )
            }),
            a.map(|u| {
                (
                    join(u.children.iter().map(|c| new.label(i18n, c.person_id))),
                    child_keys(u),
                )
            }),
        );
        group.row(
            i18n.t("history.field.privacy"),
            b.map(|u| i18n.t(&format!("privacy.{}", u.privacy))),
            a.map(|u| i18n.t(&format!("privacy.{}", u.privacy))),
        );
        group.row(
            i18n.t("history.section.notes"),
            b.map(|u| join_notes(&u.notes)),
            a.map(|u| join_notes(&u.notes)),
        );
        group.reference_row(
            i18n.t("history.section.sources"),
            b.map(|u| {
                (
                    join_citations(i18n, old, &u.citations),
                    citation_keys(&u.citations),
                )
            }),
            a.map(|u| {
                (
                    join_citations(i18n, new, &u.citations),
                    citation_keys(&u.citations),
                )
            }),
        );
        union_groups.push(group.finish());

        let no_events = Vec::new();
        union_groups.extend(
            align(
                b.map_or(&no_events, |u| &u.events),
                a.map_or(&no_events, |u| &u.events),
            )
            .into_iter()
            .map(|(eb, ea)| event_group(i18n, (old, eb), (new, ea), Some(&title))),
        );
    }
    sections.push(DiffSection {
        title: i18n.t("history.section.unions"),
        groups: union_groups,
    });

    sections.retain(|section| !section.groups.is_empty());
    sections
}

fn name_group(
    i18n: &I18n,
    before: Option<&NameSnapshot>,
    after: Option<&NameSnapshot>,
) -> DiffGroup {
    let shown = after
        .or(before)
        .expect("an aligned pair holds at least one side");
    let title = format!(
        "{} — {}",
        i18n.t(name_type_label_key(shown.name_type)),
        full_name(shown)
    );
    let mut group = GroupBuilder::new(title, before.is_some(), after.is_some());
    let field =
        |n: Option<&NameSnapshot>, get: fn(&NameSnapshot) -> Option<String>| n.and_then(get);
    group.row(
        i18n.t("history.field.name_type"),
        before.map(|n| i18n.t(name_type_label_key(n.name_type))),
        after.map(|n| i18n.t(name_type_label_key(n.name_type))),
    );
    for (key, get) in [
        (
            "history.field.prefix",
            (|n: &NameSnapshot| n.prefix.clone()) as fn(&NameSnapshot) -> Option<String>,
        ),
        ("history.field.given_names", |n| n.given_names.clone()),
        ("history.field.surname", |n| {
            n.surname
                .as_deref()
                .map(|root| join_surname_particle(n.surname_prefix.as_deref(), root))
        }),
        ("history.field.suffix", |n| n.suffix.clone()),
        ("history.field.nickname", |n| n.nickname.clone()),
    ] {
        group.row(i18n.t(key), field(before, get), field(after, get));
    }
    let primary = |n: &NameSnapshot| {
        i18n.t(if n.is_primary {
            "common.yes"
        } else {
            "common.no"
        })
    };
    group.row(
        i18n.t("history.field.primary"),
        before.map(primary),
        after.map(primary),
    );
    group.finish()
}

fn event_group(
    i18n: &I18n,
    (old, before): (&Side, Option<&EventSnapshot>),
    (new, after): (&Side, Option<&EventSnapshot>),
    union: Option<&str>,
) -> DiffGroup {
    let shown = after
        .or(before)
        .expect("an aligned pair holds at least one side");
    let mut title = i18n.t(event_type_label_key(shown.event_type));
    if let Some(union) = union.filter(|u| !u.is_empty()) {
        title = format!("{title} — {union}");
    }
    let mut group = GroupBuilder::new(title, before.is_some(), after.is_some());
    group.row(
        i18n.t("history.field.event_type"),
        before.map(|e| i18n.t(event_type_label_key(e.event_type))),
        after.map(|e| i18n.t(event_type_label_key(e.event_type))),
    );
    let date = |e: &EventSnapshot| {
        format_date(
            i18n,
            e.calendar,
            e.date_qualifier,
            e.date_value.as_deref(),
            e.date_value2.as_deref(),
        )
    };
    group.row(
        i18n.t("history.field.date"),
        before.map(date),
        after.map(date),
    );
    group.reference_row(
        i18n.t("history.field.place"),
        before
            .and_then(|e| e.place_id)
            .map(|id| (old.label(i18n, id), id)),
        after
            .and_then(|e| e.place_id)
            .map(|id| (new.label(i18n, id), id)),
    );
    group.row(
        i18n.t("history.field.description"),
        before.and_then(|e| e.description.clone()),
        after.and_then(|e| e.description.clone()),
    );
    group.row(
        i18n.t("history.field.cause"),
        before.and_then(|e| e.cause.clone()),
        after.and_then(|e| e.cause.clone()),
    );
    group.row(
        i18n.t("history.field.age"),
        before.and_then(|e| e.age.clone()),
        after.and_then(|e| e.age.clone()),
    );
    group.row(
        i18n.t("history.field.agency"),
        before.and_then(|e| e.agency.clone()),
        after.and_then(|e| e.agency.clone()),
    );
    let witnesses = |side: &Side, e: &EventSnapshot| {
        join(e.witnesses.iter().map(|w| match w.relation.as_deref() {
            Some(relation) if !relation.trim().is_empty() => {
                format!("{} ({relation})", side.label(i18n, w.person_id))
            }
            _ => side.label(i18n, w.person_id),
        }))
    };
    let witness_keys = |e: &EventSnapshot| {
        e.witnesses
            .iter()
            .map(|w| (w.person_id, w.relation.clone()))
            .collect::<Vec<_>>()
    };
    group.reference_row(
        i18n.t("history.field.witnesses"),
        before.map(|e| (witnesses(old, e), witness_keys(e))),
        after.map(|e| (witnesses(new, e), witness_keys(e))),
    );
    group.row(
        i18n.t("history.section.notes"),
        before.map(|e| join_notes(&e.notes)),
        after.map(|e| join_notes(&e.notes)),
    );
    group.reference_row(
        i18n.t("history.section.sources"),
        before.map(|e| {
            (
                join_citations(i18n, old, &e.citations),
                citation_keys(&e.citations),
            )
        }),
        after.map(|e| {
            (
                join_citations(i18n, new, &e.citations),
                citation_keys(&e.citations),
            )
        }),
    );
    group.finish()
}

/// What a joined list of citations shows: each source and page.
fn citation_keys(citations: &[CitationSnapshot]) -> Vec<(Uuid, Option<String>)> {
    citations
        .iter()
        .map(|c| (c.source_id, c.page.clone()))
        .collect()
}

fn note_group(
    i18n: &I18n,
    before: Option<&NoteSnapshot>,
    after: Option<&NoteSnapshot>,
) -> DiffGroup {
    let shown = after
        .or(before)
        .expect("an aligned pair holds at least one side");
    let mut group = GroupBuilder::new(excerpt(&shown.text), before.is_some(), after.is_some());
    group.row(
        i18n.t("history.field.text"),
        before.map(|n| n.text.clone()),
        after.map(|n| n.text.clone()),
    );
    group.finish()
}

fn citation_group(
    i18n: &I18n,
    (old, before): (&Side, Option<&CitationSnapshot>),
    (new, after): (&Side, Option<&CitationSnapshot>),
) -> DiffGroup {
    let title = after
        .map(|c| new.label(i18n, c.source_id))
        .or_else(|| before.map(|c| old.label(i18n, c.source_id)))
        .unwrap_or_default();
    let mut group = GroupBuilder::new(title, before.is_some(), after.is_some());
    group.reference_row(
        i18n.t("history.field.source"),
        before.map(|c| (old.label(i18n, c.source_id), c.source_id)),
        after.map(|c| (new.label(i18n, c.source_id), c.source_id)),
    );
    group.row(
        i18n.t("history.field.page"),
        before.and_then(|c| c.page.clone()),
        after.and_then(|c| c.page.clone()),
    );
    group.row(
        i18n.t("history.field.confidence"),
        before
            .and_then(|c| c.confidence)
            .map(|c| confidence(i18n, c)),
        after
            .and_then(|c| c.confidence)
            .map(|c| confidence(i18n, c)),
    );
    group.row(
        i18n.t("history.field.text"),
        before.and_then(|c| c.text.clone()),
        after.and_then(|c| c.text.clone()),
    );
    group.finish()
}

fn place_section(
    i18n: &I18n,
    before: Option<&PlaceSnapshot>,
    after: &PlaceSnapshot,
) -> DiffSection {
    let mut group = GroupBuilder::new(String::new(), before.is_some(), true);
    group.row(
        i18n.t("history.field.name"),
        before.map(|p| p.name.clone()),
        Some(after.name.clone()),
    );
    let coordinate = |value: Option<f64>| value.map(|v| format!("{v:.5}"));
    group.row(
        i18n.t("history.field.latitude"),
        before.and_then(|p| coordinate(p.latitude)),
        coordinate(after.latitude),
    );
    group.row(
        i18n.t("history.field.longitude"),
        before.and_then(|p| coordinate(p.longitude)),
        coordinate(after.longitude),
    );
    DiffSection {
        title: i18n.t("history.record.place"),
        groups: vec![group.finish()],
    }
}

fn source_section(
    i18n: &I18n,
    before: Option<&SourceSnapshot>,
    after: &SourceSnapshot,
) -> DiffSection {
    let mut group = GroupBuilder::new(String::new(), before.is_some(), true);
    for (key, get) in [
        (
            "history.field.title",
            (|s: &SourceSnapshot| Some(s.title.clone())) as fn(&SourceSnapshot) -> Option<String>,
        ),
        ("history.field.author", |s| s.author.clone()),
        ("history.field.publisher", |s| s.publisher.clone()),
        ("history.field.abbreviation", |s| s.abbreviation.clone()),
        ("history.field.repository", |s| s.repository_name.clone()),
        ("history.field.agency", |s| s.agency.clone()),
    ] {
        group.row(i18n.t(key), before.and_then(get), get(after));
    }
    group.row(
        i18n.t("history.section.notes"),
        before.map(|s| join_notes(&s.notes)),
        Some(join_notes(&after.notes)),
    );
    DiffSection {
        title: i18n.t("history.record.source"),
        groups: vec![group.finish()],
    }
}

fn tree_section(
    i18n: &I18n,
    (old, before): (&Side, Option<&TreeSnapshot>),
    (new, after): (&Side, &TreeSnapshot),
) -> DiffSection {
    let mut group = GroupBuilder::new(String::new(), before.is_some(), true);
    group.row(
        i18n.t("history.field.name"),
        before.map(|t| t.name.clone()),
        Some(after.name.clone()),
    );
    group.row(
        i18n.t("history.field.description"),
        before.and_then(|t| t.description.clone()),
        after.description.clone(),
    );
    let privacy = |p: TreeDefaultPrivacy| {
        i18n.t(match p {
            TreeDefaultPrivacy::Public => "privacy.public",
            TreeDefaultPrivacy::Private => "privacy.private",
        })
    };
    group.row(
        i18n.t("history.field.default_privacy"),
        before.map(|t| privacy(t.default_privacy)),
        Some(privacy(after.default_privacy)),
    );
    let on_off = |on: bool| i18n.t(if on { "common.yes" } else { "common.no" });
    group.row(
        i18n.t("history.field.entry_suggestions"),
        before.map(|t| on_off(t.entry_suggestions)),
        Some(on_off(after.entry_suggestions)),
    );
    group.reference_row(
        i18n.t("history.field.sosa_root"),
        before
            .and_then(|t| t.sosa_root_person_id)
            .map(|id| (old.label(i18n, id), id)),
        after
            .sosa_root_person_id
            .map(|id| (new.label(i18n, id), id)),
    );
    group.reference_row(
        i18n.t("history.field.self_person"),
        before
            .and_then(|t| t.self_person_id)
            .map(|id| (old.label(i18n, id), id)),
        after.self_person_id.map(|id| (new.label(i18n, id), id)),
    );
    DiffSection {
        title: i18n.t("history.record.tree"),
        groups: vec![group.finish()],
    }
}

fn full_name(name: &NameSnapshot) -> String {
    [
        name.prefix.clone(),
        name.given_names.clone(),
        name.surname
            .as_deref()
            .map(|root| join_surname_particle(name.surname_prefix.as_deref(), root)),
        name.suffix.clone(),
    ]
    .into_iter()
    .flatten()
    .filter(|part| !part.trim().is_empty())
    .collect::<Vec<_>>()
    .join(" ")
}

/// The display name a person snapshot gives its subject: the primary name,
/// or the first one.
pub fn snapshot_name(person: &PersonSnapshot) -> Option<String> {
    person
        .names
        .iter()
        .find(|n| n.is_primary)
        .or_else(|| person.names.first())
        .map(full_name)
        .filter(|name| !name.is_empty())
}

fn role(i18n: &I18n, role: SpouseRole) -> String {
    i18n.t(match role {
        SpouseRole::Husband => "history.role.husband",
        SpouseRole::Wife => "history.role.wife",
        SpouseRole::Partner => "history.role.partner",
    })
}

fn confidence(i18n: &I18n, confidence: Confidence) -> String {
    i18n.t(crate::utils::confidence_key(confidence))
}

fn join(items: impl Iterator<Item = String>) -> String {
    items.collect::<Vec<_>>().join(", ")
}

fn join_notes(notes: &[NoteSnapshot]) -> String {
    notes
        .iter()
        .map(|n| n.text.trim())
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn join_citations(i18n: &I18n, side: &Side, citations: &[CitationSnapshot]) -> String {
    citations
        .iter()
        .map(|c| match c.page.as_deref() {
            Some(page) if !page.trim().is_empty() => {
                format!("{}, {page}", side.label(i18n, c.source_id))
            }
            _ => side.label(i18n, c.source_id),
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// The first line of a text, shortened for a heading.
fn excerpt(text: &str) -> String {
    const MAX: usize = 60;
    let line = text.lines().next().unwrap_or_default().trim();
    if line.chars().count() > MAX {
        format!("{}…", line.chars().take(MAX).collect::<String>())
    } else {
        line.to_string()
    }
}

// ── Audit entries ───────────────────────────────────────────────────────

/// What an entry did, in a few words: « Modification — Événement (Naissance) ».
pub fn describe_entry(i18n: &I18n, entry: &AuditEntry) -> String {
    let action = i18n.t(&format!("history.action.{}", entry.action));
    let mut entity = i18n.t(&format!("history.entity.{}", entry.entity));
    if let Some(event_type) = entry.details.event_type
        && matches!(entry.entity, AuditEntity::Event | AuditEntity::EventWitness)
    {
        entity = format!("{entity} ({})", i18n.t(event_type_label_key(event_type)));
    }
    match entry.action {
        // The entity of an import or an export is the tree itself, which says
        // nothing the action does not.
        AuditAction::Import | AuditAction::Export => action,
        _ => format!("{action} — {entity}"),
    }
}

/// The facts an entry carries beyond its action, worded for the reader.
pub fn entry_details(i18n: &I18n, entry: &AuditEntry) -> Option<String> {
    let details = &entry.details;
    let mut parts = Vec::new();
    if let Some(format) = &details.format {
        parts.push(
            i18n.try_t(&format!("history.format.{format}"))
                .unwrap_or_else(|| format.clone()),
        );
    }
    if let Some(file_name) = &details.file_name {
        parts.push(file_name.clone());
    }
    if let Some(count) = details.count {
        parts.push(i18n.t_plural("history.persons_count", count as usize));
    }
    if let Some(version) = details.version {
        parts.push(i18n.t_args(
            "history.restored_version",
            &[("version", &version.to_string())],
        ));
    }
    if let Some(other) = &details.other_label {
        parts.push(i18n.t_args("history.merged_with", &[("name", other)]));
    }
    if let Some(new_name) = &details.new_label {
        parts.push(i18n.t_args("history.renamed_to", &[("name", new_name)]));
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}

/// A timestamp in the reader's language: « 27 sept. 2026, 14:32 ».
pub fn format_timestamp(i18n: &I18n, at: chrono::DateTime<chrono::Utc>) -> String {
    let local = at.with_timezone(&chrono::Local);
    let day = local.format("%d %b %Y").to_string().to_uppercase();
    let date = format_date(
        i18n,
        oxidgene_core::Calendar::Gregorian,
        oxidgene_core::DateQualifier::Exact,
        Some(&day),
        None,
    );
    format!("{date}, {}", local.format("%H:%M"))
}

// ── Rendering ───────────────────────────────────────────────────────────

/// The comparison of two versions of a record, as a table per section with
/// the older version on the left and the newer on the right.
#[component]
pub fn VersionDiff(
    before: Option<RecordVersion>,
    after: RecordVersion,
    /// Hide what did not change.
    #[props(default = true)]
    changes_only: bool,
) -> Element {
    let i18n = crate::i18n::use_i18n();
    let sections = diff_versions(&i18n, before.as_ref(), &after);
    let before_heading = match &before {
        Some(v) => version_label(&i18n, v),
        None => i18n.t("history.no_previous"),
    };
    let after_heading = version_label(&i18n, &after);
    let visible: Vec<&DiffSection> = sections
        .iter()
        .filter(|s| !changes_only || s.differs())
        .collect();

    rsx! {
        div { class: "hd",
            if after.deleted {
                div { class: "hd-banner hd-banner-removed", {i18n.t("history.deleted_banner")} }
            }
            if visible.is_empty() && !after.deleted {
                div { class: "hd-empty", {i18n.t("history.no_difference")} }
            }
            for section in visible {
                div { class: "hd-section", key: "{section.title}",
                    h4 { class: "hd-section-title", "{section.title}" }
                    table { class: "hd-table",
                        thead {
                            tr {
                                th { class: "hd-col-label" }
                                th { class: "hd-col-value", "{before_heading}" }
                                th { class: "hd-col-value", "{after_heading}" }
                            }
                        }
                        for group in section.groups.iter().filter(|g| !changes_only || g.kind != ChangeKind::Unchanged) {
                            tbody { class: "hd-group {group_class(group.kind)}",
                                if !group.title.is_empty() {
                                    tr { class: "hd-group-title",
                                        td { colspan: "3",
                                            span { class: "hd-kind", {kind_label(&i18n, group.kind)} }
                                            "{group.title}"
                                        }
                                    }
                                }
                                for row in group.rows.iter().filter(|r| !changes_only || r.differs()) {
                                    tr { class: if row.differs() { "hd-row hd-row-changed" } else { "hd-row" },
                                        th { class: "hd-label", scope: "row", "{row.label}" }
                                        td { class: if row.before.is_empty() { "hd-before hd-none" } else { "hd-before" }, {value_or_dash(&row.before)} }
                                        td { class: if row.after.is_empty() { "hd-after hd-none" } else { "hd-after" }, {value_or_dash(&row.after)} }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// « Version 3 », or « Version 4 (current) » for the live record.
pub fn version_label(i18n: &I18n, version: &RecordVersion) -> String {
    let label = i18n.t_args(
        "history.version_n",
        &[("version", &version.version.to_string())],
    );
    if version.current {
        format!("{label} ({})", i18n.t("history.current"))
    } else {
        label
    }
}

fn group_class(kind: ChangeKind) -> &'static str {
    match kind {
        ChangeKind::Added => "hd-added",
        ChangeKind::Removed => "hd-removed",
        ChangeKind::Changed => "hd-changed",
        ChangeKind::Unchanged => "hd-unchanged",
    }
}

fn kind_label(i18n: &I18n, kind: ChangeKind) -> String {
    i18n.t(match kind {
        ChangeKind::Added => "history.kind.added",
        ChangeKind::Removed => "history.kind.removed",
        ChangeKind::Changed => "history.kind.changed",
        ChangeKind::Unchanged => "history.kind.unchanged",
    })
}

fn value_or_dash(value: &str) -> String {
    if value.is_empty() {
        "—".to_string()
    } else {
        value.to_string()
    }
}

/// Styles of the comparison and of the history pages, on the shared tokens.
pub const HISTORY_STYLES: &str = r#"
    .hd { display: flex; flex-direction: column; gap: 18px; }
    .hd-banner {
        padding: 10px 14px;
        border-radius: var(--radius);
        font-size: 0.875rem;
    }
    .hd-banner-removed {
        background: color-mix(in srgb, var(--danger) 14%, transparent);
        color: var(--danger-text);
        border: 1px solid color-mix(in srgb, var(--danger) 40%, transparent);
    }
    .hd-empty { color: var(--text-muted); font-style: italic; }
    .hd-section-title {
        font-family: var(--font-heading);
        font-size: 0.95rem;
        color: var(--orange);
        margin: 0 0 8px;
    }
    .hd-table {
        width: 100%;
        border-collapse: collapse;
        table-layout: fixed;
        font-size: 0.85rem;
    }
    .hd-table th, .hd-table td {
        padding: 6px 10px;
        border-bottom: 1px solid var(--border);
        text-align: start;
        vertical-align: top;
        white-space: pre-wrap;
        overflow-wrap: anywhere;
    }
    .hd-col-label { width: 22%; }
    .hd-col-value {
        width: 39%;
        color: var(--text-secondary);
        font-weight: 600;
        font-size: 0.78rem;
        text-transform: uppercase;
        letter-spacing: 0.04em;
    }
    .hd-label { color: var(--text-secondary); font-weight: 400; }
    .hd-group-title td {
        padding-top: 12px;
        font-weight: 700;
        color: var(--text-primary);
    }
    .hd-kind {
        display: inline-block;
        margin-inline-end: 8px;
        padding: 1px 8px;
        border-radius: 12px;
        font-size: 0.7rem;
        font-weight: 600;
        text-transform: uppercase;
        letter-spacing: 0.04em;
        border: 1px solid var(--border);
        color: var(--text-secondary);
    }
    .hd-added .hd-kind { color: var(--green); border-color: var(--green); }
    .hd-removed .hd-kind { color: var(--danger-text); border-color: var(--danger); }
    .hd-changed .hd-kind { color: var(--orange); border-color: var(--orange); }
    .hd-row-changed .hd-before {
        background: color-mix(in srgb, var(--danger) 12%, transparent);
    }
    .hd-row-changed .hd-after {
        background: color-mix(in srgb, var(--green) 14%, transparent);
    }
    .hd-removed .hd-row-changed .hd-before { text-decoration: line-through; }
    .hd-row-changed .hd-none { background: none; color: var(--text-muted); }

    @media (max-width: 768px) {
        .hd-col-label { width: 30%; }
        .hd-col-value { width: 35%; }
        .hd-table th, .hd-table td { padding: 5px 6px; }
    }
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Language;
    use chrono::Utc;
    use oxidgene_core::history::{
        AuditCategory, AuditDetails, AuditSubject, RecordLabel, RecordType,
    };
    use oxidgene_core::{Calendar, DateQualifier, EventType, NameType, Privacy, Sex};

    fn entry() -> AuditEntry {
        AuditEntry {
            id: Uuid::nil(),
            tree_id: Uuid::nil(),
            occurred_at: Utc::now(),
            category: AuditCategory::Data,
            action: AuditAction::Update,
            entity: AuditEntity::Event,
            entity_id: None,
            subject: Some(AuditSubject::Person),
            subject_id: None,
            label: None,
            details: AuditDetails {
                event_type: Some(EventType::Birth),
                ..AuditDetails::default()
            },
            version_count: 1,
        }
    }

    fn name(id: u128, given: &str) -> NameSnapshot {
        NameSnapshot {
            id: Uuid::from_u128(id),
            name_type: NameType::Birth,
            given_names: Some(given.to_string()),
            surname: Some("Fixture".to_string()),
            surname_prefix: None,
            prefix: None,
            suffix: None,
            nickname: None,
            is_primary: true,
            sort_order: 0,
        }
    }

    fn birth(place: Option<Uuid>) -> EventSnapshot {
        EventSnapshot {
            id: Uuid::from_u128(10),
            event_type: EventType::Birth,
            date_value: Some("1900".to_string()),
            date_sort: None,
            date_qualifier: DateQualifier::Exact,
            date_value2: None,
            calendar: Calendar::Gregorian,
            cause: None,
            age: None,
            agency: None,
            place_id: place,
            description: None,
            witnesses: Vec::new(),
            notes: Vec::new(),
            citations: Vec::new(),
        }
    }

    fn version(number: i32, person: PersonSnapshot, labels: Vec<RecordLabel>) -> RecordVersion {
        RecordVersion {
            id: Uuid::from_u128(number as u128),
            tree_id: Uuid::nil(),
            record_type: RecordType::Person,
            record_id: Uuid::nil(),
            version: number,
            current: false,
            deleted: false,
            entry: Some(entry()),
            snapshot: Some(RecordSnapshot::Person(person)),
            labels,
        }
    }

    fn person(names: Vec<NameSnapshot>, events: Vec<EventSnapshot>) -> PersonSnapshot {
        PersonSnapshot {
            sex: Sex::Female,
            privacy: Privacy::Default,
            names,
            events,
            notes: Vec::new(),
            citations: Vec::new(),
            parents: Vec::new(),
            unions: Vec::new(),
        }
    }

    #[test]
    fn a_changed_field_is_the_only_changed_row() {
        let i18n = I18n(Language::En);
        let before = version(1, person(vec![name(1, "Alpha")], vec![]), vec![]);
        let after = version(2, person(vec![name(1, "Beta")], vec![]), vec![]);
        let sections = diff_versions(&i18n, Some(&before), &after);

        let identity = &sections[0];
        assert!(!identity.differs());
        let names = sections
            .iter()
            .find(|s| s.title == i18n.t("history.section.names"))
            .unwrap();
        assert_eq!(names.groups[0].kind, ChangeKind::Changed);
        let changed: Vec<&DiffRow> = names.groups[0]
            .rows
            .iter()
            .filter(|r| r.differs())
            .collect();
        assert_eq!(changed.len(), 1);
        assert_eq!(changed[0].before, "Alpha");
        assert_eq!(changed[0].after, "Beta");
    }

    #[test]
    fn places_read_through_each_version_s_own_labels() {
        let i18n = I18n(Language::En);
        let place = Uuid::from_u128(99);
        let label = |text: &str| {
            vec![RecordLabel {
                id: place,
                label: text.to_string(),
            }]
        };
        let before = version(1, person(vec![], vec![birth(None)]), vec![]);
        let after = version(
            2,
            person(vec![], vec![birth(Some(place))]),
            label("Springfield"),
        );
        let sections = diff_versions(&i18n, Some(&before), &after);
        let events = sections
            .iter()
            .find(|s| s.title == i18n.t("history.section.events"))
            .unwrap();
        let place_row = events.groups[0]
            .rows
            .iter()
            .find(|r| r.label == i18n.t("history.field.place"))
            .unwrap();
        assert_eq!(place_row.before, "");
        assert_eq!(place_row.after, "Springfield");
    }

    #[test]
    fn a_renamed_place_is_not_a_change() {
        let i18n = I18n(Language::En);
        let place = Uuid::from_u128(99);
        let label = |text: &str| {
            vec![RecordLabel {
                id: place,
                label: text.to_string(),
            }]
        };
        let before = version(
            1,
            person(vec![], vec![birth(Some(place))]),
            label("Old name"),
        );
        let after = version(
            2,
            person(vec![], vec![birth(Some(place))]),
            label("New name"),
        );
        let sections = diff_versions(&i18n, Some(&before), &after);
        assert!(sections.iter().all(|s| !s.differs()));
        let events = sections
            .iter()
            .find(|s| s.title == i18n.t("history.section.events"))
            .unwrap();
        let place_row = events.groups[0]
            .rows
            .iter()
            .find(|r| r.label == i18n.t("history.field.place"))
            .unwrap();
        // Each side still reads as it did at the time.
        assert_eq!(place_row.before, "Old name");
        assert_eq!(place_row.after, "New name");
    }

    #[test]
    fn a_deleted_state_lists_nothing_and_reads_as_empty_against() {
        let i18n = I18n(Language::En);
        let named = version(1, person(vec![name(1, "Alpha")], vec![]), vec![]);
        let mut deleted = version(2, person(vec![], vec![]), vec![]);
        deleted.deleted = true;
        deleted.snapshot = None;
        assert!(diff_versions(&i18n, Some(&named), &deleted).is_empty());
        let mut back = version(3, person(vec![name(1, "Alpha")], vec![]), vec![]);
        back.current = true;
        assert!(
            diff_versions(&i18n, Some(&deleted), &back)
                .iter()
                .flat_map(|s| &s.groups)
                .all(|g| g.kind == ChangeKind::Added)
        );
        assert_eq!(
            version_label(&i18n, &back),
            format!("Version 3 ({})", i18n.t("history.current"))
        );
        assert_eq!(version_label(&i18n, &named), "Version 1");
    }

    #[test]
    fn a_first_version_shows_everything_as_added() {
        let i18n = I18n(Language::En);
        let first = version(1, person(vec![name(1, "Alpha")], vec![]), vec![]);
        let sections = diff_versions(&i18n, None, &first);
        assert!(
            sections
                .iter()
                .flat_map(|s| &s.groups)
                .all(|g| g.kind == ChangeKind::Added)
        );
    }

    #[test]
    fn an_entry_reads_as_its_action_and_what_it_touched() {
        let i18n = I18n(Language::En);
        assert_eq!(
            describe_entry(&i18n, &entry()),
            format!(
                "{} — {} ({})",
                i18n.t("history.action.update"),
                i18n.t("history.entity.event"),
                i18n.t("event.type.birth")
            )
        );
    }

    #[test]
    fn every_history_key_exists_in_every_language() {
        let keys: Vec<String> = [
            AuditAction::ALL
                .iter()
                .map(|a| format!("history.action.{a}"))
                .collect::<Vec<_>>(),
            AuditEntity::ALL
                .iter()
                .map(|e| format!("history.entity.{e}"))
                .collect(),
            AuditCategory::ALL
                .iter()
                .map(|c| format!("history.category.{c}"))
                .collect(),
        ]
        .concat();
        for language in Language::ALL {
            for key in &keys {
                assert!(
                    language.translations().contains_key(key),
                    "{} lacks {key}",
                    language.code()
                );
            }
        }
    }
}
