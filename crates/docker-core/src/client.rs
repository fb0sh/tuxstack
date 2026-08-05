//! Docker connection management.

use std::path::PathBuf;
use std::time::Duration;

use bollard::Docker;

use crate::config::ResolvedDockerConfig;
use crate::error::{DockerError, classify_api_error, classify_connect_error};
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
    /// Credential-redacted endpoint selected after resolving configuration,
    /// `DOCKER_HOST`, and the local default.
    effective_endpoint: String,
    /// Host bind mounts used by volume export are only meaningful locally.
    is_local: bool,
}

impl DockerClient {
    /// Connect using the local default Docker socket, honoring `DOCKER_HOST`.
    pub fn connect_default() -> Result<Self, DockerError> {
        Self::connect_with_config(DockerConfig::default())
    }

    /// Connect using the given configuration.
    ///
    /// Resolution order:
    /// 1. `config.host` when set (`unix://`, `tcp://`, `http://`, `https://`;
    ///    `ssh://` requires a Bollard connector not enabled by this build).
    /// 2. `DOCKER_HOST` environment variable.
    /// 3. Bollard's local default (Unix socket).
    pub fn connect_with_config(config: DockerConfig) -> Result<Self, DockerError> {
        let connect_timeout = config.connect_timeout;
        let resolved_host = config
            .host
            .clone()
            .or_else(|| {
                std::env::var("DOCKER_HOST")
                    .ok()
                    .map(|host| host.trim().to_string())
                    .filter(|host| !host.is_empty())
            })
            .unwrap_or_else(|| "unix:///var/run/docker.sock".to_string());

        let (docker, socket_path, is_local) = if resolved_host.starts_with("unix://") {
            let path = PathBuf::from(resolved_host.trim_start_matches("unix://"));
            if !path.exists() {
                return Err(DockerError::SocketNotFound(path));
            }
            (connect_unix(&path, connect_timeout)?, Some(path), true)
        } else if resolved_host.starts_with("ssh://") {
            // The workspace's Bollard dependency does not enable its `ssh`
            // connector, so do not misroute SSH endpoints through plain HTTP.
            return Err(DockerError::UnsupportedConnection(
                redact_endpoint_userinfo(&resolved_host),
            ));
        } else if resolved_host.starts_with("tcp://")
            || resolved_host.starts_with("http://")
            || resolved_host.starts_with("https://")
        {
            let docker =
                Docker::connect_with_http(&resolved_host, connect_timeout.as_secs(), API_VERSION)
                    .map_err(|e| classify_connect_error(&e, None))?;
            (docker, None, false)
        } else {
            return Err(DockerError::UnsupportedConnection(
                redact_endpoint_userinfo(&resolved_host),
            ));
        };
        let effective_endpoint = redact_endpoint_userinfo(&resolved_host);

        Ok(Self {
            docker,
            config,
            socket_path,
            effective_endpoint,
            is_local,
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

    /// Whether this client reaches a local Unix-socket Engine. Host bind
    /// mounts must not be offered for remote engines.
    pub fn is_local(&self) -> bool {
        self.is_local
    }

    /// The resolved endpoint in credential-redacted form.
    pub fn effective_endpoint(&self) -> &str {
        &self.effective_endpoint
    }

    /// A stable, credential-redacted fingerprint of the endpoint used for
    /// cache and container-group isolation.
    pub fn endpoint_fingerprint(&self) -> String {
        self.effective_endpoint.clone()
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

fn redact_endpoint_userinfo(endpoint: &str) -> String {
    let Some((scheme, remainder)) = endpoint.split_once("://") else {
        return endpoint.to_string();
    };
    if matches!(scheme, "unix" | "npipe") {
        return endpoint.to_string();
    }

    let authority_end = remainder.find('/').unwrap_or(remainder.len());
    let (authority, suffix) = remainder.split_at(authority_end);
    let redacted_authority = authority
        .rsplit_once('@')
        .map(|(_, host)| host)
        .unwrap_or(authority);
    format!("{scheme}://{redacted_authority}{suffix}")
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

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::sync::Mutex;

    use super::*;

    static DOCKER_HOST_LOCK: Mutex<()> = Mutex::new(());

    struct DockerHostGuard(Option<OsString>);

    impl DockerHostGuard {
        fn set(value: &str) -> Self {
            let previous = std::env::var_os("DOCKER_HOST");
            // SAFETY: all DOCKER_HOST-mutating tests in this module hold
            // DOCKER_HOST_LOCK for the guard's lifetime.
            unsafe { std::env::set_var("DOCKER_HOST", value) };
            Self(previous)
        }
    }

    impl Drop for DockerHostGuard {
        fn drop(&mut self) {
            // SAFETY: the creating test still holds DOCKER_HOST_LOCK while
            // this guard is dropped.
            unsafe {
                match self.0.take() {
                    Some(value) => std::env::set_var("DOCKER_HOST", value),
                    None => std::env::remove_var("DOCKER_HOST"),
                }
            }
        }
    }

    fn remote_config(host: &str) -> DockerConfig {
        DockerConfig {
            host: Some(host.to_string()),
            ..DockerConfig::default()
        }
    }

    #[test]
    fn explicit_remote_endpoint_drives_identity_and_locality() {
        let client =
            DockerClient::connect_with_config(remote_config("tcp://docker-one.example:2375"))
                .expect("construct remote client");

        assert_eq!(client.effective_endpoint(), "tcp://docker-one.example:2375");
        assert_eq!(
            client.endpoint_fingerprint(),
            "tcp://docker-one.example:2375"
        );
        assert!(!client.is_local());

        let other =
            DockerClient::connect_with_config(remote_config("tcp://docker-two.example:2375"))
                .expect("construct second remote client");
        assert_ne!(client.endpoint_fingerprint(), other.endpoint_fingerprint());
    }

    #[test]
    fn docker_host_remote_is_the_effective_endpoint_when_config_host_is_none() {
        let _lock = DOCKER_HOST_LOCK.lock().expect("DOCKER_HOST lock");
        let _env = DockerHostGuard::set("tcp://engine-user:secret@remote.example:2375");
        let client = DockerClient::connect_with_config(DockerConfig::default())
            .expect("construct env remote client");

        assert_eq!(client.endpoint_fingerprint(), "tcp://remote.example:2375");
        assert!(!client.is_local());
    }

    #[test]
    fn docker_host_unix_is_local_and_uses_the_resolved_socket() {
        let _lock = DOCKER_HOST_LOCK.lock().expect("DOCKER_HOST lock");
        let socket = tempfile::NamedTempFile::new().expect("temporary socket path");
        let endpoint = format!("unix://{}", socket.path().display());
        let _env = DockerHostGuard::set(&endpoint);
        let client = DockerClient::connect_with_config(DockerConfig::default())
            .expect("construct env unix client");

        assert_eq!(client.endpoint_fingerprint(), endpoint);
        assert_eq!(
            client.socket_path().map(PathBuf::as_path),
            Some(socket.path())
        );
        assert!(client.is_local());
    }

    #[test]
    fn unsupported_ssh_error_redacts_userinfo() {
        let error = DockerClient::connect_with_config(remote_config(
            "ssh://alice:secret@remote.example:2222",
        ))
        .err()
        .expect("SSH should require Bollard's optional connector");

        assert_eq!(
            error.to_string(),
            "Unsupported Docker connection method: ssh://remote.example:2222"
        );
    }

    #[test]
    fn endpoint_identity_redacts_userinfo_without_losing_scheme_or_ipv6() {
        let endpoint = "https://alice:secret@[2001:db8::2]:2376/api";
        let client = DockerClient::connect_with_config(remote_config(endpoint))
            .expect("construct credentialed remote client");

        assert_eq!(
            client.endpoint_fingerprint(),
            "https://[2001:db8::2]:2376/api"
        );
        assert!(!client.endpoint_fingerprint().contains("alice"));
        assert!(!client.endpoint_fingerprint().contains("secret"));
    }
}
