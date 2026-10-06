//! Live checks of the archive portals (Archive Portals §9.1): whether each
//! catalogued collection still answers as its adapter and settings expect.
//!
//! The check builds its citation from the portal itself, so the repository
//! holds no locality or call number chosen from anyone's research:
//!
//! 1. **Search page**: the collection's search page loads and the settings'
//!    references are still in the portal ([`Probe::search_page`]), which
//!    also names the alphabetically first locality the portal's own
//!    locality filter lists (the most populated, listed first, searches
//!    slowest);
//! 2. **Discovery**: that locality with the collection's first document
//!    kind — an act, a table or a series — lists registers
//!    ([`Probe::registers`]), one of which is chosen;
//! 3. **Resolution**: a citation of that register — its locality, document
//!    kind, year, call number, first number where the register shows the
//!    numbers it spans, and a view in the middle of its images — resolves
//!    through the [`Resolver`] to that view of that register, and the same
//!    citation without its call number to the same register or the results.
//!
//! What a platform knows — where its references and locality list are, how
//! to search one locality — is its [`Probe`], beside its adapter; the
//! steps, the choice of the register and the verdicts are shared.
//! [`check_collection`] runs the three steps over any transport: the
//! `native` one in the crate's ignored `live` test, the browser page of the
//! Playwright check through [`bridge`] for a portal whose access is
//! `browser`. Steps 4 and 5, opening the target in a browser and loading
//! its images, belong to the Playwright check, which reads the [`Opening`]
//! of each report.

pub mod bridge;

use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::catalog::{Archive, Collection};
use crate::citation::{Act, CallNumber, CitationParts, CitedView};
use crate::platform::markup::fold;
use crate::platform::select::period_ranges;
use crate::platform::{Access, BoxFuture};
use crate::transport::{FetchError, PortalFetch, PortalRequest, PortalTransport};
use crate::{ArchiveImage, ArchiveRegistry, ArchiveTarget, ResolveError, Resolver};

/// How a check ended, from the best to the worst.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// Every step passed.
    Ok,
    /// An anti-bot challenge blocked the browser: unverified.
    Challenged,
    /// A timeout, a network error or a server error.
    Unreachable,
    /// The portal answered, but not as the adapter or its settings expect.
    Drift,
}

/// The steps of a check (Archive Portals §9.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    SearchPage,
    Discovery,
    Resolution,
    Opening,
    Images,
}

/// Why a step failed: what was expected and what came instead, as shapes —
/// never response content beyond the fields compared.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Failure {
    pub step: Step,
    pub outcome: Outcome,
    pub expected: String,
    pub received: String,
}

impl Failure {
    pub fn drift(step: Step, expected: impl Into<String>, received: impl Into<String>) -> Self {
        Self {
            step,
            outcome: Outcome::Drift,
            expected: expected.into(),
            received: received.into(),
        }
    }

    /// A failed request or resolution: unreachable for a timeout, a network
    /// or a server error, challenged for an anti-bot page, drift for anything
    /// else the portal answered.
    pub fn from_error(step: Step, expected: impl Into<String>, error: &ResolveError) -> Self {
        let (outcome, received) = match error {
            ResolveError::Timeout | ResolveError::Unreachable => {
                (Outcome::Unreachable, error.to_string())
            }
            ResolveError::Challenged => (
                Outcome::Challenged,
                "an anti-bot challenge in place of the portal".to_owned(),
            ),
            ResolveError::NoAdapter | ResolveError::UnexpectedResponse(_) => {
                (Outcome::Drift, error.to_string())
            }
        };
        Self {
            step,
            outcome,
            expected: expected.into(),
            received,
        }
    }

    /// An answer a probe cannot read: challenged when it is an anti-bot page
    /// rather than the portal's, drift described by `received` otherwise.
    pub(crate) fn unreadable(
        step: Step,
        expected: impl Into<String>,
        answer: &str,
        received: impl Into<String>,
    ) -> Self {
        match crate::platform::markup::unreadable(answer, received.into()) {
            ResolveError::UnexpectedResponse(received) => Self::drift(step, expected, received),
            other => Self::from_error(step, expected, &other),
        }
    }

