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
//! | `OXIDGENE_CORS_ORIGIN`           | `http://127.0.0.1:8081`                    | Allowed CORS origin         |
//! | `OXIDGENE_MEDIA_BACKEND`         | `filesystem`                               | `filesystem` or `s3`        |
//! | `OXIDGENE_MEDIA_ROOT`            | platform data dir (see below)              | Filesystem media root       |
//! | `OXIDGENE_S3_BUCKET`             | `oxidgene-media`                            | S3 bucket                   |
//! | `OXIDGENE_S3_REGION`             | `us-east-1`                                | S3 signing region           |
//! | `OXIDGENE_S3_ENDPOINT`           | unset                                      | S3-compatible endpoint      |
//! | `OXIDGENE_S3_ACCESS_KEY_ID`      | unset                                      | S3 access key               |
//! | `OXIDGENE_S3_SECRET_ACCESS_KEY`  | unset                                      | S3 secret key               |
//!
//! `OXIDGENE_MEDIA_ROOT` defaults to the platform's user-data directory —
//! `~/.local/share/oxidgene/media` on Linux. A containerised deployment
//! normally overrides it with the mount point of a persistent volume.
//!
//! An optional config file can be placed at `oxidgene.toml` in the working
//! directory. Environment variables always override file values.

use std::path::PathBuf;
use std::sync::Arc;

use config::{Config, Environment, File};
use oxidgene_api::media::{FsStore, MediaStore, S3Store, S3StoreConfig};
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
#[derive(Debug, Clone, Deserialize)]
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

    /// Allowed CORS origin (default: `http://127.0.0.1:8081`).
    #[serde(default = "default_cors_origin")]
    pub cors_origin: String,

    /// Directory uploaded media files are stored under.
    #[serde(default = "default_media_root")]
    pub media_root: PathBuf,

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
}

fn default_host() -> String {
    "127.0.0.1".to_string()
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
        let config = Config::builder()
            // Optional config file (not required to exist)
            .add_source(File::with_name("oxidgene").required(false))
            // Environment variables: OXIDGENE_HOST, OXIDGENE_PORT, etc.
            .add_source(
                Environment::with_prefix("OXIDGENE")
                    .prefix_separator("_")
                    .separator("__")
                    .try_parsing(true),
            )
            .build()?;

        config.try_deserialize()
    }

    pub fn media_store(&self) -> Result<Arc<dyn MediaStore>, String> {
        match self.media_backend {
            MediaBackend::Filesystem => Ok(Arc::new(FsStore::new(&self.media_root))),
            MediaBackend::S3 => {
                let access_key_id = self.s3_access_key_id.clone().ok_or_else(|| {
                    "OXIDGENE_S3_ACCESS_KEY_ID is required for the S3 media backend".to_string()
                })?;
                let secret_access_key = self.s3_secret_access_key.clone().ok_or_else(|| {
                    "OXIDGENE_S3_SECRET_ACCESS_KEY is required for the S3 media backend".to_string()
                })?;
                Ok(Arc::new(
                    S3Store::new(S3StoreConfig {
                        bucket: self.s3_bucket.clone(),
                        region: self.s3_region.clone(),
                        endpoint: self.s3_endpoint.clone(),
                        access_key_id,
                        secret_access_key,
                    })
                    .map_err(|_| "invalid S3 media storage configuration".to_string())?,
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_defaults_do_not_expose_the_unauthenticated_backend() {
        assert_eq!(default_host(), "127.0.0.1");
        assert_eq!(default_cors_origin(), "http://127.0.0.1:8081");
    }

    #[test]
    fn local_media_backend_defaults_to_filesystem() {
        assert_eq!(MediaBackend::default(), MediaBackend::Filesystem);
    }
}
