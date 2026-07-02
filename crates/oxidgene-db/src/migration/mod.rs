//! Database migrations for OxidGene.

pub mod m20250101_000001_initial;
pub mod m20250307_000001_add_search_indexes;
pub mod m20250307_000002_add_sosa_root_person;
pub mod m20260622_000001_person_edit_modal_fields;
pub mod m20260702_000001_person_search_fts;

use sea_orm_migration::prelude::*;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20250101_000001_initial::Migration),
            Box::new(m20250307_000001_add_search_indexes::Migration),
            Box::new(m20250307_000002_add_sosa_root_person::Migration),
            Box::new(m20260622_000001_person_edit_modal_fields::Migration),
            Box::new(m20260702_000001_person_search_fts::Migration),
        ]
    }
}
