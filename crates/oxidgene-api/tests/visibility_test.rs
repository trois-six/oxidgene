//! `person_visibility`, the stored inputs of the contemporary rule
//! (docs/privacy.md §4.5, §11 "Refresh"): written through the API's own
//! writes, it must always equal what a full derivation of the tree's
//! records gives, and change in the transaction of the write that moves it —
//! the person's own row and those of their ancestors alike.

use std::collections::HashMap;
use std::sync::Arc;

use axum::Router;
use axum::http::Method;
use oxidgene_api::profile::ProfileService;
use oxidgene_api::{AppState, build_router};
use oxidgene_core::privacy::{PersonVisibility, derive_records};
use oxidgene_db::repo::{EventRepo, FamilyChildRepo, FamilyRepo, FamilySpouseRepo, PersonRepo};
use oxidgene_db::sea_orm::DatabaseConnection;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::common::{offline, ok, send, setup_db, test_media_root};

struct Fixture {
    app: Router,
    db: DatabaseConnection,
    profiles: Arc<ProfileService>,
    tree: String,
}

impl Fixture {
    async fn new() -> Self {
        let db = setup_db().await;
        let state = offline(AppState::new(db.clone(), test_media_root()));
        let profiles = state.profiles.clone();
        let app = build_router(state);
        let tree = ok(
            &app,
            Method::POST,
            "/api/v1/trees",
            Some(json!({"name": "Fernhill"})),
        )
        .await["id"]
            .as_str()
            .unwrap()
            .to_owned();
        Self {
            app,
            db,
            profiles,
            tree,
        }
    }

    fn tree_id(&self) -> Uuid {
        self.tree.parse().unwrap()
    }

    async fn post(&self, path: &str, body: Value) -> Value {
        ok(
            &self.app,
            Method::POST,
            &format!("/api/v1/trees/{}/{path}", self.tree),
            Some(body),
        )
        .await
    }

