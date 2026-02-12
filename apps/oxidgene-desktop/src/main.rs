//! OxidGene desktop application.
//!
//! Embeds an Axum server on `127.0.0.1` (random port) backed by SQLite,
//! then opens a Dioxus desktop WebView with the shared `oxidgene-ui`
//! frontend.
//!
//! The SQLite database is stored in the platform data directory:
//! - Linux:   `~/.local/share/oxidgene/oxidgene.db`
//! - macOS:   `~/Library/Application Support/oxidgene/oxidgene.db`
//! - Windows: `C:\Users\<user>\AppData\Roaming\oxidgene\oxidgene.db`

use std::net::SocketAddr;

use axum::Router;
use axum::routing::get;
use dioxus::desktop::{Config, WindowBuilder};
use oxidgene_api::{AppState, build_router};
use oxidgene_db::repo::{connect, run_migrations};
use oxidgene_ui::api::ApiClient;
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

fn main() {
    // ── Initialize tracing ───────────────────────────────────────────
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    // ── Resolve data directory ───────────────────────────────────────
    let data_dir = dirs::data_dir()
        .expect("could not determine platform data directory")
        .join("oxidgene");

    std::fs::create_dir_all(&data_dir).unwrap_or_else(|e| {
        error!(%e, "Failed to create data directory");
        std::process::exit(1);
    });

    let db_path = data_dir.join("oxidgene.db");
    let database_url = format!("sqlite://{}?mode=rwc", db_path.display());
    info!(%database_url, "Using SQLite database");

    // ── Start embedded Axum server in a background tokio runtime ─────
    let (tx, rx) = std::sync::mpsc::channel::<u16>();

    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
        rt.block_on(async move {
            // Connect to SQLite
            let db = connect(&database_url).await.unwrap_or_else(|e| {
                error!(%e, "Failed to connect to database");
                std::process::exit(1);
            });

            // Run migrations
            run_migrations(&db).await.unwrap_or_else(|e| {
                error!(%e, "Failed to run migrations");
                std::process::exit(1);
            });

            // Build router
            let state = AppState::new(db);
            let api_router = build_router(state);

            let app = Router::new()
                .route("/healthz", get(healthz))
                .merge(api_router)
                .layer(CorsLayer::permissive());

            // Bind to random port on loopback
            let addr = SocketAddr::from(([127, 0, 0, 1], 0));
            let listener = TcpListener::bind(addr).await.unwrap_or_else(|e| {
                error!(%e, "Failed to bind TCP listener");
                std::process::exit(1);
            });

            let local_addr = listener.local_addr().expect("failed to get local address");
            info!(%local_addr, "Embedded API server listening");

            // Send the port back to the main thread
            tx.send(local_addr.port())
                .expect("failed to send port to main thread");

            // Serve until the process exits
            axum::serve(listener, app).await.unwrap_or_else(|e| {
                error!(%e, "Server error");
            });
        });
    });

    // Wait for the server to be ready
    let port = rx
        .recv()
        .expect("failed to receive port from server thread");
    let api_url = format!("http://127.0.0.1:{port}");
    info!(%api_url, "API server ready");

    // Create the API client that will be shared with the UI
    let api_client = ApiClient::new(&api_url);

    // ── Launch Dioxus desktop window ─────────────────────────────────
    dioxus::LaunchBuilder::new()
        .with_context(api_client)
        .with_cfg(
            Config::new().with_window(
                WindowBuilder::new()
                    .with_title("OxidGene — Genealogy")
                    .with_inner_size(dioxus::desktop::LogicalSize::new(1280.0, 800.0)),
            ),
        )
        .launch(oxidgene_ui::App);
}

/// Health check handler returning `200 OK` with a JSON body.
async fn healthz() -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({ "status": "ok" }))
}
