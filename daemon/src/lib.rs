pub mod docker;
pub mod incus;
pub mod server;
pub mod monitor;

use anyhow::Result;
use tokio::net::UnixListener;

pub struct Daemon {
    docker_client: docker::Client,
    incus_client: incus::Client,
    listener: UnixListener,
}

impl Daemon {
    pub async fn new() -> Result<Self> {
        let socket = tuxstack_common::socket_path();

        // Remove stale socket if exists
        if socket.exists() {
            std::fs::remove_file(&socket)?;
        }

        let listener = UnixListener::bind(&socket)?;
        tracing::info!("listening on {:?}", socket);

        Ok(Self {
            docker_client: docker::Client::new().await?,
            incus_client: incus::Client::new(),
            listener,
        })
    }

    pub async fn run(&self) -> Result<()> {
        // Main accept loop
        loop {
            let (stream, addr) = self.listener.accept().await?;
            tracing::debug!("connection from {:?}", addr);

            let docker = self.docker_client.clone();
            let incus = self.incus_client.clone();

            tokio::spawn(async move {
                server::handle_connection(stream, docker, incus).await;
            });
        }
    }
}
