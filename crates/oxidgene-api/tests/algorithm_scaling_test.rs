//! Algorithmic complexity of the tree-wide computations, measured in time.
//!
//! Each computation runs on two trees built from the same family block, one
//! eight times the size of the other. Linear work takes eight times as long
//! on the larger, quadratic work sixty-four times — in theory. In practice
//! the smaller tree fits in the processor's caches better than the larger,
//! which alone makes linear work take up to twice as long again. So each
//! growth is measured against the growth of a reference pass of known linear
//! cost over the same projections, one that reads them at random as the
//! computations do: linear work grows about as fast as the reference,
//! quadratic work several times faster once the trees are large enough for
//! the quadratic term to dominate — hence tens of thousands of persons for
//! the in-memory computations, replicated from one imported tree.
//!
//! A quadratic term with a small constant can still hide below that bound;
//! where one is known to lurk, a deterministic count of the work done guards
//! it in the normal suite instead (the duplicate candidates, for one). Timing
//! depends on the machine and wants an optimised build, so these tests are
//! opt-in: `just complexity` runs them in release mode. The SQL side of the
//! same question is answered deterministically by `query_scaling_test`.

mod common;

use std::collections::{HashMap, HashSet};
use std::hint::black_box;
use std::time::{Duration, Instant};

use axum::Router;
use axum::http::Method;
use chrono::{Datelike, NaiveDate};
use common::{
    app_on, family_blocks_gedcom, family_blocks_tree, import_job, ok, setup_db, worker_on,
};
use oxidgene_api::service::{ancestry, anomalies, duplicates, statistics};
use oxidgene_core::projection::PersonProfile;
use oxidgene_db::sea_orm::DatabaseConnection;
use serde_json::{Value, json};
use uuid::Uuid;

/// Family blocks of the smaller tree of the requests: 2 000 persons.
const SMALL: usize = 200;
/// Copies of that tree making the smaller tree of the in-memory
/// computations: 8 000 persons.
const SMALL_COPIES: usize = 4;
/// How many times larger the larger tree is.
const FACTOR: usize = 8;
/// Largest growth accepted, relative to the reference pass's: linear work
/// sits near 1, `n log n` work a little above, quadratic work near
/// [`FACTOR`].
const MAX_RELATIVE_GROWTH: f64 = 3.0;
/// Runs per measure; the median is kept.
const RUNS: usize = 3;

/// A tree of the survey: its id, its SOSA root and its projections.
struct Tree {
    id: String,
    root: Uuid,
    profiles: Vec<PersonProfile>,
}

async fn tree(app: &Router, db: &DatabaseConnection, blocks: usize) -> Tree {
    let (id, anchor) = family_blocks_tree(app, db, blocks).await;
    let json = common::all_profiles(app, &id).await;
    Tree {
        id,
        root: anchor.parse().unwrap(),
        profiles: serde_json::from_value(serde_json::Value::Array(json)).unwrap(),
    }
}

/// The smaller and the larger tree, over one router.
async fn trees() -> (Router, DatabaseConnection, Tree, Tree) {
    let db = setup_db().await;
    let app = app_on(db.clone());
    let small = tree(&app, &db, SMALL).await;
    let large = tree(&app, &db, SMALL * FACTOR).await;
    (app, db, small, large)
}

/// Years a tree of [`SMALL`] blocks spans, four blocks a year.
const SPAN_YEARS: i64 = (SMALL / 4) as i64;

