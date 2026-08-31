//! Indexes Geneanet's "download all my data" archives **without unzipping
//! them**.
//!
//! A ZIP records the uncompressed length of every entry in its central
//! directory, at the end of the file. Reading that costs a few kilobytes per
//! archive however large it is, and exact byte length is precisely what the
//! size matching needs — so the archives are consumed where they lie, across
//! however many files Geneanet split the export into.
//!
//! This is the reason the wizard tells users *not* to unzip: extraction would
//! cost hundreds of megabytes of temporary space to learn something the
//! directory already states.
//!
//! Matching is exact wherever it can be. Filenames are never used — they are
//! upload names, unrelated to the deposit title and colliding freely — so an
//! entry is recognised by its **exact byte length**, which both sides can
//! state without transferring anything.
//!
//! Where no length is available the fallback is content, not a guess. A page
//! of a multi-page deposit has no length to match on (its download is a ZIP
//! assembled on the fly and streamed without a `Content-Length`), so it is
//! recognised by a perceptual hash of its rendition instead — see
//! [`PhashIndex`] and [`crate::phash`], which document the measurement behind
//! the thresholds and why a clash there is *detected* rather than resolved.
//!
//! See `docs/specifications/geneanet-media-import.md` §5.

use std::collections::{BTreeMap, HashMap};
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use image::ImageDecoder;

use crate::phash::{self, Phash};

/// File extensions that make an archive look like a media export.
///
/// Only used to warn "is it the right download?" — nothing is skipped for
/// failing this, because a deposit's original can be any type Geneanet let
/// through.
const IMAGE_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "bmp", "tif", "tiff", "webp", "pdf",
];

/// Maximum relative aspect-ratio difference accepted before perceptual hashing.
const MAX_ASPECT_RATIO_DIFFERENCE_PERCENT: u64 = 2;

/// Originals a run can reuse instead of downloading.
///
/// Implemented by both an unzipped directory (the CLI's `--local-media`) and a
/// set of archives read in place (the wizard's step 2), so [`crate::media`]
/// need not care which the user supplied.
pub trait LocalOriginals {
    /// Bytes of the file of exactly this length, if one can be named safely.
    ///
    /// Returning `None` means "download it": with several same-size candidates
    /// that are not byte-identical, any choice would be a guess.
    ///
    /// # Errors
    ///
    /// Returns `Err` if a candidate could not be read.
    fn resolve(&self, size: u64) -> Result<Option<Vec<u8>>>;

    /// How many files were indexed, for the run report.
    fn file_count(&self) -> usize;
}

/// What one archive turned out to hold.
#[derive(Debug, Clone)]
pub struct ArchiveInfo {
    pub path: PathBuf,
    pub file_count: usize,
    /// Entries whose extension looks like a medium. Zero is the "is this the
    /// right download?" case.
    pub image_count: usize,
}

/// One indexed entry, addressed well enough to be read back on demand.
#[derive(Debug, Clone)]
struct Entry {
    archive: usize,
    index: usize,
}

/// Several Geneanet data archives, indexed by entry length.
///
/// Entries live in one flat list so they can be addressed by position; the
/// size index and the perceptual-hash index both point into it rather than
/// each holding their own copy.
#[derive(Debug, Default)]
pub struct ArchiveSet {
    archives: Vec<ArchiveInfo>,
    entries: Vec<Entry>,
    by_size: HashMap<u64, Vec<usize>>,
}

