use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener as StdUnixListener;
use std::path::Path;
use std::time::Duration;

use tempfile::TempDir;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::time::timeout;
use tuxstack_client::{Client, ClientConfig, ClientError, ReconnectState, resolve_socket_path};
use tuxstack_protocol::{
    DaemonLifecycle, DaemonStatus, DockerConnectionStatus, FRAME_HEADER_SIZE, FeatureFlags,
    HandshakeRejection, HandshakeRejectionCode, MAX_FRAME_SIZE, MountState, MountStatus,
    PROTOCOL_VERSION, ProtocolBody, ProtocolEnvelope, Request, Response, ServerEvent, ServerHello,
    SubscriptionAccepted, SubscriptionRequest, decode_payload, encode_frame, validate_frame_length,
};

struct TestSocket {
    _directory: TempDir,
    listener: UnixListener,
    config: ClientConfig,
}

fn test_socket(request_timeout: Duration) -> TestSocket {
    let directory = tempfile::tempdir().unwrap();
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let control = directory.path().join("tuxstack");
    fs::create_dir(&control).unwrap();
    fs::set_permissions(&control, fs::Permissions::from_mode(0o700)).unwrap();
    let path = control.join("control.sock");
    let listener = UnixListener::bind(&path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let config = ClientConfig::from_runtime_dir(directory.path(), "integration-test")
        .unwrap()
        .with_handshake_timeout(Duration::from_secs(2))
        .with_request_timeout(request_timeout);
    TestSocket {
        _directory: directory,
        listener,
        config,
    }
}

fn server_hello() -> ServerHello {
    ServerHello {
        daemon_version: "0.3.0-test".into(),
        negotiated_protocol_version: PROTOCOL_VERSION,
        feature_flags: FeatureFlags::SUBSCRIPTIONS
            .union(FeatureFlags::REQUEST_CANCELLATION)
            .union(FeatureFlags::RESOURCE_DESCRIPTORS),
        max_frame_size: MAX_FRAME_SIZE,
    }
}

async fn accept_handshake(listener: &UnixListener) -> UnixStream {
    let (mut stream, _) = listener.accept().await.unwrap();
    let hello = read_envelope(&mut stream).await;
    assert_eq!(hello.request_id, 0);
    assert!(matches!(hello.body, ProtocolBody::Hello(_)));
    write_envelope(
        &mut stream,
        &ProtocolEnvelope::new(0, ProtocolBody::Accepted(server_hello())),
    )
    .await;
    stream
}

async fn read_envelope<R: AsyncRead + Unpin>(reader: &mut R) -> ProtocolEnvelope {
    let mut header = [0_u8; FRAME_HEADER_SIZE];
    reader.read_exact(&mut header).await.unwrap();
    let length = u32::from_be_bytes(header);
    validate_frame_length(length, MAX_FRAME_SIZE).unwrap();
    let mut payload = vec![0; length as usize];
    reader.read_exact(&mut payload).await.unwrap();
    decode_payload(&payload, length).unwrap()
}

async fn write_envelope<W: AsyncWrite + Unpin>(writer: &mut W, envelope: &ProtocolEnvelope) {
    writer
        .write_all(&encode_frame(envelope).unwrap())
        .await
        .unwrap();
}

fn status() -> DaemonStatus {
    DaemonStatus {
        daemon_version: "0.3.0-test".into(),
        lifecycle: DaemonLifecycle::Ready,
        docker: DockerConnectionStatus::Connected { daemon_id: None },
        mount: MountStatus {
            state: MountState::Mounted,
            mount_point: Some("/tmp/TuxStack/docker".into()),
            read_only: true,
        },
        uptime_seconds: 10,
    }
}

#[tokio::test]
async fn handshake_concurrent_requests_subscription_and_ping_are_typed() {
    let socket = test_socket(Duration::from_secs(2));
    let config = socket.config.clone();
    let server = tokio::spawn(async move {
        let mut stream = accept_handshake(&socket.listener).await;

        let first = read_envelope(&mut stream).await;
        let second = read_envelope(&mut stream).await;
        assert_ne!(first.request_id, second.request_id);
        assert!(matches!(first.body, ProtocolBody::Request(_)));
        assert!(matches!(second.body, ProtocolBody::Request(_)));

        // Out-of-order responses prove request-ID dispatch is concurrent.
        write_envelope(
            &mut stream,
            &ProtocolEnvelope::new(
                second.request_id,
                ProtocolBody::Response(Box::new(Response::MountStatus(status().mount))),
            ),
        )
        .await;
        write_envelope(
            &mut stream,
            &ProtocolEnvelope::new(
                first.request_id,
                ProtocolBody::Response(Box::new(Response::DaemonStatus(status()))),
            ),
        )
        .await;

        let subscribe = read_envelope(&mut stream).await;
        assert!(matches!(subscribe.body, ProtocolBody::Subscribe(_)));
        let subscription_id = 91;
        // An event may race ahead of the subscription acknowledgement.
        write_envelope(
            &mut stream,
            &ProtocolEnvelope::new(
                0,
                ProtocolBody::Event(ServerEvent::DaemonStatus {
                    subscription_id,
                    status: status(),
                }),
            ),
        )
        .await;
        write_envelope(
            &mut stream,
            &ProtocolEnvelope::new(
                subscribe.request_id,
                ProtocolBody::Subscribed(SubscriptionAccepted { subscription_id }),
            ),
        )
        .await;

        let unsubscribe = read_envelope(&mut stream).await;
        assert!(matches!(
            unsubscribe.body,
            ProtocolBody::Unsubscribe {
                subscription_id: 91
            }
        ));
        write_envelope(
            &mut stream,
            &ProtocolEnvelope::new(
                unsubscribe.request_id,
                ProtocolBody::Response(Box::new(Response::Acknowledged)),
            ),
        )
        .await;

        let ping = read_envelope(&mut stream).await;
        assert!(matches!(ping.body, ProtocolBody::Ping));
        write_envelope(
            &mut stream,
            &ProtocolEnvelope::new(ping.request_id, ProtocolBody::Pong),
        )
        .await;
    });

    let client = Client::connect(config).await.unwrap();
    let state = client.reconnect_state().borrow().clone();
    assert!(
        matches!(state, ReconnectState::Connected { .. }),
        "unexpected reconnect state: {state:?}"
    );

    let first_client = client.clone();
    let first = tokio::spawn(async move { first_client.request(Request::GetDaemonStatus).await });
    let second_client = client.clone();
    let second = tokio::spawn(async move { second_client.request(Request::GetMountStatus).await });
    assert!(matches!(
        first.await.unwrap().unwrap(),
        Response::DaemonStatus(_)
    ));
    assert!(matches!(
        second.await.unwrap().unwrap(),
        Response::MountStatus(_)
    ));

    let mut subscription = client
        .subscribe(SubscriptionRequest::DaemonStatus)
        .await
        .unwrap();
    assert_eq!(subscription.id(), 91);
    assert!(matches!(
        timeout(Duration::from_secs(1), subscription.recv())
            .await
            .unwrap(),
        Some(ServerEvent::DaemonStatus { .. })
    ));
    subscription.unsubscribe().await.unwrap();
    client.ping().await.unwrap();
    server.await.unwrap();
}

#[tokio::test]
async fn timeout_sends_cancel_for_the_original_request() {
    let socket = test_socket(Duration::from_millis(50));
    let config = socket.config.clone();
    let server = tokio::spawn(async move {
        let mut stream = accept_handshake(&socket.listener).await;
        let request = read_envelope(&mut stream).await;
        assert!(matches!(request.body, ProtocolBody::Request(_)));
        let cancel = timeout(Duration::from_secs(1), read_envelope(&mut stream))
            .await
            .unwrap();
        assert!(matches!(
            cancel.body,
            ProtocolBody::CancelRequest { request_id } if request_id == request.request_id
        ));
    });

    let client = Client::connect(config).await.unwrap();
    let error = client.request(Request::GetDaemonStatus).await.unwrap_err();
    assert!(matches!(error, ClientError::RequestTimeout { .. }));
    server.await.unwrap();
}

#[tokio::test]
async fn reconnect_replaces_a_dropped_transport_and_updates_state() {
    let socket = test_socket(Duration::from_secs(1));
    let config = socket.config.clone();
    let server = tokio::spawn(async move {
        let first = accept_handshake(&socket.listener).await;
        drop(first);
        let mut second = accept_handshake(&socket.listener).await;
        let ping = read_envelope(&mut second).await;
        write_envelope(
            &mut second,
            &ProtocolEnvelope::new(ping.request_id, ProtocolBody::Pong),
        )
        .await;
    });

    let client = Client::connect(config).await.unwrap();
    let mut states = client.reconnect_state();
    timeout(Duration::from_secs(1), async {
        loop {
            if matches!(
                &*states.borrow_and_update(),
                ReconnectState::Disconnected { .. }
            ) {
                break;
            }
            states.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
    client.reconnect(1).await.unwrap();
    assert!(matches!(
        &*client.reconnect_state().borrow(),
        ReconnectState::Connected { .. }
    ));
    client.ping().await.unwrap();
    server.await.unwrap();
}

#[tokio::test]
async fn handshake_rejection_is_exposed_without_starting_tasks() {
    let socket = test_socket(Duration::from_secs(1));
    let config = socket.config.clone();
    let server = tokio::spawn(async move {
        let (mut stream, _) = socket.listener.accept().await.unwrap();
        let _hello = read_envelope(&mut stream).await;
        write_envelope(
            &mut stream,
            &ProtocolEnvelope::new(
                0,
                ProtocolBody::Rejected(HandshakeRejection {
                    code: HandshakeRejectionCode::UnsupportedProtocol,
                    message: "upgrade required".into(),
                    daemon_version: "9.0".into(),
                    supported_protocol_versions: vec![9],
                }),
            ),
        )
        .await;
    });

    let error = match Client::connect(config).await {
        Ok(_) => panic!("rejected handshake unexpectedly connected"),
        Err(error) => error,
    };
    assert!(matches!(error, ClientError::HandshakeRejected(_)));
    server.await.unwrap();
}

#[test]
fn socket_resolution_rejects_broad_modes_and_non_sockets() {
    let directory = tempfile::tempdir().unwrap();
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o755)).unwrap();
    assert!(matches!(
        resolve_socket_path(directory.path()),
        Err(ClientError::InsecureSocketPath { .. })
    ));

    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let control = directory.path().join("tuxstack");
    fs::create_dir(&control).unwrap();
    fs::set_permissions(&control, fs::Permissions::from_mode(0o750)).unwrap();
    assert!(matches!(
        resolve_socket_path(directory.path()),
        Err(ClientError::InsecureSocketPath { .. })
    ));

    fs::set_permissions(&control, fs::Permissions::from_mode(0o700)).unwrap();
    let socket_path = control.join("control.sock");
    fs::write(&socket_path, b"not a socket").unwrap();
    assert!(matches!(
        resolve_socket_path(directory.path()),
        Err(ClientError::InsecureSocketPath { .. })
    ));

    fs::remove_file(&socket_path).unwrap();
    let listener = StdUnixListener::bind(&socket_path).unwrap();
    fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o660)).unwrap();
    assert!(matches!(
        resolve_socket_path(directory.path()),
        Err(ClientError::InsecureSocketPath { .. })
    ));
    drop(listener);
}

#[test]
fn runtime_directory_must_be_absolute() {
    let error = ClientConfig::from_runtime_dir(Path::new("relative"), "test").unwrap_err();
    assert!(matches!(error, ClientError::InsecureSocketPath { .. }));
}