/// `copies` copies of `base`, each with ids of its own and its dates moved
/// [`SPAN_YEARS`] earlier than the previous copy's, so that the tree grows
/// back in time as an imported one does, homonyms no denser. Copy 0 is
/// `base` itself.
fn replicated(base: &Tree, copies: usize) -> Tree {
    fn remap(value: &mut Value, copy: u128, years: i64) {
        match value {
            Value::String(text) => {
                if let Ok(id) = text.parse::<Uuid>() {
                    *text = Uuid::from_u128(id.as_u128() ^ (copy << 64)).to_string();
                }
            }
            Value::Array(items) => items.iter_mut().for_each(|v| remap(v, copy, years)),
            Value::Object(fields) => {
                for (key, field) in fields.iter_mut() {
                    match (key.as_str(), field.as_str()) {
                        ("date_sort", Some(date)) => {
                            let date: NaiveDate = date.parse().unwrap();
                            let moved = date.with_year(date.year() - years as i32).unwrap();
                            *field = Value::String(moved.to_string());
                        }
                        ("date_value", Some(year)) if year.parse::<i64>().is_ok() => {
                            *field =
                                Value::String((year.parse::<i64>().unwrap() - years).to_string());
                        }
                        _ => remap(field, copy, years),
                    }
                }
            }
            _ => {}
        }
    }
    let json = serde_json::to_value(&base.profiles).unwrap();
    let profiles = (0..copies)
        .flat_map(|copy| {
            let mut copy_json = json.clone();
            remap(&mut copy_json, copy as u128, copy as i64 * SPAN_YEARS);
            serde_json::from_value::<Vec<PersonProfile>>(copy_json).unwrap()
        })
        .collect();
    Tree {
        id: base.id.clone(),
        root: base.root,
        profiles,
    }
}

/// The smaller and the larger tree of the in-memory computations.
async fn replicated_trees() -> (Tree, Tree) {
    let db = setup_db().await;
    let base = tree(&app_on(db.clone()), &db, SMALL).await;
    (
        replicated(&base, SMALL_COPIES),
        replicated(&base, SMALL_COPIES * FACTOR),
    )
}

/// Median duration of [`RUNS`] runs of `f`.
fn median(mut f: impl FnMut()) -> Duration {
    let mut times: Vec<Duration> = (0..RUNS)
        .map(|_| {
            let start = Instant::now();
            f();
            start.elapsed()
        })
        .collect();
    times.sort();
    times[RUNS / 2]
}

/// The reference pass: every person's parents, spouses and children looked
/// up by id, as the computations under test do.
fn reference(profiles: &[PersonProfile]) {
    let by_id: HashMap<Uuid, &PersonProfile> = profiles.iter().map(|p| (p.person_id, p)).collect();
    let mut found = 0usize;
    for profile in profiles {
        let parents = profile
            .family_as_child
            .iter()
            .flat_map(|l| [l.father_id, l.mother_id]);
        let unions = profile.families_as_spouse.iter().flat_map(|l| {
            l.children_ids
                .iter()
                .copied()
                .map(Some)
                .chain([l.spouse_id])
        });
        found += parents
            .chain(unions)
            .flatten()
            .filter(|id| by_id.contains_key(id))
            .count();
    }
    black_box(found);
}

/// How many times longer the reference pass takes on the larger tree.
fn reference_growth(small: &Tree, large: &Tree) -> f64 {
    growth(
        median(|| reference(&small.profiles)),
        median(|| reference(&large.profiles)),
    )
}

fn growth(small: Duration, large: Duration) -> f64 {
    large.as_secs_f64() / small.as_secs_f64().max(1e-9)
}

/// Asserts that `what` grew at most [`MAX_RELATIVE_GROWTH`] times as fast as
/// the reference pass.
fn assert_scales(what: &str, small: Duration, large: Duration, reference: f64) {
    let growth = growth(small, large);
    let relative = growth / reference;
    eprintln!(
        "{what}: {small:?} → {large:?}, ×{growth:.1} for ×{FACTOR} persons \
         (reference ×{reference:.1}, relative ×{relative:.2})"
    );
    assert!(
        relative <= MAX_RELATIVE_GROWTH,
        "{what} grows faster than n log n: ×{growth:.1} for ×{FACTOR} persons, \
         ×{relative:.2} the reference pass's growth"
    );
}

fn today() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 1, 1).unwrap()
}

/// Times `compute` on the projections of both trees.
async fn scaling(what: &str, compute: impl Fn(&[PersonProfile], Uuid)) {
    let (small, large) = replicated_trees().await;
    let reference = reference_growth(&small, &large);
    assert_scales(
        what,
        median(|| compute(&small.profiles, small.root)),
        median(|| compute(&large.profiles, large.root)),
        reference,
    );
}

