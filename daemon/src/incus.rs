use anyhow::Result;
use tuxstack_common::InstanceInfo;

/// Incus REST API client.
///
/// Incus exposes a REST API over a local Unix socket at /var/snap/incus/common/server/unix.socket
/// or /var/lib/incus/unix.socket depending on installation method.
#[derive(Clone)]
pub struct Client {
    _client: reqwest::Client,
}

impl Client {
    pub fn new() -> Self {
        Self {
            _client: reqwest::Client::new(),
        }
    }

    /// Check if Incus is available
    pub async fn ping(&self) -> Result<bool> {
        // TODO: Implement actual ping via Incus REST API
        // For now, just try to connect to the socket
        Ok(std::path::Path::new("/var/lib/incus/unix.socket").exists()
            || std::path::Path::new("/var/snap/incus/common/server/unix.socket").exists())
    }

    /// Placeholder: list instances (needs Linux to test)
    pub async fn list_instances(&self) -> Result<Vec<InstanceInfo>> {
        // TODO: Implement via Incus REST API
        Ok(vec![])
    }
}