    /// A failed request of a probe, described by the request's own error
    /// (a status, a timeout, a refused redirect).
    pub fn fetch(step: Step, expected: impl Into<String>, error: FetchError) -> Self {
        let mut failure = Self::from_error(step, expected, &ResolveError::from(error.clone()));
        if failure.outcome != Outcome::Challenged {
            failure.received = error.to_string();
        }
        failure
    }
}

/// A register a portal's search listed (step 2), as a citation would name
/// it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Register {
    /// The locality as a citation writes it, which may differ from the
    /// portal's style (`Le Bourg` for `Bourg (Le)`).
    pub locality: String,
    /// The call number, where the portal shows one: some portals show none,
    /// and their citations name registers without it.
    pub call_number: Option<String>,
    /// The period as the portal displays it.
    pub period: Option<String>,
    /// The image count, where the results show it; otherwise
    /// [`Probe::images`] reads it for the chosen register.
    pub images: Option<u16>,
    /// The address that opens the register's images: a viewer endpoint, an
    /// ARK, a register identifier.
    pub address: Option<String>,
    /// The act or matricule numbers the register spans, where the results
    /// show them: a volume of a military register.
    pub numbers: Option<(u32, u32)>,
}

impl Register {
    /// The first year of the displayed period.
    fn year(&self) -> Option<u16> {
        period_ranges(self.period.as_deref()?)
            .into_iter()
            .map(|(first, _)| first)
            .next()
    }

    /// Whether the register has what a citation of it needs: an image
    /// address, a year in the collection's period, and images when the
    /// results count them.
    fn is_citable(&self, collection: &Collection) -> bool {
        self.images.is_none_or(|images| images > 0)
            && self.address.is_some()
            && self
                .year()
                .is_some_and(|year| collection.covers(Some(year)))
    }

    /// Whether `other` would pass every selection criterion this register's
    /// citation sets: then the citation cannot tell them apart.
    fn is_confusable_with(&self, other: &Self) -> bool {
        let same_call_number = match (&self.call_number, &other.call_number) {
            (Some(mine), Some(theirs)) => CallNumber::new(mine.as_str()).matches(theirs),
            (None, _) => true,
            (Some(_), None) => false,
        };
        let year = self.year();
        // The citation names the first number this register spans.
        let same_numbers = match (self.numbers, other.numbers) {
            (Some((first, _)), Some((from, to))) => (from..=to).contains(&first),
            _ => true,
        };
        same_call_number
            && same_numbers
            && self.images == other.images
            && other
                .period
                .as_deref()
                .zip(year)
                .is_some_and(|(period, year)| {
                    period_ranges(period)
                        .iter()
                        .any(|(first, last)| (*first..=*last).contains(&year))
                })
    }
}

/// What a platform contributes to the live checks: where its portal lists
/// its references and localities, and how to search one locality. Each
/// adapter has one, in a `live` module beside it, listed by [`probe`].
pub trait Probe: Send + Sync {
    /// Step 1: loads the collection's search page and whatever the portal
    /// declares its references in, checks that the `portal` settings'
    /// references are all still there, and returns the alphabetically first
    /// locality the portal's own locality filter lists, as a citation writes
    /// it (`Le Bourg` for the portal's `Bourg (Le)`).
    fn search_page<'a>(
        &'a self,
        collection: &'a Collection,
        fetch: &'a dyn PortalFetch,
    ) -> BoxFuture<'a, Result<String, Failure>>;

    /// Step 2: the registers the collection's search lists for `locality`,
    /// as a citation writes it, and `act`, without a year: the search a
    /// citation of them would send, less the year.
    fn registers<'a>(
        &'a self,
        collection: &'a Collection,
        locality: &'a str,
        act: &'a Act,
        fetch: &'a dyn PortalFetch,
    ) -> BoxFuture<'a, Result<Vec<Register>, Failure>>;

    /// The image count of a chosen register its results did not count, as
    /// the portal's own pages give it; `None` by default, which fails the
    /// discovery.
    fn images<'a>(
        &'a self,
        _collection: &'a Collection,
        _register: &'a Register,
        _fetch: &'a dyn PortalFetch,
    ) -> BoxFuture<'a, Result<Option<u16>, Failure>> {
        Box::pin(async { Ok(None) })
    }

    /// Whether the collection's portal has an address per view. Without
    /// one, a register resolves to a `View` with no views, which opens on
    /// its first view: step 4 checks that view. Such a register may also
    /// go uncounted ([`Probe::images`] answering `None`) where counting it
    /// would cost an opening of the viewer: it is then cited at its first
    /// view.
    fn addresses_views(&self, _collection: &Collection) -> bool {
        true
    }
}

