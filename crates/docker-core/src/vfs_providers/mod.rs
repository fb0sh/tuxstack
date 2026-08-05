//! Daemon-owned read-only filesystem providers.
//!
//! These providers are not GUI services. They are composed by `tuxstackd`
//! into the FUSE namespace and are the only Docker-backed Files path.

pub mod archive;
pub mod bind;
pub mod container;
pub mod helper_bind;
pub mod image;
pub mod spool;
mod support;
pub mod tar_index;
pub mod volume;

pub use archive::*;
pub use bind::*;
pub use container::*;
pub use helper_bind::*;
pub use image::*;
pub use spool::*;
pub use tar_index::*;
pub use volume::*;
