//! The registry and the resolver, over a fictitious archive whose collections
//! are served by a scripted adapter.

use std::pin::pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll, Waker};

use super::*;
use crate::platform::BoxFuture;
use crate::transport::PortalRequest;

/// Drives a future whose every step completes at once, as the scripted
/// adapters and transports of the crate's tests do.
pub(crate) fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let mut context = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
            return output;
        }
    }
}

/// Answers as its collection's `portal.outcome` says, after one request.
pub(crate) struct Scripted;

impl Scripted {
    fn outcome(collection: &Collection) -> &str {
        collection.portal["outcome"].as_str().unwrap_or_default()
    }
}

impl Platform for Scripted {
    fn id(&self) -> &'static str {
        "scripted"
    }

    fn validate(&self, collection: &Collection) -> Result<(), CatalogError> {
        collection.portal["outcome"]
            .is_string()
            .then_some(())
            .ok_or_else(|| CatalogError::new("scripted settings: outcome"))
    }

    fn endpoint(&self, collection: &Collection) -> Option<PortalEndpoint> {
        let access = if collection.portal["browser"].as_bool() == Some(true) {
            Access::Browser
        } else {
            Access::Any
        };
        Some(PortalEndpoint {
            origin: "https://archives.example.org".to_owned(),
            other_origins: Vec::new(),
            start: format!("https://archives.example.org/{}", collection.id),
            access,
            insecure_http: false,
        })
    }

    fn results_url(&self, collection: &Collection, citation: &CitationParts) -> Option<String> {
        Some(format!(
            "https://archives.example.org/{}?act={}",
            collection.id, citation.act
        ))
    }

    fn resolve<'a>(
        &'a self,
        archive: &'a Archive,
        collection: &'a Collection,
        citation: &'a CitationParts,
        fetch: &'a dyn PortalFetch,
    ) -> BoxFuture<'a, Result<ArchiveTarget, ResolveError>> {
        Box::pin(async move {
            fetch.get(&format!("/{}/search", collection.id)).await?;
            let url = self.results_url(collection, citation).unwrap();
            match Self::outcome(collection) {
                "view" => Ok(ArchiveTarget::View {
                    url: format!("{url}#view"),
                    views: Vec::new(),
                    view_count: citation.view_count,
                    call_number: citation.call_number.as_ref().map(|c| c.as_str().to_owned()),
                    attribution: archive.attribution_for(None, &[]),
                    renumbering: None,
                }),
                // A register of fewer images than the cited view, showing
                // another call number, or the cited one (a person's row).
                outcome @ ("short" | "short-cited") => Ok(ArchiveTarget::View {
                    url: format!("{url}#first"),
                    views: Vec::new(),
                    view_count: Some(10),
                    call_number: if outcome == "short" {
                        Some("9 Z 9".to_owned())
                    } else {
                        citation.call_number.as_ref().map(|c| c.as_str().to_owned())
                    },
                    attribution: archive.attribution_for(None, &[]),
                    renumbering: None,
                }),
                "none" => Ok(ArchiveTarget::Results {
                    url,
                    matches: Some(0),
                }),
                "several" => Ok(ArchiveTarget::Results {
                    url,
                    matches: Some(2),
                }),
                "timeout" => Err(FetchError::Timeout.into()),
                _ => Err(ResolveError::UnexpectedResponse("no rows".to_owned())),
            }
        })
    }
}

/// Counts the fetchers it opens and the requests they send.
#[derive(Default)]
struct Counting {
    browser: bool,
    connects: AtomicUsize,
    requests: AtomicUsize,
}

struct CountingFetch<'a>(&'a AtomicUsize);

impl PortalFetch for CountingFetch<'_> {
    fn request<'a>(
        &'a self,
        request: &'a PortalRequest,
    ) -> BoxFuture<'a, Result<String, FetchError>> {
        Box::pin(async move {
            assert!(request.url.starts_with('/'));
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(String::new())
        })
    }
}

impl PortalTransport for Counting {
    fn is_browser(&self) -> bool {
        self.browser
    }

    fn connect<'a>(
        &'a self,
        endpoint: &'a PortalEndpoint,
    ) -> BoxFuture<'a, Result<Box<dyn PortalFetch + 'a>, FetchError>> {
        Box::pin(async move {
            assert_eq!(endpoint.origin, "https://archives.example.org");
            self.connects.fetch_add(1, Ordering::SeqCst);
            Ok(Box::new(CountingFetch(&self.requests)) as Box<dyn PortalFetch>)
        })
    }
}