/// The probe of a platform.
pub fn probe(platform: &str) -> Option<&'static dyn Probe> {
    match platform {
        "archinoe" => Some(&crate::platform::Archinoe),
        "arkotheque" => Some(&crate::platform::Arkotheque),
        "gaia" => Some(&crate::platform::Gaia),
        "ligeo" => Some(&crate::platform::Ligeo),
        "mnesys" => Some(&crate::platform::Mnesys),
        "prismia" => Some(&crate::platform::Prismia),
        "thot" => Some(&crate::platform::Thot),
        _ => None,
    }
}

/// How a collection was checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Transport {
    Native,
    Browser,
}

/// What steps 4 and 5 open: the first cited view of the resolved register.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Opening {
    /// The platform, whose viewer the browser check reads.
    pub platform: String,
    /// The portal page opened on the view.
    pub url: String,
    /// The one-based view the viewer must show.
    pub view: u16,
    pub view_count: Option<u16>,
    /// For a `display: "iiif"` archive.
    pub image: Option<ArchiveImage>,
}

/// One collection's check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CollectionReport {
    pub collection: String,
    /// The collection's position in its archive.
    pub index: usize,
    pub platform: String,
    pub transport: Transport,
    pub outcome: Outcome,
    pub failure: Option<Failure>,
    /// The requests the check sent to the portal.
    pub requests: u32,
    /// The citation built from the portal, in the normalized form.
    /// The locality the discovery searched, as a citation writes it.
    pub locality: Option<String>,
    pub citation: Option<String>,
    pub opening: Option<Opening>,
}

/// One archive's checks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ArchiveReport {
    pub archive: String,
    /// The worst outcome of its collections.
    pub outcome: Outcome,
    pub collections: Vec<CollectionReport>,
}

impl ArchiveReport {
    pub fn new(archive: &Archive, collections: Vec<CollectionReport>) -> Self {
        Self {
            archive: archive.id.clone(),
            outcome: collections
                .iter()
                .map(|report| report.outcome)
                .max()
                .unwrap_or(Outcome::Ok),
            collections,
        }
    }
}

/// The archives the live checks visit: those with a collection, whose
/// catalogue entry does not opt out, or the one named `only`.
pub fn archives<'r>(
    registry: &'r ArchiveRegistry,
    only: Option<&str>,
) -> Result<Vec<&'r Archive>, String> {
    let checked: Vec<_> = registry
        .archives()
        .iter()
        .filter(|archive| !archive.collections.is_empty() && archive.live_check)
        .filter(|archive| only.is_none_or(|id| archive.id == id))
        .collect();
    match only {
        Some(id) if checked.is_empty() => Err(format!(
            "`{id}` is not a catalogued archive with an adapter and live checks"
        )),
        _ => Ok(checked),
    }
}

/// Whether a collection's portal admits only a browser.
pub fn needs_browser(registry: &ArchiveRegistry, collection: &Collection) -> bool {
    registry
        .platform(&collection.platform)
        .and_then(|platform| platform.endpoint(collection))
        .is_some_and(|endpoint| endpoint.access == Access::Browser)
}

/// Runs steps 1 to 3 on one collection over `transport`, sequentially.
pub async fn check_collection(
    registry: &ArchiveRegistry,
    archive: &Archive,
    index: usize,
    transport: &dyn PortalTransport,
) -> CollectionReport {
    let collection = &archive.collections[index];
    let counting = Counting::new(transport);
    let mut report = CollectionReport {
        collection: collection.id.clone(),
        index,
        platform: collection.platform.clone(),
        transport: if transport.is_browser() {
            Transport::Browser
        } else {
            Transport::Native
        },
        outcome: Outcome::Ok,
        failure: None,
        requests: 0,
        locality: None,
        citation: None,
        opening: None,
    };
    match steps(registry, archive, collection, &counting, &mut report).await {
        Ok(opening) => report.opening = Some(opening),
        Err(failure) => {
            report.outcome = failure.outcome;
            report.failure = Some(failure);
        }
    }
    report.requests = counting.requests.load(Ordering::Relaxed);
    report
}

