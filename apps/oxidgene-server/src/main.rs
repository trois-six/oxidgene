//! OxidGene web backend server.
//!
//! Starts an Axum HTTP server with:
//! - REST API under `/api/v1/trees`
//! - GraphQL at `/graphql` (POST) and GraphiQL playground (GET)
//! - Health check at `/healthz`
//! - Authentication (`OXIDGENE_AUTH_MODE`): OpenID Connect with the
//!   Backend-for-Frontend's `/auth/*` routes, or `none`
//! - CORS middleware in `none` mode, and refusal of writes from any other
//!   browser origin
//! - Structured tracing
//! - Graceful shutdown on SIGINT/SIGTERM

use std::sync::Arc;

use axum::http::HeaderValue;
use oxidgene_api::auth::Auth;
use oxidgene_api::mail::MailOutbox;
use oxidgene_api::startup::{
    MailSetup, ReferenceWarmup, open_database, or_exit, spawn_background_worker,
};
use oxidgene_api::{AppState, build_router};
use oxidgene_observability::init;
use tokio::net::TcpListener;
use tracing::{error, info, warn};

use oxidgene_server::config::{MediaBackend, ServerConfig};
use oxidgene_server::http::Exposure;
use oxidgene_server::shutdown::shutdown_signal;

#[tokio::main]
async fn main() {
    // First, so that every buffer after it is allocated under the setting:
    // see `oxidgene_api::memory`.
    oxidgene_api::memory::tune();

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

    // ── Authentication and mail ──────────────────────────────────────
    let (auth, access_log_retention, mail) = start_auth().await;
    refuse_exposure_without_auth(&cfg, &auth).await;

    // ── Database and migrations ──────────────────────────────────────
    let reference_warmup = ReferenceWarmup::start();
    let db = open_database(&cfg.database_url).await;

    // ── Build application router ─────────────────────────────────────
    // The reason names the missing setting, never its value.
    let media = cfg.media_store().unwrap_or_else(|reason| {
        error!(
            error = "media_storage_configuration",
            %reason,
            "Failed to configure media storage"
        );
        std::process::exit(1)
    });
    let uses_sqlite = cfg.database_url.starts_with("sqlite:");
    let embedded_worker = uses_sqlite || cfg.media_backend == MediaBackend::Filesystem;
    let work_dir = cfg.work_dir();
    // The setting is named, never its value.
    let limits = cfg.intake_limits().unwrap_or_else(|variable| {
        error!(
            error = "intake_limits",
            variable, "Invalid size limit: expected a size such as 350M or 10G"
        );
        std::process::exit(1)
    });
    // Before any job or upload: what a crashed run left.
    let sweep = work_dir.clone();
    tokio::task::spawn_blocking(move || sweep.sweep(std::time::SystemTime::now()));
    let state = AppState::with_media_store(db, media)
        .with_work_dir(work_dir)
        .with_intake_limits(limits)
        .with_cache(cfg.cache())
        .with_auth(Arc::clone(&auth))
        .with_access_log_retention(access_log_retention);
    if embedded_worker {
        // SQLite has no separate worker: this process was the only one
        // running the jobs a previous run left marked as running.
        spawn_background_worker(&state, uses_sqlite, "embedded-server").await;
        // The embedded worker sends the mail too.
        if let Some(mail) = &mail {
            mail.spawn_sender(state.db.clone());
        }
    }
    // Under PostgreSQL the workers may run in other processes: their writes
    // reach this one's waiting job status reads as notifications, and the
    // notifications they and the other replicas write reach its long polls.
    tokio::spawn(oxidgene_api::service::job_watch::listen(state.db.clone()));
    tokio::spawn(oxidgene_api::service::updates::listen(state.db.clone()));
    let mut api_router = build_router(state);
    if cfg.graphiql && auth.mode_name() == "none" {
        api_router = api_router.layer(axum::Extension(oxidgene_api::graphql::GraphiQl));
    } else if cfg.graphiql {
        // Its page runs scripts of a public CDN on the origin the session
        // cookie belongs to: never beside signed-in users.
        warn!(
            error = "graphiql_refused",
            variable = "OXIDGENE_GRAPHIQL",
            "GraphiQL is not served with authentication on: the setting is ignored"
        );
    }
    reference_warmup.finish().await;

    let app = oxidgene_server::http::app(api_router, exposure(&cfg, &auth));

    // ── Bind and serve ───────────────────────────────────────────────
    // A host name (`localhost`) resolves as well as an IP literal; a host
    // that does neither is a startup error like any other, not a panic.
    let listener = bind(&cfg.host, cfg.port).await.unwrap_or_else(|_| {
        error!(
            error = "listener_bind",
            host = %cfg.host,
            port = cfg.port,
            "Failed to bind TCP listener"
        );
        std::process::exit(1);
    });
    let addr = listener.local_addr().unwrap_or_else(|_| {
        error!(error = "listener_bind", "Failed to read the bound address");
        std::process::exit(1);
    });

    info!(%addr, "Listening");

    or_exit(
        oxidgene_server::http::serve(listener, app, shutdown_signal()).await,
        "server_runtime",
        "Server error",
    );

    info!("Server shut down gracefully");
    telemetry.shutdown();
}