/// An archive with a parish-register collection up to 1792 and a
/// civil-status collection from 1792, with the given outcomes.
fn registry(parish: serde_json::Value, civil: serde_json::Value) -> ArchiveRegistry {
    let document = serde_json::json!({
        "id": "fr-ad00",
        "country": "FR",
        "level": "departmental",
        "name": "Archives départementales d'Exemple",
        "citation_codes": ["AD00"],
        "website": "https://archives.example.org",
        "citation": { "view_words": ["vue", "image"] },
        "collections": [{
            "id": "parish-registers",
            "acts": ["B", "M", "S"],
            "period": [null, 1792],
            "platform": "scripted",
            "portal": parish,
        }, {
            "id": "civil-status",
            "acts": ["N", "M", "D"],
            "period": [1792, null],
            "platform": "scripted",
            "portal": civil,
        }]
    })
    .to_string();
    ArchiveRegistry::new(&[("fr", document.as_str())], vec![Box::new(Scripted)])
        .expect("a valid catalogue")
}

fn outcome(outcome: &str) -> serde_json::Value {
    serde_json::json!({ "outcome": outcome })
}

fn citation(registry: &ArchiveRegistry, title: &str) -> CitationParts {
    registry.parse(title).expect("a normalized citation")
}

const MARRIAGE_1792: &str = "AD00 - Exampleville - (aucun) - M - 1792 - 3E1/2 - vue 5/13";

#[test]
fn tries_the_collections_in_order_until_one_finds_the_register() {
    let registry = registry(outcome("none"), outcome("view"));
    let resolver = Resolver::new(&registry);
    let transport = Counting::default();
    let citation = citation(&registry, MARRIAGE_1792);

    let target = block_on(resolver.resolve(&citation, &transport)).unwrap();
    assert_eq!(
        target.url(),
        "https://archives.example.org/civil-status?act=M#view"
    );
    assert!(matches!(
        &target,
        ArchiveTarget::View { call_number: Some(call_number), view_count: Some(13), .. }
            if call_number == "3E1/2"
    ));
    assert_eq!(transport.connects.load(Ordering::SeqCst), 2);
    assert_eq!(transport.requests.load(Ordering::SeqCst), 2);
}

/// A regression: a citation of an older digitisation chose a register of
/// fewer images than its cited view, opened on its first image. Such a
/// register is not the cited one: the filtered results, with the banner of
/// no register, unless another collection finds it.
#[test]
fn a_cited_view_beyond_the_chosen_register_gives_the_results() {
    let title = "AD00 - Exampleville - (aucun) - M - 1850 - acte 14 - vue 40/510";
    let registry = registry(outcome("none"), outcome("short"));
    let resolver = Resolver::new(&registry);
    let target = block_on(resolver.resolve(&citation(&registry, title), &Counting::default()));
    assert_eq!(
        target,
        Ok(ArchiveTarget::Results {
            url: "https://archives.example.org/civil-status?act=M".to_owned(),
            matches: Some(0),
        })
    );
    // Within its images, the register opens as it is.
    let title = "AD00 - Exampleville - (aucun) - M - 1850 - acte 14 - vue 4/510";
    let target = block_on(resolver.resolve(&citation(&registry, title), &Counting::default()));
    assert!(
        matches!(target, Ok(ArchiveTarget::View { .. })),
        "{target:?}"
    );
    // A register carrying the cited call number is the cited one.
    let registry = super::tests::registry(outcome("none"), outcome("short-cited"));
    let resolver = Resolver::new(&registry);
    let title = "AD00 - Exampleville - (aucun) - M - 1850 - 3E1/2 - vue 40/510";
    let target = block_on(resolver.resolve(&citation(&registry, title), &Counting::default()));
    assert!(
        matches!(target, Ok(ArchiveTarget::View { .. })),
        "{target:?}"
    );
}

#[test]
fn keeps_resolved_targets_for_the_session() {
    let registry = registry(outcome("view"), outcome("view"));
    let resolver = Resolver::new(&registry);
    let transport = Counting::default();
    let citation = citation(&registry, MARRIAGE_1792);

    let first = block_on(resolver.resolve(&citation, &transport)).unwrap();
    let second = block_on(resolver.resolve(&citation, &transport)).unwrap();
    assert_eq!(first, second);
    assert_eq!(transport.requests.load(Ordering::SeqCst), 1);
}