async fn steps(
    registry: &ArchiveRegistry,
    archive: &Archive,
    collection: &Collection,
    transport: &Counting<'_>,
    report: &mut CollectionReport,
) -> Result<Opening, Failure> {
    let probe = probe(&collection.platform).ok_or_else(|| {
        Failure::drift(
            Step::SearchPage,
            "a live probe for the platform",
            format!("none for `{}`", collection.platform),
        )
    })?;
    let endpoint = registry
        .platform(&collection.platform)
        .and_then(|platform| platform.endpoint(collection))
        .ok_or_else(|| Failure::drift(Step::SearchPage, "valid portal settings", "refused"))?;
    let fetch = transport
        .connect(&endpoint)
        .await
        .map_err(|error| Failure::fetch(Step::SearchPage, "the portal's search page", error))?;
    // The pace the portal asks of robots, for every request that follows.
    if let Ok(robots) = fetch.get("/robots.txt").await {
        transport.space(crawl_delay(&robots));
    }

    let locality = probe.search_page(collection, fetch.as_ref()).await?;
    report.locality = Some(locality.clone());
    // The first document kind: an act, a table or a series alike.
    let act = collection
        .acts
        .first()
        .ok_or_else(|| Failure::drift(Step::Discovery, "an act in the collection", "no act"))?;
    let registers = probe
        .registers(collection, &locality, act, fetch.as_ref())
        .await?;
    let mut register = choose(&registers, &locality, collection)?.clone();
    if register.images.is_none() {
        register.images = probe
            .images(collection, &register, fetch.as_ref())
            .await?
            .filter(|images| *images > 0);
    }
    drop(fetch);
    let addresses_views = probe.addresses_views(collection);
    if register.images.is_none() && addresses_views {
        return Err(Failure::drift(
            Step::Discovery,
            "the chosen register's image count",
            "none",
        ));
    }

    let citation = citation_of(archive, act, &register);
    report.citation = Some(title_of(&citation));
    resolution(
        registry,
        archive,
        collection,
        transport,
        &citation,
        addresses_views,
    )
    .await
}

/// Step 2's verdict: the first register a citation can name without being
/// confused with another, or failing that the first citable one.
fn choose<'r>(
    registers: &'r [Register],
    locality: &str,
    collection: &Collection,
) -> Result<&'r Register, Failure> {
    let searched = fold(locality);
    let citable: Vec<&Register> = registers
        .iter()
        .filter(|register| register.is_citable(collection))
        .collect();
    let at_locality: Vec<&Register> = citable
        .iter()
        .copied()
        .filter(|register| fold(&register.locality) == searched)
        .collect();
    let mut pool = if at_locality.is_empty() {
        citable
    } else {
        at_locality
    };
    // A register its citation can name by call number first.
    pool.sort_by_key(|register| register.call_number.is_none());
    pool.iter()
        .copied()
        .find(|register| {
            registers
                .iter()
                .filter(|other| register.is_confusable_with(other))
                .count()
                == 1
        })
        .or_else(|| pool.first().copied())
        .ok_or_else(|| {
            let count = |test: fn(&Register) -> bool| registers.iter().filter(|r| test(r)).count();
            Failure::drift(
                Step::Discovery,
                "a register with an image address and a year in the collection's period",
                format!(
                    "{} registers: {} with a call number, {} with images, {} with an image address, {} with a year",
                    registers.len(),
                    count(|r| r.call_number.is_some()),
                    count(|r| r.images.is_some_and(|images| images > 0)),
                    count(|r| r.address.is_some()),
                    count(|r| r.year().is_some()),
                ),
            )
        })
}

/// The citation of a chosen register, cited at its middle view, or at its
/// first one when its images are not counted.
fn citation_of(archive: &Archive, act: &Act, register: &Register) -> CitationParts {
    let year = register.year();
    CitationParts {
        code: archive.citation_codes[0].clone(),
        locality: register.locality.clone(),
        parish: None,
        act: act.clone(),
        year,
        period: year.map(|year| year.to_string()),
        call_number: register.call_number.as_deref().map(CallNumber::new),
        number: register.numbers.map(|(first, _)| first),
        views: vec![CitedView {
            view: register.images.map_or(1, |images| images.div_ceil(2)),
            side: None,
        }],
        view_count: register.images,
    }
}

