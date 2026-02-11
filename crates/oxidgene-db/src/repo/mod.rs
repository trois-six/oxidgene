//! Database connection and migration utilities.

use sea_orm::{ConnectOptions, Database, DatabaseConnection, DbErr};
use sea_orm_migration::MigratorTrait;
use tracing::info;

use crate::Migrator;

/// Connect to a database using the provided URL.
///
/// # Supported URLs
/// - `sqlite::memory:` — in-memory SQLite (for tests)
/// - `sqlite://path/to/db.sqlite` — file-based SQLite
/// - `postgres://user:pass@host/db` — PostgreSQL
pub async fn connect(database_url: &str) -> Result<DatabaseConnection, DbErr> {
    let mut opts = ConnectOptions::new(database_url);
    opts.sqlx_logging(false);
    let db = Database::connect(opts).await?;
    info!("Connected to database");
    Ok(db)
}

/// Run all pending migrations on the given database connection.
pub async fn run_migrations(db: &DatabaseConnection) -> Result<(), DbErr> {
    Migrator::up(db, None).await?;
    info!("Migrations applied successfully");
    Ok(())
}

/// Roll back all migrations on the given database connection.
pub async fn rollback_migrations(db: &DatabaseConnection) -> Result<(), DbErr> {
    Migrator::down(db, None).await?;
    info!("Migrations rolled back successfully");
    Ok(())
}
