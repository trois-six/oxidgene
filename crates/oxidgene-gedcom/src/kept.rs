//! Kept GEDCOM: the structures of a file that OxidGene's model has no place
//! for, compared at import with the same records as OxidGene exports them, and
//! put back beneath the exported records at export (see `docs/api.md`, "Kept
//! GEDCOM").
//!
//! This module is pure: it compares and merges [`oxidged`] structures, and
//! takes the pointer resolution from its callers, so the import and the export
//! decide what the identifiers mean.

use std::collections::HashSet;

use oxidged::model::{EventKind, TagId};
use oxidged::tree::{Payload, Structure, Xref};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A kept structure's payload, its pointer resolved to the entity it names.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum KeptPayload {
    /// No payload.
    None,
    /// The text of the structure.
    Text(String),
    /// The entity the pointer names.
    Pointer(Uuid),
    /// The GEDCOM 7.0 null pointer `@VOID@`, which names no entity.
    Void,
}

/// A kept substructure, or an anchor leading to kept substructures.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KeptNode {
    /// The tag, as the file wrote it.
    pub tag: String,
    /// The payload, with a pointer resolved to its entity.
    pub payload: KeptPayload,
    /// `true`: kept whole, written back as it is with all its `children`.
    /// `false`: an anchor — an understood structure the export writes itself,
    /// whose `children` are what is kept beneath it.
    pub whole: bool,
    /// For an anchor: its position, from 0, among the exported substructures
    /// with its tag that name the same entity (for a pointer) or are of the
    /// same kind (otherwise). 0 for a whole node.
    pub position: u32,
    /// For a kept record (the top node of a `Record` row): whether the file
    /// gave it an xref, which the export writes back. Always `false` beneath.
    #[serde(default)]
    pub xref: bool,
    /// The kept substructures beneath this node.
    pub children: Vec<KeptNode>,
}

/// What is kept beneath `original`, a record (or substructure) of the file,
/// against `exported`, the same entity as OxidGene exports it. `resolve_original`
/// and `resolve_exported` turn a pointer (`"@I1@"`) of each side into the entity
/// it names; `record_tag` is the tag of the record being compared (`INDI`, `FAM`, …).
#[must_use]
pub fn diff(
    original: &Structure,
    exported: &Structure,
    resolve_original: &dyn Fn(&str) -> Option<Uuid>,
    resolve_exported: &dyn Fn(&str) -> Option<Uuid>,
    record_tag: &str,
) -> Vec<KeptNode> {
    diff_witnessed(
        original,
        exported,
        resolve_original,
        resolve_exported,
        record_tag,
        &HashSet::new(),
    )
}

/// [`diff`], where `witnessed` holds the entities the export writes as
/// witnesses of `exported` from elsewhere: a family's witnesses, whose
/// `ASSO` the export writes beneath each witness's own record (5.5.1).
#[must_use]
pub fn diff_witnessed(
    original: &Structure,
    exported: &Structure,
    resolve_original: &dyn Fn(&str) -> Option<Uuid>,
    resolve_exported: &dyn Fn(&str) -> Option<Uuid>,
    record_tag: &str,
    witnessed: &HashSet<Uuid>,
) -> Vec<KeptNode> {
    let mut associations = association_targets(exported, resolve_exported);
    associations.extend(witnessed);
    diff_children(
        original,
        exported,
        resolve_original,
        resolve_exported,
        &Context {
            record_tag,
            associations: &associations,
        },
        0,
    )
}

/// What the comparison of one record knows beyond its two sides.
struct Context<'a> {
    /// The tag of the record being compared (`INDI`, `FAM`, …).
    record_tag: &'a str,
    /// The entities the exported record associates (every `ASSO` and `_ASSO`
    /// pointer at any depth of it), collected once per record.
    associations: &'a HashSet<Uuid>,
}

/// The entities the record `exported` associates, at any depth.
fn association_targets(
    exported: &Structure,
    resolve_exported: &dyn Fn(&str) -> Option<Uuid>,
) -> HashSet<Uuid> {
    let mut targets = HashSet::new();
    let mut pending: Vec<&Structure> = exported.substructures.iter().collect();
    while let Some(structure) = pending.pop() {
        if is_association(structure)
            && let Some(id) = structure
                .pointer()
                .and_then(|p| resolve_exported(p.as_str()))
        {
            targets.insert(id);
        }
        pending.extend(structure.substructures.iter());
    }
    targets
}

/// A whole structure kept as it is (a record OxidGene has no entity for, or a
/// substructure without partner); `None` when its own pointer resolves to
/// nothing. Pointers inside that resolve to nothing are dropped with their
/// substructure.
#[must_use]
pub fn keep_whole(
    structure: &Structure,
    resolve: &dyn Fn(&str) -> Option<Uuid>,
) -> Option<KeptNode> {
    let payload = stored_payload(&structure.payload, resolve)?;
    let children = structure
        .substructures
        .iter()
        .filter_map(|child| keep_whole(child, resolve))
        .collect();
    Some(KeptNode {
        tag: structure.tag.as_str().to_owned(),
        payload,
        whole: true,
        position: 0,
        xref: false,
        children,
    })
}

