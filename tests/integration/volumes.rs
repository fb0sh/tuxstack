//! Real-Docker volume integration coverage.
//!
//! Run explicitly with:
//!
//! ```bash
//! cargo test -p tuxstack-docker-core --test volumes -- --ignored --nocapture
//! ```

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use bollard::models::{ContainerCreateBody, HostConfig, Mount, MountType};
use bollard::query_parameters::{
    CreateContainerOptions, CreateImageOptions, RemoveContainerOptions as RawRemoveContainerOptions,
};
use futures_util::StreamExt;
use tokio_util::sync::CancellationToken;
use tuxstack_docker_core::{
    CloneVolumeRequest, CreateVolumeRequest, DockerClient, DockerError, DockerServices,
    ExportVolumeRequest, RemoveVolumeOptions, VolumeExportCompression,
};

fn unique(resource: &str) -> String {
    format!("tuxstack-test-{resource}-{}", uuid::Uuid::new_v4().simple())
}

async fn cleanup_volume(services: &DockerServices, name: &str) -> Result<(), String> {
    let mut last_error = None;
    for _ in 0..10 {
        match services
            .volumes
            .remove_volume(name, RemoveVolumeOptions { force: true })
            .await
        {
            Ok(()) | Err(DockerError::VolumeNotFound(_)) => return Ok(()),
            Err(error) => last_error = Some(error.to_string()),
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    Err(last_error.unwrap_or_else(|| format!("failed to clean volume {name}")))
}

#[tokio::test]
#[ignore = "requires a reachable local Docker Engine and alpine:3.20"]
async fn real_volume_lifecycle_association_export_and_clone() {
    let client = Arc::new(DockerClient::connect_default().expect("Docker must be reachable"));
    let services = DockerServices::new(client);
    let docker = bollard::Docker::connect_with_local_defaults().expect("Docker must be reachable");
    let source = unique("volume");
    let target = unique("clone");
    let container = unique("container");
    let export = PathBuf::from(format!("/tmp/{}.tar.gz", unique("export")));
    let mut verifier_id = None;

    // The production implementation intentionally never pulls implicitly.
    let _pull_results = docker
        .create_image(
            Some(CreateImageOptions {
                from_image: Some("alpine:3.20".into()),
                ..Default::default()
            }),
            None,
            None,
        )
        .collect::<Vec<_>>()
        .await;

    let result: Result<(), String> = async {
        let created = services
            .volumes
            .create_volume(CreateVolumeRequest {
                name: Some(source.clone()),
                labels: [("com.tuxstack.integration".into(), "volumes".into())]
                    .into_iter()
                    .collect(),
                ..Default::default()
            })
            .await
            .map_err(|error| error.to_string())?;
        if created.summary.labels.get("com.tuxstack.integration") != Some(&"volumes".into()) {
            return Err("created label was not returned".into());
        }

        let response = docker
            .create_container(
                Some(CreateContainerOptions {
                    name: Some(container.clone()),
                    platform: String::new(),
                }),
                ContainerCreateBody {
                    image: Some("alpine:3.20".into()),
                    cmd: Some(vec![
                        "sh".into(),
                        "-c".into(),
                        "printf tuxstack-volume-data > /data/payload.txt".into(),
                    ]),
                    host_config: Some(HostConfig {
                        mounts: Some(vec![Mount {
                            typ: Some(MountType::VOLUME),
                            source: Some(source.clone()),
                            target: Some("/data".into()),
                            ..Default::default()
                        }]),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            )
            .await
            .map_err(|error| error.to_string())?;
        docker
            .start_container(&response.id, None)
            .await
            .map_err(|error| error.to_string())?;
        let wait = docker
            .wait_container(&response.id, None)
            .next()
            .await
            .ok_or_else(|| "writer container returned no status".to_string())?
            .map_err(|error| error.to_string())?;
        if wait.status_code != 0 {
            return Err(format!("writer exited with {}", wait.status_code));
        }

        // The exited container must still make the volume In Use.
        let listed = services
            .volumes
            .list_all_volumes()
            .await
            .map_err(|error| error.to_string())?;
        let listed_source = listed
            .iter()
            .find(|volume| volume.name == source)
            .ok_or_else(|| "source volume missing from list".to_string())?;
        if listed_source.used_by.len() != 1 || listed_source.used_by[0].destination != "/data" {
            return Err("stopped-container volume association is incorrect".into());
        }
        docker
            .remove_container(
                &container,
                Some(RawRemoveContainerOptions {
                    force: true,
                    v: false,
                    link: false,
                }),
            )
            .await
            .map_err(|error| error.to_string())?;
        let listed = services
            .volumes
            .list_all_volumes()
            .await
            .map_err(|error| error.to_string())?;
        if listed
            .iter()
            .find(|volume| volume.name == source)
            .is_none_or(|volume| !volume.used_by.is_empty())
        {
            return Err("volume did not become unused after container removal".into());
        }

        services
            .volumes
            .export_volume(
                ExportVolumeRequest {
                    volume_name: source.clone(),
                    destination: export.clone(),
                    compression: VolumeExportCompression::TarGzip,
                },
                CancellationToken::new(),
            )
            .await
            .map_err(|error| error.to_string())?;
        let archive = tokio::fs::read(&export)
            .await
            .map_err(|error| error.to_string())?;
        if archive.get(..2) != Some(&[0x1f, 0x8b]) {
            return Err("export is not a gzip archive".into());
        }

        let clone = services
            .volumes
            .clone_volume(
                CloneVolumeRequest {
                    source_volume: source.clone(),
                    target_name: target.clone(),
                    target_driver: None,
                    target_driver_options: BTreeMap::new(),
                    copy_labels: true,
                },
                CancellationToken::new(),
            )
            .await
            .map_err(|error| error.to_string())?;
        if clone.summary.labels.get("com.tuxstack.integration") != Some(&"volumes".into()) {
            return Err("clone did not copy labels".into());
        }

        // Verify clone content in Docker without exposing a host mountpoint.
        let verifier = unique("verify");
        let verification = docker
            .create_container(
                Some(CreateContainerOptions {
                    name: Some(verifier),
                    platform: String::new(),
                }),
                ContainerCreateBody {
                    image: Some("alpine:3.20".into()),
                    cmd: Some(vec![
                        "grep".into(),
                        "-Fx".into(),
                        "tuxstack-volume-data".into(),
                        "/data/payload.txt".into(),
                    ]),
                    host_config: Some(HostConfig {
                        mounts: Some(vec![Mount {
                            typ: Some(MountType::VOLUME),
                            source: Some(target.clone()),
                            target: Some("/data".into()),
                            read_only: Some(true),
                            ..Default::default()
                        }]),
                        auto_remove: Some(false),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            )
            .await
            .map_err(|error| error.to_string())?;
        verifier_id = Some(verification.id.clone());
        docker
            .start_container(&verification.id, None)
            .await
            .map_err(|error| error.to_string())?;
        let wait = docker
            .wait_container(&verification.id, None)
            .next()
            .await
            .ok_or_else(|| "verification container returned no status".to_string())?
            .map_err(|error| error.to_string())?;
        if wait.status_code != 0 {
            return Err("cloned payload was not present".into());
        }
        Ok(())
    }
    .await;

    if let Some(verifier_id) = verifier_id {
        let _ = docker
            .remove_container(
                &verifier_id,
                Some(RawRemoveContainerOptions {
                    force: true,
                    v: false,
                    link: false,
                }),
            )
            .await;
    }
    let _ = docker
        .remove_container(
            &container,
            Some(RawRemoveContainerOptions {
                force: true,
                v: false,
                link: false,
            }),
        )
        .await;
    let target_cleanup = cleanup_volume(&services, &target).await;
    let source_cleanup = cleanup_volume(&services, &source).await;
    let _ = tokio::fs::remove_file(&export).await;

    if let Err(error) = result {
        panic!("volume integration flow failed: {error}");
    }
    target_cleanup.expect("target volume cleanup");
    source_cleanup.expect("source volume cleanup");
}
