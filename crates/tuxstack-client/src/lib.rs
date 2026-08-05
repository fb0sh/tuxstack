//! Tokio client for the typed TuxStack Unix-domain control protocol.
//!
//! The client only connects to `$XDG_RUNTIME_DIR/tuxstack/control.sock` (or
//! that exact path below an explicitly supplied runtime directory for tests).
//! It validates existing directory/socket ownership and permissions before
//! connecting. Peer credential enforcement remains the server's responsibility.

use std::collections::HashMap;
use std::env;
use std::fs;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::Duration;

use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::sync::{Mutex as AsyncMutex, mpsc, oneshot, watch};
use tokio::time::{Instant, timeout, timeout_at};
use tuxstack_protocol::{
    ClientHello, FRAME_HEADER_SIZE, FeatureFlags, FrameError, HandshakeRejection, MAX_FRAME_SIZE,
    PROTOCOL_VERSION, ProtocolBody, ProtocolEnvelope, Request, RequestId, Response, ServerEvent,
    ServerHello, SubscriptionAccepted, SubscriptionId, SubscriptionRequest, decode_payload,
    encode_frame_with_limit, validate_frame_length,
};

pub const SOCKET_SUBDIRECTORY: &str = "tuxstack";
pub const SOCKET_FILENAME: &str = "control.sock";
const WRITER_QUEUE_CAPACITY: usize = 128;
const SUBSCRIPTION_QUEUE_CAPACITY: usize = 128;
const EARLY_EVENT_LIMIT: usize = 16;
const EARLY_SUBSCRIPTION_LIMIT: usize = 32;

#[derive(Debug, Clone)]
pub struct ClientConfig {
    socket_path: PathBuf,
    client_version: String,
    handshake_timeout: Duration,
    request_timeout: Duration,
    feature_flags: FeatureFlags,
}

impl ClientConfig {
    /// Resolve the standard socket from `XDG_RUNTIME_DIR`.
    pub fn from_env(client_version: impl Into<String>) -> Result<Self, ClientError> {
        let runtime = env::var_os("XDG_RUNTIME_DIR").ok_or(ClientError::MissingRuntimeDirectory)?;
        Self::from_runtime_dir(runtime, client_version)
    }

    /// Resolve the standard socket below a supplied runtime directory.
    /// This exists for controlled launchers and tests; arbitrary socket paths
    /// are deliberately not accepted.
    pub fn from_runtime_dir(
        runtime_dir: impl AsRef<Path>,
        client_version: impl Into<String>,
    ) -> Result<Self, ClientError> {
        let socket_path = resolve_socket_path(runtime_dir.as_ref())?;
        Ok(Self {
            socket_path,
            client_version: client_version.into(),
            handshake_timeout: Duration::from_secs(5),
            request_timeout: Duration::from_secs(30),
            feature_flags: FeatureFlags::SUBSCRIPTIONS
                .union(FeatureFlags::REQUEST_CANCELLATION)
                .union(FeatureFlags::RESOURCE_DESCRIPTORS),
        })
    }

    #[must_use]
    pub fn with_handshake_timeout(mut self, value: Duration) -> Self {
        self.handshake_timeout = value;
        self
    }

    #[must_use]
    pub fn with_request_timeout(mut self, value: Duration) -> Self {
        self.request_timeout = value;
        self
    }

    #[must_use]
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }
}

