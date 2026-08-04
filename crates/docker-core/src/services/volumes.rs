//! Docker volume management, usage association, export, and clone operations.

use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use bollard::models::{ContainerCreateBody, HostConfig, Mount, MountType, VolumeCreateRequest};
use bollard::query_parameters::{
    CreateContainerOptions, DataUsageOptions,
    ListContainersOptions as BollardListContainersOptions,
    ListVolumesOptions as BollardListVolumesOptions, LogsOptions,
    PruneVolumesOptions as BollardPruneVolumesOptions,
    RemoveContainerOptions as BollardRemoveContainerOptions,
    RemoveVolumeOptions as BollardRemoveVolumeOptions, WaitContainerOptions,
};
use futures_util::{StreamExt, stream};
use tokio::fs;
use tokio_util::sync::CancellationToken;

use crate::client::DockerClient;
use crate::error::{DockerError, classify_api_error, classify_volume_api_error};
use crate::mapping::volumes::{
    map_system_df_volume_usage, map_volume_detail, map_volume_summary, references_from_inspect,
    references_from_summary,
};
use crate::models::{
    CloneVolumeRequest, CreateVolumeRequest, ExportVolumeRequest, PruneVolumeFilters,
    RemoveVolumeOptions, VolumeContainerReference, VolumeDetail, VolumeExportCompression,
    VolumePruneResult, VolumeSummary, VolumeUsage,
};

const CONTAINER_INSPECT_CONCURRENCY: usize = 8;
const HELPER_IMAGE: &str = "alpine:3.20";
const HELPER_LABEL: &str = "com.tuxstack.volume-helper";
const HELPER_MEMORY_BYTES: i64 = 256 * 1024 * 1024;
const HELPER_NANO_CPUS: i64 = 500_000_000;
const HELPER_PIDS_LIMIT: i64 = 64;
static UNIQUE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Options for listing volumes. Search is local and includes volume metadata
/// and associated container names/IDs.
#[derive(Debug, Clone, Default)]
pub struct ListVolumesOptions {
    pub search: Option<String>,
}

#[derive(Clone)]
pub struct VolumeService {
    client: Arc<DockerClient>,
}

impl VolumeService {
    pub fn new(client: Arc<DockerClient>) -> Self {
        Self { client }
    }

    /// List volumes with usage information and references from all containers,
    /// including stopped containers. `/system/df` is attempted concurrently;
    /// an unavailable or incompatible usage schema leaves sizes unknown.
    pub async fn list_volumes(
        &self,
        options: &ListVolumesOptions,
    ) -> Result<Vec<VolumeSummary>, DockerError> {
        let docker = self.client.inner().clone();
        let timeout = self.client.config().request_timeout;
        let list_volumes = async {
            tokio::time::timeout(
                timeout,
                docker.list_volumes(None::<BollardListVolumesOptions>),
            )
            .await
            .map_err(|_| DockerError::OperationTimeout)?
            .map_err(|error| classify_volume_api_error(&error, "list"))
        };
        let list_containers = self.container_references();
        let disk_usage = self.system_df_usage();
        let (volumes, references, disk_usage) =
            tokio::join!(list_volumes, list_containers, disk_usage);

        let mut references = references?;
        let disk_usage = disk_usage.unwrap_or_default();
        let mut volumes: Vec<_> = volumes?
            .volumes
            .unwrap_or_default()
            .into_iter()
            .map(|volume| {
                let usage = disk_usage.get(&volume.name).copied();
                let mut summary = map_volume_summary(volume, usage);
                summary.used_by = references.remove(&summary.name).unwrap_or_default();
                summary
            })
            .collect();

        if let Some(query) = normalized_search(options.search.as_deref()) {
            volumes.retain(|volume| volume_matches(volume, &query));
        }
        volumes.sort_by(|left, right| {
            right
                .in_use()
                .cmp(&left.in_use())
                .then_with(|| left.name.cmp(&right.name))
        });
        Ok(volumes)
    }

    /// Convenience for callers that need the complete list.
    pub async fn list_all_volumes(&self) -> Result<Vec<VolumeSummary>, DockerError> {
        self.list_volumes(&ListVolumesOptions::default()).await
    }

