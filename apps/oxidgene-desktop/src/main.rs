//! OxidGene desktop application.
//!
//! Embeds an Axum server on `127.0.0.1` (random port) backed by SQLite,
//! then opens a Dioxus desktop WebView with the shared `oxidgene-ui`
//! frontend. The server answers only requests carrying a token generated at
//! launch and known to this process alone.
//!
//! Every file it writes goes to one of four directories, by kind, resolved
//! once by `oxidgene_api::app_dirs::AppDirs` (the XDG Base Directory
//! convention on Linux, the platform's equivalents elsewhere):
//!
//! - **data** (`~/.local/share/oxidgene`): `oxidgene.db` and `media/`, the
//!   user's genealogy. There is no separate projection cache: the
//!   denormalized person projections live in the same SQLite file
//!   (`person_denorm`), written as part of each mutation.
//! - **config** (`~/.config/oxidgene`): `themes/`, the custom themes the
//!   user writes by hand.
//! - **state** (`~/.local/state/oxidgene`): `webview/`, the window's web
//!   profile — cookies, local storage holding the UI preferences such as the
//!   language, media keys. Not a cache: clearing it resets the preferences.
//! - **cache** (`~/.cache/oxidgene`): the working directory of jobs and
//!   staged inputs (`jobs/`, `staging/`, see `oxidgene_api::workdir`), and
//!   WebKitGTK's HTTP cache.
//!
//! The WebView data directory (`Config::with_data_directory`, set to the
//! state directory's `webview/`) is honored very differently per platform —
//! wry only forwards it to the OS webview engine on some of them:
//!
//! - **Windows (WebView2):** fully honored. Cookies, cache, IndexedDB, and
//!   WebView2's HSTS-equivalent network security state all live under
//!   `webview/`.
//! - **Linux/BSD (WebKitGTK):** wry hands it to the `WebsiteDataManager` as
//!   its `base-data-directory` and sets no `base-cache-directory`. Everything
//!   derived from the former — local storage, IndexedDB, media keys, the
//!   general `storage/`, and the cookie file wry places there — lands under
//!   `webview/`. The disk cache and `CacheStorage` follow the unset cache
//!   base to WebKit's default, `$XDG_CACHE_HOME/<prgname>`; the HSTS store
//!   (`hsts-storage.sqlite`) also ignores the data base and falls back to
//!   `$XDG_DATA_HOME/<prgname>/`, the only file of the web profile that does
//!   not follow `webview/`. `prgname` is set by GTK from the binary name
//!   (`oxidgene-desktop`); we override it to `oxidgene` at startup so those
//!   fallbacks land in the application's own directories. Neither dioxus
//!   nor wry lets the application set the HSTS directory itself, and it is a
//!   disposable cache of servers' HTTPS policies.
//! - **macOS/iOS (WKWebView):** *not* honored at all — wry's `WebContext`
//!   is a no-op stub on this backend (see `wry::web_context`), so cookies,
//!   DOM storage, and HSTS are all managed by WebKit's own
//!   `WKWebsiteDataStore::defaultDataStore()`, entirely outside
//!   `webview/`. In a properly bundled `.app` this is still
//!   namespaced per-app via `CFBundleIdentifier`; there is currently no
//!   macOS bundle/`Info.plist` in this repo, so that namespacing isn't
//!   wired up yet. Revisit when macOS packaging is added — wry's
//!   `with_data_store_identifier` (macOS >= 14) is the closest available
//!   knob, though it's an opaque store ID rather than a directory.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod archives;
mod geneanet;
mod languages;
mod mcp;
mod media_assets;
mod printing;
mod themes;

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use dioxus::desktop::tao::event::Event;
use dioxus::desktop::{Config, WindowBuilder, icon_from_memory};
use oxidgene_api::access::{AllowedHosts, LocalToken, allowed_hosts, require_local_token};
use oxidgene_api::app_dirs::AppDirs;
use oxidgene_api::startup::{
    ReferenceWarmup, open_database, spawn_background_worker, with_health_check,
};
use oxidgene_api::workdir::WorkDir;
use oxidgene_api::{AppState, build_router, request_context};
#[cfg(feature = "telemetry")]
use oxidgene_observability::{
    LogFormat, TelemetryGuard, init, init_to_stderr, make_http_span, on_http_response,
};
use oxidgene_ui::api::ApiClient;
use oxidgene_ui::assistant::AssistantLauncher;
use oxidgene_ui::theme::CustomThemeLoader;
use tokio::net::TcpListener;
#[cfg(feature = "telemetry")]
use tower_http::trace::TraceLayer;
use tracing::{error, info};

