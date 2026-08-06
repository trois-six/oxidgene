//! Gets the bytes of a medium, preferring a copy you already have.
//!
//! Geneanet's "download all my data" archive holds the originals under their
//! upload names, which say nothing about which deposit they came from. Rather
//! than guess from the name — the names collide and often bear no relation to
//! the deposit title — we ask the API for each deposit's exact byte length with
//! a `HEAD`, and match that against the local files. An exact size match on the
//! same original is a fingerprint: on a 613-file archive it produced 607
//! distinct sizes, and every collision was the same file uploaded twice.
//!
//! Where a size cannot decide, we download instead of guessing. Nothing here
//! ever attaches a file it is not sure about.

use std::collections::HashMap;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::client::Client;
use super::model::{ManifestDeposit, ManifestView};

/// Local originals, indexed by exact byte length.
#[derive(Debug, Default)]
pub struct LocalIndex {
    by_size: HashMap<u64, Vec<PathBuf>>,
    file_count: usize,
}

impl LocalIndex {
    /// Indexes every file directly inside `dir`.
    pub fn build(dir: &Path) -> Result<Self> {
        let mut by_size: HashMap<u64, Vec<PathBuf>> = HashMap::new();
        let mut file_count = 0;

        let entries =
            std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))?;

        for entry in entries {
            let entry = entry.with_context(|| format!("listing {}", dir.display()))?;
            let metadata = entry.metadata()?;
            if !metadata.is_file() {
                continue;
            }
            by_size
                .entry(metadata.len())
                .or_default()
                .push(entry.path());
            file_count += 1;
        }

        Ok(Self {
            by_size,
            file_count,
        })
    }

    pub fn file_count(&self) -> usize {
        self.file_count
    }

    /// Picks the local file of exactly this size, if that can be done safely.
    ///
    /// With several candidates, they are only interchangeable if their contents
    /// match — which is what a duplicate upload looks like. Otherwise this
    /// returns `None` and the caller downloads rather than picking one.
    fn resolve(&self, size: u64) -> Result<Option<&Path>> {
        let candidates = self.by_size.get(&size).map_or(&[][..], Vec::as_slice);

        match candidates {
            [] => Ok(None),
            [only] => Ok(Some(only)),
            [first, rest @ ..] => {
                let reference =
                    std::fs::read(first).with_context(|| format!("reading {}", first.display()))?;
                for other in rest {
                    let bytes = std::fs::read(other)
                        .with_context(|| format!("reading {}", other.display()))?;
                    if bytes != reference {
                        // Same size, different content: a name or a hash would
                        // both be guesses, so defer to the network.
                        return Ok(None);
                    }
                }
                Ok(Some(first))
            }
        }
    }
}

/// Where a medium's bytes came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// Matched against the local archive by exact size — no bytes transferred.
    Local,
    /// The original, downloaded from `/media/download`.
    Original,
    /// A downsized rendition. The only per-page URL Geneanet exposes, so this
    /// is what a page of a multi-page deposit falls back to.
    Rendition,
}

/// Tally of where a run's bytes came from, for the closing report.
#[derive(Debug, Default, Clone, Copy)]
pub struct Sources {
    pub local: usize,
    pub original: usize,
    pub rendition: usize,
}

impl Sources {
    fn record(&mut self, origin: Origin) {
        match origin {
            Origin::Local => self.local += 1,
            Origin::Original => self.original += 1,
            Origin::Rendition => self.rendition += 1,
        }
    }
}

/// Resolves media bytes, reusing the local archive wherever it can.
pub struct MediaSource {
    client: Client,
    local: Option<LocalIndex>,
    sources: Sources,
    /// Fetch multi-page originals by pulling the whole deposit archive.
    multipage_originals: bool,
    /// The archive of the deposit currently being read, so a deposit with
    /// several linked pages is downloaded once rather than once per page.
    cached_archive: Option<(i64, Vec<u8>)>,
}

impl MediaSource {
    pub fn new(client: Client, local: Option<LocalIndex>, multipage_originals: bool) -> Self {
        Self {
            client,
            local,
            sources: Sources::default(),
            multipage_originals,
            cached_archive: None,
        }
    }