/// Puts `kept` back beneath `exported`. `xref_of` gives the export's pointer
/// text (`"@I3@"`) for an entity; `resolve_exported` reads the exported
/// pointers back to entities. Returns how many kept nodes were left out
/// (anchor not found, or a pointer naming an entity the export has no xref for).
pub fn merge(
    exported: &mut Structure,
    kept: &[KeptNode],
    xref_of: &dyn Fn(Uuid) -> Option<String>,
    resolve_exported: &dyn Fn(&str) -> Option<Uuid>,
) -> usize {
    let mut left_out = 0;
    for node in kept {
        if node.whole {
            match to_structure(node, xref_of) {
                Some(structure) => put_whole(exported, structure),
                None => left_out += 1,
            }
        } else {
            match find_partner(exported, node, resolve_exported) {
                Some(index) => {
                    let partner = &mut exported.substructures[index];
                    left_out += merge(partner, &node.children, xref_of, resolve_exported);
                }
                None => left_out += 1,
            }
        }
    }
    left_out
}

/// Puts a structure kept whole beneath `exported`: in place of a bare
/// structure of its tag, when the export writes one, or after the rest.
///
/// A record compared as GEDCOM 7.x keeps whole an ordinance whose content the
/// model has no place for (`1 BAPL` with an unlisted status): its event is
/// bare, which 7.x leaves out (§1.2). Exported as GEDCOM 5.5.1, which writes
/// the bare event, the kept structure stands for it rather than doubling it;
/// a bare structure holds nothing for the kept one to lose.
fn put_whole(exported: &mut Structure, structure: Structure) {
    let bare = exported.substructures.iter().position(|s| {
        s.tag == structure.tag && matches!(s.payload, Payload::None) && s.substructures.is_empty()
    });
    match bare {
        Some(index) => exported.substructures[index] = structure,
        None => exported.substructures.push(structure),
    }
}

/// A kept node as a structure, pointers turned into the export's xrefs;
/// `None` (and the node left out) when a pointer has no xref.
#[must_use]
pub fn to_structure(
    node: &KeptNode,
    xref_of: &dyn Fn(Uuid) -> Option<String>,
) -> Option<Structure> {
    let mut structure = Structure::new(node.tag.as_str());
    structure.payload = match &node.payload {
        KeptPayload::None => Payload::None,
        KeptPayload::Text(text) => Payload::Text(text.as_str().into()),
        KeptPayload::Pointer(id) => Payload::Pointer(Xref::from(xref_of(*id)?)),
        KeptPayload::Void => Payload::Pointer(Xref::from(Xref::VOID)),
    };
    structure.substructures = node
        .children
        .iter()
        .filter_map(|child| to_structure(child, xref_of))
        .collect();
    Some(structure)
}

/// JSON storage of a list of kept nodes, and back.
#[must_use]
pub fn to_json(nodes: &[KeptNode]) -> String {
    serde_json::to_string(nodes).expect("kept nodes serialise to JSON")
}

/// Reads a list of kept nodes stored by [`to_json`].
///
/// # Errors
///
/// Returns the JSON error when `json` is not a list of kept nodes.
pub fn from_json(json: &str) -> Result<Vec<KeptNode>, serde_json::Error> {
    serde_json::from_str(json)
}

/// The entity a pointer names, `None` for `@VOID@` (which names none) and for a
/// pointer that resolves to nothing.
fn entity_of(structure: &Structure, resolve: &dyn Fn(&str) -> Option<Uuid>) -> Option<Uuid> {
    structure
        .pointer()
        .filter(|p| !p.is_void())
        .and_then(|p| resolve(p.as_str()))
}

/// Whether a structure is a `@VOID@` pointer.
fn is_void(structure: &Structure) -> bool {
    structure.pointer().is_some_and(Xref::is_void)
}

/// Whether two tags are the same for pairing: `SNOTE` is written `NOTE` in
/// GEDCOM 5.5.1.
fn same_tag(a: &str, b: &str) -> bool {
    canonical(a) == canonical(b)
}

fn canonical(tag: &str) -> &str {
    if tag == "SNOTE" { "NOTE" } else { tag }
}

/// Whether a structure is an association, which the import turns into an event
/// witness when it can, and otherwise reads as nothing.
fn is_association(structure: &Structure) -> bool {
    matches!(structure.tag.as_str(), "ASSO" | "_ASSO")
}

/// An unpaired association is understood only when the exported record
/// associates the same entity, anywhere: the export then writes the witness
/// itself, where the version permits, and an association it writes at the same
/// place pairs as any structure does, so that what it holds beneath (a
/// witness's note, a citation) is kept. An association the export writes
/// nowhere for its entity is kept whole, as the import could not make it a
/// witness, or the export would have written it.
fn is_understood_association(
    child: &Structure,
    resolve_original: &dyn Fn(&str) -> Option<Uuid>,
    context: &Context<'_>,
) -> bool {
    is_association(child)
        && entity_of(child, resolve_original).is_some_and(|id| context.associations.contains(&id))
}

/// An inline `OBJE` (one holding its `FILE`, not a pointer) is a medium the
/// import reads as a record of its own, and the export writes as one: what it
/// holds is that record's, compared with nothing beneath the link.
fn is_inline_medium(structure: &Structure) -> bool {
    structure.tag.as_str() == "OBJE" && structure.pointer().is_none()
}

