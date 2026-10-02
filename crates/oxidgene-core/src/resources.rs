//! Shared resource budgets for CPU-intensive background work.

/// Maximum parallelism for CPU-intensive work on this machine.
///
/// Uses at most 75% of the logical processors, rounded down. A single-processor
/// machine still gets one worker; machines with several processors always keep
/// at least one available for the UI, the operating system, and other services.
#[must_use]
pub fn cpu_worker_limit() -> usize {
    let available = std::thread::available_parallelism()
        .map(std::num::NonZeroUsize::get)
        .unwrap_or(1);
    cpu_worker_limit_for(available)
}

fn cpu_worker_limit_for(available: usize) -> usize {
    available.saturating_sub(available.div_ceil(4)).max(1)
}

/// Maximum parallelism for work that holds a full-size decoded image in each
/// worker: [`cpu_worker_limit`], at most [`MAX_DECODE_WORKERS`].
#[must_use]
pub fn decode_worker_limit() -> usize {
    cpu_worker_limit().min(MAX_DECODE_WORKERS)
}

/// The most images decoded at once, whatever the machine.
///
/// A scanned page decodes to 30 MiB and more, and every worker holds one, so
/// past a few workers more of them buy memory rather than time: on a
/// 16-thread machine, twelve workers instead of eight made a Geneanet
/// import's archive index peak 75 MiB higher to save half a second of a
/// twenty-second job. The cap keeps a many-core machine's peak at an
/// eight-core one's.
pub const MAX_DECODE_WORKERS: usize = 8;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_intensive_work_keeps_one_quarter_of_processors_available() {
        assert_eq!(cpu_worker_limit_for(0), 1);
        assert_eq!(cpu_worker_limit_for(1), 1);
        assert_eq!(cpu_worker_limit_for(2), 1);
        assert_eq!(cpu_worker_limit_for(4), 3);
        assert_eq!(cpu_worker_limit_for(8), 6);
        assert_eq!(cpu_worker_limit_for(16), 12);
    }

    #[test]
    fn image_decoding_never_takes_more_than_its_cap() {
        assert!(decode_worker_limit() <= MAX_DECODE_WORKERS);
        assert!(decode_worker_limit() >= 1);
    }
}
