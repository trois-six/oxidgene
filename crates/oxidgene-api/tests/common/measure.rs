//! What a measured step costs: its time, the most resident memory the
//! process reached during it, and the SQL statements it ran.
//!
//! The opt-in memory and large-tree runs share it. Resident memory is read
//! from `/proc/self/status` (Linux); elsewhere every figure reads 0 and the
//! budgets on memory pass.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Instant;

use tracing::Subscriber;
use tracing::span::{Attributes, Id};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;

/// A field of `/proc/self/status`, in MiB.
pub fn status_mib(field: &str) -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|status| {
            status
                .lines()
                .find_map(|line| line.strip_prefix(field))
                .and_then(|value| value.trim().trim_end_matches("kB").trim().parse().ok())
        })
        .map_or(0, |kib: u64| kib / 1024)
}

/// The highest peak [`take_peak_mib`] has read since [`reset_highest_peak`].
static HIGHEST_PEAK: AtomicU64 = AtomicU64::new(0);

/// The peak resident set since the last call, then a new start: writing 5 to
/// `clear_refs` sets the peak back to the current resident set (Linux).
pub fn take_peak_mib() -> u64 {
    let peak = status_mib("VmHWM:");
    let _ = std::fs::write("/proc/self/clear_refs", "5");
    HIGHEST_PEAK.fetch_max(peak, Ordering::Relaxed);
    peak
}

/// Forget the highest peak read so far.
pub fn reset_highest_peak() {
    take_peak_mib();
    HIGHEST_PEAK.store(0, Ordering::Relaxed);
}

/// The highest peak read since [`reset_highest_peak`], the current one
/// included.
pub fn highest_peak_mib() -> u64 {
    take_peak_mib();
    HIGHEST_PEAK.load(Ordering::Relaxed)
}

/// The SQL statements SeaORM ran since the process started, as
/// [`PhaseMemory`] counts them.
static STATEMENTS: AtomicUsize = AtomicUsize::new(0);

/// How many SQL statements ran so far.
pub fn statements() -> usize {
    STATEMENTS.load(Ordering::Relaxed)
}

/// Reports the time and peak memory of each span named in `phases` as it
/// closes, on lines starting with `label`, and counts the SQL statements
/// SeaORM runs (one `sea_orm.*` span each).
///
/// The peak starts over as each phase opens and closes, so a phase that
/// holds others reports what it held after the last of them.
pub struct PhaseMemory {
    label: &'static str,
    phases: &'static [&'static str],
    open: Mutex<HashMap<u64, (&'static str, Instant)>>,
}

impl PhaseMemory {
    pub fn new(label: &'static str, phases: &'static [&'static str]) -> Self {
        Self {
            label,
            phases,
            open: Mutex::default(),
        }
    }
}

impl<S: Subscriber> Layer<S> for PhaseMemory {
    fn on_new_span(&self, attributes: &Attributes<'_>, id: &Id, _: Context<'_, S>) {
        let name = attributes.metadata().name();
        // Transaction boundaries are not statements against the data.
        if name.starts_with("sea_orm.") && !matches!(name, "sea_orm.begin" | "sea_orm.commit") {
            STATEMENTS.fetch_add(1, Ordering::Relaxed);
        }
        if self.phases.contains(&name) {
            take_peak_mib();
            if let Ok(mut open) = self.open.lock() {
                open.insert(id.into_u64(), (name, Instant::now()));
            }
        }
    }

    fn on_close(&self, id: Id, _: Context<'_, S>) {
        let closed = self
            .open
            .lock()
            .ok()
            .and_then(|mut open| open.remove(&id.into_u64()));
        if let Some((name, opened)) = closed {
            let peak = take_peak_mib();
            eprintln!(
                "[{}] {name}: {:.3} s, peak RSS {peak} MiB",
                self.label,
                opened.elapsed().as_secs_f64()
            );
        }
    }
}

/// The processor time this process has used, in seconds, user and system:
/// fields 14 and 15 of `/proc/self/stat`, in hundredths of a second
/// (Linux); 0 elsewhere.
pub fn cpu_seconds() -> f64 {
    std::fs::read_to_string("/proc/self/stat")
        .ok()
        .and_then(|stat| {
            // After the command name, which may hold spaces.
            let fields: Vec<&str> = stat.rsplit_once(')')?.1.split_whitespace().collect();
            let ticks =
                fields.get(11)?.parse::<u64>().ok()? + fields.get(12)?.parse::<u64>().ok()?;
            Some(ticks as f64 / 100.0)
        })
        .unwrap_or(0.0)
}

/// What one step cost.
#[derive(Debug, Clone, Copy)]
pub struct Cost {
    pub seconds: f64,
    /// The processor time the process used during the step, all threads
    /// together: what a busy machine distorts least.
    pub cpu_seconds: f64,
    /// The most resident memory the process reached during the step.
    pub peak_mib: u64,
    /// The resident memory once the step was over.
    pub after_mib: u64,
    pub statements: usize,
}

/// Run `step` and measure it.
pub async fn measured<T>(step: impl Future<Output = T>) -> (T, Cost) {
    take_peak_mib();
    let before = statements();
    let cpu = cpu_seconds();
    let started = Instant::now();
    let out = step.await;
    let seconds = started.elapsed().as_secs_f64();
    let cost = Cost {
        seconds,
        cpu_seconds: cpu_seconds() - cpu,
        peak_mib: take_peak_mib(),
        after_mib: status_mib("VmRSS:"),
        statements: statements() - before,
    };
    (out, cost)
}
