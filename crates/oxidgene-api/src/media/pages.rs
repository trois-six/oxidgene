//! How many pages a document holds.
//!
//! Genealogy sources arrive as documents far more often than as single photos:
//! a parish register scan is forty pages, a notarial act is three, and the
//! citation that matters points at page 27. Knowing the count at upload time is
//! what lets the UI offer a page carousel instead of a download link, and what
//! lets a vignette say which page it was cropped from.
//!
//! Counting is header work, not rendering. Nothing here decodes a pixel.

/// Number of pages in `bytes`, given its MIME type.
///
/// Returns `1` for single-page formats and for anything unparseable — a
/// document we cannot count is still a document, and refusing the upload over
/// a metadata field would be worse than under-reporting it.
pub fn count(mime_type: &str, bytes: &[u8]) -> u32 {
    match mime_type {
        "application/pdf" => count_pdf(bytes).unwrap_or(1),
        "image/tiff" => count_tiff(bytes).unwrap_or(1),
        _ => 1,
    }
}

/// Whether this MIME type can carry more than one page.
pub fn is_multipage_format(mime_type: &str) -> bool {
    matches!(mime_type, "application/pdf" | "image/tiff")
}

fn count_pdf(bytes: &[u8]) -> Option<u32> {
    // `load_mem` walks the xref table, so this also handles the compressed
    // object streams that a naive `/Type /Page` grep miscounts.
    let document = lopdf::Document::load_mem(bytes).ok()?;
    let pages = document.get_pages().len();
    u32::try_from(pages).ok().filter(|n| *n > 0)
}