impl ArchiveSet {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Reads one archive's central directory and folds it into the index.
    ///
    /// Adding the same file twice is ignored rather than reported: users pick
    /// several ZIPs at once and re-picking one is a slip, not a decision.
    /// Returns `None` in that case.
    ///
    /// # Errors
    ///
    /// Returns `Err` if the file is not a readable ZIP — that archive only, so
    /// a caller adding several can keep the ones that worked.
    pub fn add(&mut self, path: &Path) -> Result<Option<&ArchiveInfo>> {
        // Canonicalise so the same archive reached by two paths is still the
        // same archive.
        let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

        if self.archives.iter().any(|a| a.path == path) {
            return Ok(None);
        }

        let file =
            std::fs::File::open(&path).with_context(|| format!("opening {}", path.display()))?;
        let mut zip = zip::ZipArchive::new(file)
            .with_context(|| format!("reading {} as a ZIP archive", path.display()))?;

        let archive = self.archives.len();
        let mut file_count = 0;
        let mut image_count = 0;

        for index in 0..zip.len() {
            // `by_index_raw` does not start a decompressor: the length comes
            // straight out of the directory we already read.
            let entry = zip
                .by_index_raw(index)
                .with_context(|| format!("reading entry {index} of {}", path.display()))?;

            if entry.is_dir() {
                continue;
            }

            file_count += 1;
            if looks_like_media(entry.name()) {
                image_count += 1;
            }

            self.by_size
                .entry(entry.size())
                .or_default()
                .push(self.entries.len());
            self.entries.push(Entry { archive, index });
        }

        self.archives.push(ArchiveInfo {
            path,
            file_count,
            image_count,
        });

        Ok(self.archives.last())
    }

    /// Drops an archive and everything it contributed.
    ///
    /// Entries hold their archive's position, so the ones after it are shifted
    /// down rather than left pointing at the wrong file.
    pub fn remove(&mut self, path: &Path) {
        let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let Some(removed) = self.archives.iter().position(|a| a.path == path) else {
            return;
        };

        self.archives.remove(removed);

        // Rebuild rather than patch: entries are addressed by position, so
        // dropping some in the middle renumbers every index that points past
        // them. Rebuilding is O(n) over a list of a few hundred and cannot get
        // that renumbering subtly wrong.
        let surviving: Vec<Entry> = self
            .entries
            .drain(..)
            .filter(|entry| entry.archive != removed)
            .map(|mut entry| {
                if entry.archive > removed {
                    entry.archive -= 1;
                }
                entry
            })
            .collect();

        // The size index is rebuilt from the surviving entries, which means
        // re-reading their lengths from the central directories.
        self.entries = surviving;
        self.reindex_sizes();
    }

    /// Re-reads the length of every surviving entry into the size index.
    fn reindex_sizes(&mut self) {
        let mut by_size: HashMap<u64, Vec<usize>> = HashMap::new();

        for (position, entry) in self.entries.iter().enumerate() {
            let info = &self.archives[entry.archive];
            let Ok(file) = std::fs::File::open(&info.path) else {
                continue;
            };
            let Ok(mut zip) = zip::ZipArchive::new(file) else {
                continue;
            };
            let Ok(zipped) = zip.by_index_raw(entry.index) else {
                continue;
            };
            by_size.entry(zipped.size()).or_default().push(position);
        }

        self.by_size = by_size;
    }

    /// How many entries are indexed across every archive.
    #[must_use]
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// Reads the entry at `position` in the flat list.
    ///
    /// # Errors
    ///
    /// Returns `Err` if the position is out of range or the entry cannot be
    /// read.
    pub fn read_at(&self, position: usize) -> Result<Vec<u8>> {
        let entry = self
            .entries
            .get(position)
            .with_context(|| format!("no archive entry at position {position}"))?;
        self.read(entry)
    }

    #[must_use]
    pub fn archives(&self) -> &[ArchiveInfo] {
        &self.archives
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.archives.is_empty()
    }

    /// Reads one indexed entry out of its archive.
    fn read(&self, entry: &Entry) -> Result<Vec<u8>> {
        let info = &self.archives[entry.archive];
        let file = std::fs::File::open(&info.path)
            .with_context(|| format!("opening {}", info.path.display()))?;
        let mut zip = zip::ZipArchive::new(file)
            .with_context(|| format!("reading {}", info.path.display()))?;

        let mut zipped = zip
            .by_index(entry.index)
            .with_context(|| format!("opening entry {} of {}", entry.index, info.path.display()))?;

        let expected_size = zipped.size();
        read_declared_size(&mut zipped, expected_size)
            .with_context(|| format!("reading entry {} of {}", entry.index, info.path.display()))
    }
}

