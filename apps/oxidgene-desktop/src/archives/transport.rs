//! The `window` transport: an adapter's requests run as the portal page's
//! own `fetch` inside the archive window.
//!
//! The resolver runs on the Dioxus runtime; a window can only be driven from
//! the event loop. The two meet in [`Shared`]: the transport queues
//! [`Command`]s that the loop's handler carries out on the next event, and
//! waits on a channel that the handler completes when the window posts its
//! answer back over IPC. Completing the channel wakes the Dioxus task, which
//! wakes the loop, so the exchange never waits for an unrelated event.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_channel::oneshot;
use oxidgene_archives::platform::BoxFuture;
use oxidgene_archives::transport::{PageAnswer, TIMEOUT, request_url};
use oxidgene_archives::{FetchError, PortalEndpoint, PortalFetch, PortalRequest, PortalTransport};
use serde::Deserialize;

use super::script;

/// One opening of a register, and its window.
pub(super) type SessionId = u64;

/// How long the window may take to show the portal's search page, an
/// anti-bot challenge included.
const LOAD_TIMEOUT: Duration = Duration::from_secs(30);

/// A margin over the page's own request timeout, after which a request the
/// page never answered is given up.
const ANSWER_MARGIN: Duration = Duration::from_secs(5);

/// A banner to show once a page has loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Banner {
    pub(super) text: String,
    pub(super) close: String,
}

/// Work for the event loop.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Command {
    /// Loads `url` in the session's window, creating the window if needed.
    /// IPC messages are then accepted from `origins` only.
    Load {
        session: SessionId,
        title: String,
        url: String,
        origins: Vec<String>,
        banner: Option<Banner>,
    },
    /// Runs one request's script in the session's window.
    Fetch {
        session: SessionId,
        ticket: u64,
        script: String,
    },
}

/// A request waiting for the window's answer.
struct Waiting {
    session: SessionId,
    origins: Vec<String>,
    reply: oneshot::Sender<Result<String, FetchError>>,
}

/// What the resolution tasks and the event loop share.
#[derive(Default)]
pub(super) struct Shared {
    commands: Mutex<Vec<Command>>,
    loading: Mutex<HashMap<SessionId, oneshot::Sender<()>>>,
    waiting: Mutex<HashMap<u64, Waiting>>,
    closed: Mutex<HashSet<SessionId>>,
    next: AtomicU64,
}

/// What the fetch script posts back: the ticket of its request and the
/// page's answer, checked by [`PageAnswer::result`].
#[derive(Debug, Deserialize, PartialEq, Eq)]
pub(super) struct Fetched {
    ticket: u64,
    #[serde(flatten)]
    answer: PageAnswer,
}

impl Shared {
    pub(super) fn next_id(&self) -> u64 {
        self.next.fetch_add(1, Ordering::Relaxed)
    }

    pub(super) fn push(&self, command: Command) {
        if let Ok(mut commands) = self.commands.lock() {
            commands.push(command);
        }
    }

    pub(super) fn drain(&self) -> Vec<Command> {
        self.commands
            .lock()
            .map(|mut commands| commands.drain(..).collect())
            .unwrap_or_default()
    }

    pub(super) fn is_closed(&self, session: SessionId) -> bool {
        self.closed
            .lock()
            .is_ok_and(|closed| closed.contains(&session))
    }

    /// The session's window has shown a page: a pending load is done.
    pub(super) fn ready(&self, session: SessionId) {
        let waiter = self
            .loading
            .lock()
            .ok()
            .and_then(|mut loading| loading.remove(&session));
        if let Some(waiter) = waiter {
            let _ = waiter.send(());
        }
    }

    /// The session's window answered a request. An answer for a request of
    /// another session, or one no longer waited for, is ignored.
    pub(super) fn fetched(&self, session: SessionId, fetched: Fetched) {
        let Ok(mut waiting) = self.waiting.lock() else {
            return;
        };
        if waiting
            .get(&fetched.ticket)
            .is_none_or(|request| request.session != session)
        {
            return;
        }
        if let Some(request) = waiting.remove(&fetched.ticket) {
            let _ = request.reply.send(fetched.answer.result(&request.origins));
        }
    }

    /// The request could not be run: its window is gone.
    pub(super) fn abandon(&self, ticket: u64) {
        let request = self
            .waiting
            .lock()
            .ok()
            .and_then(|mut waiting| waiting.remove(&ticket));
        if let Some(request) = request {
            let _ = request.reply.send(Err(FetchError::Network));
        }
    }

    /// The reader closed the session's window, or it could not be opened:
    /// what waits on it fails, and it is not reopened.
    pub(super) fn close(&self, session: SessionId) {
        if let Ok(mut closed) = self.closed.lock() {
            closed.insert(session);
        }
        if let Ok(mut loading) = self.loading.lock() {
            loading.remove(&session);
        }
        if let Ok(mut waiting) = self.waiting.lock() {
            waiting.retain(|_, request| request.session != session);
        }
    }

    fn wait_for_load(&self, session: SessionId) -> oneshot::Receiver<()> {
        let (sender, receiver) = oneshot::channel();
        if let Ok(mut loading) = self.loading.lock() {
            loading.insert(session, sender);
        }
        receiver
    }

    fn wait_for_answer(
        &self,
        session: SessionId,
        origins: Vec<String>,
    ) -> (u64, oneshot::Receiver<Result<String, FetchError>>) {
        let ticket = self.next_id();
        let (reply, receiver) = oneshot::channel();
        if let Ok(mut waiting) = self.waiting.lock() {
            waiting.insert(
                ticket,
                Waiting {
                    session,
                    origins,
                    reply,
                },
            );
        }
        (ticket, receiver)
    }
}

