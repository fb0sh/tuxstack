//! Session lifecycle: invalidation, force-removal, and orphan cleanup.

use super::error::FilesystemError;
use super::types::FilesystemSession;
use bollard::query_parameters::RemoveContainerOptions;

/// Force-remove the container backing a session. Errors are best-effort;
/// a missing container is treated as success.
pub async fn invalidate_session(
    client: &bollard::Docker,
    session: &FilesystemSession,
) -> Result<(), FilesystemError> {
    force_remove_container(client, &session.container_id).await
}

/// Force-remove a container by ID. A missing container is OK.
pub async fn force_remove_container(
    client: &bollard::Docker,
    container_id: &str,
) -> Result<(), FilesystemError> {
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        client.remove_container(
            container_id,
            Some(RemoveContainerOptions {
                force: true,
                v: false,
                link: false,
            }),
        ),
    )
    .await;

    match result {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => {
            let text = error.to_string().to_ascii_lowercase();
            if text.contains("no such container") || text.contains("not found") {
                Ok(()) // Already gone.
            } else {
                Err(FilesystemError::ExecFailed(error.to_string()))
            }
        }
        Err(_) => Ok(()), // Timeout on cleanup is best-effort.
    }
}

/// Remove all containers matching the managed labels. Used at startup and
/// during shutdown to clean up orphaned helpers.
pub async fn cleanup_orphan_sessions(
    client: &bollard::Docker,
    purpose_filter: &str,
) -> Result<usize, FilesystemError> {
    let timeout = std::time::Duration::from_secs(15);
    let mut filters = std::collections::HashMap::new();
    filters.insert(
        "label".to_string(),
        vec![
            "io.github.tuxstack.managed=true".into(),
            format!("io.github.tuxstack.purpose={purpose_filter}"),
        ],
    );

    let containers = tokio::time::timeout(
        timeout,
        client.list_containers(Some(bollard::query_parameters::ListContainersOptions {
            all: true,
            filters: Some(filters),
            ..Default::default()
        })),
    )
    .await
    .map_err(|_| FilesystemError::Timeout)?
    .map_err(|error| FilesystemError::ExecFailed(error.to_string()))?;

    let mut removed = 0usize;
    for container in containers {
        let id = match container.id {
            Some(id) => id,
            None => continue,
        };
        if force_remove_container(client, &id).await.is_ok() {
            removed += 1;
        }
    }
    Ok(removed)
}
