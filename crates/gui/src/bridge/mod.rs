//! QML-facing bridge objects (CXX-Qt).
//!
//! Each bridge module is listed in `build.rs` so its C++ code is
//! generated and compiled.

pub mod app_bridge;
pub mod container_live_bridge;
pub mod container_terminal_bridge;
pub mod container_tools_bridge;
pub mod containers_bridge;
pub mod image_bridge;
pub mod image_file_bridge;
pub mod network_bridge;
pub mod resource_bridges;
pub mod volume_bridge;
pub mod volume_file_bridge;
