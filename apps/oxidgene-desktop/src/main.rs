//! OxidGene desktop application.
//!
//! Embeds an Axum server on `127.0.0.1` (random port) backed by SQLite,
//! then opens a Dioxus desktop WebView with a welcome screen.
//!
//! The SQLite database is stored in the platform data directory:
//! - Linux:   `~/.local/share/oxidgene/oxidgene.db`
//! - macOS:   `~/Library/Application Support/oxidgene/oxidgene.db`
//! - Windows: `C:\Users\<user>\AppData\Roaming\oxidgene\oxidgene.db`

use std::net::SocketAddr;

use axum::Router;
use axum::routing::get;
use dioxus::desktop::{Config, WindowBuilder};
use dioxus::prelude::*;
use oxidgene_api::{AppState, build_router};
use oxidgene_db::repo::{connect, run_migrations};
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

/// The local API server address, injected into the Dioxus context.
#[derive(Clone, Debug)]
struct ApiServer {
    /// Full base URL, e.g. `http://127.0.0.1:12345`.
    pub url: String,
    /// Port number (used by health-check and future API calls).
    #[allow(dead_code)]
    pub port: u16,
}

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

    let api_server = ApiServer { url: api_url, port };

    // ── Launch Dioxus desktop window ─────────────────────────────────
    dioxus::LaunchBuilder::new()
        .with_context(api_server)
        .with_cfg(
            Config::new().with_window(
                WindowBuilder::new()
                    .with_title("OxidGene — Genealogy")
                    .with_inner_size(dioxus::desktop::LogicalSize::new(1280.0, 800.0)),
            ),
        )
        .launch(App);
}

/// Root application component.
#[component]
fn App() -> Element {
    let api = use_context::<ApiServer>();

    rsx! {
        style { {STYLES} }
        div { class: "container",
            div { class: "header",
                h1 { "OxidGene" }
                p { class: "subtitle", "Genealogy Application" }
            }
            div { class: "info",
                p { "Welcome to OxidGene — your offline-first genealogy desktop application." }
                p {
                    "API server running at: "
                    a {
                        href: "{api.url}",
                        target: "_blank",
                        "{api.url}"
                    }
                }
                p {
                    "GraphiQL playground: "
                    a {
                        href: "{api.url}/graphql",
                        target: "_blank",
                        "{api.url}/graphql"
                    }
                }
            }
            div { class: "status",
                HealthCheck { api_url: api.url.clone() }
            }
        }
    }
}

/// Component that checks the health of the embedded API server.
#[component]
fn HealthCheck(api_url: String) -> Element {
    let health_url = format!("{api_url}/healthz");

    let health = use_resource(move || {
        let url = health_url.clone();
        async move {
            // Simple fetch via reqwest would need an extra dep.
            // For the skeleton we just report the URL; actual health
            // polling will be added with the full UI in EPIC C.
            format!("API available at {url}")
        }
    });

    match &*health.read() {
        Some(msg) => rsx! {
            p { class: "health-ok", "{msg}" }
        },
        None => rsx! {
            p { class: "health-loading", "Checking API health..." }
        },
    }
}

/// Health check handler returning `200 OK` with a JSON body.
async fn healthz() -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({ "status": "ok" }))
}

/// Minimal CSS for the welcome screen.
const STYLES: &str = r#"
    body {
        font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif;
        margin: 0;
        padding: 0;
        background: #f5f5f5;
        color: #333;
    }
    .container {
        max-width: 640px;
        margin: 60px auto;
        padding: 40px;
        background: white;
        border-radius: 12px;
        box-shadow: 0 2px 12px rgba(0,0,0,0.08);
    }
    .header {
        text-align: center;
        margin-bottom: 32px;
    }
    .header h1 {
        font-size: 2.5rem;
        margin: 0 0 8px 0;
        color: #2c3e50;
    }
    .subtitle {
        font-size: 1.1rem;
        color: #7f8c8d;
        margin: 0;
    }
    .info {
        line-height: 1.8;
    }
    .info a {
        color: #3498db;
        text-decoration: none;
    }
    .info a:hover {
        text-decoration: underline;
    }
    .status {
        margin-top: 24px;
        padding: 16px;
        background: #f0fdf4;
        border-radius: 8px;
        border: 1px solid #bbf7d0;
    }
    .health-ok {
        color: #166534;
        margin: 0;
    }
    .health-loading {
        color: #92400e;
        margin: 0;
    }
"#;
