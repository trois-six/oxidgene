//! Database migrations for OxidGene.

pub mod m20250101_000001_initial;
pub mod m20250307_000001_add_search_indexes;

use sea_orm_migration::prelude::*;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20250101_000001_initial::Migration),
            Box::new(m20250307_000001_add_search_indexes::Migration),
        ]
    }
}
