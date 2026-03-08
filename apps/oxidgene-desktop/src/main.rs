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
//! The cache is persisted to the platform cache directory:
//! - Linux:   `~/.cache/oxidgene/`
//! - macOS:   `~/Library/Caches/oxidgene/`
//! - Windows: `C:\Users\<user>\AppData\Local\oxidgene\`

use std::net::SocketAddr;

use axum::Router;
use axum::routing::get;
use clap::Parser;
use dioxus::desktop::{Config, WindowBuilder};
use oxidgene_api::{AppState, build_router};
use oxidgene_cache::store::disk;
use oxidgene_cache::store::memory::MemoryCacheStore;
use oxidgene_db::repo::{connect, run_migrations};
use oxidgene_ui::api::ApiClient;
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

/// Default pedigree LRU budget in bytes (64 MB).
const DEFAULT_PEDIGREE_BUDGET_BYTES: usize = 64 * 1024 * 1024;

#[derive(Parser)]
#[command(name = "oxidgene-desktop", about = "OxidGene desktop genealogy app")]
struct Cli {
    /// Enable debug logging (logs all person data received from the backend)
    #[arg(long)]
    debug: bool,
}

fn main() {
    let cli = Cli::parse();

    // ── Initialize tracing ───────────────────────────────────────────
    let filter = if cli.debug {
        "info,oxidgene_ui=debug,oxidgene_api=debug,oxidgene_db=debug,oxidgene_cache=debug"
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

    // ── Resolve cache directory ──────────────────────────────────────
    let cache_dir = std::env::var("OXIDGENE_CACHE_DIR")
        .map(std::path::PathBuf::from)
        .ok()
        .or_else(disk::default_cache_dir)
        .expect("could not determine platform cache directory");

    info!(cache_dir = %cache_dir.display(), "Using cache directory");

    // ── Read pedigree budget ─────────────────────────────────────────
    let pedigree_budget = std::env::var("OXIDGENE_PEDIGREE_CACHE_MB")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .map(|mb| mb * 1024 * 1024)
        .unwrap_or(DEFAULT_PEDIGREE_BUDGET_BYTES);

    // ── Load cache from disk (if available and not stale) ────────────
    let memory_store = if disk::is_cache_stale(&cache_dir, &db_path) {
        info!("Disk cache is stale or missing, starting with empty cache");
        MemoryCacheStore::with_budget(pedigree_budget)
    } else {
        match disk::load_from_disk(&cache_dir, pedigree_budget) {
            Some(store) => {
                info!("Cache loaded from disk");
                store
            }
            None => {
                warn!("Failed to load cache from disk, starting with empty cache");
                MemoryCacheStore::with_budget(pedigree_budget)
            }
        }
    };

    // ── Start embedded Axum server in a background tokio runtime ─────
    let (tx, rx) = std::sync::mpsc::channel::<u16>();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    let db_path_for_persist = db_path.clone();
    let cache_dir_for_persist = cache_dir.clone();

    let server_handle = std::thread::spawn(move || {
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

            // Build router with the pre-loaded memory store.
            let state = AppState::with_memory_store(db, memory_store);
            let api_router = build_router(state.clone());

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

            // ── Persist cache to disk on shutdown ────────────────────
            info!("Persisting cache to disk before exit…");
            // Access the MemoryCacheStore through the CacheService.
            // The CacheStore trait is object-safe, so we call snapshot_for_disk
            // via the store() accessor which returns &dyn CacheStore.
            // However, we need the concrete MemoryCacheStore for snapshot_for_disk.
            // Since AppState::with_memory_store wraps it as Arc<dyn CacheStore>,
            // we need to use the cache service's persist method instead.
            //
            // For now, we persist through the CacheService's store accessor by
            // downcasting. We'll add a persist helper to CacheService.
            persist_cache_via_service(&state, &cache_dir_for_persist, &db_path_for_persist);
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
            Config::new()
                .with_menu(None::<dioxus::desktop::muda::Menu>)
                .with_window(
                    WindowBuilder::new()
                        .with_title("OxidGene")
                        .with_inner_size(dioxus::desktop::LogicalSize::new(1280.0, 800.0)),
                ),
        )
        .launch(oxidgene_ui::App);

    // ── Signal the server thread to shut down and persist cache ───────
    info!("Dioxus window closed, signalling server shutdown…");
    let _ = shutdown_tx.send(());
    server_handle.join().expect("server thread panicked");
    info!("Server thread exited cleanly");
}

/// Persist the cache from the CacheService's inner store.
///
/// Attempts to downcast the `dyn CacheStore` back to `MemoryCacheStore`.
fn persist_cache_via_service(
    state: &AppState,
    cache_dir: &std::path::Path,
    db_path: &std::path::Path,
) {
    use std::any::Any;

    // The CacheService exposes store() -> &dyn CacheStore.
    // We need to downcast to MemoryCacheStore for snapshot_for_disk().
    let store = state.cache.store();
    let store_any: &dyn Any = store.as_any();
    if let Some(memory_store) = store_any.downcast_ref::<MemoryCacheStore>() {
        if let Err(e) = disk::persist_to_disk(memory_store, cache_dir, Some(db_path)) {
            error!(%e, "Failed to persist cache to disk");
        }
    } else {
        warn!("Cache store is not MemoryCacheStore, skipping disk persistence");
    }
}

/// Health check handler returning `200 OK` with a JSON body.
async fn healthz() -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({ "status": "ok" }))
}
