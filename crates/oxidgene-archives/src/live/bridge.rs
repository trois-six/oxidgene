//! A browser transport for the live checks: the requests of an adapter run
//! in a page of the Playwright check, as the desktop's archive window runs
//! them (Archive Portals §4.2), while the adapter and the check stay in
//! Rust.
//!
//! The `archives-live-bridge` binary speaks JSON lines with the Playwright
//! process that starts it: one message per line on its standard output, one
//! answer per line on its standard input, each exchange finished before the
//! next starts, so requests stay sequential.
//!
//! | Out (Rust → page) | In (page → Rust) |
//! |---|---|
//! | `{"kind":"connect","start","origins"}`: load the portal's start page | `{"kind":"connected"}`, or with an `error` of `challenged`, `timeout` or `network` |
//! | `{"kind":"fetch","ticket","method","url","headers","body"}`: the page's own `fetch` | `{"kind":"fetched","ticket","status","url","body"}`, or with an `error` |
//! | `{"kind":"page","ticket","url","ready"}`: load a page, wait for `ready` to match | `{"kind":"fetched","ticket","status","url","body"}`, the rendered document, or with an `error` |
//! | `{"kind":"report","collections"}`: the checks of steps 1 to 3 | — |
//!
//! The answers are checked as the desktop window's are ([`PageAnswer`]): a
//! final address on the endpoint's origins, a success status, a bounded
//! body. A page that never got past an anti-bot challenge, or a request
//! refused by one, fails with [`FetchError::Challenged`].

use std::io::{BufRead, Write};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;
use serde_json::{Value, json};

use crate::platform::{BoxFuture, PortalEndpoint};
use crate::transport::{
    FetchError, PageAnswer, PortalFetch, PortalRequest, PortalTransport, page_url, request_url,
};

/// The page process at the other end of two byte streams.
pub struct BridgeTransport<R, W> {
    channel: Mutex<(R, W)>,
    tickets: AtomicU64,
}

impl<R: BufRead + Send, W: Write + Send> BridgeTransport<R, W> {
    pub fn new(input: R, output: W) -> Self {
        Self {
            channel: Mutex::new((input, output)),
            tickets: AtomicU64::new(0),
        }
    }

    /// Sends one message without waiting for an answer.
    pub fn send(&self, message: &impl Serialize) -> Result<(), FetchError> {
        let mut channel = self.channel.lock().map_err(|_| FetchError::Network)?;
        write_line(&mut channel.1, message)
    }

    /// Sends a message and reads the answer's line.
    fn exchange(&self, message: &Value) -> Result<Value, FetchError> {
        let mut channel = self.channel.lock().map_err(|_| FetchError::Network)?;
        let (input, output) = &mut *channel;
        write_line(output, message)?;
        let mut line = String::new();
        match input.read_line(&mut line) {
            Ok(0) | Err(_) => Err(FetchError::Network),
            Ok(_) => serde_json::from_str(&line).map_err(|_| FetchError::Network),
        }
    }
}

fn write_line(output: &mut impl Write, message: &impl Serialize) -> Result<(), FetchError> {
    let mut line = serde_json::to_vec(message).map_err(|_| FetchError::Network)?;
    line.push(b'\n');
    output
        .write_all(&line)
        .and_then(|()| output.flush())
        .map_err(|_| FetchError::Network)
}

/// The `kind` of an answer, and its `error` when it has one.
fn kind_and_error(answer: &Value) -> (Option<&str>, Option<&str>) {
    (answer["kind"].as_str(), answer["error"].as_str())
}

impl<R: BufRead + Send, W: Write + Send> PortalTransport for BridgeTransport<R, W> {
    fn is_browser(&self) -> bool {
        true
    }

