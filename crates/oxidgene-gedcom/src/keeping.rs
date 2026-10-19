//! Kept GEDCOM, both ways: what an import keeps beside the records it reads
//! ([`compute`]), and what an export puts back beneath the records it writes
//! ([`put_back`]). The comparison and the merge are [`crate::kept`]'s; this
//! module decides what the identifiers mean on each side. See `docs/api.md`,
//! "Kept GEDCOM".

use std::collections::{HashMap, HashSet};

use chrono::Utc;
use oxidged::model::Dataset;
use oxidged::tree::{Structure, Xref};
use oxidgene_core::types::{GedcomKept, KeptEntityKind};
use uuid::Uuid;

use crate::ImportResult;
use crate::export::{self, ExportChoices, ExportOptions, ExportTarget, SubmitterSettings};
use crate::kept::{self, KeptNode};

/// The entity an original xref names: its kind and id.
type Ids = HashMap<String, (KeptEntityKind, Uuid)>;

/// Computes what the import keeps, into `result.kept`.
///
/// `data` is the dataset the import read, after its restoration of kept media,
/// and `result` the finished import. The file is exported again for its own
/// version, and each of its records compared with the export's. A failure of
/// the export is a warning: the import keeps nothing then.
pub(crate) fn compute(data: &Dataset, result: &mut ImportResult) {
    let version = data.version();
    let destination_id = if version.is_v7() {
        "generic-70"
    } else {
        export::DEFAULT_DESTINATION
    };
    let Some(destination) = export::destination(destination_id) else {
        result
            .warnings
            .push("Kept GEDCOM was not computed: no export profile for the version".to_owned());
        return;
    };
    let submitter = result.submitter.clone().unwrap_or_default();
    let options = ExportOptions {
        choices: ExportChoices {
            destination,
            include_notes_and_sources: true,
            include_media: true,
        },
        target: ExportTarget::Gedcom,
        self_person_id: None,
        submitter: SubmitterSettings {
            name: submitter.name.as_deref(),
            email: submitter.email.as_deref(),
            address: submitter.address.as_deref(),
        },
    };
    let exported = match export::export_gedcom(&result.records(), &options) {
        Ok(exported) => exported,
        Err(error) => {
            result
                .warnings
                .push(format!("Kept GEDCOM was not computed: {error}"));
            return;
        }
    };

    let original = data.to_structures_for(version);
    let exported_structures = Dataset::parse(exported.gedcom.clone()).to_structures_for(version);
    let tree_id = result.tree_id;
    let rows = {
        let ids = original_ids(&original, result, tree_id);
        let exported_ids: HashMap<String, Uuid> = exported
            .xrefs
            .iter()
            .map(|(xref, id)| {
                let id = if id.is_nil() { tree_id } else { *id };
                (xref.clone(), id)
            })
            .collect();
        compare(
            &original,
            &exported_structures,
            &ids,
            &exported_ids,
            tree_id,
        )
    };
    result.kept = rows;
}

/// The entities of the original file: the imported entities by their xref,
/// the submitter `HEAD.SUBM` names, and a fresh id for every other record, as
/// pointers to those records must resolve.
fn original_ids(original: &[Structure], result: &ImportResult, tree_id: Uuid) -> Ids {
    let mut ids: Ids = HashMap::new();
    let maps = [
        (&result.person_by_xref, KeptEntityKind::Person),
        (&result.family_by_xref, KeptEntityKind::Family),
        (&result.source_by_xref, KeptEntityKind::Source),
        (&result.repository_by_xref, KeptEntityKind::Repository),
        (&result.media_by_xref, KeptEntityKind::Media),
        (&result.note_by_xref, KeptEntityKind::Note),
    ];
    for (map, kind) in maps {
        for (xref, id) in map {
            ids.insert(xref.clone(), (kind, *id));
        }
    }
    if let Some(xref) = header_submitter(original) {
        ids.insert(xref, (KeptEntityKind::Submitter, tree_id));
    }
    for record in original.iter().filter(|r| is_compared(r)) {
        if let Some(xref) = record.xref.as_ref() {
            ids.entry(xref.as_str().to_owned())
                .or_insert((KeptEntityKind::Record, Uuid::now_v7()));
        }
    }
    ids
}

/// The xref of the submitter the header points at.
fn header_submitter(original: &[Structure]) -> Option<String> {
    let header = original.iter().find(|s| s.tag.as_str() == "HEAD")?;
    let submitter = header
        .substructures
        .iter()
        .find(|s| s.tag.as_str() == "SUBM")?;
    submitter.pointer().map(|x| x.as_str().to_owned())
}

/// Whether a structure is a record the comparison covers: any record other
/// than the header and the trailer, with an xref or without one.
fn is_compared(record: &Structure) -> bool {
    // An OxidGene place record is read whole on import, its tags included, so
    // it is never kept beside the export that writes it again.
    !matches!(
        record.tag.as_str(),
        "HEAD" | "TRLR" | crate::export::PLACE_TAG
    )
}

