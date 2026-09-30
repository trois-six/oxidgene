//! OxidGene web background worker.

use std::sync::Arc;

use oxidgene_api::profile::ProfileService;
use oxidgene_api::service::background_job::BackgroundJobWorker;
use oxidgene_api::startup::{connect_and_migrate, or_exit};
use oxidgene_observability::init;
use oxidgene_server::config::ServerConfig;
use oxidgene_server::shutdown::shutdown_signal;
use tracing::info;

#[tokio::main]
async fn main() {
    let config = ServerConfig::load().unwrap_or_else(|_| {
        eprintln!("Failed to load configuration");
        std::process::exit(1);
    });
    let telemetry = init(
        "oxidgene-worker",
        env!("CARGO_PKG_VERSION"),
        &config.log_level,
    )
    .unwrap_or_else(|_| {
        eprintln!("Failed to initialize observability");
        std::process::exit(1);
    });

    let db = connect_and_migrate(&config.database_url).await;
    let media = or_exit(
        config.media_store(),
        "media_storage_configuration",
        "Failed to configure media storage",
    );
    let profiles = Arc::new(ProfileService::new(db.clone()));
    let worker_id = format!("worker-{}", uuid::Uuid::now_v7());
    info!("Starting OxidGene background worker");
    let worker = BackgroundJobWorker::new(db, profiles, media, worker_id);
    tokio::select! {
        () = worker.run() => {}
        () = shutdown_signal() => {}
    }
    info!("Background worker shut down gracefully");
    telemetry.shutdown();
}
