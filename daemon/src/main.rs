#[cfg(not(unix))]
compile_error!("tuxstack daemon only supports Unix platforms");

use anyhow::Result;
use tuxstack_daemon::Daemon;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "tuxstack=debug,info".into()),
        )
        .init();

    tracing::info!("tuxstack daemon starting...");

    let daemon = Daemon::new().await?;
    daemon.run().await?;

    Ok(())
}
