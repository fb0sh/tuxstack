//! QML-facing bridge objects (CXX-Qt).
//!
//! Each bridge module is listed in `build.rs` so its C++ code is
//! generated and compiled.

pub mod app_bridge;
pub mod container_bridge;
pub mod detail_bridge;
pub mod resource_bridges;