const ICON_PNG: &[u8] = include_bytes!("../../../assets/desktop/icon.png");

#[cfg(any(
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd"
))]
type WindowGeometry = (i32, i32, u32, u32, i32);

#[cfg(any(
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd"
))]
fn window_geometry_changed(
    last_geometry: &std::cell::Cell<Option<WindowGeometry>>,
    geometry: WindowGeometry,
) -> bool {
    last_geometry.replace(Some(geometry)) != Some(geometry)
}

#[cfg(any(
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd"
))]
fn suppress_duplicate_configure_events(
    window: std::sync::Arc<dioxus::desktop::tao::window::Window>,
) {
    use std::cell::Cell;

    use dioxus::desktop::tao::platform::unix::WindowExtUnix;
    use gtk::prelude::*;

    let last_geometry = Cell::new(None);
    window.gtk_window().connect_event(move |window, event| {
        if let Some(event) = event.downcast_ref::<gtk::gdk::EventConfigure>() {
            let (x, y) = event.position();
            let (width, height) = event.size();
            let geometry = (x, y, width, height, window.scale_factor());

            if !window_geometry_changed(&last_geometry, geometry) {
                return glib::Propagation::Stop;
            }
        }

        glib::Propagation::Proceed
    });
}

/// The whole command line: a subcommand and two flags, read by hand.
///
/// A derive-based parser links its entire help-rendering and error-reporting
/// machinery into a binary that opens a window; for a single boolean it was
/// the largest dependency in the app that no feature needed. `--help` is
/// answered here rather than dropped, because a binary on a `$PATH` that
/// ignores it is rude.
struct Cli {
    /// `mcp` as the first argument: serve MCP over stdio instead of opening
    /// the window.
    mcp: bool,
    /// Enable developer-oriented logs for OxidGene crates.
    #[cfg(feature = "telemetry")]
    debug: bool,
    /// Tracing filter, overriding `OXIDGENE_LOG_LEVEL`.
    #[cfg(feature = "telemetry")]
    log_level: Option<String>,
    /// Console log format, overriding `OXIDGENE_LOG_FORMAT`.
    #[cfg(feature = "telemetry")]
    log_format: Option<LogFormat>,
}

/// The value after a flag that takes one, or the end of the process.
#[cfg(feature = "telemetry")]
fn flag_value(args: &mut impl Iterator<Item = std::ffi::OsString>, flag: &str) -> String {
    args.next()
        .and_then(|value| value.into_string().ok())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| {
            eprintln!("oxidgene-desktop: {flag} requires a value");
            std::process::exit(2);
        })
}

