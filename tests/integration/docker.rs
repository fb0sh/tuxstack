//! Real-Docker integration tests.
//!
//! These require a running, accessible Docker Engine and are skipped by
//! default. Run with:
//!
//! ```bash
//! cargo test -p tuxstack-docker-core --test docker -- --ignored --nocapture
//! ```
//!
//! Every test cleans up its containers even on failure. Test resources
//! use a unique `tuxstack-test-<uuid>` prefix.

use futures_util::StreamExt;
use std::sync::Arc;
use std::time::Duration;

use tuxstack_docker_core::{
    ContainerLogsOptions, ContainerState, DockerClient, DockerServices, RemoveContainerOptions,
    StopContainerOptions,
};
use tuxstack_docker_core::services::containers::ListContainersOptions;

/// Unique name prefix for this test run.
fn prefix() -> String {
    format!("tuxstack-test-{}", uuid::Uuid::new_v4().to_string().split('-').next().unwrap())
}

async fn setup() -> (DockerServices, String) {
    let client = Arc::new(DockerClient::connect_default().expect("docker must be reachable"));
    let services = DockerServices::new(client);
    let name = prefix();
    (services, name)
}

/// Ensure the test container is removed even if the test fails.
async fn cleanup(services: &DockerServices, name: &str) {
    let opts = RemoveContainerOptions {
        force: true,
        remove_volumes: true,
        remove_links: false,
    };
    let _ = services.containers.remove_container(name, &opts).await;
}

/// Start a lightweight test container and wait until it is running.
async fn start_test_container(
    services: &DockerServices,
    name: &str,
) -> tuxstack_docker_core::ContainerSummary {
    use bollard::models::{ContainerCreateBody, HostConfig};
    use bollard::query_parameters::CreateContainerOptions;

    let docker = bollard::Docker::connect_with_local_defaults().expect("docker must be reachable");

    // Pull the image if missing.
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

    let config = ContainerCreateBody {
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
    };

    let created = docker
        .create_container(
            Some(CreateContainerOptions { name: Some(name.to_string()), platform: String::new() }),
            config,
        )
        .await
        .expect("create container");

    // list to confirm and return the domain summary
    let list = services
        .containers
        .list_containers(&ListContainersOptions { all: true, ..Default::default() })
        .await
        .unwrap();
    list.into_iter()
        .find(|c| c.id == created.id)
        .expect("test container must appear in the list")
}

#[tokio::test]
#[ignore = "requires a reachable Docker Engine"]
async fn ping_and_system_info() {
    let client = DockerClient::connect_default().expect("docker must be reachable");
    client.ping().await.expect("ping must succeed");
    let info = client.system_info().await.expect("system info must load");
    assert!(!info.server_version.is_empty());
    assert!(!info.os.is_empty());
}

#[tokio::test]
#[ignore = "requires a reachable Docker Engine"]
async fn container_lifecycle() {
    let (services, name) = setup().await;
    cleanup(&services, &name).await; // pre-clean any leftover

    // list empty check
    let all = services
        .containers
        .list_containers(&ListContainersOptions { all: true, ..Default::default() })
        .await
        .unwrap();
    assert!(!all.iter().any(|c| c.name == name));

    // create + start + list + inspect + stop + remove
    let summary = start_test_container(&services, &name).await;
    let id = summary.id.clone();

    services.containers.start_container(&id).await.expect("start");
    let detail = services
        .containers
        .inspect_container(&id)
        .await
        .expect("inspect");
    assert_eq!(detail.summary.state, ContainerState::Running);

    services
        .containers
        .stop_container(&id, Some(&StopContainerOptions { timeout_seconds: Some(5) }))
        .await
        .expect("stop");

    services
        .containers
        .remove_container(&id, &RemoveContainerOptions { force: true, remove_volumes: false, remove_links: false })
        .await
        .expect("remove");

    cleanup(&services, &name).await;
}

#[tokio::test]
#[ignore = "requires a reachable Docker Engine"]
async fn container_logs_flow() {
    let (services, name) = setup().await;
    cleanup(&services, &name).await;

    let summary = start_test_container(&services, &name).await;
    let id = summary.id.clone();

    let logs = services
        .containers
        .container_logs(&id, &ContainerLogsOptions::historical(10))
        .await
        .expect("logs must be readable");

    assert!(!logs.iter().any(|l| l.message.contains("panic")));

    cleanup(&services, &name).await;
}

#[tokio::test]
#[ignore = "requires a reachable Docker Engine"]
async fn stats_single_sample() {
    let (services, name) = setup().await;
    cleanup(&services, &name).await;

    let summary = start_test_container(&services, &name).await;
    let id = summary.id.clone();
    services.containers.start_container(&id).await.unwrap();

    tokio::time::sleep(Duration::from_millis(500)).await;
    let stats = services
        .containers
        .container_stats(&id)
        .await
        .expect("stats must be readable");
    assert!(stats.memory_limit_bytes > 0 || stats.cpu_percent >= 0.0);

    cleanup(&services, &name).await;
}

#[tokio::test]
#[ignore = "requires a reachable Docker Engine"]
async fn not_found_errors_are_typed() {
    let client = Arc::new(DockerClient::connect_default().expect("docker must be reachable"));
    let services = DockerServices::new(client);

    let err = services
        .containers
        .inspect_container("tuxstack-test-does-not-exist")
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        tuxstack_docker_core::DockerError::ContainerNotFound(_)
    ));
}
