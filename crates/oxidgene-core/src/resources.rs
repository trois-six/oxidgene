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
}
