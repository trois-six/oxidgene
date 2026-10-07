//! When the archive window's progress overlay covers the page.
//!
//! The overlay (`overlay.js`) covers the window from the start page's load
//! until the resolution lands, and says where it stands ([`Stage`]). It
//! gives way at once whenever the reader has to act on the page: an
//! anti-bot check they were asked to answer — until the portal's page shows,
//! as the window's [`Status`](super::Status) tracks it —, or a cookie banner
//! `consent.js` could not refuse — until it is gone or another document
//! loads. It is gone once the landing's page — the target,
//! the filtered results or the failure's page — has shown, or when the
//! reader cancels. The decisions are apart from the window so that they can
//! be tested; the window renders [`Cover::shown`] after each.

use std::time::{Duration, Instant};

use super::ConsentState;
use super::transport::{Progress, Stage};

#[derive(Debug, Default)]
pub(super) struct Cover {
    /// The overlay of the running resolution, and when its stage began.
    progress: Option<(Progress, Instant)>,
    /// Whether a cookie banner on the page is the reader's to answer.
    consent: bool,
}

impl Cover {
    /// A page loads, with the overlay of a resolution or none.
    pub(super) fn load(&mut self, progress: Option<Progress>, now: Instant) {
        self.progress = progress.map(|progress| (progress, now));
        self.consent = false;
    }

    /// A new document starts: a banner of the last one is gone with it.
    pub(super) fn document(&mut self) {
        self.consent = false;
    }

    /// A document was classified: the landing's ends the resolution.
    pub(super) fn page(&mut self) {
        if self
            .progress
            .as_ref()
            .is_some_and(|(progress, _)| matches!(progress.stage, Stage::Opening { .. }))
        {
            self.progress = None;
        }
    }

    /// The resolution moved on.
    pub(super) fn stage(&mut self, stage: Stage, now: Instant) {
        if let Some((progress, since)) = &mut self.progress {
            progress.stage = stage;
            *since = now;
        }
    }

    /// What `consent.js` did with a cookie banner.
    pub(super) fn consent(&mut self, state: ConsentState) {
        match state {
            ConsentState::Left => self.consent = true,
            ConsentState::Closed => self.consent = false,
            ConsentState::Refused | ConsentState::Dismissed => {}
        }
    }

    /// The reader cancelled the resolution.
    pub(super) fn cancel(&mut self) {
        self.progress = None;
    }

    /// The overlay to show, and how long its stage has lasted; none when
    /// there is no resolution or the reader has to act: `asking` when they
    /// were asked to answer an anti-bot check.
    pub(super) fn shown(&self, now: Instant, asking: bool) -> Option<(&Progress, Duration)> {
        if asking || self.consent {
            return None;
        }
        let (progress, since) = self.progress.as_ref()?;
        Some((progress, now.saturating_duration_since(*since)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn progress(stage: Stage) -> Option<Progress> {
        Some(Progress {
            archive: "Archives of Example".to_owned(),
            citation: "AD00 - Exampleville - (aucun) - N - 1877".to_owned(),
            stage,
        })
    }

    fn stage(cover: &Cover, now: Instant) -> Option<Stage> {
        cover.shown(now, false).map(|(progress, _)| progress.stage)
    }

    #[test]
    fn shows_each_stage_with_its_elapsed_time() {
        let start = Instant::now();
        let mut cover = Cover::default();
        assert_eq!(cover.shown(start, false), None);
        cover.load(progress(Stage::Connecting), start);
        cover.document();
        cover.page();
        let later = start + Duration::from_secs(2);
        assert_eq!(
            cover
                .shown(later, false)
                .map(|(progress, lasted)| (progress.stage, lasted)),
            Some((Stage::Connecting, Duration::from_secs(2)))
        );
        cover.stage(Stage::Searching, later);
        let much_later = later + Duration::from_secs(4);
        assert_eq!(
            cover
                .shown(much_later, false)
                .map(|(progress, lasted)| (progress.stage, lasted)),
            Some((Stage::Searching, Duration::from_secs(4)))
        );
    }

    #[test]
    fn gives_way_while_the_reader_answers_a_check() {
        let now = Instant::now();
        let mut cover = Cover::default();
        cover.load(progress(Stage::Connecting), now);
        // A check clearing itself stays covered.
        cover.page();
        assert_eq!(stage(&cover, now), Some(Stage::Connecting));
        assert_eq!(cover.shown(now, true), None);
        // Covered again once the reader is no longer asked.
        cover.document();
        cover.page();
        assert_eq!(stage(&cover, now), Some(Stage::Connecting));
    }

    #[test]
    fn gives_way_to_a_cookie_banner_left_to_the_reader() {
        let now = Instant::now();
        let mut cover = Cover::default();
        cover.load(progress(Stage::Connecting), now);
        cover.consent(ConsentState::Refused);
        assert!(cover.shown(now, false).is_some());
        // An information notice acknowledged leaves nothing to the reader.
        cover.consent(ConsentState::Dismissed);
        assert!(cover.shown(now, false).is_some());
        cover.consent(ConsentState::Left);
        assert_eq!(cover.shown(now, false), None);
        // Still the reader's while the search runs on.
        cover.stage(Stage::Searching, now);
        assert_eq!(cover.shown(now, false), None);
        cover.consent(ConsentState::Closed);
        assert_eq!(stage(&cover, now), Some(Stage::Searching));
        // A banner goes with its document.
        cover.consent(ConsentState::Left);
        cover.document();
        assert!(cover.shown(now, false).is_some());
    }

    #[test]
    fn is_gone_once_the_landing_shows_or_the_reader_cancels() {
        let now = Instant::now();
        let mut cover = Cover::default();
        cover.load(progress(Stage::Connecting), now);
        cover.page();
        cover.load(progress(Stage::Opening { view: Some(5) }), now);
        // The landing's document starts under the overlay.
        cover.document();
        assert_eq!(stage(&cover, now), Some(Stage::Opening { view: Some(5) }));
        // Whatever the landing's page is: the portal's, a check or a block.
        cover.page();
        assert_eq!(cover.shown(now, false), None);
        // A stage after the landing shows nothing.
        cover.stage(Stage::Searching, now);
        assert_eq!(cover.shown(now, false), None);

        cover.load(progress(Stage::Searching), now);
        cover.cancel();
        assert_eq!(cover.shown(now, false), None);
        // A page without a resolution has no overlay.
        cover.load(None, now);
        assert_eq!(cover.shown(now, false), None);
    }
}
