//! Docker engine event stream.
//!
//! Streams `/events` from the Docker Engine **directly over the socket**
//! using a plain HTTP request, rather than through Bollard's `events()`
//! stream, which hangs in this environment (the daemon accepts the same
//! request over curl and manual sockets, so the failure is Bollard-side).
//!
//! The daemon answers with `Transfer-Encoding: chunked` JSON lines, one
//! event per line. This module decodes chunk framing itself and maps each
//! line to a domain [`DockerEvent`].

use std::collections::HashMap;
use std::io;
use std::pin::Pin;
use std::sync::Arc;

use futures_util::Stream;
use serde::Deserialize;
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::net::{TcpStream, UnixStream};
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::CancellationToken;

use crate::client::DockerClient;
use crate::error::DockerError;
use crate::models::DockerEvent;

pub type EventStreamResult = Pin<Box<dyn Stream<Item = Result<DockerEvent, DockerError>> + Send>>;

/// Raw JSON shape of one daemon event line.
#[derive(Debug, Clone, Deserialize)]
struct RawEvent {
    #[serde(rename = "Type")]
    event_type: String,
    #[serde(rename = "Action")]
    action: String,
    #[serde(rename = "Actor")]
    actor: Option<RawActor>,
    #[serde(rename = "time")]
    time: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawActor {
    #[serde(rename = "ID")]
    id: Option<String>,
    #[serde(rename = "Attributes")]
    attributes: Option<HashMap<String, String>>,
}

fn map_raw_event(raw: RawEvent) -> DockerEvent {
    let actor = raw.actor;
    let mut attributes: Vec<(String, String)> = actor
        .as_ref()
        .and_then(|a| a.attributes.clone())
        .map(|m| m.into_iter().collect())
        .unwrap_or_default();
    // Deterministic ordering for tests and stable UI presentation.
    attributes.sort();
    DockerEvent {
        event_type: raw.event_type.to_lowercase(),
        action: raw.action,
        actor_id: actor.as_ref().and_then(|a| a.id.clone()),
        actor_attributes: attributes,
        time: raw
            .time
            .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0)),
    }
}

/// A connected HTTP reader over the Docker socket that yields JSON lines.
struct SocketLines {
    reader: BufReader<Box<dyn AsyncRead + Unpin + Send>>,
    /// Bytes remaining in the current chunk (chunked transfer encoding).
    chunk_remaining: usize,
    /// True once the response status line and headers were consumed.
    headers_done: bool,
    /// True once the terminal chunk was seen.
    done: bool,
}

impl SocketLines {
    async fn read_line(&mut self) -> io::Result<Option<String>> {
        let mut line = String::new();
        loop {
            if self.done {
                return Ok(None);
            }
            if !self.headers_done {
                // Status line + headers end with an empty line.
                if !self.consume_headers().await? {
                    return Ok(None);
                }
                continue;
            }
            if self.chunk_remaining == 0 {
                // Next chunk header: "<hex-size>[;extensions]\r\n".
                let mut size_line = String::new();
                let n = self.reader.read_line(&mut size_line).await?;
                if n == 0 {
                    return Ok(None);
                }
                let size_str = size_line.trim_end();
                let size_str = size_str.split(';').next().unwrap_or("").trim();
                let size = usize::from_str_radix(size_str, 16)
                    .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "bad chunk size"))?;
                if size == 0 {
                    self.done = true;
                    // Drain the trailing CRLF of the terminal chunk.
                    let mut trailing = String::new();
                    let _ = self.reader.read_line(&mut trailing).await;
                    return Ok(None);
                }
                self.chunk_remaining = size;
                continue;
            }
            // Read one byte inside the current chunk.
            use tokio::io::AsyncReadExt as _;
            let mut byte = [0u8; 1];
            let n = self.reader.read(&mut byte).await?;
            if n == 0 {
                return Ok(None);
            }
            self.chunk_remaining -= 1;
            if byte[0] == b'\n' {
                // Strip the trailing CR (if any) of the JSON line.
                if line.ends_with('\r') {
                    line.pop();
                }
                if self.chunk_remaining == 0 {
                    // Chunk data is followed by CRLF; drain it before the
                    // next chunk header.
                    let mut trailing = [0u8; 2];
                    self.reader.read_exact(&mut trailing).await?;
                }
                return Ok(Some(line));
            }
            line.push(byte[0] as char);
            if self.chunk_remaining == 0 {
                // Chunk data is followed by CRLF; drain it before the
                // next chunk header.
                let mut trailing = [0u8; 2];
                self.reader.read_exact(&mut trailing).await?;
            }
        }
    }

    async fn consume_headers(&mut self) -> io::Result<bool> {
        let mut line = String::new();
        let n = self.reader.read_line(&mut line).await?;
        if n == 0 {
            return Ok(false);
        }
        if !line.starts_with("HTTP/1.1 200") && !line.starts_with("HTTP/1.0 200") {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unexpected status: {}", line.trim_end()),
            ));
        }
        loop {
            let mut header = String::new();
            let n = self.reader.read_line(&mut header).await?;
            if n == 0 {
                return Ok(false);
            }
            if header == "\r\n" || header == "\n" {
                self.headers_done = true;
                return Ok(true);
            }
        }
    }
}

