use tuxstack_common::SystemStatus;

/// System detection and monitoring.
pub struct Monitor;

impl Monitor {
    /// Detect what's available on the system
    pub async fn detect_system() -> SystemStatus {
        let docker_sock = std::path::Path::new("/var/run/docker.sock").exists();

        // We can't really check incus on macOS
        let incus_sock = cfg!(target_os = "linux")
            && (std::path::Path::new("/var/lib/incus/unix.socket").exists()
                || std::path::Path::new("/var/snap/incus/common/server/unix.socket").exists());

        SystemStatus {
            docker_available: docker_sock,
            incus_available: incus_sock,
            docker_version: None,
            incus_version: None,
            containers_running: 0,
            containers_total: 0,
            instances_running: 0,
            instances_total: 0,
        }
    }
}
