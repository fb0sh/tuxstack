//! Real-Docker integration tests for read-only volume file browsing.
//!
//! Run with:
//! `cargo test -p tuxstack-docker-core --test volume_files -- --ignored --nocapture`

use std::sync::Arc;

use bollard::models::{ContainerCreateBody, HostConfig, Mount, MountType};
use bollard::query_parameters::{
    CreateContainerOptions, RemoveContainerOptions, WaitContainerOptions,
};
use futures_util::StreamExt;
use tokio_util::sync::CancellationToken;
use tuxstack_docker_core::{
    CreateVolumeRequest, DockerClient, DockerServices, ListVolumeDirectoryRequest,
    PreviewVolumeFileRequest, RemoveVolumeOptions, VolumeFileType, VolumePath,
};

fn unique(prefix: &str) -> String {
    format!("tuxstack-test-{prefix}-{}", uuid::Uuid::new_v4().simple())
}

async fn seed_volume(docker: &bollard::Docker, volume: &str) {
    let name = unique("seed");
    let create = docker
        .create_container(
            Some(CreateContainerOptions {
                name: Some(name.clone()),
                platform: String::new(),
            }),
            ContainerCreateBody {
                image: Some("alpine:3.20".into()),
                cmd: Some(vec![
                    "sh".into(),
                    "-c".into(),
                    r#"
set -eu
mkdir -p /data/subdir
printf 'hello volume files\n' > /data/readme.txt
printf '{"a":1,"b":[2,3]}\n' > /data/config.json
printf 'secret\n' > /data/.hidden
ln -s readme.txt /data/link-in
ln -s /etc/passwd /data/link-out
dd if=/dev/zero of=/data/binary.bin bs=1024 count=4 2>/dev/null
"#
                    .into(),
                ]),
                host_config: Some(HostConfig {
                    mounts: Some(vec![Mount {
                        typ: Some(MountType::VOLUME),
                        source: Some(volume.into()),
                        target: Some("/data".into()),
                        ..Default::default()
                    }]),
                    auto_remove: Some(false),
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .await
        .expect("seed create");
    docker
        .start_container(&create.id, None)
        .await
        .expect("seed start");
    let mut wait = Box::pin(docker.wait_container(
        &create.id,
        Some(WaitContainerOptions {
            condition: "not-running".into(),
        }),
    ));
    let _ = wait.next().await;
    let _ = docker
        .remove_container(
            &create.id,
            Some(RemoveContainerOptions {
                force: true,
                v: false,
                link: false,
            }),
        )
        .await;
}

#[tokio::test]
#[ignore = "requires local Docker Engine and alpine:3.20"]
async fn volume_files_list_preview_symlink_and_cleanup() {
    let client = Arc::new(DockerClient::connect_default().expect("docker connect"));
    let services = DockerServices::new(client);
    let docker = bollard::Docker::connect_with_local_defaults().expect("docker connect");
    let volume_name = unique("vfiles");

    services
        .volumes
        .create_volume(CreateVolumeRequest {
            name: Some(volume_name.clone()),
            ..Default::default()
        })
        .await
        .expect("create volume");

    let result = async {
        seed_volume(&docker, &volume_name).await;
        let token = CancellationToken::new();
        let session = services
            .volume_files
            .start_session(&volume_name, token.clone())
            .await
            .expect("start session");

        let listed = services
            .volume_files
            .list_directory(
                &session,
                &ListVolumeDirectoryRequest {
                    volume_name: volume_name.clone(),
                    path: VolumePath::root(),
                    show_hidden: true,
                },
                token.clone(),
            )
            .await
            .expect("list root");
        assert!(listed.iter().any(|e| e.name == "readme.txt"));
        assert!(listed.iter().any(|e| e.name == "subdir"));
        assert!(listed.iter().any(|e| e.name == ".hidden"));
        assert!(listed.iter().any(|e| e.name == "link-out"));

        let preview = services
            .volume_files
            .preview_file(
                &session,
                &PreviewVolumeFileRequest {
                    volume_name: volume_name.clone(),
                    path: VolumePath::parse("/readme.txt").unwrap(),
                    max_bytes: 1024 * 1024,
                },
                token.clone(),
            )
            .await
            .expect("preview text");
        match preview.content {
            tuxstack_docker_core::FilePreviewContent::Text(text) => {
                assert!(text.contains("hello volume files"));
            }
            other => panic!("expected text preview, got {other:?}"),
        }

        let json = services
            .volume_files
            .preview_file(
                &session,
                &PreviewVolumeFileRequest {
                    volume_name: volume_name.clone(),
                    path: VolumePath::parse("/config.json").unwrap(),
                    max_bytes: 1024 * 1024,
                },
                token.clone(),
            )
            .await
            .expect("preview json");
        assert_eq!(
            json.preview_kind,
            tuxstack_docker_core::FilePreviewKind::Json
        );

        let outside = services
            .volume_files
            .resolve_entry(
                &session,
                &volume_name,
                &VolumePath::parse("/link-out").unwrap(),
                token.clone(),
            )
            .await;
        assert!(matches!(
            outside,
            Err(tuxstack_docker_core::DockerError::VolumeSymlinkOutsideRoot(
                _
            ))
        ));

        let subdir = listed
            .iter()
            .find(|e| e.entry_type == VolumeFileType::Directory && e.name == "subdir")
            .expect("subdir");
        let nested = services
            .volume_files
            .list_directory(
                &session,
                &ListVolumeDirectoryRequest {
                    volume_name: volume_name.clone(),
                    path: subdir.path.clone(),
                    show_hidden: false,
                },
                token.clone(),
            )
            .await
            .expect("list subdir");
        assert!(nested.is_empty());

        services
            .volume_files
            .stop_session(session)
            .await
            .expect("stop session");
        Ok::<(), ()>(())
    }
    .await;

    let _ = services
        .volumes
        .remove_volume(&volume_name, RemoveVolumeOptions { force: true })
        .await;
    let _ = services.volume_files.cleanup_orphan_sessions().await;

    assert!(result.is_ok());
}
