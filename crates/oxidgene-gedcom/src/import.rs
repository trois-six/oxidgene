//! GEDCOM → OxidGene domain model import.
//!
//! Reads a GEDCOM file and converts it into OxidGene domain model entities.
//! Tracks xref → UUID mappings so that cross-references between GEDCOM records
//! are correctly translated into foreign-key relationships.
//!
//! `oxidged` reads any file as it is — any encoding, any line terminators,
//! extensions and structures out of place — and keeps every line: what its
//! model has no field for stays, as an untyped structure, where it was. The
//! import reads the typed model, and the few untyped structures it knows.
//!
//! [`import_gedzip`] reads the same genealogy out of a `.gdz` archive and, in
//! addition, hands back the media bytes the archive carries — the one thing
//! that format has which a bare `.ged` does not.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek};

use chrono::Utc;
use oxidged::gedzip::GedzipReader;
use oxidged::model::{
    Address, Age, Association, Certainty, Citation as GedCitation, CitationSource, Crop, Dataset,
    Date, EnumList, Event as GedEvent, EventKind, EventSpouse, File, Items, Medium, Multimedia,
    MultimediaLink, Name as GedName, NamePieceKind, NameType as GedNameType, Node, Note as GedNote,
    Ordinance, OrdinanceKind, Pedigree, Place as GedPlace, Record, Removal, Restriction,
    Sex as GedSex, Store, TagId, Text, TextRef, View,
};
use oxidged::tree::{Payload, Structure};
use oxidged::{GedcomBuilder, GedcomVersion};
use uuid::Uuid;

use oxidgene_core::enums::SourceMediaType;
use oxidgene_core::search::fold_words;
use oxidgene_core::types::{
    Citation, DOCUMENT_MIME, Event, EventWitness, Family, FamilyChild, FamilySpouse, Media,
    MediaLink, Note, Person, PersonName, Place, Repository, Source, SourceRepository,
    TAG_COLOR_SLOTS, TAG_LABEL_MAX, TagRef, Vignette, is_remote_url, last_path_segment,
    normalize_mime, split_surname_particle, split_surname_with,
};
use oxidgene_core::{ChildType, Confidence, EventType, NameType, Privacy, Sex, SpouseRole};

use crate::{DocumentExtension, ImportResult, MediaMetadataExtension, TagExtension};

/// The largest `gedcom.ged` a GEDZIP may hold, uncompressed.
const GEDZIP_GEDCOM_LIMIT: u64 = 1024 * 1024 * 1024;
/// The largest media file a GEDZIP may hold, uncompressed: the upload limit
/// of a single file.
const GEDZIP_MEDIA_LIMIT: u64 = 128 * 1024 * 1024;

/// Import a GEDCOM text into OxidGene domain model entities.
///
/// All entities are assigned to the given `tree_id`. Reading never fails:
/// lines out of the grammar, unknown tags and misplaced structures are read
/// as far as they go, and what the import does not map is left out.
///
/// # Errors
///
/// Returns `Err` if the model cannot be converted.
pub fn import_gedcom(gedcom_str: &str, tree_id: Uuid) -> Result<ImportResult, String> {
    import_dataset(Dataset::parse(gedcom_str), tree_id)
}

/// Import a GEDCOM file as stored, whatever character set it was written in.
///
/// A `.ged` declares its character set in `HEAD.CHAR`, and software of the
/// GEDCOM 5 era writes ANSEL or ANSI (Windows-1252) as often as UTF-8. `oxidged`
/// decodes the bytes by what they hold first — a byte-order mark, a UTF-16
/// pattern, valid UTF-8 — and by `CHAR` after that, Windows-1252 standing in
/// for a declaration the bytes contradict (8-bit bytes under `CHAR ASCII`),
/// so decoding never fails and no accent is lost.
///
/// # Errors
///
/// Returns `Err` if the model cannot be converted.
pub fn import_gedcom_bytes(bytes: &[u8], tree_id: Uuid) -> Result<ImportResult, String> {
    import_dataset(Dataset::from_bytes(bytes), tree_id)
}

/// The full import of a dataset read from a `.ged` or a `.gdz`: the media
/// an OxidGene GEDZIP keeps as extensions restored, the standard model, the
/// OxidGene extensions beneath its `OBJE` records, and the portraits.
fn import_dataset(data: Dataset, tree_id: Uuid) -> Result<ImportResult, String> {
    import_dataset_keeping(data, tree_id).map(|(result, _)| result)
}

/// [`import_dataset`], with the pages of the media the file kept as
/// extensions, which name no file of an archive.
fn import_dataset_keeping(
    mut data: Dataset,
    tree_id: Uuid,
) -> Result<(ImportResult, std::collections::HashSet<Uuid>), String> {
    let kept = restore_kept_media(&mut data);
    let mut result = import_gedcom_data(&data, tree_id)?;
    import_oxidgene_media_extensions(&data, &mut result);
    crate::keeping::compute(&data, &mut result);
    let kept = kept
        .iter()
        .filter_map(|xref| result.media_by_xref.get(xref).copied())
        .collect();
    Ok((result, kept))
}

/// Restore the media an OxidGene GEDZIP keeps as extensions into the
/// standard model, each where it was written.
///
/// GEDZIP names no file outside the archive, so a medium OxidGene holds no
/// bytes for is written there as an `_OXIDGENE_OBJE` record — an `OBJE`
/// record by another tag, the producer's path as its `FILE` — and each link
/// to it as an `_OXIDGENE_OBJE` pointer, both with their position among the
/// `OBJE` records or the links beside them (`_OXIDGENE_POS`). Each becomes
/// the `OBJE` record or link it stands for, at that position, so the rest
/// of the import reads it as it reads the `.ged` of the same tree. The
/// xrefs of the records restored.
fn restore_kept_media(data: &mut Dataset) -> Vec<String> {
    let kept_tag = data.store_mut().intern_tag(crate::export::KEPT_MEDIA_TAG);
    let position_tag = data.store_mut().intern_tag(crate::export::POSITION_TAG);
    let records = data.extra();
    // The positions of the kept records among the untyped ones.
    let kept: Vec<usize> = records
        .into_iter()
        .enumerate()
        .filter(|(_, node)| node.tag_id() == kept_tag && node.xref().is_some())
        .map(|(at, _)| at)
        .collect();
    if kept.is_empty() {
        return Vec::new();
    }
    let st = data.store();
    let kept_nodes = || kept.iter().filter_map(|&at| records.get(at));
    let xrefs = kept_nodes()
        .filter_map(|node| Some(st.xref(node.xref()?).to_string()))
        .collect();
    let multimedia: Vec<_> = kept_nodes()
        .map(|node| {
            (
                position(node, position_tag),
                kept_multimedia(node, position_tag),
            )
        })
        .collect();
    let links = KeptLinks::of(data, kept_tag, position_tag);
    // Last first, so that the positions of the others hold. The links to
    // them become links to the `OBJE` records restored in their place.
    for &at in kept.iter().rev() {
        if let Some(handle) = data.handle_at::<Node>(at) {
            let _ = data.remove(handle, Removal::LeaveDangling);
        }
    }
    insert_multimedia(data, multimedia);
    links.restore(data, kept_tag);
    xrefs
}

/// The position an `_OXIDGENE_POS` beneath `node` gives, if any.
fn position(node: View<'_, Node>, position_tag: TagId) -> Option<usize> {
    node.children()
        .into_iter()
        .filter(|child| child.tag_id() == position_tag)
        .find_map(|child| node_text(child)?.trim().parse().ok())
}

/// The `OBJE` record an `_OXIDGENE_OBJE` record stands for: its `FILE`s
/// with their format, medium and title, its notes, and its other
/// substructures (OxidGene's extensions) as they are.
fn kept_multimedia(node: View<'_, Node>, position_tag: TagId) -> Multimedia {
    let text_of = |node: View<'_, Node>| node.text().map(TextRef::to_text).unwrap_or_default();
    let child = |node: View<'_, Node>, tag: &str| {
        node.children()
            .into_iter()
            .find(|child| child.tag() == tag)
            .map(text_of)
    };
    let mut multimedia = Multimedia {
        xref: node.xref(),
        ..Default::default()
    };
    let mut extra = Vec::new();
    for sub in node.children() {
        match sub.tag() {
            _ if sub.tag_id() == position_tag => {}
            "FILE" => {
                let form = sub
                    .children()
                    .into_iter()
                    .find(|child| child.tag() == "FORM")
                    .map(|form| oxidged::model::FileForm {
                        format: text_of(form),
                        medium_type: child(form, "MEDI")
                            .or_else(|| child(form, "TYPE"))
                            .map(|medium| Box::new(Medium::parse(&medium.to_str(node.store())))),
                        ..Default::default()
                    });
                multimedia.files.push(File {
                    path: text_of(sub),
                    form,
                    title: child(sub, "TITL"),
                    ..Default::default()
                });
            }
            "NOTE" => multimedia.notes.push(GedNote::text(text_of(sub))),
            _ => extra.push(sub.into_owned()),
        }
    }
    multimedia.extra = extra.into();
    multimedia
}

/// The `OBJE` link an `_OXIDGENE_OBJE` link stands for: its pointer, its
/// `CROP` (so a crop of a medium OxidGene holds no file for is still an
/// identification) and its other substructures, such as `_OXIDGENE_PERSON`,
/// as they are.
fn kept_link(node: View<'_, Node>, position_tag: TagId) -> Option<MultimediaLink> {
    let mut link = MultimediaLink {
        object: Some(node.pointer()?),
        ..Default::default()
    };
    let mut extra = Vec::new();
    for sub in node.children() {
        if sub.tag_id() == position_tag {
            continue;
        }
        if sub.tag() == "CROP" {
            link.crop = Some(Box::new(kept_crop(sub)));
        } else {
            extra.push(sub.into_owned());
        }
    }
    link.extra = extra.into();
    Some(link)
}

/// The rectangle a `CROP` beneath a kept link names, its absent or unreadable
/// values left absent.
fn kept_crop(node: View<'_, Node>) -> Crop {
    let value = |tag: &str| {
        node.children()
            .into_iter()
            .find(|child| child.tag() == tag)
            .and_then(|child| node_text(child)?.trim().parse().ok())
    };
    Crop {
        top: value("TOP"),
        left: value("LEFT"),
        height: value("HEIGHT"),
        width: value("WIDTH"),
        ..Default::default()
    }
}

/// Insert each of `kept` into `items` at its position, in ascending order of
/// positions, so that each lands where it was written; one without a
/// position goes last.
fn insert_at_positions<T>(items: &mut Vec<T>, mut kept: Vec<(Option<usize>, T)>) {
    kept.sort_by_key(|(position, _)| position.unwrap_or(usize::MAX));
    for (position, item) in kept {
        let at = position.map_or(items.len(), |p| p.min(items.len()));
        items.insert(at, item);
    }
}

/// Insert each of `kept` among the `OBJE` records of `data` at its position,
/// as [`insert_at_positions`] does: the records from the first position on
/// are copied out, put in their new order with the kept ones, and stored
/// back.
fn insert_multimedia(data: &mut Dataset, kept: Vec<(Option<usize>, Multimedia)>) {
    let len = data.multimedia().len();
    let from = kept
        .iter()
        .map(|(position, _)| position.map_or(len, |p| p.min(len)))
        .min()
        .unwrap_or(len);
    let mut moved: Vec<Multimedia> = data
        .multimedia()
        .into_iter()
        .skip(from)
        .map(View::into_owned)
        .collect();
    let kept = kept
        .into_iter()
        .map(|(position, record)| (position.map(|p| p - from.min(p)), record))
        .collect();
    insert_at_positions(&mut moved, kept);
    for (at, record) in (from..).zip(moved) {
        if at < len {
            if let Some(mut slot) = data.get_mut_at::<Multimedia>(at) {
                *slot = record;
            }
        } else {
            data.push(Record::Multimedia(Box::new(record)));
        }
    }
}

/// The `OBJE` links some `_OXIDGENE_OBJE` links stand for, each with its
/// position.
type PlacedLinks = Vec<(Option<usize>, MultimediaLink)>;

/// The links of each record holding some, by its index and the index of its
/// event (`None` for the record's own).
type LinksByHolder = HashMap<(usize, Option<usize>), PlacedLinks>;

/// The `_OXIDGENE_OBJE` links of each person, family and event, with their
/// positions, read before the records holding them are changed.
#[derive(Default)]
struct KeptLinks {
    persons: LinksByHolder,
    families: LinksByHolder,
}

impl KeptLinks {
    fn of(data: &Dataset, kept_tag: TagId, position_tag: TagId) -> Self {
        let links = |extra: Items<'_, Node>| -> PlacedLinks {
            extra
                .into_iter()
                .filter(|node| node.tag_id() == kept_tag)
                .filter_map(|node| {
                    Some((position(node, position_tag), kept_link(node, position_tag)?))
                })
                .collect()
        };
        let mut kept = Self::default();
        let collect = |map: &mut LinksByHolder, key, extra: Items<'_, Node>| {
            let found = links(extra);
            if !found.is_empty() {
                map.insert(key, found);
            }
        };
        for (i, person) in data.individuals().into_iter().enumerate() {
            collect(&mut kept.persons, (i, None), person.extra());
            for (j, event) in person.events().into_iter().enumerate() {
                collect(&mut kept.persons, (i, Some(j)), event.extra());
            }
        }
        for (i, family) in data.families().into_iter().enumerate() {
            collect(&mut kept.families, (i, None), family.extra());
            for (j, event) in family.events().into_iter().enumerate() {
                collect(&mut kept.families, (i, Some(j)), event.extra());
            }
        }
        kept
    }

    /// Replace the `_OXIDGENE_OBJE` links by the `OBJE` links they stand
    /// for, in the records `data` lends.
    fn restore(self, data: &mut Dataset, kept_tag: TagId) {
        let unkept = |extra: &mut oxidged::model::Extra| extra.retain(|node| node.tag != kept_tag);
        for ((i, event), links) in self.persons {
            let Some(mut person) = data.get_mut_at::<oxidged::model::Individual>(i) else {
                continue;
            };
            let person = &mut *person;
            match event.and_then(|j| person.events.get_mut(j)) {
                Some(event) => {
                    unkept(&mut event.extra);
                    insert_at_positions(&mut event.detail_mut().multimedia, links);
                }
                None => {
                    unkept(&mut person.extra);
                    insert_at_positions(&mut person.detail_mut().multimedia, links);
                }
            }
        }
        for ((i, event), links) in self.families {
            let Some(mut family) = data.get_mut_at::<oxidged::model::Family>(i) else {
                continue;
            };
            let family = &mut *family;
            match event.and_then(|j| family.events.get_mut(j)) {
                Some(event) => {
                    unkept(&mut event.extra);
                    insert_at_positions(&mut event.detail_mut().multimedia, links);
                }
                None => {
                    unkept(&mut family.extra);
                    insert_at_positions(&mut family.detail_mut().multimedia, links);
                }
            }
        }
    }
}

/// A GEDZIP archive, read: the genealogy it holds and the files it carries.
///
/// See [`import_gedzip`].
#[derive(Debug)]
pub struct GedzipImport {
    /// Everything the enclosed `gedcom.ged` yielded — exactly what
    /// [`import_gedcom`] would have produced from the same bytes on their own.
    pub result: ImportResult,
    /// The bytes each medium's `FILE` named inside the archive, keyed by the
    /// id of the matching record in [`ImportResult::media`].
    ///
    /// A medium is absent here when its `FILE` is a URL, or names an entry the
    /// archive does not hold. Neither is fatal — both are reported in
    /// [`ImportResult::warnings`] and leave the record as an unheld one, which
    /// is what a plain `.ged` would have produced anyway.
    pub files: Vec<(Uuid, Vec<u8>)>,
}

/// A parsed GEDZIP whose referenced media are still inside the archive.
///
/// Keeping entry names rather than bytes lets a server retain a seekable
/// reader on a temporary file and extract only the bounded batch it is about
/// to ingest.
#[derive(Debug)]
pub struct GedzipImportPlan {
    pub result: ImportResult,
    pub files: Vec<(Uuid, String)>,
}

/// Parse the genealogy and resolve each media record to its archive entry
/// without extracting those entries.
///
/// The returned reader refuses any media entry larger than the upload limit
/// of a single file, checked against the size the archive declares and
/// enforced while reading; `gedcom.ged` itself is read within a larger one.
/// A `FILE` names its entry the way its producer wrote it — backslashes, a
/// leading `./`, percent-encoding, another letter case — and `oxidged` matches
/// it to the archive's own spelling.
///
/// # Errors
///
/// Returns `Err` if `source` is not a readable ZIP, if it holds no
/// `gedcom.ged`, or if that entry is larger than the limit.
pub fn prepare_gedzip<R: Read + Seek>(
    source: R,
    tree_id: Uuid,
) -> Result<(GedzipReader<R>, GedzipImportPlan), String> {
    let mut reader = GedzipReader::new(source)
        .map_err(|error| format!("GEDZIP read error: {error}"))?
        .max_entry_size(GEDZIP_GEDCOM_LIMIT);
    let data = reader
        .read_dataset(&GedcomBuilder::new())
        .map_err(|error| format!("GEDZIP read error: {error}"))?;

    let (mut result, kept) = import_dataset_keeping(data, tree_id)?;
    let reader = reader.max_entry_size(GEDZIP_MEDIA_LIMIT);

    let mut wanted: Vec<(Uuid, String)> = Vec::new();
    for media in &result.media {
        // Only a page names a file. A document holds no bytes, so looking one
        // up for it asks the archive for an entry called nothing at all.
        // Nor does a medium the archive keeps as an extension: GEDZIP names
        // no file outside it, so it was written without one.
        if media.is_document() || is_remote_url(&media.file_path) || kept.contains(&media.id) {
            continue;
        }
        match reader.find_entry(&media.file_path) {
            Some(name) => wanted.push((media.id, name.to_string())),
            None => result.warnings.push(format!(
                "GEDZIP: the archive holds no file named '{}'; the media record was kept without its bytes",
                media.file_path
            )),
        }
    }

    let referenced: std::collections::HashSet<&str> =
        wanted.iter().map(|(_, name)| name.as_str()).collect();
    let unreferenced = reader
        .media_files()
        .into_iter()
        .filter(|name| !referenced.contains(name))
        .count();
    if unreferenced > 0 {
        result.warnings.push(format!(
            "GEDZIP: {unreferenced} file(s) in the archive are named by no OBJE record and were not imported"
        ));
    }

    Ok((
        reader,
        GedzipImportPlan {
            result,
            files: wanted,
        },
    ))
}

/// Import a GEDZIP archive (`.gdz`) — a ZIP wrapping a `gedcom.ged` together
/// with the media files it references, per GEDCOM 7.0.
///
/// The genealogy half is the ordinary GEDCOM import; what the format adds is
/// that the referenced files travel with it, so this also hands back the bytes
/// of every medium the archive actually carries. Writing them into a store is
/// the caller's business — this crate holds no store — so the bytes come back
/// paired with the media record that named them.
///
/// # Errors
///
/// Returns `Err` if `archive` is not a readable ZIP, if it holds no
/// `gedcom.ged`, or if that entry is larger than the limit. A media file that
/// is missing or unreadable is a warning, not an error.
pub fn import_gedzip(archive: &[u8], tree_id: Uuid) -> Result<GedzipImport, String> {
    let (mut reader, plan) = prepare_gedzip(std::io::Cursor::new(archive), tree_id)?;
    let GedzipImportPlan {
        mut result,
        files: wanted,
    } = plan;

    let mut files = Vec::with_capacity(wanted.len());
    for (media_id, name) in wanted {
        match reader.read_media_file(&name) {
            Ok(bytes) => files.push((media_id, bytes)),
            Err(e) => result.warnings.push(format!(
                "GEDZIP: '{name}' could not be read out of the archive: {e}"
            )),
        }
    }

    Ok(GedzipImport { result, files })
}

// ═══════════════════════════════════════════════════════════════════════
// Reading texts
// ═══════════════════════════════════════════════════════════════════════

/// A text when it has characters: an empty text stands for no payload.
fn opt(value: Option<TextRef<'_>>) -> Option<String> {
    value.filter(|t| !t.is_empty()).map(|t| t.to_string())
}

/// An owned text of `st` when it has characters, as [`opt`] reads a text
/// of a view: what an enumeration value no variant names, or a structure
/// converted to another version, holds.
fn opt_owned(st: &Store, value: &Text) -> Option<String> {
    (!value.is_empty()).then(|| value.to_str(st).into_owned())
}

/// A text when it says something, trimmed.
fn non_blank(value: Option<TextRef<'_>>) -> Option<String> {
    non_blank_str(value.map(TextRef::to_str).as_deref())
}