#[test]
fn falls_back_to_the_first_search_results() {
    let registry = registry(outcome("several"), outcome("none"));
    let resolver = Resolver::new(&registry);
    let transport = Counting::default();
    let citation = citation(&registry, MARRIAGE_1792);

    assert_eq!(
        block_on(resolver.resolve(&citation, &transport)),
        Ok(ArchiveTarget::Results {
            url: "https://archives.example.org/parish-registers?act=M".to_owned(),
            matches: Some(2),
        })
    );
    // Results are kept too.
    block_on(resolver.resolve(&citation, &transport)).unwrap();
    assert_eq!(transport.requests.load(Ordering::SeqCst), 2);
}

#[test]
fn search_results_win_over_a_failing_collection_and_errors_over_nothing() {
    let searched = registry(outcome("timeout"), outcome("none"));
    let citation = citation(&searched, MARRIAGE_1792);
    let target = block_on(Resolver::new(&searched).resolve(&citation, &Counting::default()));
    assert!(matches!(
        target,
        Ok(ArchiveTarget::Results {
            matches: Some(0),
            ..
        })
    ));

    let failing = registry(outcome("timeout"), outcome("changed"));
    let resolver = Resolver::new(&failing);
    let transport = Counting::default();
    assert_eq!(
        block_on(resolver.resolve(&citation, &transport)),
        Err(ResolveError::Timeout)
    );
    // A failure is not kept: the next click asks the portal again.
    block_on(resolver.resolve(&citation, &transport)).unwrap_err();
    assert_eq!(transport.requests.load(Ordering::SeqCst), 4);
}

#[test]
fn a_browser_portal_without_a_browser_gives_the_offline_results() {
    let registry = registry(
        serde_json::json!({ "outcome": "view", "browser": true }),
        serde_json::json!({ "outcome": "view", "browser": true }),
    );
    let citation = citation(&registry, MARRIAGE_1792);

    let transport = Counting::default();
    assert_eq!(
        block_on(Resolver::new(&registry).resolve(&citation, &transport)),
        Ok(ArchiveTarget::Results {
            url: "https://archives.example.org/parish-registers?act=M".to_owned(),
            matches: None,
        })
    );
    assert_eq!(transport.connects.load(Ordering::SeqCst), 0);

    let browser = Counting {
        browser: true,
        ..Counting::default()
    };
    let target = block_on(Resolver::new(&registry).resolve(&citation, &browser)).unwrap();
    assert!(matches!(target, ArchiveTarget::View { .. }));
}

#[test]
fn a_year_outside_every_period_tries_every_collection_holding_the_act() {
    let registry = registry(outcome("none"), outcome("view"));
    // A burial is only held by the parish registers, which end in 1792.
    let late = citation(&registry, "AD00 - Exampleville - (aucun) - S - 1850");
    let (_, collections) = registry.candidates(&late).unwrap();
    assert_eq!(collections.len(), 1);
    assert_eq!(collections[0].id, "parish-registers");

    let birth = citation(&registry, "AD00 - Exampleville - (aucun) - N - 1850");
    let (_, collections) = registry.candidates(&birth).unwrap();
    assert_eq!(collections[0].id, "civil-status");
}

#[test]
fn refuses_what_no_adapter_serves() {
    let registry = registry(outcome("view"), outcome("view"));
    let resolver = Resolver::new(&registry);
    for title in [
        "AD99 - Exampleville - (aucun) - N - 1850",
        "AD00 - Exampleville - (aucun) - TD - 1850",
    ] {
        let citation = citation(&registry, title);
        assert_eq!(
            block_on(resolver.resolve(&citation, &Counting::default())),
            Err(ResolveError::NoAdapter),
            "{title}"
        );
        assert_eq!(
            registry.offline_target(&citation),
            Err(ResolveError::NoAdapter)
        );
        assert!(registry.link(title).is_none(), "{title}");
    }
}

