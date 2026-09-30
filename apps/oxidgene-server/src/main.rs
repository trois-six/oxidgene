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

use axum::http::{HeaderValue, Method};
use oxidgene_api::access::same_origin_writes;
use oxidgene_api::startup::{
    ReferenceWarmup, open_database, or_exit, spawn_background_worker, with_health_check,
};
use oxidgene_api::{AppState, build_router};
use oxidgene_observability::{init, make_http_span, on_http_response};
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
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
    let telemetry = init("oxidgene-server", env!("CARGO_PKG_VERSION"), &cfg.log_level)
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

    // ── Database, migrations and history baselines ───────────────────
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
    let api_router = build_router(state);
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
    let cors = CorsLayer::new()
        .allow_origin(cors_origin.clone())
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers(tower_http::cors::Any);

    let app = with_health_check(same_origin_writes(api_router, cors_origin.clone()))
        .layer(cors)
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(make_http_span)
                .on_response(on_http_response),
        );

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
