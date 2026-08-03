//! Docker connection management.

use std::path::PathBuf;
use std::time::Duration;

use bollard::Docker;

use crate::config::ResolvedDockerConfig;
use crate::error::{classify_api_error, classify_connect_error, DockerError};
use crate::mapping::system::map_system_info;
use crate::models::DockerSystemInfo;

/// Connection settings for the Docker Engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DockerConfig {
    /// Explicit Docker host. If `None`, the local default
    /// (`/var/run/docker.sock` or `DOCKER_HOST`) is used.
    pub host: Option<String>,
    /// Per-operation timeout.
    pub request_timeout: Duration,
    /// Connection establishment timeout.
    pub connect_timeout: Duration,
}

impl Default for DockerConfig {
    fn default() -> Self {
        Self {
            host: None,
            request_timeout: Duration::from_secs(30),
            connect_timeout: Duration::from_secs(5),
        }
    }
}

impl From<ResolvedDockerConfig> for DockerConfig {
    fn from(value: ResolvedDockerConfig) -> Self {
        Self {
            host: value.host.filter(|h| !h.is_empty()),
            connect_timeout: Duration::from_secs(value.connect_timeout_seconds),
            request_timeout: Duration::from_secs(value.operation_timeout_seconds),
        }
    }
}

/// A connection to the Docker Engine backed by Bollard.
#[derive(Clone)]
pub struct DockerClient {
    docker: Docker,
    config: DockerConfig,
    /// The socket path in use, when connected over a local Unix socket.
    socket_path: Option<PathBuf>,
}

impl DockerClient {
    /// Connect using the local default Docker socket, honoring `DOCKER_HOST`.
    pub fn connect_default() -> Result<Self, DockerError> {
        Self::connect_with_config(DockerConfig::default())
    }

    /// Connect using the given configuration.
    ///
    /// Resolution order:
    /// 1. `config.host` when set (accepts `unix://`, `tcp://`, `http://`, `https://`, `ssh://`).
    /// 2. `DOCKER_HOST` environment variable.
    /// 3. Bollard's local default (Unix socket).
    pub fn connect_with_config(config: DockerConfig) -> Result<Self, DockerError> {
        let host = config.host.clone();
        let connect_timeout = config.connect_timeout;

        let docker = match host.as_deref() {
            Some(h) if h.starts_with("unix://") => {
                let path = h.trim_start_matches("unix://");
                let path_buf = PathBuf::from(path);
                if !path_buf.exists() {
                    return Err(DockerError::SocketNotFound(path_buf));
                }
                connect_unix(&path_buf, connect_timeout)?
            }
            Some(h)
                if h.starts_with("tcp://")
                    || h.starts_with("http://")
                    || h.starts_with("https://")
                    || h.starts_with("ssh://") =>
            {
                Docker::connect_with_http(h, connect_timeout.as_secs(), API_VERSION)
                    .map_err(|e| classify_connect_error(&e, None))?
            }
            Some(other) => {
                return Err(DockerError::UnsupportedConnection(other.to_string()));
            }
            None => {
                // DOCKER_HOST takes priority over the local default.
                if let Ok(env_host) = std::env::var("DOCKER_HOST") {
                    if !env_host.trim().is_empty() {
                        let env = env_host.trim().to_string();
                        if env.starts_with("unix://") {
                            let path_buf = PathBuf::from(env.trim_start_matches("unix://"));
                            if !path_buf.exists() {
                                return Err(DockerError::SocketNotFound(path_buf));
                            }
                            connect_unix(&path_buf, connect_timeout)?
                        } else if env.starts_with("tcp://")
                            || env.starts_with("http://")
                            || env.starts_with("https://")
                            || env.starts_with("ssh://")
                        {
                            Docker::connect_with_http(&env, connect_timeout.as_secs(), API_VERSION)
                                .map_err(|e| classify_connect_error(&e, None))?
                        } else {
                            return Err(DockerError::UnsupportedConnection(env));
                        }
                    } else {
                        connect_local_default(connect_timeout)?
                    }
                } else {
                    connect_local_default(connect_timeout)?
                }
            }
        };

        Ok(Self {
            docker,
            config,
            socket_path: host
                .as_deref()
                .filter(|h| h.starts_with("unix://"))
                .map(|h| PathBuf::from(h.trim_start_matches("unix://"))),
        })
    }

    /// The effective connection settings.
    pub fn config(&self) -> &DockerConfig {
        &self.config
    }

    /// The socket path in use, when connected over a local Unix socket.
    pub fn socket_path(&self) -> Option<&PathBuf> {
        self.socket_path.as_ref()
    }

    /// Access to the underlying Bollard client (internal use only).
    pub(crate) fn inner(&self) -> &Docker {
        &self.docker
    }

    /// Verify connectivity by pinging the Docker Engine.
    pub async fn ping(&self) -> Result<(), DockerError> {
        let docker = self.docker.clone();
        tokio::time::timeout(self.config.request_timeout, docker.ping())
            .await
            .map_err(|_| DockerError::OperationTimeout)?
            .map_err(|e| classify_api_error(&e, "system"))?;
        Ok(())
    }

    /// Fetch the Docker Engine system information.
    pub async fn system_info(&self) -> Result<DockerSystemInfo, DockerError> {
        let docker = self.docker.clone();
        let info = tokio::time::timeout(self.config.request_timeout, docker.info())
            .await
            .map_err(|_| DockerError::OperationTimeout)?
            .map_err(|e| classify_api_error(&e, "system"))?;
        Ok(map_system_info(info))
    }
}

/// API version we target. Docker Engine 27+ supports v1.47; the Engine
/// negotiates downwards when it is older.
const API_VERSION: &bollard::ClientVersion = &bollard::ClientVersion {
    major_version: 1,
    minor_version: 47,
};

fn connect_unix(path: &PathBuf, timeout: Duration) -> Result<Docker, DockerError> {
    let path_str = path.to_string_lossy().to_string();
    Docker::connect_with_unix(&path_str, timeout.as_secs(), API_VERSION)
        .map_err(|e| classify_connect_error(&e, Some(path)))
}

fn connect_local_default(timeout: Duration) -> Result<Docker, DockerError> {
    let socket = PathBuf::from("/var/run/docker.sock");
    if !socket.exists() {
        return Err(DockerError::SocketNotFound(socket));
    }
    connect_unix(&socket, timeout)
}
