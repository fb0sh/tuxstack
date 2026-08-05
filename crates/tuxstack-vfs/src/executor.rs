use std::future::Future;
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::runtime::{Builder, Runtime};

use crate::VfsError;

/// Dedicated provider executor used by synchronous FUSE callbacks.
///
/// A fixed token pool bounds active and waiting provider operations. Futures run on a
/// daemon-owned, four-thread Tokio runtime. The callback waits on a standard channel;
/// it never enters a Tokio runtime and never calls `block_on`, avoiding nested-runtime
/// panics. Dropping the timed-out future cancels cooperative provider work.
pub struct ProviderExecutor {
    runtime: Arc<Runtime>,
    available: Arc<Mutex<Receiver<()>>>,
    return_token: SyncSender<()>,
    timeout: Duration,
}

impl std::fmt::Debug for ProviderExecutor {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProviderExecutor")
            .field("timeout", &self.timeout)
            .finish_non_exhaustive()
    }
}

impl ProviderExecutor {
    pub const WORKER_THREADS: usize = 4;

    pub fn new(max_in_flight: usize, timeout: Duration) -> Result<Self, VfsError> {
        if max_in_flight == 0 {
            return Err(VfsError::InvalidInput("executor limit must be non-zero"));
        }
        if timeout.is_zero() {
            return Err(VfsError::InvalidInput("operation timeout must be non-zero"));
        }
        let runtime = Builder::new_multi_thread()
            .worker_threads(Self::WORKER_THREADS)
            .thread_name("tuxstack-vfs-provider")
            .enable_all()
            .build()
            .map_err(VfsError::from)?;
        let (return_token, available) = sync_channel(max_in_flight);
        for _ in 0..max_in_flight {
            return_token
                .send(())
                .map_err(|_| VfsError::Unavailable("executor token pool closed".to_owned()))?;
        }
        Ok(Self {
            runtime: Arc::new(runtime),
            available: Arc::new(Mutex::new(available)),
            return_token,
            timeout,
        })
    }

    pub fn execute<F, T>(&self, future: F) -> Result<T, VfsError>
    where
        F: Future<Output = Result<T, VfsError>> + Send + 'static,
        T: Send + 'static,
    {
        self.available
            .lock()
            .map_err(|_| VfsError::Unavailable("executor token lock poisoned".to_owned()))?
            .recv()
            .map_err(|_| VfsError::Unavailable("executor stopped".to_owned()))?;

        let (sender, receiver) = sync_channel(1);
        let timeout = self.timeout;
        self.runtime.spawn(async move {
            let result = tokio::time::timeout(timeout, future)
                .await
                .map_err(|_| VfsError::TimedOut)
                .and_then(|result| result);
            let _ = sender.send(result);
        });
        let result = receiver
            .recv()
            .map_err(|_| VfsError::Unavailable("provider task was cancelled".to_owned()));
        let _ = self.return_token.send(());
        result?
    }

    pub fn timeout(&self) -> Duration {
        self.timeout
    }
}
