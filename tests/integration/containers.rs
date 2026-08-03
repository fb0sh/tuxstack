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

use tuxstack_docker_core::services::containers::ListContainersOptions;
use tuxstack_docker_core::{
    ContainerState, DockerClient, DockerServices, RemoveContainerOptions, StopContainerOptions,
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
                    "while true; do sleep 1; done".to_string(),
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
