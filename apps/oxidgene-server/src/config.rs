//! Server configuration loaded from environment variables and optional config file.
//!
//! Environment variables (all prefixed with `OXIDGENE_`):
//!
//! | Variable                | Default                                    | Description            |
//! |-------------------------|--------------------------------------------|------------------------|
//! | `OXIDGENE_HOST`         | `0.0.0.0`                                  | Bind address           |
//! | `OXIDGENE_PORT`         | `8080`                                     | Bind port              |
//! | `OXIDGENE_DATABASE_URL` | `postgres://oxidgene:oxidgene@localhost/oxidgene` | Database connection URL |
//! | `OXIDGENE_LOG_LEVEL`    | `info`                                     | Tracing filter         |
//! | `OXIDGENE_CORS_ORIGIN`  | `*`                                        | Allowed CORS origin    |
//!
//! An optional config file can be placed at `oxidgene.toml` in the working
//! directory. Environment variables always override file values.

use config::{Config, Environment, File};
use serde::Deserialize;

/// Application configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    /// Bind address (default: `0.0.0.0`).
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

    /// Allowed CORS origin (default: `*`).
    #[serde(default = "default_cors_origin")]
    pub cors_origin: String,
}

fn default_host() -> String {
    "0.0.0.0".to_string()
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
    "*".to_string()
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
                    .separator("_")
                    .try_parsing(true),
            )
            .build()?;

        config.try_deserialize()
    }
}
