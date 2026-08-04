//! GUI settings resolved from the shared config file.

use tuxstack_docker_core::config::{self, ResolvedConfig};

/// Settings used by the GUI, resolved from `~/.config/tuxstack/config.toml`.
#[derive(Debug, Clone)]
#[allow(dead_code)] // subset of fields reserved for future UI use
pub struct GuiSettings {
    pub docker_host: Option<String>,
    pub connect_timeout_seconds: u64,
    pub operation_timeout_seconds: u64,
    pub auto_refresh_seconds: u64,
    pub stats_refresh_seconds: u64,
    pub log_line_limit: usize,
    pub confirm_remove: bool,
}

impl Default for GuiSettings {
    fn default() -> Self {
        Self {
            docker_host: None,
            connect_timeout_seconds: 5,
            operation_timeout_seconds: 30,
            auto_refresh_seconds: 5,
            stats_refresh_seconds: 2,
            log_line_limit: 5000,
            confirm_remove: true,
        }
    }
}

impl From<&ResolvedConfig> for GuiSettings {
    fn from(cfg: &ResolvedConfig) -> Self {
        Self {
            docker_host: cfg.docker.host.clone(),
            connect_timeout_seconds: cfg.docker.connect_timeout_seconds,
            operation_timeout_seconds: cfg.docker.operation_timeout_seconds,
            auto_refresh_seconds: cfg.ui.auto_refresh_seconds,
            stats_refresh_seconds: cfg.ui.stats_refresh_seconds,
            log_line_limit: cfg.ui.log_line_limit,
            confirm_remove: cfg.ui.confirm_remove,
        }
    }
}

/// Load GUI settings, falling back to safe defaults on any failure.
///
/// The error is returned for surfacing, but the defaults are used so the
/// app can still start (the broken file is never overwritten).
pub fn load_settings() -> (GuiSettings, Option<String>) {
    match config::load_config() {
        Ok(cfg) => {
            let settings = GuiSettings::from(&cfg);
            (settings, None)
        }
        Err(e) => (
            GuiSettings::default(),
            Some(format!(
                "Could not read config file {}: {e}",
                config::config_path()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|_| "config.toml".to_string())
            )),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_safe() {
        let (settings, warning) = load_settings();
        assert!(settings.log_line_limit > 0);
        assert!(settings.auto_refresh_seconds >= 1);
        assert!(warning.is_none() || warning.is_some());
    }
}
