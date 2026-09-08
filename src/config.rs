use std::path::{Path, PathBuf};
use std::str::FromStr;

use config::{Environment as EnvSource, File};
use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
pub enum Environment {
    #[default]
    Local,
    Production,
}

impl Environment {
    const ENV_VAR: &'static str = "APP_ENVIRONMENT";

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Production => "production",
        }
    }

    fn from_env() -> Result<Self, AppConfigError> {
        std::env::var(Self::ENV_VAR).map_or_else(|_| Ok(Self::default()), |value| value.parse())
    }
}

impl FromStr for Environment {
    type Err = AppConfigError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "local" => Ok(Self::Local),
            "production" => Ok(Self::Production),
            other => Err(AppConfigError::UnknownEnvironment(other.to_owned())),
        }
    }
}

#[derive(Debug, Error)]
pub enum AppConfigError {
    #[error("`{0}` is not a known environment (expected `local` or `production`)")]
    UnknownEnvironment(String),

    #[error("could not read configuration from `{}`", dir.display())]
    Unreadable {
        dir: PathBuf,
        #[source]
        source: config::ConfigError,
    },

    #[error("configuration is invalid (check `{}` and `APP_*` env vars)", dir.display())]
    Invalid {
        dir: PathBuf,
        #[source]
        source: config::ConfigError,
    },
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ServerConfig {
    host: String,
    port: u16,
}

impl ServerConfig {
    pub(crate) fn address(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TelemetryConfig {
    log_filter: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppConfig {
    /// Populated from `APP_ENVIRONMENT` via the env-var source, then overwritten by
    /// [`AppConfig::load`] with the case-insensitive parse it already had to do to
    /// pick the config file.
    #[serde(default)]
    environment: Environment,
    server: ServerConfig,
    telemetry: TelemetryConfig,
}

impl AppConfig {
    /// Layered load: `base.yaml`, then `{environment}.yaml`, then `APP_*` env vars.
    /// Later sources win. Env vars nest with `__`, e.g. `APP_SERVER__PORT=9999`.
    pub fn load() -> Result<Self, AppConfigError> {
        let environment = Environment::from_env()?;
        let dir = Self::config_dir();

        let settings = config::Config::builder()
            .add_source(File::from(dir.join("base.yaml")))
            .add_source(File::from(
                dir.join(format!("{}.yaml", environment.as_str())),
            ))
            .add_source(
                EnvSource::with_prefix("APP")
                    .prefix_separator("_")
                    .separator("__")
                    .try_parsing(true),
            )
            .build()
            .map_err(|source| AppConfigError::Unreadable {
                dir: dir.clone(),
                source,
            })?;

        let mut config: Self =
            settings
                .try_deserialize()
                .map_err(|source| AppConfigError::Invalid {
                    dir: dir.clone(),
                    source,
                })?;
        config.environment = environment;
        Ok(config)
    }

    /// `APP_CONFIG_DIR` wins so a deployed binary can point elsewhere; otherwise the
    /// crate's own `configuration/`, which is correct under both `cargo run` and
    /// `cargo test` (those have different working directories).
    fn config_dir() -> PathBuf {
        std::env::var("APP_CONFIG_DIR").map_or_else(
            |_| Path::new(env!("CARGO_MANIFEST_DIR")).join("configuration"),
            PathBuf::from,
        )
    }

    pub const fn environment(&self) -> Environment {
        self.environment
    }

    pub(crate) const fn server(&self) -> &ServerConfig {
        &self.server
    }

    pub fn log_filter(&self) -> &str {
        &self.telemetry.log_filter
    }
}