/// A citation in the normalized form, for the report.
fn title_of(citation: &CitationParts) -> String {
    let mut fields = vec![
        citation.code.clone(),
        citation.locality.clone(),
        "(aucun)".to_owned(),
        citation.act.to_string(),
    ];
    fields.extend(citation.period.clone());
    fields.extend(
        citation
            .call_number
            .as_ref()
            .map(|call_number| call_number.as_str().to_owned()),
    );
    fields.extend(citation.number.map(|number| format!("n° {number}")));
    if let (Some(view), Some(count)) = (citation.views.first(), citation.view_count) {
        fields.push(format!("vue {}/{count}", view.view));
    }
    fields.join(crate::citation::SEPARATOR)
}

/// Step 3: the citation resolves to its view of its register, and without
/// its call number to that register or to the results.
async fn resolution(
    registry: &ArchiveRegistry,
    archive: &Archive,
    collection: &Collection,
    transport: &dyn PortalTransport,
    citation: &CitationParts,
    addresses_views: bool,
) -> Result<Opening, Failure> {
    let expected = || {
        format!(
            "View of the register, view {}/{}",
            citation.views[0].view,
            citation.view_count.unwrap_or_default()
        )
    };
    let target = Resolver::new(registry)
        .resolve(citation, transport)
        .await
        .map_err(|error| Failure::from_error(Step::Resolution, expected(), &error))?;
    let opening = check_view(archive, collection, citation, &target, addresses_views)
        .map_err(|received| Failure::drift(Step::Resolution, expected(), received))?;
    if citation.call_number.is_none() {
        return Ok(opening);
    }

    let uncalled = CitationParts {
        call_number: None,
        ..citation.clone()
    };
    let without = Resolver::new(registry)
        .resolve(&uncalled, transport)
        .await
        .map_err(|error| {
            Failure::from_error(
                Step::Resolution,
                "without the call number, the register or the results",
                &error,
            )
        })?;
    match &without {
        ArchiveTarget::View { url, .. } if *url != opening.url => Err(Failure::drift(
            Step::Resolution,
            "without the call number, the register or the results",
            "another register",
        )),
        _ => Ok(opening),
    }
}

/// The opening of a resolved target, or what differs from the citation.
fn check_view(
    archive: &Archive,
    collection: &Collection,
    citation: &CitationParts,
    target: &ArchiveTarget,
    addresses_views: bool,
) -> Result<Opening, String> {
    let (views, view_count, call_number, attribution) = match target {
        ArchiveTarget::View {
            views,
            view_count,
            call_number,
            attribution,
            ..
        } => (views, view_count, call_number, attribution),
        ArchiveTarget::Results { matches, .. } => {
            return Err(match matches {
                Some(count) => format!("Results with {count} matches"),
                None => "Results built offline".to_owned(),
            });
        }
    };
    // A register its portal lists without a call number is cited without
    // one, and found by the other parts.
    if let Some(cited) = &citation.call_number
        && !call_number
            .as_deref()
            .is_some_and(|found| cited.matches(found))
    {
        return Err(format!(
            "View of a register with {} call number than {}",
            if call_number.is_some() {
                "another"
            } else {
                "no"
            },
            cited.as_str()
        ));
    }
    if !addresses_views {
        // The register, which its viewer opens on the first view.
        if !views.is_empty() {
            return Err("View with views of a viewer without an address per view".to_owned());
        }
        return Ok(Opening {
            platform: collection.platform.clone(),
            url: target.url().to_owned(),
            view: 1,
            view_count: view_count.or(citation.view_count),
            image: None,
        });
    }
    let view = citation.views[0].view;
    let Some(first) = views.first().filter(|first| first.view == view) else {
        return Err(format!(
            "View with views {:?} of {} images",
            views.iter().map(|view| view.view).collect::<Vec<_>>(),
            view_count.unwrap_or_default()
        ));
    };
    // An adapter that does not count a register's images leaves the count
    // to the viewer, which step 4 reads.
    if let Some(count) = view_count
        && Some(*count) != citation.view_count
    {
        return Err(format!("View of a register of {count} images"));
    }
    if archive.display == crate::Display::Iiif {
        if first.image.is_none() {
            return Err("View without the image of a `display: iiif` archive".to_owned());
        }
        if attribution
            .as_deref()
            .is_none_or(|text| text.contains(['{', '}']))
        {
            return Err("an attribution with a placeholder left, or none".to_owned());
        }
    }
    Ok(Opening {
        platform: collection.platform.clone(),
        url: first.url.clone(),
        view,
        // Counted by the probe where the adapter does not count.
        view_count: view_count.or(citation.view_count),
        image: first.image.clone(),
    })
}

