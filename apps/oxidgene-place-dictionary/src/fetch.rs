//! Downloads, kept on disk. Every run downloads afresh unless asked to reuse
//! what an earlier run kept, which is for iterating on the generator itself.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, bail};

use crate::table::Table;

/// Wikidata asks every client to name itself and say where it comes from.
const USER_AGENT: &str = concat!(
    "oxidgene-place-dictionary/",
    env!("CARGO_PKG_VERSION"),
    " (https://github.com/trois-six/oxidgene)"
);
const SPARQL_ENDPOINT: &str = "https://query.wikidata.org/sparql";
const ATTEMPTS: u32 = 4;

pub struct Fetcher {
    client: reqwest::Client,
    cache: PathBuf,
    reuse: bool,
}

impl Fetcher {
    pub fn new(cache: PathBuf, reuse: bool) -> Result<Self> {
        std::fs::create_dir_all(&cache)
            .with_context(|| format!("cannot create the cache {}", cache.display()))?;
        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(600))
            .build()?;
        Ok(Self {
            client,
            cache,
            reuse,
        })
    }

    /// The body at `url`, read from the cache entry `name` when present.
    pub async fn bytes(&self, name: &str, url: &str) -> Result<Vec<u8>> {
        self.cached(name, || self.client.get(url), |_| Ok(())).await
    }

    /// The body at `url`, or `None` when the server says it does not exist:
    /// for probing which edition of a source has been published.
    pub async fn optional_bytes(&self, name: &str, url: &str) -> Result<Option<Vec<u8>>> {
        let path = self.cache.join(name);
        if self.reuse
            && let Ok(bytes) = std::fs::read(&path)
        {
            return Ok(Some(bytes));
        }
        let response = self.client.get(url).send().await?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let bytes = response.error_for_status()?.bytes().await?.to_vec();
        std::fs::write(&path, &bytes)
            .with_context(|| format!("cannot write {}", path.display()))?;
        Ok(Some(bytes))
    }

    /// The answer to a form posted to `url` with the URL-encoded `fields`.
    pub async fn post_form(
        &self,
        name: &str,
        url: &str,
        fields: &[(&str, &str)],
    ) -> Result<Vec<u8>> {
        let mut form = reqwest::Url::parse("form:")?;
        form.query_pairs_mut().extend_pairs(fields);
        let body = form.query().unwrap_or_default().to_string();
        self.cached(
            name,
            || {
                self.client
                    .post(url)
                    .header(
                        reqwest::header::CONTENT_TYPE,
                        "application/x-www-form-urlencoded",
                    )
                    .body(body.clone())
            },
            |_| Ok(()),
        )
        .await
    }

    /// A Wikidata query, answered as CSV.
    pub async fn sparql(&self, query: &str) -> Result<Table> {
        let mut hasher = DefaultHasher::new();
        query.hash(&mut hasher);
        let name = &format!("wikidata-{:016x}.csv", hasher.finish());
        let read = |bytes: &[u8]| {
            let text = std::str::from_utf8(bytes).context("invalid UTF-8")?;
            Table::parse(text, ',')
        };
        // Posted: a query listing codes outgrows what a URL may carry.
        let mut form = reqwest::Url::parse("form:")?;
        form.query_pairs_mut().append_pair("query", query);
        let body = form.query().unwrap_or_default().to_string();
        let bytes = self
            .cached(
                name,
                || {
                    self.client
                        .post(SPARQL_ENDPOINT)
                        .header(reqwest::header::ACCEPT, "text/csv")
                        .header(
                            reqwest::header::CONTENT_TYPE,
                            "application/x-www-form-urlencoded",
                        )
                        .body(body.clone())
                },
                |bytes| read(bytes).map(drop),
            )
            .await?;
        read(&bytes).with_context(|| format!("cannot read the answer to {query}"))
    }

    /// Only a body `check` accepts is cached: the query service answers a
    /// timeout with a success status and a stack trace after the rows.
    async fn cached(
        &self,
        name: &str,
        request: impl Fn() -> reqwest::RequestBuilder,
        check: impl Fn(&[u8]) -> Result<()>,
    ) -> Result<Vec<u8>> {
        let path = self.cache.join(name);
        if self.reuse
            && let Ok(bytes) = std::fs::read(&path)
            && check(&bytes).is_ok()
        {
            return Ok(bytes);
        }
        eprintln!("downloading {name}");
        let bytes = self
            .download(&request, &check)
            .await
            .with_context(|| format!("cannot download {name}"))?;
        std::fs::write(&path, &bytes)
            .with_context(|| format!("cannot write {}", path.display()))?;
        Ok(bytes)
    }

    /// Wikidata's query service times out or throttles now and then; a retry
    /// after a pause is what its usage policy asks for.
    async fn download(
        &self,
        request: &impl Fn() -> reqwest::RequestBuilder,
        check: &impl Fn(&[u8]) -> Result<()>,
    ) -> Result<Vec<u8>> {
        let mut attempt = 1;
        loop {
            let outcome = match request().send().await {
                Ok(response) if response.status().is_success() => {
                    let bytes = response.bytes().await?.to_vec();
                    match check(&bytes) {
                        Ok(()) => return Ok(bytes),
                        Err(error) => format!("unusable answer: {error:#}"),
                    }
                }
                // A refusal is not going to change on a retry.
                Ok(response)
                    if response.status().is_client_error()
                        && response.status() != reqwest::StatusCode::TOO_MANY_REQUESTS =>
                {
                    bail!("HTTP {}", response.status());
                }
                Ok(response) => format!("HTTP {}", response.status()),
                Err(error) => error.to_string(),
            };
            if attempt == ATTEMPTS {
                bail!("{outcome} after {ATTEMPTS} attempts");
            }
            eprintln!("  {outcome}; retrying");
            tokio::time::sleep(Duration::from_secs(20 * u64::from(attempt))).await;
            attempt += 1;
        }
    }
}