/// The kept rows of each original record against its exported counterpart.
fn compare(
    original: &[Structure],
    exported: &[Structure],
    ids: &Ids,
    exported_ids: &HashMap<String, Uuid>,
    tree_id: Uuid,
) -> Vec<GedcomKept> {
    let resolve_original = |xref: &str| ids.get(xref).map(|(_, id)| *id);
    let resolve_exported = |xref: &str| exported_ids.get(xref).copied();
    let mut exported_by_id: HashMap<Uuid, &Structure> = HashMap::new();
    for record in exported {
        let Some(id) = record
            .xref
            .as_ref()
            .and_then(|x| resolve_exported(x.as_str()))
        else {
            continue;
        };
        exported_by_id.entry(id).or_insert(record);
    }
    let witnessed = witnesses_by_family(exported, &resolve_exported);
    let mut rows = Vec::new();
    for record in original.iter().filter(|r| is_compared(r)) {
        // A record without an xref has no entity and no pointer names it: it
        // is kept whole under a fresh id, and written without an xref.
        let (kind, id) = match record.xref.as_ref().and_then(|x| ids.get(x.as_str())) {
            Some(&entity) => entity,
            None => (KeptEntityKind::Record, Uuid::now_v7()),
        };
        let partner = match kind {
            KeptEntityKind::Record => None,
            _ => exported_by_id.get(&id),
        };
        let (kind, nodes) = match partner {
            Some(partner) => (
                kind,
                kept::diff_witnessed(
                    record,
                    partner,
                    &resolve_original,
                    &resolve_exported,
                    record.tag.as_str(),
                    witnessed.get(&id).unwrap_or(&HashSet::new()),
                ),
            ),
            // A record the export writes nothing for (a shared note nobody
            // points at, a record OxidGene has no entity for) is kept whole.
            // Its row keeps the record's entity id, so pointers to it resolve,
            // and its own tag, which is what the choices of an export act on.
            None => (
                KeptEntityKind::Record,
                kept::keep_whole(record, &resolve_original)
                    .map(|mut node| {
                        node.xref = record.xref.is_some();
                        node
                    })
                    .into_iter()
                    .collect(),
            ),
        };
        // The submitter's row is kept even empty: a pointer to the tree's
        // submitter, kept anywhere in the file, finds the submitter through it.
        if nodes.is_empty() && kind != KeptEntityKind::Submitter {
            continue;
        }
        rows.push(GedcomKept {
            id: Uuid::now_v7(),
            tree_id,
            entity_kind: kind,
            entity_id: id,
            nodes: kept::to_json(&nodes),
            created_at: Utc::now(),
        });
    }
    rows
}

/// The witnesses each family holds in the export, by the family's entity id:
/// the individuals whose record carries an `ASSO` or `_ASSO` to the family.
/// GEDCOM 5.5.1 writes a family event's witness beneath the witness's own
/// record, not beneath the family, so the family's record alone does not show
/// them.
fn witnesses_by_family(
    exported: &[Structure],
    resolve_exported: &dyn Fn(&str) -> Option<Uuid>,
) -> HashMap<Uuid, HashSet<Uuid>> {
    let mut witnesses: HashMap<Uuid, HashSet<Uuid>> = HashMap::new();
    for record in exported.iter().filter(|r| r.tag.as_str() == "INDI") {
        let Some(person) = record
            .xref
            .as_ref()
            .and_then(|x| resolve_exported(x.as_str()))
        else {
            continue;
        };
        for child in &record.substructures {
            if !matches!(child.tag.as_str(), "ASSO" | "_ASSO") {
                continue;
            }
            if let Some(family) = child.pointer().and_then(|p| resolve_exported(p.as_str())) {
                witnesses.entry(family).or_default().insert(person);
            }
        }
    }
    witnesses
}

/// A kept row an export writes, its nodes parsed.
struct Chosen {
    kind: KeptEntityKind,
    id: Uuid,
    nodes: Vec<KeptNode>,
}

/// Puts what is kept back beneath `structures`, the records of an export, and
/// returns how many kept nodes were left out: an anchor no longer exported,
/// or a pointer to an entity the export does not write.
///
/// `xrefs` is the export's xref of every record it writes, mapped to the entity
/// id (the submitter to `Uuid::nil()`). Records kept for the tree are added as
/// `@K…@` records before the trailer, and their ids resolve as the others do.
/// A row that cannot be read is a warning.
pub(crate) fn put_back(
    structures: &mut Vec<Structure>,
    rows: &[GedcomKept],
    choices: ExportChoices,
    xrefs: &HashMap<String, Uuid>,
    warnings: &mut Vec<String>,
) -> usize {
    let chosen = choose(rows, choices, warnings);
    let submitter = rows
        .iter()
        .find(|row| row.entity_kind == KeptEntityKind::Submitter)
        .map_or(Uuid::nil(), |row| row.entity_id);
    let table = Table::new(xrefs, &chosen, submitter);
    let xref_of = |id: Uuid| table.by_id.get(&id).cloned();
    let resolve = |xref: &str| table.by_xref.get(xref).copied();

    let mut left_out = 0;
    for row in chosen
        .iter()
        .filter(|row| row.kind == KeptEntityKind::Record)
    {
        let Some(node) = row.nodes.first() else {
            continue;
        };
        // A record the file gave no xref is written without one.
        let xref = if node.xref {
            match xref_of(row.id) {
                Some(xref) => Some(xref),
                None => {
                    left_out += 1;
                    continue;
                }
            }
        } else {
            None
        };
        match kept::to_structure(node, &xref_of) {
            Some(mut record) => {
                record.xref = xref.map(Xref::from);
                insert_before_trailer(structures, record);
            }
            None => left_out += 1,
        }
    }

    let index: HashMap<String, usize> = structures
        .iter()
        .enumerate()
        .filter_map(|(at, s)| Some((s.xref.as_ref()?.as_str().to_owned(), at)))
        .collect();
    for row in chosen
        .iter()
        .filter(|row| row.kind != KeptEntityKind::Record)
    {
        let at = xref_of(row.id).and_then(|xref| index.get(&xref).copied());
        match at {
            Some(at) => {
                left_out += kept::merge(&mut structures[at], &row.nodes, &xref_of, &resolve);
            }
            None => left_out += row.nodes.len(),
        }
    }
    left_out
}

