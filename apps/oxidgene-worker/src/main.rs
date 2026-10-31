//! OxidGene web background worker.

use std::sync::Arc;

use oxidgene_api::profile::ProfileService;
use oxidgene_api::service::background_job::BackgroundJobWorker;
use oxidgene_api::startup::connect_and_migrate;
use oxidgene_observability::init;
use oxidgene_server::config::ServerConfig;
use oxidgene_server::shutdown::shutdown_signal;
use tracing::{error, info};

#[tokio::main]
async fn main() {
    // First, so that every buffer after it is allocated under the setting:
    // see `oxidgene_api::memory`.
    oxidgene_api::memory::tune();

    let config = ServerConfig::load().unwrap_or_else(|_| {
        eprintln!("Failed to load configuration");
        std::process::exit(1);
    });
    let log_format = config.log_format().unwrap_or_else(|_| {
        eprintln!("Invalid OXIDGENE_LOG_FORMAT: expected text or json");
        std::process::exit(1);
    });
    let telemetry = init(
        "oxidgene-worker",
        env!("CARGO_PKG_VERSION"),
        &config.log_level,
        log_format,
    )
    .unwrap_or_else(|_| {
        eprintln!("Failed to initialize observability");
        std::process::exit(1);
    });

    let db = connect_and_migrate(&config.database_url).await;
    // The reason names the missing setting, never its value.
    let media = config.media_store().unwrap_or_else(|reason| {
        error!(
            error = "media_storage_configuration",
            %reason,
            "Failed to configure media storage"
        );
        std::process::exit(1)
    });
    let profiles = Arc::new(ProfileService::new(db.clone()));
    let worker_id = format!("worker-{}", uuid::Uuid::now_v7());
    info!("Starting OxidGene background worker");
    // Its maintenance passes, the first at start, sweep what a crashed run
    // left in the working directory.
    let worker =
        BackgroundJobWorker::new(db, profiles, media, worker_id).with_work_dir(config.work_dir());
    tokio::select! {
        () = worker.run() => {}
        () = shutdown_signal() => {}
    }
    info!("Background worker shut down gracefully");
    telemetry.shutdown();
}
