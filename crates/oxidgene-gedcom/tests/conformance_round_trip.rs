//! Round trip of oxidged's "every structure" conformance files.
//!
//! Each fixture is imported, then exported for its own GEDCOM version. The
//! export must be conformant to that version, and idempotent: importing it and
//! exporting again gives the same text, the header aside. What the export
//! loses is compared with `fixtures/conformance/known_gaps.tsv`, in both
//! directions: a new loss fails, and so does a listed loss that no longer
//! happens, so the list only shrinks as the export improves.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

use oxidged::spec::validate_text;
use oxidged::tree::{Structure, Tree};
use oxidgene_gedcom::ImportResult;
use oxidgene_gedcom::export::{
    ExportChoices, ExportOptions, ExportTarget, SubmitterSettings, destination, export_gedcom,
};
use oxidgene_gedcom::import::import_gedcom_bytes;
use uuid::Uuid;

/// The number of structures per tag path, e.g. `INDI.BIRT.PLAC.FONE`.
type TagCounts = BTreeMap<String, usize>;

/// The accepted losses: `(fixture, tag path)` to the missing count and reason.
type KnownGaps = BTreeMap<(String, String), (usize, String)>;

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/conformance")
        .join(name)
}

fn import_bytes(bytes: &[u8]) -> ImportResult {
    import_gedcom_bytes(bytes, Uuid::now_v7()).expect("the text imports")
}

/// The import exported again for `destination_id`, its notes, sources and
/// media included, with the submitter it names as the submitter settings.
fn export_for(imported: &ImportResult, destination_id: &str) -> String {
    let choices = ExportChoices {
        destination: destination(destination_id).expect("a known export destination"),
        ..ExportChoices::default()
    };
    let submitter = imported.submitter.clone().unwrap_or_default();
    let options = ExportOptions {
        choices,
        target: ExportTarget::Gedcom,
        self_person_id: None,
        submitter: SubmitterSettings {
            name: submitter.name.as_deref(),
            email: submitter.email.as_deref(),
            address: submitter.address.as_deref(),
        },
    };
    export_gedcom(&imported.records(), &options)
        .expect("the export writes")
        .gedcom
}

fn count_paths(prefix: &str, structures: &[Structure], counts: &mut TagCounts) {
    for structure in structures
        .iter()
        .filter(|s| !matches!(s.tag.as_str(), "CONC" | "CONT"))
    {
        let tag = structure.tag.as_str();
        let path = if prefix.is_empty() {
            tag.to_owned()
        } else {
            format!("{prefix}.{tag}")
        };
        *counts.entry(path.clone()).or_default() += 1;
        count_paths(&path, &structure.substructures, counts);
    }
}

/// The tag paths of a GEDCOM text, the header and trailer aside.
fn tag_counts(text: &str) -> TagCounts {
    let records: Vec<Structure> = Tree::parse(text)
        .to_structures()
        .into_iter()
        .filter(|r| !matches!(r.tag.as_str(), "HEAD" | "TRLR"))
        .collect();
    let mut counts = TagCounts::new();
    count_paths("", &records, &mut counts);
    counts
}

/// Each path of `original` that `exported` holds fewer times, with the
/// difference.
fn lost_paths(original: &TagCounts, exported: &TagCounts) -> TagCounts {
    original
        .iter()
        .filter_map(|(path, &count)| {
            let kept = exported.get(path).copied().unwrap_or(0);
            (kept < count).then(|| (path.clone(), count - kept))
        })
        .collect()
}

fn known_gaps() -> KnownGaps {
    let text = fs::read_to_string(fixture_path("known_gaps.tsv")).expect("known_gaps.tsv reads");
    text.lines()
        .filter(|line| !line.trim().is_empty() && !line.starts_with('#'))
        .map(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            let &[fixture, path, count, reason] = fields.as_slice() else {
                panic!("known_gaps.tsv needs 4 tab-separated fields: {line}");
            };
            let count = count
                .parse()
                .unwrap_or_else(|_| panic!("known_gaps.tsv has a bad count: {line}"));
            (
                (fixture.to_owned(), path.to_owned()),
                (count, reason.to_owned()),
            )
        })
        .collect()
}