/// The longest pause a portal's `Crawl-delay` imposes between two requests
/// of a check.
const MAX_CRAWL_DELAY: Duration = Duration::from_secs(30);

/// The `Crawl-delay` a `robots.txt` asks of every robot (`User-agent: *`) or
/// of OxidGene, bounded by [`MAX_CRAWL_DELAY`]; none when it asks none.
pub(crate) fn crawl_delay(robots: &str) -> Duration {
    let mut delay = 0.0_f64;
    let mut applies = false;
    let mut in_agents = false;
    for line in robots.lines() {
        let line = line.split('#').next().unwrap_or_default().trim();
        let Some((field, value)) = line.split_once(':') else {
            continue;
        };
        let (field, value) = (field.trim().to_ascii_lowercase(), value.trim());
        if field == "user-agent" {
            let agent = value.to_ascii_lowercase();
            let ours = agent == "*" || agent.contains("oxidgene");
            // Consecutive agents share a group; a new group starts after a
            // rule.
            applies = if in_agents { applies || ours } else { ours };
            in_agents = true;
            continue;
        }
        in_agents = false;
        if applies
            && field == "crawl-delay"
            && let Ok(seconds) = value.parse::<f64>()
            && seconds.is_finite()
        {
            delay = delay.max(seconds);
        }
    }
    Duration::from_secs_f64(delay.clamp(0.0, MAX_CRAWL_DELAY.as_secs_f64()))
}

/// Counts the requests a check sends, over whatever transport carries them,
/// and spaces them by the portal's `Crawl-delay`. Requests are sequential,
/// so the pause blocks the check's own thread.
struct Counting<'t> {
    inner: &'t dyn PortalTransport,
    requests: AtomicU32,
    pace: Mutex<Pace>,
}

/// The spacing of a check's requests.
#[derive(Default)]
struct Pace {
    delay: Duration,
    last: Option<Instant>,
}

impl<'t> Counting<'t> {
    fn new(inner: &'t dyn PortalTransport) -> Self {
        Self {
            inner,
            requests: AtomicU32::new(0),
            pace: Mutex::new(Pace::default()),
        }
    }

    /// Spaces the following requests by `delay`.
    fn space(&self, delay: Duration) {
        if let Ok(mut pace) = self.pace.lock() {
            pace.delay = delay;
        }
    }

    /// Counts a request about to start, once the portal's pause is over.
    fn start(&self) {
        self.requests.fetch_add(1, Ordering::Relaxed);
        let Ok(mut pace) = self.pace.lock() else {
            return;
        };
        if let Some(wait) = pace
            .last
            .map(|last| pace.delay.saturating_sub(last.elapsed()))
            .filter(|wait| !wait.is_zero())
        {
            std::thread::sleep(wait);
        }
        pace.last = Some(Instant::now());
    }
}

struct CountingFetch<'c> {
    inner: Box<dyn PortalFetch + 'c>,
    counting: &'c Counting<'c>,
}

impl PortalTransport for Counting<'_> {
    fn is_browser(&self) -> bool {
        self.inner.is_browser()
    }

    fn connect<'a>(
        &'a self,
        endpoint: &'a crate::PortalEndpoint,
    ) -> BoxFuture<'a, Result<Box<dyn PortalFetch + 'a>, FetchError>> {
        Box::pin(async move {
            let inner = self.inner.connect(endpoint).await?;
            Ok(Box::new(CountingFetch {
                inner,
                counting: self,
            }) as Box<dyn PortalFetch + 'a>)
        })
    }
}

impl PortalFetch for CountingFetch<'_> {
    fn request<'a>(
        &'a self,
        request: &'a PortalRequest,
    ) -> BoxFuture<'a, Result<String, FetchError>> {
        self.counting.start();
        self.inner.request(request)
    }
}

#[cfg(test)]
mod tests;