    pub async fn inspect_volume(&self, name: &str) -> Result<VolumeDetail, DockerError> {
        let name = require_name(name)?;
        let docker = self.client.inner().clone();
        let timeout = self.client.config().request_timeout;
        let inspect = async {
            tokio::time::timeout(timeout, docker.inspect_volume(name))
                .await
                .map_err(|_| DockerError::OperationTimeout)?
                .map_err(|error| classify_volume_api_error(&error, "inspect"))
        };
        let references = self.container_references();
        let disk_usage = self.system_df_usage();
        let (volume, references, disk_usage) = tokio::join!(inspect, references, disk_usage);
        let mut references = references?;
        let usage = disk_usage.ok().and_then(|usage| usage.get(name).copied());
        Ok(map_volume_detail(
            volume?,
            usage,
            references.remove(name).unwrap_or_default(),
        ))
    }

    pub async fn create_volume(
        &self,
        request: CreateVolumeRequest,
    ) -> Result<VolumeDetail, DockerError> {
        validate_map_keys(&request.driver_options)?;
        validate_map_keys(&request.labels)?;
        let requested_name = normalize_optional(request.name);
        if let Some(name) = requested_name.as_deref() {
            let docker = self.client.inner().clone();
            match tokio::time::timeout(
                self.client.config().request_timeout,
                docker.inspect_volume(name),
            )
            .await
            .map_err(|_| DockerError::OperationTimeout)?
            {
                Ok(_) => return Err(DockerError::VolumeAlreadyExists(name.into())),
                Err(error)
                    if matches!(
                        classify_volume_api_error(&error, "inspect"),
                        DockerError::VolumeNotFound(_)
                    ) => {}
                Err(error) => return Err(classify_volume_api_error(&error, "inspect")),
            }
        }
        let config = VolumeCreateRequest {
            name: requested_name.clone(),
            driver: normalize_optional(request.driver),
            driver_opts: non_empty_map(request.driver_options),
            labels: non_empty_map(request.labels),
            ..Default::default()
        };
        let docker = self.client.inner().clone();
        let volume = tokio::time::timeout(
            self.client.config().request_timeout,
            docker.create_volume(config),
        )
        .await
        .map_err(|_| DockerError::OperationTimeout)?
        .map_err(|error| classify_volume_api_error(&error, "create"))?;

        // Inspect to return the same complete domain shape as other detail
        // operations. Creation has already succeeded, so a later inspect,
        // container-list, or df failure must not report the create as failed.
        match self.inspect_volume(&volume.name).await {
            Ok(detail) => Ok(detail),
            Err(_) => Ok(map_volume_detail(volume, None, Vec::new())),
        }
    }

    pub async fn remove_volume(
        &self,
        name: &str,
        options: RemoveVolumeOptions,
    ) -> Result<(), DockerError> {
        let name = require_name(name)?;
        let docker = self.client.inner().clone();
        tokio::time::timeout(
            self.client.config().request_timeout,
            docker.remove_volume(
                name,
                Some(BollardRemoveVolumeOptions {
                    force: options.force,
                }),
            ),
        )
        .await
        .map_err(|_| DockerError::OperationTimeout)?
        .map_err(|error| classify_volume_api_error(&error, "remove"))
    }

    pub async fn prune_volumes(
        &self,
        filters: PruneVolumeFilters,
    ) -> Result<VolumePruneResult, DockerError> {
        validate_prune_filters(&filters)?;
        let options = BollardPruneVolumesOptions {
            filters: (!filters.filters.is_empty()).then(|| {
                filters
                    .filters
                    .into_iter()
                    .collect::<HashMap<String, Vec<String>>>()
            }),
        };
        let docker = self.client.inner().clone();
        let result = tokio::time::timeout(
            self.client.config().request_timeout,
            docker.prune_volumes(Some(options)),
        )
        .await
        .map_err(|_| DockerError::OperationTimeout)?
        .map_err(|error| classify_volume_api_error(&error, "prune"))?;
        Ok(VolumePruneResult {
            volumes_deleted: result.volumes_deleted.unwrap_or_default(),
            space_reclaimed_bytes: result
                .space_reclaimed
                .and_then(|value| value.try_into().ok()),
        })
    }