/// The rows an export with `choices` writes, parsed. A row the choices leave
/// out is dropped, and so is a node of a tag they leave out (a note, a source,
/// a repository, a medium's link).
fn choose(rows: &[GedcomKept], choices: ExportChoices, warnings: &mut Vec<String>) -> Vec<Chosen> {
    let dropped = dropped_tags(choices);
    let mut chosen = Vec::new();
    for row in rows {
        if !is_chosen(row.entity_kind, choices) {
            continue;
        }
        let mut nodes = match kept::from_json(&row.nodes) {
            Ok(nodes) => nodes,
            Err(error) => {
                warnings.push(format!(
                    "A kept GEDCOM row could not be read and was left out: {error}"
                ));
                continue;
            }
        };
        // A record kept whole is one node: left out with its own tag.
        if row.entity_kind == KeptEntityKind::Record
            && nodes
                .first()
                .is_none_or(|node| dropped.contains(&node.tag.as_str()))
        {
            continue;
        }
        without_tags(&mut nodes, &dropped);
        if nodes.is_empty() {
            continue;
        }
        chosen.push(Chosen {
            kind: row.entity_kind,
            id: row.entity_id,
            nodes,
        });
    }
    chosen
}

/// Whether a row of `kind` is written under `choices`.
fn is_chosen(kind: KeptEntityKind, choices: ExportChoices) -> bool {
    match kind {
        KeptEntityKind::Note | KeptEntityKind::Source | KeptEntityKind::Repository => {
            choices.include_notes_and_sources
        }
        KeptEntityKind::Media => choices.include_media,
        _ => true,
    }
}

/// The tags an export with `choices` does not write: the notes and sources
/// with their repositories, and the media links.
fn dropped_tags(choices: ExportChoices) -> Vec<&'static str> {
    let mut tags = Vec::new();
    if !choices.include_notes_and_sources {
        tags.extend(["NOTE", "SNOTE", "SOUR", "REPO"]);
    }
    if !choices.include_media {
        tags.push("OBJE");
    }
    tags
}

/// Removes, at any depth, the nodes tagged with one of `tags`.
fn without_tags(nodes: &mut Vec<KeptNode>, tags: &[&str]) {
    nodes.retain(|node| !tags.contains(&node.tag.as_str()));
    for node in nodes.iter_mut() {
        without_tags(&mut node.children, tags);
    }
}

/// Inserts `record` before the trailer, which ends the records.
fn insert_before_trailer(structures: &mut Vec<Structure>, record: Structure) {
    match structures.last() {
        Some(last) if last.tag.as_str() == "TRLR" => {
            let at = structures.len() - 1;
            structures.insert(at, record);
        }
        _ => structures.push(record),
    }
}

/// The xref of each entity an export writes, both ways. Notes are the one
/// kind an entity may have several xrefs for (one shared record per text);
/// the first in xref order stands for the group.
#[derive(Default)]
struct Table {
    by_id: HashMap<Uuid, String>,
    by_xref: HashMap<String, Uuid>,
}

impl Table {
    fn new(xrefs: &HashMap<String, Uuid>, chosen: &[Chosen], submitter: Uuid) -> Self {
        let mut table = Self::default();
        let mut sorted: Vec<(&String, &Uuid)> = xrefs.iter().collect();
        sorted.sort();
        for (xref, id) in sorted {
            let id = if id.is_nil() { submitter } else { *id };
            table.insert(xref.clone(), id);
        }
        let records = chosen.iter().filter(|row| {
            row.kind == KeptEntityKind::Record && row.nodes.first().is_some_and(|node| node.xref)
        });
        for (n, row) in records.enumerate() {
            table.insert(format!("@K{}@", n + 1), row.id);
        }
        table
    }

    fn insert(&mut self, xref: String, id: Uuid) {
        self.by_id.entry(id).or_insert_with(|| xref.clone());
        self.by_xref.insert(xref, id);
    }
}