fn read_declared_size(reader: &mut impl Read, expected_size: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take(expected_size.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()) != Ok(expected_size) {
        anyhow::bail!(
            "archive entry size mismatch: declared {expected_size} bytes, decoded {}",
            bytes.len()
        );
    }
    Ok(bytes)
}

impl ArchiveSet {
    /// The entry of exactly this length, if one can be named safely.
    ///
    /// Separate from [`LocalOriginals::resolve`] because a caller often wants
    /// to know *which* entry matched rather than its bytes — knowing that an
    /// entry is already accounted for is what lets the perceptual index skip
    /// it, and that index costs a full decode per entry it does not skip.
    ///
    /// # Errors
    ///
    /// Returns `Err` if a candidate could not be read for comparison.
    pub fn locate_by_size(&self, size: u64) -> Result<Option<usize>> {
        let candidates = self.by_size.get(&size).map_or(&[][..], Vec::as_slice);

        match candidates {
            [] => Ok(None),
            [only] => Ok(Some(*only)),
            [first, rest @ ..] => {
                // Same size, several entries: interchangeable only if they are
                // byte-identical, which is what a duplicate upload looks like.
                // Anything else defers to the network rather than guessing.
                let reference = self.read_at(*first)?;
                for other in rest {
                    if self.read_at(*other)? != reference {
                        return Ok(None);
                    }
                }
                Ok(Some(*first))
            }
        }
    }
}

impl LocalOriginals for ArchiveSet {
    fn resolve(&self, size: u64) -> Result<Option<Vec<u8>>> {
        match self.locate_by_size(size)? {
            Some(position) => self.read_at(position).map(Some),
            None => Ok(None),
        }
    }

    fn file_count(&self) -> usize {
        self.archives.iter().map(|a| a.file_count).sum()
    }
}

/// The archive's entries, keyed by what they look like.
///
/// Built separately from [`ArchiveSet`] because it is expensive in a way the
/// size index is not. Candidate headers are inspected first, then only images
/// with a compatible aspect ratio are decoded. It is built once, on demand,
/// by the caller that needs it.
///
/// Entries that do not decode as images — PDFs, above all — are simply absent.
/// A page that would have matched one of those is downloaded instead, which is
/// correct rather than a gap.
#[derive(Debug, Default)]
pub struct PhashIndex {
    /// Position in [`ArchiveSet::entries`], and that entry's hash.
    hashes: Vec<(usize, Phash)>,
    /// Entries rejected from their dimensions before pixel decoding.
    filtered: usize,
    /// Entries that could not be decoded, for the run report.
    undecodable: usize,
}

type HashSliceResult = (Vec<(usize, Phash)>, usize, usize);

impl PhashIndex {
    /// Hashes every entry of `set` that decodes as an image.
    #[must_use]
    pub fn build(set: &ArchiveSet) -> Self {
        Self::build_from(set, &(0..set.entry_count()).collect::<Vec<_>>())
    }

    /// Hashes only the entries at `positions`.
    ///
    /// This is the form callers should reach for. Decoding is the expensive
    /// step by orders of magnitude — a data archive is hundreds of full-size
    /// photographs — and only the entries an exact size match could *not*
    /// claim are ever candidates here. On the reference account that is 244
    /// entries out of 623, and they are the ones a single-page deposit's
    /// `Content-Length` never covered.
    #[must_use]
    pub fn build_from(set: &ArchiveSet, positions: &[usize]) -> Self {
        Self::build_from_matching_dimensions(set, positions, &[])
    }