/// `text` when it says something, trimmed.
fn non_blank_str(text: Option<&str>) -> Option<String> {
    text.map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

/// The first of `values` that says something.
fn first_non_blank(values: Items<'_, Text>) -> Option<String> {
    values.into_iter().find_map(|v| non_blank(Some(v)))
}

/// The text a structure gave where its type wants a pointer (`REPO Archives
/// of Somewhere` for `REPO @R1@`): `oxidged` keeps it beside the empty pointer.
fn aside_text(extra: Items<'_, Node>) -> Option<Cow<'_, str>> {
    extra
        .into_iter()
        .filter(|node| node.tag_id() == TagId::ASIDE)
        .find_map(node_text)
}

/// The untyped structures of `extra` with this tag.
fn tagged<'d>(extra: Items<'d, Node>, tag: &'d str) -> impl Iterator<Item = View<'d, Node>> + 'd {
    extra.into_iter().filter(move |node| node.tag() == tag)
}

/// The text payload of an untyped structure.
fn node_text(node: View<'_, Node>) -> Option<Cow<'_, str>> {
    node.text().map(TextRef::to_str)
}

/// Import an already-read GEDCOM dataset into OxidGene domain model entities.
///
/// This is the conversion half of [`import_gedcom`], exposed separately so that
/// other readers producing a [`Dataset`] — notably the GeneWeb `.gw` reader in
/// [`crate::geneweb`] — reuse the exact same mapping.
///
/// All entities are assigned to the given `tree_id`.
///
/// # Errors
///
/// Returns `Err` if the model cannot be converted.
pub fn import_gedcom_data(data: &Dataset, tree_id: Uuid) -> Result<ImportResult, String> {
    let ctx = ImportContext::allocate(data, tree_id);
    let now = ctx.now;
    let mut result = ImportResult {
        tree_id,
        ..ImportResult::default()
    };
    import_submitter(data, &mut result);

    // Place name → UUID (dedup by exact name match)
    let mut place_map: HashMap<String, Uuid> = HashMap::new();
    // Free-text SOUR description → UUID of a synthesized Source (dedup by
    // exact text match) — see `get_or_create_text_source` below.
    let mut text_source_map: HashMap<String, Uuid> = HashMap::new();

    // ── Helper: get or create a Place by name ───────────────────────
    let mut get_or_create_place = |name: &str, result: &mut ImportResult| -> Uuid {
        if let Some(&id) = place_map.get(name) {
            return id;
        }
        let id = Uuid::now_v7();
        place_map.insert(name.to_string(), id);
        result.places.push(Place {
            id,
            tree_id,
            name: name.to_string(),
            latitude: None,
            longitude: None,
            created_at: now,
            updated_at: now,
        });
        id
    };

    // ── Helper: get or create a Source from a free-text SOUR description ──
    //
    // Some exporters write a free-text description
    // (an archive reference, a URL...) instead of a pointer to a structured
    // SOUR record — valid per the GEDCOM 5.5.1 SOURCE_CITATION grammar. We
    // synthesize a `Source` from that text so the citation survives import
    // instead of being dropped, deduplicating by exact text so citations
    // that repeat the same reference (e.g. several people in the same
    // parish register) share one `Source`.
    let mut get_or_create_text_source = |text: &str, result: &mut ImportResult| -> Uuid {
        if let Some(&id) = text_source_map.get(text) {
            return id;
        }
        let id = Uuid::now_v7();
        text_source_map.insert(text.to_string(), id);
        result.sources.push(Source {
            id,
            tree_id,
            title: text.to_string(),
            author: None,
            publisher: None,
            abbreviation: None,
            agency: None,
            created_at: now,
            updated_at: now,
            deleted_at: None,
        });
        id
    };

    let mut repositories = import_repositories(data, &ctx, &mut result);
    import_sources(data, &ctx, &mut repositories, &mut result);
    let page_map = import_multimedia(data, &ctx, &mut result);
    let mut lookups = Lookups {
        place: &mut get_or_create_place,
        text_source: &mut get_or_create_text_source,
    };
    for indi in data.individuals() {
        import_individual(indi, &ctx, &mut lookups, &mut result);
    }
    for fam in data.families() {
        import_family(fam, &ctx, &mut lookups, &mut result);
    }
    import_person_and_family_tags(data, &ctx, &mut result);
    apply_individual_family_links(data, &ctx, &mut result);
    import_associations(data, &ctx, &mut result);

    // Handed back so a caller holding links keyed by something outside the
    // domain model can resolve them to person ids — see the field's docs.
    result.person_by_xref = ctx.indi_map;
    result.family_by_xref = ctx.fam_map;
    result.source_by_xref = ctx.source_map;
    result.repository_by_xref = repositories.by_xref;
    result.note_by_xref = ctx.note_map;
    result.media_by_xref = page_map;

    Ok(result)
}

/// The places and the sources named by text, created on first use.
struct Lookups<'l> {
    place: &'l mut dyn FnMut(&str, &mut ImportResult) -> Uuid,
    text_source: &'l mut dyn FnMut(&str, &mut ImportResult) -> Uuid,
}

/// What every record of one import is converted against: the tree, the
/// import time, the UUID allocated to each top-level record's xref, and the
/// shared note records notes point at.
///
/// Records are found by the text of their xref, so that structures read
/// apart from the dataset (see [`undefined_event`]) resolve their pointers
/// against the same records.
struct ImportContext<'d> {
    tree_id: Uuid,
    now: chrono::DateTime<Utc>,
    indi_map: HashMap<String, Uuid>,
    fam_map: HashMap<String, Uuid>,
    source_map: HashMap<String, Uuid>,
    /// The shared note record each `NOTE` record (`0 @N1@ NOTE` in 5.5.1,
    /// `0 @N1@ SNOTE` in 7.0) becomes a group of; the notes pointing at it
    /// carry its UUID as `Note::shared_group`.
    note_map: HashMap<String, Uuid>,
    /// The document each `OBJE` record becomes; links point at it.
    media_map: HashMap<String, Uuid>,
    /// The text of each shared note record, by xref: a file can point at one
    /// from every person, so they are looked up rather than searched.
    shared_notes: HashMap<&'d str, Cow<'d, str>>,
}

impl<'d> ImportContext<'d> {
    /// Pass 1: allocate UUIDs for all top-level records.
    fn allocate(data: &'d Dataset, tree_id: Uuid) -> Self {
        let st = data.store();
        let ids = |xrefs: &mut dyn Iterator<Item = Option<oxidged::model::XrefId>>| {
            allocate_ids(st, xrefs)
        };
        Self {
            tree_id,
            now: Utc::now(),
            indi_map: ids(&mut data.individuals().into_iter().map(|v| v.xref())),
            fam_map: ids(&mut data.families().into_iter().map(|v| v.xref())),
            source_map: ids(&mut data.sources().into_iter().map(|v| v.xref())),
            note_map: ids(&mut data.notes().into_iter().map(|v| v.xref())),
            media_map: ids(&mut data.multimedia().into_iter().map(|v| v.xref())),
            shared_notes: data
                .notes()
                .into_iter()
                .filter_map(|note| Some((st.xref(note.xref()?), note.text().to_str())))
                .collect(),
        }
    }

    /// The text of a note: its own, or that of the shared note record it
    /// points at. A pointer to a record the file does not hold is a warning,
    /// and no text.
    fn note_text(
        &self,
        note: View<'_, GedNote>,
        warnings: &mut Vec<String>,
    ) -> Option<(String, Option<Uuid>)> {
        if let Some(text) = note.text() {
            return Some((text.to_string(), None));
        }
        let xref = note.store().xref(note.shared()?);
        let text = self.shared_notes.get(xref).map(|text| text.to_string());
        if text.is_none() {
            warnings.push(format!(
                "A NOTE points at {xref}, which the file does not hold; it was left out"
            ));
        }
        text.map(|text| (text, self.note_map.get(xref).copied()))
    }

    /// The texts of `notes` that say something, as written, pointers
    /// resolved.
    fn note_texts(&self, notes: Items<'_, GedNote>, warnings: &mut Vec<String>) -> Vec<String> {
        self.note_pieces(notes, warnings)
            .into_iter()
            .map(|(text, _)| text)
            .collect()
    }

    /// The texts of `notes` that say something, each with the shared note
    /// record it was read from, if any.
    fn note_pieces(
        &self,
        notes: Items<'_, GedNote>,
        warnings: &mut Vec<String>,
    ) -> Vec<(String, Option<Uuid>)> {
        notes
            .into_iter()
            .filter_map(|note| self.note_text(note, warnings))
            .filter(|(text, _)| !text.trim().is_empty())
            .collect()
    }

    /// One note row per note of `notes`, attached to `owner`.
    fn import_notes(&self, notes: Items<'_, GedNote>, owner: NoteOwner, result: &mut ImportResult) {
        for (text, shared_group) in self.note_pieces(notes, &mut result.warnings) {
            let mut note = owner.note(self.tree_id, self.now, text);
            note.shared_group = shared_group;
            result.notes.push(note);
        }
    }
}

/// The record an imported note is attached to.
#[derive(Clone, Copy)]
enum NoteOwner {
    Person(Uuid),
    Family(Uuid),
    Event(Uuid),
    Source(Uuid),
    Repository(Uuid),
}

impl NoteOwner {
    /// A note row holding `text`, attached to this owner.
    fn note(self, tree_id: Uuid, now: chrono::DateTime<Utc>, text: String) -> Note {
        let mut note = Note {
            id: Uuid::now_v7(),
            tree_id,
            text,
            person_id: None,
            event_id: None,
            family_id: None,
            source_id: None,
            // GEDCOM attaches a NOTE to a record, never to an OBJE's bytes.
            media_id: None,
            repository_id: None,
            shared_group: None,
            created_at: now,
            updated_at: now,
            deleted_at: None,
        };
        match self {
            Self::Person(id) => note.person_id = Some(id),
            Self::Family(id) => note.family_id = Some(id),
            Self::Event(id) => note.event_id = Some(id),
            Self::Source(id) => note.source_id = Some(id),
            Self::Repository(id) => note.repository_id = Some(id),
        }
        note
    }
}

/// The record an event belongs to: a person's, or a family's.
#[derive(Clone, Copy)]
struct EventOwner<'x> {
    /// The record's xref, to name it in a warning.
    xref: &'x str,
    person_id: Option<Uuid>,
    family_id: Option<Uuid>,
}

impl EventOwner<'_> {
    /// How a warning names the record.
    fn label(&self) -> String {
        let kind = if self.family_id.is_some() {
            "Family"
        } else {
            "Individual"
        };
        format!("{kind} {}", self.xref)
    }
}

/// A new UUID for each xref, in record order. A record without one gets none.
fn allocate_ids(
    st: &Store,
    xrefs: &mut dyn Iterator<Item = Option<oxidged::model::XrefId>>,
) -> HashMap<String, Uuid> {
    xrefs
        .flatten()
        .map(|xref| (st.xref(xref).to_string(), Uuid::now_v7()))
        .collect()
}

/// What `export` names the submitter when nobody is: not a name to keep.
const NOT_PROVIDED: &str = "Not Provided";

/// The submitter `HEAD.SUBM` points at. The other `SUBM` records are kept
/// (see `kept`), not imported.
fn import_submitter(data: &Dataset, result: &mut ImportResult) {
    let pointer = data.header().and_then(|h| h.submitter);
    let submitter = pointer.and_then(|pointer| {
        data.submitters()
            .into_iter()
            .find(|s| s.xref() == Some(pointer))
    });
    if let Some(submitter) = submitter {
        let imported = crate::ImportedSubmitter {
            name: non_blank(submitter.name()).filter(|n| n != NOT_PROVIDED),
            email: first_non_blank(submitter.emails()),
            address: submitter.address().and_then(address_text),
        };
        if imported != crate::ImportedSubmitter::default() {
            result.submitter = Some(imported);
        }
    }
}

/// The repositories an import knows: the `REPO` records by xref, and the
/// ones synthesised from the text of pointerless `SOUR.REPO` citations.
struct RepositoryIndex {
    by_xref: HashMap<String, Uuid>,
    by_text: HashMap<String, Uuid>,
}

/// Import the `REPO` records and their notes.
fn import_repositories(
    data: &Dataset,
    ctx: &ImportContext,
    result: &mut ImportResult,
) -> RepositoryIndex {
    let st = data.store();
    let mut index = RepositoryIndex {
        by_xref: HashMap::new(),
        by_text: HashMap::new(),
    };
    for repo in data.repositories() {
        let Some(xref) = repo.xref().map(|id| st.xref(id)) else {
            result
                .warnings
                .push("Skipping repository without xref".into());
            continue;
        };
        let id = Uuid::now_v7();
        index.by_xref.insert(xref.to_string(), id);
        let notes = ctx.note_texts(repo.notes(), &mut result.warnings);
        let name = non_blank(repo.name())
            .or_else(|| {
                let first = notes.first()?.trim();
                Some(first.lines().next().unwrap_or_default().to_string())
            })
            .unwrap_or_else(|| xref.trim_matches('@').to_string());
        result.repositories.push(Repository {
            id,
            tree_id: ctx.tree_id,
            name,
            address: repo.address().and_then(address_text),
            phone: first_non_blank(repo.phones()),
            email: first_non_blank(repo.emails()),
            website: first_non_blank(repo.websites()),
            created_at: ctx.now,
            updated_at: ctx.now,
            deleted_at: None,
        });
        for text in notes {
            let text = text.trim().to_string();
            result
                .notes
                .push(NoteOwner::Repository(id).note(ctx.tree_id, ctx.now, text));
        }
    }
    index
}

/// An address as one text over several lines: its free text, else its
/// structured parts.
fn address_text(address: View<'_, Address>) -> Option<String> {
    if let Some(text) = non_blank(Some(address.value())) {
        return Some(text);
    }
    let part = |part: Option<TextRef<'_>>| part.map(|t| t.to_string());
    let city_line = [part(address.postal_code()), part(address.city())]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ");
    let lines: Vec<String> = [
        part(address.line1()),
        part(address.line2()),
        part(address.line3()),
        Some(city_line),
        part(address.state()),
        part(address.country()),
    ]
    .into_iter()
    .filter_map(|line| non_blank_str(line.as_deref()))
    .collect();
    (!lines.is_empty()).then(|| lines.join("\n"))
}

/// The word a medium is written as: the GEDCOM value, or a producer's own.
fn medium_text(st: &Store, medium: &Medium) -> Option<String> {
    match medium {
        Medium::Unknown(t) => opt_owned(st, t),
        known => known.as_str(GedcomVersion::V7_0).map(str::to_string),
    }
}

/// The links of source `source_id` to the repositories its `SOUR.REPO`
/// citations name, one per call number (`CALN`), each with its own medium
/// (`MEDI`), and one without a call number for a citation that has none. A
/// pointer to a record the file does not hold is a warning; a citation
/// without a pointer names a repository by its text — the line's own, else
/// its notes' — created once per distinct text.
fn import_repository_citations(
    src: View<'_, oxidged::model::Source>,
    source_id: Uuid,
    ctx: &ImportContext,
    repositories: &mut RepositoryIndex,
    result: &mut ImportResult,
) {
    let st = src.store();
    let mut order = 0;
    for citation in src.repositories() {
        let repository_id = match citation.repository() {
            Some(pointer) => {
                let pointer = st.xref(pointer);
                let Some(&id) = repositories.by_xref.get(pointer) else {
                    result.warnings.push(format!(
                        "Source {}: REPO {pointer} not found",
                        src.xref().map(|x| st.xref(x)).unwrap_or_default()
                    ));
                    continue;
                };
                id
            }
            None => {
                let text = non_blank_str(aside_text(citation.extra()).as_deref()).or_else(|| {
                    let notes: Vec<String> = ctx
                        .note_texts(citation.notes(), &mut result.warnings)
                        .iter()
                        .map(|text| text.trim().to_string())
                        .collect();
                    (!notes.is_empty()).then(|| notes.join("\n"))
                });
                let Some(text) = text else { continue };
                text_repository(&text, ctx, repositories, result)
            }
        };
        let call_numbers: Vec<(Option<String>, Option<String>)> =
            if citation.call_numbers().is_empty() {
                vec![(None, None)]
            } else {
                citation
                    .call_numbers()
                    .into_iter()
                    .map(|call| {
                        (
                            non_blank(Some(call.value())),
                            call.medium()
                                .and_then(|medium| medium_text(st, &medium.value())),
                        )
                    })
                    .collect()
            };
        for (call_number, medium) in call_numbers {
            result.source_repositories.push(SourceRepository {
                id: Uuid::now_v7(),
                source_id,
                repository_id,
                call_number,
                media_type: non_blank_str(medium.as_deref())
                    .map(|m| SourceMediaType::parse(&m).unwrap_or(SourceMediaType::Other)),
                sort_order: order,
            });
            order += 1;
        }
    }
}

/// The repository a pointerless citation's `text` names, created on first
/// use.
fn text_repository(
    text: &str,
    ctx: &ImportContext,
    repositories: &mut RepositoryIndex,
    result: &mut ImportResult,
) -> Uuid {
    if let Some(&id) = repositories.by_text.get(text) {
        return id;
    }
    let id = Uuid::now_v7();
    repositories.by_text.insert(text.to_string(), id);
    result.repositories.push(Repository {
        id,
        tree_id: ctx.tree_id,
        name: text.to_string(),
        address: None,
        phone: None,
        email: None,
        website: None,
        created_at: ctx.now,
        updated_at: ctx.now,
        deleted_at: None,
    });
    id
}

/// Import the `SOUR` records, their notes and the repositories holding them.
fn import_sources(
    data: &Dataset,
    ctx: &ImportContext,
    repositories: &mut RepositoryIndex,
    result: &mut ImportResult,
) {
    let st = data.store();
    for src in data.sources() {
        let Some(xref) = src.xref().map(|id| st.xref(id)) else {
            result.warnings.push("Skipping source without xref".into());
            continue;
        };
        let id = ctx.source_map[xref];
        result.sources.push(Source {
            id,
            tree_id: ctx.tree_id,
            title: opt(src.title()).unwrap_or_else(|| "Untitled".into()),
            author: opt(src.author()),
            publisher: opt(src.publication()),
            abbreviation: opt(src.abbreviation()),
            agency: src.data().and_then(|data| non_blank(data.agency())),
            created_at: ctx.now,
            updated_at: ctx.now,
            deleted_at: None,
        });

        ctx.import_notes(src.notes(), NoteOwner::Source(id), result);
        import_repository_citations(src, id, ctx, repositories, result);
    }
}

/// Import the `OBJE` records, and return the page each one became, by xref.
///
/// One `OBJE` becomes two rows: the page that names the file, and the
/// document that describes it. GEDCOM has no container, so a foreign file
/// gives us one document per record — which is exactly right, an ordinary
/// photograph being a document of one page. An OxidGene file then regroups
/// its pages from `_OXIDGENE_DOC`.
fn import_multimedia(
    data: &Dataset,
    ctx: &ImportContext,
    result: &mut ImportResult,
) -> HashMap<String, Uuid> {
    let st = data.store();
    // The page each `OBJE` became, as distinct from the document holding it.
    // Links point at the document; crops and page metadata at the page.
    let mut page_map: HashMap<String, Uuid> = HashMap::new();
    for mm in data.multimedia() {
        let Some(xref) = mm.xref().map(|id| st.xref(id)) else {
            result
                .warnings
                .push("Skipping multimedia without xref".into());
            continue;
        };
        let document_id = ctx.media_map[xref];
        let id = Uuid::now_v7();
        page_map.insert(xref.to_string(), id);

        let (file_path, mime_type, source_media_type) = media_file(mm.files());
        let file_name = file_name_of(&file_path);
        // The title is the file's (`FILE.TITL`); GEDCOM 5.5 and earlier
        // OxidGene exports wrote it on the record itself.
        let title = media_title(mm.files()).or_else(|| {
            tagged(mm.extra(), "TITL").find_map(|n| non_blank_str(node_text(n).as_deref()))
        });
        let description = media_description(mm.notes(), ctx, result);
        let page = media_page(
            ctx,
            id,
            document_id,
            file_name,
            mime_type,
            file_path,
            source_media_type,
        );
        result.media.push(Media {
            id: document_id,
            file_name: title.clone().unwrap_or_else(|| page.file_name.clone()),
            mime_type: DOCUMENT_MIME.to_string(),
            file_path: String::new(),
            parent_media_id: None,
            title,
            description,
            ..page.clone()
        });
        result.media.push(page);
    }
    page_map
}

