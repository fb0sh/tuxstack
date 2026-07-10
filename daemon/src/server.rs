use crate::docker;
use crate::incus;
use crate::monitor::Monitor;
use tuxstack_common::protocol::*;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

/// Handle a single daemon connection over Unix socket.
pub async fn handle_connection(
    stream: UnixStream,
    docker: docker::Client,
    incus: incus::Client,
) {
    tracing::debug!("handling connection");

    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();

    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }

        let request: JsonRpcRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let resp = JsonRpcResponse::error(0, -32700, format!("Parse error: {e}"));
                let _ = writer
                    .write_all(format!("{}\n", serde_json::to_string(&resp).unwrap()).as_bytes())
                    .await;
                continue;
            }
        };

        let response = dispatch(&request, &docker, &incus).await;
        let _ = writer
            .write_all(format!("{}\n", serde_json::to_string(&response).unwrap()).as_bytes())
            .await;
    }
}

async fn dispatch(
    req: &JsonRpcRequest,
    docker: &docker::Client,
    _incus: &incus::Client,
) -> JsonRpcResponse {
    match req.method.as_str() {
        methods::SYSTEM_DETECT => {
            let status = Monitor::detect_system().await;
            JsonRpcResponse::success(req.id, serde_json::to_value(status).unwrap())
        }
        methods::DOCKER_LIST_CONTAINERS => {
            let all = req.params.get("all").and_then(|v| v.as_bool()).unwrap_or(true);
            match docker.list_containers(all).await {
                Ok(containers) => {
                    JsonRpcResponse::success(req.id, serde_json::to_value(containers).unwrap())
                }
                Err(e) => JsonRpcResponse::error(req.id, -1, e.to_string()),
            }
        }
        methods::DOCKER_CONTAINER_ACTION => {
            JsonRpcResponse::error(req.id, -32000, "not implemented")
        }
        methods::DOCKER_CONTAINER_LOGS => {
            JsonRpcResponse::error(req.id, -32000, "not implemented")
        }
        methods::DOCKER_LIST_IMAGES => {
            JsonRpcResponse::error(req.id, -32000, "not implemented")
        }
        methods::DOCKER_PULL_IMAGE => {
            JsonRpcResponse::error(req.id, -32000, "not implemented")
        }
        methods::SYSTEM_STATUS => {
            let status = Monitor::detect_system().await;
            JsonRpcResponse::success(req.id, serde_json::to_value(status).unwrap())
        }
        _ => JsonRpcResponse::error(req.id, -32601, format!("Method not found: {}", req.method)),
    }
}
