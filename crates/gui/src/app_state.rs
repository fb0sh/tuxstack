//! Shared daemon connection registry and pure page-state helpers.
//!
//! The GUI stores only the typed IPC client. Docker clients, event monitors,
//! caches, repositories, helper pools, and Files backends belong to tuxstackd.

use std::sync::{Arc, Mutex};

use tuxstack_client::{Client, DaemonServices};

use crate::error::AppError;

static CLIENT: Mutex<Option<Arc<Client>>> = Mutex::new(None);

pub fn set_client(client: Client) {
    *CLIENT.lock().expect("client lock") = Some(Arc::new(client));
}

pub fn clear_client() {
    *CLIENT.lock().expect("client lock") = None;
}

pub fn get_client() -> Option<Arc<Client>> {
    CLIENT.lock().expect("client lock").clone()
}

/// Construct a lightweight typed facade for the current IPC connection.
/// No backend state is created or retained by this accessor.
pub fn daemon_services() -> Option<DaemonServices> {
    get_client().map(DaemonServices::new)
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum LoadState<T> {
    Idle,
    Loading,
    Ready(T),
    Error(AppError),
}

impl<T> LoadState<T> {
    #[allow(dead_code)]
    pub fn is_loading(&self) -> bool {
        matches!(self, Self::Loading)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disconnected_registry_has_no_facade() {
        clear_client();
        assert!(get_client().is_none());
        assert!(daemon_services().is_none());
    }
}
