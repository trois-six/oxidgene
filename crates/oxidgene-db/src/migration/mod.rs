//! Database migrations for OxidGene.
//!
//! While the product is unreleased, the schema is one consolidated initial
//! migration: a schema change edits it, and existing databases are recreated
//! and their genealogy reimported. The migrator and its `seaql_migrations`
//! table stay so that, once released, changes can ship as incremental
//! migrations appended here.

pub mod m20250101_000001_initial;

use sea_orm_migration::prelude::*;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![Box::new(m20250101_000001_initial::Migration)]
    }
}