/// Connect to the daemon and send a raw `GET /events` request.
async fn open_events(client: &DockerClient) -> Result<SocketLines, DockerError> {
    let timeout = client.config().connect_timeout;
    let request = b"GET /events HTTP/1.1\r\nHost: localhost\r\n\r\n";

    // Build the concrete stream, write the request, then wrap in a reader.
    enum Conn {
        Unix(UnixStream),
        Tcp(TcpStream),
    }
    let conn = if let Some(path) = client.socket_path() {
        tracing::debug!(path = %path.display(), "events connecting via unix socket");
        let stream = tokio::time::timeout(timeout, UnixStream::connect(path))
            .await
            .map_err(|_| DockerError::ConnectionTimeout)?
            .map_err(|e| {
                tracing::debug!(path = %path.display(), error = %e, "events socket connect failed");
                DockerError::EngineUnavailable
            })?;
        Conn::Unix(stream)
    } else if let Some(host) = client.config().host.as_deref() {
        // Remote TCP/HTTP host: parse host:port from tcp:// or http://.
        let trimmed = host
            .trim_start_matches("tcp://")
            .trim_start_matches("http://")
            .trim_start_matches("https://");
        let addr = if let Some(rest) = trimmed.split_once(':') {
            let port = rest.1.trim_end_matches('/');
            if let Ok(port) = port.parse::<u16>() {
                format!("{}:{}", rest.0, port)
            } else {
                format!("{}:2375", rest.0)
            }
        } else {
            format!("{}:2375", trimmed)
        };
        let stream = tokio::time::timeout(timeout, TcpStream::connect(&addr))
            .await
            .map_err(|_| DockerError::ConnectionTimeout)?
            .map_err(|e| {
                tracing::debug!(addr = %addr, error = %e, "events tcp connect failed");
                DockerError::EngineUnavailable
            })?;
        Conn::Tcp(stream)
    } else {
        return Err(DockerError::EngineUnavailable);
    };

    // Write the request on the concrete stream before boxing into a reader.
    use tokio::io::AsyncWriteExt as _;
    let conn: Box<dyn AsyncRead + Unpin + Send> = match conn {
        Conn::Unix(mut stream) => {
            stream.write_all(request).await.map_err(|e| {
                tracing::debug!(error = %e, "events request write failed");
                DockerError::EngineUnavailable
            })?;
            stream
                .flush()
                .await
                .map_err(|_| DockerError::EngineUnavailable)?;
            Box::new(stream)
        }
        Conn::Tcp(mut stream) => {
            stream.write_all(request).await.map_err(|e| {
                tracing::debug!(error = %e, "events request write failed");
                DockerError::EngineUnavailable
            })?;
            stream
                .flush()
                .await
                .map_err(|_| DockerError::EngineUnavailable)?;
            Box::new(stream)
        }
    };

    Ok(SocketLines {
        reader: BufReader::new(conn),
        chunk_remaining: 0,
        headers_done: false,
        done: false,
    })
}

/// Event stream service.
#[derive(Clone)]
pub struct EventStream {
    client: Arc<DockerClient>,
}

impl EventStream {
    pub fn new(client: Arc<DockerClient>) -> Self {
        Self { client }
    }

