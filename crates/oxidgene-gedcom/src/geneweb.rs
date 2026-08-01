//! GeneWeb `.gw` → OxidGene domain model import.
//!
//! `.gw` is the textual interchange format of the [GeneWeb] genealogy software
//! — what `gwu` writes and `gwc` reads. The [`geneweb`] crate reads it into a
//! lossless syntax tree and converts that tree into `ged_io`'s GEDCOM model, so
//! the whole GEDCOM → domain mapping in [`crate::import`] is reused as-is here.
//!
//! Two things are worth knowing about the format:
//!
//! - A `.gw` file is ISO-8859-1 unless it opts into UTF-8 with an `encoding:`
//!   directive, and the switch takes effect mid-file. The reader therefore
//!   takes raw bytes, never a `String` — decoding upstream would mangle
//!   accented names in every Latin-1 file.
//! - GeneWeb records concepts GEDCOM has no room for (per-person access rights,
//!   wizard notes, wiki pages). Those survive the conversion as user-defined
//!   `_GW…` tags, which OxidGene's GEDCOM importer does not model, so they are
//!   dropped here.
//!
//! [GeneWeb]: https://geneweb.tuxfamily.org
//! [`geneweb`]: https://github.com/trois-six/rust-geneweb

use geneweb::database::GwDatabase;
use uuid::Uuid;

use crate::ImportResult;
use crate::import::import_gedcom_data;

/// Import a GeneWeb `.gw` file into OxidGene domain model entities.
///
/// `input` is the raw file content — see the module docs on why this is bytes
/// and not a string. `origin_file` is the file's name, which GeneWeb records on
/// every family and which is echoed back in parse errors.
///
/// Reading is lenient: a malformed block is skipped and reported in
/// [`ImportResult::warnings`] rather than aborting the whole file, so a partly
/// broken export still yields everything it can.
///
/// # Errors
///
/// Returns `Err` if the file yielded no persons at all while reporting parse
/// errors — that is a file that failed to parse, not an empty genealogy.
pub fn import_geneweb(
    input: &[u8],
    origin_file: &str,
    tree_id: Uuid,
) -> Result<ImportResult, String> {
    let (db, errors) = GwDatabase::read_lenient(input, origin_file);

    if db.persons.is_empty() && !errors.is_empty() {
        let detail = errors
            .iter()
            .take(3)
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ");
        return Err(format!(
            "GeneWeb parse error: no person could be read from {origin_file} ({} error(s)): {detail}",
            errors.len()
        ));
    }

    let mut result = import_gedcom_data(&db.to_gedcom(), tree_id)?;

    // Prepend the reader's own warnings: they point at source lines of the .gw
    // file, which the GEDCOM-level warnings that follow cannot do.
    let mut warnings: Vec<String> = errors
        .iter()
        .map(|e| format!("GeneWeb: {}", e.to_string().replace('\n', " ")))
        .collect();
    warnings.append(&mut result.warnings);
    result.warnings = warnings;

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
encoding: utf-8

fam Doe Jean.0 +1900 #mp Paris Roe Marie.0
beg
- h Pierre.0 1925
end
";

    #[test]
    fn imports_a_minimal_family() {
        let tree_id = Uuid::now_v7();
        let result = import_geneweb(SAMPLE.as_bytes(), "sample.gw", tree_id).unwrap();

        assert_eq!(result.persons.len(), 3);
        assert_eq!(result.families.len(), 1);
        assert_eq!(result.family_spouses.len(), 2);
        assert_eq!(result.family_children.len(), 1);
        assert!(result.persons.iter().all(|p| p.tree_id == tree_id));

        let surnames: Vec<_> = result
            .person_names
            .iter()
            .filter_map(|n| n.surname.clone())
            .collect();
        assert!(surnames.iter().all(|s| s == "Doe" || s == "Roe"));
    }

    #[test]
    fn decodes_iso_8859_1_by_default() {
        // "Émile" as ISO-8859-1: É is 0xC9. Without a `encoding:` directive the
        // reader must treat the byte as Latin-1, not as invalid UTF-8.
        let mut input = Vec::new();
        input.extend_from_slice(b"fam Doe \xC9mile.0 + Roe Marie.0\n");

        let result = import_geneweb(&input, "latin1.gw", Uuid::now_v7()).unwrap();
        assert!(
            result
                .person_names
                .iter()
                .any(|n| n.given_names.as_deref() == Some("Émile")),
            "expected the Latin-1 É to decode, got {:?}",
            result.person_names
        );
    }

    #[test]
    fn rejects_a_file_that_yields_nothing() {
        let err = import_geneweb(b"this is not a gw file at all\n", "junk.gw", Uuid::now_v7())
            .unwrap_err();
        assert!(err.contains("GeneWeb parse error"), "got: {err}");
    }
}
