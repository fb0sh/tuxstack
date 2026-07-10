//! Shared types and protocol definitions for tuxstack.
//!
//! This crate defines the types used across daemon, GUI, and CLI
//! to ensure consistent serialization over the Unix socket JSON-RPC protocol.

pub mod container;
pub mod instance;
pub mod protocol;
pub mod monitor;

mod util;
pub use container::*;
pub use instance::*;
pub use protocol::*;
pub use monitor::*;
pub use util::*;
