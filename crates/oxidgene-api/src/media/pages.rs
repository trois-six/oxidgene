//! How many pages a document holds.
//!
//! Genealogy sources arrive as documents far more often than as single photos:
//! a parish register scan is forty pages, a notarial act is three, and the
//! citation that matters points at page 27. Knowing the count at upload time is
//! what lets the UI say how many pages a file holds rather than presenting it
//! as a single image.
//!
//! Counting is header work, not rendering. Nothing here decodes a pixel.
//!
//! PDFs are not counted. Reading a page count out of one means walking the
//! xref table — a naive `/Type /Page` scan miscounts the compressed object
//! streams every modern writer emits — and the only crate that does it
//! properly brought a PDF encryption stack, a parser combinator library and
//! two duplicated hash crates along for a single call. A PDF therefore
//! reports one page, which is what the code already does for anything it
//! cannot parse.

/// Number of pages in `bytes`, given its MIME type.
///
/// Returns `1` for single-page formats, for PDFs, and for anything
/// unparseable — a document we cannot count is still a document, and refusing
/// the upload over a metadata field would be worse than under-reporting it.
pub fn count(mime_type: &str, bytes: &[u8]) -> u32 {
    match mime_type {
        "image/tiff" => count_tiff(bytes).unwrap_or(1),
        _ => 1,
    }
}

/// Walk a TIFF's chain of image file directories.
///
/// A multi-page TIFF is a linked list: the header points at the first IFD, and
/// each IFD ends with the offset of the next, or zero. Counting the links needs
/// only the entry counts, so this reads a handful of bytes per page rather than
/// handing a 300 MB register scan to a decoder.
fn count_tiff(bytes: &[u8]) -> Option<u32> {
    // A malformed file can point an IFD at itself. The cap is far above any
    // real scan and keeps a hostile upload from spinning a worker forever.
    const MAX_PAGES: u32 = 10_000;
    let (tiff, shape) = Tiff::open(bytes)?;
    let mut next = tiff.uint(shape.first_offset_at, shape.offset_width)?;
    let mut pages = 0u32;
    while next != 0 && pages < MAX_PAGES {
        next = tiff.next_ifd(&shape, usize::try_from(next).ok()?)?;
        pages += 1;
    }
    Some(pages).filter(|n| *n > 0)
}

/// A TIFF's bytes and their byte order.
struct Tiff<'a> {
    bytes: &'a [u8],
    big_endian: bool,
}

impl<'a> Tiff<'a> {
    /// The TIFF in `bytes` and the shape of its IFDs, if it is one.
    fn open(bytes: &'a [u8]) -> Option<(Self, IfdShape)> {
        let big_endian = match bytes.get(0..2)? {
            b"II" => false,
            b"MM" => true,
            _ => return None,
        };
        let tiff = Self { bytes, big_endian };
        // Classic TIFF carries magic 42 with 32-bit offsets; BigTIFF carries
        // 43 with 64-bit ones and a different IFD shape. Scanner software
        // emits BigTIFF once a register run crosses 4 GB, so both are worth
        // reading.
        let shape = match tiff.uint(2, 2)? {
            42 => IfdShape::CLASSIC,
            // 8-byte offsets is the only defined value.
            43 if tiff.uint(4, 2)? == 8 => IfdShape::BIG,
            _ => return None,
        };
        Some((tiff, shape))
    }

    /// The offset of the IFD after the one at `base`, 0 after the last.
    fn next_ifd(&self, shape: &IfdShape, base: usize) -> Option<u64> {
        let entries = usize::try_from(self.uint(base, shape.count_width)?).ok()?;
        let after_entries = base
            .checked_add(shape.count_width)?
            .checked_add(entries.checked_mul(shape.entry_size)?)?;
        self.uint(after_entries, shape.offset_width)
    }

    /// The unsigned integer `width` bytes long at `offset`.
    fn uint(&self, offset: usize, width: usize) -> Option<u64> {
        let raw = self.bytes.get(offset..offset.checked_add(width)?)?;
        let push = |value: u64, byte: &u8| (value << 8) | u64::from(*byte);
        Some(if self.big_endian {
            raw.iter().fold(0, push)
        } else {
            raw.iter().rev().fold(0, push)
        })
    }
}

/// Where a TIFF flavour keeps its first IFD offset, and the widths of an
/// IFD's parts.
struct IfdShape {
    first_offset_at: usize,
    offset_width: usize,
    count_width: usize,
    entry_size: usize,
}

impl IfdShape {
    const CLASSIC: Self = Self {
        first_offset_at: 4,
        offset_width: 4,
        count_width: 2,
        entry_size: 12,
    };
    const BIG: Self = Self {
        first_offset_at: 8,
        offset_width: 8,
        count_width: 8,
        entry_size: 20,
    };
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
    fn a_pdf_counts_as_one_page_however_many_it_really_has() {
        assert_eq!(count("application/pdf", b"%PDF-1.4\ntruncated"), 1);
    }

    #[test]
    fn single_page_formats_are_not_probed() {
        assert_eq!(count("image/jpeg", b"\xff\xd8\xff"), 1);
        assert_eq!(count("image/png", b"\x89PNG"), 1);
    }
}