    fn connect<'a>(
        &'a self,
        endpoint: &'a PortalEndpoint,
    ) -> BoxFuture<'a, Result<Box<dyn PortalFetch + 'a>, FetchError>> {
        Box::pin(async move {
            let answer = self.exchange(&json!({
                "kind": "connect",
                "start": endpoint.start,
                "origins": endpoint.origins().collect::<Vec<_>>(),
            }))?;
            match kind_and_error(&answer) {
                (Some("connected"), None) => Ok(Box::new(BridgeFetch {
                    transport: self,
                    endpoint: endpoint.clone(),
                }) as Box<dyn PortalFetch + 'a>),
                (Some("connected"), Some("timeout")) => Err(FetchError::Timeout),
                (Some("connected"), Some("challenged")) => Err(FetchError::Challenged),
                _ => Err(FetchError::Network),
            }
        })
    }
}

struct BridgeFetch<'t, R, W> {
    transport: &'t BridgeTransport<R, W>,
    endpoint: PortalEndpoint,
}

impl<R: BufRead + Send, W: Write + Send> BridgeFetch<'_, R, W> {
    /// Sends `message` under a new ticket and checks the page's answer to it.
    fn answer(&self, mut message: Value) -> Result<String, FetchError> {
        let ticket = self.transport.tickets.fetch_add(1, Ordering::Relaxed);
        message["ticket"] = ticket.into();
        let answer = self.transport.exchange(&message)?;
        if answer["kind"] != "fetched" || answer["ticket"] != ticket {
            return Err(FetchError::Network);
        }
        let origins: Vec<String> = self.endpoint.origins().map(str::to_owned).collect();
        serde_json::from_value::<PageAnswer>(answer)
            .map_err(|_| FetchError::Network)?
            .result(&origins)
    }
}