    /// Export through a constrained helper container. The source volume is
    /// read-only, the destination directory is the only bind mount, networking
    /// is disabled, and output is atomically renamed from a unique temporary
    /// file. The fixed helper image must already exist locally.
    pub async fn export_volume(
        &self,
        request: ExportVolumeRequest,
        cancellation: CancellationToken,
    ) -> Result<(), DockerError> {
        check_cancellation(&cancellation)?;
        if request.compression == VolumeExportCompression::TarZstd {
            return Err(DockerError::UnsupportedVolumeCompression("tar.zst".into()));
        }
        let volume_name = require_name(&request.volume_name)?.to_string();
        self.ensure_local_file_operation()?;
        // Fail with a typed volume error before creating host files/helpers.
        cancellable(&cancellation, self.inspect_volume(&volume_name)).await?;
        let output = prepare_export_paths(&request.destination).await?;
        check_cancellation(&cancellation)?;
        let mut temporary_guard =
            TemporaryFileGuard::new(output.temporary.clone(), output.staging_directory.clone());
        let command = export_command(request.compression, &output.temporary_file_name);
        let mounts = vec![
            volume_mount(&volume_name, "/source", true),
            bind_mount(&output.staging_directory, "/output", false)?,
        ];
        let (helper, mut helper_guard) =
            cancellable(&cancellation, self.create_helper("export", command, mounts))
                .await
                .map_err(as_export_error)?;
        let operation = self.run_helper(&helper, &cancellation).await;
        let cleanup = self.remove_helper(&helper).await;
        if cleanup.is_ok() {
            helper_guard.disarm();
        }

        match (operation, cleanup) {
            (Ok(()), Ok(())) => {
                fs::rename(&output.temporary, &output.destination)
                    .await
                    .map_err(|error| classify_destination_io(error, &output.destination))?;
                fs::remove_dir(&output.staging_directory)
                    .await
                    .map_err(|error| classify_destination_io(error, &output.staging_directory))?;
                temporary_guard.disarm();
                Ok(())
            }
            (Err(error), Ok(())) => Err(as_export_error(error)),
            (Err(error), Err(cleanup_error)) => Err(DockerError::CleanupFailed(format!(
                "export failed ({error}) and helper cleanup failed: {cleanup_error}"
            ))),
            (Ok(()), Err(error)) => Err(DockerError::CleanupFailed(format!(
                "export helper cleanup failed: {error}"
            ))),
        }
    }

    /// Clone through a constrained helper using `cp -a /source/. /target/`.
    /// This includes dotfiles and preserves ordinary ownership, modes,
    /// symlinks, and timestamps supported by the source/target drivers.
    pub async fn clone_volume(
        &self,
        request: CloneVolumeRequest,
        cancellation: CancellationToken,
    ) -> Result<VolumeDetail, DockerError> {
        check_cancellation(&cancellation)?;
        let source = require_name(&request.source_volume)?.to_string();
        let target = require_name(&request.target_name)?.to_string();
        if source == target {
            return Err(DockerError::InvalidVolumeName(
                "source and target volume names must differ".into(),
            ));
        }
        validate_map_keys(&request.target_driver_options)?;
        let source_detail = cancellable(&cancellation, self.inspect_volume(&source)).await?;
        match cancellable(&cancellation, self.inspect_volume(&target)).await {
            Ok(_) => return Err(DockerError::VolumeAlreadyExists(target)),
            Err(DockerError::VolumeNotFound(_)) => {}
            Err(error) => return Err(error),
        }
        let labels = if request.copy_labels {
            source_detail.summary.labels.clone()
        } else {
            BTreeMap::new()
        };
        let target_detail = self
            .create_volume(CreateVolumeRequest {
                name: Some(target.clone()),
                driver: normalize_optional(request.target_driver),
                driver_options: request.target_driver_options,
                labels,
            })
            .await?;
        let mut target_guard = DockerResourceGuard::volume(
            self.client.inner().clone(),
            target.clone(),
            self.client.config().request_timeout,
        );
        if let Err(cancelled) = check_cancellation(&cancellation) {
            return self.cleanup_failed_clone(&target, cancelled).await;
        }

        let mounts = vec![
            volume_mount(&source, "/source", true),
            volume_mount(&target, "/target", false),
        ];
        let helper = cancellable(
            &cancellation,
            self.create_helper(
                "clone",
                vec![
                    "cp".into(),
                    "-a".into(),
                    "/source/.".into(),
                    "/target/".into(),
                ],
                mounts,
            ),
        )
        .await;
        let (helper, mut helper_guard) = match helper {
            Ok(helper) => helper,
            Err(error) => {
                return self
                    .cleanup_failed_clone(&target, as_clone_error(error))
                    .await;
            }
        };
        let operation = self.run_helper(&helper, &cancellation).await;
        let helper_cleanup = self.remove_helper(&helper).await;
        if helper_cleanup.is_ok() {
            helper_guard.disarm();
        }
        if let Err(error) = operation {
            let operation_error = match helper_cleanup {
                Ok(()) => as_clone_error(error),
                Err(cleanup) => DockerError::CleanupFailed(format!(
                    "clone failed and helper cleanup failed: {cleanup}"
                )),
            };
            return self.cleanup_failed_clone(&target, operation_error).await;
        }
        if let Err(error) = helper_cleanup {
            return self
                .cleanup_failed_clone(
                    &target,
                    DockerError::CleanupFailed(format!("clone helper cleanup failed: {error}")),
                )
                .await;
        }
        target_guard.disarm();
        Ok(target_detail)
    }

