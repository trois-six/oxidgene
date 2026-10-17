//! Kept GEDCOM: the structures of a file that OxidGene's model has no place
//! for, compared at import with the same records as OxidGene exports them, and
//! put back beneath the exported records at export (see `docs/api.md`, "Kept
//! GEDCOM").
//!
//! This module is pure: it compares and merges [`oxidged`] structures, and
//! takes the pointer resolution from its callers, so the import and the export
//! decide what the identifiers mean.

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
    /// For an anchor without a pointer: its position, from 0, among the
    /// exported substructures with its tag (pass-2 pairing below). 0 otherwise.
    pub position: u32,
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
    diff_children(
        original,
        exported,
        resolve_original,
        resolve_exported,
        record_tag,
        0,
    )
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
                Some(structure) => exported.substructures.push(structure),
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

/// Whether two tags are the same for pairing: `SNOTE` is written `NOTE` in
/// GEDCOM 5.5.1.
fn same_tag(a: &str, b: &str) -> bool {
    canonical(a) == canonical(b)
}

fn canonical(tag: &str) -> &str {
    if tag == "SNOTE" { "NOTE" } else { tag }
}

/// Associations are understood: the import turns them into event witnesses,
/// which the export writes where the version permits — an association it
/// writes at the same place pairs as any structure does, so that what it
/// holds beneath (a witness's note, a citation) is kept; one it writes
/// elsewhere is never kept whole, which would name the witness twice.
fn is_association(structure: &Structure) -> bool {
    matches!(structure.tag.as_str(), "ASSO" | "_ASSO")
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
    matches!(
        (parent.tag.as_str(), child.tag.as_str()),
        ("REPO", "CALN") | ("ASSO" | "_ASSO", "TYPE")
    )
}

/// Whether an unpaired child is one the import reads, so that the export
/// writes it or is right to leave it out: an association, an age, an event
/// the import reads (direct children of `INDI` and `FAM` only), the
/// restriction of a person or family, which is its privacy, and the death
/// reason the `geneweb` crate writes beneath a death (`_GWDEATH`), which the
/// import reads as the cause.
fn is_understood(child: &Structure, depth: usize, record_tag: &str) -> bool {
    // An age is stored, or left out with a warning, and an empty one says
    // nothing: none is kept.
    is_association(child)
        || child.tag.as_str() == "AGE"
        || child.tag.as_str() == "_GWDEATH"
        || (depth == 0 && is_understood_event(child, record_tag))
        || (depth == 0 && child.tag.as_str() == "RESN" && matches!(record_tag, "INDI" | "FAM"))
}

/// Whether an unpaired direct child of an `INDI` or `FAM` record is one the
/// import reads as a generic or standard event, so that the export writes it.
fn is_understood_event(child: &Structure, record_tag: &str) -> bool {
    if !matches!(record_tag, "INDI" | "FAM") {
        return false;
    }
    let tag = child.tag.as_str();
    // The LDS ordinances, under GEDCOM's tags or the extension tags 5.5.1
    // writes them under, are events the import reads whatever the export holds.
    if matches!(
        tag,
        "MILI" | "SEP" | "BAPL" | "CONL" | "INIL" | "ENDL" | "SLGC" | "SLGS" | "_INIL" | "_SLGC"
    ) {
        return true;
    }
    if TagId::standard(tag).is_some() {
        // Every event the record holds is imported, an `EVEN` too, and
        // exported under the tag its type takes, which may be another one.
        return tag == "EVEN" || EventKind::parse(tag).is_known();
    }
    !tag.is_empty() && !tag.starts_with('_') && !child.substructures.is_empty()
}

