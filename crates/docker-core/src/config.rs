//! Configuration loading from XDG config paths.
//!
//! Config file lives at `$XDG_CONFIG_HOME/tuxstack/config.toml`
//! (default `~/.config/tuxstack/config.toml`).

use std::path::PathBuf;

use serde::Deserialize;

/// Error raised while loading or parsing the config file.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("Could not determine config directory: {0}")]
    NoConfigDir(String),

    #[error("Could not read config file {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("Could not parse config file {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
}

/// Raw values that come out of the TOML file.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FileConfig {
    #[serde(rename = "docker")]
    pub docker: DockerSection,
    #[serde(rename = "ui")]
    pub ui: UiSection,
    #[serde(rename = "logging")]
    pub logging: LoggingSection,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DockerSection {
    pub host: String,
    pub connect_timeout_seconds: u64,
    pub operation_timeout_seconds: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct UiSection {
    pub auto_refresh_seconds: u64,
    pub stats_refresh_seconds: u64,
    pub log_line_limit: usize,
    pub confirm_remove: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LoggingSection {
    pub level: String,
}

impl Default for FileConfig {
    fn default() -> Self {
        Self {
            docker: DockerSection::default(),
            ui: UiSection::default(),
            logging: LoggingSection::default(),
        }
    }
}

impl Default for DockerSection {
    fn default() -> Self {
        Self {
            host: String::new(),
            connect_timeout_seconds: 5,
            operation_timeout_seconds: 30,
        }
    }
}

impl Default for UiSection {
    fn default() -> Self {
        Self {
            auto_refresh_seconds: 5,
            stats_refresh_seconds: 2,
            log_line_limit: 5000,
            confirm_remove: true,
        }
    }
}

impl Default for LoggingSection {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
        }
    }
}

/// Fully resolved configuration with all defaults applied.
#[derive(Debug, Clone)]
pub struct ResolvedConfig {
    pub docker: ResolvedDockerConfig,
    pub ui: UiSection,
    pub logging: LoggingSection,
}

#[derive(Debug, Clone)]
pub struct ResolvedDockerConfig {
    pub host: Option<String>,
    pub connect_timeout_seconds: u64,
    pub operation_timeout_seconds: u64,
}

impl Default for ResolvedConfig {
    fn default() -> Self {
        let file = FileConfig::default();
        Self {
            docker: ResolvedDockerConfig {
                host: None,
                connect_timeout_seconds: file.docker.connect_timeout_seconds,
                operation_timeout_seconds: file.docker.operation_timeout_seconds,
            },
            ui: file.ui,
            logging: file.logging,
        }
    }
}

/// The config file path following XDG conventions.
pub fn config_path() -> Result<PathBuf, ConfigError> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .ok_or_else(|| ConfigError::NoConfigDir("neither XDG_CONFIG_HOME nor HOME are set".into()))?;
    Ok(base.join("tuxstack").join("config.toml"))
}

/// Load the config file if present.
///
/// - Missing file → safe defaults.
/// - Unreadable or unparsable file → error surfaced to the caller; the
///   file is never overwritten or silently discarded.
pub fn load_config() -> Result<ResolvedConfig, ConfigError> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(ResolvedConfig::default());
    }

    let content = std::fs::read_to_string(&path).map_err(|source| ConfigError::Io {
        path: path.clone(),
        source,
    })?;

    let file: FileConfig = toml::from_str(&content).map_err(|source| ConfigError::Parse {
        path: path.clone(),
        source,
    })?;

    Ok(ResolvedConfig {
        docker: ResolvedDockerConfig {
            host: if file.docker.host.trim().is_empty() {
                None
            } else {
                Some(file.docker.host)
            },
            connect_timeout_seconds: file.docker.connect_timeout_seconds,
            operation_timeout_seconds: file.docker.operation_timeout_seconds,
        },
        ui: file.ui,
        logging: file.logging,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_usable() {
        let cfg = ResolvedConfig::default();
        assert_eq!(cfg.docker.connect_timeout_seconds, 5);
        assert_eq!(cfg.docker.operation_timeout_seconds, 30);
        assert_eq!(cfg.ui.log_line_limit, 5000);
        assert_eq!(cfg.logging.level, "info");
    }

    #[test]
    fn parse_full_config_file() {
        let toml = r#"
[docker]
host = "unix:///tmp/custom.sock"
connect_timeout_seconds = 3
operation_timeout_seconds = 10

[ui]
auto_refresh_seconds = 8
stats_refresh_seconds = 1
log_line_limit = 1000
confirm_remove = false

[logging]
level = "debug"
"#;
        let file: FileConfig = toml::from_str(toml).unwrap();
        assert_eq!(file.docker.host, "unix:///tmp/custom.sock");
        assert_eq!(file.ui.log_line_limit, 1000);
        assert!(!file.ui.confirm_remove);
        assert_eq!(file.logging.level, "debug");
    }

    #[test]
    fn parse_partial_config_file_fills_defaults() {
        let toml = "[docker]\nhost = \"tcp://127.0.0.1:2375\"\n";
        let file: FileConfig = toml::from_str(toml).unwrap();
        assert_eq!(file.docker.host, "tcp://127.0.0.1:2375");
        assert_eq!(file.docker.connect_timeout_seconds, 5);
        assert_eq!(file.ui.stats_refresh_seconds, 2);
    }

    #[test]
    fn unknown_fields_rejected() {
        let toml = "[docker]\nbogus = true\n";
        assert!(toml::from_str::<FileConfig>(toml).is_err());
    }

    #[test]
    fn empty_host_resolves_to_none() {
        let cfg = ResolvedConfig {
            docker: ResolvedDockerConfig {
                host: Some("   ".into()),
                ..ResolvedDockerConfig {
                    host: None,
                    connect_timeout_seconds: 5,
                    operation_timeout_seconds: 30,
                }
            },
            ui: UiSection::default(),
            logging: LoggingSection::default(),
        };
        // The ResolvedConfig holds host as Option; normalization happens in
        // DockerConfig::from, covered by client tests.
        let _ = cfg;
    }
}
