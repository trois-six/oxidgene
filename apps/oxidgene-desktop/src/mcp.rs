//! `oxidgene-desktop mcp`: the trees served to an MCP client over stdio.
//!
//! Headless, and deliberately smaller than the application: no HTTP listener,
//! no WebView, no background job worker, no purge worker and no job recovery —
//! the desktop window, which may be open on the same database at the same
//! time, owns all of those. Requeuing jobs here would restart an import that
//! window is still running. See `docs/specifications/mcp.md` §4.

use std::path::Path;

use oxidgene_db::repo::{connect, run_migrations};

/// Serve MCP until the client closes standard input. Returns the exit status.
///
/// Failures are reported on standard error with a generic message: no path,
/// no ID, nothing from the tree.
pub fn run(db_path: &Path) -> i32 {
    // An assistant must never be the thing that creates an empty database:
    // that is the application's first launch, not a side effect of a client
    // configured on a machine where it has never run.
    if !db_path.is_file() {
        eprintln!("oxidgene-desktop mcp: no OxidGene database; open the application once first");
        return 1;
    }
    let database_url = format!("sqlite://{}?mode=rw", db_path.display());

    let runtime = match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime,
        Err(_) => {
            eprintln!("oxidgene-desktop mcp: failed to start the async runtime");
            return 1;
        }
    };
    runtime.block_on(async {
        let Ok(db) = connect(&database_url).await else {
            eprintln!("oxidgene-desktop mcp: failed to open the database");
            return 1;
        };
        // The same single migration the application applies at startup; on a
        // database it has already opened this changes nothing.
        if run_migrations(&db).await.is_err() {
            eprintln!("oxidgene-desktop mcp: failed to prepare the database");
            return 1;
        }
        match oxidgene_api::mcp::serve_stdio(db).await {
            Ok(()) => 0,
            Err(_) => {
                eprintln!("oxidgene-desktop mcp: the session ended with an error");
                1
            }
        }
    })
}
