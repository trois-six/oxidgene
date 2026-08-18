//! Saving what the login window collected, so it need only be collected once.
//!
//! Step 3 is the only part of the import that talks to Geneanet, and it is not
//! cheap in the way that matters: collecting the mapping is a handful of calls,
//! but measuring the deposits is one `HEAD` each — several hundred against a
//! real account. Re-running the wizard to try something out therefore means
//! re-running all of that, against a live account, for a result that has not
//! changed.
//!
//! So the window's output is saveable and reloadable. That buys three things:
//!
//! - **Testing without touching the account.** Collect once, then iterate.
//! - **A second machine.** Collect where the browser is, import where the
//!   archives are.
//! - **An import with no connection at all**, if the file was saved after step
//!   4: by then the media the archives could not account for have been fetched
//!   too, and they travel with it.
//! - **A record.** The mapping is the part that cannot be recovered from the
//!   exports (see `docs/specifications/geneanet-media-import.md` §1); having
//!   it on disk means it survives Geneanet changing its API.
//!
//! # One file, however far you got
//!
//! There is one format, not two. A file saved after step 3 carries the
//! collection and the deposit sizes; one saved after step 4 carries the fetched
//! media as well. The wizard looks at what it was given and asks for only what
//! is missing — so the same *Load* button covers "skip the collection" and
//! "skip Geneanet entirely", and neither needs the user to know which kind of
//! file they have.
//!
//! # A ZIP holding the JSON it used to be
//!
//! The file is a ZIP with `session.json` inside it, and the media beside that
//! as the files they are. Base64 inside JSON was the obvious first shape and
//! the wrong one: it inflates binary by a third, and an account with no data
//! archive has every medium in there.
//!
//! `session.json` *is* the JSON the window produced with a few fields added
//! beside it. [`crate::model::BrowserCollection`] ignores fields it does not
//! know, so it feeds [`crate::manifest_from_collection`] unchanged and there is
//! no second format to keep in step — unzip the file and the collection is
//! right there, readable.
//!
//! [`decode`] also accepts a **bare JSON file**: sessions saved before this
//! became a ZIP, and the raw output of a browser console script. Both still
//! carry the mapping, which is the part that matters.

use std::collections::HashMap;

use anyhow::{Context, Result, bail};
use serde_json::{Map, Value};

/// Bumped only when a field changes meaning, never when one is added.
///
/// Additions need no bump: readers ignore what they do not know, so an old
/// file stays loadable and a new file stays useful to an old build.
pub const VERSION: u32 = 1;

/// Key the version is written under, namespaced so it cannot collide with
/// anything Geneanet later adds to its own payload.
const VERSION_KEY: &str = "oxidgene_session_version";
const SIZES_KEY: &str = "oxidgene_deposit_sizes";
const ACCOUNT_KEY: &str = "oxidgene_account";
const MEDIA_KEY: &str = "oxidgene_media";

/// Everything step 3 produced.
#[derive(Debug, Clone, Default)]
pub struct Session {
    /// The collection JSON, exactly as the window emitted it.
    pub collection: String,
    /// Byte length of each single-page deposit — the expensive half, one
    /// `HEAD` per deposit.
    pub deposit_sizes: HashMap<i64, u64>,
    pub account: Option<String>,
    /// Media the login window fetched, keyed by URL and base64-encoded.
    ///
    /// Empty in a file saved after step 3, populated in one saved after step
    /// 4 — and that difference is the whole point. With it, an import needs no
    /// Geneanet connection at all: the collection says what goes where, the
    /// sizes match the archives, and these are the pieces the archives could
    /// not account for. Without it the wizard still has to open the window at
    /// step 4 to gather them.
    ///
    /// It is what makes the file large — a few hundred renditions — so it is
    /// written only when there is something to write.
    pub media: HashMap<String, String>,
}

/// Name of the metadata entry inside the archive.
const MANIFEST_ENTRY: &str = "session.json";

/// Directory the media sit in, one file each.
const MEDIA_DIR: &str = "media";