    async fn cleanup_failed_clone<T>(
        &self,
        target: &str,
        operation_error: DockerError,
    ) -> Result<T, DockerError> {
        match self
            .remove_volume(target, RemoveVolumeOptions { force: true })
            .await
        {
            Ok(()) | Err(DockerError::VolumeNotFound(_)) => Err(operation_error),
            Err(cleanup) => Err(DockerError::CleanupFailed(format!(
                "incomplete clone volume cleanup failed after {operation_error}: {cleanup}"
            ))),
        }
    }

    async fn system_df_usage(&self) -> Result<HashMap<String, VolumeUsage>, DockerError> {
        let docker = self.client.inner().clone();
        let response = tokio::time::timeout(
            self.client.config().request_timeout,
            docker.df(None::<DataUsageOptions>),
        )
        .await
        .map_err(|_| DockerError::OperationTimeout)?
        .map_err(|error| classify_api_error(&error, "system"))?;
        Ok(map_system_df_volume_usage(
            response.volume_usage.and_then(|usage| usage.items),
        ))
    }

    async fn container_references(
        &self,
    ) -> Result<HashMap<String, Vec<VolumeContainerReference>>, DockerError> {
        let docker = self.client.inner().clone();
        let containers = tokio::time::timeout(
            self.client.config().request_timeout,
            docker.list_containers(Some(BollardListContainersOptions {
                all: true,
                ..Default::default()
            })),
        )
        .await
        .map_err(|_| DockerError::OperationTimeout)?
        .map_err(|error| classify_api_error(&error, "container"))?;

        let mut result: HashMap<String, Vec<VolumeContainerReference>> = HashMap::new();
        let mut inspect_ids = Vec::new();
        for container in containers {
            if container.mounts.is_some() {
                extend_references(&mut result, references_from_summary(&container));
            } else if let Some(id) = container.id.filter(|id| !id.is_empty()) {
                inspect_ids.push(id);
            }
        }

        let timeout = self.client.config().request_timeout;
        let inspected = stream::iter(inspect_ids.into_iter().map(|id| {
            let docker = self.client.inner().clone();
            async move {
                tokio::time::timeout(timeout, docker.inspect_container(&id, None))
                    .await
                    .map_err(|_| DockerError::OperationTimeout)?
                    .map_err(|error| classify_api_error(&error, "container"))
            }
        }))
        .buffer_unordered(CONTAINER_INSPECT_CONCURRENCY)
        .collect::<Vec<_>>()
        .await;
        for container in inspected {
            match container {
                Ok(container) => {
                    extend_references(&mut result, references_from_inspect(&container));
                }
                // A container may disappear between list and inspect. It no
                // longer references a volume, so this race is safe to skip.
                Err(DockerError::ContainerNotFound(_)) => {}
                Err(error) => return Err(error),
            }
        }
        for references in result.values_mut() {
            references.sort_by(|left, right| {
                left.name
                    .cmp(&right.name)
                    .then_with(|| left.id.cmp(&right.id))
                    .then_with(|| left.destination.cmp(&right.destination))
            });
        }
        Ok(result)
    }

