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
//! Matching is on exact size and nothing else. Filenames cannot be used —
//! they are upload names, unrelated to the deposit title — and a perceptual
//! hash would answer a harder question with a threshold, which silently
//! misattributes the scanned dossiers that make up a third of a real archive.
//! See `docs/specifications/geneanet-media-import.md` §5.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// File extensions that make an archive look like a media export.
///
/// Only used to warn "is it the right download?" — nothing is skipped for
/// failing this, because a deposit's original can be any type Geneanet let
/// through.
const IMAGE_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "bmp", "tif", "tiff", "webp", "pdf",
];

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
#[derive(Debug, Default)]
pub struct ArchiveSet {
    archives: Vec<ArchiveInfo>,
    by_size: HashMap<u64, Vec<Entry>>,
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
                .push(Entry { archive, index });
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

        self.by_size.retain(|_, entries| {
            entries.retain(|e| e.archive != removed);
            for entry in entries.iter_mut() {
                if entry.archive > removed {
                    entry.archive -= 1;
                }
            }
            !entries.is_empty()
        });
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

        let mut bytes = Vec::with_capacity(usize::try_from(zipped.size()).unwrap_or(0));
        zipped
            .read_to_end(&mut bytes)
            .with_context(|| format!("reading entry {} of {}", entry.index, info.path.display()))?;

        Ok(bytes)
    }
}

impl LocalOriginals for ArchiveSet {
    fn resolve(&self, size: u64) -> Result<Option<Vec<u8>>> {
        let candidates = self.by_size.get(&size).map_or(&[][..], Vec::as_slice);

        match candidates {
            [] => Ok(None),
            [only] => self.read(only).map(Some),
            [first, rest @ ..] => {
                // Same size, several entries: interchangeable only if they are
                // byte-identical, which is what a duplicate upload looks like.
                // Anything else defers to the network rather than guessing.
                let reference = self.read(first)?;
                for other in rest {
                    if self.read(other)? != reference {
                        return Ok(None);
                    }
                }
                Ok(Some(reference))
            }
        }
    }

    fn file_count(&self) -> usize {
        self.archives.iter().map(|a| a.file_count).sum()
    }
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

    /// A scratch directory that cleans itself up.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let path = std::env::temp_dir().join(format!("oxidgene-archive-{tag}"));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("creates");
            Self(path)
        }

        fn zip(&self, name: &str, entries: &[(&str, &[u8])]) -> PathBuf {
            let path = self.0.join(name);
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

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
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
        let path = dir.0.join("broken.zip");
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
    fn media_extensions_are_recognised_case_insensitively() {
        assert!(looks_like_media("PANTIN_002.JPG"));
        assert!(looks_like_media("scan.pdf"));
        assert!(!looks_like_media("readme.txt"));
        assert!(!looks_like_media("no-extension"));
    }
}
