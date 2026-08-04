//! # tuxstack-docker-core
//!
//! The internal Docker core library used directly by the GUI.
//!
//! It connects to the Docker Engine through [Bollard], exposes domain
//! models and services, and never leaks Bollard types to callers.
//!
//! [Bollard]: https://docs.rs/bollard

pub mod cache;
pub mod client;
pub mod config;
pub mod error;
pub mod instrument;
pub mod mapping;
pub mod models;
pub mod services;
pub mod streams;

pub use client::{DockerClient, DockerConfig};
pub use config::ResolvedConfig;
pub use error::{ContainerError, DockerError};
pub use models::*;
pub use services::container_files::*;
pub use services::container_terminal::*;
pub use services::filesystem::FilesystemService;
pub use services::filesystem::error::FilesystemError;
pub use services::filesystem::types::{
    FilesystemEntry, FilesystemEntryType, FilesystemSession, FilesystemSource, HashRequest,
    ListDirectoryRequest, ListDirectoryResult, PreviewRequest, PreviewResult, StatRequest,
};
pub use services::{
    ComposeService, ContainerService, DockerServices, ImageService, NetworkService, SystemService,
    VolumeService,
};
pub use streams::{ImageExportStream, ImagePullStream};
pub use tuxstack_fs_protocol::FilesystemPathToken;
pub use tuxstack_fs_protocol::decode_base64 as filesystem_decode_base64;

/// Re-export of common formatting helpers for byte sizes.
pub mod format {
    /// Format a byte count as a human readable string.
    pub fn bytes(bytes: u64) -> String {
        const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
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
        assert_eq!(bytes(1024), "1.0 KiB");
        assert_eq!(bytes(2048), "2.0 KiB");
        assert_eq!(bytes(5 * 1024 * 1024), "5.0 MiB");
        assert_eq!(bytes(3 * 1024 * 1024 * 1024), "3.0 GiB");
        assert_eq!(bytes(2 * 1024_u64.pow(4)), "2.0 TiB");
    }
}