/// The page of a document: the row that names the file.
fn media_page(
    ctx: &ImportContext,
    id: Uuid,
    document_id: Uuid,
    file_name: String,
    mime_type: String,
    file_path: String,
    source_media_type: SourceMediaType,
) -> Media {
    Media {
        id,
        tree_id: ctx.tree_id,
        file_name,
        mime_type,
        file_path,
        // A GEDCOM names a file; it does not carry one. Everything below
        // stays empty until the bytes are uploaded and attached.
        storage_key: None,
        sha256: None,
        thumbnail_key: None,
        thumbnail_url: None,
        width: None,
        height: None,
        page_count: 1,
        parent_media_id: Some(document_id),
        page_index: 0,
        file_size: 0, // Unknown from GEDCOM
        title: None,
        description: None,
        date_value: None,
        date_sort: None,
        date_qualifier: Default::default(),
        date_value2: None,
        calendar: Default::default(),
        // GEDCOM 5.5.1 gives an OBJE no RESN; a scan arrives following the
        // tree.
        privacy: Privacy::default(),
        source_media_type,
        // GEDCOM has no field for this; it stays unset until a user
        // classifies the record or a Geneanet import supplies one.
        document_category: None,
        tags: Vec::new(),
        place_id: None,
        created_at: ctx.now,
        updated_at: ctx.now,
        deleted_at: None,
    }
}

/// The path, MIME type and physical medium the first `FILE` of an `OBJE`
/// names.
///
/// GEDCOM's `FORM` is the "multimedia format", and what producers put
/// there is an extension (`jpeg`) when they put anything at all — the
/// sample exports carry `FORM application/octet-stream` or no FORM and
/// a URL ending `.jpg`. Taking it verbatim as a MIME type is how a
/// photograph that renders perfectly well in an `<img>` ends up
/// labelled OCTET-STREAM in the gallery beside it.
///
/// What the thing physically is — `PHOTO`, `MANUSCRIPT`, `TOMBSTONE` — is the
/// separate question of 5.5.1's `FORM.TYPE` and 7.0's `FORM.MEDI`. A value
/// outside GEDCOM's enumeration is a producer writing its own vocabulary, and
/// is kept as `Other` rather than guessed at.
fn media_file(files: Items<'_, File>) -> (String, String, SourceMediaType) {
    let Some(file) = files.first() else {
        return (
            String::new(),
            "application/octet-stream".into(),
            SourceMediaType::default(),
        );
    };
    let st = file.store();
    let path = file.path().to_string();
    let form = file.form();
    let declared = form.and_then(|f| opt(Some(f.format())));
    let mime = normalize_mime(declared.as_deref(), &path);
    let medium = form
        .and_then(|f| f.medium().map(|m| m.value()).or_else(|| f.medium_type()))
        .and_then(|m| medium_text(st, &m))
        .and_then(|m| SourceMediaType::parse(&m))
        .unwrap_or_default();
    (path, mime, medium)
}

/// The title of the first `FILE` of an `OBJE` that has one.
fn media_title(files: Items<'_, File>) -> Option<String> {
    files.into_iter().find_map(|file| opt(file.title()))
}

/// The last segment of a GEDCOM `FILE` path, a Windows one included.
fn file_name_of(file_path: &str) -> String {
    last_path_segment(file_path).to_string()
}

/// Whether an event of this kind is an attribute of an individual: GEDCOM's
/// `INDIVIDUAL_ATTRIBUTE_STRUCTURE` (`OCCU`, `RESI`, `TITL`, …), whose line
/// value is what the attribute says, rather than an event.
fn is_individual_attribute(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::Caste
            | EventKind::PhysicalDescription
            | EventKind::Education
            | EventKind::IdNumber
            | EventKind::Nationality
            | EventKind::ChildrenCount
            | EventKind::MarriageCount
            | EventKind::Occupation
            | EventKind::Property
            | EventKind::Religion
            | EventKind::Residence
            | EventKind::SocialSecurityNumber
            | EventKind::Title
            | EventKind::Fact
    )
}

/// Import an `INDI` record: the person, their names, events, attributes,
/// citations, note and media links.
fn import_individual(
    indi: View<'_, oxidged::model::Individual>,
    ctx: &ImportContext,
    lookups: &mut Lookups<'_>,
    result: &mut ImportResult,
) {
    let st = indi.store();
    let Some(xref) = indi.xref().map(|id| st.xref(id)) else {
        result
            .warnings
            .push("Skipping individual without xref".into());
        return;
    };
    let person_id = ctx.indi_map[xref];

    result.persons.push(Person {
        tags: Vec::new(),
        id: person_id,
        tree_id: ctx.tree_id,
        sex: indi.sex().as_ref().map_or(Sex::Unknown, convert_gender),
        privacy: privacy_from_restriction(st, indi.restriction().as_ref()),
        // A GEDCOM names a person's media but never says which one
        // represents them: `OBJE` carries no primary flag.
        portrait_media_id: None,
        portrait_vignette_id: None,
        created_at: ctx.now,
        updated_at: ctx.now,
        deleted_at: None,
    });

    // Names (GEDCOM allows {0:M} NAME structures per individual; the
    // first is primary, the rest import as additional PersonNames).
    let mut primary: Option<PersonName> = None;
    for (i, name) in indi.names().into_iter().enumerate() {
        let (mut person_name, aliases) = convert_name(name, person_id, i == 0, ctx.now);
        match &primary {
            None => primary = Some(person_name.clone()),
            Some(primary) => reduce_to_byname(&mut person_name, primary),
        }
        result.person_names.push(person_name);
        result.person_names.extend(aliases);
    }

    // Events and attributes (GEDCOM's INDIVIDUAL_ATTRIBUTE_STRUCTURE: OCCU,
    // RESI, TITL, ... — distinct from INDIVIDUAL_EVENT_STRUCTURE in both the
    // 5.5.1 and 7.0 specs, but modeled as `Event`s in our domain), then the
    // LDS ordinances and the event tags no GEDCOM version defines, read as
    // events too.
    let owner = EventOwner {
        xref,
        person_id: Some(person_id),
        family_id: None,
    };
    for event in indi.events() {
        if is_individual_attribute(&event.kind()) {
            import_attribute_detail(EventView::Event(event), owner, ctx, lookups, result);
        } else {
            import_event_detail(EventView::Event(event), owner, ctx, lookups, result);
        }
    }
    for ordinance in indi.ordinances() {
        import_event_detail(EventView::Ordinance(ordinance), owner, ctx, lookups, result);
    }
    for read in undefined_events(indi.extra(), "INDI", owner, &mut result.warnings) {
        for event in events_read(&read) {
            import_event_detail(EventView::Event(event), owner, ctx, lookups, result);
        }
    }

    // Source citations on the individual
    for cite in indi.citations() {
        import_citation(
            cite,
            CitationOwner {
                person_id: Some(person_id),
                event_id: None,
                family_id: None,
            },
            &ctx.source_map,
            lookups.text_source,
            result,
        );
    }

    ctx.import_notes(indi.notes(), NoteOwner::Person(person_id), result);
    if let Some(picture) =
        import_media_links(indi.multimedia(), LinkOwner::Person(person_id), ctx, result)
    {
        set_portrait(result, person_id, picture);
    }
}

/// Import a `FAM` record: the family, its spouses and children, events,
/// citations, notes and media links.
fn import_family(
    fam: View<'_, oxidged::model::Family>,
    ctx: &ImportContext,
    lookups: &mut Lookups<'_>,
    result: &mut ImportResult,
) {
    let st = fam.store();
    let Some(xref) = fam.xref().map(|id| st.xref(id)) else {
        result.warnings.push("Skipping family without xref".into());
        return;
    };
    let family_id = ctx.fam_map[xref];

    result.families.push(Family {
        tags: Vec::new(),
        id: family_id,
        tree_id: ctx.tree_id,
        privacy: privacy_from_restriction(st, fam.restriction().as_ref()),
        created_at: ctx.now,
        updated_at: ctx.now,
        deleted_at: None,
    });

    import_spouses(fam, xref, family_id, ctx, result);

    // Children
    for (idx, child) in fam.children().into_iter().enumerate() {
        let child_xref = child.individual().map_or("", |id| st.xref(id));
        let Some(&person_id) = ctx.indi_map.get(child_xref) else {
            result
                .warnings
                .push(format!("Family {xref}: CHIL {child_xref} not found"));
            continue;
        };
        result.family_children.push(FamilyChild {
            id: Uuid::now_v7(),
            family_id,
            person_id,
            child_type: ChildType::Biological, // default; PEDI tag handled below
            sort_order: idx as i32,
        });
    }

    // Family events, then the couple's LDS sealing and the event tags no
    // GEDCOM version defines, read as events too.
    let owner = EventOwner {
        xref,
        person_id: None,
        family_id: Some(family_id),
    };
    let spouses = FamilySpouses {
        st,
        husband: fam.husband_id(),
        wife: fam.wife_id(),
    };
    for event in fam.events() {
        import_family_event(
            EventView::Event(event),
            &spouses,
            owner,
            ctx,
            lookups,
            result,
        );
    }
    for ordinance in fam.ordinances() {
        let event = EventView::Ordinance(ordinance);
        import_family_event(event, &spouses, owner, ctx, lookups, result);
    }
    for read in undefined_events(fam.extra(), "FAM", owner, &mut result.warnings) {
        for event in events_read(&read) {
            import_family_event(
                EventView::Event(event),
                &spouses,
                owner,
                ctx,
                lookups,
                result,
            );
        }
    }

    // Source citations on the family
    for cite in fam.citations() {
        import_citation(
            cite,
            CitationOwner {
                person_id: None,
                event_id: None,
                family_id: Some(family_id),
            },
            &ctx.source_map,
            lookups.text_source,
            result,
        );
    }

    ctx.import_notes(fam.notes(), NoteOwner::Family(family_id), result);
    import_media_links(fam.multimedia(), LinkOwner::Family(family_id), ctx, result);
}

/// A family's `HUSB` and `WIFE`, to read the ages its events give them.
struct FamilySpouses<'s> {
    st: &'s Store,
    husband: Option<oxidged::model::XrefId>,
    wife: Option<oxidged::model::XrefId>,
}

/// A family event, with the ages it gives the family's spouses.
fn import_family_event(
    event: EventView<'_>,
    spouses: &FamilySpouses<'_>,
    owner: EventOwner<'_>,
    ctx: &ImportContext,
    lookups: &mut Lookups<'_>,
    result: &mut ImportResult,
) {
    // An attribute of a family (its `NCHI`, `FACT`, `RESI`) reads as an
    // individual's does: its value is its line value, its `TYPE` the
    // classification beside it, not one text.
    if is_individual_attribute(&event.kind()) {
        import_attribute_detail(event, owner, ctx, lookups, result);
    } else {
        import_event_detail(event, owner, ctx, lookups, result);
    }
    let spouse_ages = spouse_ages(event, spouses, owner, ctx, &mut result.warnings);
    if let Some(event) = result.events.last_mut() {
        event.spouse_ages = spouse_ages;
    }
}

/// An event as the import reads it: an event or attribute structure, or an
/// LDS ordinance (`BAPL`, `CONL`, `ENDL`, `INIL`, `SLGC`, `SLGS`).
///
/// An ordinance imports as its LDS type, typed by its tag the way a generic
/// `EVEN` with that `TYPE` is, with its date, place, notes and citations as
/// the event's. Its temple and status are not read into the event's
/// description; they are left for the kept GEDCOM to hold beside it. It has
/// nothing else an event has: no cause, age, agency, spouses, associations,
/// media or extensions.
#[derive(Clone, Copy)]
enum EventView<'d> {
    Event(View<'d, GedEvent>),
    Ordinance(View<'d, Ordinance>),
}

impl<'d> EventView<'d> {
    /// The store its texts and pointers read from.
    fn store(self) -> &'d Store {
        match self {
            Self::Event(e) => e.store(),
            Self::Ordinance(o) => o.store(),
        }
    }

    fn kind(self) -> EventKind {
        match self {
            Self::Event(e) => e.kind(),
            Self::Ordinance(_) => EventKind::Event,
        }
    }

    /// The `TYPE`, when it has characters: an ordinance's tag.
    fn classification(self) -> Option<String> {
        match self {
            Self::Event(e) => opt(e.classification()),
            Self::Ordinance(o) => Some(o.kind().tag().to_string()),
        }
    }

    /// The line value: an ordinance has none, its type being its tag.
    fn value(self) -> Cow<'d, str> {
        match self {
            Self::Event(e) => e.value().to_str(),
            Self::Ordinance(_) => Cow::Borrowed(""),
        }
    }

    fn date(self) -> Option<View<'d, Date>> {
        match self {
            Self::Event(e) => e.date(),
            Self::Ordinance(o) => o.date(),
        }
    }

    fn place(self) -> Option<View<'d, GedPlace>> {
        match self {
            Self::Event(e) => e.place(),
            Self::Ordinance(o) => o.place(),
        }
    }

    fn citations(self) -> Items<'d, GedCitation> {
        match self {
            Self::Event(e) => e.citations(),
            Self::Ordinance(o) => o.citations(),
        }
    }

    fn notes(self) -> Items<'d, GedNote> {
        match self {
            Self::Event(e) => e.notes(),
            Self::Ordinance(o) => o.notes(),
        }
    }

    fn cause(self) -> Option<TextRef<'d>> {
        self.event().and_then(|v| v.cause())
    }

    fn agency(self) -> Option<TextRef<'d>> {
        self.event().and_then(|v| v.agency())
    }

    fn age(self) -> Option<View<'d, Age>> {
        self.event().and_then(|v| v.age())
    }

    fn husband(self) -> Option<View<'d, EventSpouse>> {
        self.event().and_then(|v| v.husband())
    }

    fn wife(self) -> Option<View<'d, EventSpouse>> {
        self.event().and_then(|v| v.wife())
    }

    fn associations(self) -> Option<Items<'d, Association>> {
        self.event().map(|v| v.associations())
    }

    fn multimedia(self) -> Option<Items<'d, MultimediaLink>> {
        self.event().map(|v| v.multimedia())
    }

    fn extra(self) -> Option<Items<'d, Node>> {
        self.event().map(|v| v.extra())
    }

    /// The event structure, unless an ordinance.
    fn event(self) -> Option<View<'d, GedEvent>> {
        match self {
            Self::Event(e) => Some(e),
            Self::Ordinance(_) => None,
        }
    }
}

/// The events a person's or a family's record holds outside the structures
/// `oxidged` types there, each as a dataset of its own record holding it,
/// whose texts and pointers resolve against its own store (see
/// [`read_as_event`]):
///
/// - an event of the other kind of record — a `MARR` under an `INDI`, a
///   `BIRT` under a `FAM` — which some software writes, read as it is;
/// - a tag that no GEDCOM version defines, such as `MILI`, as a generic
///   event: typed `Military service` for a `MILI`, a `Separation` for a `SEP`
///   (which GeneWeb writes) and by the tag itself otherwise, a `TYPE` beneath
///   it joining its line value in the descriptor (`1 EVEN Army: Sergeant`),
///   and everything else beneath it — date, place, notes, citations, media —
///   the event's.
///
/// `oxidged` keeps such a structure whole, untyped, where it was. A custom `_`
/// tag, any other tag of GEDCOM's own (one its model keeps aside, such as a
/// second `SEX`) and an undefined tag other than `MILI` and `SEP` with
/// nothing beneath it are left alone.
fn undefined_events(
    extra: Items<'_, Node>,
    record: &str,
    owner: EventOwner<'_>,
    warnings: &mut Vec<String>,
) -> Vec<Dataset> {
    let mut read = Vec::new();
    for node in extra {
        let tag = node.tag();
        let event = if TagId::standard(tag).is_some() {
            if !EventKind::parse(tag).is_known() || tag == "EVEN" {
                continue;
            }
            // Where GEDCOM puts an event of this kind.
            let home = if record == "INDI" { "FAM" } else { "INDI" };
            (home, node.to_structure())
        } else if tag.is_empty()
            || tag.starts_with('_')
            || (node.children().is_empty() && !matches!(tag, "MILI" | "SEP"))
        {
            continue;
        } else {
            (record, generic_event(node, tag))
        };
        match read_as_event(event) {
            Some(dataset) => read.push(dataset),
            None => warnings.push(format!(
                "{}: a {tag} that could not be read as an event was left out",
                owner.label()
            )),
        }
    }
    read
}

/// The generic `EVEN` an undefined tag stands for (see [`undefined_events`]).
fn generic_event(node: View<'_, Node>, tag: &str) -> Structure {
    let structure = node.to_structure();
    let (types, mut children): (Vec<Structure>, Vec<Structure>) = structure
        .substructures
        .into_iter()
        .partition(|child| child.tag == "TYPE");
    let descriptor: Vec<&str> = types
        .iter()
        .filter_map(Structure::text)
        // A `Y` asserts that the event happened; it describes nothing.
        .chain(
            structure
                .payload
                .as_str()
                .filter(|value| value.trim() != "Y"),
        )
        .map(str::trim)
        .filter(|word| !word.is_empty())
        .collect();
    let type_text = match tag {
        "MILI" => crate::export::even_type_label(EventType::MilitaryService).unwrap_or(tag),
        "SEP" => crate::export::even_type_label(EventType::Separation).unwrap_or(tag),
        other => other,
    };
    children.insert(
        0,
        Structure {
            payload: Payload::Text(type_text.into()),
            ..Structure::new("TYPE")
        },
    );
    Structure {
        payload: if descriptor.is_empty() {
            Payload::None
        } else {
            Payload::Text(descriptor.join(": ").into())
        },
        substructures: children,
        ..Structure::new("EVEN")
    }
}

/// An event structure, read by `oxidged`'s reader under a record of `record`
/// (`INDI` or `FAM`), so that it is read like any other event: the dataset
/// of that one record, or `None` when the reader typed no event of it.
fn read_as_event((record, event): (&str, Structure)) -> Option<Dataset> {
    let gedcom = format!(
        "0 HEAD\n0 @R@ {record}\n{}0 TRLR\n",
        event.to_gedcom(1, GedcomVersion::V5_5_1)
    );
    let dataset = Dataset::parse(gedcom);
    let has_events = events_read(&dataset).next().is_some();
    has_events.then_some(dataset)
}

/// The events of a dataset [`undefined_events`] read.
fn events_read(read: &Dataset) -> impl Iterator<Item = View<'_, GedEvent>> {
    read.individuals()
        .into_iter()
        .flat_map(|v| v.events())
        .chain(read.families().into_iter().flat_map(|v| v.events()))
}

/// The ages a family event gives for the family's `HUSB` and `WIFE`
/// (`HUSB.AGE`, `WIFE.AGE`), for the spouses the file holds.
fn spouse_ages(
    event: EventView<'_>,
    spouses: &FamilySpouses<'_>,
    owner: EventOwner<'_>,
    ctx: &ImportContext,
    warnings: &mut Vec<String>,
) -> Vec<oxidgene_core::types::SpouseAge> {
    [
        (event.husband(), spouses.husband),
        (event.wife(), spouses.wife),
    ]
    .into_iter()
    .filter_map(|(member, spouse)| {
        let xref = spouses.st.xref(spouse?);
        Some(oxidgene_core::types::SpouseAge {
            person_id: *ctx.indi_map.get(xref)?,
            age: import_age(member?.age(), owner, warnings)?,
        })
    })
    .collect()
}

/// A family's `HUSB` and `WIFE`, in that order. A spouse the file does not
/// hold is a warning.
fn import_spouses(
    fam: View<'_, oxidged::model::Family>,
    xref: &str,
    family_id: Uuid,
    ctx: &ImportContext,
    result: &mut ImportResult,
) {
    let st = fam.store();
    let mut sort_order = 0i32;
    for (spouse, role, tag) in [
        (fam.husband(), SpouseRole::Husband, "HUSB"),
        (fam.wife(), SpouseRole::Wife, "WIFE"),
    ] {
        let Some(spouse) = spouse else {
            continue;
        };
        let spouse_xref = spouse.individual().map_or("", |id| st.xref(id));
        let Some(&person_id) = ctx.indi_map.get(spouse_xref) else {
            result
                .warnings
                .push(format!("Family {xref}: {tag} {spouse_xref} not found"));
            continue;
        };
        result.family_spouses.push(FamilySpouse {
            id: Uuid::now_v7(),
            family_id,
            person_id,
            role,
            sort_order,
        });
        sort_order += 1;
    }
}

/// The record a media link attaches to.
#[derive(Clone, Copy)]
enum LinkOwner {
    Person(Uuid),
    Event(Uuid),
    Family(Uuid),
}

/// The picture a person's first link gives them: a whole page or a crop.
#[derive(Clone, Copy)]
enum Picture {
    Whole(Uuid),
    Crop(Uuid),
}

