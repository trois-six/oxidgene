//! Migration policy: until the first release, every schema change goes into
//! the initial migration.
//!
//! Drift it prevents: a dated migration file, or a data migration, added to
//! an unreleased schema. Nobody runs an install that needs upgrading yet;
//! databases are recreated and trees re-imported (docs/data-model.md), and a
//! second migration would only fork the schema's definition in two.
//!
//! Fixing a failure: fold the change into
//! `m20250101_000001_initial.rs`. Once a release ships, set `RELEASED` to
//! `true`: the guard then lets new migrations in and checks only that the
//! initial one is still there.

use oxidgene_guards::{files, relative};

/// Whether a release has shipped. While it has not, the initial migration is
/// the only one.
const RELEASED: bool = false;

const ALLOWED: &[&str] = &["mod.rs", "m20250101_000001_initial.rs"];

#[test]
fn the_initial_migration_is_the_only_one_before_a_release() {
    let found: Vec<String> = files("crates/oxidgene-db/src/migration", "rs")
        .iter()
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert!(
        found
            .iter()
            .any(|name| name == "m20250101_000001_initial.rs"),
        "the initial migration is missing"
    );
    if RELEASED {
        return;
    }
    let extra: Vec<_> = found
        .iter()
        .filter(|name| !ALLOWED.contains(&name.as_str()))
        .collect();
    assert!(
        extra.is_empty(),
        "fold these into the initial migration until the first release: {extra:?}"
    );
    let registered = std::fs::read_to_string(
        oxidgene_guards::root().join("crates/oxidgene-db/src/migration/mod.rs"),
    )
    .unwrap();
    assert_eq!(
        registered.matches("Box::new(").count(),
        1,
        "{} registers more than the initial migration",
        relative(&oxidgene_guards::root().join("crates/oxidgene-db/src/migration/mod.rs"))
    );
}
