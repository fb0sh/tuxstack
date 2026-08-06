use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use tokio_util::sync::CancellationToken;
use tuxstack_docker_core::{ContainerTerminalOptions, DockerClient, DockerServices};
use tuxstack_domain::{
    ContainerLogsOptions, CreateContainerRequest, PullImageOptions, RemoveContainerOptions,
};
use tuxstack_protocol::{
    ProtocolBody, ProtocolEnvelope, Request, ServerEvent, SubscriptionEndReason,
    SubscriptionRequest, TerminalState, decode_frame, encode_frame,
};

#[test]
fn typed_stream_messages_round_trip_through_cbor() {
    let log_options = ContainerLogsOptions {
        stdout: true,
        stderr: false,
        timestamps: true,
        follow: true,
        tail: Some(37),
        since: None,
        until: None,
    };
    let envelopes = [
        ProtocolEnvelope::new(
            1,
            ProtocolBody::Subscribe(SubscriptionRequest::ContainerStats {
                container_ids: vec!["alpha".into(), "beta".into()],
            }),
        ),
        ProtocolEnvelope::new(
            2,
            ProtocolBody::Subscribe(SubscriptionRequest::ContainerLogs {
                container_ids: vec!["alpha".into(), "beta".into()],
                options: log_options,
            }),
        ),
        ProtocolEnvelope::new(
            3,
            ProtocolBody::Subscribe(SubscriptionRequest::ImagePull {
                options: PullImageOptions {
                    reference: "alpine:latest".into(),
                    platform: Some("linux/amd64".into()),
                    registry_auth: None,
                },
            }),
        ),
        ProtocolEnvelope::new(
            4,
            ProtocolBody::Subscribe(SubscriptionRequest::ContainerTerminal {
                container_id: "alpha".into(),
                rows: 24,
                cols: 80,
                shell: tuxstack_protocol::ShellSelection::Auto,
                user: None,
                workdir: None,
            }),
        ),
        ProtocolEnvelope::new(
            5,
            ProtocolBody::Request(Box::new(Request::ContainerTerminalInput {
                subscription_id: 91,
                bytes: b"printf test\r".to_vec(),
            })),
        ),
        ProtocolEnvelope::new(
            6,
            ProtocolBody::Request(Box::new(Request::ContainerTerminalResize {
                subscription_id: 91,
                rows: 48,
                cols: 160,
            })),
        ),
        ProtocolEnvelope::new(
            7,
            ProtocolBody::Request(Box::new(Request::ContainerTerminalClose {
                subscription_id: 91,
            })),
        ),
        ProtocolEnvelope::new(
            0,
            ProtocolBody::Event(ServerEvent::TerminalOutput {
                subscription_id: 91,
                bytes: vec![0, 0xff, b'X'],
            }),
        ),
        ProtocolEnvelope::new(
            0,
            ProtocolBody::Event(ServerEvent::TerminalState {
                subscription_id: 91,
                state: TerminalState::Exited { exit_code: Some(0) },
            }),
        ),
        ProtocolEnvelope::new(
            0,
            ProtocolBody::Event(ServerEvent::SubscriptionEnded {
                subscription_id: 91,
                reason: SubscriptionEndReason::Completed,
            }),
        ),
    ];

    for envelope in envelopes {
        let frame = encode_frame(&envelope).expect("encode typed IPC message");
        assert_eq!(
            decode_frame(&frame).expect("decode typed IPC message"),
            envelope
        );
    }
}

fn docker_services() -> DockerServices {
    let client = Arc::new(DockerClient::connect_default().expect("connect to Docker"));
    DockerServices::new(client)
}

async fn test_container(services: &DockerServices) -> String {
    services
        .containers
        .create_container(&CreateContainerRequest {
            name: Some(format!("tuxstack-ipc-stream-{}", uuid::Uuid::new_v4())),
            image: "busybox:latest".into(),
            command: vec![
                "sh".into(),
                "-c".into(),
                "echo tuxstack-log-ready; sleep 60".into(),
            ],
            tty: true,
            open_stdin: true,
            create_and_start: true,
            ..Default::default()
        })
        .await
        .expect("create and start stream test container")
        .id
}

async fn remove_test_container(services: &DockerServices, id: &str) {
    services
        .containers
        .remove_container(
            id,
            &RemoveContainerOptions {
                force: true,
                ..Default::default()
            },
        )
        .await
        .expect("remove stream test container");
}

#[tokio::test]
#[ignore = "requires local Docker and busybox:latest"]
async fn real_docker_stats_and_logs_stream_and_cancel() {
    let services = docker_services();
    let container_id = test_container(&services).await;

    let stats_cancel = CancellationToken::new();
    let mut stats = services
        .containers
        .watch_stats(&container_id, stats_cancel.clone());
    let first = tokio::time::timeout(Duration::from_secs(15), stats.next())
        .await
        .expect("stats stream timed out")
        .expect("stats stream ended")
        .expect("stats stream failed");
    assert!(first.cpu_percent.is_finite());
    stats_cancel.cancel();
    assert!(
        tokio::time::timeout(Duration::from_secs(2), stats.next())
            .await
            .expect("stats cancellation did not wake stream")
            .is_none()
    );

    let logs_cancel = CancellationToken::new();
    let options = ContainerLogsOptions {
        stdout: true,
        stderr: true,
        timestamps: true,
        follow: true,
        tail: Some(10),
        since: None,
        until: None,
    };
    let mut logs = services
        .containers
        .watch_logs(&container_id, &options, logs_cancel.clone());
    logs_cancel.cancel();
    assert!(
        tokio::time::timeout(Duration::from_secs(2), logs.next())
            .await
            .expect("logs cancellation did not wake stream")
            .is_none()
    );
    remove_test_container(&services, &container_id).await;
}

#[tokio::test]
#[ignore = "requires local Docker and busybox:latest"]
async fn real_docker_terminal_session_takes_output_resizes_and_closes() {
    let services = docker_services();
    let container_id = test_container(&services).await;
    let cancellation = CancellationToken::new();
    let session = services
        .container_terminal
        .connect(
            &container_id,
            ContainerTerminalOptions::default(),
            cancellation,
        )
        .await
        .expect("connect terminal");
    let mut output = session.take_output().await.expect("take terminal output");
    session.resize(30, 100).await.expect("resize terminal");
    session
        .write_input(b"printf tuxstack-ipc-test\\n\r".to_vec())
        .await
        .expect("write terminal input");
    let chunk = tokio::time::timeout(Duration::from_secs(10), output.next())
        .await
        .expect("terminal output timed out")
        .expect("terminal output ended")
        .expect("terminal output failed");
    assert!(
        !match chunk {
            tuxstack_docker_core::ContainerTerminalOutput::StdOut(bytes)
            | tuxstack_docker_core::ContainerTerminalOutput::StdErr(bytes)
            | tuxstack_docker_core::ContainerTerminalOutput::StdIn(bytes)
            | tuxstack_docker_core::ContainerTerminalOutput::Console(bytes) => bytes,
        }
        .is_empty()
    );
    session.close().await;
    remove_test_container(&services, &container_id).await;
}