    async fn create_helper(
        &self,
        operation: &str,
        command: Vec<String>,
        mounts: Vec<Mount>,
    ) -> Result<(String, DockerResourceGuard), DockerError> {
        let helper_name = unique_name(&format!("tuxstack-helper-{operation}"));
        let docker = self.client.inner().clone();
        // Arm cleanup before the create request so dropping this future while
        // the daemon is processing it still schedules removal by unique name.
        let guard = DockerResourceGuard::helper(
            docker.clone(),
            helper_name.clone(),
            self.client.config().request_timeout,
        );
        let response = tokio::time::timeout(
            self.client.config().request_timeout,
            docker.create_container(
                Some(CreateContainerOptions {
                    name: Some(helper_name),
                    platform: String::new(),
                }),
                ContainerCreateBody {
                    image: Some(HELPER_IMAGE.into()),
                    cmd: Some(command),
                    labels: Some(
                        [(HELPER_LABEL.into(), operation.into())]
                            .into_iter()
                            .collect(),
                    ),
                    host_config: Some(HostConfig {
                        network_mode: Some("none".into()),
                        memory: Some(HELPER_MEMORY_BYTES),
                        nano_cpus: Some(HELPER_NANO_CPUS),
                        pids_limit: Some(HELPER_PIDS_LIMIT),
                        readonly_rootfs: Some(true),
                        security_opt: Some(vec!["no-new-privileges:true".into()]),
                        cap_drop: Some(vec!["ALL".into()]),
                        cap_add: Some(vec![
                            "CHOWN".into(),
                            "DAC_OVERRIDE".into(),
                            "DAC_READ_SEARCH".into(),
                            "FOWNER".into(),
                        ]),
                        mounts: Some(mounts),
                        auto_remove: Some(false),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            ),
        )
        .await
        .map_err(|_| DockerError::OperationTimeout)?
        .map_err(|error| classify_api_error(&error, "container"))?;
        Ok((response.id, guard))
    }

    async fn run_helper(
        &self,
        helper: &str,
        cancellation: &CancellationToken,
    ) -> Result<(), DockerError> {
        let docker = self.client.inner().clone();
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => return Err(DockerError::OperationCancelled),
            result = tokio::time::timeout(
                self.client.config().request_timeout,
                docker.start_container(helper, None),
            ) => {
                result
                    .map_err(|_| DockerError::OperationTimeout)?
                    .map_err(|error| classify_api_error(&error, "container"))?;
            }
        }

        let mut wait = Box::pin(docker.wait_container(
            helper,
            Some(WaitContainerOptions {
                condition: "not-running".into(),
            }),
        ));
        let response = tokio::select! {
            biased;
            _ = cancellation.cancelled() => return Err(DockerError::OperationCancelled),
            response = tokio::time::timeout(self.client.config().request_timeout, wait.next()) => {
                let response = response
                    .map_err(|_| DockerError::OperationTimeout)?
                    .ok_or_else(|| DockerError::InvalidResponse("helper wait returned no status".into()))?;
                match response {
                    Ok(response) => response,
                    Err(bollard::errors::Error::DockerContainerWaitError { code, error }) => {
                        let detail = if error.trim().is_empty() {
                            self.helper_error_output(helper).await
                        } else {
                            error
                        };
                        return Err(DockerError::Api(if detail.trim().is_empty() {
                            format!("helper exited with status {code}")
                        } else {
                            format!("helper exited with status {code}: {}", detail.trim())
                        }));
                    }
                    Err(error) => return Err(classify_api_error(&error, "container")),
                }
            }
        };
        if response.status_code == 0 {
            Ok(())
        } else {
            let message = response
                .error
                .and_then(|error| error.message)
                .filter(|message| !message.is_empty())
                .unwrap_or_else(|| format!("helper exited with status {}", response.status_code));
            Err(DockerError::Api(message))
        }
    }

    async fn helper_error_output(&self, helper: &str) -> String {
        let docker = self.client.inner().clone();
        let logs = docker.logs(
            helper,
            Some(LogsOptions {
                stdout: true,
                stderr: true,
                tail: "20".into(),
                ..Default::default()
            }),
        );
        let collected = tokio::time::timeout(
            self.client.config().request_timeout,
            logs.filter_map(|entry| async move { entry.ok().map(|output| output.to_string()) })
                .collect::<String>(),
        )
        .await
        .unwrap_or_default();
        collected.chars().take(4096).collect()
    }

    async fn remove_helper(&self, helper: &str) -> Result<(), DockerError> {
        let docker = self.client.inner().clone();
        tokio::time::timeout(
            self.client.config().request_timeout,
            docker.remove_container(
                helper,
                Some(BollardRemoveContainerOptions {
                    force: true,
                    v: false,
                    link: false,
                }),
            ),
        )
        .await
        .map_err(|_| DockerError::OperationTimeout)?
        .map_err(|error| classify_api_error(&error, "container"))
    }

    fn ensure_local_file_operation(&self) -> Result<(), DockerError> {
        if self.client.is_local() {
            Ok(())
        } else {
            Err(DockerError::UnsupportedConnection(
                "volume export requires a local Docker Engine".into(),
            ))
        }
    }
}

fn extend_references(
    result: &mut HashMap<String, Vec<VolumeContainerReference>>,
    references: Vec<(String, VolumeContainerReference)>,
) {
    for (volume, reference) in references {
        result.entry(volume).or_default().push(reference);
    }
}

async fn cancellable<T>(
    cancellation: &CancellationToken,
    operation: impl Future<Output = Result<T, DockerError>>,
) -> Result<T, DockerError> {
    tokio::select! {
        biased;
        _ = cancellation.cancelled() => Err(DockerError::OperationCancelled),
        result = operation => result,
    }
}

fn check_cancellation(cancellation: &CancellationToken) -> Result<(), DockerError> {
    if cancellation.is_cancelled() {
        Err(DockerError::OperationCancelled)
    } else {
        Ok(())
    }
}

fn require_name(name: &str) -> Result<&str, DockerError> {
    let name = name.trim();
    if name.is_empty() {
        Err(DockerError::InvalidVolumeName(
            "volume name cannot be empty".into(),
        ))
    } else {
        Ok(name)
    }
}

fn normalize_optional(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let value = value.trim().to_string();
        (!value.is_empty()).then_some(value)
    })
}