/// Return the only supported control socket location and validate every
/// existing component relevant to the client. The runtime directory and the
/// TuxStack child directory must be 0700; an existing socket must be 0600.
/// Every existing component must be owned by the current effective UID.
pub fn resolve_socket_path(runtime_dir: &Path) -> Result<PathBuf, ClientError> {
    if !runtime_dir.is_absolute() {
        return Err(ClientError::InsecureSocketPath {
            path: runtime_dir.to_path_buf(),
            reason: "XDG runtime directory is not absolute".into(),
        });
    }
    validate_private_directory(runtime_dir, "XDG runtime directory")?;

    let control_dir = runtime_dir.join(SOCKET_SUBDIRECTORY);
    match fs::symlink_metadata(&control_dir) {
        Ok(metadata) => validate_directory_metadata(&control_dir, &metadata, "control directory")?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(ClientError::Io(error)),
    }

    let socket_path = control_dir.join(SOCKET_FILENAME);
    match fs::symlink_metadata(&socket_path) {
        Ok(metadata) => {
            if !metadata.file_type().is_socket() {
                return Err(ClientError::InsecureSocketPath {
                    path: socket_path,
                    reason: "existing control path is not a Unix socket".into(),
                });
            }
            validate_owner_and_mode(&socket_path, &metadata, "control socket", 0o600)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(ClientError::Io(error)),
    }
    Ok(socket_path)
}

fn validate_private_directory(path: &Path, label: &str) -> Result<(), ClientError> {
    let metadata = fs::symlink_metadata(path).map_err(ClientError::Io)?;
    validate_directory_metadata(path, &metadata, label)
}

fn validate_directory_metadata(
    path: &Path,
    metadata: &fs::Metadata,
    label: &str,
) -> Result<(), ClientError> {
    if !metadata.file_type().is_dir() {
        return Err(ClientError::InsecureSocketPath {
            path: path.to_path_buf(),
            reason: format!("{label} is not a directory or is a symbolic link"),
        });
    }
    validate_owner_and_mode(path, metadata, label, 0o700)
}

fn validate_owner_and_mode(
    path: &Path,
    metadata: &fs::Metadata,
    label: &str,
    required_mode: u32,
) -> Result<(), ClientError> {
    let current_uid = unsafe { libc::geteuid() };
    if metadata.uid() != current_uid {
        return Err(ClientError::InsecureSocketPath {
            path: path.to_path_buf(),
            reason: format!(
                "{label} is owned by UID {}, expected {current_uid}",
                metadata.uid()
            ),
        });
    }
    let mode = metadata.permissions().mode() & 0o777;
    if mode != required_mode {
        return Err(ClientError::InsecureSocketPath {
            path: path.to_path_buf(),
            reason: format!(
                "{label} mode {mode:04o} does not match required mode {required_mode:04o}"
            ),
        });
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ReconnectState {
    Disconnected { reason: Option<String> },
    Connecting,
    Handshaking,
    Connected { server: ServerHello },
    Reconnecting { attempt: u32 },
    Failed { reason: String },
}

#[derive(Clone)]
pub struct Client {
    inner: Arc<Inner>,
}

impl Client {
    pub async fn connect(config: ClientConfig) -> Result<Self, ClientError> {
        let (state_tx, _) = watch::channel(ReconnectState::Disconnected { reason: None });
        let client = Self {
            inner: Arc::new(Inner {
                config,
                next_request_id: AtomicU64::new(1),
                next_generation: AtomicU64::new(1),
                connection: Mutex::new(None),
                dispatch: Mutex::new(Dispatch::default()),
                reconnect_lock: AsyncMutex::new(()),
                state_tx,
            }),
        };
        client.establish(false, 0).await?;
        Ok(client)
    }

    /// Manually reconnect after a transport failure. Callers can combine this
    /// with [`Client::reconnect_state`] to implement their preferred bounded
    /// retry/backoff policy without hidden infinite retry loops.
    pub async fn reconnect(&self, attempt: u32) -> Result<(), ClientError> {
        self.establish(true, attempt).await
    }

    #[must_use]
    pub fn reconnect_state(&self) -> watch::Receiver<ReconnectState> {
        self.inner.state_tx.subscribe()
    }

    #[must_use]
    pub fn is_connected(&self) -> bool {
        lock(&self.inner.connection).is_some()
    }

    pub async fn request(&self, request: Request) -> Result<Response, ClientError> {
        let (request_id, body) = self
            .inner
            .round_trip(
                ProtocolBody::Request(request),
                self.inner.config.request_timeout,
            )
            .await?;
        match body {
            ProtocolBody::Response(response) => Ok(response),
            other => Err(ClientError::UnexpectedMessage {
                request_id,
                expected: "Response",
                actual: body_name(&other),
            }),
        }
    }

    pub async fn ping(&self) -> Result<(), ClientError> {
        let (request_id, body) = self
            .inner
            .round_trip(ProtocolBody::Ping, self.inner.config.request_timeout)
            .await?;
        if matches!(body, ProtocolBody::Pong) {
            Ok(())
        } else {
            Err(ClientError::UnexpectedMessage {
                request_id,
                expected: "Pong",
                actual: body_name(&body),
            })
        }
    }

    pub async fn subscribe(
        &self,
        request: SubscriptionRequest,
    ) -> Result<Subscription, ClientError> {
        let (request_id, body) = self
            .inner
            .round_trip(
                ProtocolBody::Subscribe(request),
                self.inner.config.request_timeout,
            )
            .await?;
        let SubscriptionAccepted { subscription_id } = match body {
            ProtocolBody::Subscribed(accepted) => accepted,
            other => {
                return Err(ClientError::UnexpectedMessage {
                    request_id,
                    expected: "Subscribed",
                    actual: body_name(&other),
                });
            }
        };

        let (sender, receiver) = mpsc::channel(SUBSCRIPTION_QUEUE_CAPACITY);
        let early = {
            let mut dispatch = lock(&self.inner.dispatch);
            dispatch
                .subscriptions
                .insert(subscription_id, sender.clone());
            dispatch
                .early_events
                .remove(&subscription_id)
                .unwrap_or_default()
        };
        for event in early {
            if sender.send(event).await.is_err() {
                break;
            }
        }
        Ok(Subscription {
            id: subscription_id,
            receiver,
            inner: Arc::downgrade(&self.inner),
            active: true,
        })
    }

    async fn establish(&self, reconnecting: bool, attempt: u32) -> Result<(), ClientError> {
        let _guard = self.inner.reconnect_lock.lock().await;
        if reconnecting {
            self.inner
                .state_tx
                .send_replace(ReconnectState::Reconnecting { attempt });
        } else {
            self.inner.state_tx.send_replace(ReconnectState::Connecting);
        }

        if let Err(error) = validate_existing_socket(&self.inner.config.socket_path) {
            self.inner.fail_state(&error);
            return Err(error);
        }
        let stream = match timeout(
            self.inner.config.handshake_timeout,
            UnixStream::connect(&self.inner.config.socket_path),
        )
        .await
        {
            Ok(Ok(stream)) => stream,
            Ok(Err(error)) => {
                let error = ClientError::Io(error);
                self.inner.fail_state(&error);
                return Err(error);
            }
            Err(_) => {
                let error = ClientError::HandshakeTimeout;
                self.inner.fail_state(&error);
                return Err(error);
            }
        };

        self.inner
            .state_tx
            .send_replace(ReconnectState::Handshaking);
        let (stream, server) = match timeout(
            self.inner.config.handshake_timeout,
            perform_handshake(stream, &self.inner.config),
        )
        .await
        {
            Ok(Ok(value)) => value,
            Ok(Err(error)) => {
                self.inner.fail_state(&error);
                return Err(error);
            }
            Err(_) => {
                let error = ClientError::HandshakeTimeout;
                self.inner.fail_state(&error);
                return Err(error);
            }
        };

        self.inner.replace_connection(stream, server.clone());
        self.inner
            .state_tx
            .send_replace(ReconnectState::Connected { server });
        Ok(())
    }
}

pub struct Subscription {
    id: SubscriptionId,
    receiver: mpsc::Receiver<ServerEvent>,
    inner: Weak<Inner>,
    active: bool,
}

impl Subscription {
    #[must_use]
    pub const fn id(&self) -> SubscriptionId {
        self.id
    }

    pub async fn recv(&mut self) -> Option<ServerEvent> {
        self.receiver.recv().await
    }

    pub async fn unsubscribe(mut self) -> Result<(), ClientError> {
        self.active = false;
        let Some(inner) = self.inner.upgrade() else {
            return Err(ClientError::Disconnected("client was dropped".into()));
        };
        lock(&inner.dispatch).subscriptions.remove(&self.id);
        let (request_id, body) = inner
            .round_trip(
                ProtocolBody::Unsubscribe {
                    subscription_id: self.id,
                },
                inner.config.request_timeout,
            )
            .await?;
        match body {
            ProtocolBody::Response(Response::Acknowledged) => Ok(()),
            other => Err(ClientError::UnexpectedMessage {
                request_id,
                expected: "Response::Acknowledged",
                actual: body_name(&other),
            }),
        }
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        let Some(inner) = self.inner.upgrade() else {
            return;
        };
        lock(&inner.dispatch).subscriptions.remove(&self.id);
        let request_id = inner.next_request_id();
        let envelope = ProtocolEnvelope::new(
            request_id,
            ProtocolBody::Unsubscribe {
                subscription_id: self.id,
            },
        );
        if let Some(connection) = lock(&inner.connection).as_ref() {
            let _ = connection.writer.try_send(envelope);
        }
    }
}

struct Inner {
    config: ClientConfig,
    next_request_id: AtomicU64,
    next_generation: AtomicU64,
    connection: Mutex<Option<Connection>>,
    dispatch: Mutex<Dispatch>,
    reconnect_lock: AsyncMutex<()>,
    state_tx: watch::Sender<ReconnectState>,
}

struct Connection {
    generation: u64,
    writer: mpsc::Sender<ProtocolEnvelope>,
}

#[derive(Default)]
struct Dispatch {
    pending: HashMap<RequestId, oneshot::Sender<Result<ProtocolBody, DispatchFailure>>>,
    subscriptions: HashMap<SubscriptionId, mpsc::Sender<ServerEvent>>,
    early_events: HashMap<SubscriptionId, Vec<ServerEvent>>,
}

#[derive(Debug, Clone)]
struct DispatchFailure(String);

impl Inner {
    fn replace_connection(self: &Arc<Self>, stream: UnixStream, server: ServerHello) {
        self.terminate_current("connection replaced".into());
        let generation = self.next_generation.fetch_add(1, Ordering::Relaxed);
        let (reader, writer) = stream.into_split();
        let (writer_tx, writer_rx) = mpsc::channel(WRITER_QUEUE_CAPACITY);
        *lock(&self.connection) = Some(Connection {
            generation,
            writer: writer_tx,
        });
        let weak = Arc::downgrade(self);
        tokio::spawn(writer_task(
            writer,
            writer_rx,
            weak.clone(),
            generation,
            server.max_frame_size,
        ));
        tokio::spawn(reader_task(reader, weak, generation, server.max_frame_size));
    }

    async fn round_trip(
        self: &Arc<Self>,
        body: ProtocolBody,
        duration: Duration,
    ) -> Result<(RequestId, ProtocolBody), ClientError> {
        let request_id = self.next_request_id();
        let (sender, receiver) = oneshot::channel();
        lock(&self.dispatch).pending.insert(request_id, sender);
        let mut pending_guard = PendingGuard {
            inner: Arc::downgrade(self),
            request_id,
            active: true,
        };

        let writer = lock(&self.connection)
            .as_ref()
            .map(|connection| connection.writer.clone())
            .ok_or_else(|| ClientError::Disconnected("no active connection".into()))?;
        let deadline = Instant::now() + duration;
        let result = match timeout_at(
            deadline,
            writer.send(ProtocolEnvelope::new(request_id, body)),
        )
        .await
        {
            Ok(Ok(())) => match timeout_at(deadline, receiver).await {
                Ok(Ok(Ok(body))) => Ok((request_id, body)),
                Ok(Ok(Err(error))) => Err(ClientError::Disconnected(error.0)),
                Ok(Err(_)) => Err(ClientError::Disconnected(
                    "response dispatcher stopped".into(),
                )),
                Err(_) => Err(ClientError::RequestTimeout { request_id }),
            },
            Ok(Err(_)) => Err(ClientError::Disconnected("writer task stopped".into())),
            Err(_) => Err(ClientError::RequestTimeout { request_id }),
        };
        if matches!(result, Err(ClientError::RequestTimeout { .. })) {
            self.send_cancel(request_id);
        }
        lock(&self.dispatch).pending.remove(&request_id);
        pending_guard.active = false;
        result
    }

    fn next_request_id(&self) -> RequestId {
        loop {
            let id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
            if id != 0 {
                return id;
            }
        }
    }

    fn send_cancel(&self, target: RequestId) {
        let cancel_id = self.next_request_id();
        let envelope = ProtocolEnvelope::new(
            cancel_id,
            ProtocolBody::CancelRequest { request_id: target },
        );
        if let Some(connection) = lock(&self.connection).as_ref() {
            let _ = connection.writer.try_send(envelope);
        }
    }

    fn terminate_generation(&self, generation: u64, reason: String) {
        let is_current = lock(&self.connection)
            .as_ref()
            .is_some_and(|connection| connection.generation == generation);
        if !is_current {
            return;
        }
        self.terminate_current(reason);
    }

    fn terminate_current(&self, reason: String) {
        lock(&self.connection).take();
        let failure = DispatchFailure(reason.clone());
        let mut dispatch = lock(&self.dispatch);
        for (_, sender) in dispatch.pending.drain() {
            let _ = sender.send(Err(failure.clone()));
        }
        dispatch.subscriptions.clear();
        dispatch.early_events.clear();
        drop(dispatch);
        self.state_tx.send_replace(ReconnectState::Disconnected {
            reason: Some(reason),
        });
    }

    fn fail_state(&self, error: &ClientError) {
        self.state_tx.send_replace(ReconnectState::Failed {
            reason: error.to_string(),
        });
    }
}

struct PendingGuard {
    inner: Weak<Inner>,
    request_id: RequestId,
    active: bool,
}

impl Drop for PendingGuard {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        if let Some(inner) = self.inner.upgrade() {
            lock(&inner.dispatch).pending.remove(&self.request_id);
            inner.send_cancel(self.request_id);
        }
    }
}

async fn perform_handshake(
    mut stream: UnixStream,
    config: &ClientConfig,
) -> Result<(UnixStream, ServerHello), ClientError> {
    let hello = ClientHello {
        client_version: config.client_version.clone(),
        supported_protocol_versions: vec![PROTOCOL_VERSION],
        feature_flags: config.feature_flags,
        max_frame_size: MAX_FRAME_SIZE,
    };
    write_envelope(
        &mut stream,
        &ProtocolEnvelope::new(0, ProtocolBody::Hello(hello)),
        MAX_FRAME_SIZE,
    )
    .await?;
    let envelope = read_envelope(&mut stream, MAX_FRAME_SIZE).await?;
    if envelope.request_id != 0 {
        return Err(ClientError::HandshakeProtocol(
            "handshake response request ID must be zero".into(),
        ));
    }
    match envelope.body {
        ProtocolBody::Accepted(server) => {
            if server.negotiated_protocol_version != PROTOCOL_VERSION {
                return Err(ClientError::HandshakeProtocol(format!(
                    "server negotiated unsupported protocol {}",
                    server.negotiated_protocol_version
                )));
            }
            validate_frame_length(server.max_frame_size, MAX_FRAME_SIZE)?;
            Ok((stream, server))
        }
        ProtocolBody::Rejected(rejection) => Err(ClientError::HandshakeRejected(rejection)),
        other => Err(ClientError::HandshakeProtocol(format!(
            "expected Accepted or Rejected, received {}",
            body_name(&other)
        ))),
    }
}

async fn writer_task<W>(
    mut writer: W,
    mut receiver: mpsc::Receiver<ProtocolEnvelope>,
    inner: Weak<Inner>,
    generation: u64,
    maximum: u32,
) where
    W: AsyncWrite + Unpin,
{
    while let Some(envelope) = receiver.recv().await {
        if let Err(error) = write_envelope(&mut writer, &envelope, maximum).await {
            if let Some(inner) = inner.upgrade() {
                inner.terminate_generation(generation, error.to_string());
            }
            return;
        }
    }
}

async fn reader_task<R>(mut reader: R, inner: Weak<Inner>, generation: u64, maximum: u32)
where
    R: AsyncRead + Unpin,
{
    loop {
        let envelope = match read_envelope(&mut reader, maximum).await {
            Ok(envelope) => envelope,
            Err(error) => {
                if let Some(inner) = inner.upgrade() {
                    inner.terminate_generation(generation, error.to_string());
                }
                return;
            }
        };
        let Some(inner) = inner.upgrade() else {
            return;
        };
        match envelope.body {
            ProtocolBody::Response(response) => {
                dispatch_pending(
                    &inner,
                    envelope.request_id,
                    ProtocolBody::Response(response),
                );
            }
            ProtocolBody::Subscribed(accepted) => {
                dispatch_pending(
                    &inner,
                    envelope.request_id,
                    ProtocolBody::Subscribed(accepted),
                );
            }
            ProtocolBody::Pong => {
                dispatch_pending(&inner, envelope.request_id, ProtocolBody::Pong);
            }
            ProtocolBody::Rejected(rejection) => {
                let reason = format!("server rejected request: {}", rejection.message);
                if let Some(sender) = lock(&inner.dispatch).pending.remove(&envelope.request_id) {
                    let _ = sender.send(Err(DispatchFailure(reason)));
                }
            }
            ProtocolBody::Event(event) => dispatch_event(&inner, event),
            _ => {
                let reason = format!(
                    "unexpected server message {} for request {}",
                    body_name(&envelope.body),
                    envelope.request_id
                );
                inner.terminate_generation(generation, reason);
                return;
            }
        }
    }
}

fn dispatch_pending(inner: &Inner, request_id: RequestId, body: ProtocolBody) {
    if let Some(sender) = lock(&inner.dispatch).pending.remove(&request_id) {
        let _ = sender.send(Ok(body));
    }
}

fn dispatch_event(inner: &Inner, event: ServerEvent) {
    let subscription_id = event.subscription_id();
    let mut dispatch = lock(&inner.dispatch);
    if let Some(sender) = dispatch.subscriptions.get(&subscription_id) {
        let _ = sender.try_send(event);
    } else if dispatch.early_events.contains_key(&subscription_id)
        || dispatch.early_events.len() < EARLY_SUBSCRIPTION_LIMIT
    {
        let events = dispatch.early_events.entry(subscription_id).or_default();
        if events.len() < EARLY_EVENT_LIMIT {
            events.push(event);
        }
    }
}

async fn write_envelope<W: AsyncWrite + Unpin>(
    writer: &mut W,
    envelope: &ProtocolEnvelope,
    maximum: u32,
) -> Result<(), ClientError> {
    let frame = encode_frame_with_limit(envelope, maximum)?;
    writer.write_all(&frame).await.map_err(ClientError::Io)
}

async fn read_envelope<R: AsyncRead + Unpin>(
    reader: &mut R,
    maximum: u32,
) -> Result<ProtocolEnvelope, ClientError> {
    let mut header = [0_u8; FRAME_HEADER_SIZE];
    read_exact_async(reader, &mut header, true).await?;
    let length = u32::from_be_bytes(header);
    validate_frame_length(length, maximum)?;
    let mut payload = vec![0_u8; length as usize];
    read_exact_async(reader, &mut payload, false).await?;
    decode_payload(&payload, length).map_err(ClientError::Frame)
}

async fn read_exact_async<R: AsyncRead + Unpin>(
    reader: &mut R,
    buffer: &mut [u8],
    header: bool,
) -> Result<(), ClientError> {
    let mut received = 0;
    while received < buffer.len() {
        match reader.read(&mut buffer[received..]).await {
            Ok(0) => {
                let error = if header {
                    FrameError::TruncatedHeader { actual: received }
                } else {
                    FrameError::TruncatedBody {
                        expected: buffer.len() as u32,
                        actual: received,
                    }
                };
                return Err(ClientError::Frame(error));
            }
            Ok(count) => received += count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(ClientError::Io(error)),
        }
    }
    Ok(())
}

fn validate_existing_socket(path: &Path) -> Result<(), ClientError> {
    let runtime =
        path.parent()
            .and_then(Path::parent)
            .ok_or_else(|| ClientError::InsecureSocketPath {
                path: path.to_path_buf(),
                reason: "socket is not below a runtime directory".into(),
            })?;
    let resolved = resolve_socket_path(runtime)?;
    if resolved != path {
        return Err(ClientError::InsecureSocketPath {
            path: path.to_path_buf(),
            reason: "socket path is not the standard control socket".into(),
        });
    }
    if !path.exists() {
        return Err(ClientError::SocketUnavailable(path.to_path_buf()));
    }
    Ok(())
}

fn body_name(body: &ProtocolBody) -> &'static str {
    match body {
        ProtocolBody::Hello(_) => "Hello",
        ProtocolBody::Accepted(_) => "Accepted",
        ProtocolBody::Rejected(_) => "Rejected",
        ProtocolBody::Request(_) => "Request",
        ProtocolBody::Response(_) => "Response",
        ProtocolBody::Subscribe(_) => "Subscribe",
        ProtocolBody::Subscribed(_) => "Subscribed",
        ProtocolBody::Unsubscribe { .. } => "Unsubscribe",
        ProtocolBody::Event(_) => "Event",
        ProtocolBody::CancelRequest { .. } => "CancelRequest",
        ProtocolBody::Ping => "Ping",
        ProtocolBody::Pong => "Pong",
        _ => "Unknown",
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("XDG_RUNTIME_DIR is not set")]
    MissingRuntimeDirectory,
    #[error("insecure socket path {}: {reason}", path.display())]
    InsecureSocketPath { path: PathBuf, reason: String },
    #[error("TuxStack control socket is unavailable at {}", .0.display())]
    SocketUnavailable(PathBuf),
    #[error("daemon handshake timed out")]
    HandshakeTimeout,
    #[error("daemon rejected handshake: {0:?}")]
    HandshakeRejected(HandshakeRejection),
    #[error("invalid handshake: {0}")]
    HandshakeProtocol(String),
    #[error("request {request_id} timed out")]
    RequestTimeout { request_id: RequestId },
    #[error("connection closed: {0}")]
    Disconnected(String),
    #[error("request {request_id} expected {expected}, received {actual}")]
    UnexpectedMessage {
        request_id: RequestId,
        expected: &'static str,
        actual: &'static str,
    },
    #[error(transparent)]
    Frame(#[from] FrameError),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