/// Link each `OBJE` of a record to it, creating the media an inline `OBJE`
/// describes.
///
/// A link with a `CROP` beneath a person or an event identifies the part of
/// its page it cuts out, so it becomes a [`Vignette`] rather than a link; one
/// that cannot be kept that way stays a whole-picture link, with a warning,
/// and its `CROP` is kept as written by the kept GEDCOM. Beneath a family
/// there is no identification to make, so a crop there is kept the same way.
///
/// A person's links record the file's order rather than leaving it to the
/// ids: a person's first picture is the one that represents them when no
/// portrait is stored, and GEDCOM states a portrait only by writing it first.
/// That first picture, whole or cropped, is returned.
fn import_media_links(
    multimedia: Items<'_, MultimediaLink>,
    owner: LinkOwner,
    ctx: &ImportContext,
    result: &mut ImportResult,
) -> Option<Picture> {
    let mut picture = None;
    for (index, mm) in multimedia.into_iter().enumerate() {
        let Some(media_id) = resolve_or_create_media(mm, ctx, result) else {
            continue;
        };
        let crop = match mm.crop() {
            Some(_) if matches!(owner, LinkOwner::Family(_)) => {
                result.warnings.push(format!(
                    "Media {media_id}: a crop beneath a family is not an identification — kept as the whole picture"
                ));
                None
            }
            Some(crop) => match crop_identification(crop, media_id, result) {
                Ok(identification) => Some(identification),
                Err(reason) => {
                    result.warnings.push(format!(
                        "Media {media_id}: a crop {reason} — kept as the whole picture"
                    ));
                    None
                }
            },
            None => None,
        };
        if let Some((
            page_id,
            Rectangle {
                x,
                y,
                width,
                height,
            },
        )) = crop
        {
            let (person_id, event_id) = match owner {
                LinkOwner::Person(person_id) => (Some(person_id), None),
                LinkOwner::Event(event_id) => (event_person(mm, ctx, result), Some(event_id)),
                LinkOwner::Family(_) => (None, None),
            };
            let now = ctx.now;
            let id = add_vignette(
                Vignette {
                    id: Uuid::now_v7(),
                    media_id: page_id,
                    x,
                    y,
                    width,
                    height,
                    person_id,
                    event_id,
                    created_at: now,
                    updated_at: now,
                },
                result,
            );
            if matches!(owner, LinkOwner::Person(_)) {
                picture.get_or_insert(Picture::Crop(id));
            }
            continue;
        }
        let (person_id, event_id, family_id, sort_order) = match owner {
            LinkOwner::Person(id) => (
                Some(id),
                None,
                None,
                i32::try_from(index).unwrap_or(i32::MAX),
            ),
            LinkOwner::Event(id) => (None, Some(id), None, 0),
            LinkOwner::Family(id) => (None, None, Some(id), 0),
        };
        if matches!(owner, LinkOwner::Person(_)) {
            picture.get_or_insert(Picture::Whole(media_id));
        }
        result.media_links.push(MediaLink {
            id: Uuid::now_v7(),
            media_id,
            person_id,
            event_id,
            source_id: None,
            family_id,
            sort_order,
        });
    }
    picture
}

/// A pixel rectangle of a page: its origin and its size.
#[derive(Clone, Copy)]
struct Rectangle {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

/// The page a crop on `media_id` cuts, and the rectangle its `CROP` names on
/// it. A crop that names no page, or a rectangle the page cannot hold, is the
/// reason it cannot be an identification.
fn crop_identification(
    crop: View<'_, Crop>,
    media_id: Uuid,
    result: &ImportResult,
) -> Result<(Uuid, Rectangle), String> {
    let page =
        crop_page(media_id, result).ok_or_else(|| "does not cut a single page".to_owned())?;
    let coordinate = |value: u32| {
        i32::try_from(value).map_err(|_| format!("has the value {value}, which is too large"))
    };
    let x = crop.left().map_or(Ok(0), coordinate)?;
    let y = crop.top().map_or(Ok(0), coordinate)?;
    let width = match (crop.width(), page.width) {
        (Some(width), _) => coordinate(width)?,
        (None, Some(page_width)) => page_width - x,
        (None, None) => return Err("has no WIDTH and the image size is unknown".to_owned()),
    };
    let height = match (crop.height(), page.height) {
        (Some(height), _) => coordinate(height)?,
        (None, Some(page_height)) => page_height - y,
        (None, None) => return Err("has no HEIGHT and the image size is unknown".to_owned()),
    };
    page.validate_crop(x, y, width, height)
        .map_err(|err| format!("is not a valid rectangle: {err}"))?;
    Ok((
        page.id,
        Rectangle {
            x,
            y,
            width,
            height,
        },
    ))
}

/// The page a crop on `media_id` cuts: the media itself when it is a page,
/// or the one page of a document. A document of several pages has no single
/// page to cut.
fn crop_page(media_id: Uuid, result: &ImportResult) -> Option<&Media> {
    let media = result.media.iter().find(|media| media.id == media_id)?;
    if !media.is_document() {
        return Some(media);
    }
    let mut pages = result
        .media
        .iter()
        .filter(|page| page.parent_media_id == Some(media_id));
    let page = pages.next()?;
    pages.next().is_none().then_some(page)
}

/// The person an event's crop identifies, from its `_OXIDGENE_PERSON`
/// pointer. A pointer to nobody in the file is a warning, and the crop is
/// then unattributed.
fn event_person(
    mm: View<'_, MultimediaLink>,
    ctx: &ImportContext,
    result: &mut ImportResult,
) -> Option<Uuid> {
    let node = tagged(mm.extra(), crate::export::PERSON_TAG).next()?;
    let xref = mm.store().xref(node.pointer()?);
    let person = ctx.indi_map.get(xref).copied();
    if person.is_none() {
        result
            .warnings
            .push(format!("Vignette: person {xref} was not found"));
    }
    person
}

/// Record an identification and return the id of the vignette holding it. A
/// crop beneath a person and the same crop beneath one of that person's
/// events are one identification, which carries the event: the second is
/// merged into the first.
fn add_vignette(vignette: Vignette, result: &mut ImportResult) -> Uuid {
    let same_region = |other: &Vignette| {
        other.media_id == vignette.media_id
            && other.person_id == vignette.person_id
            && (other.x, other.y, other.width, other.height)
                == (vignette.x, vignette.y, vignette.width, vignette.height)
    };
    if vignette.person_id.is_some()
        && let Some(existing) = result.vignettes.iter_mut().rev().find(|other| {
            same_region(other) && (other.event_id.is_none() || vignette.event_id.is_none())
        })
    {
        existing.event_id = existing.event_id.or(vignette.event_id);
        return existing.id;
    }
    let id = vignette.id;
    result.vignettes.push(vignette);
    id
}

/// Give a person the picture their first link gives them, unless a portrait
/// is stored already.
fn set_portrait(result: &mut ImportResult, person_id: Uuid, picture: Picture) {
    let Some(person) = result
        .persons
        .iter_mut()
        .rev()
        .find(|person| person.id == person_id)
    else {
        return;
    };
    if person.portrait_media_id.is_some() || person.portrait_vignette_id.is_some() {
        return;
    }
    match picture {
        Picture::Whole(media_id) => person.portrait_media_id = Some(media_id),
        Picture::Crop(vignette_id) => person.portrait_vignette_id = Some(vignette_id),
    }
}

/// Individual-side family links. A family and its member can each name the
/// other, and a file may write only one side: an individual's `FAMC` or `FAMS`
/// whose family does not list them is linked all the same, with a warning, so
/// that the link and its `PEDI` are not lost.
fn apply_individual_family_links(data: &Dataset, ctx: &ImportContext, result: &mut ImportResult) {
    link_children(data, ctx, result);
    link_spouses(data, ctx, result);
}

/// The `FAMC` links: a child the family lists keeps its row and takes the
/// `PEDI` of the link; a child only the link names gets a row.
fn link_children(data: &Dataset, ctx: &ImportContext, result: &mut ImportResult) {
    let st = data.store();
    let mut rows: HashMap<(Uuid, Uuid), Vec<usize>> = HashMap::new();
    for (index, fc) in result.family_children.iter().enumerate() {
        rows.entry((fc.person_id, fc.family_id))
            .or_default()
            .push(index);
    }
    let mut next_order = rows_per_family(result.family_children.iter().map(|fc| fc.family_id));

    for indi in data.individuals() {
        let Some(xref) = indi.xref().map(|x| st.xref(x)) else {
            continue;
        };
        let Some(&person_id) = ctx.indi_map.get(xref) else {
            continue;
        };
        for link in indi.child_of() {
            let Some(family) = link.family() else {
                continue;
            };
            let family_xref = st.xref(family);
            let Some(&family_id) = ctx.fam_map.get(family_xref) else {
                continue;
            };
            let pedigree = link
                .pedigree()
                .map(|pedigree| convert_pedigree(st, &pedigree.value()));
            let key = (person_id, family_id);
            if let Some(indexes) = rows.get(&key) {
                if let Some(child_type) = pedigree {
                    for &index in indexes {
                        result.family_children[index].child_type = child_type;
                    }
                }
                continue;
            }
            let sort_order = next_order_of(&mut next_order, family_id);
            rows.entry(key)
                .or_default()
                .push(result.family_children.len());
            result.family_children.push(FamilyChild {
                id: Uuid::now_v7(),
                family_id,
                person_id,
                child_type: pedigree.unwrap_or(ChildType::Biological),
                sort_order,
            });
            result.warnings.push(format!(
                "Individual {xref}: FAMC {family_xref} names a family that does not list them as a child; linked as its child"
            ));
        }
    }
}

/// The `FAMS` links: a spouse the family does not list gets a `FamilySpouse`
/// row, its role taken from their sex.
fn link_spouses(data: &Dataset, ctx: &ImportContext, result: &mut ImportResult) {
    let st = data.store();
    let sexes: HashMap<Uuid, Sex> = result.persons.iter().map(|p| (p.id, p.sex)).collect();
    let mut rows: HashSet<(Uuid, Uuid)> = result
        .family_spouses
        .iter()
        .map(|fs| (fs.person_id, fs.family_id))
        .collect();
    let mut next_order = rows_per_family(result.family_spouses.iter().map(|fs| fs.family_id));

    for indi in data.individuals() {
        let Some(xref) = indi.xref().map(|x| st.xref(x)) else {
            continue;
        };
        let Some(&person_id) = ctx.indi_map.get(xref) else {
            continue;
        };
        for link in indi.spouse_of() {
            let Some(family) = link.family() else {
                continue;
            };
            let family_xref = st.xref(family);
            let Some(&family_id) = ctx.fam_map.get(family_xref) else {
                continue;
            };
            if !rows.insert((person_id, family_id)) {
                continue;
            }
            let role = match sexes.get(&person_id) {
                Some(Sex::Male) => SpouseRole::Husband,
                Some(Sex::Female) => SpouseRole::Wife,
                _ => SpouseRole::Partner,
            };
            let sort_order = next_order_of(&mut next_order, family_id);
            result.family_spouses.push(FamilySpouse {
                id: Uuid::now_v7(),
                family_id,
                person_id,
                role,
                sort_order,
            });
            result.warnings.push(format!(
                "Individual {xref}: FAMS {family_xref} names a family that does not list them as a spouse; linked as its spouse"
            ));
        }
    }
}

/// How many rows each family already holds, keyed by family: the next
/// `sort_order` of a family is its count.
fn rows_per_family(families: impl Iterator<Item = Uuid>) -> HashMap<Uuid, i32> {
    let mut counts = HashMap::new();
    for family_id in families {
        *counts.entry(family_id).or_insert(0) += 1;
    }
    counts
}

/// The `sort_order` for the next row of a family, which then counts that row.
fn next_order_of(counts: &mut HashMap<Uuid, i32>, family_id: Uuid) -> i32 {
    let count = counts.entry(family_id).or_insert(0);
    let order = *count;
    *count += 1;
    order
}

/// An association's relation in words: its `RELA` (5.5.1), else the phrase
/// or the value of its `ROLE` (7.0).
fn association_relation(assoc: View<'_, Association>) -> Option<String> {
    opt(assoc.relation()).or_else(|| {
        let role = assoc.role()?;
        opt(role.phrase()).or_else(|| match role.value() {
            oxidged::model::Role::Unknown(t) => opt_owned(assoc.store(), &t),
            known => known.as_str(GedcomVersion::V7_0).map(str::to_string),
        })
    })
}

/// The associations a record holds: its `ASSO`, and the `_ASSO` a conformant
/// writer makes of an association its version does not permit — one
/// pointing at a family, or one beneath an event in 5.5.1 — as `(pointer,
/// relation)`. An `_ASSO`'s relation is read as an `ASSO`'s: its `RELA`,
/// else the phrase or the value of its `ROLE`, which a GEDCOM 7.0 writer
/// makes of the `RELA`.
fn associations_of<'a>(
    st: &'a Store,
    typed: Items<'a, Association>,
    extra: Items<'a, Node>,
) -> impl Iterator<Item = (&'a str, Option<String>)> + 'a {
    let typed = typed
        .into_iter()
        .filter_map(move |assoc| Some((st.xref(assoc.individual()?), association_relation(assoc))));
    let kept = tagged(extra, "_ASSO").filter_map(move |node| {
        let pointer = node.pointer()?;
        let text_of = |tags: &[&str]| {
            node.children()
                .into_iter()
                .filter(|child| tags.contains(&child.tag()))
                .find_map(|child| non_blank_str(node_text(child).as_deref()))
        };
        let role = || {
            let role = node
                .children()
                .into_iter()
                .find(|child| child.tag() == "ROLE")?;
            tagged(role.children(), "PHRASE")
                .find_map(|child| non_blank_str(node_text(child).as_deref()))
                .or_else(|| non_blank_str(node_text(role).as_deref()))
        };
        let relation = text_of(&["RELA", "_RELA"]).or_else(role);
        Some((st.xref(pointer), relation))
    });
    typed.chain(kept)
}

/// Associations (top-level `1 ASSO` — Gramps' witness/godparent convention).
///
/// Gramps (and the GEDCOM 5.5.1 grammar) places `ASSO` as a direct child of
/// the INDI record that "owns" the relationship, never nested inside a
/// specific event. Which direction it describes depends on what the xref
/// resolves to:
///   - target is a FAM: the owner witnessed that family's marriage
///     (`1 ASSO @F1@` / `2 RELA witness` on the witness's own record, which a
///     conformant 5.5.1 writer keeps as `1 _ASSO @F1@`).
///   - target is an INDI: the target holds a role (godparent, ...) at
///     the owner's own birth/baptism (`1 ASSO @I2@` / `2 RELA GODM` on
///     the godchild's own record).
///
/// GEDCOM's ASSO has no way to name which specific event it applies to
/// when the owner has several candidate events, so that case is a
/// best-effort guess (flagged with a warning), not a guarantee.
fn import_associations(data: &Dataset, ctx: &ImportContext, result: &mut ImportResult) {
    let st = data.store();
    let mut witnesses = WitnessLog::new(result);
    for indi in data.individuals() {
        let Some(owner_xref) = indi.xref().map(|id| st.xref(id)) else {
            continue;
        };
        let Some(&owner_person_id) = ctx.indi_map.get(owner_xref) else {
            continue;
        };
        for (target, relation) in associations_of(st, indi.associations(), indi.extra()) {
            let assoc = Associated { target, relation };
            if let Some(&family_id) = ctx.fam_map.get(target) {
                witness_family_event(
                    owner_xref,
                    owner_person_id,
                    family_id,
                    assoc,
                    &mut witnesses,
                    result,
                );
            } else if let Some(&role_holder_id) = ctx.indi_map.get(target) {
                witness_individual_event(
                    owner_xref,
                    owner_person_id,
                    role_holder_id,
                    assoc,
                    &mut witnesses,
                    result,
                );
            } else {
                result.warnings.push(format!(
                    "Individual {owner_xref}: ASSO {target} target not found in file — skipped"
                ));
            }
        }
    }
}

/// An association read: who or what it points at, and the relation.
struct Associated<'a> {
    target: &'a str,
    relation: Option<String>,
}

/// The key an association's relation is compared by when two of them name the
/// same person at one event: its text trimmed and without case, an absent
/// relation as the empty text.
fn relation_key(relation: Option<&str>) -> String {
    relation
        .map(|r| r.trim().to_lowercase())
        .unwrap_or_default()
}

/// The witnesses recorded so far, and the next sort order of each event's.
///
/// Some exporters (Gramps included) redundantly *also* nest an ASSO inside
/// the witnessed event's own detail (caught by `import_event_detail`'s
/// `detail.associations` loop) for the same fact the association pass would
/// otherwise reconstruct — an (event, person, relation) triple already present
/// in `result.event_witnesses` is not recorded twice, and a witness the pass
/// adds to an event comes after the ones nested in it. The same person in
/// another role at that event is another witness.
struct WitnessLog {
    seen: std::collections::HashSet<(Uuid, Uuid, String)>,
    next_sort: HashMap<Uuid, i32>,
}

impl WitnessLog {
    fn new(result: &ImportResult) -> Self {
        let mut next_sort: HashMap<Uuid, i32> = HashMap::new();
        for w in &result.event_witnesses {
            let next = next_sort.entry(w.event_id).or_insert(0);
            *next = (*next).max(w.sort_order + 1);
        }
        Self {
            seen: result
                .event_witnesses
                .iter()
                .map(|w| (w.event_id, w.person_id, relation_key(w.relation.as_deref())))
                .collect(),
            next_sort,
        }
    }

    fn contains(&self, event_id: Uuid, person_id: Uuid, relation: Option<&str>) -> bool {
        self.seen
            .contains(&(event_id, person_id, relation_key(relation)))
    }

    /// Record `person_id` as a witness of `event_id`, after the others.
    fn record(
        &mut self,
        result: &mut ImportResult,
        event_id: Uuid,
        person_id: Uuid,
        relation: Option<String>,
    ) {
        self.seen
            .insert((event_id, person_id, relation_key(relation.as_deref())));
        let sort_order = self.next_sort.entry(event_id).or_insert(0);
        result.event_witnesses.push(EventWitness {
            id: Uuid::now_v7(),
            event_id,
            person_id,
            relation,
            sort_order: *sort_order,
        });
        *sort_order += 1;
    }
}

/// An `ASSO` to a family: the owner witnessed its marriage, or its first
/// event when it has none.
fn witness_family_event(
    owner_xref: &str,
    owner_person_id: Uuid,
    family_id: Uuid,
    assoc: Associated<'_>,
    witnesses: &mut WitnessLog,
    result: &mut ImportResult,
) {
    let family_events = || {
        result
            .events
            .iter()
            .filter(move |e| e.family_id == Some(family_id))
    };
    let target_event = family_events()
        .find(|e| e.event_type == EventType::Marriage)
        .or_else(|| family_events().next())
        .map(|e| e.id);
    match target_event {
        Some(event_id)
            if witnesses.contains(event_id, owner_person_id, assoc.relation.as_deref()) => {}
        Some(event_id) => witnesses.record(result, event_id, owner_person_id, assoc.relation),
        None => result.warnings.push(format!(
            "Individual {owner_xref}: ASSO {} (family) has no event to attach the witness to — skipped",
            assoc.target
        )),
    }
}

/// An `ASSO` to an individual: the target holds a role at one of the
/// owner's own events.
///
/// GEDCOM 5.5.1 puts `ASSO` on the individual, not on an event — Gramps
/// rejects the event-nested form — so the event has to be inferred. Baptism
/// first, because that is where a godparent belongs and godparents are most
/// of what this tag carries; then birth, which stands in for a baptism
/// nobody recorded; then the owner's first event.
fn witness_individual_event(
    owner_xref: &str,
    owner_person_id: Uuid,
    role_holder_id: Uuid,
    assoc: Associated<'_>,
    witnesses: &mut WitnessLog,
    result: &mut ImportResult,
) {
    let candidates: Vec<&Event> = result
        .events
        .iter()
        .filter(|e| e.person_id == Some(owner_person_id))
        .collect();
    let of_type = |event_type| {
        candidates
            .iter()
            .find(|e| e.event_type == event_type)
            .copied()
    };
    let baptism = of_type(EventType::Baptism);
    let birth = of_type(EventType::Birth);
    // Only when the choice was genuinely arbitrary. An earlier version
    // warned whenever the person had more than one event, which fired even
    // where a baptism had been found and the answer was simply right — so a
    // clean import reported warnings nobody could act on, and the ones that
    // mattered were lost among them.
    let guessed = baptism.is_none() && birth.is_none() && candidates.len() > 1;
    let target_event = baptism
        .or(birth)
        .or_else(|| candidates.first().copied())
        .map(|e| (e.id, e.event_type));

    match target_event {
        Some((event_id, _))
            if witnesses.contains(event_id, role_holder_id, assoc.relation.as_deref()) => {}
        Some((event_id, event_type)) => {
            if guessed {
                result.warnings.push(format!(
                    "Individual {owner_xref}: ASSO {} attached to its {:?} event — \
                     {owner_xref} has several events and none of them is a birth or \
                     a baptism, so this is a guess",
                    assoc.target, event_type
                ));
            }
            witnesses.record(result, event_id, role_holder_id, assoc.relation);
        }
        None => result.warnings.push(format!(
            "Individual {owner_xref}: ASSO {} has no individual event to attach to — skipped",
            assoc.target
        )),
    }
}

