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
//!
//! There is no separate cache directory: the denormalized person projections
//! live in the same SQLite file (`person_denorm`), written as part of each
//! mutation, so nothing has to be warmed at startup or flushed at exit.
//!
//! The WebView data directory (`Config::with_data_directory`, set to
//! `<data_dir>/webview/`) is honored very differently per platform — wry
//! only forwards it to the OS webview engine on some of them:
//!
//! - **Windows (WebView2):** fully honored. Cookies, cache, IndexedDB, and
//!   WebView2's HSTS-equivalent network security state all live under
//!   `<data_dir>/webview/`.
//! - **Linux/BSD (WebKitGTK):** mostly honored via `WebsiteDataManager`'s
//!   `base-data-directory`, but a few legacy properties — notably HSTS
//!   storage — ignore it and fall back to `$XDG_DATA_HOME/<prgname>/`,
//!   where `prgname` is set by GTK from the binary name
//!   (`oxidgene-desktop`). We override it to `oxidgene` at startup so those
//!   fallbacks land in the same namespace too.
//! - **macOS/iOS (WKWebView):** *not* honored at all — wry's `WebContext`
//!   is a no-op stub on this backend (see `wry::web_context`), so cookies,
//!   DOM storage, and HSTS are all managed by WebKit's own
//!   `WKWebsiteDataStore::defaultDataStore()`, entirely outside
//!   `<data_dir>/webview/`. In a properly bundled `.app` this is still
//!   namespaced per-app via `CFBundleIdentifier`; there is currently no
//!   macOS bundle/`Info.plist` in this repo, so that namespacing isn't
//!   wired up yet. Revisit when macOS packaging is added — wry's
//!   `with_data_store_identifier` (macOS >= 14) is the closest available
//!   knob, though it's an opaque store ID rather than a directory.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::routing::get;
use clap::Parser;
use dioxus::desktop::tao::event::Event;
use dioxus::desktop::tao::window::Icon;
use dioxus::desktop::{Config, WindowBuilder, icon_from_memory};
use oxidgene_api::{AppState, build_router};
use oxidgene_db::repo::{connect, run_migrations};
use oxidgene_ui::api::ApiClient;
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

const ICON_PNG: &[u8] = include_bytes!("../assets/icon.png");

#[derive(Parser)]
#[command(name = "oxidgene-desktop", about = "OxidGene desktop genealogy app")]
struct Cli {
    /// Enable debug logging (logs all person data received from the backend)
    #[arg(long)]
    debug: bool,
}

fn main() {
    // WebKitGTK's WebsiteDataManager derives some default paths (e.g. HSTS
    // storage) from GLib's prgname rather than our configured data
    // directory. GTK would otherwise set it to the binary name
    // (`oxidgene-desktop`); pin it to `oxidgene` before any GTK/WebKit
    // initialization so those fallbacks stay under the same directory.
    #[cfg(any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    ))]
    glib::set_prgname(Some("oxidgene"));

    let cli = Cli::parse();

    // ── Initialize tracing ───────────────────────────────────────────
    let filter = if cli.debug {
        "info,oxidgene_ui=debug,oxidgene_api=debug,oxidgene_db=debug"
    } else {
        "info"
    };
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(filter)),
        )
        .init();

    // ── Resolve data directory (SQLite) ──────────────────────────────
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
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    // Wrap shutdown_tx so it can be captured by the Dioxus event handler closure.
    let shutdown_tx = Arc::new(Mutex::new(Some(shutdown_tx)));

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

            // Serve with graceful shutdown.
            let shutdown = async {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {
                        info!("Ctrl+C received, shutting down…");
                    }
                    _ = shutdown_rx => {
                        info!("Window closed, shutting down server…");
                    }
                }
            };

            axum::serve(listener, app)
                .with_graceful_shutdown(shutdown)
                .await
                .unwrap_or_else(|e| {
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
    // Dioxus `launch()` returns `-> !` (never returns), so we use a custom
    // event handler to intercept `Event::LoopDestroyed` and shut the embedded
    // server down cleanly before the process exits. Nothing needs flushing:
    // person projections live in SQLite, written as part of each mutation.
    let window_icon: Option<Icon> = icon_from_memory(ICON_PNG).ok();
    let shutdown_tx_for_handler = Arc::clone(&shutdown_tx);
    let mut cfg = Config::new()
        .with_data_directory(data_dir.join("webview"))
        .with_menu(None::<dioxus::desktop::muda::Menu>)
        .with_window(
            WindowBuilder::new()
                .with_title("OxidGene")
                .with_inner_size(dioxus::desktop::LogicalSize::new(1280.0, 800.0)),
        );
    if let Some(icon) = window_icon {
        cfg = cfg.with_icon(icon);
    }
    dioxus::LaunchBuilder::new()
        .with_context(api_client)
        .with_cfg(cfg.with_custom_event_handler(move |event, _target| {
            if let Event::LoopDestroyed = event {
                info!("Window closing, shutting the embedded server down…");
                // Take the sender (only fires once).
                if let Some(sender) = shutdown_tx_for_handler.lock().unwrap().take() {
                    let _ = sender.send(());
                }
            }
        }))
        .launch(oxidgene_ui::App);
}

/// Health check handler returning `200 OK` with a JSON body.
async fn healthz() -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({ "status": "ok" }))
}
