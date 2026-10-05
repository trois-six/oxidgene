//! Steps 1 to 3 of the live checks (Archive Portals §9.1) for the
//! collections of one archive that only a browser reaches, their requests
//! run in the page of the Playwright check that starts this binary
//! (`e2e/archives/live.spec.ts`, `just archives-live`).
//!
//! `archives-live-bridge <archive id>` exchanges JSON lines on its standard
//! streams (`oxidgene_archives::live::bridge`) and ends with the report of
//! those collections. `archives-live-bridge --list [archive id]` prints the
//! ids of the archives the live checks visit, one per line.

use std::io::{BufReader, stdin, stdout};
use std::pin::pin;
use std::process::ExitCode;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::thread::{self, Thread};

use oxidgene_archives::ArchiveRegistry;
use oxidgene_archives::live::{self, bridge::BridgeTransport};

/// Wakes the thread that drives the check.
struct Unpark(Thread);

impl Wake for Unpark {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}

/// Drives a future on this thread. The bridge's requests block on the
/// standard streams, so the check needs no runtime.
fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let waker = Waker::from(Arc::new(Unpark(thread::current())));
    let mut context = Context::from_waker(&waker);
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
            return output;
        }
        thread::park();
    }
}

fn main() -> ExitCode {
    let registry = ArchiveRegistry::embedded();
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let (list, only) = match arguments.as_slice() {
        [flag, rest @ ..] if flag == "--list" => (true, rest.first()),
        [id] => (false, Some(id)),
        _ => {
            eprintln!("usage: archives-live-bridge <archive id> | --list [archive id]");
            return ExitCode::from(2);
        }
    };
    let archives = match live::archives(registry, only.map(String::as_str)) {
        Ok(archives) => archives,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
    };
    if list {
        for archive in archives {
            println!("{}", archive.id);
        }
        return ExitCode::SUCCESS;
    }

    let archive = archives[0];
    let transport = BridgeTransport::new(BufReader::new(stdin()), stdout());
    let mut collections = Vec::new();
    // Checked by the native test instead, whose report the page reads.
    let mut native = Vec::new();
    for (index, collection) in archive.collections.iter().enumerate() {
        if live::needs_browser(registry, collection) {
            collections.push(block_on(live::check_collection(
                registry, archive, index, &transport,
            )));
        } else {
            native.push(index);
        }
    }
    let report = serde_json::json!({
        "kind": "report",
        "collections": collections,
        "native": native,
    });
    match transport.send(&report) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("could not send the report: {error}");
            ExitCode::FAILURE
        }
    }
}
