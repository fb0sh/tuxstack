//! Image lifecycle integration tests requiring a reachable Docker Engine.
//!
//! Run with:
//! `cargo test -p tuxstack-docker-core --test images -- --ignored --nocapture`

use std::sync::Arc;
use std::time::Duration;

use bollard::models::ContainerCreateBody;
use bollard::query_parameters::{CreateContainerOptions, TagImageOptions};
use futures_util::StreamExt;
use tuxstack_docker_core::services::images::ListImagesOptions;
use tuxstack_docker_core::{
    DockerClient, DockerError, DockerServices, PullImageOptions, RemoveContainerOptions,
    RemoveImageOptions,
};

#[tokio::test]
#[ignore = "pulls busybox and requires a reachable Docker Engine"]
async fn pull_inspect_usage_export_and_remove() {
    let client = Arc::new(DockerClient::connect_default().expect("docker must be reachable"));
    let services = DockerServices::new(client);
    let docker = bollard::Docker::connect_with_local_defaults().expect("docker must be reachable");
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let container_name = format!("tuxstack-image-test-{}", &suffix[..12]);
    let repository = format!("tuxstack-image-test-{}", &suffix[..12]);
    let local_reference = format!("{repository}:test");

    let result: Result<(), Box<dyn std::error::Error>> = async {
        let mut pull = services.images.pull_image(PullImageOptions {
            reference: "busybox:1.36".into(),
            platform: None,
            registry_auth: None,
        });
        while let Some(update) = pull.next().await {
            update?;
        }

        docker
            .tag_image(
                "busybox:1.36",
                Some(TagImageOptions {
                    repo: Some(repository.clone()),
                    tag: Some("test".into()),
                }),
            )
            .await?;

        let detail = services.images.inspect_image(&local_reference).await?;
        if !detail.summary.repo_tags.contains(&local_reference) {
            return Err("inspect did not return the local test tag".into());
        }

        docker
            .create_container(
                Some(CreateContainerOptions {
                    name: Some(container_name.clone()),
                    platform: String::new(),
                }),
                ContainerCreateBody {
                    image: Some(local_reference.clone()),
                    cmd: Some(vec!["true".into()]),
                    ..Default::default()
                },
            )
            .await?;

        let images = services
            .images
            .list_images(ListImagesOptions::default())
            .await?;
        let image = images
            .iter()
            .find(|image| image.repo_tags.contains(&local_reference))
            .ok_or("test image did not appear in list")?;
        if !image.in_use || !image.containers.iter().any(|c| c.name == container_name) {
            return Err("created container was not associated with test image".into());
        }

        services
            .containers
            .remove_container(
                &container_name,
                &RemoveContainerOptions {
                    force: true,
                    remove_volumes: true,
                    remove_links: false,
                },
            )
            .await?;

        let images = services
            .images
            .list_images(ListImagesOptions::default())
            .await?;
        let image = images
            .iter()
            .find(|image| image.repo_tags.contains(&local_reference))
            .ok_or("test image disappeared unexpectedly")?;
        if image.in_use {
            return Err("image remained in use after removing the test container".into());
        }

        let mut export = services.images.export_image(&local_reference);
        let exported_bytes = tokio::time::timeout(Duration::from_secs(60), async {
            let mut bytes = 0_u64;
            while let Some(chunk) = export.next().await {
                bytes = bytes.saturating_add(chunk?.len() as u64);
            }
            Ok::<_, DockerError>(bytes)
        })
        .await??;
        if exported_bytes == 0 {
            return Err("image export returned an empty archive".into());
        }

        let actions = services
            .images
            .remove_image(
                &local_reference,
                RemoveImageOptions {
                    force: false,
                    prune_children: false,
                },
            )
            .await?;
        if actions.is_empty() {
            return Err("Docker returned no image removal actions".into());
        }
        Ok(())
    }
    .await;

    // Best-effort cleanup runs whether the validation body succeeded or not.
    let _ = services
        .containers
        .remove_container(
            &container_name,
            &RemoveContainerOptions {
                force: true,
                remove_volumes: true,
                remove_links: false,
            },
        )
        .await;
    let _ = services
        .images
        .remove_image(
            &local_reference,
            RemoveImageOptions {
                force: true,
                prune_children: false,
            },
        )
        .await;

    result.expect("image integration lifecycle");
}

#[tokio::test]
#[ignore = "requires a reachable Docker Engine"]
async fn missing_image_is_typed() {
    let client = Arc::new(DockerClient::connect_default().expect("docker must be reachable"));
    let services = DockerServices::new(client);
    let error = services
        .images
        .inspect_image("tuxstack-test-image-does-not-exist")
        .await
        .unwrap_err();
    assert!(matches!(error, DockerError::ImageNotFound(_)));
}
