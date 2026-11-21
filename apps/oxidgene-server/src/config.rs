//! Server configuration loaded from environment variables and optional config file.
//!
//! Environment variables (all prefixed with `OXIDGENE_`):
//!
//! | Variable                         | Default                                    | Description                 |
//! |----------------------------------|--------------------------------------------|-----------------------------|
//! | `OXIDGENE_HOST`                  | `127.0.0.1`                                | Bind address                |
//! | `OXIDGENE_PORT`                  | `8080`                                     | Bind port                   |
//! | `OXIDGENE_DATABASE_URL`          | `postgres://oxidgene:oxidgene@localhost/oxidgene` | Database connection URL |
//! | `OXIDGENE_LOG_LEVEL`             | `info`                                     | Tracing filter              |
//! | `OXIDGENE_LOG_FORMAT`            | `text`                                     | Console format, or `json`   |
//! | `OXIDGENE_CORS_ORIGIN`           | `http://127.0.0.1:8081`                    | Allowed CORS origin         |
//! | `OXIDGENE_ALLOWED_HOSTS`         | unset                                      | More `Host` names to answer |
//! | `OXIDGENE_TRUSTED_PROXIES`       | unset                                      | Reverse proxies believed on the client's address |
//! | `OXIDGENE_GRAPHIQL`              | `false`                                    | Serve GraphiQL at `GET /graphql` |
//! | `OXIDGENE_MEDIA_BACKEND`         | `filesystem`                               | `filesystem` or `s3`        |
//! | `OXIDGENE_MEDIA_ROOT`            | platform data dir (see below)              | Filesystem media root       |
//! | `OXIDGENE_WORK_DIR`              | user cache dir (see below)                 | Job scratch, staged uploads |
//! | `OXIDGENE_S3_BUCKET`             | `oxidgene-media`                            | S3 bucket                   |
//! | `OXIDGENE_S3_REGION`             | `us-east-1`                                | S3 signing region           |
//! | `OXIDGENE_S3_ENDPOINT`           | unset                                      | S3-compatible endpoint      |
//! | `OXIDGENE_S3_ACCESS_KEY_ID`      | unset                                      | S3 access key               |
//! | `OXIDGENE_S3_SECRET_ACCESS_KEY`  | unset                                      | S3 secret key               |
//! | `OXIDGENE_IMPORT_GEDCOM_MAX`     | `350M`                                     | Largest genealogy text      |
//! | `OXIDGENE_MEDIA_QUOTA`           | `10G`                                      | Media a tree may hold; `0`: none |
//! | `OXIDGENE_IMPORT_ARCHIVE_ENTRIES`| `100000`                                   | Most files of an archive    |
//! | `OXIDGENE_CACHE_MIB`             | `256`                                      | Tree cache budget, MiB      |
//! | `OXIDGENE_CACHE_IDLE_SECS`       | `300`                                      | Tree cache idle delay, s    |
//!
//! The tree cache (see `oxidgene_api::tree_cache`) keeps the trees in use
//! within `OXIDGENE_CACHE_MIB` mebibytes of estimated memory, the least
//! recently used dropped first, and drops a tree no request has used for
//! `OXIDGENE_CACHE_IDLE_SECS` seconds. A budget of `0` keeps nothing.
//!
//! The server answers only requests whose `Host` is a loopback name, the
//! host of `OXIDGENE_CORS_ORIGIN` (of `OXIDGENE_PUBLIC_URL` in the `oidc`
//! mode), or one of the comma-separated `OXIDGENE_ALLOWED_HOSTS` (see
//! `oxidgene_api::access::allowed_hosts`).
//!
//! The rate limits per address count a request under the address of its
//! connection, unless that is one of the comma-separated addresses or CIDR
//! ranges of `OXIDGENE_TRUSTED_PROXIES`: a reverse proxy, whose
//! `X-Forwarded-For` then names the client (see
//! `oxidgene_api::auth::client`). Behind a proxy that is not named, every
//! visitor counts as one.
//!
//! What the server and the worker take in is bounded
//! (`oxidgene_api::limits::IntakeLimits`): a genealogy text by
//! `OXIDGENE_IMPORT_GEDCOM_MAX`, the media a tree holds — and so the
//! archive it can import — by `OXIDGENE_MEDIA_QUOTA`, an archive's files by
//! `OXIDGENE_IMPORT_ARCHIVE_ENTRIES`. Sizes are bytes or a number with a
//! binary unit (`350M`, `10G`).
//!
//! The authentication settings — `OXIDGENE_AUTH_MODE`, `OXIDGENE_PUBLIC_URL`,
//! `OXIDGENE_OIDC_*`, `OXIDGENE_AUTH_*`, `OXIDGENE_SESSION_*`,
//! `OXIDGENE_REDIS_URL` — are read from the same sources by
//! [`ServerConfig::load_auth`] (docs/authentication.md §3.2), and the SMTP
//! relay's — `OXIDGENE_SMTP_*`, `OXIDGENE_MAIL_FROM` — by
//! [`ServerConfig::load_mail`] (docs/messaging.md §3.2).
//!
//! `OXIDGENE_MEDIA_ROOT` defaults to the platform's user-data directory —
//! `~/.local/share/oxidgene/media` on Linux. A containerised deployment
//! normally overrides it with the mount point of a persistent volume.
//!
//! `OXIDGENE_WORK_DIR` holds the disposable working files of jobs and
//! uploads (see `oxidgene_api::workdir`). It defaults to the user's cache
//! directory — `~/.cache/oxidgene` on Linux — and to the system's temporary
//! directory only without one. It must be on disk: an import stages files
//! as large as the archives it reads, which a RAM-backed `/tmp` cannot hold.
//!
//! An optional config file can be placed at `oxidgene.toml` in the working
//! directory. Environment variables always override file values.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use config::builder::DefaultState;
use config::{Config, ConfigBuilder, Environment, File};
use oxidgene_api::access::AllowedHosts;
use oxidgene_api::auth::AuthSettings;
use oxidgene_api::auth::client::{InvalidTrustedProxies, TrustedProxies};
use oxidgene_api::limits::{IntakeLimits, parse_size};
use oxidgene_api::mail::MailSettings;
use oxidgene_api::media::{FsStore, MediaStore, S3Store, S3StoreConfig};
use oxidgene_api::tree_cache::CacheSettings;
use oxidgene_api::workdir::WorkDir;
use oxidgene_observability::{InvalidLogFormat, LogFormat};
use serde::Deserialize;

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MediaBackend {
    #[default]
    Filesystem,
    S3,
}