    /// Stream Docker engine events directly over the socket.
    ///
    /// The stream ends when the token is cancelled, the connection drops,
    /// or the terminal chunk arrives.
    pub fn watch_events(&self, cancel: CancellationToken) -> EventStreamResult {
        let client = self.client.clone();
        let (tx, rx) = tokio::sync::mpsc::channel::<Result<DockerEvent, DockerError>>(128);
        tokio::spawn(async move {
            tracing::debug!("events stream task started");
            let mut lines = match open_events(&client).await {
                Ok(lines) => lines,
                Err(e) => {
                    tracing::debug!(error = %e, "events open failed");
                    let _ = tx.send(Err(e)).await;
                    return;
                }
            };
            loop {
                if cancel.is_cancelled() {
                    break;
                }
                let line = match lines.read_line().await {
                    Ok(Some(line)) => line,
                    Ok(None) => {
                        tracing::debug!("events stream ended");
                        break;
                    }
                    Err(e) => {
                        let _ = tx.send(Err(DockerError::EngineUnavailable)).await;
                        tracing::debug!(error = %e, "events read failed");
                        break;
                    }
                };
                if line.trim().is_empty() {
                    continue;
                }
                match serde_json::from_str::<RawEvent>(&line) {
                    Ok(raw) => {
                        if tx.send(Ok(map_raw_event(raw))).await.is_err() {
                            break; // consumer dropped
                        }
                    }
                    Err(e) => {
                        tracing::debug!(error = %e, "events line did not parse");
                    }
                }
            }
        });
        Box::pin(ReceiverStream::new(rx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_raw_event_fields() {
        let raw = RawEvent {
            event_type: "container".into(),
            action: "start".into(),
            actor: Some(RawActor {
                id: Some("abc123".into()),
                attributes: Some(HashMap::from([
                    ("name".into(), "web".into()),
                    ("image".into(), "nginx:latest".into()),
                ])),
            }),
            time: Some(1_700_000_000),
        };
        let mapped = map_raw_event(raw);
        assert_eq!(mapped.event_type, "container");
        assert_eq!(mapped.action, "start");
        assert_eq!(mapped.actor_id.as_deref(), Some("abc123"));
        assert_eq!(mapped.time.unwrap().timestamp(), 1_700_000_000);
        // Attributes are sorted deterministically.
        assert_eq!(
            mapped.actor_attributes,
            vec![
                ("image".to_string(), "nginx:latest".to_string()),
                ("name".to_string(), "web".to_string()),
            ]
        );
    }

    #[test]
    fn lowercases_event_type() {
        let raw = RawEvent {
            event_type: "Network".into(),
            action: "create".into(),
            actor: None,
            time: None,
        };
        assert_eq!(map_raw_event(raw).event_type, "network");
    }

    #[tokio::test]
    async fn socket_lines_decodes_chunked_json_lines() {
        // Simulate a daemon response: status, headers, then two chunks
        // containing one JSON line each, and the terminal chunk. Chunk
        // sizes are computed so the fixture matches real framing.
        let line1 = "{\"Type\":\"image\",\"Action\":\"pull\"}\n";
        let line2 = "{\"Type\":\"volume\",\"Action\":\"create\"}\n";
        let payload = format!(
            "HTTP/1.1 200 OK\r\n\
             Content-Type: application/jsonl\r\n\
             Transfer-Encoding: chunked\r\n\
             \r\n\
             {:x}\r\n{}\r\n\
             {:x}\r\n{}\r\n\
             0\r\n\r\n",
            line1.len(),
            line1,
            line2.len(),
            line2,
        );
        let (mut writer, reader) = tokio::io::duplex(1024);
        let reader: BufReader<Box<dyn AsyncRead + Unpin + Send>> = BufReader::new(Box::new(reader));
        let mut lines = SocketLines {
            reader,
            chunk_remaining: 0,
            headers_done: false,
            done: false,
        };
        // Feed the payload through the duplex writer.
        use tokio::io::AsyncWriteExt as _;
        writer.write_all(payload.as_bytes()).await.unwrap();
        drop(writer);

        let first = lines.read_line().await.unwrap().unwrap();
        assert!(first.contains("\"Type\":\"image\""));
        let second = lines.read_line().await.unwrap().unwrap();
        assert!(second.contains("\"Type\":\"volume\""));
        assert!(lines.read_line().await.unwrap().is_none());
    }
}
