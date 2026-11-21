//! The bounds a GEDZIP is read within: archives generated here, fictitious,
//! each one over a bound of [`GedzipLimits`] or just within it.

use std::io::{Cursor, Write as _};

use oxidgene_core::OxidGeneError;
use oxidgene_gedcom::import::{GedzipLimits, GedzipReadError, prepare_gedzip};
use uuid::Uuid;
use zip::CompressionMethod;
use zip::write::SimpleFileOptions;

/// A GEDCOM of one fictitious person, padded with notes to at least
/// `bytes` bytes: text that deflates to almost nothing.
fn gedcom_of(bytes: usize) -> Vec<u8> {
    let mut text =
        String::from("0 HEAD\n1 GEDC\n2 VERS 7.0\n0 @I1@ INDI\n1 NAME person_a /BRANCH_A/\n");
    let mut note = 0;
    while text.len() < bytes {
        note += 1;
        text.push_str(&format!("0 @N{note}@ SNOTE {}\n", "a".repeat(200)));
    }
    text.push_str("0 TRLR\n");
    text.into_bytes()
}

/// A GEDZIP of `gedcom` and `media` empty files named `media/NNNNNN.jpg`.
fn archive_of(gedcom: &[u8], media: usize, method: CompressionMethod) -> Vec<u8> {
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(method);
    archive.start_file("gedcom.ged", options).unwrap();
    archive.write_all(gedcom).unwrap();
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for index in 0..media {
        archive
            .start_file(format!("media/{index:06}.jpg"), stored)
            .unwrap();
    }
    archive.finish().unwrap().into_inner()
}

fn prepare(archive: Vec<u8>, limits: &GedzipLimits) -> Result<usize, GedzipReadError> {
    prepare_gedzip(Cursor::new(archive), Uuid::now_v7(), limits)
        .map(|(_, plan)| plan.result.persons.len())
}

/// A `gedcom.ged` that inflates past the bound is refused while it
/// inflates, however little the archive weighs.
#[test]
fn a_gedcom_that_inflates_past_its_bound_is_refused() {
    let megabyte = 1024 * 1024;
    let gedcom = gedcom_of(2 * megabyte);
    let archive = archive_of(&gedcom, 0, CompressionMethod::Deflated);
    assert!(
        archive.len() < megabyte / 8,
        "the archive is {} bytes for {} of text",
        archive.len(),
        gedcom.len()
    );
    let limits = GedzipLimits {
        gedcom_bytes: megabyte as u64,
        ..GedzipLimits::default()
    };
    let refused = prepare(archive.clone(), &limits).expect_err("over the bound");
    assert!(
        matches!(refused, GedzipReadError::GedcomTooLarge { limit } if limit == megabyte as u64),
        "{refused}"
    );
    assert!(matches!(
        OxidGeneError::from(refused),
        OxidGeneError::TooLarge(_)
    ));
    // The same archive within the default bound imports.
    assert_eq!(prepare(archive, &GedzipLimits::default()).unwrap(), 1);
}

/// The entries are counted before any is read: the archive's `gedcom.ged`
/// and its media, within the bound or refused.
#[test]
fn an_archive_of_more_entries_than_its_bound_is_refused() {
    let limits = GedzipLimits {
        entries: 10,
        ..GedzipLimits::default()
    };
    let gedcom = gedcom_of(0);
    let within = archive_of(&gedcom, 9, CompressionMethod::Deflated);
    assert_eq!(prepare(within, &limits).unwrap(), 1);
    let over = archive_of(&gedcom, 10, CompressionMethod::Deflated);
    let refused = prepare(over, &limits).expect_err("eleven entries");
    assert!(
        matches!(refused, GedzipReadError::TooManyEntries { limit: 10 }),
        "{refused}"
    );
    assert!(matches!(
        OxidGeneError::from(refused),
        OxidGeneError::TooLarge(_)
    ));
}

/// An archive made of entries alone — a directory far larger than the
/// entries accepted take — is refused while its directory is read, before
/// a record is built for each of its entries.
#[test]
fn an_archive_that_is_all_directory_is_refused_while_it_opens() {
    let limits = GedzipLimits {
        entries: 10,
        ..GedzipLimits::default()
    };
    let archive = archive_of(&gedcom_of(0), 20_000, CompressionMethod::Deflated);
    let refused = prepare(archive, &limits).expect_err("twenty thousand entries");
    assert!(
        matches!(refused, GedzipReadError::TooManyEntries { limit: 10 }),
        "{refused}"
    );
}

/// What is no archive at all stays invalid genealogy data, not a size.
#[test]
fn what_is_not_an_archive_is_invalid_not_too_large() {
    let refused =
        prepare(b"0 HEAD\n0 TRLR\n".to_vec(), &GedzipLimits::default()).expect_err("a plain text");
    assert!(matches!(refused, GedzipReadError::Archive(_)), "{refused}");
    assert!(matches!(
        OxidGeneError::from(refused),
        OxidGeneError::Gedcom(_)
    ));
}
