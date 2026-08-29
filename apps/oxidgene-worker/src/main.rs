//! OxidGene web background worker.

use std::sync::Arc;

use oxidgene_api::profile::ProfileService;
use oxidgene_api::service::background_job::BackgroundJobWorker;
use oxidgene_db::repo::{connect, run_migrations};
use oxidgene_server::config::ServerConfig;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    let config = ServerConfig::load().unwrap_or_else(|_| {
        eprintln!("Failed to load configuration");
        std::process::exit(1);
    });
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&config.log_level)),
        )
        .init();

    let db = connect(&config.database_url).await.unwrap_or_else(|_| {
        error!(
            error = "database_connection",
            "Failed to connect to database"
        );
        std::process::exit(1);
    });
    run_migrations(&db).await.unwrap_or_else(|_| {
        error!(error = "database_migration", "Failed to run migrations");
        std::process::exit(1);
    });
    let media = config.media_store().unwrap_or_else(|_| {
        error!(
            error = "media_storage_configuration",
            "Failed to configure media storage"
        );
        std::process::exit(1);
    });
    let profiles = Arc::new(ProfileService::new(db.clone()));
    let worker_id = format!("worker-{}", uuid::Uuid::now_v7());
    info!(%worker_id, "Starting OxidGene background worker");
    BackgroundJobWorker::new(db, profiles, media, worker_id)
        .run()
        .await;
}
