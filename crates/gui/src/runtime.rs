//! Tokio runtime handling for the GUI.
//!
//! A single shared runtime is created at startup. Docker operations are
//! spawned onto it and their results are marshalled back to the Qt
//! thread via `CxxQtThread::queue`; the Qt main thread never blocks.

use std::sync::OnceLock;

use tokio::runtime::Runtime;

static RUNTIME: OnceLock<Runtime> = OnceLock::new();

/// Initialize the shared Tokio runtime (call once from `main`).
pub fn init() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("tuxstack-tokio")
        .build()
        .expect("failed to create Tokio runtime");
    let _ = RUNTIME.set(runtime);
}

/// Spawn a future onto the shared runtime.
pub fn spawn<F>(future: F)
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    handle().spawn(future);
}

/// Get a handle to the shared runtime.
pub fn handle() -> tokio::runtime::Handle {
    RUNTIME
        .get()
        .expect("Tokio runtime must be initialized before use")
        .handle()
        .clone()
}
