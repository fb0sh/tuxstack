//! Network lifecycle integration test requiring a reachable Docker Engine.
//!
//! Run with:
//! `cargo test -p tuxstack-docker-core --test networks -- --ignored --nocapture`

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use bollard::models::{ContainerCreateBody, EndpointSettings, NetworkingConfig};
use bollard::query_parameters::{CreateContainerOptions, RemoveImageOptions};
use futures_util::StreamExt;
use tuxstack_docker_core::services::networks::ListNetworksOptions;
use tuxstack_docker_core::{
    CreateNetworkOptions, DockerClient, DockerError, DockerServices, RemoveContainerOptions,
};

#[tokio::test]
#[ignore = "pulls busybox and requires a reachable Docker Engine"]
async fn create_inspect_attach_list_in_use_and_remove() {
    let client = Arc::new(DockerClient::connect_default().expect("docker must be reachable"));
    let services = DockerServices::new(client);
    let docker = bollard::Docker::connect_with_local_defaults().expect("docker must be reachable");
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let network_name = format!("tuxstack-network-test-{}", &suffix[..12]);
    let container_name = format!("{network_name}-container");
    let mut network_id = None;
    let mut container_id = None;
    let image_reference = "busybox:1.36";
    let image_was_present = docker.inspect_image(image_reference).await.is_ok();

    let result: Result<(), Box<dyn std::error::Error>> = async {
        let created = services
            .networks
            .create_network(CreateNetworkOptions {
                name: network_name.clone(),
                labels: BTreeMap::from([("com.tuxstack.test".into(), "network".into())]),
                ..Default::default()
            })
            .await?;
        if created.id.is_empty() {
            return Err("Docker returned an empty network ID".into());
        }
        network_id = Some(created.id.clone());

        let detail = services.networks.inspect_network(&created.id).await?;
        if detail.summary.id != created.id || detail.summary.name != network_name {
            return Err("inspect returned a different network".into());
        }
        if detail.summary.driver != "bridge" || detail.ipam.subnets.is_empty() {
            return Err("inspect did not return bridge IPAM configuration".into());
        }
        if detail
            .summary
            .labels
            .get("com.tuxstack.test")
            .map(String::as_str)
            != Some("network")
        {
            return Err("inspect did not return the network label".into());
        }

        let listed = services
            .networks
            .list_networks(&ListNetworksOptions {
                search: Some(network_name.clone()),
            })
            .await?;
        if !listed.iter().any(|network| network.id == created.id) {
            return Err("created network did not appear in the one-call list".into());
        }

        let mut pull = docker.create_image(
            Some(bollard::query_parameters::CreateImageOptions {
                from_image: Some(image_reference.to_string()),
                ..Default::default()
            }),
            None,
            None,
        );
        while let Some(update) = pull.next().await {
            update?;
        }

        let container = docker
            .create_container(
                Some(CreateContainerOptions {
                    name: Some(container_name.clone()),
                    platform: String::new(),
                }),
                ContainerCreateBody {
                    image: Some(image_reference.into()),
                    cmd: Some(vec!["sleep".into(), "60".into()]),
                    networking_config: Some(NetworkingConfig {
                        endpoints_config: Some(HashMap::from([(
                            network_name.clone(),
                            EndpointSettings::default(),
                        )])),
                    }),
                    ..Default::default()
                },
            )
            .await?;
        container_id = Some(container.id.clone());

        services.containers.start_container(&container.id).await?;

        let detail = services.networks.inspect_network(&created.id).await?;
        let endpoint = detail
            .containers
            .iter()
            .find(|endpoint| endpoint.id == container.id)
            .ok_or("attached container was absent from network inspect")?;
        if endpoint.name != container_name || endpoint.endpoint_id.is_empty() {
            return Err("network inspect did not preserve endpoint name and ID".into());
        }
        if endpoint.ipv4_address.as_deref().unwrap_or("").is_empty() {
            return Err("network inspect did not return the endpoint IPv4 address".into());
        }

        match services.networks.remove_network(&created.id).await {
            Err(DockerError::NetworkInUse(_)) => {}
            Err(error) => return Err(format!("expected NetworkInUse, got {error}").into()),
            Ok(()) => return Err("network with an attached container was removed".into()),
        }

        services
            .containers
            .remove_container(
                &container.id,
                &RemoveContainerOptions {
                    force: true,
                    remove_volumes: true,
                    remove_links: false,
                },
            )
            .await?;
        container_id = None;
        services.networks.remove_network(&created.id).await?;
        network_id = None;

        match services.networks.inspect_network(&created.id).await {
            Err(DockerError::NetworkNotFound(_)) => {}
            Err(error) => return Err(format!("expected NetworkNotFound, got {error}").into()),
            Ok(_) => return Err("removed network remained inspectable".into()),
        }
        Ok(())
    }
    .await;

    let container_cleanup = services
        .containers
        .remove_container(
            container_id.as_deref().unwrap_or(&container_name),
            &RemoveContainerOptions {
                force: true,
                remove_volumes: true,
                remove_links: false,
            },
        )
        .await;
    let network_cleanup = if let Some(id) = network_id {
        services.networks.remove_network(&id).await
    } else {
        services.networks.remove_network(&network_name).await
    };
    let image_cleanup = if image_was_present {
        Ok(())
    } else {
        docker
            .remove_image(
                image_reference,
                Some(RemoveImageOptions {
                    force: true,
                    noprune: false,
                    ..Default::default()
                }),
                None,
            )
            .await
            .map(|_| ())
            .map_err(|error| error.to_string())
    };

    if result.is_ok() {
        // Not-found cleanup results are expected after the successful path,
        // but a test-created image must be removed when it was absent before.
        if let Err(error) = image_cleanup {
            panic!("failed to restore test image state: {error}");
        }
    } else {
        if let Err(error) = &container_cleanup {
            eprintln!("container cleanup failed: {error}");
        }
        if let Err(error) = &network_cleanup {
            eprintln!("network cleanup failed: {error}");
        }
        if let Err(error) = &image_cleanup {
            eprintln!("image cleanup failed: {error}");
        }
    }

    result.expect("network integration lifecycle");
}
