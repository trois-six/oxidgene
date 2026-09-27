//! Database migrations for OxidGene.

pub mod m20250101_000001_initial;
pub mod m20260918_000001_search_relatives;
pub mod m20260926_000001_drop_redundant_indexes;
pub mod m20260927_000001_file_couple_media;
pub mod m20260927_000002_person_distinct;

use sea_orm_migration::prelude::*;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20250101_000001_initial::Migration),
            Box::new(m20260918_000001_search_relatives::Migration),
            Box::new(m20260926_000001_drop_redundant_indexes::Migration),
            Box::new(m20260927_000001_file_couple_media::Migration),
            Box::new(m20260927_000002_person_distinct::Migration),
        ]
    }
}
