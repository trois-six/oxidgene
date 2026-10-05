//! Steps 1 to 3 of the live checks (Archive Portals §9.1) for every
//! collection a plain HTTP client may reach, over the `native` transport and
//! its identifying `User-Agent`, one request at a time.
//!
//! Opt-in: it contacts the real portals, so it is `#[ignore]`d and runs only
//! through `just archives-live [archive id]`, which then runs the browser
//! steps in Playwright. `OXIDGENE_LIVE_ARCHIVE` names the one archive to
//! check; the reports land in `native.json` under `OXIDGENE_LIVE_REPORT_DIR`
//! (default `target/archives-live`), where the Playwright check reads the
//! openings. A drift fails the test; an unreachable portal is only reported.

use std::path::PathBuf;

use oxidgene_archives::live::{self, ArchiveReport, Outcome};
use oxidgene_archives::{ArchiveRegistry, NativeTransport};

fn report_dir() -> PathBuf {
    std::env::var_os("OXIDGENE_LIVE_REPORT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/archives-live")
        })
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "contacts the real archive portals: run by `just archives-live`"]
async fn native_collections_resolve_on_their_portals() {
    let registry = ArchiveRegistry::embedded();
    let only = std::env::var("OXIDGENE_LIVE_ARCHIVE")
        .ok()
        .filter(|id| !id.is_empty());
    let archives =
        live::archives(registry, only.as_deref()).unwrap_or_else(|error| panic!("{error}"));
    let transport = NativeTransport::new().expect("an HTTP client");

    let mut reports = Vec::new();
    for archive in archives {
        let mut collections = Vec::new();
        for (index, collection) in archive.collections.iter().enumerate() {
            if live::needs_browser(registry, collection) {
                continue;
            }
            collections.push(live::check_collection(registry, archive, index, &transport).await);
        }
        let report = ArchiveReport::new(archive, collections);
        eprintln!(
            "{}: {:?} ({} native collections)",
            report.archive,
            report.outcome,
            report.collections.len()
        );
        reports.push(report);
    }

    let dir = report_dir();
    std::fs::create_dir_all(&dir).expect("the report directory");
    let json = serde_json::json!({ "archives": reports });
    std::fs::write(
        dir.join("native.json"),
        serde_json::to_string_pretty(&json).expect("a report") + "\n",
    )
    .expect("the native report");

    let drifted: Vec<_> = reports
        .iter()
        .flat_map(|report| &report.collections)
        .filter(|collection| collection.outcome == Outcome::Drift)
        .map(|collection| format!("{} {:?}", collection.collection, collection.failure))
        .collect();
    assert!(drifted.is_empty(), "drift:\n{}", drifted.join("\n"));
}
