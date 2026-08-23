//! Database migrations for OxidGene.

pub mod m20250101_000001_initial;
pub mod m20260813_000001_media_storage;
pub mod m20260813_000002_media_event_fields;
pub mod m20260813_000003_media_pages;
pub mod m20260818_000001_media_type;
pub mod m20260819_000001_person_portrait;
pub mod m20260819_000002_privacy_family_media;
pub mod m20260819_000003_tree_default_privacy;
pub mod m20260821_000001_person_denorm_schema_version;
pub mod m20260823_000001_tree_self_person;
pub mod m20260823_000002_media_tags;
pub mod m20260823_000003_media_tag_relation;

use sea_orm_migration::prelude::*;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20250101_000001_initial::Migration),
            Box::new(m20260813_000001_media_storage::Migration),
            Box::new(m20260813_000002_media_event_fields::Migration),
            Box::new(m20260813_000003_media_pages::Migration),
            Box::new(m20260818_000001_media_type::Migration),
            Box::new(m20260819_000001_person_portrait::Migration),
            Box::new(m20260819_000002_privacy_family_media::Migration),
            Box::new(m20260819_000003_tree_default_privacy::Migration),
            Box::new(m20260821_000001_person_denorm_schema_version::Migration),
            Box::new(m20260823_000001_tree_self_person::Migration),
            Box::new(m20260823_000002_media_tags::Migration),
            Box::new(m20260823_000003_media_tag_relation::Migration),
        ]
    }
}