impl MediaBackend {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Filesystem => "filesystem",
            Self::S3 => "s3",
        }
    }
}

/// Application configuration.
///
/// Its `Debug` output redacts the credentials it holds: the database URL's
/// password and the S3 secret key.
#[derive(Clone, Deserialize)]
pub struct ServerConfig {
    /// Bind address (default: loopback only).
    #[serde(default = "default_host")]
    pub host: String,

    /// Bind port (default: `8080`).
    #[serde(default = "default_port")]
    pub port: u16,

    /// Database connection URL.
    #[serde(default = "default_database_url")]
    pub database_url: String,

    /// Tracing log level filter (default: `info`).
    #[serde(default = "default_log_level")]
    pub log_level: String,

    /// Console log format, `text` or `json` (default: `text`). Kept as the
    /// raw string so an invalid value is reported by [`Self::log_format`]
    /// with a stable category rather than echoed by the config loader.
    #[serde(default = "default_log_format")]
    pub log_format: String,

    /// Allowed CORS origin (default: `http://127.0.0.1:8081`).
    #[serde(default = "default_cors_origin")]
    pub cors_origin: String,

    /// Further host names the API answers under, comma-separated, beyond
    /// the loopback names and the host of [`Self::cors_origin`].
    #[serde(default)]
    pub allowed_hosts: String,

    /// The reverse proxies in front of the server, as comma-separated
    /// addresses and CIDR ranges: the only peers whose `X-Forwarded-For`
    /// names the client the rate limits count.
    #[serde(default)]
    pub trusted_proxies: String,

    /// Whether `GET /graphql` serves GraphiQL (default: off). The page loads
    /// its scripts from a public CDN, so it is a development convenience.
    #[serde(default)]
    pub graphiql: bool,

    /// Directory uploaded media files are stored under.
    #[serde(default = "default_media_root")]
    pub media_root: PathBuf,

    /// Where jobs and uploads keep their disposable working files; the
    /// user's cache directory when unset.
    #[serde(default)]
    pub work_dir: Option<PathBuf>,

    /// Media storage implementation selected at startup.
    #[serde(default)]
    pub media_backend: MediaBackend,

