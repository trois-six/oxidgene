//! Bounded exponential backoff (docs/cross-cutting.md §6.4): how long to
//! wait before asking again for something that failed without an answer.

/// A schedule of pauses that double from `first_ms` up to `max_ms`, for at
/// most `retries` attempts after the first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Backoff {
    first_ms: u32,
    max_ms: u32,
    retries: u32,
}

impl Backoff {
    /// Never asks again.
    pub const NONE: Self = Self::new(0, 0, 0);

    /// Pauses doubling from `first_ms` to `max_ms`, `retries` times at most.
    pub const fn new(first_ms: u32, max_ms: u32, retries: u32) -> Self {
        Self {
            first_ms,
            max_ms,
            retries,
        }
    }

    /// The same pauses, however many times the request fails: for a follower
    /// that runs as long as the application does.
    pub const fn unbounded(first_ms: u32, max_ms: u32) -> Self {
        Self::new(first_ms, max_ms, u32::MAX)
    }

    /// The pause in milliseconds before retry number `retry`, counted from
    /// zero, or `None` once the retries are spent. An unbounded schedule
    /// never spends them.
    pub fn delay(&self, retry: u32) -> Option<u32> {
        let spent = self.retries != u32::MAX && retry >= self.retries;
        (!spent).then(|| {
            2_u32
                .checked_pow(retry)
                .and_then(|factor| self.first_ms.checked_mul(factor))
                .map_or(self.max_ms, |pause| pause.min(self.max_ms))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pauses_double_up_to_the_ceiling_and_then_stop() {
        let backoff = Backoff::new(400, 2_000, 4);
        let pauses: Vec<_> = (0..6).map(|retry| backoff.delay(retry)).collect();
        assert_eq!(
            pauses,
            [Some(400), Some(800), Some(1_600), Some(2_000), None, None]
        );
    }

    #[test]
    fn no_retry_means_no_pause() {
        assert_eq!(Backoff::NONE.delay(0), None);
    }

    #[test]
    fn an_unbounded_schedule_stays_at_its_ceiling_without_overflowing() {
        let backoff = Backoff::unbounded(1_000, 60_000);
        assert_eq!(backoff.delay(0), Some(1_000));
        assert_eq!(backoff.delay(5), Some(32_000));
        assert_eq!(backoff.delay(6), Some(60_000));
        assert_eq!(backoff.delay(40), Some(60_000));
        assert_eq!(backoff.delay(u32::MAX), Some(60_000));
    }
}