/// Writes a session as a ZIP.
///
/// `session.json` carries the collection and the sizes; each medium is written
/// beside it under `media/`, stored rather than deflated — they are already
/// compressed formats, so deflating them costs CPU to save nothing.
///
/// # Errors
///
/// Returns `Err` if the collection is not a JSON object, or the archive cannot
/// be assembled.
pub fn encode(session: &Session) -> Result<Vec<u8>> {
    use base64::Engine as _;
    use std::io::Write as _;

    // Names are assigned here and recorded in the manifest, so a URL never has
    // to survive being turned into a filename.
    let mut names: Map<String, Value> = Map::new();
    let mut bodies: Vec<(String, Vec<u8>)> = Vec::new();
    for (index, (url, encoded)) in session.media.iter().enumerate() {
        let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(encoded) else {
            continue;
        };
        let name = format!("{MEDIA_DIR}/{index:05}{}", extension_of(url));
        names.insert(url.clone(), Value::from(name.clone()));
        bodies.push((name, bytes));
    }

    let manifest = encode_manifest(session, names)?;

    let mut buffer = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buffer);
        let deflated: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
        let stored = deflated.compression_method(zip::CompressionMethod::Stored);

        zip.start_file(MANIFEST_ENTRY, deflated)
            .context("starting the session manifest")?;
        zip.write_all(manifest.as_bytes())
            .context("writing the session manifest")?;

        for (name, bytes) in bodies {
            zip.start_file(&name, stored)
                .with_context(|| format!("starting {name}"))?;
            zip.write_all(&bytes)
                .with_context(|| format!("writing {name}"))?;
        }

        zip.finish().context("finishing the session archive")?;
    }

    Ok(buffer.into_inner())
}

/// A medium's extension, taken from its URL and kept only if it looks like one.
///
/// Cosmetic — the manifest is what maps a URL to its entry — but it makes an
/// unzipped session browsable instead of a directory of numbers.
fn extension_of(url: &str) -> String {
    url.split('?')
        .next()
        .and_then(|path| path.rsplit('.').next())
        .filter(|ext| {
            ext.len() <= 5 && !ext.is_empty() && ext.chars().all(|c| c.is_ascii_alphanumeric())
        })
        .map_or_else(String::new, |ext| format!(".{}", ext.to_ascii_lowercase()))
}

/// Builds `session.json`.
fn encode_manifest(session: &Session, media: Map<String, Value>) -> Result<String> {
    let parsed: Value =
        serde_json::from_str(&session.collection).context("the collection is not valid JSON")?;
    let Value::Object(mut object) = parsed else {
        bail!("the collection is not a JSON object");
    };

    object.insert(VERSION_KEY.to_string(), Value::from(VERSION));
    object.insert(
        SIZES_KEY.to_string(),
        Value::Object(
            session
                .deposit_sizes
                .iter()
                // Keys are strings because JSON object keys are, which is also
                // how the window reports them.
                .map(|(deposit, size)| (deposit.to_string(), Value::from(*size)))
                .collect::<Map<_, _>>(),
        ),
    );
    if let Some(account) = &session.account {
        object.insert(ACCOUNT_KEY.to_string(), Value::from(account.clone()));
    }
    // URL → the entry holding it, not the bytes themselves.
    if !media.is_empty() {
        object.insert(MEDIA_KEY.to_string(), Value::Object(media));
    }

    serde_json::to_string(&Value::Object(object)).context("serialising the session")
}

/// Reads a session back, from either shape the wizard has ever written.
///
/// A ZIP is the current one. A bare JSON file is accepted too: sessions saved
/// before the container changed, and the raw output of a browser console
/// script. Both carry the mapping, which is the part that cannot be recovered
/// any other way.
///
/// Told apart by content rather than by extension, because a file that has
/// been renamed is still the file it was.
///
/// # Errors
///
/// Returns `Err` if the bytes are neither, or hold no collection this crate
/// can read.
pub fn decode(bytes: &[u8]) -> Result<Session> {
    if bytes.starts_with(b"PK\x03\x04") {
        return decode_zip(bytes);
    }

    let json = std::str::from_utf8(bytes).context("the session file is not UTF-8")?;
    decode_manifest(json, &HashMap::new())
}