/// Restore OxidGene metadata and crop annotations beneath top-level `OBJE`
/// records. `oxidged` keeps them as extension structures of the records; the
/// standard model is imported first and they are resolved against the xref
/// maps it produced.
fn import_oxidgene_media_extensions(data: &Dataset, result: &mut ImportResult) {
    import_media_metadata_extensions(data, result);
    import_document_extensions(data, result);
    import_document_tags(data, result);
    import_vignette_extensions(data, result);
}

/// The tags of the `_OXIDGENE_TAG` lines of `extra`, in the order of the
/// tag vocabulary, each read once: labels that fold alike are one tag, the
/// first spelling and colour read stay (docs/tags.md §3). A line that is not
/// read is left out with a warning.
fn read_tags(
    extra: Items<'_, Node>,
    vocabulary: &mut Vec<TagRef>,
    warnings: &mut Vec<String>,
) -> Vec<TagRef> {
    let mut found: Vec<TagRef> = Vec::new();
    for node in tagged(extra, crate::export::TAG_TAG) {
        let Some(value) = node_text(node) else {
            warnings.push("Empty _OXIDGENE_TAG line — skipped".to_string());
            continue;
        };
        let line = match serde_json::from_str::<TagExtension>(&value) {
            Ok(line) if line.v == 1 => line,
            Ok(line) => {
                warnings.push(format!(
                    "Unsupported _OXIDGENE_TAG version {} — skipped",
                    line.v
                ));
                continue;
            }
            Err(err) => {
                warnings.push(format!("Invalid _OXIDGENE_TAG line: {err} — skipped"));
                continue;
            }
        };
        let label = line.label.trim();
        if label.is_empty() || label.chars().count() > TAG_LABEL_MAX {
            warnings.push(format!(
                "_OXIDGENE_TAG label {label:?} is empty or longer than {TAG_LABEL_MAX} characters — skipped"
            ));
            continue;
        }
        let folded = fold_words(label);
        let tag = match vocabulary
            .iter()
            .find(|tag| fold_words(&tag.label) == folded)
        {
            Some(tag) => tag.clone(),
            None => {
                let color = if (0..TAG_COLOR_SLOTS).contains(&line.color) {
                    line.color
                } else {
                    warnings.push(format!(
                        "_OXIDGENE_TAG colour {} is out of range — the first colour is used",
                        line.color
                    ));
                    0
                };
                let tag = TagRef {
                    id: Uuid::now_v7(),
                    label: label.to_string(),
                    color,
                };
                vocabulary.push(tag.clone());
                tag
            }
        };
        if !found.iter().any(|known| known.id == tag.id) {
            found.push(tag);
        }
    }
    sorted_tags(found)
}

/// Tags sorted by folded label, as a record carries them (docs/tags.md §4).
fn sorted_tags(mut tags: Vec<TagRef>) -> Vec<TagRef> {
    tags.sort_by_cached_key(|tag| (fold_words(&tag.label), tag.id));
    tags
}

/// Read the tags of each person and couple beneath its `INDI` or `FAM`
/// record, and build the file's tag vocabulary.
fn import_person_and_family_tags(data: &Dataset, ctx: &ImportContext, result: &mut ImportResult) {
    let st = data.store();
    let mut vocabulary = std::mem::take(&mut result.tags);
    let mut person_tags: HashMap<Uuid, Vec<TagRef>> = HashMap::new();
    for indi in data.individuals() {
        let Some(&id) = indi.xref().and_then(|xref| ctx.indi_map.get(st.xref(xref))) else {
            continue;
        };
        let tags = read_tags(indi.extra(), &mut vocabulary, &mut result.warnings);
        if !tags.is_empty() {
            person_tags.insert(id, tags);
        }
    }
    let mut family_tags: HashMap<Uuid, Vec<TagRef>> = HashMap::new();
    for fam in data.families() {
        let Some(&id) = fam.xref().and_then(|xref| ctx.fam_map.get(st.xref(xref))) else {
            continue;
        };
        let tags = read_tags(fam.extra(), &mut vocabulary, &mut result.warnings);
        if !tags.is_empty() {
            family_tags.insert(id, tags);
        }
    }
    for person in &mut result.persons {
        if let Some(tags) = person_tags.remove(&person.id) {
            person.tags = tags;
        }
    }
    for family in &mut result.families {
        if let Some(tags) = family_tags.remove(&family.id) {
            family.tags = tags;
        }
    }
    result.tags = vocabulary;
}

/// Read the tags beneath each document's first `OBJE` (docs/tags.md §3).
/// Runs once the pages are regrouped, so that the tags of a page land on the
/// document it now belongs to.
fn import_document_tags(data: &Dataset, result: &mut ImportResult) {
    let st = data.store();
    let mut vocabulary = std::mem::take(&mut result.tags);
    let mut page_tags: Vec<(Uuid, Vec<TagRef>)> = Vec::new();
    for mm in data.multimedia() {
        let Some(&page_id) = mm
            .xref()
            .and_then(|xref| result.media_by_xref.get(st.xref(xref)))
        else {
            continue;
        };
        let tags = read_tags(mm.extra(), &mut vocabulary, &mut result.warnings);
        if !tags.is_empty() {
            page_tags.push((page_id, tags));
        }
    }
    let parent_of: HashMap<Uuid, Uuid> = result
        .media
        .iter()
        .filter_map(|item| Some((item.id, item.parent_media_id?)))
        .collect();
    let mut document_tags: HashMap<Uuid, Vec<TagRef>> = HashMap::new();
    for (page_id, tags) in page_tags {
        let Some(&document_id) = parent_of.get(&page_id) else {
            continue;
        };
        let known = document_tags.entry(document_id).or_default();
        for tag in tags {
            if !known.iter().any(|existing| existing.id == tag.id) {
                known.push(tag);
            }
        }
    }
    for item in &mut result.media {
        if let Some(tags) = document_tags.remove(&item.id) {
            item.tags = sorted_tags(tags);
        }
    }
    result.tags = vocabulary;
}

/// The values of the `tag` extension beneath each top-level `OBJE`, with the
/// xref of the record each sits under.
fn media_extensions<'d>(data: &'d Dataset, tag: &'d str) -> Vec<(&'d str, Cow<'d, str>)> {
    let st = data.store();
    data.multimedia()
        .into_iter()
        .filter_map(|mm| Some((st.xref(mm.xref()?), mm.extra())))
        .flat_map(|(xref, extensions)| {
            tagged(extensions, tag).filter_map(move |extension| Some((xref, node_text(extension)?)))
        })
        .collect()
}

/// Restore what a page says about itself: its file name, its timestamps and
/// its transcript.
fn import_media_metadata_extensions(data: &Dataset, result: &mut ImportResult) {
    for (media_xref, value) in media_extensions(data, "_OXIDGENE_MEDIA") {
        let Some(media_id) = result.media_by_xref.get(media_xref).copied() else {
            continue;
        };
        let metadata = match serde_json::from_str::<MediaMetadataExtension>(&value) {
            Ok(metadata) if metadata.version == 1 => metadata,
            Ok(metadata) => {
                result.warnings.push(format!(
                    "Media {media_xref}: unsupported OxidGene metadata version {}",
                    metadata.version
                ));
                continue;
            }
            Err(err) => {
                result.warnings.push(format!(
                    "Media {media_xref}: invalid OxidGene metadata: {err}"
                ));
                continue;
            }
        };

        let Some(media) = result.media.iter_mut().find(|item| item.id == media_id) else {
            continue;
        };
        media.file_name = metadata.file_name;
        media.created_at = metadata.created_at.unwrap_or(media.created_at);
        media.updated_at = metadata.updated_at.unwrap_or(media.updated_at);
        let tree_id = media.tree_id;

        for note in metadata.notes {
            let created_at = note.created_at.unwrap_or_else(Utc::now);
            result.notes.push(Note {
                id: Uuid::now_v7(),
                tree_id,
                text: note.text,
                person_id: None,
                event_id: None,
                family_id: None,
                source_id: None,
                media_id: Some(media_id),
                repository_id: None,
                shared_group: None,
                created_at,
                updated_at: note.updated_at.unwrap_or(created_at),
                deleted_at: None,
            });
        }
    }
}

/// Put the pages of a document back together.
///
/// The standard import gave every `OBJE` a document of its own, which is the
/// right answer for a foreign file. Here the pages that name the same `doc`
/// token are re-parented onto one of those documents, the now-empty ones are
/// dropped, and any link that pointed at a dropped document is moved to the
/// survivor — so a person attached to a forty-page dossier stays attached to
/// it rather than to page one.
fn import_document_extensions(data: &Dataset, result: &mut ImportResult) {
    let by_token = document_pages(data, result);
    let dropped = regroup_document_pages(result, by_token);
    if !dropped.is_empty() {
        redirect_to_kept_documents(result, &dropped);
    }
}

/// The imported pages carrying an OxidGene document extension, by document
/// token; a page whose extension cannot be read is left alone, with a warning.
fn document_pages(
    data: &Dataset,
    result: &mut ImportResult,
) -> HashMap<String, Vec<(Uuid, DocumentExtension)>> {
    let mut by_token: HashMap<String, Vec<(Uuid, DocumentExtension)>> = HashMap::new();
    for (media_xref, value) in media_extensions(data, "_OXIDGENE_DOC") {
        let Some(page_id) = result.media_by_xref.get(media_xref).copied() else {
            continue;
        };
        let container = match serde_json::from_str::<DocumentExtension>(&value) {
            Ok(container) if container.version == 1 => container,
            Ok(container) => {
                result.warnings.push(format!(
                    "Media {media_xref}: unsupported OxidGene document version {}",
                    container.version
                ));
                continue;
            }
            Err(err) => {
                result.warnings.push(format!(
                    "Media {media_xref}: invalid OxidGene document metadata: {err}"
                ));
                continue;
            }
        };
        by_token
            .entry(container.doc.clone())
            .or_default()
            .push((page_id, container));
    }
    by_token
}

/// Re-parent each document's pages onto the document written for its first
/// page; the documents left empty, each with the one kept in its place.
fn regroup_document_pages(
    result: &mut ImportResult,
    by_token: HashMap<String, Vec<(Uuid, DocumentExtension)>>,
) -> HashMap<Uuid, Uuid> {
    let mut dropped: HashMap<Uuid, Uuid> = HashMap::new();
    for (token, mut pages) in by_token {
        pages.sort_by_key(|(_, container)| container.index);
        // The document to keep is the one written for the first page, because
        // that is the page the shared metadata travelled on.
        let Some(keeper) = pages
            .first()
            .and_then(|(page_id, _)| document_of(result, *page_id))
        else {
            result
                .warnings
                .push(format!("Document {token}: none of its pages was imported"));
            continue;
        };

        for (index, (page_id, _)) in pages.iter().enumerate() {
            if let Some(previous) = document_of(result, *page_id)
                && previous != keeper
            {
                dropped.insert(previous, keeper);
            }
            if let Some(page) = result.media.iter_mut().find(|item| item.id == *page_id) {
                page.parent_media_id = Some(keeper);
                page.page_index = index as i32;
            }
        }

        let page_count = pages.len() as i32;
        let meta = pages.into_iter().find_map(|(_, container)| container.meta);
        apply_document_metadata(result, keeper, page_count, meta);
    }
    dropped
}

/// Remove the `dropped` documents, moving what named them to the document
/// kept in their place.
fn redirect_to_kept_documents(result: &mut ImportResult, dropped: &HashMap<Uuid, Uuid>) {
    // A link, a portrait or a note that named a dropped document now names the
    // survivor; anything still pointing at a row we are about to remove would
    // dangle.
    for link in &mut result.media_links {
        if let Some(keeper) = dropped.get(&link.media_id) {
            link.media_id = *keeper;
        }
    }
    for note in &mut result.notes {
        if let Some(media_id) = note.media_id
            && let Some(keeper) = dropped.get(&media_id)
        {
            note.media_id = Some(*keeper);
        }
    }
    for person in &mut result.persons {
        if let Some(media_id) = person.portrait_media_id
            && let Some(keeper) = dropped.get(&media_id)
        {
            person.portrait_media_id = Some(*keeper);
        }
    }
    result.media.retain(|item| !dropped.contains_key(&item.id));
    // Two links to the same document, one per page, are one link.
    result.media_links.sort_by_key(|link| {
        (
            link.media_id,
            link.person_id,
            link.family_id,
            link.event_id,
            link.source_id,
        )
    });
    result.media_links.dedup_by_key(|link| {
        (
            link.media_id,
            link.person_id,
            link.family_id,
            link.event_id,
            link.source_id,
        )
    });
}

/// The document a page belongs to, as the result currently records it.
fn document_of(result: &ImportResult, page_id: Uuid) -> Option<Uuid> {
    result
        .media
        .iter()
        .find(|item| item.id == page_id)
        .and_then(|page| page.parent_media_id)
}

/// Write what a document says about itself onto its row.
fn apply_document_metadata(
    result: &mut ImportResult,
    document_id: Uuid,
    page_count: i32,
    meta: Option<crate::DocumentMetadataExtension>,
) {
    let Some(meta) = meta else {
        if let Some(document) = result.media.iter_mut().find(|item| item.id == document_id) {
            document.page_count = page_count;
        }
        return;
    };

    let place_id = meta.place.as_ref().and_then(|place| {
        if place.name.trim().is_empty() {
            return None;
        }
        if let Some(existing) = result
            .places
            .iter_mut()
            .find(|item| item.name == place.name)
        {
            existing.latitude = existing.latitude.or(place.latitude);
            existing.longitude = existing.longitude.or(place.longitude);
            return Some(existing.id);
        }
        let id = Uuid::now_v7();
        let now = Utc::now();
        let tree_id = result
            .media
            .iter()
            .find(|item| item.id == document_id)
            .map(|item| item.tree_id)?;
        result.places.push(Place {
            id,
            tree_id,
            name: place.name.clone(),
            latitude: place.latitude,
            longitude: place.longitude,
            created_at: now,
            updated_at: now,
        });
        Some(id)
    });

    let Some(document) = result.media.iter_mut().find(|item| item.id == document_id) else {
        return;
    };
    document.file_name = meta.file_name;
    document.created_at = meta.created_at.unwrap_or(document.created_at);
    document.updated_at = meta.updated_at.unwrap_or(document.updated_at);
    document.title = meta.title;
    document.description = meta.description;
    document.date_value = meta.date_value;
    document.date_qualifier = meta.date_qualifier;
    document.date_value2 = meta.date_value2;
    document.calendar = meta.calendar;
    document.date_sort = crate::date::sort_key(document.calendar, document.date_value.as_deref());
    document.privacy = meta.privacy;
    document.source_media_type = meta.source_media_type;
    document.document_category = meta.document_category;
    document.place_id = place_id;
    document.page_count = page_count;
    let tree_id = document.tree_id;

    for note in meta.notes {
        let created_at = note.created_at.unwrap_or_else(Utc::now);
        result.notes.push(Note {
            id: Uuid::now_v7(),
            tree_id,
            text: note.text,
            person_id: None,
            event_id: None,
            family_id: None,
            source_id: None,
            media_id: Some(document_id),
            repository_id: None,
            shared_group: None,
            created_at,
            updated_at: note.updated_at.unwrap_or(created_at),
            deleted_at: None,
        });
    }
}

