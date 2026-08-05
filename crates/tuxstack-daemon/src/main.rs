mod config;
mod ipc;
mod state;

use anyhow::{Context, Result};
use config::DaemonPaths;
use ipc::IpcServer;
use state::DaemonState;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    let paths = DaemonPaths::from_env().context("resolve daemon paths")?;
    let state = DaemonState::start(paths)
        .await
        .context("start TuxStack daemon")?;
    let server = IpcServer::bind(state.clone()).context("start typed IPC server")?;
    tracing::info!(
        socket = %state.paths.socket_path.display(),
        mount = %state.paths.mount_point.display(),
        "tuxstackd ready"
    );

    let server_task = tokio::spawn(server.run());
    tokio::select! {
        result = server_task => result.context("join IPC server")??,
        signal = shutdown_signal() => signal?,
    }
    state.stop().await.context("stop TuxStack daemon")?;
    tracing::info!("tuxstackd stopped");
    Ok(())
}

async fn shutdown_signal() -> Result<()> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut terminate = signal(SignalKind::terminate()).context("install SIGTERM handler")?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => result.context("wait for SIGINT")?,
            _ = terminate.recv() => {},
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c().await.context("wait for shutdown")
    }
}
