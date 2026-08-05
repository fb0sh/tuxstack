//! Container-focused integration tests (require a reachable Docker Engine).
//!
//! Run with:
//!
//! ```bash
//! cargo test -p tuxstack-docker-core --test containers -- --ignored --nocapture
//! ```

use futures_util::StreamExt;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

use tuxstack_docker_core::services::containers::ListContainersOptions;
use tuxstack_docker_core::streams::events::EventStream;
use tuxstack_docker_core::{
    ContainerDirectoryQuery, ContainerPortProtocol, ContainerState, ContainerTerminalOptions,
    ContainerTerminalOutput, CreateContainerPort, CreateContainerRequest, DockerClient,
    DockerServices, RemoveContainerOptions, StopContainerOptions,
};

fn prefix() -> String {
    format!(
        "tuxstack-test-{}",
        uuid::Uuid::new_v4().to_string().split('-').next().unwrap()
    )
}

async fn setup() -> (DockerServices, String) {
    let client = Arc::new(DockerClient::connect_default().expect("docker must be reachable"));
    let services = DockerServices::new(client);
    let name = prefix();
    (services, name)
}

async fn cleanup(services: &DockerServices, name: &str) {
    let opts = RemoveContainerOptions {
        force: true,
        remove_volumes: true,
        remove_links: false,
    };
    let _ = services.containers.remove_container(name, &opts).await;
}