impl<R: BufRead + Send, W: Write + Send> PortalFetch for BridgeFetch<'_, R, W> {
    fn request<'a>(
        &'a self,
        request: &'a PortalRequest,
    ) -> BoxFuture<'a, Result<String, FetchError>> {
        Box::pin(async move {
            let url = request_url(&self.endpoint, request)?;
            let headers: serde_json::Map<String, Value> = request
                .headers
                .iter()
                .map(|(name, value)| (name.clone(), value.as_str().into()))
                .collect();
            self.answer(json!({
                "kind": "fetch",
                "method": request.method.as_str(),
                "url": url,
                "headers": headers,
                "body": request.body,
            }))
        })
    }

    fn page<'a>(
        &'a self,
        path_and_query: &'a str,
        ready: &'a str,
    ) -> BoxFuture<'a, Result<String, FetchError>> {
        Box::pin(async move {
            let url = page_url(&self.endpoint, path_and_query, ready)?;
            self.answer(json!({ "kind": "page", "url": url, "ready": ready }))
        })
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;
    use crate::platform::Access;
    use crate::tests::block_on;

    fn endpoint() -> PortalEndpoint {
        PortalEndpoint {
            origin: "https://archives.example.org".to_owned(),
            other_origins: Vec::new(),
            start: "https://archives.example.org/search".to_owned(),
            access: Access::Browser,
        }
    }

    fn transport(answers: &str) -> BridgeTransport<Cursor<Vec<u8>>, Vec<u8>> {
        BridgeTransport::new(Cursor::new(answers.as_bytes().to_vec()), Vec::new())
    }

    fn sent(transport: BridgeTransport<Cursor<Vec<u8>>, Vec<u8>>) -> Vec<Value> {
        let (_, output) = transport.channel.into_inner().unwrap();
        String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    #[test]
    fn runs_each_request_in_the_page_and_checks_its_answer() {
        let transport = transport(concat!(
            "{\"kind\":\"connected\"}\n",
            "{\"kind\":\"fetched\",\"ticket\":0,\"status\":200,\"url\":\"https://archives.example.org/a\",\"body\":\"first\"}\n",
            "{\"kind\":\"fetched\",\"ticket\":1,\"status\":200,\"url\":\"https://elsewhere.example.org/\",\"body\":\"x\"}\n",
            "{\"kind\":\"fetched\",\"ticket\":7,\"status\":200,\"url\":\"https://archives.example.org/c\",\"body\":\"x\"}\n",
        ));
        let endpoint = endpoint();
        let fetch = block_on(transport.connect(&endpoint)).expect("a connection");
        assert_eq!(block_on(fetch.get("/a")), Ok("first".to_owned()));
        // A redirect away from the portal, then an answer to another ticket.
        assert_eq!(block_on(fetch.get("/b")), Err(FetchError::NotSameOrigin));
        assert_eq!(block_on(fetch.get("/c")), Err(FetchError::Network));
        // A request leaving the portal is refused before it is sent.
        assert_eq!(
            block_on(fetch.get("//elsewhere.example.org/")),
            Err(FetchError::NotSameOrigin)
        );
        // The page closed its end.
        assert_eq!(block_on(fetch.get("/d")), Err(FetchError::Network));
        drop(fetch);

        let sent = sent(transport);
        assert_eq!(sent[0]["kind"], "connect");
        assert_eq!(sent[0]["start"], "https://archives.example.org/search");
        assert_eq!(sent[1]["kind"], "fetch");
        assert_eq!(sent[1]["url"], "https://archives.example.org/a");
        assert_eq!(sent[1]["method"], "GET");
        assert_eq!(sent[1]["body"], Value::Null);
        assert_eq!(sent.len(), 5);
    }

    #[test]
    fn loads_a_page_of_a_portal_that_admits_pages_only() {
        let transport = transport(concat!(
            "{\"kind\":\"connected\"}\n",
            "{\"kind\":\"fetched\",\"ticket\":0,\"status\":200,\"url\":\"https://archives.example.org/search?q=1\",\"body\":\"<table>rows</table>\"}\n",
            "{\"kind\":\"fetched\",\"ticket\":1,\"error\":\"challenged\"}\n",
        ));
        let endpoint = PortalEndpoint {
            access: Access::Page,
            ..endpoint()
        };
        let fetch = block_on(transport.connect(&endpoint)).expect("a connection");
        assert_eq!(
            block_on(fetch.page("/search?q=1", "table.results")),
            Ok("<table>rows</table>".to_owned())
        );
        assert_eq!(
            block_on(fetch.page("/search?q=2", "table.results")),
            Err(FetchError::Challenged)
        );
        // No script's request reaches such a portal, nor a page elsewhere.
        assert_eq!(block_on(fetch.get("/search")), Err(FetchError::NotAllowed));
        assert_eq!(
            block_on(fetch.page("//elsewhere.example.org/", "table")),
            Err(FetchError::NotSameOrigin)
        );
        drop(fetch);

        let sent = sent(transport);
        assert_eq!(sent.len(), 3);
        assert_eq!(sent[1]["kind"], "page");
        assert_eq!(sent[1]["ticket"], 0);
        assert_eq!(sent[1]["url"], "https://archives.example.org/search?q=1");
        assert_eq!(sent[1]["ready"], "table.results");
    }

    #[test]
    fn a_page_stopped_by_a_challenge_fails_as_challenged() {
        let connect = |answer: &str| {
            let transport = transport(answer);
            block_on(transport.connect(&endpoint())).map(drop)
        };
        assert_eq!(
            connect("{\"kind\":\"connected\",\"error\":\"challenged\"}\n"),
            Err(FetchError::Challenged)
        );
        assert_eq!(
            connect("{\"kind\":\"connected\",\"error\":\"timeout\"}\n"),
            Err(FetchError::Timeout)
        );
        assert_eq!(connect(""), Err(FetchError::Network));

        // A request a challenge refuses once the page is in.
        let transport = transport(concat!(
            "{\"kind\":\"connected\"}\n",
            "{\"kind\":\"fetched\",\"ticket\":0,\"status\":403,\"url\":\"https://archives.example.org/a\",\"body\":\"<title>Request Rejected</title>\"}\n",
        ));
        let endpoint = endpoint();
        let fetch = block_on(transport.connect(&endpoint)).expect("a connection");
        assert_eq!(block_on(fetch.get("/a")), Err(FetchError::Challenged));
    }
}
