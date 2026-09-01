//! OxidGene web background worker.

use std::sync::Arc;

use oxidgene_api::profile::ProfileService;
use oxidgene_api::service::background_job::BackgroundJobWorker;
use oxidgene_db::repo::{connect, run_migrations};
use oxidgene_observability::init;
use oxidgene_server::config::ServerConfig;
use tokio::signal;
use tracing::{error, info};

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
    info!("Starting OxidGene background worker");
    let worker = BackgroundJobWorker::new(db, profiles, media, worker_id);
    tokio::select! {
        () = worker.run() => {}
        () = shutdown_signal() => {}
    }
    info!("Background worker shut down gracefully");
    telemetry.shutdown();
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => info!("Received SIGINT, shutting down"),
        () = terminate => info!("Received SIGTERM, shutting down"),
    }
}
