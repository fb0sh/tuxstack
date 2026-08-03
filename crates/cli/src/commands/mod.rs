//! CLI subcommands.

pub mod images;
pub mod info;
pub mod inspect;
pub mod logs;
pub mod networks;
pub mod ps;
pub mod remove;
pub mod restart;
pub mod start;
pub mod stop;
pub mod volumes;

use std::sync::Arc;

use tuxstack_docker_core::{DockerClient, DockerServices};

use crate::error::CliError;

/// Shared state passed to every subcommand.
pub struct CommandContext {
    pub services: DockerServices,
    pub json: bool,
}

impl CommandContext {
    /// Build the context from global options.
    pub fn build(host: Option<String>, timeout_secs: Option<u64>, json: bool) -> Result<Self, CliError> {
        let mut config = tuxstack_docker_core::DockerConfig::default();
        config.host = host.filter(|h| !h.is_empty());
        if let Some(t) = timeout_secs {
            config.request_timeout = std::time::Duration::from_secs(t);
        }

        let client = Arc::new(DockerClient::connect_with_config(config)?);
        Ok(Self {
            services: DockerServices::new(client),
            json,
        })
    }
}