impl Cli {
    fn parse() -> Self {
        #[cfg(feature = "telemetry")]
        let mut debug = false;
        #[cfg(feature = "telemetry")]
        let mut log_level = None;
        #[cfg(feature = "telemetry")]
        let mut log_format = None;
        let mut args = std::env::args_os().skip(1).peekable();
        let mcp = args.next_if(|arg| arg == "mcp").is_some();
        // A build without telemetry accepts no flags at all, so every arm below
        // ends the process and the loop provably runs at most once — which is
        // what both lints report. The loop is still what the telemetry build
        // needs, where `--log-level` consumes the argument after it.
        #[cfg_attr(
            not(feature = "telemetry"),
            expect(
                clippy::never_loop,
                clippy::while_let_on_iterator,
                reason = "no flag is accepted without telemetry, so every arm exits"
            )
        )]
        while let Some(arg) = args.next() {
            match arg.to_str() {
                #[cfg(feature = "telemetry")]
                Some("--debug") => debug = true,
                #[cfg(feature = "telemetry")]
                Some("--log-level") => log_level = Some(flag_value(&mut args, "--log-level")),
                #[cfg(feature = "telemetry")]
                Some("--log-format") => {
                    log_format = Some(
                        flag_value(&mut args, "--log-format")
                            .parse()
                            .unwrap_or_else(|_| {
                                eprintln!("oxidgene-desktop: --log-format must be text or json");
                                std::process::exit(2);
                            }),
                    );
                }
                Some("-h" | "--help") => {
                    #[cfg(feature = "telemetry")]
                    println!(
                        "oxidgene-desktop — OxidGene desktop genealogy app\n\
                         \n\
                         Usage: oxidgene-desktop [mcp] [--debug] [--log-level FILTER] [--log-format FORMAT]\n\
                         \n\
                         Commands:\n    \
                             mcp          Serve the trees to an MCP client over stdio\n\
                         \n\
                         Options:\n    \
                             --debug      Enable debug logs for OxidGene crates\n    \
                             --log-level FILTER  Override OXIDGENE_LOG_LEVEL\n    \
                             --log-format FORMAT  text or json; overrides OXIDGENE_LOG_FORMAT\n    \
                             -h, --help   Show this message\n    \
                             -V, --version  Show the version\n"
                    );
                    #[cfg(not(feature = "telemetry"))]
                    println!(
                        "oxidgene-desktop — OxidGene desktop genealogy app\n\
                         \n\
                         Usage: oxidgene-desktop [mcp]\n\
                         \n\
                         Commands:\n    \
                             mcp          Serve the trees to an MCP client over stdio\n\
                         \n\
                         Options:\n    \
                             -h, --help   Show this message\n    \
                             -V, --version  Show the version\n"
                    );
                    std::process::exit(0);
                }
                Some("-V" | "--version") => {
                    println!("oxidgene-desktop {}", env!("CARGO_PKG_VERSION"));
                    std::process::exit(0);
                }
                _ => {
                    eprintln!(
                        "oxidgene-desktop: unrecognised argument '{}'",
                        arg.to_string_lossy()
                    );
                    eprintln!("Try 'oxidgene-desktop --help'.");
                    std::process::exit(2);
                }
            }
        }
        Self {
            mcp,
            #[cfg(feature = "telemetry")]
            debug,
            #[cfg(feature = "telemetry")]
            log_level,
            #[cfg(feature = "telemetry")]
            log_format,
        }
    }
}

