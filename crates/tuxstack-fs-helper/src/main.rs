//! `tuxstack-fs-helper` — static filesystem browsing helper.
//!
//! Runs inside temporary preview containers (image rootfs or a volume
//! mounted at the browse root) and speaks the `tuxstack-fs-protocol` JSON
//! Lines protocol on stdout. The container's entrypoint is this binary
//! (`hold` keeps it alive); every browse operation is one `docker exec` of a
//! subcommand. stderr is reserved for diagnostics; all protocol output goes
//! to stdout.

mod error;
mod hash;
mod hold;
mod list;
mod metadata;
mod path;
mod preview;
mod stat;

use std::env;

use tuxstack_fs_protocol::HelperMessage;

use crate::error::HelperError as HelperFailure;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("hold") => {
            hold::run();
        }
        Some("hello") => {
            emit(&HelperMessage::Hello {
                protocol: tuxstack_fs_protocol::FS_HELPER_PROTOCOL_VERSION,
                helper_version: env!("CARGO_PKG_VERSION").to_string(),
            });
            Ok(())
        }
        Some("list") => list::run(&args[1..]),
        Some("stat") => stat::run(&args[1..]),
        Some("preview") => preview::run(&args[1..]),
        Some("hash") => hash::run(&args[1..]),
        Some("readlink") => path::run_readlink(&args[1..]),
        Some(other) => Err(HelperFailure::new(
            HelperErrorCode::InvalidArgs,
            format!("unknown command: {other}"),
        )),
        None => Err(HelperFailure::new(
            HelperErrorCode::InvalidArgs,
            "no command given",
        )),
    };
    if let Err(error) = result {
        emit(&error.into_message());
        std::process::exit(1);
    }
}

/// Serialize one protocol message as a JSON line on stdout and flush.
pub fn emit(message: &HelperMessage) {
    let line = serde_json::to_string(message).unwrap_or_else(|_| {
        // Serialization of the fixed message set cannot fail; this guard
        // keeps stdout protocol-clean even under pathological conditions.
        r#"{"kind":"error","code":"io","message":"helper internal serialization failure"}"#
            .to_string()
    });
    use std::io::Write;
    let mut stdout = std::io::stdout().lock();
    let _ = stdout.write_all(line.as_bytes());
    let _ = stdout.write_all(b"\n");
    let _ = stdout.flush();
}

// Re-export for the subcommand modules.
pub use tuxstack_fs_protocol::HelperErrorCode;
