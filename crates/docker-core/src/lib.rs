//! # tuxstack-docker-core
//!
//! The shared Docker core library used by both the GUI and the CLI.
//!
//! It connects to the Docker Engine through [Bollard], exposes domain
//! models and services, and never leaks Bollard types to callers.
//!
//! [Bollard]: https://docs.rs/bollard

pub mod client;
pub mod config;
pub mod error;
pub mod mapping;
pub mod models;
pub mod services;
pub mod streams;

pub use client::{DockerClient, DockerConfig};
pub use config::ResolvedConfig;
pub use error::DockerError;
pub use models::*;
pub use services::{
    ContainerService, DockerServices, ImageService, NetworkService, SystemService, VolumeService,
};

/// Re-export of common formatting helpers for byte sizes.
pub mod format {
    /// Format a byte count as a human readable string.
    pub fn bytes(bytes: u64) -> String {
        const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
        if bytes == 0 {
            return "0 B".to_string();
        }
        let mut value = bytes as f64;
        let mut unit = 0;
        while value >= 1024.0 && unit < UNITS.len() - 1 {
            value /= 1024.0;
            unit += 1;
        }
        if unit == 0 {
            format!("{bytes} B")
        } else {
            format!("{value:.1} {}", UNITS[unit])
        }
    }

    /// Format a byte count as a raw integer string (for machine output).
    pub fn bytes_raw(bytes: u64) -> String {
        bytes.to_string()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn format_bytes_units() {
        use crate::format::bytes;
        assert_eq!(bytes(0), "0 B");
        assert_eq!(bytes(512), "512 B");
        assert_eq!(bytes(2048), "2.0 KB");
        assert_eq!(bytes(5 * 1024 * 1024), "5.0 MB");
        assert_eq!(bytes(3 * 1024 * 1024 * 1024), "3.0 GB");
    }
}