/// Whether an unpaired child of `parent` is understood as the structure it
/// belongs to. Each call number of a repository citation is one link the
/// export writes beside the citation, whichever link the pairing chose. A
/// `TYPE` under an association names the kind of record its pointer names,
/// which the pointer itself says, and the export writes none.
fn is_understood_beneath(parent: &Structure, child: &Structure) -> bool {
    if child.tag.as_str() == crate::export::TAG_TAG {
        // A pointer to a shared note carries no tags: its record does.
        let shared_pointer = parent.tag.as_str() == "NOTE" && parent.pointer().is_some();
        return !shared_pointer && is_tagged_structure(parent.tag.as_str());
    }
    matches!(
        (parent.tag.as_str(), child.tag.as_str()),
        ("REPO", "CALN") | ("ASSO" | "_ASSO", "TYPE")
    )
}

/// Whether a structure is one the export writes an `_OXIDGENE_TAG` line
/// beneath, and the import reads it from: an event or attribute structure
/// (`BIRT`, `OCCU`), an LDS ordinance, a military service or separation (`MILI`,
/// `SEP`, which the import reads as generic events), a `SOUR` (a source record,
/// or the pointer a citation is written as), a `PLAC` (the place an event
/// names, whose tags repeat in each one) and a `NOTE` (an inline note; a
/// pointer is judged by [`is_understood_beneath`]). Any other structure holds
/// no tag line the import reads, so one there is kept.
fn is_tagged_structure(tag: &str) -> bool {
    tag == "SOUR"
        || tag == "PLAC"
        || tag == "NOTE"
        || tag == "EVEN"
        || matches!(tag, "MILI" | "SEP")
        || is_lds_ordinance(tag)
        || EventKind::parse(tag).is_known()
}

/// Whether a `TEXT` beneath a citation `SOUR` (a pointerless `SOUR` at
/// `depth` 1 or more, not a source record) is understood. GEDCOM 5.5.1 gives
/// a citation no `TEXT` of its own, so the import reads it as the citation's
/// text and the export writes it as the `DATA.TEXT` of the citation it pairs
/// with: understood when the exported partner holds such a `DATA` with a
/// `TEXT`. Nothing else is judged this way.
fn is_understood_citation_text(
    original: &Structure,
    exported: &Structure,
    child: &Structure,
    depth: usize,
) -> bool {
    depth >= 1
        && original.tag.as_str() == "SOUR"
        && original.pointer().is_none()
        && child.tag.as_str() == "TEXT"
        && exported
            .substructures
            .iter()
            .any(|data| data.tag.as_str() == "DATA" && tagged_text(data))
}

/// Whether a structure holds a `TEXT` beneath it.
fn tagged_text(structure: &Structure) -> bool {
    structure
        .substructures
        .iter()
        .any(|s| s.tag.as_str() == "TEXT")
}

/// Whether an unpaired child is one the import reads, so that the export
/// writes it or is right to leave it out: an age, an event the import reads
/// (direct children of `INDI` and `FAM` only), the death reason the
/// `geneweb` crate writes beneath a death (`_GWDEATH`), which the import reads
/// as the cause, and an `_OXIDGENE_TAG` line directly beneath an `INDI`, `FAM`,
/// `OBJE`, `REPO` or shared note (`NOTE`, `SNOTE`) record, which the import
/// reads into the tag vocabulary and the export writes from it (a folded duplicate, or a tag the export has no place
/// for, is left out). The tag lines of a `SOUR` record, of an event, an
/// attribute or a citation are judged by [`is_understood_beneath`], as the
/// structures they sit beneath. A tag line anywhere else is not read, so it is
/// kept. An association is judged by [`is_understood_association`].
fn is_understood(child: &Structure, depth: usize, record_tag: &str) -> bool {
    // An age is stored, or left out with a warning, and an empty one says
    // nothing: none is kept.
    child.tag.as_str() == "AGE"
        || child.tag.as_str() == "_GWDEATH"
        || (child.tag.as_str() == crate::export::TAG_TAG
            && depth == 0
            && matches!(
                record_tag,
                "INDI" | "FAM" | "OBJE" | "REPO" | "NOTE" | "SNOTE"
            ))
        || (depth == 0 && is_understood_event(child, record_tag))
}

/// Whether an unpaired direct child of an `INDI` or `FAM` record is one the
/// import reads as a generic or standard event, so that the export writes it.
/// The LDS ordinances are not: each is written as its own tag, so an unpaired
/// one is missing from the export and kept whole.
fn is_understood_event(child: &Structure, record_tag: &str) -> bool {
    if !matches!(record_tag, "INDI" | "FAM") || is_lds_ordinance(child.tag.as_str()) {
        return false;
    }
    let tag = child.tag.as_str();
    if matches!(tag, "MILI" | "SEP") {
        return true;
    }
    if TagId::standard(tag).is_some() {
        // Every event the record holds is imported, an `EVEN` too, and
        // exported under the tag its type takes, which may be another one.
        return tag == "EVEN" || EventKind::parse(tag).is_known();
    }
    !tag.is_empty() && !tag.starts_with('_') && !child.substructures.is_empty()
}

/// Whether a tag is an LDS ordinance. oxidged reads back the extension tags
/// its writer repairs one into (a 5.5.1 `_INIL`), so only GEDCOM's own tags
/// reach the comparison.
fn is_lds_ordinance(tag: &str) -> bool {
    matches!(tag, "BAPL" | "CONL" | "INIL" | "ENDL" | "SLGC" | "SLGS")
}