    /// Bucket used by the S3 backend.
    #[serde(default = "default_s3_bucket")]
    pub s3_bucket: String,

    /// Signing region used by the S3 backend.
    #[serde(default = "default_s3_region")]
    pub s3_region: String,

    /// Optional custom S3-compatible endpoint.
    #[serde(default)]
    pub s3_endpoint: Option<String>,

    /// Access key used by the S3 backend.
    #[serde(default)]
    pub s3_access_key_id: Option<String>,

    /// Secret key used by the S3 backend.
    #[serde(default)]
    pub s3_secret_access_key: Option<String>,

    /// The largest genealogy text taken in — a `.ged`, a `.gw`, the
    /// `gedcom.ged` of a GEDZIP once inflated — as a size (`350M`).
    #[serde(default = "default_import_gedcom_max")]
    pub import_gedcom_max: String,

    /// The media a tree may hold in the store, as a size (`10G`); `0` for
    /// no quota.
    #[serde(default = "default_media_quota")]
    pub media_quota: String,

    /// The most files an archive may hold.
    #[serde(default = "default_import_archive_entries")]
    pub import_archive_entries: usize,

    /// The tree cache's budget, in mebibytes of estimated memory.
    #[serde(default = "default_cache_mib")]
    pub cache_mib: usize,

    /// How many seconds the tree cache keeps a tree no request uses.
    #[serde(default = "default_cache_idle_secs")]
    pub cache_idle_secs: u64,
}

impl std::fmt::Debug for ServerConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerConfig")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("database_url", &redact_url_password(&self.database_url))
            .field("log_level", &self.log_level)
            .field("log_format", &self.log_format)
            .field("cors_origin", &self.cors_origin)
            .field("allowed_hosts", &self.allowed_hosts)
            .field("trusted_proxies", &self.trusted_proxies)
            .field("graphiql", &self.graphiql)
            .field("media_root", &self.media_root)
            .field("work_dir", &self.work_dir)
            .field("media_backend", &self.media_backend)
            .field("s3_bucket", &self.s3_bucket)
            .field("s3_region", &self.s3_region)
            .field("s3_endpoint", &self.s3_endpoint)
            .field("s3_access_key_id", &self.s3_access_key_id)
            .field(
                "s3_secret_access_key",
                &self.s3_secret_access_key.as_ref().map(|_| REDACTED),
            )
            .field("import_gedcom_max", &self.import_gedcom_max)
            .field("media_quota", &self.media_quota)
            .field("import_archive_entries", &self.import_archive_entries)
            .field("cache_mib", &self.cache_mib)
            .field("cache_idle_secs", &self.cache_idle_secs)
            .finish()
    }
}

/// What a redacted credential prints as.
const REDACTED: &str = "<redacted>";

/// `url` with the password of its user information, if any, replaced by
/// [`REDACTED`]: `postgres://user:secret@host/db` prints as
/// `postgres://user:<redacted>@host/db`.
fn redact_url_password(url: &str) -> String {
    let Some(scheme_end) = url.find("://").map(|index| index + 3) else {
        return url.to_string();
    };
    let authority_end = url[scheme_end..]
        .find(['/', '?', '#'])
        .map_or(url.len(), |index| scheme_end + index);
    let Some(at) = url[scheme_end..authority_end].rfind('@') else {
        return url.to_string();
    };
    let user_info = &url[scheme_end..scheme_end + at];
    match user_info.find(':') {
        Some(colon) => format!(
            "{}{}:{REDACTED}{}",
            &url[..scheme_end],
            &user_info[..colon],
            &url[scheme_end + at..]
        ),
        None => url.to_string(),
    }
}

fn default_host() -> String {
    "127.0.0.1".to_string()
}

fn default_import_gedcom_max() -> String {
    "350M".to_string()
}

fn default_media_quota() -> String {
    "10G".to_string()
}

fn default_import_archive_entries() -> usize {
    IntakeLimits::default().archive_entries
}

fn default_cache_mib() -> usize {
    oxidgene_api::tree_cache::DEFAULT_BUDGET / (1024 * 1024)
}

fn default_cache_idle_secs() -> u64 {
    oxidgene_api::tree_cache::DEFAULT_IDLE.as_secs()
}

fn default_port() -> u16 {
    8080
}