    /// Hashes candidates whose aspect ratio can match one of `target_dimensions`.
    ///
    /// Reading dimensions only parses an image header. Candidates with unknown
    /// dimensions remain eligible so this optimization cannot hide a format the
    /// perceptual hasher can decode.
    #[must_use]
    pub fn build_from_matching_dimensions(
        set: &ArchiveSet,
        positions: &[usize],
        target_dimensions: &[(u32, u32)],
    ) -> Self {
        // Decoding is the whole cost here — a data archive is several hundred
        // full-size photographs — and each entry is independent of every
        // other, so the work is split across the machine's cores. Reading is
        // grouped by archive within each worker so a ZIP's central directory
        // is parsed once per worker rather than once per entry: on a 725 MB
        // archive of 600 entries that difference is 600 parses against 8.
        let workers = oxidgene_core::resources::cpu_worker_limit().min(positions.len().max(1));

        let chunk = positions.len().div_ceil(workers.max(1));
        if chunk == 0 {
            return Self::default();
        }

        let results: Vec<HashSliceResult> = std::thread::scope(|scope| {
            let handles: Vec<_> = positions
                .chunks(chunk)
                .map(|slice| scope.spawn(move || hash_slice(set, slice, target_dimensions)))
                .collect();

            handles
                .into_iter()
                .filter_map(|handle| handle.join().ok())
                .collect()
        });

        let mut hashes = Vec::with_capacity(positions.len());
        let mut filtered = 0;
        let mut undecodable = 0;
        for (mut part, failed, rejected) in results {
            hashes.append(&mut part);
            undecodable += failed;
            filtered += rejected;
        }
        // Threads finish out of order; the index is searched linearly but a
        // stable order keeps `locate`'s answer reproducible run to run.
        hashes.sort_unstable_by_key(|(position, _)| *position);

        Self {
            hashes,
            filtered,
            undecodable,
        }
    }

    #[must_use]
    pub fn hashed_count(&self) -> usize {
        self.hashes.len()
    }

    #[must_use]
    pub fn undecodable_count(&self) -> usize {
        self.undecodable
    }

    #[must_use]
    pub fn filtered_count(&self) -> usize {
        self.filtered
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.hashes.is_empty()
    }

    /// Finds the archive entry that is the same picture as `query`.
    ///
    /// Needs `set` because settling a tie means comparing bytes: several
    /// entries at the same distance is, on a real archive, overwhelmingly the
    /// *same file uploaded twice* — and then either will do. This is the same
    /// rule [`ArchiveSet::resolve`] applies to a size clash, and it has to be
    /// the same rule, or a duplicate upload would be matched by one path and
    /// downloaded by the other.
    ///
    /// Returns `None` when nothing is close enough, and when the tied entries
    /// genuinely differ — a clash is detected, never resolved on probability.
    ///
    /// # Errors
    ///
    /// Returns `Err` if a tied entry could not be read back for comparison.
    pub fn locate(&self, set: &ArchiveSet, query: Phash) -> Result<Option<usize>> {
        let candidates: Vec<Phash> = self.hashes.iter().map(|(_, hash)| *hash).collect();

        match phash::find(query, &candidates) {
            phash::Match::Found(index) => Ok(Some(self.hashes[index].0)),
            phash::Match::None => Ok(None),
            phash::Match::Ambiguous(tied) => {
                let Some((first, rest)) = tied.split_first() else {
                    return Ok(None);
                };
                let reference = set.read_at(self.hashes[*first].0)?;
                for other in rest {
                    if set.read_at(self.hashes[*other].0)? != reference {
                        // Different pictures the hash cannot tell apart, or the
                        // same picture stored two different ways. Either way a
                        // choice would be a guess.
                        return Ok(None);
                    }
                }
                Ok(Some(self.hashes[*first].0))
            }
        }
    }

    /// Reads the bytes of the entry that is the same picture as `query`.
    ///
    /// # Errors
    ///
    /// Returns `Err` if the matched entry could not be read back.
    pub fn resolve(&self, set: &ArchiveSet, query: Phash) -> Result<Option<Vec<u8>>> {
        match self.locate(set, query)? {
            Some(position) => set.read_at(position).map(Some),
            None => Ok(None),
        }
    }
}