/// The payload as it is stored: a pointer must resolve, else `None`; the
/// `@VOID@` pointer names no entity and is stored as it is.
fn stored_payload(
    payload: &Payload,
    resolve: &dyn Fn(&str) -> Option<Uuid>,
) -> Option<KeptPayload> {
    match payload {
        Payload::None => Some(KeptPayload::None),
        Payload::Text(text) => Some(KeptPayload::Text(text.to_string())),
        Payload::Pointer(pointer) if pointer.is_void() => Some(KeptPayload::Void),
        Payload::Pointer(pointer) => resolve(pointer.as_str()).map(KeptPayload::Pointer),
    }
}

/// The partner of each original child (by index), among the exported children.
/// Pass 1 pairs pointers naming the same entity; pass 2 pairs the rest, in
/// order, with the same tag.
fn pair(
    original: &[&Structure],
    exported: &[Structure],
    resolve_original: &dyn Fn(&str) -> Option<Uuid>,
    resolve_exported: &dyn Fn(&str) -> Option<Uuid>,
) -> Vec<Option<usize>> {
    let mut partners = vec![None; original.len()];
    let mut taken = vec![false; exported.len()];
    for (i, child) in original.iter().enumerate() {
        let Some(target) = entity_of(child, resolve_original) else {
            continue;
        };
        let found = (0..exported.len()).find(|&j| {
            !taken[j]
                && same_tag(exported[j].tag.as_str(), child.tag.as_str())
                && entity_of(&exported[j], resolve_exported) == Some(target)
        });
        if let Some(j) = found {
            partners[i] = Some(j);
            taken[j] = true;
        }
    }
    for (i, child) in original.iter().enumerate() {
        if partners[i].is_some() {
            continue;
        }
        let found = (0..exported.len())
            .find(|&j| !taken[j] && same_tag(exported[j].tag.as_str(), child.tag.as_str()));
        if let Some(j) = found {
            partners[i] = Some(j);
            taken[j] = true;
        }
    }
    partners
}

/// The position of `exported[index]` among the exported structures with its tag
/// and its kind: a `@VOID@` pointer counts only with the others, an entity
/// pointer or text only with those that are not.
fn position_among(
    exported: &[Structure],
    index: usize,
    resolve_exported: &dyn Fn(&str) -> Option<Uuid>,
) -> u32 {
    let tag = exported[index].tag.as_str();
    let before = match entity_of(&exported[index], resolve_exported) {
        // A pointer names its entity: a structure pointing at the same entity
        // (two citations of one repository) is told apart by its position.
        Some(id) => exported[..index]
            .iter()
            .filter(|s| same_tag(s.tag.as_str(), tag) && entity_of(s, resolve_exported) == Some(id))
            .count(),
        None => {
            let void = is_void(&exported[index]);
            exported[..index]
                .iter()
                .filter(|s| same_tag(s.tag.as_str(), tag) && is_void(s) == void)
                .count()
        }
    };
    u32::try_from(before).unwrap_or(u32::MAX)
}

/// The exported child an anchor leads to: by its pointer's entity, else by its
/// position among the children with its tag and kind (see [`position_among`]).
fn find_partner(
    exported: &Structure,
    node: &KeptNode,
    resolve_exported: &dyn Fn(&str) -> Option<Uuid>,
) -> Option<usize> {
    if let KeptPayload::Pointer(id) = node.payload {
        return exported
            .substructures
            .iter()
            .enumerate()
            .filter(|(_, s)| {
                same_tag(s.tag.as_str(), &node.tag) && entity_of(s, resolve_exported) == Some(id)
            })
            .nth(node.position as usize)
            .map(|(index, _)| index);
    }
    let void = node.payload == KeptPayload::Void;
    exported
        .substructures
        .iter()
        .enumerate()
        .filter(|(_, s)| same_tag(s.tag.as_str(), &node.tag) && is_void(s) == void)
        .nth(node.position as usize)
        .map(|(index, _)| index)
}

fn diff_children(
    original: &Structure,
    exported: &Structure,
    resolve_original: &dyn Fn(&str) -> Option<Uuid>,
    resolve_exported: &dyn Fn(&str) -> Option<Uuid>,
    context: &Context<'_>,
    depth: usize,
) -> Vec<KeptNode> {
    let split = split_repository_citations(&original.substructures);
    let children: Vec<&Structure> = split
        .iter()
        .filter(|child| !is_inline_medium(child))
        .collect();
    let partners = pair(
        &children,
        &exported.substructures,
        resolve_original,
        resolve_exported,
    );
    let mut kept = Vec::new();
    for (child, partner) in children.into_iter().zip(partners) {
        match partner {
            Some(index) => {
                let below = diff_children(
                    child,
                    &exported.substructures[index],
                    resolve_original,
                    resolve_exported,
                    context,
                    depth + 1,
                );
                if !below.is_empty() {
                    kept.push(anchor(child, exported, index, resolve_exported, below));
                }
            }
            None => {
                if is_understood_beneath(original, child)
                    || is_understood_citation_text(original, exported, child, depth)
                    || is_understood(child, depth, context.record_tag)
                    || is_understood_association(child, resolve_original, context)
                {
                    continue;
                }
                kept.extend(keep_whole(child, resolve_original));
            }
        }
    }
    kept
}