    pub fn sources(&self) -> Sources {
        self.sources
    }

    /// Fetches the bytes for one view of one deposit.
    ///
    /// A deposit holding a single page *is* the file, so its exact length can
    /// be asked for with a `HEAD` and matched locally. A multi-page deposit
    /// downloads as an archive whose length says nothing about any one page —
    /// Geneanet streams it without a `Content-Length` at all — so those go
    /// through the per-page rendition URL instead.
    pub async fn bytes(
        &mut self,
        deposit: &ManifestDeposit,
        view: &ManifestView,
    ) -> Result<(Vec<u8>, Origin)> {
        let single_page = deposit.views.len() == 1;

        if single_page
            && let Some(local) = &self.local
            && let Some(size) = self.client.content_length(deposit.id).await?
            && let Some(path) = local.resolve(size)?
        {
            let bytes =
                std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
            self.sources.record(Origin::Local);
            return Ok((bytes, Origin::Local));
        }

        if single_page {
            let (bytes, _) = self.client.download_deposit(deposit.id).await?;
            self.sources.record(Origin::Original);
            return Ok((bytes, Origin::Original));
        }

        let page = usize::try_from(view.page.unwrap_or(1)).unwrap_or(1);

        if self.multipage_originals {
            let archive = self.archive(deposit.id).await?;
            let bytes = archive_entry(archive, page)
                .with_context(|| format!("extracting page {page} of deposit {}", deposit.id))?;
            self.sources.record(Origin::Original);
            return Ok((bytes, Origin::Original));
        }

        let url = rendition_url(view).with_context(|| {
            format!(
                "deposit {} page {page} has no rendition URL to fall back on",
                deposit.id
            )
        })?;
        let bytes = self.client.download_url(&url).await?;
        self.sources.record(Origin::Rendition);
        Ok((bytes, Origin::Rendition))
    }

    /// The deposit's archive, downloading it unless it is the one already held.
    ///
    /// Attachments arrive grouped by deposit, so holding a single archive is
    /// enough to stop a deposit with several linked pages from being pulled
    /// once per page.
    async fn archive(&mut self, deposit_id: i64) -> Result<&[u8]> {
        if self
            .cached_archive
            .as_ref()
            .is_none_or(|(id, _)| *id != deposit_id)
        {
            let (bytes, _) = self.client.download_deposit(deposit_id).await?;
            self.cached_archive = Some((deposit_id, bytes));
        }

        Ok(&self
            .cached_archive
            .as_ref()
            .expect("just populated above")
            .1)
    }
}

/// Picks the largest rendition a view exposes.
fn rendition_url(view: &ManifestView) -> Option<String> {
    for rendition in ["normal", "screen", "medium", "thumbnail"] {
        if let Some(path) = view.files.get(rendition) {
            // Manifest paths are host-relative and served from the gw subdomain,
            // not the www one the API lives on.
            return Some(if path.starts_with("http") {
                path.clone()
            } else {
                format!("https://gw.geneanet.org{path}")
            });
        }
    }
    None
}

