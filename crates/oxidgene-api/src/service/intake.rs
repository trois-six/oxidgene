//! How many files the process takes in at once.
//!
//! Every way a file's bytes enter the backend takes one of [`INTAKE_SLOTS`]
//! slots first and holds it until the bytes are stored: an import job's
//! upload, a media upload on either surface, a saved Geneanet session being
//! decoded, and a Geneanet import copying its inputs into job storage. Each
//! of those can hold a gigabyte of spool file, a decoded photograph or a
//! blocking thread; past the slots, callers wait their turn instead of
//! adding theirs. A caller still waiting when its client leaves is dropped
//! with its request, and the server's time limits bound how long one waits
//! (see [`crate::limits`]).

use oxidgene_core::OxidGeneError;
use tokio::sync::{Semaphore, SemaphorePermit};

/// How many intakes run at once, process-wide.
pub(crate) const INTAKE_SLOTS: usize = 2;

static INTAKE: Semaphore = Semaphore::const_new(INTAKE_SLOTS);

/// Wait for an intake slot; it is given back when the permit drops.
pub(crate) async fn slot() -> Result<SemaphorePermit<'static>, OxidGeneError> {
    INTAKE
        .acquire()
        .await
        .map_err(|_| OxidGeneError::Internal("file intake is unavailable".into()))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[tokio::test]
    async fn intakes_past_the_slots_wait_for_one_to_end() {
        let mut held = Vec::new();
        for _ in 0..INTAKE_SLOTS {
            held.push(slot().await.expect("a free slot"));
        }
        assert!(
            tokio::time::timeout(Duration::from_millis(50), slot())
                .await
                .is_err(),
            "an intake past the slots did not wait"
        );

        held.pop();
        let next = tokio::time::timeout(Duration::from_secs(5), slot())
            .await
            .expect("a slot freed for the waiting intake");
        assert!(next.is_ok());
    }
}
