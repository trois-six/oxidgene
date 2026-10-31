//! How the process gives back the memory its media work used.
//!
//! An import reads files of several megabytes and decodes images of tens of
//! megabytes, on a dozen threads at once. glibc's `malloc` maps a block that
//! large on its own at first, but each time one is freed it raises that
//! threshold to the block's size (up to 32 MiB), and the threshold at which
//! it trims an arena's heap with it. From then on such buffers come out of
//! the per-thread arenas, and what they free stays there: `malloc_trim`
//! hands back free pages inside an arena's heap, but never the free space at
//! the top of a thread's own arena. On a reference tree of 10,000 people and
//! 632 pictures, that is what made a Geneanet import peak at 1.8 to 2.0 GiB
//! and leave the process holding 1.2 to 1.4 GiB once it had finished.
//!
//! [`tune`] pins the threshold, which also pins the trimming one at its
//! 128 KiB default: every block of [`MMAP_THRESHOLD`] or more is mapped when
//! allocated and unmapped when freed, and an arena's free top goes back as it
//! frees. [`release_free_memory`] then hands back what the arenas still hold
//! after a background job, and [`release_when_idle`] after a burst of
//! requests. The same import peaks under 600 MiB and leaves 70 MiB.
//! Elsewhere — macOS, Windows, musl — the allocator calls do nothing: those
//! allocators return large blocks on their own.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;

/// Allocations of this size or more are mapped, and unmapped when freed.
///
/// A mebibyte is far above what the database layer and the request handlers
/// allocate, so their small allocations keep the arenas' speed, and below a
/// photograph's file or its decoded pixels, which no longer outlive their
/// use. Mapping and trimming cost system calls and fresh pages: on the
/// reference tree, a few seconds of system time over a whole import and a
/// GEDZIP round trip, within the noise of their wall time. A threshold of
/// 4 MiB, or a trimming threshold of 8 MiB, saved some of those seconds but
/// let the import peak 75 to 150 MiB higher.
pub const MMAP_THRESHOLD: usize = 1024 * 1024;

/// Pin the allocator's mapping threshold at [`MMAP_THRESHOLD`]. Each binary
/// calls it first thing in `main`; whether it applied.
pub fn tune() -> bool {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    {
        let threshold = libc::c_int::try_from(MMAP_THRESHOLD).unwrap_or(libc::c_int::MAX);
        // SAFETY: `mallopt` only sets an allocator parameter, under the
        // allocator's own lock; it touches no memory of the caller's.
        unsafe { libc::mallopt(libc::M_MMAP_THRESHOLD, threshold) == 1 }
    }
    #[cfg(not(all(target_os = "linux", target_env = "gnu")))]
    {
        false
    }
}

/// Return the free memory the allocator's arenas hold to the system.
///
/// Called after each background job, which is when a process has just
/// stopped needing the most memory it will hold. It costs a walk over the
/// arenas, milliseconds against a job's seconds.
pub fn release_free_memory() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    // SAFETY: `malloc_trim` only releases pages the allocator holds free,
    // under its own locks; it touches no memory of the caller's.
    unsafe {
        libc::malloc_trim(0);
    }
}

/// The requests the process is serving.
static IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);
/// The requests the process has started serving.
static STARTED: AtomicU64 = AtomicU64::new(0);
/// The releases [`release_when_idle`] has made, for its test.
static RELEASES: AtomicU64 = AtomicU64::new(0);

/// How long the process serves no request before [`release_when_idle`]
/// hands back what the last ones freed.
pub const IDLE_RELEASE_DELAY: Duration = Duration::from_millis(500);

/// Middleware handing back to the system, once the process has served no
/// request for [`IDLE_RELEASE_DELAY`], the free memory requests left in the
/// allocator's arenas.
///
/// A request reading a whole tree — the statistics, the anomalies, the
/// duplicates, a deep pedigree — decodes every projection, tens of megabytes
/// in allocations far below [`MMAP_THRESHOLD`], spread over the arenas of
/// the threads that served it. Freed, they stay in those arenas: on a tree of
/// 10,000 persons the server held some 100 MB more after a statistics page
/// than before it, and kept it, each tree-wide page adding its own share.
/// One release after a burst of requests gives it back, a walk over the
/// arenas costing milliseconds, and never runs while requests keep coming.
pub async fn release_when_idle(request: Request, next: Next) -> Response {
    let _serving = Serving::start();
    next.run(request).await
}

/// One request being served, counted until it is answered or dropped — a
/// client gone before the answer must not leave the process busy forever.
struct Serving;

impl Serving {
    fn start() -> Self {
        STARTED.fetch_add(1, Ordering::AcqRel);
        IN_FLIGHT.fetch_add(1, Ordering::AcqRel);
        Self
    }
}

impl Drop for Serving {
    fn drop(&mut self) {
        if IN_FLIGHT.fetch_sub(1, Ordering::AcqRel) != 1 {
            return;
        }
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let started = STARTED.load(Ordering::Acquire);
        runtime.spawn(async move {
            tokio::time::sleep(IDLE_RELEASE_DELAY).await;
            let idle = IN_FLIGHT.load(Ordering::Acquire) == 0
                && STARTED.load(Ordering::Acquire) == started;
            if idle {
                RELEASES.fetch_add(1, Ordering::AcqRel);
                // A failed join is a panic of the release itself, which has
                // nothing to report.
                let _ = tokio::task::spawn_blocking(release_free_memory).await;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use axum::Router;
    use axum::body::Body;
    use axum::routing::get;
    use tower::ServiceExt as _;

    /// The idle releases since `before`, once a quiet period has passed.
    async fn releases_after_quiet(before: u64) -> u64 {
        tokio::time::sleep(IDLE_RELEASE_DELAY * 3).await;
        RELEASES.load(Ordering::Acquire) - before
    }

    #[tokio::test]
    async fn a_burst_of_requests_ends_in_one_release_and_none_runs_while_requests_come() {
        let app = Router::new()
            .route("/", get(|| async { "ok" }))
            .layer(axum::middleware::from_fn(release_when_idle));
        let request = || Request::builder().uri("/").body(Body::empty()).unwrap();
        let before = RELEASES.load(Ordering::Acquire);
        for _ in 0..5 {
            app.clone().oneshot(request()).await.unwrap();
            tokio::time::sleep(IDLE_RELEASE_DELAY / 10).await;
        }
        assert_eq!(
            RELEASES.load(Ordering::Acquire),
            before,
            "released mid-burst"
        );
        assert_eq!(releases_after_quiet(before).await, 1);
    }

    #[test]
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    fn glibc_takes_the_mapping_threshold() {
        assert!(tune());
        release_free_memory();
    }
}