/// Reads one entry out of a multi-page deposit's archive.
///
/// Entries come out in page order, which is the only correspondence Geneanet
/// gives us: the archive names its entries after the original uploads, which
/// carry no page number.
fn archive_entry(archive: &[u8], page: usize) -> Result<Vec<u8>> {
    let mut zip = zip::ZipArchive::new(Cursor::new(archive)).context("reading the archive")?;

    let index = page.saturating_sub(1);
    anyhow::ensure!(
        index < zip.len(),
        "the archive holds {} entries, so there is no page {page}",
        zip.len()
    );

    let mut entry = zip.by_index(index).context("opening the archive entry")?;
    let mut bytes = Vec::with_capacity(usize::try_from(entry.size()).unwrap_or(0));
    entry
        .read_to_end(&mut bytes)
        .context("reading the archive entry")?;

    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn write(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, bytes).expect("writes");
        path
    }

    /// A scratch directory that cleans itself up.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let path = std::env::temp_dir().join(format!("oxidgene-media-{tag}"));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("creates");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn matches_a_unique_size() {
        let dir = TempDir::new("unique");
        let expected = write(dir.path(), "a.jpg", b"hello");
        write(dir.path(), "b.jpg", b"a longer file");

        let index = LocalIndex::build(dir.path()).expect("indexes");

        assert_eq!(index.file_count(), 2);
        assert_eq!(
            index.resolve(5).expect("resolves"),
            Some(expected.as_path())
        );
    }

    #[test]
    fn a_size_nothing_has_resolves_to_nothing() {
        let dir = TempDir::new("absent");
        write(dir.path(), "a.jpg", b"hello");

        let index = LocalIndex::build(dir.path()).expect("indexes");

        assert_eq!(index.resolve(999).expect("resolves"), None);
    }

    #[test]
    fn duplicate_uploads_are_interchangeable() {
        // The real collision case: `Photo.jpg` and `Photo (1).jpg`, same size
        // and byte-identical. Either will do.
        let dir = TempDir::new("duplicate");
        write(dir.path(), "photo.jpg", b"same bytes");
        write(dir.path(), "photo (1).jpg", b"same bytes");

        let index = LocalIndex::build(dir.path()).expect("indexes");

        assert!(index.resolve(10).expect("resolves").is_some());
    }

    #[test]
    fn a_size_clash_between_different_files_resolves_to_nothing() {
        // This is the case the user was right to worry about. Rather than pick
        // one, or reach for a perceptual hash and its threshold, we decline —
        // and the caller downloads.
        let dir = TempDir::new("clash");
        write(dir.path(), "a.jpg", b"abcdefghij");
        write(dir.path(), "b.jpg", b"0123456789");

        let index = LocalIndex::build(dir.path()).expect("indexes");

        assert_eq!(index.resolve(10).expect("resolves"), None);
    }

    #[test]
    fn an_empty_directory_indexes_cleanly() {
        let dir = TempDir::new("empty");

        let index = LocalIndex::build(dir.path()).expect("indexes");

        assert_eq!(index.file_count(), 0);
        assert_eq!(index.resolve(0).expect("resolves"), None);
    }

    fn view_with(files: &[(&str, &str)]) -> ManifestView {
        ManifestView {
            id: 1,
            page: Some(1),
            files: files
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect::<BTreeMap<_, _>>(),
            references: Vec::new(),
        }
    }

    #[test]
    fn prefers_the_largest_rendition_and_absolutises_the_path() {
        let view = view_with(&[("thumbnail", "/t.jpg"), ("normal", "/n.jpg")]);

        assert_eq!(
            rendition_url(&view).as_deref(),
            Some("https://gw.geneanet.org/n.jpg")
        );
    }

    #[test]
    fn falls_down_the_rendition_ladder() {
        let view = view_with(&[("thumbnail", "/t.jpg")]);

        assert_eq!(
            rendition_url(&view).as_deref(),
            Some("https://gw.geneanet.org/t.jpg")
        );
        assert_eq!(rendition_url(&view_with(&[])), None);
    }

    #[test]
    fn leaves_an_absolute_rendition_url_alone() {
        let view = view_with(&[("normal", "https://gw.geneanet.org/n.jpg")]);

        assert_eq!(
            rendition_url(&view).as_deref(),
            Some("https://gw.geneanet.org/n.jpg")
        );
    }

    #[test]
    fn reads_a_page_out_of_an_archive_by_position() {
        let mut buffer = Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut buffer);
            let options: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
            for (name, bytes) in [
                ("page_1.jpg", &b"first"[..]),
                ("page_2.jpg", &b"second"[..]),
            ] {
                use std::io::Write;
                writer.start_file(name, options).expect("starts");
                writer.write_all(bytes).expect("writes");
            }
            writer.finish().expect("finishes");
        }
        let archive = buffer.into_inner();

        assert_eq!(archive_entry(&archive, 1).expect("page 1"), b"first");
        assert_eq!(archive_entry(&archive, 2).expect("page 2"), b"second");
        assert!(archive_entry(&archive, 3).is_err());
    }
}