/// Walk a TIFF's chain of image file directories.
///
/// A multi-page TIFF is a linked list: the header points at the first IFD, and
/// each IFD ends with the offset of the next, or zero. Counting the links needs
/// only the entry counts, so this reads a handful of bytes per page rather than
/// handing a 300 MB register scan to a decoder.
fn count_tiff(bytes: &[u8]) -> Option<u32> {
    let big_endian = match bytes.get(0..2)? {
        b"II" => false,
        b"MM" => true,
        _ => return None,
    };
    let u16_at = |offset: usize| -> Option<u16> {
        let raw: [u8; 2] = bytes.get(offset..offset + 2)?.try_into().ok()?;
        Some(if big_endian {
            u16::from_be_bytes(raw)
        } else {
            u16::from_le_bytes(raw)
        })
    };
    let u32_at = |offset: usize| -> Option<u32> {
        let raw: [u8; 4] = bytes.get(offset..offset + 4)?.try_into().ok()?;
        Some(if big_endian {
            u32::from_be_bytes(raw)
        } else {
            u32::from_le_bytes(raw)
        })
    };
    let u64_at = |offset: usize| -> Option<u64> {
        let raw: [u8; 8] = bytes.get(offset..offset + 8)?.try_into().ok()?;
        Some(if big_endian {
            u64::from_be_bytes(raw)
        } else {
            u64::from_le_bytes(raw)
        })
    };

    // Classic TIFF carries magic 42 with 32-bit offsets; BigTIFF carries 43
    // with 64-bit ones and a different IFD shape. Scanner software emits
    // BigTIFF once a register run crosses 4 GB, so both are worth reading.
    let (mut next, entry_size, count_size, big) = match u16_at(2)? {
        42 => (u32_at(4)? as u64, 12usize, 2usize, false),
        43 => {
            if u16_at(4)? != 8 {
                return None; // 8-byte offsets is the only defined value
            }
            (u64_at(8)?, 20usize, 8usize, true)
        }
        _ => return None,
    };

    let mut pages = 0u32;
    // A malformed file can point an IFD at itself. The cap is far above any
    // real scan and keeps a hostile upload from spinning a worker forever.
    const MAX_PAGES: u32 = 10_000;
    while next != 0 && pages < MAX_PAGES {
        let base = usize::try_from(next).ok()?;
        let entries = if big {
            u64_at(base)?
        } else {
            u16_at(base)? as u64
        };
        let after_entries = base
            .checked_add(count_size)?
            .checked_add(usize::try_from(entries).ok()?.checked_mul(entry_size)?)?;
        pages += 1;
        next = if big {
            u64_at(after_entries)?
        } else {
            u32_at(after_entries)? as u64
        };
    }
    Some(pages).filter(|n| *n > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a classic little-endian TIFF whose IFD chain has `pages` links.
    ///
    /// Each IFD carries one minimal entry, which is enough for the walk under
    /// test: it never looks at what the entries mean, only at how many there
    /// are and where the chain goes next.
    fn tiff_with_pages(pages: usize) -> Vec<u8> {
        const HEADER: usize = 8;
        const IFD: usize = 2 + 12 + 4; // entry count + one entry + next offset
        let mut out = Vec::new();
        out.extend_from_slice(b"II");
        out.extend_from_slice(&42u16.to_le_bytes());
        out.extend_from_slice(&(HEADER as u32).to_le_bytes());
        for page in 0..pages {
            out.extend_from_slice(&1u16.to_le_bytes()); // one entry
            out.extend_from_slice(&[0u8; 12]); // the entry itself
            let next = if page + 1 == pages {
                0
            } else {
                (HEADER + IFD * (page + 1)) as u32
            };
            out.extend_from_slice(&next.to_le_bytes());
        }
        out
    }

    /// The same, big-endian, to prove the byte-order branch is exercised.
    fn tiff_big_endian_two_pages() -> Vec<u8> {
        const HEADER: usize = 8;
        const IFD: usize = 2 + 12 + 4;
        let mut out = Vec::new();
        out.extend_from_slice(b"MM");
        out.extend_from_slice(&42u16.to_be_bytes());
        out.extend_from_slice(&(HEADER as u32).to_be_bytes());
        out.extend_from_slice(&1u16.to_be_bytes());
        out.extend_from_slice(&[0u8; 12]);
        out.extend_from_slice(&((HEADER + IFD) as u32).to_be_bytes());
        out.extend_from_slice(&1u16.to_be_bytes());
        out.extend_from_slice(&[0u8; 12]);
        out.extend_from_slice(&0u32.to_be_bytes());
        out
    }

    #[test]
    fn a_single_page_tiff_counts_as_one() {
        assert_eq!(count("image/tiff", &tiff_with_pages(1)), 1);
    }

    #[test]
    fn a_forty_page_register_scan_reports_forty() {
        assert_eq!(count("image/tiff", &tiff_with_pages(40)), 40);
    }

    #[test]
    fn big_endian_tiffs_count_the_same() {
        assert_eq!(count("image/tiff", &tiff_big_endian_two_pages()), 2);
    }

    #[test]
    fn an_ifd_pointing_at_itself_does_not_hang() {
        // next-offset loops back to the first IFD instead of terminating.
        let mut bytes = tiff_with_pages(1);
        let len = bytes.len();
        bytes[len - 4..].copy_from_slice(&8u32.to_le_bytes());
        assert_eq!(count("image/tiff", &bytes), 10_000, "capped, not infinite");
    }

    #[test]
    fn a_truncated_tiff_falls_back_to_one_page() {
        let bytes = tiff_with_pages(5);
        assert_eq!(count("image/tiff", &bytes[..12]), 1);
    }

    #[test]
    fn something_claiming_to_be_a_tiff_but_is_not_counts_as_one() {
        assert_eq!(count("image/tiff", b"not a tiff"), 1);
        assert_eq!(count("image/tiff", b""), 1);
    }

    #[test]
    fn an_unparseable_pdf_counts_as_one_rather_than_failing_the_upload() {
        assert_eq!(count("application/pdf", b"%PDF-1.4\ntruncated"), 1);
    }

    #[test]
    fn single_page_formats_are_not_probed() {
        assert_eq!(count("image/jpeg", b"\xff\xd8\xff"), 1);
        assert_eq!(count("image/png", b"\x89PNG"), 1);
        assert!(!is_multipage_format("image/jpeg"));
        assert!(is_multipage_format("application/pdf"));
        assert!(is_multipage_format("image/tiff"));
    }
}
