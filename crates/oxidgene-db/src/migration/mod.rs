//! Database migrations for OxidGene.

pub mod m20250101_000001_initial;
pub mod m20260813_000001_media_storage;

use sea_orm_migration::prelude::*;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20250101_000001_initial::Migration),
            Box::new(m20260813_000001_media_storage::Migration),
        ]
    }
}