/// Intrinsic dimensions without decoding the image's pixels.
#[must_use]
pub fn image_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    let reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    reader
        .into_decoder()
        .ok()
        .map(|decoder| decoder.dimensions())
}

fn compatible_aspect_ratio(left: (u32, u32), right: (u32, u32)) -> bool {
    let left_product = u64::from(left.0) * u64::from(right.1);
    let right_product = u64::from(right.0) * u64::from(left.1);
    let largest = left_product.max(right_product);

    largest > 0
        && left_product.abs_diff(right_product) * 100
            <= largest * MAX_ASPECT_RATIO_DIFFERENCE_PERCENT
}

/// Hashes one worker's share, opening each archive it touches once.
fn hash_slice(
    set: &ArchiveSet,
    positions: &[usize],
    target_dimensions: &[(u32, u32)],
) -> HashSliceResult {
    let mut by_archive: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for position in positions {
        if let Some(entry) = set.entries.get(*position) {
            by_archive.entry(entry.archive).or_default().push(*position);
        }
    }

    let mut hashes = Vec::with_capacity(positions.len());
    let mut filtered = 0;
    let mut undecodable = 0;

    for (archive, members) in by_archive {
        let info = &set.archives[archive];
        let Ok(file) = std::fs::File::open(&info.path) else {
            undecodable += members.len();
            continue;
        };
        let Ok(mut zip) = zip::ZipArchive::new(file) else {
            undecodable += members.len();
            continue;
        };

        for position in members {
            let index = set.entries[position].index;
            let Ok(mut entry) = zip.by_index(index) else {
                undecodable += 1;
                continue;
            };
            let expected_size = entry.size();
            let Ok(bytes) = read_declared_size(&mut entry, expected_size) else {
                undecodable += 1;
                continue;
            };

            let compatible = image_dimensions(&bytes).is_none_or(|dimensions| {
                target_dimensions.is_empty()
                    || target_dimensions.iter().any(|target| {
                        compatible_aspect_ratio(dimensions, *target)
                            || compatible_aspect_ratio((dimensions.1, dimensions.0), *target)
                    })
            });
            if !compatible {
                filtered += 1;
                continue;
            }

            match phash::hash_image(&bytes) {
                Ok(hash) => hashes.push((position, hash)),
                // PDFs and anything else `image` cannot read. The page that
                // would have matched one is downloaded instead.
                _ => undecodable += 1,
            }
        }
    }

    (hashes, undecodable, filtered)
}