/// Every difference between the losses of `fixture` and its listed gaps.
/// `expected` is the listed count (0 when not listed), `found` the lost one.
fn gap_differences(fixture: &str, lost: &TagCounts, known: &KnownGaps) -> Vec<String> {
    let listed: TagCounts = known
        .iter()
        .filter(|((name, _), _)| name == fixture)
        .map(|((_, path), (count, _))| (path.clone(), *count))
        .collect();
    let paths: BTreeSet<&str> = lost
        .keys()
        .chain(listed.keys())
        .map(String::as_str)
        .collect();
    paths
        .into_iter()
        .filter_map(|path| {
            let found = lost.get(path).copied().unwrap_or(0);
            let expected = listed.get(path).copied().unwrap_or(0);
            (found != expected)
                .then(|| format!("{fixture}: {path} expected {expected} found {found}"))
        })
        .collect()
}

fn conformance_problems(fixture: &str, export: &str) -> Vec<String> {
    validate_text(export)
        .iter()
        .map(|deviation| format!("{fixture}: export not conformant: {deviation}"))
        .collect()
}

/// The text after the header record: it carries the export date.
fn without_header(text: &str) -> String {
    let mut in_header = false;
    let mut body = String::new();
    for line in text.lines() {
        if line.starts_with("0 ") {
            in_header = line == "0 HEAD";
        }
        if !in_header {
            body.push_str(line);
            body.push('\n');
        }
    }
    body
}

fn idempotence_problems(fixture: &str, export: &str, destination_id: &str) -> Vec<String> {
    let again = export_for(&import_bytes(export.as_bytes()), destination_id);
    let (first, second) = (without_header(export), without_header(&again));
    if first == second {
        return Vec::new();
    }
    let index = first
        .lines()
        .zip(second.lines())
        .position(|(a, b)| a != b)
        .unwrap_or_else(|| first.lines().count().min(second.lines().count()));
    vec![format!(
        "{fixture}: export not idempotent, first difference at body line {}: {:?} then {:?}",
        index + 1,
        first.lines().nth(index).unwrap_or_default(),
        second.lines().nth(index).unwrap_or_default(),
    )]
}

/// Checks `fixture` as its own `destination_id` export: conformance, and the
/// losses against `known_gaps.tsv`.
fn check_fixture(fixture: &str, destination_id: &str) {
    let original = fs::read(fixture_path(fixture)).expect("the fixture reads");
    let original_text = std::str::from_utf8(&original).expect("the fixture is UTF-8");
    let export = export_for(&import_bytes(&original), destination_id);

    let mut problems = conformance_problems(fixture, &export);
    let lost = lost_paths(&tag_counts(original_text), &tag_counts(&export));
    problems.extend(gap_differences(fixture, &lost, &known_gaps()));

    assert!(
        problems.is_empty(),
        "{fixture}: {} problem(s)\n{}",
        problems.len(),
        problems.join("\n")
    );
}

/// Checks that importing `fixture`'s export and exporting it again gives the
/// same text.
fn check_idempotent(fixture: &str, destination_id: &str) {
    let original = fs::read(fixture_path(fixture)).expect("the fixture reads");
    let export = export_for(&import_bytes(&original), destination_id);
    let problems = idempotence_problems(fixture, &export, destination_id);
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn maximal551_round_trips_with_its_known_gaps() {
    check_fixture("maximal551.ged", "generic-551");
}

#[test]
fn full551_round_trips_with_its_known_gaps() {
    check_fixture("full551.ged", "generic-551");
}

#[test]
fn full70_round_trips_with_its_known_gaps() {
    check_fixture("full70.ged", "generic-70");
}

#[test]
fn extensions70_round_trips_with_its_known_gaps() {
    check_fixture("extensions70.ged", "generic-70");
}

#[test]
fn maximal551_export_is_idempotent() {
    check_idempotent("maximal551.ged", "generic-551");
}

#[test]
fn full551_export_is_idempotent() {
    check_idempotent("full551.ged", "generic-551");
}

#[test]
fn full70_export_is_idempotent() {
    check_idempotent("full70.ged", "generic-70");
}

#[test]
fn extensions70_export_is_idempotent() {
    check_idempotent("extensions70.ged", "generic-70");
}