fn main() {
    // First, so that every buffer after it is allocated under the setting:
    // see `oxidgene_api::memory`.
    oxidgene_api::memory::tune();

    #[cfg(all(windows, not(debug_assertions)))]
    unsafe {
        windows_sys::Win32::System::Console::AttachConsole(u32::MAX);
    }

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

    // ── Initialize observability ─────────────────────────────────────
    #[cfg(feature = "telemetry")]
    let telemetry = Arc::new(Mutex::new(Some(init_telemetry(&cli))));

    // ── Resolve the application's directories ────────────────────────
    let app_dirs = app_dirs_or_exit();

    if cli.mcp {
        let status = mcp::run(&app_dirs.database());
        #[cfg(feature = "telemetry")]
        shutdown_telemetry(&telemetry);
        std::process::exit(status);
    }

    create_data_dir_or_exit(&app_dirs);

    let db_path = app_dirs.database();
    let media_root = app_dirs.media();
    let work_dir = WorkDir::new(app_dirs.work());
    let server_work_dir = work_dir.clone();
    let database_url = format!("sqlite://{}?mode=rwc", db_path.display());
    info!("Using local SQLite database");

    // ── Start embedded Axum server in a background tokio runtime ─────
    // Only this process may call the embedded server: see `oxidgene_api::access`.
    let token = LocalToken::generate();
    let server_token = token.clone();
    let (tx, rx) = std::sync::mpsc::channel::<u16>();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    // Wrap shutdown_tx so it can be captured by the Dioxus event handler closure.
    let shutdown_tx = Arc::new(Mutex::new(Some(shutdown_tx)));

    let server_thread = std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
        rt.block_on(async move {
            let reference_warmup = ReferenceWarmup::start();
            let db = open_database(&database_url).await;

            // Same platform data directory the web server defaults to, so a
            // desktop tree exported and re-imported on the server finds its
            // files in the expected place. The worker's maintenance passes,
            // the first at start, sweep what a crashed run left in the
            // working directory.
            let state = AppState::new(db, media_root)
                .with_work_dir(server_work_dir)
                .with_local_file_access();
            // This process is the only worker of its SQLite database.
            spawn_background_worker(&state, true, "desktop").await;
            // Outside the token and host checks, so a refused request still
            // carries its route, and a panic anywhere below answers the
            // standard envelope. The client dials `127.0.0.1`, so loopback
            // names are the only hosts this server answers under.
            let api_router = request_context::wrap(allowed_hosts(
                require_local_token(build_router(state), server_token),
                AllowedHosts::loopback(),
            ));
            reference_warmup.finish().await;

            #[cfg(feature = "telemetry")]
            let api_router = api_router.layer(
                TraceLayer::new_for_http()
                    .make_span_with(make_http_span)
                    .on_response(on_http_response),
            );
            // `/healthz` stays outside the trace layer: a probe is not an
            // operation worth a span or a metric point.
            let app = with_health_check(api_router);

            // Bind to random port on loopback
            let addr = SocketAddr::from(([127, 0, 0, 1], 0));
            let listener = TcpListener::bind(addr).await.unwrap_or_else(|_| {
                error!(error = "listener_bind", "Failed to bind TCP listener");
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
                .unwrap_or_else(|_| {
                    error!(error = "server_runtime", "Server error");
                });
        });
    });

    let api_client = api_client_when_ready(&rx, &token);

    // ── Launch Dioxus desktop window ─────────────────────────────────
    // Dioxus `launch()` returns `-> !` (never returns), so we use a custom
    // event handler to intercept `Event::LoopDestroyed` and shut the embedded
    // server down cleanly before the process exits. Nothing needs flushing:
    // person projections live in SQLite, written as part of each mutation.
    let shutdown_tx_for_handler = Arc::clone(&shutdown_tx);
    let server_thread_for_handler = Arc::new(Mutex::new(Some(server_thread)));
    #[cfg(feature = "telemetry")]
    let telemetry_for_handler = Arc::clone(&telemetry);

    // The Geneanet import wizard's step 3 needs a second browser window on
    // geneanet.org, which only the event loop can create — so the bridge the
    // UI talks to and the handler that services it are installed together.
    let (geneanet_bridge, mut geneanet_handler) = geneanet::install(work_dir);
    let (archive_viewer, mut archive_viewer_handler) = archives::install();
    let theme_loader =
        CustomThemeLoader::new(themes::DesktopThemeSource::install(&app_dirs.themes()));
    let language_loader = oxidgene_ui::i18n::CustomLanguageLoader::new(
        languages::DesktopLanguageSource::install(&app_dirs.languages()),
    );
    let cfg = window_config(&app_dirs.webview());
    let mut launch = dioxus::LaunchBuilder::new()
        .with_context(api_client)
        .with_context(geneanet_bridge)
        .with_context(archive_viewer)
        .with_context(printing::bridge())
        .with_context(theme_loader)
        .with_context(language_loader);
    // App Settings shows MCP clients the command that runs this very binary
    // with `mcp`. Without a resolvable path the page falls back to its note
    // rather than printing a command that would not run.
    if let Some(executable) = std::env::current_exe()
        .ok()
        .and_then(|path| path.into_os_string().into_string().ok())
    {
        launch = launch.with_context(AssistantLauncher { executable });
    }
    launch
        .with_cfg(cfg.with_custom_event_handler(move |event, target| {
            geneanet_handler(event, target);
            archive_viewer_handler(event, target);

            if let Event::LoopDestroyed = event {
                info!("Window closing, shutting the embedded server down…");
                // Take the sender (only fires once).
                if let Some(sender) = shutdown_tx_for_handler.lock().unwrap().take() {
                    let _ = sender.send(());
                }
                if let Some(server_thread) = server_thread_for_handler.lock().unwrap().take() {
                    let _ = server_thread.join();
                }
                #[cfg(feature = "telemetry")]
                shutdown_telemetry(&telemetry_for_handler);
            }
        }))
        .launch(media_assets::DesktopApp);
}

/// The application's directories, or the end of the process when the
/// platform knows no home for them.
fn app_dirs_or_exit() -> AppDirs {
    AppDirs::resolve().unwrap_or_else(|| {
        error!(
            error = "data_directory",
            "Could not determine the user's directories"
        );
        std::process::exit(1);
    })
}

/// Create the data directory the database opens in, or end the process.
fn create_data_dir_or_exit(app_dirs: &AppDirs) {
    std::fs::create_dir_all(&app_dirs.data).unwrap_or_else(|_| {
        error!(error = "data_directory", "Failed to create data directory");
        std::process::exit(1);
    });
}