    async fn person(&self) -> Uuid {
        self.post("persons", json!({"sex": "unknown"})).await["id"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap()
    }

    async fn event(&self, person: Uuid, kind: &str, date: &str) -> String {
        self.post(
            "events",
            json!({"event_type": kind, "date_value": date, "person_id": person}),
        )
        .await["id"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    /// A family of `parents` with `child`; the family and the child link.
    async fn family(&self, parents: &[Uuid], child: Uuid) -> (String, String) {
        let family = self.post("families", json!({})).await["id"]
            .as_str()
            .unwrap()
            .to_owned();
        for (order, parent) in parents.iter().enumerate() {
            self.post(
                &format!("families/{family}/spouses"),
                json!({"person_id": parent, "role": if order == 0 { "husband" } else { "wife" }, "sort_order": order}),
            )
            .await;
        }
        let link = self
            .post(
                &format!("families/{family}/children"),
                json!({"person_id": child, "child_type": "biological", "sort_order": 0}),
            )
            .await["id"]
            .as_str()
            .unwrap()
            .to_owned();
        (family, link)
    }

    /// The stored rows of every live person, materialized if needed.
    async fn stored(&self) -> HashMap<Uuid, PersonVisibility> {
        let persons = PersonRepo::list_all(&self.db, self.tree_id())
            .await
            .unwrap();
        let ids: Vec<Uuid> = persons.iter().map(|p| p.id).collect();
        self.profiles
            .visibility(self.tree_id(), &ids)
            .await
            .unwrap()
    }

    /// What a full derivation of the tree's records gives.
    async fn derived(&self) -> HashMap<Uuid, PersonVisibility> {
        let tree = self.tree_id();
        let persons = PersonRepo::list_all(&self.db, tree).await.unwrap();
        let events = EventRepo::list_all(&self.db, tree).await.unwrap();
        let families = FamilyRepo::list_all(&self.db, tree).await.unwrap();
        let ids: Vec<Uuid> = families.iter().map(|f| f.id).collect();
        let spouses = FamilySpouseRepo::list_by_families(&self.db, &ids)
            .await
            .unwrap();
        let children = FamilyChildRepo::list_by_families(&self.db, &ids)
            .await
            .unwrap();
        derive_records(&persons, &events, &spouses, &children)
    }

    /// The stored rows, checked against a full derivation.
    async fn checked(&self) -> HashMap<Uuid, PersonVisibility> {
        let stored = self.stored().await;
        assert_eq!(stored, self.derived().await);
        stored
    }
}

/// A line of four generations: great-grandparent, grandparent, parent and
/// child, from the top down, undated.
async fn line(f: &Fixture) -> [Uuid; 4] {
    let ids = [
        f.person().await,
        f.person().await,
        f.person().await,
        f.person().await,
    ];
    for pair in ids.windows(2) {
        f.family(&[pair[0]], pair[1]).await;
    }
    ids
}

#[tokio::test]
async fn the_rows_follow_every_write_and_equal_a_full_derivation() {
    let f = Fixture::new().await;
    let [top, grand, parent, child] = line(&f).await;
    // The first read materializes the tree.
    let rows = f.checked().await;
    assert_eq!(rows.len(), 4);
    assert!(rows.values().all(|row| row.latest_birth_year.is_none()));

    // A great-grandchild's birth reaches every ancestor, a generation gap
    // earlier at each step.
    let birth = f.event(child, "birth", "1950").await;
    let rows = f.checked().await;
    assert_eq!(rows[&child].latest_birth_year, Some(1950));
    assert_eq!(rows[&parent].latest_birth_year, Some(1935));
    assert_eq!(rows[&grand].latest_birth_year, Some(1920));
    assert_eq!(rows[&top].latest_birth_year, Some(1905));

    // A parent's death dated: their own row says deceased, their child's
    // gains a bound.
    let other = f.person().await;
    f.family(&[other], top).await;
    f.event(other, "death", "1880").await;
    let rows = f.checked().await;
    assert!(rows[&other].deceased);
    assert_eq!(rows[&top].latest_birth_year, Some(1881));

    // The birth moved later: the ancestors' bounds follow.
    ok(
        &f.app,
        Method::PUT,
        &format!("/api/v1/trees/{}/events/{birth}", f.tree),
        Some(json!({"date_value": "1990"})),
    )
    .await;
    let rows = f.checked().await;
    assert_eq!(rows[&grand].latest_birth_year, Some(1960));

    // The birth gone, the ancestors lose the bound it gave.
    let (status, _) = send(
        &f.app,
        Method::DELETE,
        &format!("/api/v1/trees/{}/events/{birth}", f.tree),
        None,
    )
    .await;
    assert!(status.is_success());
    let rows = f.checked().await;
    assert_eq!(rows[&grand].latest_birth_year, None);
}

#[tokio::test]
async fn a_link_removed_or_a_person_deleted_refreshes_the_ancestors() {
    let f = Fixture::new().await;
    let [top, grand, parent, child] = line(&f).await;
    f.event(child, "baptism", "1900").await;
    let rows = f.checked().await;
    assert_eq!(rows[&top].latest_birth_year, Some(1855));

    // The parent's link to the grandparent removed: above it, no bound.
    let families = FamilyRepo::list_all(&f.db, f.tree_id()).await.unwrap();
    let ids: Vec<Uuid> = families.iter().map(|family| family.id).collect();
    let links = FamilyChildRepo::list_by_families(&f.db, &ids)
        .await
        .unwrap();
    let link = links.iter().find(|link| link.person_id == parent).unwrap();
    let (status, _) = send(
        &f.app,
        Method::DELETE,
        &format!(
            "/api/v1/trees/{}/families/{}/children/{}",
            f.tree, link.family_id, link.id
        ),
        None,
    )
    .await;
    assert!(status.is_success());
    let rows = f.checked().await;
    assert_eq!(rows[&grand].latest_birth_year, None);
    assert_eq!(rows[&parent].latest_birth_year, Some(1885));

    // The child deleted: its row goes, and the parent loses its bound.
    let (status, _) = send(
        &f.app,
        Method::DELETE,
        &format!("/api/v1/trees/{}/persons/{child}", f.tree),
        None,
    )
    .await;
    assert!(status.is_success());
    let rows = f.checked().await;
    assert!(!rows.contains_key(&child));
    assert_eq!(rows[&parent].latest_birth_year, None);
}

#[tokio::test]
async fn a_privacy_change_and_a_marriage_are_stored() {
    let f = Fixture::new().await;
    let (husband, wife) = (f.person().await, f.person().await);
    let child = f.person().await;
    let (family, _) = f.family(&[husband, wife], child).await;
    f.checked().await;
    ok(
        &f.app,
        Method::PUT,
        &format!("/api/v1/trees/{}/persons/{husband}", f.tree),
        Some(json!({"privacy": "private"})),
    )
    .await;
    f.post(
        "events",
        json!({"event_type": "marriage", "date_value": "1920", "family_id": family}),
    )
    .await;
    let rows = f.checked().await;
    assert_eq!(rows[&husband].privacy, oxidgene_core::Privacy::Private);
    assert_eq!(rows[&wife].latest_birth_year, Some(1906));
}

/// A connected fictitious tree of eleven generations, about 11,000
/// persons: 64 founders born in 1700, each generation's couples having
/// three children 25 years later, so every generation half again as large
/// as the one before; the persons of the first eight generations have a
/// death recorded. The persons as GEDCOM `INDI` ids, by generation.
fn generations_gedcom() -> (String, Vec<Vec<String>>) {
    let mut out =
        String::from("0 HEAD\n1 GEDC\n2 VERS 5.5.1\n2 FORM LINEAGE-LINKED\n1 CHAR UTF-8\n");
    let mut generations: Vec<Vec<String>> = vec![(0..64).map(|i| format!("I0_{i}")).collect()];
    let mut families = String::new();
    let mut links: HashMap<String, Vec<String>> = HashMap::new();
    for g in 0..10 {
        let parents = generations[g].clone();
        let mut children = Vec::new();
        for (f, couple) in parents.chunks(2).enumerate() {
            let family = format!("F{g}_{f}");
            families += &format!("0 @{family}@ FAM\n1 HUSB @{}@\n", couple[0]);
            links
                .entry(couple[0].clone())
                .or_default()
                .push(format!("1 FAMS @{family}@"));
            if let Some(wife) = couple.get(1) {
                families += &format!("1 WIFE @{wife}@\n");
                links
                    .entry(wife.clone())
                    .or_default()
                    .push(format!("1 FAMS @{family}@"));
            }
            for _ in 0..3 {
                let child = format!("I{}_{}", g + 1, children.len());
                families += &format!("1 CHIL @{child}@\n");
                links
                    .entry(child.clone())
                    .or_default()
                    .push(format!("1 FAMC @{family}@"));
                children.push(child);
            }
        }
        generations.push(children);
    }
    for (g, persons) in generations.iter().enumerate() {
        let year = 1700 + 25 * g as i32;
        for (i, person) in persons.iter().enumerate() {
            let sex = if i % 2 == 0 { 'M' } else { 'F' };
            out += &format!("0 @{person}@ INDI\n1 NAME Given{i} /Fernhill{g}/\n1 SEX {sex}\n");
            // Most persons dated, some not, as in a real tree.
            if i % 5 != 0 {
                out += &format!("1 BIRT\n2 DATE {year}\n");
            }
            if g < 8 {
                out += &format!("1 DEAT\n2 DATE {}\n", year + 60);
            }
            for link in links.get(person).into_iter().flatten() {
                out += link;
                out += "\n";
            }
        }
    }
    out += &families;
    out += "0 TRLR\n";
    (out, generations)
}

/// The cost of keeping `person_visibility` current on a generated tree of
/// about 11,000 persons: the full derivation, then single writes — a birth
/// added at the bottom, which reaches ten generations of ancestors, and one
/// at the top — each timed before the rows exist (no refresh) and after.
#[tokio::test]
#[ignore = "benchmark: run by `just bench` in release mode"]
async fn bench_visibility_on_a_generated_tree() {
    use std::time::Instant;

    let f = Fixture::new().await;
    let (gedcom, generations) = generations_gedcom();
    let persons: usize = generations.iter().map(Vec::len).sum();
    let started = Instant::now();
    crate::common::import_gedcom(&f.app, &f.db, &f.tree, &gedcom).await;
    let import = started.elapsed();
    let ids: Vec<Uuid> = PersonRepo::list_all(&f.db, f.tree_id())
        .await
        .unwrap()
        .iter()
        .map(|p| p.id)
        .collect();
    // The newest persons sort last; the founders first.
    let (leaf, founder) = (*ids.last().unwrap(), ids[0]);
    let writes = |round: &'static str| {
        let f = &f;
        async move {
            let mut timings = Vec::new();
            for (label, person, year) in [("leaf", leaf, "1990"), ("founder", founder, "1730")] {
                let started = Instant::now();
                f.event(person, "census", year).await;
                timings.push((round, label, started.elapsed()));
            }
            timings
        }
    };
    // Without rows a write refreshes nothing: the baseline.
    use oxidgene_db::sea_orm::{ConnectionTrait, Statement};
    f.db.execute_raw(Statement::from_string(
        f.db.get_database_backend(),
        "DELETE FROM person_visibility",
    ))
    .await
    .unwrap();
    let mut timings = writes("without rows").await;
    let started = Instant::now();
    let rows = f.profiles.visibility(f.tree_id(), &ids).await.unwrap();
    let rebuild = started.elapsed();
    assert_eq!(rows.len(), persons);
    let contemporary = rows
        .values()
        .filter(|row| row.is_contemporary(Default::default(), 2026))
        .count();
    timings.extend(writes("with refresh").await);
    println!(
        "visibility bench ({persons} persons, {contemporary} contemporary): import={import:?}, \
         rebuild={rebuild:?}, writes: {timings:?}"
    );
    f.checked().await;
}