fn non_empty_map(map: BTreeMap<String, String>) -> Option<HashMap<String, String>> {
    (!map.is_empty()).then(|| map.into_iter().collect())
}

fn validate_map_keys(map: &BTreeMap<String, String>) -> Result<(), DockerError> {
    if map.keys().any(|key| key.trim().is_empty()) {
        Err(DockerError::InvalidVolumeName(
            "label and driver-option keys cannot be empty".into(),
        ))
    } else {
        Ok(())
    }
}

fn validate_prune_filters(filters: &PruneVolumeFilters) -> Result<(), DockerError> {
    const ALLOWED: [&str; 2] = ["label", "all"];
    if let Some(key) = filters
        .filters
        .keys()
        .find(|key| !ALLOWED.contains(&key.as_str()))
    {
        return Err(DockerError::InvalidVolumeName(format!(
            "unsupported volume prune filter: {key}"
        )));
    }
    Ok(())
}

fn normalized_search(search: Option<&str>) -> Option<String> {
    search
        .map(str::trim)
        .filter(|search| !search.is_empty())
        .map(str::to_lowercase)
}

fn volume_matches(volume: &VolumeSummary, query: &str) -> bool {
    let matches = |value: &str| value.to_lowercase().contains(query);
    matches(&volume.name)
        || matches(&volume.driver)
        || matches(&volume.scope)
        || volume.mountpoint.as_deref().is_some_and(matches)
        || volume
            .labels
            .iter()
            .chain(volume.options.iter())
            .any(|(key, value)| matches(key) || matches(value))
        || volume
            .used_by
            .iter()
            .any(|container| matches(&container.name) || matches(&container.id))
}

fn volume_mount(source: &str, target: &str, read_only: bool) -> Mount {
    Mount {
        typ: Some(MountType::VOLUME),
        source: Some(source.into()),
        target: Some(target.into()),
        read_only: Some(read_only),
        ..Default::default()
    }
}

fn bind_mount(source: &Path, target: &str, read_only: bool) -> Result<Mount, DockerError> {
    let source = source
        .to_str()
        .ok_or_else(|| DockerError::DestinationPermissionDenied(source.to_path_buf()))?;
    Ok(Mount {
        typ: Some(MountType::BIND),
        source: Some(source.into()),
        target: Some(target.into()),
        read_only: Some(read_only),
        ..Default::default()
    })
}

