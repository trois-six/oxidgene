//! Database migrations for OxidGene.

pub mod m20250101_000001_initial;
pub mod m20260724_000001_search_display_names;
pub mod m20260724_000002_citation_media_link_fk_indexes;
pub mod m20260728_000001_person_denorm;

use sea_orm_migration::prelude::*;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20250101_000001_initial::Migration),
            Box::new(m20260724_000001_search_display_names::Migration),
            Box::new(m20260724_000002_citation_media_link_fk_indexes::Migration),
            Box::new(m20260728_000001_person_denorm::Migration),
        ]
    }
}