#[test]
fn offers_links_for_catalogued_archives_holding_the_act() {
    let registry = registry(outcome("view"), outcome("view"));
    let (archive, citation) = registry
        .link("AD00 - Exampleville - (aucun) - BMS - 1750 - image 3/40")
        .expect("a catalogued register");
    assert_eq!(archive.id, "fr-ad00");
    // The archive's own grammar reads `image` as a view word.
    assert_eq!(citation.views.len(), 1);
    assert_eq!(
        registry.offline_target(&citation),
        Ok(ArchiveTarget::Results {
            url: "https://archives.example.org/parish-registers?act=BMS".to_owned(),
            matches: None,
        })
    );
    assert!(registry.link("Parish register of Exampleville").is_none());
    assert_eq!(
        registry.platform("scripted").map(Platform::id),
        Some("scripted")
    );
    assert!(registry.platform("unknown").is_none());
}

#[test]
fn the_resolution_future_can_cross_threads() {
    fn assert_send<T: Send>(_: &T) {}
    let registry = registry(outcome("view"), outcome("view"));
    let resolver = Resolver::new(&registry);
    let citation = citation(&registry, MARRIAGE_1792);
    let transport = Counting::default();
    let future = resolver.resolve(&citation, &transport);
    assert_send(&future);
    block_on(future).unwrap();
}

#[test]
fn errors_have_stable_codes() {
    assert_eq!(ResolveError::NoAdapter.code(), "no_adapter");
    assert_eq!(
        ResolveError::UnexpectedResponse(String::new()).code(),
        "unexpected_response"
    );
    assert_eq!(ResolveError::Challenged.code(), "challenged");
    assert_eq!(ResolveError::Timeout.code(), "timeout");
    assert_eq!(ResolveError::Unreachable.code(), "unreachable");

    assert_eq!(
        ResolveError::from(FetchError::Timeout),
        ResolveError::Timeout
    );
    assert_eq!(
        ResolveError::from(FetchError::Network),
        ResolveError::Unreachable
    );
    assert_eq!(
        ResolveError::from(FetchError::Status(503)),
        ResolveError::Unreachable
    );
    // A gateway's timeout is the portal's.
    assert_eq!(
        ResolveError::from(FetchError::Status(504)),
        ResolveError::Timeout
    );
    assert_eq!(
        ResolveError::from(FetchError::Status(404)).code(),
        "unexpected_response"
    );
}

#[test]
fn targets_serialize_with_their_kind() {
    let target = ArchiveTarget::Results {
        url: "https://archives.example.org/search".to_owned(),
        matches: Some(2),
    };
    assert_eq!(
        serde_json::to_value(&target).unwrap(),
        serde_json::json!({
            "kind": "results",
            "url": "https://archives.example.org/search",
            "matches": 2,
        })
    );
}

#[test]
fn the_embedded_registry_links_the_catalogued_archives() {
    let registry = ArchiveRegistry::embedded();
    let (archive, citation) = registry
        .link("AD44 - Exampleville - (aucun) - N - 1877 - 3E1/2 - acte 26 - vue 5d/13")
        .expect("a catalogued birth");
    assert_eq!(archive.id, "fr-ad44");
    assert_eq!(citation.locality, "Exampleville");
    assert!(
        registry
            .link("AD99 - Exampleville - (aucun) - N - 1877")
            .is_none()
    );
    for archive in registry.archives() {
        for collection in &archive.collections {
            assert!(registry.platform(&collection.platform).is_some());
        }
    }
}

#[test]
fn a_citation_page_completes_its_source_title() {
    let registry = ArchiveRegistry::embedded();
    let title = "AD44 - Exampleville - (aucun) - N - 1877 - 3E1/2";
    assert_eq!(cited_text(title, None), title);
    assert_eq!(cited_text(title, Some("  ")), title);

    let text = cited_text(title, Some("acte 26 - vue 5d/13"));
    assert_eq!(
        text,
        "AD44 - Exampleville - (aucun) - N - 1877 - 3E1/2 - acte 26 - vue 5d/13"
    );
    let (_, citation) = registry.link(&text).expect("a catalogued birth");
    assert_eq!(
        citation.views.iter().map(|v| v.view).collect::<Vec<_>>(),
        [5]
    );
    assert_eq!(citation.view_count, Some(13));

    // The page's views win over the title's.
    let text = cited_text(&format!("{title} - vue 2/13"), Some("vue 7/13"));
    let (_, citation) = registry.link(&text).expect("a catalogued birth");
    assert_eq!(
        citation.views.iter().map(|v| v.view).collect::<Vec<_>>(),
        [7]
    );
}