/// Restore the identifications that name neither a person nor an event: one
/// `_OXIDGENE_VIGNETTE <x> <y> <width> <height>` beneath each page. The others
/// are `CROP`s beneath their links (see [`import_media_links`]).
fn import_vignette_extensions(data: &Dataset, result: &mut ImportResult) {
    for (media_xref, value) in media_extensions(data, "_OXIDGENE_VIGNETTE") {
        let fields = value.split_whitespace().collect::<Vec<_>>();
        let [x, y, width, height] = fields.as_slice() else {
            result.warnings.push(format!(
                "Vignette on {media_xref}: unrecognised _OXIDGENE_VIGNETTE line — skipped"
            ));
            continue;
        };
        let Some(media_id) = result.media_by_xref.get(media_xref).copied() else {
            continue;
        };
        let parsed = (
            x.parse::<i32>(),
            y.parse::<i32>(),
            width.parse::<i32>(),
            height.parse::<i32>(),
        );
        let (Ok(x), Ok(y), Ok(width), Ok(height)) = parsed else {
            result.warnings.push(format!(
                "Vignette on {media_xref}: invalid crop coordinates"
            ));
            continue;
        };
        if x < 0 || y < 0 || width <= 0 || height <= 0 {
            result
                .warnings
                .push(format!("Vignette on {media_xref}: invalid crop rectangle"));
            continue;
        }
        let now = Utc::now();
        result.vignettes.push(Vignette {
            id: Uuid::now_v7(),
            media_id,
            x,
            y,
            width,
            height,
            person_id: None,
            event_id: None,
            created_at: now,
            updated_at: now,
        });
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Conversion helpers
// ═══════════════════════════════════════════════════════════════════════

fn convert_gender(sex: &GedSex) -> Sex {
    match sex {
        GedSex::Male => Sex::Male,
        GedSex::Female => Sex::Female,
        _ => Sex::Unknown,
    }
}

/// The first piece of a kind of a name, as written.
fn piece(name: View<'_, GedName>, kind: NamePieceKind) -> Option<String> {
    name.pieces_of(kind).find_map(|piece| opt(Some(piece)))
}

/// Converts a GEDCOM `NAME` structure into a `PersonName`, plus any surname
/// aliases packed into its `SURN` sub-tag (see `surname_aliases_from_surn`).
fn convert_name(
    name: View<'_, GedName>,
    person_id: Uuid,
    is_primary: bool,
    now: chrono::DateTime<Utc>,
) -> (PersonName, Vec<PersonName>) {
    let name_type = name.kind().map_or(NameType::Birth, |kind| {
        convert_name_type(name.store(), &kind.value())
    });

    // A name has its pieces (GIVN, SURN, …) only when the file writes them;
    // most GEDCOM files have the full name on the NAME line alone (e.g.
    // "John /DOE/"). The NAME line's surname wins over SURN: some exporters
    // (Geneanet) pack a comma-separated list of surname aliases into SURN
    // instead of matching NAME, so SURN can't be trusted as the primary
    // surname.
    let value = opt(Some(name.value()));
    let (parsed_given, parsed_surname) = parse_name_value(value.as_deref());
    let surn = piece(name, NamePieceKind::Surname);
    let given_names = piece(name, NamePieceKind::Given).or(parsed_given);
    let surname = parsed_surname.or_else(|| surn.clone());

    let spfx = piece(name, NamePieceKind::SurnamePrefix);
    let (surname_prefix, surname) = split_import_surname(surname.as_deref(), spfx.as_deref());

    // Compare SURN against the *root*, not the full surname: a file writing
    // `1 NAME Lois /de la Cruz/` + `2 SURN Cruz` is stating the same surname
    // twice, and matching on the full form would read the SURN as an alias.
    let aliases = surname_aliases_from_surn(surn.as_deref(), surname.as_deref())
        .into_iter()
        .map(|alias_surname| {
            // Aliases carry no SPFX of their own, so derive one.
            let (alias_prefix, alias_root) = split_import_surname(Some(&alias_surname), None);
            PersonName {
                id: Uuid::now_v7(),
                person_id,
                name_type: NameType::AlsoKnownAs,
                given_names: given_names.clone(),
                surname: alias_root,
                surname_prefix: alias_prefix,
                prefix: None,
                suffix: None,
                nickname: None,
                is_primary: false,
                sort_order: 0,
                created_at: now,
                updated_at: now,
            }
        })
        .collect();

    let person_name = PersonName {
        id: Uuid::now_v7(),
        person_id,
        name_type,
        given_names,
        surname,
        surname_prefix,
        prefix: piece(name, NamePieceKind::Prefix),
        suffix: piece(name, NamePieceKind::Suffix),
        nickname: piece(name, NamePieceKind::Nickname),
        is_primary,
        sort_order: 0,
        created_at: now,
        updated_at: now,
    };

    (person_name, aliases)
}

/// Turns an `aka` name that only adds a nickname into a byname.
///
/// GEDCOM gives a `NAME` one `NICK`, so a person known by several nicknames
/// gets one `aka` name per further nickname, each restating the person's
/// name (as the `geneweb` crate writes the extra `#nick` of a `.gw`) or
/// leaving it empty (as OxidGene writes a byname). The name it restates is
/// not a variant of anything: what the record holds is a byname, which is how
/// the person form records one — the nickname alone.
fn reduce_to_byname(name: &mut PersonName, primary: &PersonName) {
    let restates = |own: &Option<String>, primary: &Option<String>| own.is_none() || own == primary;
    if name.name_type == NameType::AlsoKnownAs
        && name.nickname.is_some()
        && name.prefix.is_none()
        && name.suffix.is_none()
        && restates(&name.given_names, &primary.given_names)
        && restates(&name.surname, &primary.surname)
        && restates(&name.surname_prefix, &primary.surname_prefix)
    {
        name.name_type = NameType::Byname;
        name.given_names = None;
        name.surname = None;
        name.surname_prefix = None;
    }
}

/// Separates an imported surname into `(particle, root)`.
///
/// The particle comes from the file's `SPFX` when it has one, and is derived
/// from the surname itself otherwise. The two need reconciling because GEDCOM
/// repeats the particle in both places: `1 NAME Lois /de la Cruz/` carries it
/// inside the slashes *and* in `2 SPFX de la`, so taking both at face value
/// would yield "de la de la Cruz".
fn split_import_surname(
    surname: Option<&str>,
    spfx: Option<&str>,
) -> (Option<String>, Option<String>) {
    let raw = surname.map(str::trim).filter(|s| !s.is_empty());
    let spfx = spfx.map(str::trim).filter(|s| !s.is_empty());

    let Some(raw) = raw else {
        return (spfx.map(str::to_string), None);
    };

    // An explicit SPFX overrides detection, exactly as a user correcting the
    // particle in the person form does — same helper, so the two agree.
    let (particle, root) = match spfx {
        Some(spfx) => split_surname_with(raw, spfx),
        None => split_surname_particle(raw),
    };
    (particle, Some(root))
}

/// Splits a `SURN` value on `,` and returns the parts that differ from the
/// resolved primary surname. Geneanet's exporter packs surname variants into
/// a single `SURN` tag (e.g. "LE NADEN,NADAM") instead of emitting one
/// `NAME`/`SURN` structure per variant; each distinct part becomes its own
/// "also known as" `PersonName` so the alternate spellings aren't lost.
fn surname_aliases_from_surn(surn: Option<&str>, primary_root: Option<&str>) -> Vec<String> {
    let Some(surn) = surn else {
        return Vec::new();
    };
    let primary = primary_root.unwrap_or_default().trim().to_lowercase();
    surn.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        // Compare roots, not raw strings: `1 NAME Lois /de la Cruz/` with
        // `2 SURN de la Cruz` gives the full "de la Cruz" while `primary_root` is
        // the split-off "Cruz". Comparing those verbatim would read the
        // person's own surname back as an alias of itself.
        .filter(|s| split_surname_particle(s).1.to_lowercase() != primary)
        .collect()
}

/// Parse given name and surname from a GEDCOM NAME value (e.g. "John /DOE/").
/// Surname is the text between `/` delimiters; given names are everything before the first `/`.
fn parse_name_value(value: Option<&str>) -> (Option<String>, Option<String>) {
    let Some(val) = value else {
        return (None, None);
    };

    let surname = val.find('/').and_then(|start| {
        val[start + 1..].find('/').and_then(|end| {
            let s = val[start + 1..start + 1 + end].trim();
            if s.is_empty() {
                None
            } else {
                Some(s.to_string())
            }
        })
    });

    let given = val.find('/').and_then(|slash| {
        let g = val[..slash].trim();
        if g.is_empty() {
            None
        } else {
            Some(g.to_string())
        }
    });

    (given, surname)
}

fn convert_name_type(st: &Store, nt: &GedNameType) -> NameType {
    match nt {
        GedNameType::Birth => NameType::Birth,
        GedNameType::Married => NameType::Married,
        GedNameType::Maiden => NameType::Maiden,
        GedNameType::Aka => NameType::AlsoKnownAs,
        // GEDCOM 5.5.1's NAME.TYPE is an open set: `religious` is the value
        // OxidGene's export writes for a religious name.
        GedNameType::Unknown(t) if t.to_str(st).trim().eq_ignore_ascii_case("religious") => {
            NameType::Religious
        }
        _ => NameType::Other,
    }
}

/// The type of an event or an attribute, by its tag; a generic `EVEN`'s from
/// its free-text `TYPE`.
fn convert_event_type(kind: &EventKind, type_text: Option<&str>) -> EventType {
    match kind {
        EventKind::Birth => EventType::Birth,
        EventKind::Death => EventType::Death,
        EventKind::Baptism => EventType::Baptism,
        EventKind::Confirmation => EventType::Confirmation,
        EventKind::FirstCommunion => EventType::FirstCommunion,
        EventKind::BarMitzvah => EventType::BarBatMitzvah,
        EventKind::BasMitzvah => EventType::BasMitzvah,
        EventKind::Burial => EventType::Burial,
        EventKind::Cremation => EventType::Cremation,
        EventKind::Graduation => EventType::Graduation,
        EventKind::Immigration => EventType::Immigration,
        EventKind::Emigration => EventType::Emigration,
        EventKind::Naturalization => EventType::Naturalization,
        EventKind::Census => EventType::Census,
        EventKind::Residence => EventType::Residence,
        EventKind::Retirement => EventType::Retirement,
        EventKind::Will => EventType::Will,
        EventKind::Probate => EventType::Probate,
        EventKind::Marriage => EventType::Marriage,
        EventKind::Divorce => EventType::Divorce,
        EventKind::Annulment => EventType::Annulment,
        EventKind::Engagement => EventType::Engagement,
        EventKind::MarriageBann => EventType::MarriageBann,
        EventKind::MarriageContract => EventType::MarriageContract,
        EventKind::MarriageLicense => EventType::MarriageLicense,
        EventKind::MarriageSettlement => EventType::MarriageSettlement,
        EventKind::DivorceFiled => EventType::DivorceFiled,
        EventKind::Adoption => EventType::Adoption,
        EventKind::Blessing => EventType::Blessing,
        EventKind::Ordination => EventType::Ordination,
        EventKind::Christening => EventType::Christening,
        EventKind::AdultChristening => EventType::AdultChristening,
        // GEDCOM's INDIVIDUAL_ATTRIBUTE_STRUCTURE (and a family's NCHI,
        // RESI and FACT).
        EventKind::Occupation => EventType::Occupation,
        EventKind::Caste => EventType::CasteName,
        EventKind::PhysicalDescription => EventType::PhysicalDescription,
        EventKind::Education => EventType::Education,
        EventKind::IdNumber => EventType::NationalId,
        EventKind::Nationality => EventType::NationalOrigin,
        EventKind::ChildrenCount => EventType::ChildrenCount,
        EventKind::MarriageCount => EventType::MarriagesCount,
        EventKind::Property => EventType::Property,
        EventKind::Religion => EventType::Religion,
        EventKind::SocialSecurityNumber => EventType::SocialSecurityNumber,
        EventKind::Title => EventType::NobilityTitle,
        EventKind::Fact => EventType::Fact,
        // A generic `EVEN` carries its meaning in its free-text `TYPE`.
        EventKind::Event => event_type_from_type_text(type_text).unwrap_or(EventType::Other),
        _ => EventType::Other,
    }
}

fn convert_pedigree(st: &Store, pedi: &Pedigree) -> ChildType {
    match pedi {
        Pedigree::Birth => ChildType::Biological,
        Pedigree::Adopted => ChildType::Adopted,
        Pedigree::Foster => ChildType::Foster,
        // A step-child, in the words of the producers that say so.
        Pedigree::Unknown(t)
            if matches!(
                t.to_str(st).trim().to_ascii_lowercase().as_str(),
                "step" | "stepchild" | "step-child"
            ) =>
        {
            ChildType::Step
        }
        // A sealing, GEDCOM 7's OTHER and any other value say the child is
        // linked to the family otherwise than by birth, but not how.
        _ => ChildType::Unknown,
    }
}

/// The privacy a record's `RESN` asks for.
///
/// `confidential` (the data was marked confidential) and `privacy` (it is
/// withheld for privacy) both say the record is not for the public, which is
/// what [`Privacy::Private`] records. `locked` only forbids changes and says
/// nothing about who may read the record, so it, an unknown value and a record
/// without `RESN` follow the tree. GEDCOM 7 may list several values,
/// comma-separated, in any case.
fn privacy_from_restriction(st: &Store, resn: Option<&EnumList<Restriction>>) -> Privacy {
    let withheld = resn.is_some_and(|resn| {
        resn.0.iter().any(|value| match value {
            Restriction::Confidential | Restriction::Privacy => true,
            Restriction::Unknown(t) => {
                let t = t.to_str(st);
                let t = t.trim();
                t.eq_ignore_ascii_case("confidential") || t.eq_ignore_ascii_case("privacy")
            }
            _ => false,
        })
    });
    if withheld {
        Privacy::Private
    } else {
        Privacy::Default
    }
}

/// The confidence a `QUAY` stands for: `0` to `3` are `VeryLow`, `Low`,
/// `Medium` and `High`, and a citation without a readable `QUAY` is not
/// assessed (`None`), so an export writes back the `QUAY` it read and none
/// where there was none.
fn convert_quay(quay: Option<&Certainty>) -> Option<Confidence> {
    match quay? {
        Certainty::Unreliable => Some(Confidence::VeryLow),
        Certainty::Questionable => Some(Confidence::Low),
        Certainty::Secondary => Some(Confidence::Medium),
        Certainty::Primary => Some(Confidence::High),
        _ => None,
    }
}

/// The canonical form of an age.
///
/// `oxidged` keeps an `AGE` as written. What OxidGene reads as an age
/// (`child`, `1 y`) is held in GEDCOM's form; anything else — free text such
/// as `majeur`, an empty value, a count beyond 999 — is left out, with a
/// warning naming the record, never the value, which is the person's data. A
/// GEDCOM 7 age with a `PHRASE` is read as GEDCOM 5.5.1 writes it: a
/// duration whose phrase names the keyword it stands for as that keyword
/// (`< 8y` and `PHRASE CHILD` as `CHILD`, which a GEDZIP writes of a
/// 5.5.1 keyword), and an age known only in words (an empty `AGE` with a
/// `PHRASE`) from its phrase.
/// The family an individual event names without belonging to it: its
/// `FAMC` — the adoptive family of an `ADOP`, the family of a `BIRT` or a
/// `CHR`, the family a child is sealed to by an `SLGC`, and the `_FAMC`
/// oxidged reads back beneath an event GEDCOM gives none. A pointer to a family the file
/// does not hold is a warning, and the event names none.
fn related_family(
    event: EventView<'_>,
    ctx: &ImportContext,
    warnings: &mut Vec<String>,
) -> Option<Uuid> {
    let family = match event {
        EventView::Event(e) => e.family()?.family()?,
        EventView::Ordinance(o) if matches!(o.kind(), OrdinanceKind::ChildSealing) => o.family()?,
        _ => return None,
    };
    let family_xref = event.store().xref(family);
    let found = ctx.fam_map.get(family_xref).copied();
    if found.is_none() {
        warnings.push(format!(
            "family {family_xref} named by an individual event is not in the file; the event names no family"
        ));
    }
    found
}

fn import_age(
    age: Option<View<'_, Age>>,
    owner: EventOwner<'_>,
    warnings: &mut Vec<String>,
) -> Option<String> {
    let age = age?;
    let written = if age.phrase().is_some() {
        let st = age.store();
        let converted = age.into_owned().to_version(st, GedcomVersion::V5_5_1);
        opt_owned(st, &converted.value)
            .or_else(|| converted.phrase.as_ref().and_then(|p| opt_owned(st, p)))
    } else {
        opt(Some(age.value()))
    }
    .unwrap_or_default();
    match written.parse::<oxidgene_core::types::AgeAtEvent>() {
        Ok(age) => Some(age.to_string()),
        Err(_) => {
            warnings.push(format!(
                "{}: an AGE that is not a GEDCOM age was left out",
                owner.label()
            ));
            None
        }
    }
}

/// Reads a `TYPE` that is exactly a GEDCOM tag name, matched whole so that
/// "will" here is the tag and not the "will" inside another word.
///
/// Kept apart from the descriptive phrases [`event_type_from_type_text`] also
/// recognises, because these two kinds of `TYPE` deserve opposite treatment as
/// a description: see [`type_text_restates_event_type`]. `t` is expected
/// trimmed and lowercased.
fn event_type_from_gedcom_tag(t: &str) -> Option<EventType> {
    match t {
        "adop" => Some(EventType::Adoption),
        "anul" => Some(EventType::Annulment),
        "birt" => Some(EventType::Birth),
        "bles" => Some(EventType::Blessing),
        "chr" => Some(EventType::Christening),
        "deat" => Some(EventType::Death),
        "div" => Some(EventType::Divorce),
        "divf" => Some(EventType::DivorceFiled),
        "enga" => Some(EventType::Engagement),
        "marb" => Some(EventType::MarriageBann),
        "marc" => Some(EventType::MarriageContract),
        "marl" => Some(EventType::MarriageLicense),
        "marr" => Some(EventType::Marriage),
        "mars" => Some(EventType::MarriageSettlement),
        "ordn" => Some(EventType::Ordination),
        "bapm" => Some(EventType::Baptism),
        "barm" => Some(EventType::BarBatMitzvah),
        "basm" => Some(EventType::BasMitzvah),
        "buri" => Some(EventType::Burial),
        "cast" => Some(EventType::CasteName),
        "cens" => Some(EventType::Census),
        "chra" => Some(EventType::AdultChristening),
        "conf" => Some(EventType::Confirmation),
        "crem" => Some(EventType::Cremation),
        "dscr" => Some(EventType::PhysicalDescription),
        "educ" => Some(EventType::Education),
        "emig" => Some(EventType::Emigration),
        "fcom" => Some(EventType::FirstCommunion),
        "grad" => Some(EventType::Graduation),
        "idno" => Some(EventType::NationalId),
        "immi" => Some(EventType::Immigration),
        "mili" => Some(EventType::MilitaryService),
        "nati" => Some(EventType::NationalOrigin),
        "natu" => Some(EventType::Naturalization),
        "nchi" => Some(EventType::ChildrenCount),
        "nmr" => Some(EventType::MarriagesCount),
        "occu" => Some(EventType::Occupation),
        "prob" => Some(EventType::Probate),
        "prop" => Some(EventType::Property),
        "reli" => Some(EventType::Religion),
        "resi" => Some(EventType::Residence),
        "reti" => Some(EventType::Retirement),
        "ssn" => Some(EventType::SocialSecurityNumber),
        "titl" => Some(EventType::NobilityTitle),
        "will" => Some(EventType::Will),
        _ => None,
    }
}

/// A `TYPE` that is nothing but a name of an event type, in whichever language
/// the exporter wrote it. Matched whole: these are fixed vocabularies, so
/// recognising them by substring would only add false hits, and a `TYPE` that
/// merely *contains* one ("Military service in Algeria") says more than the
/// name alone and is not one of these.
///
/// Two vocabularies feed it. `GENEWEB_LABELS` are the labels the `geneweb`
/// crate writes for events GEDCOM cannot express (see its
/// `src/gedcom/event.rs`); `RESTATED_TAGS` are the ordinary GEDCOM tags an
/// exporter chose to spell out in words instead of using.
fn type_name_phrase(t: &str) -> Option<EventType> {
    const GENEWEB_LABELS: &[(&str, EventType)] = &[
        ("accomplishment", EventType::Accomplishment),
        ("acquisition", EventType::Acquisition),
        ("membership", EventType::Membership),
        ("change name", EventType::ChangeName),
        ("circumcision", EventType::Circumcision),
        ("award", EventType::Award),
        ("military discharge", EventType::MilitaryDischarge),
        ("degree", EventType::Degree),
        ("distinction", EventType::Distinction),
        ("election", EventType::Election),
        ("excommunication", EventType::Excommunication),
        ("funeral", EventType::Funeral),
        ("hospitalization", EventType::Hospitalization),
        ("illness", EventType::Illness),
        ("passenger list", EventType::PassengerList),
        ("military distinction", EventType::MilitaryDistinction),
        ("military promotion", EventType::MilitaryPromotion),
        ("military mobilization", EventType::MilitaryMobilization),
        ("property sale", EventType::PropertySale),
        ("endl", EventType::Endowment),
        ("dotationlds", EventType::LdsDotation),
        ("slgc", EventType::SealingChild),
        ("slgs", EventType::SealingSpouse),
        ("scellent parent lds", EventType::SealingParent),
        ("family link lds", EventType::FamilyLinkLds),
        ("bapl", EventType::LdsBaptism),
        ("conl", EventType::LdsConfirmation),
        ("inil", EventType::LdsInitiatory),
        ("unmarried", EventType::NoMarriage),
        ("nomen", EventType::NoMention),
    ];
    // Deliberately absent: the civil-union wordings. "PACS", "Concubinage"
    // and "Cohabitation" all resolve to `CivilUnion`, so there the phrase is
    // the only thing telling one union from another and must survive as the
    // description.
    const RESTATED_TAGS: &[(&str, EventType)] = &[
        ("military service", EventType::MilitaryService),
        ("service militaire", EventType::MilitaryService),
        ("physical description", EventType::PhysicalDescription),
        ("description physique", EventType::PhysicalDescription),
        ("national origin", EventType::NationalOrigin),
        ("origine nationale", EventType::NationalOrigin),
        ("nationality", EventType::NationalOrigin),
        ("nationalité", EventType::NationalOrigin),
        ("national id", EventType::NationalId),
        ("identity number", EventType::NationalId),
        ("social security number", EventType::SocialSecurityNumber),
        (
            "numéro de sécurité sociale",
            EventType::SocialSecurityNumber,
        ),
        ("number of children", EventType::ChildrenCount),
        ("nombre d'enfants", EventType::ChildrenCount),
        ("number of marriages", EventType::MarriagesCount),
        ("nombre de mariages", EventType::MarriagesCount),
        ("nobility title", EventType::NobilityTitle),
        ("titre de noblesse", EventType::NobilityTitle),
        ("first communion", EventType::FirstCommunion),
        ("première communion", EventType::FirstCommunion),
        ("premiere communion", EventType::FirstCommunion),
        ("bar mitzvah", EventType::BarBatMitzvah),
        ("bat mitzvah", EventType::BasMitzvah),
        ("confirmation", EventType::Confirmation),
        ("naturalization", EventType::Naturalization),
        ("naturalisation", EventType::Naturalization),
        ("immigration", EventType::Immigration),
        ("emigration", EventType::Emigration),
        ("émigration", EventType::Emigration),
        ("graduation", EventType::Graduation),
        ("diplôme", EventType::Graduation),
        ("diplome", EventType::Graduation),
        ("occupation", EventType::Occupation),
        ("profession", EventType::Occupation),
        ("métier", EventType::Occupation),
        ("residence", EventType::Residence),
        ("résidence", EventType::Residence),
        ("domicile", EventType::Residence),
        ("retirement", EventType::Retirement),
        ("retraite", EventType::Retirement),
        ("property", EventType::Property),
        ("propriété", EventType::Property),
        ("possessions", EventType::Property),
        ("religion", EventType::Religion),
        ("education", EventType::Education),
        ("éducation", EventType::Education),
        ("caste", EventType::CasteName),
        ("caste name", EventType::CasteName),
        ("census", EventType::Census),
        ("recensement", EventType::Census),
        ("baptism", EventType::Baptism),
        ("baptême", EventType::Baptism),
        ("bapteme", EventType::Baptism),
        ("burial", EventType::Burial),
        ("inhumation", EventType::Burial),
        ("enterrement", EventType::Burial),
        ("cremation", EventType::Cremation),
        ("crémation", EventType::Cremation),
        ("probate", EventType::Probate),
        ("homologation", EventType::Probate),
        ("last will", EventType::Will),
        ("testament", EventType::Will),
        ("adoption", EventType::Adoption),
        ("separation", EventType::Separation),
        ("séparation", EventType::Separation),
    ];
    GENEWEB_LABELS
        .iter()
        .chain(RESTATED_TAGS)
        .find(|(l, _)| *l == t)
        .map(|(_, et)| *et)
}

/// Whether a `TYPE` says nothing the resolved [`EventType`] does not already
/// say, and so should not also become the event's description.
///
/// Two shapes say nothing. A bare GEDCOM tag name — `2 TYPE EDUC` under an
/// event already typed `Education` is the same fact twice, in a spelling
/// nobody typed, which surfaced as a profession labelled "OCCU" in the person
/// form and as `1 EDUC EDUC` on the way back out. And the type's own name
/// spelled out — a `Military service` description beside a badge that already
/// reads « Service militaire » repeats it, and repeats it in the exporter's
/// language rather than the reader's.
///
/// A `TYPE` that says more is still kept: "PACS" and "Concubinage" both arrive
/// as `CivilUnion` and only the description tells them apart, and "Military
/// service in Algeria" carries a fact the type does not.
fn type_text_restates_event_type(type_text: &str, event_type: EventType) -> bool {
    let t = type_text.trim().to_lowercase();
    event_type_from_gedcom_tag(&t) == Some(event_type)
        || type_name_phrase(&t) == Some(event_type)
        || crate::export::even_type_label(event_type).is_some_and(|label| label.to_lowercase() == t)
}

/// Whether a generic `EVEN` typed `type_text` reads back as `event_type`,
/// with `type_text` as its description — what the export asks before
/// writing a description as the `TYPE` itself.
pub(crate) fn type_text_reads_as(type_text: &str, event_type: EventType) -> bool {
    event_type_from_type_text(Some(type_text)).unwrap_or(EventType::Other) == event_type
        && !type_text_restates_event_type(type_text, event_type)
}

/// Reads a generic `EVEN`'s free-text `TYPE` sub-tag as an [`EventType`].
///
/// Exporters record anything GEDCOM has no tag for as `EVEN` plus a `TYPE`
/// describing it — a civil union, but also plain restatements of tags they
/// chose not to use ("PROP", "Military service"). Left unread, all of it
/// imported as [`EventType::Other`], so a whole shelf of events showed up
/// under one meaningless label.
///
/// The `TYPE` text is kept as the event's description only when it carries
/// more than the type does — see [`type_text_restates_event_type`].
fn event_type_from_type_text(type_text: Option<&str>) -> Option<EventType> {
    let t = type_text?.trim().to_lowercase();
    if t.is_empty() {
        return None;
    }

    if let Some(et) = event_type_from_gedcom_tag(&t) {
        return Some(et);
    }
    if let Some(et) = type_name_phrase(&t) {
        return Some(et);
    }

    // Otherwise a descriptive phrase, in English or French. Order matters: the
    // first match wins, so anything that is a substring of another entry comes
    // after it. Only phrases specific enough to be unambiguous appear — a bare
    // "title" or "will" would swallow "job title" and "Williams".
    const KEYWORDS: &[(&str, EventType)] = &[
        // Unmarried partnerships: GEDCOM has no tag, so this is always EVEN.
        ("civil union", EventType::CivilUnion),
        ("civil partnership", EventType::CivilUnion),
        ("domestic partnership", EventType::CivilUnion),
        ("registered partnership", EventType::CivilUnion),
        ("cohabitation", EventType::CivilUnion),
        ("common-law", EventType::CivilUnion),
        ("common law", EventType::CivilUnion),
        ("pacs", EventType::CivilUnion),
        ("union libre", EventType::CivilUnion),
        ("concubinage", EventType::CivilUnion),
        // Tags an exporter restated in words.
        ("military", EventType::MilitaryService),
        ("service militaire", EventType::MilitaryService),
        ("physical description", EventType::PhysicalDescription),
        ("description physique", EventType::PhysicalDescription),
        ("national origin", EventType::NationalOrigin),
        ("origine nationale", EventType::NationalOrigin),
        ("nationalit", EventType::NationalOrigin),
        ("national id", EventType::NationalId),
        ("identity number", EventType::NationalId),
        ("social security", EventType::SocialSecurityNumber),
        ("sécurité sociale", EventType::SocialSecurityNumber),
        ("securite sociale", EventType::SocialSecurityNumber),
        ("number of children", EventType::ChildrenCount),
        ("nombre d'enfants", EventType::ChildrenCount),
        ("number of marriages", EventType::MarriagesCount),
        ("nombre de mariages", EventType::MarriagesCount),
        ("nobility", EventType::NobilityTitle),
        ("noblesse", EventType::NobilityTitle),
        ("first communion", EventType::FirstCommunion),
        ("première communion", EventType::FirstCommunion),
        ("premiere communion", EventType::FirstCommunion),
        ("bar mitzvah", EventType::BarBatMitzvah),
        ("bat mitzvah", EventType::BasMitzvah),
        ("confirmation", EventType::Confirmation),
        ("naturalisation", EventType::Naturalization),
        ("naturalization", EventType::Naturalization),
        ("immigration", EventType::Immigration),
        ("emigration", EventType::Emigration),
        ("émigration", EventType::Emigration),
        ("graduation", EventType::Graduation),
        ("diplôme", EventType::Graduation),
        ("diplome", EventType::Graduation),
        ("occupation", EventType::Occupation),
        ("profession", EventType::Occupation),
        ("métier", EventType::Occupation),
        ("residence", EventType::Residence),
        ("résidence", EventType::Residence),
        ("domicile", EventType::Residence),
        ("retirement", EventType::Retirement),
        ("retraite", EventType::Retirement),
        ("property", EventType::Property),
        ("possessions", EventType::Property),
        ("propriété", EventType::Property),
        ("religion", EventType::Religion),
        ("religious", EventType::Religion),
        ("education", EventType::Education),
        ("éducation", EventType::Education),
        ("scholastic", EventType::Education),
        ("caste", EventType::CasteName),
        ("census", EventType::Census),
        ("recensement", EventType::Census),
        ("baptism", EventType::Baptism),
        ("baptême", EventType::Baptism),
        ("bapteme", EventType::Baptism),
        ("burial", EventType::Burial),
        ("inhumation", EventType::Burial),
        ("enterrement", EventType::Burial),
        ("cremation", EventType::Cremation),
        ("crémation", EventType::Cremation),
        ("incinération", EventType::Cremation),
        ("probate", EventType::Probate),
        ("homologation", EventType::Probate),
        ("last will", EventType::Will),
        ("testament", EventType::Will),
        ("adoption", EventType::Adoption),
    ];
    KEYWORDS
        .iter()
        .find(|(k, _)| t.contains(k))
        .map(|(_, et)| *et)
}

/// An event's description: the `TYPE` unless it only restates the event
/// type, and the line value of a generic `EVEN` — GEDCOM's event descriptor,
/// as in `1 EVEN Appointed chairperson` / `2 TYPE Civic appointment`. When
/// the file gives both, the description keeps both.
///
/// Only an `EVEN` line and an attribute's (a family's `NCHI 3`, a `PROP`
/// some software writes on a family) describe the event: on an event's own
/// tag the value is the `Y` asserting that it happened.
fn event_description(event: EventView<'_>, event_type: EventType) -> Option<String> {
    let type_text = event
        .classification()
        .filter(|t| !type_text_restates_event_type(t, event_type));
    let kind = event.kind();
    let descriptor = non_blank_str(Some(&event.value()))
        .filter(|_| kind == EventKind::Event || is_individual_attribute(&kind));
    match (type_text, descriptor) {
        (Some(type_text), Some(descriptor)) => Some(format!("{}: {descriptor}", type_text.trim())),
        (type_text, descriptor) => descriptor.or(type_text),
    }
}

/// The date of an event split into the domain's columns (see `crate::date`).
/// A GEDCOM 7 date with a `PHRASE` is read as GEDCOM 5.5.1 writes it: a date
/// known only in words — an empty `DATE` with a `PHRASE` — as `(the
/// phrase)`, and a single date with one as the interpreted date `INT date
/// (the phrase)`, which a GEDZIP writes of a 5.5.1 one.
fn event_date(date: Option<View<'_, Date>>) -> crate::date::ImportedDate {
    let Some(date) = date else {
        return crate::date::ImportedDate::default();
    };
    let (value, phrase) = if date.phrase().is_some() {
        let st = date.store();
        let converted = date.into_owned().to_version(st, GedcomVersion::V5_5_1);
        let phrase = converted.detail().phrase.as_ref();
        (
            opt_owned(st, &converted.value),
            phrase.and_then(|p| opt_owned(st, p)),
        )
    } else {
        (opt(Some(date.value())), None)
    };
    match (value, phrase) {
        (Some(value), _) => crate::date::parse(&value),
        (None, Some(phrase)) => crate::date::parse(&format!("({})", phrase.trim())),
        (None, None) => crate::date::ImportedDate::default(),
    }
}

/// The record a citation is attached to.
#[derive(Clone, Copy)]
struct CitationOwner {
    person_id: Option<Uuid>,
    event_id: Option<Uuid>,
    family_id: Option<Uuid>,
}

fn import_event_detail(
    event: EventView<'_>,
    owner: EventOwner<'_>,
    ctx: &ImportContext,
    lookups: &mut Lookups<'_>,
    result: &mut ImportResult,
) {
    let EventOwner {
        person_id,
        family_id,
        ..
    } = owner;
    let type_text = event.classification();
    let event_type = convert_event_type(&event.kind(), type_text.as_deref());
    let date = event_date(event.date());
    let place_id = import_place(event.place(), lookups.place, result);
    let cause = death_cause(event_type, event);

    // The GEDCOM `TYPE` sub-tag classifies a generic `EVEN`/`FACT` event
    // (e.g. "PACS", "Concubinage") — preserve it as the description so the
    // original wording survives even when several TYPE values map to the
    // same EventType. Unless it only restates the type it was just read as,
    // which is not wording anyone chose.
    let description = event_description(event, event_type);

    // A family the individual event names without belonging to it (the
    // adoptive family, the family a child is sealed to). Never `family_id`,
    // which makes an event a family's own.
    let related_family_id = match owner.family_id {
        None => related_family(event, ctx, &mut result.warnings),
        Some(_) => None,
    };

    let event_id = Uuid::now_v7();
    // A family event states each spouse's age instead (`HUSB.AGE`).
    let age = import_age(event.age(), owner, &mut result.warnings).filter(|_| family_id.is_none());
    result.events.push(Event {
        id: event_id,
        tree_id: ctx.tree_id,
        event_type,
        date_value: date.value,
        date_sort: date.sort,
        date_qualifier: date.qualifier,
        date_value2: date.value2,
        calendar: date.calendar,
        cause,
        age,
        agency: non_blank(event.agency()),
        classification: None,
        spouse_ages: Vec::new(),
        place_id,
        person_id,
        family_id,
        related_family_id,
        description,
        created_at: ctx.now,
        updated_at: ctx.now,
        deleted_at: None,
    });

    // Associations (witnesses, godparents, ...) attached to this event —
    // only associations pointing at a known INDI xref can be captured
    // (an `ASSO` may point at a FAM record per the GEDCOM grammar, which
    // has no home in `EventWitness`). GEDCOM 5.5.1 has no `ASSO` beneath an
    // event: a conformant writer keeps one as `_ASSO`, which is read too.
    if let (Some(typed), Some(extra)) = (event.associations(), event.extra()) {
        // A person is one witness of an event per relation: two `ASSO` naming
        // the same person with the same relation (compared trimmed and without
        // case) are one witness, and with different relations they are two,
        // as a godparent who is also a witness of the same baptism.
        let mut witnesses: std::collections::HashSet<(Uuid, String)> =
            std::collections::HashSet::new();
        for (target, relation) in associations_of(event.store(), typed, extra) {
            if let Some(&witness_person_id) = ctx.indi_map.get(target)
                && witnesses.insert((witness_person_id, relation_key(relation.as_deref())))
            {
                result.event_witnesses.push(EventWitness {
                    id: Uuid::now_v7(),
                    event_id,
                    person_id: witness_person_id,
                    relation,
                    sort_order: witnesses.len() as i32 - 1,
                });
            }
        }
    }

    import_event_parts(event, event_id, family_id, ctx, lookups, result);
}

/// What an event and an attribute import alike: their citations, media and
/// notes.
fn import_event_parts(
    event: EventView<'_>,
    event_id: Uuid,
    family_id: Option<Uuid>,
    ctx: &ImportContext,
    lookups: &mut Lookups<'_>,
    result: &mut ImportResult,
) {
    for cite in event.citations() {
        import_citation(
            cite,
            CitationOwner {
                person_id: None,
                event_id: Some(event_id),
                family_id,
            },
            &ctx.source_map,
            lookups.text_source,
            result,
        );
    }
    if let Some(multimedia) = event.multimedia() {
        import_media_links(multimedia, LinkOwner::Event(event_id), ctx, result);
    }
    ctx.import_notes(event.notes(), NoteOwner::Event(event_id), result);
}

/// The extension the `geneweb` crate writes beneath a death, holding how
/// the person died: `killed`, `murdered`, `executed`, `disappeared`, `died
/// young` or `presumed dead` (`2 _GWDEATH killed`).
const GENEWEB_DEATH_REASON_TAG: &str = "_GWDEATH";

/// An event's cause: the one the file states, then, on a death, the reason
/// a GeneWeb file gives for it, GEDCOM's word for which is `CAUS` too.
fn death_cause(event_type: EventType, event: EventView<'_>) -> Option<String> {
    let cause = opt(event.cause());
    if event_type != EventType::Death {
        return cause;
    }
    let reasons: Vec<String> = event
        .extra()
        .into_iter()
        .flat_map(|extra| tagged(extra, GENEWEB_DEATH_REASON_TAG))
        .filter_map(|tag| non_blank_str(node_text(tag).as_deref()))
        .collect();
    if reasons.is_empty() {
        return cause;
    }
    let reason = reasons.join("; ");
    Some(match cause.filter(|cause| !cause.trim().is_empty()) {
        Some(cause) => format!("{cause}; {reason}"),
        None => reason,
    })
}

/// Imports a GEDCOM individual attribute (OCCU, RESI, TITL, ...) as one or
/// more `Event`s. Mirrors `import_event_detail`, adapted to what an attribute
/// says: the tag's own value (e.g. "Presales" for OCCU) or its TYPE sub-tag
/// is preserved as the description. OCCU is special-cased: its value is
/// split on common separators and case-normalized into one Occupation event
/// per profession (see `split_occupations`/`normalize_occupation_case`).
fn import_attribute_detail(
    attribute: EventView<'_>,
    owner: EventOwner<'_>,
    ctx: &ImportContext,
    lookups: &mut Lookups<'_>,
    result: &mut ImportResult,
) {
    let event_type = convert_event_type(&attribute.kind(), None);
    let date = event_date(attribute.date());
    let place_id = import_place(attribute.place(), lookups.place, result);
    let cause = opt(attribute.cause());
    let age = import_age(attribute.age(), owner, &mut result.warnings);
    let agency = non_blank(attribute.agency());

    // Preserve the tag's own value (e.g. "Acccount Manager", "Presales, Trainer"
    // for OCCU) or, failing that, its TYPE sub-tag — but not either of them
    // when all it says is the name of the tag it sits under.
    let value = Some(attribute.value())
        .filter(|value| !value.is_empty())
        .map(Cow::into_owned);
    // Beside a value, the TYPE says what the value is (`FACT Sample fact` /
    // `TYPE Hobby`): it is kept as the classification, so it survives an
    // export. Without a value it is the description itself, as above.
    let classification = value
        .as_ref()
        .and_then(|_| attribute.classification())
        .filter(|t| !type_text_restates_event_type(t, event_type));
    let description = value
        .or_else(|| attribute.classification())
        .filter(|t| !type_text_restates_event_type(t, event_type));

    // Some exporters (e.g. Geneanet) pack several professions into a single
    // OCCU value ("Presales, Trainer") because they only support one
    // profession field. Split those into distinct Occupation events so each
    // can be edited and sourced independently, and uppercase each one's
    // first letter (some exporters write them all lowercase) — the rest of
    // the string is left as written. Every other attribute tag keeps its
    // raw value verbatim.
    let descriptions: Vec<Option<String>> = match &description {
        Some(text) if event_type == EventType::Occupation => split_occupations(text)
            .into_iter()
            .map(|p| Some(normalize_occupation_case(&p)))
            .collect(),
        _ => vec![description],
    };

    for description in descriptions {
        let event_id = Uuid::now_v7();
        result.events.push(Event {
            id: event_id,
            tree_id: ctx.tree_id,
            event_type,
            date_value: date.value.clone(),
            date_sort: date.sort,
            date_qualifier: date.qualifier,
            date_value2: date.value2.clone(),
            calendar: date.calendar,
            cause: cause.clone(),
            age: age.clone(),
            agency: agency.clone(),
            classification: classification.clone(),
            spouse_ages: Vec::new(),
            place_id,
            person_id: owner.person_id,
            family_id: owner.family_id,
            related_family_id: None,
            description,
            created_at: ctx.now,
            updated_at: ctx.now,
            deleted_at: None,
        });

        // Its citations, and its media — the trade card behind an OCCU, the
        // deed behind a TITL. A value that split into several professions
        // gives each of them the scan, the same way each gets the citations:
        // one line documented them all.
        import_event_parts(attribute, event_id, owner.family_id, ctx, lookups, result);
    }
}

/// The place a `PLAC` names, created on first use, with the coordinates of
/// its `MAP` when it has them.
fn import_place(
    plac: Option<View<'_, GedPlace>>,
    get_or_create_place: &mut dyn FnMut(&str, &mut ImportResult) -> Uuid,
    result: &mut ImportResult,
) -> Option<Uuid> {
    let plac = plac?;
    let name = opt(Some(plac.name()))?;
    let pid = get_or_create_place(&name, result);
    if let Some(map) = plac.map()
        && let (Some(lat_str), Some(lon_str)) = (map.latitude(), map.longitude())
        && let (Ok(lat), Ok(lon)) = (
            parse_gedcom_coord(&lat_str.to_str()),
            parse_gedcom_coord(&lon_str.to_str()),
        )
        && let Some(place) = result.places.iter_mut().find(|pl| pl.id == pid)
    {
        place.latitude = Some(lat);
        place.longitude = Some(lon);
    }
    Some(pid)
}

/// Splits a free-text value on common list separators, trimming whitespace
/// and dropping empty segments. Used to break up multi-profession OCCU
/// values from exporters that only support a single profession field.
fn split_occupations(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// Normalizes a profession's case to "first letter upper, rest as it was written"
fn normalize_occupation_case(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn import_citation(
    cite: View<'_, GedCitation>,
    owner: CitationOwner,
    source_map: &HashMap<String, Uuid>,
    get_or_create_text_source: &mut dyn FnMut(&str, &mut ImportResult) -> Uuid,
    result: &mut ImportResult,
) {
    let st = cite.store();
    let source_id = match cite.source() {
        CitationSource::Pointer(pointer) => {
            let xref = st.xref(pointer);
            match source_map.get(xref) {
                Some(&id) => id,
                None => {
                    result
                        .warnings
                        .push(format!("Citation references unknown source {xref}"));
                    return;
                }
            }
        }
        CitationSource::Description(text) if text.is_empty() => {
            result
                .warnings
                .push("Skipping citation without source xref".into());
            return;
        }
        CitationSource::Description(description) => {
            get_or_create_text_source(&description.to_str(st), result)
        }
    };

    // The transcript: the first `DATA.TEXT`, else the first `TEXT` of the
    // citation itself (5.5.1's text of a described source, which the import
    // gives a source record of its own, where a pointer cannot carry it). A
    // citation may hold several `TEXT` lines; the others are kept GEDCOM.
    let text = cite
        .data()
        .into_iter()
        .flat_map(|v| v.texts())
        .chain(cite.texts())
        .find_map(|source_text| opt(Some(source_text.text())));

    result.citations.push(Citation {
        id: Uuid::now_v7(),
        source_id,
        person_id: owner.person_id,
        event_id: owner.event_id,
        family_id: owner.family_id,
        page: opt(cite.page()),
        confidence: convert_quay(cite.quality().as_ref()),
        text,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    });
}

/// What an `OBJE` record says of its media: the text of its notes, one after
/// the other.
fn media_description(
    notes: Items<'_, GedNote>,
    ctx: &ImportContext,
    result: &mut ImportResult,
) -> Option<String> {
    let texts = ctx.note_texts(notes, &mut result.warnings);
    (!texts.is_empty()).then(|| texts.join("\n"))
}

/// Resolve a multimedia link to a `Media` UUID.
///
/// If the link points at a top-level OBJE record, return its document UUID.
/// Otherwise, if it holds its files itself (a GEDCOM 5.5.1 link without a
/// record), create a document and its page and return the document UUID.
/// Returns `None` if neither applies.
fn resolve_or_create_media(
    mm: View<'_, MultimediaLink>,
    ctx: &ImportContext,
    result: &mut ImportResult,
) -> Option<Uuid> {
    // Case 1: cross-reference to a top-level OBJE record
    if let Some(object) = mm.object()
        && let Some(&media_id) = ctx.media_map.get(mm.store().xref(object))
    {
        return Some(media_id);
    }

    // Case 2: inline multimedia with file data, read the same way as a
    // top-level OBJE: see `media_file`.
    let (file_path, mime_type, source_media_type) = media_file(mm.files());
    if file_path.is_empty() {
        return None;
    }
    let file_name = file_name_of(&file_path);
    let title = opt(mm.title()).or_else(|| media_title(mm.files()));
    // GEDCOM 5.5 gave a link without a record notes of its own, which 5.5.1
    // and 7.0 do not: they are kept aside, and read.
    let notes: Vec<String> = tagged(mm.extra(), "NOTE")
        .filter_map(|note| non_blank_str(node_text(note).as_deref()))
        .collect();
    let description = (!notes.is_empty()).then(|| notes.join("\n"));
    let document_id = Uuid::now_v7();
    let page = media_page(
        ctx,
        Uuid::now_v7(),
        document_id,
        file_name,
        mime_type,
        file_path,
        source_media_type,
    );
    result.media.push(Media {
        id: document_id,
        file_name: title.clone().unwrap_or_else(|| page.file_name.clone()),
        mime_type: DOCUMENT_MIME.to_string(),
        file_path: String::new(),
        parent_media_id: None,
        title,
        description,
        ..page.clone()
    });
    result.media.push(page);
    Some(document_id)
}

/// Parse a GEDCOM coordinate string (e.g. `"N01.4242"` or `"W1.4242"`).
fn parse_gedcom_coord(s: &str) -> Result<f64, std::num::ParseFloatError> {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix('N').or_else(|| s.strip_prefix('E')) {
        rest.parse::<f64>()
    } else if let Some(rest) = s.strip_prefix('S').or_else(|| s.strip_prefix('W')) {
        rest.parse::<f64>().map(|v| -v)
    } else {
        s.parse::<f64>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_name_value_full() {
        let (g, s) = parse_name_value(Some("John /DOE/"));
        assert_eq!(g, Some("John".to_string()));
        assert_eq!(s, Some("DOE".to_string()));
    }

    #[test]
    fn test_parse_name_value_multiple_given() {
        let (g, s) = parse_name_value(Some("John Mickael Louis /DOE/"));
        assert_eq!(g, Some("John Mickael Louis".to_string()));
        assert_eq!(s, Some("DOE".to_string()));
    }

    #[test]
    fn test_parse_name_value_surname_only() {
        let (g, s) = parse_name_value(Some("/Doe/"));
        assert_eq!(g, None);
        assert_eq!(s, Some("Doe".to_string()));
    }

    #[test]
    fn test_parse_name_value_no_slashes() {
        let (g, s) = parse_name_value(Some("John"));
        assert_eq!(g, None);
        assert_eq!(s, None);
    }

    #[test]
    fn test_parse_name_value_none() {
        let (g, s) = parse_name_value(None);
        assert_eq!(g, None);
        assert_eq!(s, None);
    }

    #[test]
    fn test_parse_name_value_empty_surname() {
        let (g, s) = parse_name_value(Some("John //"));
        assert_eq!(g, Some("John".to_string()));
        assert_eq!(s, None);
    }
}

#[cfg(test)]
mod type_text_description_tests {
    use super::*;

    #[test]
    fn a_bare_tag_name_is_not_kept_as_a_description() {
        // `geneweb` emits every event GEDCOM cannot express as `EVEN` + a
        // `TYPE` naming the tag it would have used. Read back, the type is
        // already `Education`, so keeping "EDUC" beside it said nothing and
        // showed up in the form as a bare "EDUC".
        assert!(type_text_restates_event_type("EDUC", EventType::Education));
        assert!(type_text_restates_event_type("OCCU", EventType::Occupation));
        // Case and surrounding space are the exporter's business, not a
        // difference in meaning.
        assert!(type_text_restates_event_type(
            "  prop ",
            EventType::Property
        ));
    }

    #[test]
    fn a_descriptive_type_is_kept() {
        // Several of these collapse onto one EventType, so the description is
        // the only thing telling them apart.
        assert!(!type_text_restates_event_type(
            "PACS",
            EventType::CivilUnion
        ));
        assert!(!type_text_restates_event_type(
            "Concubinage",
            EventType::CivilUnion
        ));
        // A phrase that says more than the type does.
        assert!(!type_text_restates_event_type(
            "Military service in Algeria",
            EventType::MilitaryService
        ));
        // A user-defined GeneWeb event name is the whole point of the field.
        assert!(!type_text_restates_event_type(
            "Succession",
            EventType::Other
        ));
    }

    /// A `TYPE` spelling out the very type it resolved to is the badge again,
    /// in the exporter's language — which is how a French tree ended up with
    /// an event badged « Service militaire » described "Military service".
    #[test]
    fn a_type_spelled_out_in_words_is_not_kept_as_a_description() {
        for (text, event_type) in [
            ("Military service", EventType::MilitaryService),
            ("service militaire", EventType::MilitaryService),
            ("  Occupation  ", EventType::Occupation),
            ("Recensement", EventType::Census),
            // Now that a sale has its own type, the phrase adds nothing.
            ("Property sale", EventType::PropertySale),
            ("Funeral", EventType::Funeral),
        ] {
            assert!(
                type_text_restates_event_type(text, event_type),
                "for {text}"
            );
        }
    }

    /// The rule is "names *this* type", not "names some type".
    #[test]
    fn a_phrase_naming_another_type_is_kept() {
        assert!(!type_text_restates_event_type(
            "Military service",
            EventType::Occupation
        ));
    }

    #[test]
    fn a_tag_naming_some_other_type_is_kept() {
        // The rule is "says what this event already is", not "looks like a
        // tag" — an OCCU sitting on a Residence still carries information.
        assert!(!type_text_restates_event_type("OCCU", EventType::Residence));
    }

    #[test]
    fn every_recognised_tag_round_trips_through_the_full_reader() {
        // `event_type_from_type_text` must keep answering for bare tags now
        // that the table lives in its own function.
        for (text, expected) in [
            ("EDUC", EventType::Education),
            ("occu", EventType::Occupation),
            ("WILL", EventType::Will),
            ("nmr", EventType::MarriagesCount),
        ] {
            assert_eq!(
                event_type_from_type_text(Some(text)),
                Some(expected),
                "for {text}"
            );
        }
    }
}

#[cfg(test)]
mod geneweb_death_reason_tests {
    use super::*;

    /// The cause `death_cause` gives an event of `event_type` read by
    /// `oxidged` as a death with this cause and these `_GWDEATH` reasons
    /// beneath it.
    fn cause_of(event_type: EventType, cause: Option<&str>, reasons: &[&str]) -> Option<String> {
        let mut gedcom = String::from("0 HEAD\n0 @I1@ INDI\n1 DEAT\n");
        if let Some(cause) = cause {
            gedcom.push_str(&format!("2 CAUS {cause}\n"));
        }
        for reason in reasons {
            gedcom.push_str(&format!("2 _GWDEATH {reason}\n"));
        }
        gedcom.push_str("0 TRLR\n");
        let data = Dataset::parse(gedcom);
        let death = data.individuals().first().and_then(|i| i.events().first());
        death_cause(event_type, EventView::Event(death.expect("a death")))
    }

    #[test]
    fn the_death_reason_becomes_the_cause() {
        assert_eq!(
            cause_of(EventType::Death, None, &["killed"]),
            Some("killed".into())
        );
        assert_eq!(
            cause_of(EventType::Death, Some("Fever"), &["died young"]),
            Some("Fever; died young".into())
        );
    }

    #[test]
    fn a_death_without_a_reason_and_other_events_keep_their_cause() {
        assert_eq!(
            cause_of(EventType::Death, Some("Fever"), &[]),
            Some("Fever".into())
        );
        assert_eq!(cause_of(EventType::Burial, None, &["killed"]), None);
    }
}

#[cfg(test)]
mod event_type_text_tests {
    use super::*;

    #[test]
    fn a_bare_gedcom_tag_names_its_own_type() {
        for (text, expected) in [
            ("PROP", EventType::Property),
            ("prop", EventType::Property),
            ("  MILI  ", EventType::MilitaryService),
            ("OCCU", EventType::Occupation),
            ("TITL", EventType::NobilityTitle),
            ("WILL", EventType::Will),
        ] {
            assert_eq!(
                event_type_from_type_text(Some(text)),
                Some(expected),
                "for {text}"
            );
        }
    }

    #[test]
    fn a_described_type_is_recognised_in_either_language() {
        for (text, expected) in [
            ("Military service", EventType::MilitaryService),
            ("Service militaire", EventType::MilitaryService),
            // Its own type now that GeneWeb's vocabulary is modelled — a sale
            // is not merely a possession.
            ("Property sale", EventType::PropertySale),
            ("Vente de propriété", EventType::Property),
            ("Recensement", EventType::Census),
            ("Nombre d'enfants", EventType::ChildrenCount),
        ] {
            assert_eq!(
                event_type_from_type_text(Some(text)),
                Some(expected),
                "for {text}"
            );
        }
    }

    /// The behaviour this table replaced, kept working.
    #[test]
    fn civil_unions_still_win_over_everything_else() {
        for text in ["PACS", "Union libre", "Common law marriage", "Cohabitation"] {
            assert_eq!(
                event_type_from_type_text(Some(text)),
                Some(EventType::CivilUnion),
                "for {text}"
            );
        }
    }

    /// Guessing wrong is worse than not guessing: an unrecognised type must
    /// stay `Other` rather than be forced into a neighbouring meaning.
    #[test]
    fn an_unrecognised_type_is_not_guessed() {
        for text in ["", "   ", "Bought a horse", "Sold the farm", "Divers"] {
            assert_eq!(event_type_from_type_text(Some(text)), None, "for {text}");
        }
        assert_eq!(event_type_from_type_text(None), None);
    }

    /// Every label the `geneweb` crate emits must come back as its own type:
    /// this is the join between the two crates, and a typo on either side
    /// silently lands the event in `Other`.
    #[test]
    fn every_geneweb_label_is_recognised() {
        for (label, expected) in [
            ("Accomplishment", EventType::Accomplishment),
            ("Acquisition", EventType::Acquisition),
            ("Membership", EventType::Membership),
            ("Change name", EventType::ChangeName),
            ("Circumcision", EventType::Circumcision),
            ("Award", EventType::Award),
            ("Military discharge", EventType::MilitaryDischarge),
            ("Degree", EventType::Degree),
            ("Distinction", EventType::Distinction),
            ("Election", EventType::Election),
            ("Excommunication", EventType::Excommunication),
            ("Funeral", EventType::Funeral),
            ("Hospitalization", EventType::Hospitalization),
            ("Illness", EventType::Illness),
            ("Passenger list", EventType::PassengerList),
            ("Military distinction", EventType::MilitaryDistinction),
            ("Military promotion", EventType::MilitaryPromotion),
            ("Military mobilization", EventType::MilitaryMobilization),
            ("Property sale", EventType::PropertySale),
            ("BAPL", EventType::LdsBaptism),
            ("CONL", EventType::LdsConfirmation),
            ("INIL", EventType::LdsInitiatory),
            ("ENDL", EventType::Endowment),
            ("DotationLDS", EventType::LdsDotation),
            ("SLGC", EventType::SealingChild),
            ("SLGS", EventType::SealingSpouse),
            ("Scellent parent LDS", EventType::SealingParent),
            ("Family link LDS", EventType::FamilyLinkLds),
            ("unmarried", EventType::NoMarriage),
            ("nomen", EventType::NoMention),
            ("OCCU", EventType::Occupation),
            ("PROP", EventType::Property),
        ] {
            assert_eq!(
                event_type_from_type_text(Some(label)),
                Some(expected),
                "geneweb label {label}"
            );
        }
    }

    /// Substrings that would misfire if the table used looser keywords.
    #[test]
    fn a_word_containing_a_tag_is_not_that_tag() {
        // "Williams" contains "will"; "job title" contains "title".
        assert_eq!(event_type_from_type_text(Some("Estate of Williams")), None);
        assert_eq!(event_type_from_type_text(Some("Job title change")), None);
    }
}

#[cfg(test)]
mod gedzip_tests {
    use super::*;

    /// A one-person GEDCOM whose single `OBJE` names `path`.
    fn gedcom_naming(path: &str) -> String {
        format!(
            "0 HEAD\n\
             1 GEDC\n\
             2 VERS 5.5.1\n\
             1 CHAR UTF-8\n\
             0 @I1@ INDI\n\
             1 NAME Branch /A/\n\
             1 OBJE @M1@\n\
             0 @M1@ OBJE\n\
             1 FILE {path}\n\
             2 FORM image/jpeg\n\
             0 TRLR\n"
        )
    }

    fn archive(gedcom: &str, files: &[(&str, &[u8])]) -> Vec<u8> {
        let files: Vec<(String, Vec<u8>)> = files
            .iter()
            .map(|(name, bytes)| ((*name).to_string(), (*bytes).to_vec()))
            .collect();
        crate::export::export_gedzip(gedcom, &files).expect("writes the archive")
    }

    /// The single page in a result, which is where the bytes and the file
    /// path live; the other row is the document describing it.
    fn page_of(result: &ImportResult) -> &Media {
        result
            .media
            .iter()
            .find(|item| !item.is_document())
            .expect("a page was imported")
    }

    /// The document an inline `OBJE` became holds its title and note, and no
    /// file of its own.
    fn assert_describing_document(document: &Media) {
        assert_eq!(document.title.as_deref(), Some("Sample portrait"));
        assert_eq!(document.description.as_deref(), Some("Sample description"));
        assert_eq!(document.mime_type, DOCUMENT_MIME);
        assert!(document.file_path.is_empty());
        assert_eq!(document.page_count, 1);
    }

    /// The page names the file and nothing more.
    fn assert_bare_page_of(page: &Media, document: &Media) {
        assert_eq!(page.parent_media_id, Some(document.id));
        assert_eq!(page.page_index, 0);
        assert_eq!(page.mime_type, "image/jpeg");
        assert_eq!(page.file_name, "portrait.jpg");
        assert_eq!(page.file_path, "media/portrait.jpg");
        assert!(page.title.is_none());
        assert!(page.description.is_none());
    }

    #[test]
    fn inline_media_imports_as_a_document_with_a_page_and_keeps_its_gedzip_bytes() {
        let gedcom = "0 HEAD\n1 GEDC\n2 VERS 5.5.1\n1 CHAR UTF-8\n\
                      0 @I1@ INDI\n1 NAME Sample /Person/\n\
                      1 OBJE\n2 FILE media/portrait.jpg\n3 FORM jpeg\n\
                      2 TITL Sample portrait\n2 NOTE Sample description\n0 TRLR\n";
        let bytes = archive(gedcom, &[("media/portrait.jpg", b"IMAGE BYTES")]);
        let imported = import_gedzip(&bytes, Uuid::now_v7()).expect("imports");
        let result = &imported.result;
        assert!(result.warnings.is_empty(), "{:?}", result.warnings);
        assert_eq!(result.media.len(), 2);
        let document = result.media.iter().find(|m| m.is_document()).unwrap();
        let page = page_of(result);
        assert_describing_document(document);
        assert_bare_page_of(page, document);
        assert_eq!(result.media_links.len(), 1);
        assert_eq!(result.media_links[0].media_id, document.id);
        assert_eq!(result.media_links[0].person_id, Some(result.persons[0].id));
        assert_eq!(imported.files, vec![(page.id, b"IMAGE BYTES".to_vec())]);
    }

    #[test]
    fn a_windows_file_path_names_the_page_by_its_last_segment() {
        let gedcom = "0 HEAD\n1 GEDC\n2 VERS 5.5.1\n1 CHAR UTF-8\n\
                      0 @I1@ INDI\n1 NAME Sample /Person/\n\
                      1 OBJE\n2 FILE C:\\Scans\\portrait.jpg\n3 FORM jpeg\n0 TRLR\n";
        let result = import_gedcom(gedcom, Uuid::now_v7()).expect("imports");
        let page = page_of(&result);
        assert_eq!(page.file_name, "portrait.jpg");
        assert_eq!(page.file_path, "C:\\Scans\\portrait.jpg");
    }

    #[test]
    fn the_bytes_of_every_medium_the_archive_carries_come_back_with_it() {
        let bytes = archive(
            &gedcom_naming("media/portrait.jpg"),
            &[("media/portrait.jpg", b"JPEGBYTES")],
        );

        let import = import_gedzip(&bytes, Uuid::now_v7()).expect("imports");

        assert_eq!(import.result.persons.len(), 1);
        // A document and the page that holds the file.
        assert_eq!(import.result.media.len(), 2);
        let page = page_of(&import.result);
        assert_eq!(import.files.len(), 1);
        assert_eq!(import.files[0].0, page.id);
        assert_eq!(import.files[0].1, b"JPEGBYTES");
        assert!(
            import.result.warnings.is_empty(),
            "got {:?}",
            import.result.warnings
        );
    }

    #[test]
    fn a_file_the_archive_does_not_hold_is_a_warning_and_an_unheld_record() {
        let bytes = archive(&gedcom_naming("media/portrait.jpg"), &[]);

        let import = import_gedzip(&bytes, Uuid::now_v7()).expect("imports");

        // The record survives — it is exactly what a plain `.ged` produces.
        assert_eq!(import.result.media.len(), 2);
        assert!(page_of(&import.result).storage_key.is_none());
        assert!(import.files.is_empty());
        assert!(
            import
                .result
                .warnings
                .iter()
                .any(|w| w.contains("media/portrait.jpg")),
            "got {:?}",
            import.result.warnings
        );
    }

    #[test]
    fn a_producers_own_separators_and_case_still_find_the_entry() {
        // What a Windows exporter writes, against the entry a ZIP holds.
        let bytes = archive(
            &gedcom_naming(r".\Media\Portrait.JPG"),
            &[("media/portrait.jpg", b"JPEGBYTES")],
        );

        let import = import_gedzip(&bytes, Uuid::now_v7()).expect("imports");

        assert_eq!(import.files.len(), 1, "got {:?}", import.result.warnings);
        assert_eq!(import.files[0].1, b"JPEGBYTES");
    }

    #[test]
    fn a_percent_encoded_reference_still_finds_the_entry() {
        // GEDCOM 7.0 writes a `FILE` as a URI reference.
        let bytes = archive(
            &gedcom_naming("media/my%20portrait.jpg"),
            &[("media/my portrait.jpg", b"JPEGBYTES")],
        );

        let import = import_gedzip(&bytes, Uuid::now_v7()).expect("imports");

        assert_eq!(import.files.len(), 1, "got {:?}", import.result.warnings);
        assert_eq!(import.files[0].1, b"JPEGBYTES");
    }

    #[test]
    fn a_url_is_recorded_rather_than_looked_for_in_the_archive() {
        let bytes = archive(&gedcom_naming("https://example.org/portrait.jpg"), &[]);

        let import = import_gedzip(&bytes, Uuid::now_v7()).expect("imports");

        assert_eq!(import.result.media.len(), 2);
        assert!(import.files.is_empty());
        // Nothing to warn about: we never meant to hold this one's bytes.
        assert!(
            import.result.warnings.is_empty(),
            "got {:?}",
            import.result.warnings
        );
    }

    #[test]
    fn a_file_no_record_names_is_reported_and_left_alone() {
        let bytes = archive(
            &gedcom_naming("media/portrait.jpg"),
            &[
                ("media/portrait.jpg", b"JPEGBYTES"),
                ("media/stray.jpg", b"ORPHANBYTES"),
            ],
        );

        let import = import_gedzip(&bytes, Uuid::now_v7()).expect("imports");

        assert_eq!(import.files.len(), 1);
        assert!(
            import
                .result
                .warnings
                .iter()
                .any(|w| w.contains("1 file(s)")),
            "got {:?}",
            import.result.warnings
        );
    }

    #[test]
    fn something_that_is_not_an_archive_at_all_fails_rather_than_importing_nothing() {
        let err = import_gedzip(b"0 HEAD\n0 TRLR\n", Uuid::now_v7()).unwrap_err();
        assert!(err.contains("GEDZIP"), "got {err}");
    }

    /// A file written before the page number was dropped carries one field too
    /// many. It cannot be imported — the coordinate it names no longer exists —
    /// but losing an identification without a word is the outcome worth
    /// preventing, so the arity mismatch has to be reported.
    #[test]
    fn a_vignette_line_of_an_unexpected_arity_is_reported_rather_than_dropped() {
        let gedcom = "0 HEAD\n\
             1 GEDC\n\
             2 VERS 5.5.1\n\
             0 @I1@ INDI\n\
             1 SEX U\n\
             0 @M1@ OBJE\n\
             1 FILE photo.jpg\n\
             2 FORM image/jpeg\n\
             1 _OXIDGENE_VIGNETTE @I1@ 0 120 45 64 82\n\
             0 TRLR\n";

        let result = import_gedcom(gedcom, Uuid::now_v7()).expect("imports");

        assert!(result.vignettes.is_empty(), "the line is not understood");
        assert!(
            result
                .warnings
                .iter()
                .any(|warning| warning.contains("_OXIDGENE_VIGNETTE")),
            "got {:?}",
            result.warnings
        );
    }

    /// The person of an event's crop is the pointer of its
    /// `_OXIDGENE_PERSON`.
    #[test]
    fn an_event_crop_reads_its_person_from_its_pointer() {
        let gedcom = "0 HEAD\n1 GEDC\n2 VERS 5.5.1\n0 @I1@ INDI\n1 SEX U\n\
             1 BIRT\n2 OBJE @M1@\n3 _CROP\n4 TOP 6\n4 LEFT 5\n\
             4 HEIGHT 80\n4 WIDTH 70\n3 _OXIDGENE_PERSON @I1@\n\
             0 @M1@ OBJE\n1 FILE photo.jpg\n2 FORM image/jpeg\n0 TRLR\n";
        let result = import_gedcom(gedcom, Uuid::now_v7()).expect("imports");
        assert_eq!(result.vignettes.len(), 1, "{:?}", result.warnings);
        let vignette = &result.vignettes[0];
        assert_eq!(vignette.person_id, Some(result.persons[0].id));
        assert_eq!(vignette.event_id, Some(result.events[0].id));
        assert_eq!(
            (vignette.x, vignette.y, vignette.width, vignette.height),
            (5, 6, 70, 80)
        );
    }
}