struct ExportPaths {
    staging_directory: PathBuf,
    destination: PathBuf,
    temporary: PathBuf,
    temporary_file_name: String,
}

async fn prepare_export_paths(destination: &Path) -> Result<ExportPaths, DockerError> {
    let destination = absolute_path(destination)?;
    if fs::try_exists(&destination)
        .await
        .map_err(|error| classify_destination_io(error, &destination))?
    {
        return Err(DockerError::ExportFailed(format!(
            "destination already exists: {}",
            destination.display()
        )));
    }
    let parent = destination
        .parent()
        .ok_or_else(|| DockerError::DestinationPermissionDenied(destination.clone()))?;
    let parent = fs::canonicalize(parent)
        .await
        .map_err(|error| classify_destination_io(error, parent))?;
    let file_name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .ok_or_else(|| DockerError::DestinationPermissionDenied(destination.clone()))?;
    let suffix = unique_suffix();
    let temporary_file_name = format!("{file_name}.partial");
    let staging_directory = parent.join(format!(".tuxstack-volume-export-{suffix}"));
    fs::create_dir(&staging_directory)
        .await
        .map_err(|error| classify_destination_io(error, &staging_directory))?;
    if let Err(error) =
        fs::set_permissions(&staging_directory, std::fs::Permissions::from_mode(0o733)).await
    {
        let _ = fs::remove_dir(&staging_directory).await;
        return Err(classify_destination_io(error, &staging_directory));
    }
    let temporary = staging_directory.join(&temporary_file_name);
    Ok(ExportPaths {
        staging_directory,
        destination,
        temporary,
        temporary_file_name,
    })
}

fn absolute_path(path: &Path) -> Result<PathBuf, DockerError> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        std::env::current_dir()
            .map(|current| current.join(path))
            .map_err(|error| classify_destination_io(error, path))
    }
}

fn export_command(compression: VolumeExportCompression, output: &str) -> Vec<String> {
    let flag = match compression {
        VolumeExportCompression::Tar => "-cpf",
        VolumeExportCompression::TarGzip => "-czpf",
        VolumeExportCompression::TarZstd => unreachable!("zstd is rejected before helper creation"),
    };
    vec![
        "tar".into(),
        "-C".into(),
        "/source".into(),
        flag.into(),
        format!("/output/{output}"),
        ".".into(),
    ]
}

#[derive(Clone, Copy)]
enum DockerResource {
    Helper,
    Volume,
}

/// Best-effort async cleanup if an operation future is aborted at any await.
/// Normal paths perform and report cleanup synchronously, then disarm this
/// guard. The guard intentionally logs neither labels nor driver options.
struct DockerResourceGuard {
    docker: bollard::Docker,
    name: Option<String>,
    resource: DockerResource,
    timeout: Duration,
}

impl DockerResourceGuard {
    fn helper(docker: bollard::Docker, name: String, timeout: Duration) -> Self {
        Self {
            docker,
            name: Some(name),
            resource: DockerResource::Helper,
            timeout,
        }
    }

    fn volume(docker: bollard::Docker, name: String, timeout: Duration) -> Self {
        Self {
            docker,
            name: Some(name),
            resource: DockerResource::Volume,
            timeout,
        }
    }

    fn disarm(&mut self) {
        self.name = None;
    }
}

impl Drop for DockerResourceGuard {
    fn drop(&mut self) {
        let Some(name) = self.name.take() else {
            return;
        };
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let docker = self.docker.clone();
        let timeout = self.timeout;
        let resource = self.resource;
        handle.spawn(async move {
            match resource {
                DockerResource::Helper => {
                    let _ = tokio::time::timeout(
                        timeout,
                        docker.remove_container(
                            &name,
                            Some(BollardRemoveContainerOptions {
                                force: true,
                                v: false,
                                link: false,
                            }),
                        ),
                    )
                    .await;
                }
                DockerResource::Volume => {
                    let _ = tokio::time::timeout(
                        timeout,
                        docker
                            .remove_volume(&name, Some(BollardRemoveVolumeOptions { force: true })),
                    )
                    .await;
                }
            }
        });
    }
}

struct TemporaryFileGuard {
    path: Option<PathBuf>,
    staging_directory: Option<PathBuf>,
}

impl TemporaryFileGuard {
    fn new(path: PathBuf, staging_directory: PathBuf) -> Self {
        Self {
            path: Some(path),
            staging_directory: Some(staging_directory),
        }
    }