/// Whether an archive entry's name looks like a medium.
fn looks_like_media(name: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| {
            let ext = ext.to_ascii_lowercase();
            IMAGE_EXTENSIONS.contains(&ext.as_str())
        })
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    /// A deterministic PNG, distinct per `seed`, for content-matching tests.
    fn png(width: u32, height: u32, seed: u32) -> Vec<u8> {
        let image = image::RgbImage::from_fn(width, height, |x, y| {
            let v = ((x * 7 + y * 13 + seed * 53) % 251) as u8;
            image::Rgb([v, v.wrapping_mul(3), v.wrapping_add(seed as u8)])
        });
        let mut buffer = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(image)
            .write_to(&mut buffer, image::ImageFormat::Png)
            .expect("encodes");
        buffer.into_inner()
    }

    /// A scratch directory that cleans itself up.
    struct TempDir(tempfile::TempDir);

    impl TempDir {
        fn new(tag: &str) -> Self {
            Self(
                tempfile::Builder::new()
                    .prefix(&format!("oxidgene-archive-{tag}-"))
                    .tempdir()
                    .expect("creates"),
            )
        }

        fn zip(&self, name: &str, entries: &[(&str, &[u8])]) -> PathBuf {
            let path = self.0.path().join(name);
            let file = std::fs::File::create(&path).expect("creates");
            let mut writer = zip::ZipWriter::new(file);
            let options: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
            for (entry, bytes) in entries {
                writer.start_file(*entry, options).expect("starts");
                writer.write_all(bytes).expect("writes");
            }
            writer.finish().expect("finishes");
            path
        }
    }

    #[test]
    fn indexes_entries_without_extracting_them() {
        let dir = TempDir::new("index");
        let archive = dir.zip(
            "data.zip",
            &[("portrait.jpg", b"hello"), ("notes.txt", b"a longer entry")],
        );

        let mut set = ArchiveSet::new();
        let info = set.add(&archive).expect("indexes").expect("is new").clone();

        assert_eq!(info.file_count, 2);
        // Only the .jpg looks like a medium; the .txt is counted as a file but
        // not as an image.
        assert_eq!(info.image_count, 1);
        assert_eq!(set.file_count(), 2);
        assert_eq!(
            set.resolve(5).expect("resolves").as_deref(),
            Some(&b"hello"[..])
        );
    }

    #[test]
    fn declared_size_must_match_the_decoded_bytes() {
        assert_eq!(
            read_declared_size(&mut &b"hello"[..], 5).expect("matching size"),
            b"hello"
        );
        assert!(read_declared_size(&mut &b"hello"[..], 4).is_err());
        assert!(read_declared_size(&mut &b"hello"[..], 6).is_err());
    }

    #[test]
    fn a_size_nothing_holds_resolves_to_nothing() {
        let dir = TempDir::new("absent");
        let archive = dir.zip("data.zip", &[("a.jpg", b"hello")]);

        let mut set = ArchiveSet::new();
        set.add(&archive).expect("indexes");

        assert!(set.resolve(999).expect("resolves").is_none());
    }

    #[test]
    fn matches_across_several_archives() {
        // Geneanet splits a large export, and the wizard tells users to add all
        // of the parts. A file in part 2 must be as findable as one in part 1.
        let dir = TempDir::new("split");
        let one = dir.zip("part1.zip", &[("a.jpg", b"hello")]);
        let two = dir.zip("part2.zip", &[("b.jpg", b"a longer entry")]);

        let mut set = ArchiveSet::new();
        set.add(&one).expect("indexes");
        set.add(&two).expect("indexes");

        assert_eq!(set.archives().len(), 2);
        assert_eq!(set.file_count(), 2);
        assert_eq!(
            set.resolve(14).expect("resolves").as_deref(),
            Some(&b"a longer entry"[..])
        );
    }

    #[test]
    fn the_same_archive_added_twice_is_ignored_silently() {
        let dir = TempDir::new("twice");
        let archive = dir.zip("data.zip", &[("a.jpg", b"hello")]);

        let mut set = ArchiveSet::new();
        assert!(set.add(&archive).expect("indexes").is_some());
        assert!(set.add(&archive).expect("indexes").is_none());

        assert_eq!(set.archives().len(), 1);
        assert_eq!(set.file_count(), 1);
    }

    #[test]
    fn duplicate_uploads_are_interchangeable() {
        // The real collision: the same photo uploaded twice, byte-identical.
        // Either entry will do.
        let dir = TempDir::new("duplicate");
        let archive = dir.zip(
            "data.zip",
            &[
                ("photo.jpg", b"same bytes"),
                ("photo (1).jpg", b"same bytes"),
            ],
        );

        let mut set = ArchiveSet::new();
        set.add(&archive).expect("indexes");

        assert_eq!(
            set.resolve(10).expect("resolves").as_deref(),
            Some(&b"same bytes"[..])
        );
    }

    #[test]
    fn a_size_clash_between_different_files_resolves_to_nothing() {
        // Detected, not silently resolved: the caller downloads instead.
        let dir = TempDir::new("clash");
        let archive = dir.zip(
            "data.zip",
            &[("a.jpg", b"abcdefghij"), ("b.jpg", b"0123456789")],
        );

        let mut set = ArchiveSet::new();
        set.add(&archive).expect("indexes");

        assert!(set.resolve(10).expect("resolves").is_none());
    }

    #[test]
    fn a_clash_spanning_two_archives_is_caught_too() {
        let dir = TempDir::new("cross-clash");
        let one = dir.zip("part1.zip", &[("a.jpg", b"abcdefghij")]);
        let two = dir.zip("part2.zip", &[("b.jpg", b"0123456789")]);

        let mut set = ArchiveSet::new();
        set.add(&one).expect("indexes");
        set.add(&two).expect("indexes");

        assert!(set.resolve(10).expect("resolves").is_none());
    }

    #[test]
    fn an_archive_holding_no_media_is_indexed_and_flagged() {
        // Accepted with a warning, per the wizard's error table — a user who
        // downloaded the wrong export should be told, not blocked.
        let dir = TempDir::new("no-media");
        let archive = dir.zip("data.zip", &[("readme.txt", b"nothing here")]);

        let mut set = ArchiveSet::new();
        let info = set.add(&archive).expect("indexes").expect("is new");

        assert_eq!(info.file_count, 1);
        assert_eq!(info.image_count, 0);
    }

    #[test]
    fn a_file_that_is_not_a_zip_fails_on_its_own() {
        let dir = TempDir::new("corrupt");
        let path = dir.0.path().join("broken.zip");
        std::fs::write(&path, b"this is not a ZIP archive").expect("writes");

        let mut set = ArchiveSet::new();

        assert!(set.add(&path).is_err());
        // The failure is that archive's alone — the set is still usable.
        assert!(set.is_empty());
    }

    #[test]
    fn removing_an_archive_keeps_the_others_readable() {
        // The entries of the archives after the removed one carry its index,
        // so this is where an off-by-one would read the wrong file's bytes.
        let dir = TempDir::new("remove");
        let one = dir.zip("part1.zip", &[("a.jpg", b"hello")]);
        let two = dir.zip("part2.zip", &[("b.jpg", b"a longer entry")]);

        let mut set = ArchiveSet::new();
        set.add(&one).expect("indexes");
        set.add(&two).expect("indexes");
        set.remove(&one);

        assert_eq!(set.archives().len(), 1);
        assert!(set.resolve(5).expect("resolves").is_none());
        assert_eq!(
            set.resolve(14).expect("resolves").as_deref(),
            Some(&b"a longer entry"[..])
        );
    }

    #[test]
    fn the_phash_index_finds_the_entry_that_is_the_same_picture() {
        // The multi-page case end to end: an entry the size index cannot help
        // with, recognised by content instead.
        let dir = TempDir::new("phash-find");
        let archive = dir.zip(
            "data.zip",
            &[
                ("page1.png", &png(40, 30, 3)),
                ("page2.png", &png(40, 30, 91)),
            ],
        );

        let mut set = ArchiveSet::new();
        set.add(&archive).expect("indexes");
        let index = PhashIndex::build(&set);

        assert_eq!(index.hashed_count(), 2);

        let wanted = crate::phash::hash_image(&png(40, 30, 91)).expect("hashes");
        let found = index
            .resolve(&set, wanted)
            .expect("resolves")
            .expect("matched");
        assert_eq!(found, png(40, 30, 91));
    }

    #[test]
    fn the_phash_index_decodes_only_compatible_aspect_ratios() {
        let dir = TempDir::new("phash-ratio");
        let archive = dir.zip(
            "data.zip",
            &[
                ("landscape.png", &png(40, 30, 3)),
                ("square.png", &png(40, 40, 5)),
                ("portrait.png", &png(30, 40, 7)),
            ],
        );

        let mut set = ArchiveSet::new();
        set.add(&archive).expect("indexes");
        let positions: Vec<_> = (0..set.entry_count()).collect();
        let index = PhashIndex::build_from_matching_dimensions(&set, &positions, &[(20, 15)]);

        assert_eq!(index.hashed_count(), 2);
        assert_eq!(index.filtered_count(), 1);
        assert_eq!(index.undecodable_count(), 0);
    }

    #[test]
    fn aspect_ratio_filter_keeps_swapped_dimensions_for_exif_orientation() {
        let dir = TempDir::new("phash-orientation");
        let archive = dir.zip("data.zip", &[("portrait.png", &png(30, 40, 7))]);

        let mut set = ArchiveSet::new();
        set.add(&archive).expect("indexes");
        let index = PhashIndex::build_from_matching_dimensions(&set, &[0], &[(20, 15)]);

        assert_eq!(index.hashed_count(), 1);
        assert_eq!(index.filtered_count(), 0);
    }

    #[test]
    fn aspect_ratio_filter_tolerates_rendition_rounding_only() {
        assert!(compatible_aspect_ratio((400, 300), (200, 149)));
        assert!(!compatible_aspect_ratio((400, 300), (200, 145)));
        assert!(!compatible_aspect_ratio((0, 300), (200, 150)));
    }

    #[test]
    fn the_phash_index_skips_what_it_cannot_decode() {
        // A PDF is counted, not hashed; a page that would have matched it is
        // downloaded instead, which is correct rather than a gap.
        let dir = TempDir::new("phash-pdf");
        let archive = dir.zip(
            "data.zip",
            &[
                ("scan.pdf", b"%PDF-1.4 not really"),
                ("photo.png", &png(20, 20, 5)),
            ],
        );

        let mut set = ArchiveSet::new();
        set.add(&archive).expect("indexes");
        let index = PhashIndex::build(&set);

        assert_eq!(index.hashed_count(), 1);
        assert_eq!(index.undecodable_count(), 1);
    }

    #[test]
    fn the_phash_index_declines_rather_than_guessing_between_twins() {
        // The same picture stored twice: the margin rule refuses both.
        let dir = TempDir::new("phash-twins");
        let archive = dir.zip(
            "data.zip",
            &[("a.png", &png(30, 30, 7)), ("b.png", &png(30, 30, 7))],
        );

        let mut set = ArchiveSet::new();
        set.add(&archive).expect("indexes");
        let index = PhashIndex::build(&set);

        let wanted = crate::phash::hash_image(&png(30, 30, 7)).expect("hashes");
        // Byte-identical twins: the same file stored twice, so either will do
        // — exactly what a size clash between duplicate uploads resolves to.
        assert!(
            index.locate(&set, wanted).expect("resolves").is_some(),
            "byte-identical duplicates are interchangeable"
        );
    }

    #[test]
    fn twins_that_are_not_byte_identical_are_declined() {
        // The clash that must never be resolved: two entries the hash cannot
        // separate whose bytes differ. The same picture re-encoded, or two
        // near-white scans of different documents — either way, choosing would
        // be a guess, so the caller downloads.
        let dir = TempDir::new("phash-differ");
        let same = png(30, 30, 7);
        let mut recoded = same.clone();
        // Same pixels, different file: append a PNG comment chunk so the bytes
        // differ while the decoded image does not.
        recoded.extend_from_slice(b"\x00\x00\x00\x00tEXtx");
        let archive = dir.zip("data.zip", &[("a.png", &same), ("b.png", &recoded)]);

        let mut set = ArchiveSet::new();
        set.add(&archive).expect("indexes");
        let index = PhashIndex::build(&set);

        let wanted = crate::phash::hash_image(&same).expect("hashes");
        assert!(
            index.locate(&set, wanted).expect("resolves").is_none(),
            "entries that differ must not be chosen between"
        );
    }

    #[test]
    fn media_extensions_are_recognised_case_insensitively() {
        assert!(looks_like_media("PANTIN_002.JPG"));
        assert!(looks_like_media("scan.pdf"));
        assert!(!looks_like_media("readme.txt"));
        assert!(!looks_like_media("no-extension"));
    }
}