fn default_database_url() -> String {
    "postgres://oxidgene:oxidgene@localhost/oxidgene".to_string()
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_log_format() -> String {
    "text".to_string()
}

fn default_cors_origin() -> String {
    "http://127.0.0.1:8081".to_string()
}

fn default_media_root() -> PathBuf {
    oxidgene_api::media::default_root()
}

fn default_s3_bucket() -> String {
    "oxidgene-media".to_string()
}

fn default_s3_region() -> String {
    "us-east-1".to_string()
}

impl ServerConfig {
    /// Load configuration from optional `oxidgene.toml` file and environment
    /// variables prefixed with `OXIDGENE_`.
    pub fn load() -> Result<Self, config::ConfigError> {
        sources().build()?.try_deserialize()
    }

    /// Load the authentication settings from the same sources.
    pub fn load_auth() -> Result<AuthSettings, config::ConfigError> {
        sources().build()?.try_deserialize()
    }

    /// Load the SMTP relay's settings (`OXIDGENE_SMTP_*`,
    /// `OXIDGENE_MAIL_FROM`, docs/messaging.md §3.2) from the same sources.
    pub fn load_mail() -> Result<MailSettings, config::ConfigError> {
        sources().build()?.try_deserialize()
    }

    /// The host names the API answers under: loopback, the frontend's
    /// origin's host — the public URL in `oidc`, the CORS origin otherwise —
    /// and [`Self::allowed_hosts`].
    pub fn allowed_hosts(&self, public_origin: Option<&str>) -> AllowedHosts {
        AllowedHosts::new(
            std::iter::once(public_origin.unwrap_or(&self.cors_origin))
                .chain(self.allowed_hosts.split(',')),
        )
    }

    /// What the server and the worker take in: the bounds
    /// `OXIDGENE_IMPORT_GEDCOM_MAX`, `OXIDGENE_MEDIA_QUOTA` and
    /// `OXIDGENE_IMPORT_ARCHIVE_ENTRIES` set.
    ///
    /// # Errors
    ///
    /// The variable that is no size, or names no file or no byte at all.
    pub fn intake_limits(&self) -> Result<IntakeLimits, &'static str> {
        let genealogy_bytes = parse_size(&self.import_gedcom_max)
            .ok()
            .filter(|bytes| *bytes > 0)
            .ok_or("OXIDGENE_IMPORT_GEDCOM_MAX")?;
        let quota = parse_size(&self.media_quota).map_err(|_| "OXIDGENE_MEDIA_QUOTA")?;
        if self.import_archive_entries == 0 {
            return Err("OXIDGENE_IMPORT_ARCHIVE_ENTRIES");
        }
        Ok(IntakeLimits {
            genealogy_bytes,
            archive_entries: self.import_archive_entries,
            tree_media_bytes: (quota > 0).then_some(quota),
        })
    }

    /// The reverse proxies `OXIDGENE_TRUSTED_PROXIES` names.
    ///
    /// # Errors
    ///
    /// When an entry is neither an address nor a CIDR range.
    pub fn trusted_proxies(&self) -> Result<TrustedProxies, InvalidTrustedProxies> {
        TrustedProxies::parse(&self.trusted_proxies)
    }

    /// The console log format `OXIDGENE_LOG_FORMAT` selects.
    pub fn log_format(&self) -> Result<LogFormat, InvalidLogFormat> {
        self.log_format.parse()
    }

    /// The tree cache `OXIDGENE_CACHE_MIB` and `OXIDGENE_CACHE_IDLE_SECS`
    /// describe: trees within the budget, each dropped once idle.
    pub fn cache(&self) -> CacheSettings {
        CacheSettings::shared(
            self.cache_mib.saturating_mul(1024 * 1024),
            Duration::from_secs(self.cache_idle_secs),
        )
    }

    /// The working directory `OXIDGENE_WORK_DIR` names, or the default.
    pub fn work_dir(&self) -> WorkDir {
        self.work_dir
            .clone()
            .map_or_else(WorkDir::user_default, WorkDir::new)
    }

    /// The media store `media_backend` selects.
    ///
    /// # Errors
    ///
    /// [`MediaConfigError`] when the S3 backend lacks a key or its settings
    /// are refused.
    pub fn media_store(&self) -> Result<Arc<dyn MediaStore>, MediaConfigError> {
        match self.media_backend {
            MediaBackend::Filesystem => Ok(Arc::new(FsStore::new(&self.media_root))),
            MediaBackend::S3 => {
                let access_key_id = self
                    .s3_access_key_id
                    .clone()
                    .ok_or(MediaConfigError::MissingS3Key("OXIDGENE_S3_ACCESS_KEY_ID"))?;
                let secret_access_key =
                    self.s3_secret_access_key
                        .clone()
                        .ok_or(MediaConfigError::MissingS3Key(
                            "OXIDGENE_S3_SECRET_ACCESS_KEY",
                        ))?;
                Ok(Arc::new(
                    S3Store::new(S3StoreConfig {
                        bucket: self.s3_bucket.clone(),
                        region: self.s3_region.clone(),
                        endpoint: self.s3_endpoint.clone(),
                        access_key_id,
                        secret_access_key,
                    })
                    .map_err(|_| MediaConfigError::InvalidS3)?,
                ))
            }
        }
    }
}

