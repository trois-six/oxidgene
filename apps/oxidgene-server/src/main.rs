//! OxidGene web backend server.
//!
//! Starts an Axum HTTP server with:
//! - REST API under `/api/v1/trees`
//! - GraphQL at `/graphql` (POST) and GraphiQL playground (GET)
//! - Health check at `/healthz`
//! - CORS middleware
//! - Structured tracing
//! - Graceful shutdown on SIGINT/SIGTERM

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::http::{HeaderValue, Method};
use axum::routing::get;
use oxidgene_api::service::background_job::BackgroundJobWorker;
use oxidgene_api::{AppState, build_router};
use oxidgene_db::repo::{BackgroundJobRepo, connect, run_migrations};
use tokio::net::TcpListener;
use tokio::signal;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

use oxidgene_server::config::{MediaBackend, ServerConfig};

#[tokio::main]
async fn main() {
    // ── Load configuration ───────────────────────────────────────────
    let cfg = ServerConfig::load().unwrap_or_else(|_| {
        eprintln!("Failed to load configuration");
        std::process::exit(1);
    });

    // ── Initialize tracing ───────────────────────────────────────────
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&cfg.log_level)),
        )
        .init();

    info!(
        host = %cfg.host,
        port = %cfg.port,
        log_level = %cfg.log_level,
        media_backend = cfg.media_backend.as_str(),
        "Starting OxidGene server"
    );

    // ── Connect to database ──────────────────────────────────────────
    let db = connect(&cfg.database_url).await.unwrap_or_else(|_| {
        error!(
            error = "database_connection",
            "Failed to connect to database"
        );
        std::process::exit(1);
    });

    // ── Run migrations ───────────────────────────────────────────────
    run_migrations(&db).await.unwrap_or_else(|_| {
        error!(error = "database_migration", "Failed to run migrations");
        std::process::exit(1);
    });

    // ── Build application router ─────────────────────────────────────
    let media = cfg.media_store().unwrap_or_else(|_| {
        error!(
            error = "media_storage_configuration",
            "Failed to configure media storage"
        );
        std::process::exit(1);
    });
    let uses_sqlite = cfg.database_url.starts_with("sqlite:");
    let embedded_worker = uses_sqlite || cfg.media_backend == MediaBackend::Filesystem;
    let state = AppState::with_media_store(db, media);
    if embedded_worker {
        if uses_sqlite {
            BackgroundJobRepo::requeue_running(&state.db)
                .await
                .unwrap_or_else(|_| {
                    error!(
                        error = "background_job_recovery",
                        "Failed to recover background jobs"
                    );
                    std::process::exit(1);
                });
        }
        let worker = BackgroundJobWorker::new(
            state.db.clone(),
            Arc::clone(&state.profiles),
            Arc::clone(&state.media),
            "embedded-server",
        );
        tokio::spawn(worker.run());
    }
    let api_router = build_router(state);

    // CORS remains single-origin until authentication and authorization ship.
    if cfg.cors_origin == "*" {
        error!(error = "cors_origin", "Wildcard CORS origin is not allowed");
        std::process::exit(1);
    }
    let cors_origin = cfg.cors_origin.parse::<HeaderValue>().unwrap_or_else(|_| {
        error!(error = "cors_origin", "Invalid CORS origin");
        std::process::exit(1);
    });
    let cors = CorsLayer::new()
        .allow_origin(cors_origin)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers(tower_http::cors::Any);

    let app = Router::new()
        .route("/healthz", get(healthz))
        .merge(api_router)
        .layer(cors)
        .layer(TraceLayer::new_for_http().make_span_with(
            |request: &axum::http::Request<_>| {
                tracing::debug_span!("http_request", method = %request.method())
            },
        ));

    // ── Bind and serve ───────────────────────────────────────────────
    let addr = SocketAddr::new(cfg.host.parse().expect("invalid host address"), cfg.port);
    let listener = TcpListener::bind(addr).await.unwrap_or_else(|_| {
        error!(error = "listener_bind", %addr, "Failed to bind TCP listener");
        std::process::exit(1);
    });

    info!(%addr, "Listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap_or_else(|_| {
            error!(error = "server_runtime", "Server error");
            std::process::exit(1);
        });

    info!("Server shut down gracefully");
}

/// Health check handler returning `200 OK` with a JSON body.
async fn healthz() -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({ "status": "ok" }))
}

/// Wait for SIGINT (Ctrl+C) or SIGTERM to initiate graceful shutdown.
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
