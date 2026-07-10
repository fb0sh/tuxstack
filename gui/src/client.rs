use anyhow::{Context, Result};
use tuxstack_common::protocol::*;
use tuxstack_common::{ContainerInfo, SystemStatus};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

/// Client for communicating with the tuxstack daemon over Unix socket.
pub struct DaemonClient {
    stream: BufReader<UnixStream>,
    next_id: u64,
}

impl DaemonClient {
    /// Connect to the daemon Unix socket.
    pub async fn connect() -> Result<Self> {
        let socket_path = tuxstack_common::socket_path();
        let stream = UnixStream::connect(&socket_path)
            .await
            .with_context(|| format!("could not connect to daemon at {:?}", socket_path))?;

        Ok(Self {
            stream: BufReader::new(stream),
            next_id: 1,
        })
    }

    /// Send a JSON-RPC request and await the response.
    /// Uses alternating read/write on the full duplex stream.
    async fn call(&mut self, method: &str, params: serde_json::Value) -> Result<serde_json::Value> {
        let id = self.next_id;
        self.next_id += 1;

        let request = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id,
            method: method.into(),
            params,
        };

        let body = serde_json::to_string(&request)?;

        // Write request via the underlying stream
        {
            let stream = self.stream.get_mut();
            stream.writable().await?;
            stream.write_all(body.as_bytes()).await?;
            stream.write_all(b"\n").await?;
            stream.flush().await?;
        }

        // Read response
        let mut line = String::new();
        self.stream.read_line(&mut line).await?;

        let response: JsonRpcResponse = serde_json::from_str(&line)?;

        match (response.result, response.error) {
            (Some(result), _) => Ok(result),
            (_, Some(err)) => Err(anyhow::anyhow!("RPC error [{}]: {}", err.code, err.message)),
            (None, None) => Err(anyhow::anyhow!("empty RPC response")),
        }
    }

    /// Detect system status
    pub async fn detect(&mut self) -> Result<SystemStatus> {
        let value = self.call(methods::SYSTEM_DETECT, serde_json::json!({})).await?;
        Ok(serde_json::from_value(value)?)
    }

    /// List containers
    pub async fn list_containers(&mut self, all: bool) -> Result<Vec<ContainerInfo>> {
        let value = self
            .call(methods::DOCKER_LIST_CONTAINERS, serde_json::json!({"all": all}))
            .await?;
        Ok(serde_json::from_value(value)?)
    }
}