/// The authentication `OXIDGENE_AUTH_MODE` and its settings describe, how
/// long the access log keeps its events (`OXIDGENE_ACCESS_LOG_RETENTION`),
/// which the embedded worker enforces, and the SMTP relay of the `oidc`
/// mode, if `OXIDGENE_SMTP_HOST` names one. An invalid setting ends the
/// process with its category and variable, never its value.
async fn start_auth() -> (Arc<Auth>, std::time::Duration, Option<MailSetup>) {
    let settings = or_exit(
        // Its text may quote a value, and is not logged.
        ServerConfig::load_auth().map_err(|error| error.to_string()),
        "auth_config_invalid",
        "Failed to load the authentication settings",
    );
    let config = settings
        .resolve()
        .unwrap_or_else(|failure| invalid_auth(failure));
    let retention = settings
        .access_log_retention()
        .unwrap_or_else(|failure| invalid_auth(failure));
    let mail_settings = or_exit(
        ServerConfig::load_mail().map_err(|error| error.to_string()),
        "mail_config_invalid",
        "Failed to load the mail settings",
    );
    let mail = MailSetup::resolve(&settings, &mail_settings);
    let auth = or_exit(
        Auth::start(config).await,
        "session_store_unavailable",
        "Failed to connect to the session store",
    )
    .with_invitation_outbox(Arc::new(MailOutbox::new(mail.is_some())));
    let auth = Arc::new(auth);
    announce(&auth);
    if mail.is_some() {
        info!(mail = "smtp", "Mail goes through the configured SMTP relay");
    }
    (auth, retention, mail)
}

/// End the process over an invalid authentication setting, named by its
/// category and variable, never its value.
fn invalid_auth(failure: oxidgene_api::auth::AuthConfigError) -> ! {
    error!(
        error = failure.category,
        variable = failure.variable,
        "Invalid authentication configuration"
    );
    std::process::exit(1)
}

/// Say how requests are authenticated, at each start; the `none` mode with
/// its `auth_disabled` warning.
fn announce(auth: &Arc<Auth>) {
    if auth.mode_name() == "none" {
        warn_disabled();
    } else {
        info!(auth = "oidc", "Authentication through OpenID Connect");
        auth.warm_up();
    }
}

fn warn_disabled() {
    warn!(
        auth = "auth_disabled",
        "Authentication is disabled: every request acts as the owner of every tree"
    );
}

/// End the process rather than serve the `none` mode beyond loopback
/// unasked: there, whoever reaches the port reads and writes every tree.
/// `OXIDGENE_EXPOSE_WITHOUT_AUTH=true` acknowledges it — a container, whose
/// only address is its network's, behind something that authenticates.
async fn refuse_exposure_without_auth(cfg: &ServerConfig, auth: &Auth) {
    if auth.mode_name() != "none" || !cfg.listens_beyond_loopback().await {
        return;
    }
    if !cfg.expose_without_auth {
        exit_exposed_without_auth();
    }
    warn!(
        auth = "auth_disabled_exposed",
        "Authentication is disabled on an address other than loopback: \
         whoever reaches it owns every tree"
    );
}

/// End the process: the `none` mode would listen beyond loopback and
/// nothing acknowledges it.
fn exit_exposed_without_auth() -> ! {
    error!(
        error = "auth_disabled_exposed",
        variable = "OXIDGENE_EXPOSE_WITHOUT_AUTH",
        "Refusing to serve without authentication on an address other than loopback: \
         bind to loopback, choose the oidc mode, or acknowledge the exposure"
    );
    std::process::exit(1)
}

/// Where browsers reach the API from: the public URL alone in `oidc`, which
/// serves the frontend and the API under one origin with no CORS; the one
/// trusted CORS origin in `none`.
fn exposure(cfg: &ServerConfig, auth: &Auth) -> Exposure {
    // The setting is named, never its value.
    let proxies = cfg.trusted_proxies().unwrap_or_else(|_| {
        error!(
            error = "trusted_proxies",
            variable = "OXIDGENE_TRUSTED_PROXIES",
            "Invalid trusted proxies: expected addresses or CIDR ranges"
        );
        std::process::exit(1)
    });
    if let Some(origin) = auth.public_origin() {
        return Exposure {
            proxies,
            cors: None,
            origin: or_exit(
                origin.parse::<HeaderValue>(),
                "auth_config_invalid",
                "Invalid public URL",
            ),
            hosts: cfg.allowed_hosts(Some(origin)),
            hsts: auth.uses_https(),
        };
    }
    if cfg.cors_origin == "*" {
        error!(error = "cors_origin", "Wildcard CORS origin is not allowed");
        std::process::exit(1);
    }
    let cors_origin = or_exit(
        cfg.cors_origin.parse::<HeaderValue>(),
        "cors_origin",
        "Invalid CORS origin",
    );
    Exposure {
        proxies,
        ..Exposure::cors(cors_origin, cfg.allowed_hosts(None))
    }
}

/// A listener on `host`, an IP literal or a host name, and `port`.
async fn bind(host: &str, port: u16) -> std::io::Result<TcpListener> {
    TcpListener::bind((host, port)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `OXIDGENE_HOST=localhost` binds rather than panicking on a host that
    /// is no IP literal.
    #[tokio::test]
    async fn a_host_name_binds_like_an_address() {
        for host in ["localhost", "127.0.0.1"] {
            let listener = bind(host, 0).await.expect("binds");
            assert!(listener.local_addr().unwrap().ip().is_loopback(), "{host}");
        }
    }
}