    fn disarm(&mut self) {
        self.path = None;
        self.staging_directory = None;
    }
}

impl Drop for TemporaryFileGuard {
    fn drop(&mut self) {
        let path = self.path.take();
        let staging_directory = self.staging_directory.take();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                if let Some(path) = path {
                    let _ = fs::remove_file(path).await;
                }
                if let Some(staging_directory) = staging_directory {
                    let _ = fs::remove_dir(staging_directory).await;
                }
            });
        }
    }
}

fn classify_destination_io(error: std::io::Error, path: &Path) -> DockerError {
    if error.raw_os_error() == Some(28) {
        DockerError::DiskFull(path.to_path_buf())
    } else if error.kind() == std::io::ErrorKind::PermissionDenied {
        DockerError::DestinationPermissionDenied(path.to_path_buf())
    } else {
        DockerError::ExportFailed(format!("{}: {error}", path.display()))
    }
}

fn as_export_error(error: DockerError) -> DockerError {
    match error {
        DockerError::OperationCancelled
        | DockerError::OperationTimeout
        | DockerError::PermissionDenied
        | DockerError::EngineUnavailable
        | DockerError::SocketNotFound(_)
        | DockerError::DestinationPermissionDenied(_)
        | DockerError::DiskFull(_)
        | DockerError::CleanupFailed(_) => error,
        error => DockerError::ExportFailed(error.to_string()),
    }
}

fn as_clone_error(error: DockerError) -> DockerError {
    match error {
        DockerError::OperationCancelled
        | DockerError::OperationTimeout
        | DockerError::PermissionDenied
        | DockerError::EngineUnavailable
        | DockerError::SocketNotFound(_)
        | DockerError::CleanupFailed(_) => error,
        error => DockerError::CloneFailed(error.to_string()),
    }
}

fn unique_suffix() -> String {
    let count = UNIQUE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    format!("{}-{nanos}-{count}", std::process::id())
}

fn unique_name(prefix: &str) -> String {
    format!("{prefix}-{}", unique_suffix())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ContainerState, VolumeUsage};

    fn searchable_volume() -> VolumeSummary {
        VolumeSummary {
            name: "postgres_data".into(),
            driver: "local".into(),
            scope: "local".into(),
            mountpoint: Some("/var/lib/docker/volumes/postgres_data/_data".into()),
            created_at: None,
            labels: [("application".into(), "database".into())]
                .into_iter()
                .collect(),
            options: [("type".into(), "nfs".into())].into_iter().collect(),
            usage: VolumeUsage::default(),
            used_by: vec![VolumeContainerReference {
                id: "abcdef0123456789".into(),
                short_id: "abcdef012345".into(),
                name: "postgres".into(),
                state: ContainerState::Exited,
                destination: "/data".into(),
                read_only: false,
                propagation: None,
            }],
            anonymous: false,
        }
    }

    #[test]
    fn search_includes_metadata_and_container_fields() {
        let volume = searchable_volume();
        for query in [
            "POSTGRES_DATA",
            "NFS",
            "database",
            "/var/lib/docker",
            "abcdef0123",
        ] {
            assert!(volume_matches(&volume, &query.to_lowercase()), "{query}");
        }
    }

    #[test]
    fn tar_commands_are_argv_without_shell_interpolation() {
        assert_eq!(
            export_command(VolumeExportCompression::Tar, "out.tar"),
            ["tar", "-C", "/source", "-cpf", "/output/out.tar", "."]
        );
        assert_eq!(
            export_command(VolumeExportCompression::TarGzip, "out.tar.gz"),
            ["tar", "-C", "/source", "-czpf", "/output/out.tar.gz", "."]
        );
    }

    #[test]
    fn invalid_keys_and_prune_filters_are_rejected() {
        assert!(validate_map_keys(&[("".into(), "secret".into())].into_iter().collect()).is_err());
        assert!(
            validate_prune_filters(&PruneVolumeFilters {
                filters: [("until".into(), vec!["24h".into()])].into_iter().collect(),
            })
            .is_err()
        );
    }

    #[test]
    fn pre_cancelled_operations_stop_before_docker_work() {
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        assert!(matches!(
            check_cancellation(&cancellation),
            Err(DockerError::OperationCancelled)
        ));
    }
}