async fn create_test_container(_services: &DockerServices, name: &str) -> String {
    use bollard::models::{ContainerCreateBody, HostConfig};
    use bollard::query_parameters::CreateContainerOptions;

    let docker = bollard::Docker::connect_with_local_defaults().expect("docker must be reachable");
    let _ = docker
        .create_image(
            Some(bollard::query_parameters::CreateImageOptions {
                from_image: Some("busybox:latest".to_string()),
                ..Default::default()
            }),
            None,
            None,
        )
        .collect::<Vec<_>>()
        .await;

    let created = docker
        .create_container(
            Some(CreateContainerOptions {
                name: Some(name.to_string()),
                platform: String::new(),
            }),
            ContainerCreateBody {
                image: Some("busybox:latest".to_string()),
                cmd: Some(vec![
                    "sh".to_string(),
                    "-c".to_string(),
                    "mkdir -p /tuxstack-fixture; printf 'snapshot-ok\\n' > /tuxstack-fixture/hello.txt; while true; do sleep 1; done".to_string(),
                ]),
                host_config: Some(HostConfig {
                    auto_remove: Some(false),
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .await
        .expect("create container");
    created.id
}

#[tokio::test]
#[ignore = "requires a reachable Docker Engine"]
async fn full_lifecycle_start_stop_restart_remove() {
    let (services, name) = setup().await;
    cleanup(&services, &name).await;

    let id = create_test_container(&services, &name).await;

    services
        .containers
        .start_container(&id)
        .await
        .expect("start");
    let detail = services.containers.inspect_container(&id).await.unwrap();
    assert_eq!(detail.summary.state, ContainerState::Running);

    services
        .containers
        .restart_container(&id)
        .await
        .expect("restart");
    let detail = services.containers.inspect_container(&id).await.unwrap();
    assert_eq!(detail.summary.state, ContainerState::Running);

    services
        .containers
        .stop_container(
            &id,
            Some(&StopContainerOptions {
                timeout_seconds: Some(5),
            }),
        )
        .await
        .expect("stop");
    let detail = services.containers.inspect_container(&id).await.unwrap();
    assert_eq!(detail.summary.state, ContainerState::Exited);

    // list must show it with all=true and hide it with all=false
    let all = services
        .containers
        .list_containers(&ListContainersOptions {
            all: true,
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(all.iter().any(|c| c.id == id));
    let running = services
        .containers
        .list_containers(&ListContainersOptions::default())
        .await
        .unwrap();
    assert!(!running.iter().any(|c| c.id == id));

    // search filter
    let searched = services
        .containers
        .list_containers(&ListContainersOptions {
            all: true,
            search: Some(name.clone()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(searched.iter().any(|c| c.id == id));

    // state filter
    let exited = services
        .containers
        .list_containers(&ListContainersOptions {
            all: true,
            state: Some(ContainerState::Exited),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(exited.iter().any(|c| c.id == id));

    services
        .containers
        .remove_container(
            &id,
            &RemoveContainerOptions {
                force: true,
                remove_volumes: false,
                remove_links: false,
            },
        )
        .await
        .expect("remove");

    let err = services
        .containers
        .inspect_container(&id)
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        tuxstack_docker_core::DockerError::ContainerNotFound(_)
    ));
    cleanup(&services, &name).await;
}

#[tokio::test]
#[ignore = "requires a reachable Docker Engine"]
async fn pause_unpause_roundtrip() {
    let (services, name) = setup().await;
    cleanup(&services, &name).await;

    let id = create_test_container(&services, &name).await;
    services.containers.start_container(&id).await.unwrap();

    services
        .containers
        .pause_container(&id)
        .await
        .expect("pause");
    let detail = services.containers.inspect_container(&id).await.unwrap();
    assert_eq!(detail.summary.state, ContainerState::Paused);

    services
        .containers
        .unpause_container(&id)
        .await
        .expect("unpause");
    let detail = services.containers.inspect_container(&id).await.unwrap();
    assert_eq!(detail.summary.state, ContainerState::Running);

    cleanup(&services, &name).await;
}

#[tokio::test]
#[ignore = "requires a reachable Docker Engine"]
async fn stats_and_logs_streams_cancel() {
    use futures_util::StreamExt;
    use tokio_util::sync::CancellationToken;

    let (services, name) = setup().await;
    cleanup(&services, &name).await;

    let id = create_test_container(&services, &name).await;
    services.containers.start_container(&id).await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;

    let cancel = CancellationToken::new();
    let mut stats = services.containers.watch_stats(&id, cancel.clone());
    let first = tokio::time::timeout(Duration::from_secs(10), stats.next())
        .await
        .expect("stats stream must produce a sample")
        .expect("no stats error")
        .expect("stats value");
    assert!(first.memory_limit_bytes > 0);
    drop(stats);
    cancel.cancel();

    let cancel2 = CancellationToken::new();
    let opts = tuxstack_docker_core::ContainerLogsOptions::follow();
    let mut logs = services.containers.watch_logs(&id, &opts, cancel2.clone());
    cancel2.cancel();
    // After cancellation the stream must end promptly (no leak).
    let result = tokio::time::timeout(Duration::from_secs(5), logs.next()).await;
    assert!(result.is_ok());

    cleanup(&services, &name).await;
}

#[tokio::test]
#[ignore = "requires a reachable Docker Engine"]
async fn files_snapshot_preview_and_save_are_real() {
    use tokio_util::sync::CancellationToken;

    let (services, name) = setup().await;
    cleanup(&services, &name).await;

    let id = create_test_container(&services, &name).await;
    services.containers.start_container(&id).await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;

    let snapshot = services
        .container_files
        .snapshot(&id, CancellationToken::new())
        .await
        .expect("snapshot exported rootfs");
    assert_eq!(snapshot.container_id, id);
    let page = snapshot
        .list_directory(&ContainerDirectoryQuery {
            directory: "/tuxstack-fixture".into(),
            ..Default::default()
        })
        .expect("list fixture directory");
    assert!(
        page.entries.iter().any(|entry| {
            entry.logical_path == "/tuxstack-fixture/hello.txt" && entry.size == 12
        })
    );

    let preview = services
        .container_files
        .preview_file(
            &id,
            "/tuxstack-fixture/hello.txt",
            Some(64),
            CancellationToken::new(),
        )
        .await
        .expect("preview fixture file");
    assert_eq!(preview.bytes, b"snapshot-ok\n");
    assert!(!preview.truncated);

    let destination_dir = tempfile::tempdir().unwrap();
    let destination = destination_dir.path().join("hello.txt");
    let transfer = services
        .container_files
        .save_file(
            &id,
            "/tuxstack-fixture/hello.txt",
            &destination,
            CancellationToken::new(),
        )
        .await
        .expect("save fixture file");
    assert_eq!(transfer.bytes_written, 12);
    assert_eq!(
        tokio::fs::read(&destination).await.unwrap(),
        b"snapshot-ok\n"
    );

    cleanup(&services, &name).await;
}

#[tokio::test]
#[ignore = "requires a reachable Docker Engine"]
async fn terminal_exec_is_interactive_and_resizable() {
    use tokio_util::sync::CancellationToken;

    let (services, name) = setup().await;
    cleanup(&services, &name).await;

    let id = create_test_container(&services, &name).await;
    services.containers.start_container(&id).await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;

    let session = services
        .container_terminal
        .connect(
            &id,
            ContainerTerminalOptions::default(),
            CancellationToken::new(),
        )
        .await
        .expect("connect real terminal");
    assert_eq!(session.container_id(), id);
    assert!(!session.shell().is_empty());
    session.resize(32, 100).await.expect("resize terminal");

    let mut output = session.take_output().await.expect("take output once");
    session
        .write_input(b"printf 'TUXSTACK_TERM_OK\\n'\n".to_vec())
        .await
        .expect("write terminal input");

    let observed = tokio::time::timeout(Duration::from_secs(10), async {
        let mut bytes = Vec::new();
        while let Some(chunk) = output.next().await {
            let chunk = chunk.expect("terminal output chunk");
            match chunk {
                ContainerTerminalOutput::StdOut(value)
                | ContainerTerminalOutput::StdErr(value)
                | ContainerTerminalOutput::StdIn(value)
                | ContainerTerminalOutput::Console(value) => bytes.extend(value),
            }
            if bytes
                .windows(b"TUXSTACK_TERM_OK".len())
                .any(|window| window == b"TUXSTACK_TERM_OK")
            {
                return true;
            }
        }
        false
    })
    .await
    .expect("terminal output deadline");
    assert!(observed, "interactive command output was not observed");

    session.close().await;
    cleanup(&services, &name).await;
}

#[tokio::test]
#[ignore = "requires a reachable Docker Engine"]
async fn typed_create_starts_and_publishes_daemon_assigned_port() {
    let (services, name) = setup().await;
    cleanup(&services, &name).await;

    let result = services
        .containers
        .create_container(&CreateContainerRequest {
            name: Some(name.clone()),
            image: "busybox:latest".into(),
            command: vec!["sleep".into(), "60".into()],
            ports: vec![CreateContainerPort {
                container_port: 8080,
                protocol: ContainerPortProtocol::Tcp,
                host_ip: None,
                host_port: None,
            }],
            create_and_start: true,
            ..Default::default()
        })
        .await
        .expect("typed create and start");

    assert!(result.started);
    assert!(result.start_error.is_none());
    let detail = services
        .containers
        .inspect_container(&result.id)
        .await
        .expect("inspect created container");
    let published = detail
        .summary
        .ports
        .iter()
        .find(|port| port.container_port == 8080 && port.protocol == "tcp")
        .expect("published 8080/tcp binding");
    assert!(published.host_port.is_some_and(|port| port > 0));

    cleanup(&services, &name).await;
}

#[tokio::test]
#[ignore = "requires a reachable Docker Engine"]
async fn event_stream_preserves_real_actor_and_action() {
    let (services, name) = setup().await;
    cleanup(&services, &name).await;

    let id = create_test_container(&services, &name).await;
    let cancel = CancellationToken::new();
    let mut events = EventStream::new(services.client()).watch_events(cancel.clone());
    let (sender, mut receiver) = tokio::sync::mpsc::channel(32);
    let consumer = tokio::spawn(async move {
        while let Some(event) = events.next().await {
            if sender.send(event).await.is_err() {
                break;
            }
        }
    });
    tokio::time::sleep(Duration::from_millis(150)).await;

    let new_name = format!("{name}-event-renamed");
    cleanup(&services, &new_name).await;
    services
        .containers
        .rename_container(&id, &new_name)
        .await
        .expect("rename for event");

    let observed = tokio::time::timeout(Duration::from_secs(10), async {
        while let Some(event) = receiver.recv().await {
            let event = event.expect("event stream item");
            if event.event_type == "container"
                && event.actor_id.as_deref() == Some(id.as_str())
                && event.action == "rename"
            {
                return true;
            }
        }
        false
    })
    .await
    .expect("event deadline");
    cancel.cancel();
    let _ = consumer.await;
    assert!(observed, "matching rename event was not observed");

    cleanup(&services, &new_name).await;
}

#[tokio::test]
#[ignore = "requires a reachable Docker Engine"]
async fn rename_container_works() {
    let (services, name) = setup().await;
    cleanup(&services, &name).await;

    let id = create_test_container(&services, &name).await;
    let new_name = format!("{name}-renamed");
    cleanup(&services, &new_name).await;

    services
        .containers
        .rename_container(&id, &new_name)
        .await
        .expect("rename");

    let detail = services
        .containers
        .inspect_container(&new_name)
        .await
        .unwrap();
    assert_eq!(detail.summary.name, new_name);

    cleanup(&services, &new_name).await;
}