/// The citations of a source as the export writes them: a `REPO` holding
/// several call numbers (`CALN`) is written as one `REPO` per call number, so
/// it is split the same way for the comparison. The first citation keeps what
/// the `REPO` holds beside its call numbers, the others only their own call
/// number, with what each holds beneath it (a medium and its phrase).
fn split_repository_citations(children: &[Structure]) -> Vec<Structure> {
    let mut split = Vec::new();
    for child in children {
        let calns: Vec<&Structure> = child
            .substructures
            .iter()
            .filter(|s| s.tag.as_str() == "CALN")
            .collect();
        if child.tag.as_str() != "REPO" || calns.len() < 2 {
            split.push(child.clone());
            continue;
        }
        for (number, caln) in calns.iter().enumerate() {
            let mut citation = child.clone();
            citation.substructures = child
                .substructures
                .iter()
                .filter(|s| {
                    if s.tag.as_str() == "CALN" {
                        std::ptr::eq(*s, *caln)
                    } else {
                        number == 0
                    }
                })
                .cloned()
                .collect();
            split.push(citation);
        }
    }
    split
}

/// An anchor for a paired child with kept substructures beneath it. It is
/// found again as the export writes its partner: by the entity the
/// partner's pointer names — a child paired in order may have pointed
/// elsewhere in the file — else by the partner's position among the
/// structures with its tag and kind.
fn anchor(
    child: &Structure,
    exported: &Structure,
    index: usize,
    resolve_exported: &dyn Fn(&str) -> Option<Uuid>,
    children: Vec<KeptNode>,
) -> KeptNode {
    let partner = &exported.substructures[index];
    let (payload, position) = match entity_of(partner, resolve_exported) {
        Some(id) => (
            KeptPayload::Pointer(id),
            position_among(&exported.substructures, index, resolve_exported),
        ),
        None if is_void(partner) => (
            KeptPayload::Void,
            position_among(&exported.substructures, index, resolve_exported),
        ),
        None => (
            child
                .text()
                .map_or(KeptPayload::None, |text| KeptPayload::Text(text.to_owned())),
            position_among(&exported.substructures, index, resolve_exported),
        ),
    };
    KeptNode {
        tag: child.tag.as_str().to_owned(),
        payload,
        whole: false,
        position,
        xref: false,
        children,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    use oxidged::GedcomVersion;
    use oxidged::tree::Tree;

    /// The record with `xref` in `text`.
    fn record(text: &str, xref: &str) -> Structure {
        Tree::parse(text)
            .to_structures()
            .into_iter()
            .find(|s| s.xref.as_ref().is_some_and(|x| x.as_str() == xref))
            .expect("record in the test text")
    }

    fn resolver(entries: &[(&str, u128)]) -> impl Fn(&str) -> Option<Uuid> {
        let map: HashMap<String, Uuid> = entries
            .iter()
            .map(|(xref, n)| ((*xref).to_owned(), Uuid::from_u128(*n)))
            .collect();
        move |xref: &str| map.get(xref).copied()
    }

    fn xref_resolver(entries: &[(&str, u128)]) -> impl Fn(Uuid) -> Option<String> {
        let map: HashMap<Uuid, String> = entries
            .iter()
            .map(|(xref, n)| (Uuid::from_u128(*n), (*xref).to_owned()))
            .collect();
        move |id: Uuid| map.get(&id).cloned()
    }

    fn text_of(structure: &Structure) -> String {
        structure.to_gedcom(0, GedcomVersion::V7_0)
    }

    #[test]
    fn nothing_kept_when_export_holds_the_same_structures() {
        let original = record(
            "0 HEAD\n0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 DATE 1 jan 1900\n0 TRLR\n",
            "@I1@",
        );
        let exported = record(
            "0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 DATE 1 JAN 1900\n",
            "@I1@",
        );
        let ro = resolver(&[("@I1@", 1)]);
        let re = resolver(&[("@I1@", 1)]);
        assert!(diff(&original, &exported, &ro, &re, "INDI").is_empty());
    }

    #[test]
    fn absent_refn_is_kept_whole_and_merged_back() {
        let original = record(
            "0 HEAD\n0 @I1@ INDI\n1 NAME A /B/\n1 REFN 42\n0 TRLR\n",
            "@I1@",
        );
        let exported = record("0 @I1@ INDI\n1 NAME A /B/\n", "@I1@");
        let ro = resolver(&[("@I1@", 1)]);
        let re = resolver(&[("@I1@", 1)]);
        let kept = diff(&original, &exported, &ro, &re, "INDI");
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].tag, "REFN");
        assert!(kept[0].whole);

        let mut merged = exported.clone();
        let left_out = merge(&mut merged, &kept, &xref_resolver(&[]), &re);
        assert_eq!(left_out, 0);
        assert!(text_of(&merged).contains("1 REFN 42\n"));
    }

    /// An ordinance kept whole from a GEDCOM 7.x record, whose bare event
    /// 7.x leaves out, stands for the bare one a 5.5.1 export writes.
    #[test]
    fn a_whole_kept_structure_fills_a_bare_one_of_its_tag() {
        let original = record(
            "0 HEAD\n0 @I1@ INDI\n1 BAPL\n2 _STAT RECORDED\n1 CONL\n0 TRLR\n",
            "@I1@",
        );
        let exported = record("0 @I1@ INDI\n1 CONL\n", "@I1@");
        let ro = resolver(&[("@I1@", 1)]);
        let re = resolver(&[("@I1@", 1)]);
        let kept = diff(&original, &exported, &ro, &re, "INDI");
        assert!(kept[0].whole);

        let mut merged = record("0 @I1@ INDI\n1 BAPL\n1 CONL\n", "@I1@");
        let left_out = merge(&mut merged, &kept, &xref_resolver(&[]), &re);
        assert_eq!(left_out, 0);
        assert_eq!(
            text_of(&merged).matches("BAPL").count(),
            1,
            "{}",
            text_of(&merged)
        );
        assert!(text_of(&merged).contains("1 BAPL\n2 _STAT RECORDED\n1 CONL\n"));
    }

    #[test]
    fn unpaired_fone_is_anchored_under_matching_place() {
        let original = record(
            "0 @I1@ INDI\n1 BIRT\n2 PLAC Town\n3 FONE Taun\n4 TYPE kana\n",
            "@I1@",
        );
        let exported = record("0 @I1@ INDI\n1 BIRT\n2 PLAC Town\n", "@I1@");
        let ro = resolver(&[("@I1@", 1)]);
        let re = resolver(&[("@I1@", 1)]);
        let kept = diff(&original, &exported, &ro, &re, "INDI");

        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].tag, "BIRT");
        assert!(!kept[0].whole);
        assert_eq!(kept[0].children[0].tag, "PLAC");
        assert!(!kept[0].children[0].whole);
        let fone = &kept[0].children[0].children[0];
        assert_eq!(fone.tag, "FONE");
        assert!(fone.whole);
        assert_eq!(fone.children[0].tag, "TYPE");

        let mut merged = exported.clone();
        assert_eq!(merge(&mut merged, &kept, &xref_resolver(&[]), &re), 0);
        assert!(text_of(&merged).contains("2 PLAC Town\n3 FONE Taun\n4 TYPE kana\n"));
    }

    #[test]
    fn pointer_anchor_follows_the_entity_it_names() {
        let original = record(
            "0 @I1@ INDI\n1 FAMC @F1@\n2 STAT CHALLENGED\n1 FAMC @F2@\n",
            "@I1@",
        );
        let exported = record("0 @I1@ INDI\n1 FAMC @F20@\n1 FAMC @F10@\n", "@I1@");
        let ro = resolver(&[("@F1@", 101), ("@F2@", 102)]);
        let re = resolver(&[("@F10@", 101), ("@F20@", 102)]);
        let kept = diff(&original, &exported, &ro, &re, "INDI");

        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].payload, KeptPayload::Pointer(Uuid::from_u128(101)));
        assert_eq!(kept[0].position, 0);

        let mut merged = exported.clone();
        assert_eq!(merge(&mut merged, &kept, &xref_resolver(&[]), &re), 0);
        assert!(text_of(&merged).contains("1 FAMC @F10@\n2 STAT CHALLENGED\n"));
    }

    #[test]
    fn understood_events_are_never_kept() {
        let original = record(
            "0 @I1@ INDI\n\
             1 MILI army\n\
             2 DATE 1914\n\
             1 REFN 42\n\
             2 NOTE x\n\
             2 ASSO @I2@\n",
            "@I1@",
        );
        let exported = record("0 @I1@ INDI\n", "@I1@");
        let ro = resolver(&[("@I1@", 1), ("@I2@", 2)]);
        let re = resolver(&[("@I1@", 1)]);
        let kept = diff(&original, &exported, &ro, &re, "INDI");

        // The MILI is understood. The REFN is kept whole with what it holds,
        // the association beneath it included: the export writes no
        // association to @I2@ anywhere, so it is not understood.
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].tag, "REFN");
        let tags: Vec<&str> = kept[0].children.iter().map(|c| c.tag.as_str()).collect();
        assert_eq!(tags, ["NOTE", "ASSO"]);
    }

    #[test]
    fn an_unpaired_association_the_export_writes_nowhere_is_kept_whole() {
        let original = record(
            "0 @I1@ INDI\n1 ASSO @I2@\n2 RELA Friend\n2 NOTE x\n",
            "@I1@",
        );
        let exported = record("0 @I1@ INDI\n", "@I1@");
        let ro = resolver(&[("@I1@", 1), ("@I2@", 2)]);
        let re = resolver(&[("@I1@", 1)]);
        let kept = diff(&original, &exported, &ro, &re, "INDI");

        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].tag, "ASSO");
        assert!(kept[0].whole);
        assert_eq!(kept[0].payload, KeptPayload::Pointer(Uuid::from_u128(2)));
        let tags: Vec<&str> = kept[0].children.iter().map(|c| c.tag.as_str()).collect();
        assert_eq!(tags, ["RELA", "NOTE"]);
    }

    #[test]
    fn an_unpaired_association_the_export_writes_elsewhere_is_understood() {
        // The export writes the witness at the record level, for the same
        // entity, under a different pointer: the association is understood.
        let original = record("0 @I1@ INDI\n1 BIRT\n2 ASSO @I2@\n3 ROLE GODP\n", "@I1@");
        let exported = record("0 @I1@ INDI\n1 BIRT\n1 _ASSO @I20@\n", "@I1@");
        let ro = resolver(&[("@I1@", 1), ("@I2@", 2)]);
        let re = resolver(&[("@I1@", 1), ("@I20@", 2)]);
        let kept = diff(&original, &exported, &ro, &re, "INDI");
        assert!(kept.is_empty(), "{kept:?}");
    }

    #[test]
    fn an_association_paired_in_order_keeps_what_it_holds_beneath_its_anchor() {
        let original = record("0 @I1@ INDI\n1 ASSO @I2@\n2 NOTE x\n", "@I1@");
        let exported = record("0 @I1@ INDI\n1 ASSO @I30@\n", "@I1@");
        let ro = resolver(&[("@I1@", 1), ("@I2@", 2)]);
        let re = resolver(&[("@I1@", 1), ("@I30@", 3)]);
        let kept = diff(&original, &exported, &ro, &re, "INDI");
        // Pass 2 pairs the association with the export's one in order, as any
        // structure: what it holds beneath is kept under an anchor.
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].tag, "ASSO");
        assert!(!kept[0].whole);
        assert_eq!(kept[0].children[0].tag, "NOTE");
    }

    #[test]
    fn unpaired_restriction_is_kept_and_paired_one_is_not() {
        let original = record("0 @I1@ INDI\n1 RESN locked\n", "@I1@");
        let exported = record("0 @I1@ INDI\n", "@I1@");
        let ro = resolver(&[("@I1@", 1)]);
        let re = resolver(&[("@I1@", 1)]);
        let kept = diff(&original, &exported, &ro, &re, "INDI");
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].tag, "RESN");
        assert!(kept[0].whole);

        let original = record("0 @I1@ INDI\n1 RESN confidential\n", "@I1@");
        let exported = record("0 @I1@ INDI\n1 RESN confidential\n", "@I1@");
        assert!(diff(&original, &exported, &ro, &re, "INDI").is_empty());
    }

    #[test]
    fn unpaired_lds_ordinance_is_kept_whole_and_paired_one_keeps_what_it_holds() {
        let original = record("0 @I1@ INDI\n1 BAPL\n2 DATE 1 JAN 1900\n", "@I1@");
        let exported = record("0 @I1@ INDI\n", "@I1@");
        let ro = resolver(&[("@I1@", 1)]);
        let re = resolver(&[("@I1@", 1)]);
        let kept = diff(&original, &exported, &ro, &re, "INDI");
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].tag, "BAPL");
        assert!(kept[0].whole);

        let original = record("0 @I1@ INDI\n1 BAPL\n2 TEMP Paris\n", "@I1@");
        let exported = record("0 @I1@ INDI\n1 BAPL\n2 DATE 1 JAN 1900\n", "@I1@");
        let kept = diff(&original, &exported, &ro, &re, "INDI");
        assert_eq!(kept.len(), 1);
        assert!(!kept[0].whole);
        assert_eq!(kept[0].tag, "BAPL");
        assert_eq!(kept[0].children[0].tag, "TEMP");
        assert!(kept[0].children[0].whole);
    }

    #[test]
    fn void_pointer_absent_from_the_export_is_kept_and_merged_back() {
        let original = record(
            "0 @F1@ FAM\n1 CHIL @VOID@\n2 PHRASE An unknown child\n",
            "@F1@",
        );
        let exported = record("0 @F1@ FAM\n", "@F1@");
        let ro = resolver(&[("@F1@", 1)]);
        let re = resolver(&[("@F1@", 1)]);
        let kept = diff(&original, &exported, &ro, &re, "FAM");
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].payload, KeptPayload::Void);
        assert!(kept[0].whole);

        let mut merged = exported.clone();
        assert_eq!(merge(&mut merged, &kept, &xref_resolver(&[]), &re), 0);
        assert!(text_of(&merged).contains("1 CHIL @VOID@\n2 PHRASE An unknown child\n"));
    }

    #[test]
    fn void_pointers_pair_only_in_the_second_pass() {
        let original = record("0 @F1@ FAM\n1 CHIL @VOID@\n2 PHRASE a\n", "@F1@");
        let exported = record("0 @F1@ FAM\n1 CHIL @VOID@\n", "@F1@");
        let ro = resolver(&[("@F1@", 1)]);
        let re = resolver(&[("@F1@", 1)]);
        let kept = diff(&original, &exported, &ro, &re, "FAM");
        // Paired in order with the exported void child: the PHRASE is kept
        // beneath the anchor, which finds its void partner again.
        assert_eq!(kept.len(), 1);
        assert!(!kept[0].whole);
        assert_eq!(kept[0].payload, KeptPayload::Void);
        assert_eq!(kept[0].children[0].tag, "PHRASE");

        let mut merged = exported.clone();
        assert_eq!(merge(&mut merged, &kept, &xref_resolver(&[]), &re), 0);
        assert!(text_of(&merged).contains("1 CHIL @VOID@\n2 PHRASE a\n"));
    }

    #[test]
    fn an_association_written_at_the_same_place_keeps_what_it_holds() {
        let original = record(
            "0 @I1@ INDI
1 ASSO @I2@
2 RELA Godfather
2 NOTE Stood in for his brother
",
            "@I1@",
        );
        let exported = record(
            "0 @I1@ INDI
1 ASSO @I20@
2 RELA Godfather
",
            "@I1@",
        );
        let ro = resolver(&[("@I1@", 1), ("@I2@", 2)]);
        let re = resolver(&[("@I1@", 1), ("@I20@", 2)]);
        let kept = diff(&original, &exported, &ro, &re, "INDI");
        assert_eq!(kept.len(), 1);
        assert!(!kept[0].whole);
        assert_eq!(kept[0].children[0].tag, "NOTE");
    }

    #[test]
    fn kept_pointer_without_xref_is_left_out() {
        let original = record("0 @I1@ INDI\n1 ALIA @I9@\n", "@I1@");
        let exported = record("0 @I1@ INDI\n", "@I1@");
        let ro = resolver(&[("@I1@", 1), ("@I9@", 9)]);
        let re = resolver(&[("@I1@", 1)]);
        let kept = diff(&original, &exported, &ro, &re, "INDI");
        assert_eq!(kept.len(), 1);

        let mut merged = exported.clone();
        assert_eq!(merge(&mut merged, &kept, &xref_resolver(&[]), &re), 1);
        assert!(merged.substructures.is_empty());
    }

    #[test]
    fn anchor_without_partner_is_left_out() {
        let original = record("0 @I1@ INDI\n1 BIRT\n2 PLAC Town\n3 FONE Taun\n", "@I1@");
        let with_birth = record("0 @I1@ INDI\n1 BIRT\n2 PLAC Town\n", "@I1@");
        let ro = resolver(&[("@I1@", 1)]);
        let re = resolver(&[("@I1@", 1)]);
        let kept = diff(&original, &with_birth, &ro, &re, "INDI");
        assert_eq!(kept.len(), 1);

        let without_birth = record("0 @I1@ INDI\n1 NAME A /B/\n", "@I1@");
        let mut merged = without_birth.clone();
        assert_eq!(merge(&mut merged, &kept, &xref_resolver(&[]), &re), 1);
        assert!(!text_of(&merged).contains("FONE"));
    }

    #[test]
    fn json_round_trip() {
        let nodes = vec![
            KeptNode {
                tag: "BIRT".to_owned(),
                payload: KeptPayload::None,
                whole: false,
                position: 2,
                xref: false,
                children: vec![KeptNode {
                    tag: "PLAC".to_owned(),
                    payload: KeptPayload::Text("Town".to_owned()),
                    whole: false,
                    position: 0,
                    xref: false,
                    children: vec![KeptNode {
                        tag: "FONE".to_owned(),
                        payload: KeptPayload::Text("Taun".to_owned()),
                        whole: true,
                        position: 0,
                        xref: false,
                        children: Vec::new(),
                    }],
                }],
            },
            KeptNode {
                tag: "FAMC".to_owned(),
                payload: KeptPayload::Pointer(Uuid::from_u128(7)),
                whole: false,
                position: 0,
                xref: false,
                children: Vec::new(),
            },
        ];
        let json = to_json(&nodes);
        assert_eq!(from_json(&json).expect("valid JSON"), nodes);
    }

    #[test]
    fn text_source_pairs_with_pointer_source_and_keeps_only_the_note() {
        let original = record(
            "0 @I1@ INDI\n1 SOUR Parish book\n2 PAGE 12\n2 NOTE x\n",
            "@I1@",
        );
        let exported = record("0 @I1@ INDI\n1 SOUR @S1@\n2 PAGE 12\n", "@I1@");
        let ro = resolver(&[("@I1@", 1)]);
        let re = resolver(&[("@I1@", 1), ("@S1@", 5)]);
        let kept = diff(&original, &exported, &ro, &re, "INDI");

        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].tag, "SOUR");
        assert!(!kept[0].whole);
        assert_eq!(kept[0].position, 0);
        assert_eq!(kept[0].children.len(), 1);
        assert_eq!(kept[0].children[0].tag, "NOTE");
        assert!(kept[0].children[0].whole);

        let mut merged = exported.clone();
        assert_eq!(merge(&mut merged, &kept, &xref_resolver(&[]), &re), 0);
        assert!(text_of(&merged).contains("1 SOUR @S1@\n2 PAGE 12\n2 NOTE x\n"));
    }

    #[test]
    fn a_citation_text_the_export_writes_as_data_text_is_understood() {
        let original = record(
            "0 @I1@ INDI\n1 SOUR Parish book\n2 TEXT Born at home\n",
            "@I1@",
        );
        let exported = record(
            "0 @I1@ INDI\n1 SOUR @S1@\n2 DATA\n3 TEXT Born at home\n",
            "@I1@",
        );
        let ro = resolver(&[("@I1@", 1)]);
        let re = resolver(&[("@I1@", 1), ("@S1@", 5)]);
        let kept = diff(&original, &exported, &ro, &re, "INDI");
        assert!(kept.is_empty(), "{kept:?}");
    }

    #[test]
    fn a_citation_text_without_a_data_text_partner_is_kept_whole() {
        let original = record(
            "0 @I1@ INDI\n1 SOUR Parish book\n2 TEXT Born at home\n",
            "@I1@",
        );
        let exported = record("0 @I1@ INDI\n1 SOUR @S1@\n2 PAGE 12\n", "@I1@");
        let ro = resolver(&[("@I1@", 1)]);
        let re = resolver(&[("@I1@", 1), ("@S1@", 5)]);
        let kept = diff(&original, &exported, &ro, &re, "INDI");
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].tag, "SOUR");
        assert_eq!(kept[0].children[0].tag, "TEXT");
        assert!(kept[0].children[0].whole);
    }
}