/// The payload as it is stored: a pointer must resolve, else `None`.
fn stored_payload(
    payload: &Payload,
    resolve: &dyn Fn(&str) -> Option<Uuid>,
) -> Option<KeptPayload> {
    match payload {
        Payload::None => Some(KeptPayload::None),
        Payload::Text(text) => Some(KeptPayload::Text(text.to_string())),
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
        let Some(target) = child.pointer().and_then(|p| resolve_original(p.as_str())) else {
            continue;
        };
        let found = (0..exported.len()).find(|&j| {
            !taken[j]
                && same_tag(exported[j].tag.as_str(), child.tag.as_str())
                && exported[j]
                    .pointer()
                    .and_then(|p| resolve_exported(p.as_str()))
                    == Some(target)
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

/// The position of `exported[index]` among the exported structures with its tag.
fn position_among(exported: &[Structure], index: usize) -> u32 {
    let tag = exported[index].tag.as_str();
    let before = exported[..index]
        .iter()
        .filter(|s| same_tag(s.tag.as_str(), tag))
        .count();
    u32::try_from(before).unwrap_or(u32::MAX)
}

/// The exported child an anchor leads to: by its pointer's entity, else by its
/// position among the children with its tag.
fn find_partner(
    exported: &Structure,
    node: &KeptNode,
    resolve_exported: &dyn Fn(&str) -> Option<Uuid>,
) -> Option<usize> {
    if let KeptPayload::Pointer(id) = node.payload {
        return exported.substructures.iter().position(|s| {
            same_tag(s.tag.as_str(), &node.tag)
                && s.pointer().and_then(|p| resolve_exported(p.as_str())) == Some(id)
        });
    }
    exported
        .substructures
        .iter()
        .enumerate()
        .filter(|(_, s)| same_tag(s.tag.as_str(), &node.tag))
        .nth(node.position as usize)
        .map(|(index, _)| index)
}

fn diff_children(
    original: &Structure,
    exported: &Structure,
    resolve_original: &dyn Fn(&str) -> Option<Uuid>,
    resolve_exported: &dyn Fn(&str) -> Option<Uuid>,
    record_tag: &str,
    depth: usize,
) -> Vec<KeptNode> {
    let children: Vec<&Structure> = original
        .substructures
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
                    record_tag,
                    depth + 1,
                );
                if !below.is_empty() {
                    kept.push(anchor(child, exported, index, resolve_exported, below));
                }
            }
            None => {
                if is_understood_beneath(original, child) || is_understood(child, depth, record_tag)
                {
                    continue;
                }
                kept.extend(keep_whole(child, resolve_original));
            }
        }
    }
    kept
}

/// An anchor for a paired child with kept substructures beneath it. It is
/// found again as the export writes its partner: by the entity the
/// partner's pointer names — a child paired in order may have pointed
/// elsewhere in the file — else by the partner's position among the
/// structures with its tag.
fn anchor(
    child: &Structure,
    exported: &Structure,
    index: usize,
    resolve_exported: &dyn Fn(&str) -> Option<Uuid>,
    children: Vec<KeptNode>,
) -> KeptNode {
    let partner = &exported.substructures[index];
    let (payload, position) = match partner.pointer().and_then(|p| resolve_exported(p.as_str())) {
        Some(id) => (KeptPayload::Pointer(id), 0),
        None => (
            child
                .text()
                .map_or(KeptPayload::None, |text| KeptPayload::Text(text.to_owned())),
            position_among(&exported.substructures, index),
        ),
    };
    KeptNode {
        tag: child.tag.as_str().to_owned(),
        payload,
        whole: false,
        position,
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
    fn associations_and_understood_events_are_never_kept() {
        let original = record(
            "0 @I1@ INDI\n\
             1 MILI army\n\
             2 DATE 1914\n\
             1 BIRT\n\
             2 DATE 1 JAN 1900\n\
             3 ASSO @I2@\n\
             2 ASSO @I2@\n\
             1 ASSO @I2@\n\
             1 _ASSO @I2@\n\
             1 REFN 42\n\
             2 NOTE x\n\
             2 ASSO @I2@\n",
            "@I1@",
        );
        let exported = record("0 @I1@ INDI\n1 BIRT\n2 DATE 1 JAN 1900\n", "@I1@");
        let ro = resolver(&[("@I1@", 1), ("@I2@", 2)]);
        let re = resolver(&[("@I1@", 1)]);
        let kept = diff(&original, &exported, &ro, &re, "INDI");

        // Only the REFN is kept, whole: an association the export does not
        // write at the same place is understood, unless what holds it is
        // kept whole, which keeps it with it.
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].tag, "REFN");
        let tags: Vec<&str> = kept[0].children.iter().map(|c| c.tag.as_str()).collect();
        assert_eq!(tags, ["NOTE", "ASSO"]);
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
                children: vec![KeptNode {
                    tag: "PLAC".to_owned(),
                    payload: KeptPayload::Text("Town".to_owned()),
                    whole: false,
                    position: 0,
                    children: vec![KeptNode {
                        tag: "FONE".to_owned(),
                        payload: KeptPayload::Text("Taun".to_owned()),
                        whole: true,
                        position: 0,
                        children: Vec::new(),
                    }],
                }],
            },
            KeptNode {
                tag: "FAMC".to_owned(),
                payload: KeptPayload::Pointer(Uuid::from_u128(7)),
                whole: false,
                position: 0,
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
}
