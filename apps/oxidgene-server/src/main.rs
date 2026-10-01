//! OxidGene web backend server.
//!
//! Starts an Axum HTTP server with:
//! - REST API under `/api/v1/trees`
//! - GraphQL at `/graphql` (POST) and GraphiQL playground (GET)
//! - Health check at `/healthz`
//! - CORS middleware, and refusal of writes from any other browser origin
//! - Structured tracing
//! - Graceful shutdown on SIGINT/SIGTERM

use std::net::SocketAddr;

use axum::http::HeaderValue;
use oxidgene_api::startup::{ReferenceWarmup, open_database, or_exit, spawn_background_worker};
use oxidgene_api::{AppState, build_router};
use oxidgene_observability::init;
use tokio::net::TcpListener;
use tracing::{error, info};

use oxidgene_server::config::{MediaBackend, ServerConfig};
use oxidgene_server::shutdown::shutdown_signal;

#[tokio::main]
async fn main() {
    // ── Load configuration ───────────────────────────────────────────
    let cfg = ServerConfig::load().unwrap_or_else(|_| {
        eprintln!("Failed to load configuration");
        std::process::exit(1);
    });

    // ── Initialize observability ─────────────────────────────────────
    let log_format = cfg.log_format().unwrap_or_else(|_| {
        eprintln!("Invalid OXIDGENE_LOG_FORMAT: expected text or json");
        std::process::exit(1);
    });
    let telemetry = init(
        "oxidgene-server",
        env!("CARGO_PKG_VERSION"),
        &cfg.log_level,
        log_format,
    )
    .unwrap_or_else(|_| {
        eprintln!("Failed to initialize observability");
        std::process::exit(1);
    });

    info!(
        host = %cfg.host,
        port = %cfg.port,
        log_level = %cfg.log_level,
        media_backend = cfg.media_backend.as_str(),
        "Starting OxidGene server"
    );

    // ── Database and migrations ──────────────────────────────────────
    let reference_warmup = ReferenceWarmup::start();
    let db = open_database(&cfg.database_url).await;

    // ── Build application router ─────────────────────────────────────
    let media = or_exit(
        cfg.media_store(),
        "media_storage_configuration",
        "Failed to configure media storage",
    );
    let uses_sqlite = cfg.database_url.starts_with("sqlite:");
    let embedded_worker = uses_sqlite || cfg.media_backend == MediaBackend::Filesystem;
    let state = AppState::with_media_store(db, media);
    if embedded_worker {
        // SQLite has no separate worker: this process was the only one
        // running the jobs a previous run left marked as running.
        spawn_background_worker(&state, uses_sqlite, "embedded-server").await;
    }
    let mut api_router = build_router(state);
    if cfg.graphiql {
        api_router = api_router.layer(axum::Extension(oxidgene_api::graphql::GraphiQl));
    }
    reference_warmup.finish().await;

    // CORS remains single-origin until authentication and authorization ship.
    if cfg.cors_origin == "*" {
        error!(error = "cors_origin", "Wildcard CORS origin is not allowed");
        std::process::exit(1);
    }
    let cors_origin = or_exit(
        cfg.cors_origin.parse::<HeaderValue>(),
        "cors_origin",
        "Invalid CORS origin",
    );
    let app = oxidgene_server::http::app(api_router, cors_origin, cfg.allowed_hosts());

    // ── Bind and serve ───────────────────────────────────────────────
    let addr = SocketAddr::new(cfg.host.parse().expect("invalid host address"), cfg.port);
    let listener = TcpListener::bind(addr).await.unwrap_or_else(|_| {
        error!(error = "listener_bind", %addr, "Failed to bind TCP listener");
        std::process::exit(1);
    });

    info!(%addr, "Listening");

    or_exit(
        axum::serve(listener, app)
            .with_graceful_shutdown(shutdown_signal())
            .await,
        "server_runtime",
        "Server error",
    );

    info!("Server shut down gracefully");
    telemetry.shutdown();
}