/// Reads the ZIP shape: `session.json` plus the media it names.
fn decode_zip(bytes: &[u8]) -> Result<Session> {
    use base64::Engine as _;
    use std::io::Read as _;

    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .context("the session file is not a readable archive")?;

    let mut manifest = String::new();
    zip.by_name(MANIFEST_ENTRY)
        .with_context(|| format!("the archive holds no {MANIFEST_ENTRY}"))?
        .read_to_string(&mut manifest)
        .context("reading the session manifest")?;

    // Read every medium first: the manifest names them, and a name that is not
    // there is simply a medium the import will have to do without.
    let mut bodies: HashMap<String, String> = HashMap::new();
    for index in 0..zip.len() {
        let Ok(mut entry) = zip.by_index(index) else {
            continue;
        };
        let name = entry.name().to_string();
        if name == MANIFEST_ENTRY || entry.is_dir() {
            continue;
        }
        let mut body = Vec::new();
        if entry.read_to_end(&mut body).is_ok() {
            bodies.insert(
                name,
                base64::engine::general_purpose::STANDARD.encode(&body),
            );
        }
    }

    decode_manifest(&manifest, &bodies)
}

/// Reads `session.json`, resolving the media names against `bodies`.
fn decode_manifest(json: &str, bodies: &HashMap<String, String>) -> Result<Session> {
    let parsed: Value = serde_json::from_str(json).context("the session file is not valid JSON")?;
    let Value::Object(object) = &parsed else {
        bail!("the session file is not a JSON object");
    };

    if let Some(version) = object.get(VERSION_KEY).and_then(Value::as_u64)
        && version > u64::from(VERSION)
    {
        bail!(
            "this session file was written by a newer version of OxidGene \
             (format {version}, this build reads {VERSION})"
        );
    }

    // Parsing it proves the file is a collection rather than some other JSON,
    // and does it through the same reader the import will use.
    crate::manifest_from_collection(json)
        .context("the session file does not hold a Geneanet collection")?;

    let deposit_sizes = object
        .get(SIZES_KEY)
        .and_then(Value::as_object)
        .map(|sizes| {
            sizes
                .iter()
                .filter_map(|(deposit, size)| Some((deposit.parse::<i64>().ok()?, size.as_u64()?)))
                .collect()
        })
        .unwrap_or_default();

    let media = object
        .get(MEDIA_KEY)
        .and_then(Value::as_object)
        .map(|media| {
            media
                .iter()
                .filter_map(|(url, entry)| {
                    let entry = entry.as_str()?;
                    // A file written before the container changed holds the
                    // bytes inline; the ZIP holds a name to look up.
                    let body = bodies
                        .get(entry)
                        .cloned()
                        .unwrap_or_else(|| entry.to_string());
                    Some((url.clone(), body))
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(Session {
        collection: json.to_string(),
        deposit_sizes,
        account: object
            .get(ACCOUNT_KEY)
            .and_then(Value::as_str)
            .map(str::to_string),
        media,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collection() -> String {
        r#"{"deposits":[{"id":1,"title":"t","type":"portraits","private":true,
            "views":[{"id":10,"page":1,"files":{"normal":"/n.jpg"}}]}],
            "references":[],"view_references":{}}"#
            .to_string()
    }

    fn session(media: HashMap<String, String>) -> Session {
        Session {
            collection: collection(),
            deposit_sizes: HashMap::from([(1, 69122), (2, 4096)]),
            account: Some("someone".into()),
            media,
        }
    }

    #[test]
    fn a_session_round_trips_through_the_archive() {
        let original = session(HashMap::new());

        let restored = decode(&encode(&original).expect("encodes")).expect("decodes");

        assert_eq!(restored.deposit_sizes, original.deposit_sizes);
        assert_eq!(restored.account.as_deref(), Some("someone"));
    }

    #[test]
    fn media_travel_as_files_rather_than_base64() {
        // The reason for the container: base64 inflates binary by a third, and
        // an account with no data archive puts every medium in here.
        let media = HashMap::from([(
            "https://gw.geneanet.org/a/medium.jpg".to_string(),
            // "hello" — recognisable in the raw archive bytes if stored as
            // bytes, absent if it were re-encoded as base64.
            "aGVsbG8=".to_string(),
        )]);

        let archive = encode(&session(media)).expect("encodes");

        assert!(
            archive.windows(5).any(|w| w == b"hello"),
            "the medium should be stored as its own bytes"
        );
        assert!(
            !archive.windows(8).any(|w| w == b"aGVsbG8="),
            "the base64 form should not be in the archive"
        );
    }

    #[test]
    fn media_come_back_keyed_by_their_url() {
        let media = HashMap::from([(
            "https://gw.geneanet.org/a/medium.jpg".to_string(),
            "aGVsbG8=".to_string(),
        )]);

        let restored = decode(&encode(&session(media)).expect("encodes")).expect("decodes");

        assert_eq!(
            restored.media.get("https://gw.geneanet.org/a/medium.jpg"),
            Some(&"aGVsbG8=".to_string())
        );
    }

    #[test]
    fn the_manifest_is_readable_on_its_own() {
        // Unzipping a session should show the collection, not an opaque blob:
        // it is the one part of this that cannot be recovered any other way.
        use std::io::Read as _;
        let archive = encode(&session(HashMap::new())).expect("encodes");

        let mut zip =
            zip::ZipArchive::new(std::io::Cursor::new(&archive[..])).expect("is an archive");
        let mut manifest = String::new();
        zip.by_name(MANIFEST_ENTRY)
            .expect("holds session.json")
            .read_to_string(&mut manifest)
            .expect("reads");

        let rebuilt = crate::manifest_from_collection(&manifest).expect("still a collection");
        assert_eq!(rebuilt.deposit_count, 1);
    }

    #[test]
    fn a_bare_json_file_still_loads() {
        // Sessions saved before the container changed, and the raw output of a
        // browser console script. Both carry the mapping.
        let restored = decode(collection().as_bytes()).expect("decodes");

        assert!(restored.deposit_sizes.is_empty());
        assert!(restored.media.is_empty());
    }

    #[test]
    fn a_session_with_no_media_names_none() {
        let archive = encode(&session(HashMap::new())).expect("encodes");
        let restored = decode(&archive).expect("decodes");

        assert!(restored.media.is_empty());
    }

    #[test]
    fn a_file_from_a_newer_build_is_refused_by_name() {
        let mut object: Map<String, Value> = serde_json::from_str(&collection()).expect("parses");
        object.insert(VERSION_KEY.to_string(), Value::from(VERSION + 1));
        let future = serde_json::to_string(&Value::Object(object)).expect("serialises");

        let err = decode(future.as_bytes()).expect_err("refused");
        assert!(err.to_string().contains("newer version"), "got {err}");
    }

    #[test]
    fn something_that_is_not_a_collection_is_refused() {
        // A user picking the wrong file should be told, not left with an
        // import that silently attaches nothing.
        assert!(decode(br#"{"hello":"world"}"#).is_err());
        assert!(decode(b"not json at all").is_err());
        assert!(decode(b"PK\x03\x04 but not really an archive").is_err());
    }

    #[test]
    fn deposit_sizes_survive_the_string_keys_json_forces() {
        let mut original = session(HashMap::new());
        original.deposit_sizes = HashMap::from([(90571786, 725763922)]);

        let restored = decode(&encode(&original).expect("encodes")).expect("decodes");

        assert_eq!(restored.deposit_sizes.get(&90571786), Some(&725763922));
    }

    #[test]
    fn an_entry_name_keeps_the_extension_its_url_had() {
        assert_eq!(
            extension_of("https://gw.geneanet.org/a/medium.jpg?t=1"),
            ".jpg"
        );
        assert_eq!(extension_of("https://x/a/normal.PNG"), ".png");
        assert_eq!(extension_of("https://x/media/download/?deposits[]=1"), "");
    }
}
