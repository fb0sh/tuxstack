use std::path::PathBuf;

/// The path for the tuxstack daemon Unix socket.
pub fn socket_path() -> PathBuf {
    let runtime_dir =
        std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(runtime_dir).join("tuxstack.sock")
}