/// Wait for the embedded server to report its port, then build the client
/// the UI shares to call it.
fn api_client_when_ready(port: &std::sync::mpsc::Receiver<u16>, token: &LocalToken) -> ApiClient {
    // Wait for the server to be ready
    let port = port
        .recv()
        .expect("failed to receive port from server thread");
    let api_url = format!("http://127.0.0.1:{port}");
    info!(%api_url, "API server ready");

    // Create the API client that will be shared with the UI
    // Pictures are served from the window's own origin rather than encoded
    // into every payload — see `media_assets`.
    ApiClient::new(&api_url)
        .with_auth_token(token.as_str())
        .with_image_host(media_assets::host())
}

/// Start logging and tracing with the filter the command line or the
/// environment asks for, exiting if that fails.
#[cfg(feature = "telemetry")]
fn init_telemetry(cli: &Cli) -> TelemetryGuard {
    let filter = cli
        .log_level
        .clone()
        .or_else(|| std::env::var("OXIDGENE_LOG_LEVEL").ok())
        .unwrap_or_else(|| {
            if cli.debug {
                "info,oxidgene_ui=debug,oxidgene_api=debug,oxidgene_db=debug".to_string()
            } else {
                "info".to_string()
            }
        });
    let log_format = cli.log_format.unwrap_or_else(|| {
        std::env::var("OXIDGENE_LOG_FORMAT")
            .ok()
            .filter(|value| !value.is_empty())
            .map_or(Ok(LogFormat::Text), |value| value.parse())
            .unwrap_or_else(|_| {
                eprintln!("Invalid OXIDGENE_LOG_FORMAT: expected text or json");
                std::process::exit(1);
            })
    });
    // An MCP session owns standard output for its protocol, so its logs go to
    // standard error.
    let init = if cli.mcp { init_to_stderr } else { init };
    init(
        "oxidgene-desktop",
        env!("CARGO_PKG_VERSION"),
        &filter,
        log_format,
    )
    .unwrap_or_else(|_| {
        eprintln!("Failed to initialize observability");
        std::process::exit(1);
    })
}

/// Flush and stop telemetry, once: later calls find nothing left to stop.
#[cfg(feature = "telemetry")]
fn shutdown_telemetry(telemetry: &Mutex<Option<TelemetryGuard>>) {
    if let Some(telemetry) = telemetry.lock().unwrap().take() {
        telemetry.shutdown();
    }
}

/// The main window's configuration: its title, size and icon, and the
/// webview's data directory, `webview_dir`.
fn window_config(webview_dir: &std::path::Path) -> Config {
    let mut cfg = Config::new()
        // The window's page loads no plugin and resolves no URL against a
        // `<base>` some markup could slip in. Scripts and styles stay as the
        // Dioxus runtime needs them: it evaluates scripts and writes styles
        // at run time.
        .with_custom_head(
            r#"<meta http-equiv="Content-Security-Policy" content="object-src 'none'; base-uri 'none'">"#
                .to_string(),
        )
        .with_data_directory(webview_dir)
        .with_menu(None::<dioxus::desktop::muda::Menu>)
        .with_window(
            WindowBuilder::new()
                .with_title("OxidGene")
                .with_inner_size(dioxus::desktop::LogicalSize::new(1280.0, 800.0)),
        );
    #[cfg(any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    ))]
    {
        // GTK emits the generic `event` signal before the specialized
        // `configure-event` consumed by Tao, so duplicates never reach
        // Dioxus's WebView::set_bounds path.
        cfg = cfg.with_on_window(|window, _| suppress_duplicate_configure_events(window));
    }
    if let Ok(icon) = icon_from_memory(ICON_PNG) {
        cfg = cfg.with_icon(icon);
    }
    cfg
}

#[cfg(all(
    test,
    any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    )
))]
mod tests {
    use std::cell::Cell;

    use super::window_geometry_changed;

    #[test]
    fn configure_events_pass_only_when_geometry_changes() {
        let last_geometry = Cell::new(None);

        assert!(window_geometry_changed(
            &last_geometry,
            (0, 0, 1280, 800, 1)
        ));
        assert!(!window_geometry_changed(
            &last_geometry,
            (0, 0, 1280, 800, 1)
        ));
        assert!(window_geometry_changed(
            &last_geometry,
            (0, 0, 1200, 800, 1)
        ));
        assert!(window_geometry_changed(
            &last_geometry,
            (20, 30, 1200, 800, 1)
        ));
        assert!(window_geometry_changed(
            &last_geometry,
            (20, 30, 1200, 800, 2)
        ));
    }
}