#[tokio::test]
#[ignore = "timing: run by `just complexity` in release mode"]
async fn scaling_duplicates() {
    scaling("duplicates", |profiles, _| {
        black_box(duplicates::potential_duplicates(profiles, &HashSet::new()));
    })
    .await;
}

#[tokio::test]
#[ignore = "timing: run by `just complexity` in release mode"]
async fn scaling_anomalies() {
    scaling("anomalies", |profiles, _| {
        black_box(anomalies::compute(profiles, &[], today()));
    })
    .await;
}

#[tokio::test]
#[ignore = "timing: run by `just complexity` in release mode"]
async fn scaling_statistics() {
    scaling("statistics", |profiles, _| {
        black_box(statistics::compute(
            profiles,
            &[],
            0,
            today(),
            false,
            |_| Vec::new(),
        ));
    })
    .await;
}

#[tokio::test]
#[ignore = "timing: run by `just complexity` in release mode"]
async fn scaling_ancestry() {
    scaling("ancestry completeness", |profiles, root| {
        black_box(ancestry::compute(profiles, root, 8, today()));
    })
    .await;
}

/// Median duration of [`RUNS`] of the request.
async fn timed_request(
    app: &Router,
    method: &Method,
    uri: &str,
    body: Option<serde_json::Value>,
) -> Duration {
    let mut runs = Vec::new();
    for _ in 0..RUNS {
        let start = Instant::now();
        ok(app, method.clone(), uri, body.clone()).await;
        runs.push(start.elapsed());
    }
    runs.sort();
    runs[RUNS / 2]
}

/// Times each request on both trees; `{tree}` is substituted.
async fn scaling_requests(requests: &[(Method, &str)]) {
    let (app, _db, small, large) = trees().await;
    let reference = reference_growth(&small, &large);
    for (method, uri) in requests {
        let on = |tree: &Tree| uri.replace("{tree}", &tree.id);
        assert_scales(
            &format!("{method} {uri}"),
            timed_request(&app, method, &on(&small), None).await,
            timed_request(&app, method, &on(&large), None).await,
            reference,
        );
    }
}

#[tokio::test]
#[ignore = "timing: run by `just complexity` in release mode"]
async fn scaling_import() {
    let (app, db, small, large) = trees().await;
    let reference = reference_growth(&small, &large);
    let worker = worker_on(&db);
    let mut times = Vec::new();
    for blocks in [SMALL, SMALL * FACTOR] {
        let gedcom = family_blocks_gedcom(blocks);
        let mut runs = Vec::new();
        for _ in 0..RUNS {
            let tree = ok(
                &app,
                Method::POST,
                "/api/v1/trees",
                Some(json!({ "name": "Scaling" })),
            )
            .await;
            let tree_id = tree["id"].as_str().unwrap();
            let start = Instant::now();
            let status = import_job(&app, &worker, tree_id, "format=gedcom", gedcom.clone()).await;
            runs.push(start.elapsed());
            assert_eq!(status["phase"], "completed", "{status}");
        }
        runs.sort();
        times.push(runs[RUNS / 2]);
    }
    assert_scales("GEDCOM import", times[0], times[1], reference);
}

#[tokio::test]
#[ignore = "timing: run by `just complexity` in release mode"]
async fn scaling_tree_wide_requests() {
    scaling_requests(&[
        (Method::POST, "/api/v1/trees/{tree}/profiles/rebuild"),
        (Method::GET, "/api/v1/trees/{tree}/gedcom/export"),
        (Method::GET, "/api/v1/trees/{tree}/statistics?lang=en"),
        (Method::GET, "/api/v1/trees/{tree}/dictionary/family-names"),
        (Method::GET, "/api/v1/trees/{tree}/dictionary/places"),
        (Method::GET, "/api/v1/trees/{tree}/dictionary/occupations"),
    ])
    .await;
}