/// Requests through one session's archive window.
pub(super) struct WindowTransport {
    pub(super) shared: Arc<Shared>,
    pub(super) session: SessionId,
    /// The window's title: the source title.
    pub(super) title: String,
    /// Shown while the resolution runs.
    pub(super) searching: Banner,
}

impl PortalTransport for WindowTransport {
    fn is_browser(&self) -> bool {
        true
    }

    fn connect<'a>(
        &'a self,
        endpoint: &'a PortalEndpoint,
    ) -> BoxFuture<'a, Result<Box<dyn PortalFetch + 'a>, FetchError>> {
        Box::pin(async move {
            if self.shared.is_closed(self.session) {
                return Err(FetchError::Network);
            }
            let loaded = self.shared.wait_for_load(self.session);
            self.shared.push(Command::Load {
                session: self.session,
                title: self.title.clone(),
                url: endpoint.start.clone(),
                origins: vec![endpoint.origin.clone()],
                banner: Some(self.searching.clone()),
            });
            match tokio::time::timeout(LOAD_TIMEOUT, loaded).await {
                Ok(Ok(())) => Ok(Box::new(WindowFetch {
                    shared: Arc::clone(&self.shared),
                    session: self.session,
                    endpoint: endpoint.clone(),
                }) as Box<dyn PortalFetch>),
                Ok(Err(_)) => Err(FetchError::Network),
                Err(_) => Err(FetchError::Timeout),
            }
        })
    }
}

struct WindowFetch {
    shared: Arc<Shared>,
    session: SessionId,
    endpoint: PortalEndpoint,
}

impl PortalFetch for WindowFetch {
    fn request<'a>(
        &'a self,
        request: &'a PortalRequest,
    ) -> BoxFuture<'a, Result<String, FetchError>> {
        Box::pin(async move {
            let url = request_url(&self.endpoint, request)?;
            let origins = self.endpoint.origins().map(str::to_owned).collect();
            let (ticket, answer) = self.shared.wait_for_answer(self.session, origins);
            self.shared.push(Command::Fetch {
                session: self.session,
                ticket,
                script: script::fetch(
                    ticket,
                    request.method,
                    &url,
                    &request.headers,
                    request.body.as_deref(),
                ),
            });
            match tokio::time::timeout(TIMEOUT + ANSWER_MARGIN, answer).await {
                Ok(Ok(result)) => result,
                Ok(Err(_)) => Err(FetchError::Network),
                Err(_) => {
                    self.shared.abandon(ticket);
                    Err(FetchError::Timeout)
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn origins() -> Vec<String> {
        vec!["https://archives.example.org".to_owned()]
    }

    fn answer(ticket: u64, status: u16, url: &str, body: &str) -> Fetched {
        Fetched {
            ticket,
            answer: PageAnswer {
                status: Some(status),
                url: Some(url.to_owned()),
                body: Some(body.to_owned()),
                error: None,
            },
        }
    }

    /// The checks themselves are `PageAnswer`'s, tested in
    /// `oxidgene-archives`.
    #[test]
    fn reads_the_ticket_beside_the_answer() {
        let parsed: Fetched = serde_json::from_str(
            r#"{"ticket": 3, "status": 200, "url": "https://archives.example.org/", "body": "x"}"#,
        )
        .unwrap();
        assert_eq!(parsed, answer(3, 200, "https://archives.example.org/", "x"));
        assert_eq!(parsed.answer.result(&origins()), Ok("x".to_owned()));
        let failed: Fetched = serde_json::from_str(r#"{"ticket": 4, "error": "timeout"}"#).unwrap();
        assert_eq!(failed.ticket, 4);
        assert_eq!(failed.answer.result(&origins()), Err(FetchError::Timeout));
    }

    #[test]
    fn answers_reach_their_own_request_only() {
        let shared = Shared::default();
        let (ticket, mut receiver) = shared.wait_for_answer(1, origins());
        let url = "https://archives.example.org/";

        // Another session's window cannot answer it.
        shared.fetched(2, answer(ticket, 200, url, "forged"));
        assert_eq!(receiver.try_recv(), Ok(None));

        shared.fetched(1, answer(ticket, 200, url, "body"));
        assert_eq!(receiver.try_recv(), Ok(Some(Ok("body".to_owned()))));
        // A second answer to the same ticket is ignored.
        shared.fetched(1, answer(ticket, 200, url, "again"));
    }

    #[test]
    fn closing_a_window_fails_what_waits_on_it() {
        let shared = Shared::default();
        let mut loaded = shared.wait_for_load(4);
        let (_, mut answer) = shared.wait_for_answer(4, origins());
        let (other_ticket, mut other) = shared.wait_for_answer(5, origins());

        shared.close(4);
        assert!(shared.is_closed(4));
        assert!(loaded.try_recv().is_err());
        assert!(answer.try_recv().is_err());
        assert_eq!(other.try_recv(), Ok(None));

        shared.abandon(other_ticket);
        assert_eq!(other.try_recv(), Ok(Some(Err(FetchError::Network))));
    }

    #[test]
    fn a_loaded_page_releases_the_connection() {
        let shared = Shared::default();
        let mut loaded = shared.wait_for_load(6);
        shared.ready(7);
        assert_eq!(loaded.try_recv(), Ok(None));
        shared.ready(6);
        assert_eq!(loaded.try_recv(), Ok(Some(())));
    }

    #[test]
    fn commands_are_queued_for_the_event_loop() {
        let shared = Shared::default();
        let command = Command::Fetch {
            session: 1,
            ticket: shared.next_id(),
            script: String::new(),
        };
        shared.push(command);
        assert_eq!(shared.drain().len(), 1);
        assert!(shared.drain().is_empty());
    }
}