/// The optional `oxidgene.toml` of the working directory, then the
/// `OXIDGENE_*` environment, which overrides it.
fn sources() -> ConfigBuilder<DefaultState> {
    Config::builder()
        .add_source(File::with_name("oxidgene").required(false))
        .add_source(
            Environment::with_prefix("OXIDGENE")
                .prefix_separator("_")
                .separator("__")
                .try_parsing(true),
        )
}

/// Why the media store cannot be configured. Its message names a setting,
/// never a value, so it can be logged: a key or an endpoint is a secret or
/// close to one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaConfigError {
    /// The S3 backend needs this environment variable.
    MissingS3Key(&'static str),
    /// The S3 client refused the bucket, region or endpoint settings.
    InvalidS3,
}

impl std::fmt::Display for MediaConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingS3Key(variable) => {
                write!(f, "{variable} is required for the S3 media backend")
            }
            Self::InvalidS3 => f.write_str("invalid S3 media storage configuration"),
        }
    }
}

impl std::error::Error for MediaConfigError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_defaults_do_not_expose_the_unauthenticated_backend() {
        assert_eq!(default_host(), "127.0.0.1");
        assert_eq!(default_cors_origin(), "http://127.0.0.1:8081");
    }

    /// The server's bounds are Geneanet's sizes unless the operator says
    /// otherwise; a quota of `0` is none, and what is no size names its
    /// variable.
    #[test]
    fn the_intake_limits_default_to_geneanets_and_follow_the_settings() {
        let with = |settings: &[(&str, &str)]| {
            let mut builder = Config::builder();
            for (key, value) in settings {
                builder = builder.set_override(*key, *value).expect("a setting");
            }
            builder
                .build()
                .and_then(Config::try_deserialize::<ServerConfig>)
                .expect("a configuration")
                .intake_limits()
        };
        assert_eq!(with(&[]), Ok(IntakeLimits::default()));
        assert_eq!(
            with(&[
                ("import_gedcom_max", "64M"),
                ("media_quota", "2G"),
                ("import_archive_entries", "500"),
            ]),
            Ok(IntakeLimits {
                genealogy_bytes: 64 * 1024 * 1024,
                archive_entries: 500,
                tree_media_bytes: Some(2 * 1024 * 1024 * 1024),
            })
        );
        assert_eq!(
            with(&[("media_quota", "0")]).map(|limits| limits.tree_media_bytes),
            Ok(None)
        );
        assert_eq!(
            with(&[("import_gedcom_max", "0")]),
            Err("OXIDGENE_IMPORT_GEDCOM_MAX")
        );
        assert_eq!(
            with(&[("media_quota", "plenty")]),
            Err("OXIDGENE_MEDIA_QUOTA")
        );
        assert_eq!(
            with(&[("import_archive_entries", "0")]),
            Err("OXIDGENE_IMPORT_ARCHIVE_ENTRIES")
        );
    }

    /// The reason names the missing variable, so the operator knows which
    /// one to set, and no value.
    #[test]
    fn a_missing_s3_key_is_named() {
        let config: ServerConfig = Config::builder()
            .set_override("media_backend", "s3")
            .and_then(|builder| builder.set_override("s3_access_key_id", "AKEXAMPLE"))
            .and_then(|builder| builder.build())
            .and_then(Config::try_deserialize)
            .expect("an S3 configuration");
        let Err(error) = config.media_store() else {
            panic!("a missing secret key must be refused");
        };
        assert_eq!(
            error,
            MediaConfigError::MissingS3Key("OXIDGENE_S3_SECRET_ACCESS_KEY")
        );
        assert_eq!(
            error.to_string(),
            "OXIDGENE_S3_SECRET_ACCESS_KEY is required for the S3 media backend"
        );
    }

    #[test]
    fn graphiql_is_off_unless_asked_for() {
        let config: ServerConfig = Config::builder()
            .build()
            .and_then(Config::try_deserialize)
            .expect("every field has a default");
        assert!(!config.graphiql);
    }

    #[test]
    fn the_work_dir_is_the_one_configured_or_the_users_cache() {
        let configured: ServerConfig = Config::builder()
            .set_override("work_dir", "/srv/oxidgene-work")
            .and_then(|builder| builder.build())
            .and_then(Config::try_deserialize)
            .expect("a configured work directory");
        assert_eq!(
            configured.work_dir().root(),
            std::path::Path::new("/srv/oxidgene-work")
        );
        let unset: ServerConfig = Config::builder()
            .build()
            .and_then(Config::try_deserialize)
            .expect("every field has a default");
        assert_eq!(unset.work_dir(), WorkDir::user_default());
    }

    #[test]
    fn the_tree_cache_defaults_to_its_budget_and_idle_delay() {
        let unset: ServerConfig = Config::builder()
            .build()
            .and_then(Config::try_deserialize)
            .expect("every field has a default");
        assert_eq!(unset.cache(), CacheSettings::default());
        let configured: ServerConfig = Config::builder()
            .set_override("cache_mib", 64)
            .and_then(|builder| builder.set_override("cache_idle_secs", 30))
            .and_then(|builder| builder.build())
            .and_then(Config::try_deserialize)
            .expect("a configured cache");
        assert_eq!(
            configured.cache(),
            CacheSettings::shared(64 * 1024 * 1024, Duration::from_secs(30))
        );
    }

    #[test]
    fn console_logs_default_to_text() {
        assert_eq!(default_log_format().parse(), Ok(LogFormat::Text));
    }

    #[test]
    fn debug_output_redacts_credentials() {
        let mut config: ServerConfig = Config::builder()
            .build()
            .and_then(Config::try_deserialize)
            .expect("every field has a default");
        config.database_url = "postgres://fixture:hunter2@db.example.invalid/app".into();
        config.s3_access_key_id = Some("fixture-key".into());
        config.s3_secret_access_key = Some("s3-secret-value".into());

        let printed = format!("{config:?}");

        assert!(!printed.contains("hunter2"), "{printed}");
        assert!(!printed.contains("s3-secret-value"), "{printed}");
        assert!(
            printed.contains("postgres://fixture:<redacted>@db.example.invalid/app"),
            "{printed}"
        );
        assert!(printed.contains("fixture-key"), "{printed}");
    }

    #[test]
    fn urls_without_a_password_print_unchanged() {
        for url in [
            "sqlite:///data/oxidgene.db?mode=rwc",
            "postgres://db.example.invalid/app",
            "postgres://fixture@db.example.invalid/app",
            "postgres://db.example.invalid/app?options=a@b:c",
        ] {
            assert_eq!(redact_url_password(url), url);
        }
        assert_eq!(
            redact_url_password("postgres://fixture:p@ss:w@rd@db.example.invalid"),
            "postgres://fixture:<redacted>@db.example.invalid"
        );
    }

    #[test]
    fn authentication_defaults_to_oidc_and_reads_the_variables_of_the_specification() {
        let settings: AuthSettings = Config::builder()
            .build()
            .and_then(Config::try_deserialize)
            .expect("every field has a default");
        assert_eq!(settings.auth_mode, "oidc");
        let settings: AuthSettings = Config::builder()
            .set_override("auth_mode", "none")
            .and_then(|builder| builder.set_override("oidc_client_secret", 12345))
            .and_then(|builder| builder.set_override("oidc_trust_email", true))
            .and_then(|builder| builder.build())
            .and_then(Config::try_deserialize)
            .expect("settings");
        assert_eq!(settings.auth_mode, "none");
        assert_eq!(settings.oidc_client_secret.as_deref(), Some("12345"));
        assert!(settings.oidc_trust_email);
    }

    #[test]
    fn local_media_backend_defaults_to_filesystem() {
        assert_eq!(MediaBackend::default(), MediaBackend::Filesystem);
    }
}
