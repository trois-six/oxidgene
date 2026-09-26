//! Database migrations for OxidGene.

pub mod m20250101_000001_initial;
pub mod m20260918_000001_search_relatives;
pub mod m20260926_000001_drop_redundant_indexes;

use sea_orm_migration::prelude::*;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20250101_000001_initial::Migration),
            Box::new(m20260918_000001_search_relatives::Migration),
            Box::new(m20260926_000001_drop_redundant_indexes::Migration),
        ]
    }
}
