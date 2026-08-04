//! Read-only Docker volume file browsing through constrained helper containers.

mod protocol;
mod session;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use bollard::exec::{CreateExecOptions, StartExecOptions, StartExecResults};
use bollard::models::{ContainerCreateBody, HostConfig, Mount, MountType, ResourcesUlimits};
use bollard::query_parameters::{
    CreateContainerOptions, ListContainersOptions,
    RemoveContainerOptions as BollardRemoveContainerOptions,
};
use chrono::{TimeZone, Utc};
use futures_util::StreamExt;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::client::DockerClient;
use crate::error::{DockerError, classify_api_error};
use crate::models::{
    DownloadVolumeFileRequest, FilePreviewContent, FilePreviewKind, ListVolumeDirectoryRequest,
    PreviewVolumeFileRequest, VolumeFileEntry, VolumeFilePreview, VolumeFileProperties,
    VolumeFileType, VolumeHelperConfig, VolumePath, VolumePreviewSession,
};

use protocol::{LIST_SCRIPT, PREVIEW_HEAD_SCRIPT, STAT_SCRIPT, decode_name, parse_list_line};
use session::{LABEL_MANAGED, LABEL_PURPOSE, LABEL_SESSION, LABEL_VOLUME, PURPOSE_VALUE};

pub use session::VolumePreviewSessionHandle;

const ORPHAN_LABEL_FILTER: &str = "io.github.tuxstack.managed=true";

/// Service for secure, read-only volume file browsing.
#[derive(Clone)]
pub struct VolumeFileService {
    client: Arc<DockerClient>,
    config: VolumeHelperConfig,
}

impl VolumeFileService {
    pub fn new(client: Arc<DockerClient>) -> Self {
        Self {
            client,
            config: VolumeHelperConfig::default(),
        }
    }

    pub fn with_config(client: Arc<DockerClient>, config: VolumeHelperConfig) -> Self {
        Self { client, config }
    }

    pub fn config(&self) -> &VolumeHelperConfig {
        &self.config
    }

    /// Start a long-lived sleep helper with the volume mounted read-only.
    pub async fn start_session(
        &self,
        volume_name: &str,
        cancellation: CancellationToken,
    ) -> Result<VolumePreviewSession, DockerError> {
        let timer = crate::instrument::Timer::start("volume_preview.start_session");
        let result = self.start_session_inner(volume_name, cancellation).await;
        match &result {
            Ok(_) => timer.finish_ok(1, "live"),
            Err(error) => timer.finish_err(&error.to_string()),
        }
        result
    }

    async fn start_session_inner(
        &self,
        volume_name: &str,
        cancellation: CancellationToken,
    ) -> Result<VolumePreviewSession, DockerError> {
        let volume_name = volume_name.trim();
        if volume_name.is_empty() {
            return Err(DockerError::InvalidVolumeName(
                "volume name is required".into(),
            ));
        }
        check_cancel(&cancellation)?;
        self.ensure_helper_image().await?;

        let session_id = Uuid::new_v4();
        let container_name = format!("tuxstack-volume-preview-{session_id}");
        let docker = self.client.inner().clone();
        let timeout = self.config.operation_timeout;

        let labels = [
            (LABEL_MANAGED.to_string(), "true".into()),
            (LABEL_PURPOSE.to_string(), PURPOSE_VALUE.into()),
            (LABEL_VOLUME.to_string(), volume_name.to_string()),
            (LABEL_SESSION.to_string(), session_id.to_string()),
        ]
        .into_iter()
        .collect();

        let mounts = vec![Mount {
            typ: Some(MountType::VOLUME),
            source: Some(volume_name.into()),
            target: Some(self.config.mount_path.clone()),
            read_only: Some(true),
            ..Default::default()
        }];

        let create = async {
            docker
                .create_container(
                    Some(CreateContainerOptions {
                        name: Some(container_name.clone()),
                        platform: String::new(),
                    }),
                    ContainerCreateBody {
                        image: Some(self.config.image.clone()),
                        // Keep the session alive for exec; never auto-remove.
                        cmd: Some(vec!["sleep".into(), "infinity".into()]),
                        labels: Some(labels),
                        host_config: Some(HostConfig {
                            network_mode: Some("none".into()),
                            memory: Some(self.config.memory_limit_bytes),
                            nano_cpus: Some(self.config.nano_cpus),
                            pids_limit: Some(self.config.pids_limit),
                            readonly_rootfs: Some(true),
                            security_opt: Some(vec!["no-new-privileges:true".into()]),
                            cap_drop: Some(vec!["ALL".into()]),
                            mounts: Some(mounts),
                            auto_remove: Some(false),
                            tmpfs: Some(
                                [("/tmp".into(), "rw,noexec,nosuid,size=16m".into())]
                                    .into_iter()
                                    .collect(),
                            ),
                            ulimits: Some(vec![ResourcesUlimits {
                                name: Some("nofile".into()),
                                soft: Some(1024),
                                hard: Some(1024),
                            }]),
                            privileged: Some(false),
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                )
                .await
                .map_err(|error| {
                    let text = error.to_string().to_ascii_lowercase();
                    if text.contains("no such image") || text.contains("not found") {
                        DockerError::VolumePreviewHelperImageMissing
                    } else if text.contains("no such volume") {
                        DockerError::VolumeNotFound(volume_name.to_string())
                    } else {
                        DockerError::VolumePreviewSessionFailed(error.to_string())
                    }
                })
        };

        let response = tokio::select! {
            biased;
            _ = cancellation.cancelled() => return Err(DockerError::OperationCancelled),
            result = tokio::time::timeout(timeout, create) => {
                result.map_err(|_| DockerError::OperationTimeout)??
            }
        };

        let start = async {
            docker
                .start_container(&response.id, None)
                .await
                .map_err(|error| DockerError::VolumePreviewSessionFailed(error.to_string()))
        };
        if let Err(error) = tokio::select! {
            biased;
            _ = cancellation.cancelled() => Err(DockerError::OperationCancelled),
            result = tokio::time::timeout(timeout, start) => {
                result.map_err(|_| DockerError::OperationTimeout)?
            }
        } {
            let _ = self.force_remove_container(&response.id).await;
            return Err(error);
        }

        Ok(VolumePreviewSession {
            id: session_id,
            volume_name: volume_name.to_string(),
            container_id: response.id,
            container_name,
            started_at: Utc::now(),
        })
    }

    pub async fn stop_session(&self, session: VolumePreviewSession) -> Result<(), DockerError> {
        self.force_remove_container(&session.container_id).await
    }

    pub async fn list_directory(
        &self,
        session: &VolumePreviewSession,
        request: &ListVolumeDirectoryRequest,
        cancellation: CancellationToken,
    ) -> Result<Vec<VolumeFileEntry>, DockerError> {
        let timer = crate::instrument::Timer::start("volume_preview.list_directory");
        let result = self
            .list_directory_inner(session, request, cancellation)
            .await;
        match &result {
            Ok(entries) => timer.finish_ok(entries.len(), "live"),
            Err(error) => timer.finish_err(&error.to_string()),
        }
        result
    }

    async fn list_directory_inner(
        &self,
        session: &VolumePreviewSession,
        request: &ListVolumeDirectoryRequest,
        cancellation: CancellationToken,
    ) -> Result<Vec<VolumeFileEntry>, DockerError> {
        self.ensure_session_volume(session, &request.volume_name)?;
        check_cancel(&cancellation)?;
        let helper_path = request.path.helper_absolute();
        let output = self
            .exec_collect(
                session,
                vec![
                    "sh".into(),
                    "-c".into(),
                    LIST_SCRIPT.into(),
                    "list".into(),
                    helper_path,
                    if request.show_hidden {
                        "1".into()
                    } else {
                        "0".into()
                    },
                    self.config.max_directory_entries.to_string(),
                ],
                &cancellation,
            )
            .await?;

        let mut entries = Vec::new();
        for line in output.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if line == "TRUNCATED" {
                break;
            }
            let parsed = parse_list_line(line).map_err(|e| {
                DockerError::VolumeHelperProtocolError(format!("list parse failed: {e}"))
            })?;
            let name = decode_name(&parsed.name_b64).map_err(|e| {
                DockerError::VolumeHelperProtocolError(format!("list name decode: {e}"))
            })?;
            let path = request.path.join_name(&name).map_err(|e| {
                DockerError::VolumeHelperProtocolError(format!("invalid entry name: {e}"))
            })?;
            let target = if parsed.target_b64.is_empty() {
                None
            } else {
                Some(decode_name(&parsed.target_b64).map_err(|e| {
                    DockerError::VolumeHelperProtocolError(format!("symlink decode: {e}"))
                })?)
            };
            let entry_type = VolumeFileType::from_protocol(&parsed.type_code);
            let size_bytes = if entry_type.is_directory() {
                None
            } else {
                parsed.size
            };
            entries.push(VolumeFileEntry {
                hidden: name.starts_with('.'),
                name,
                path,
                entry_type,
                size_bytes,
                modified_at: parsed
                    .mtime
                    .and_then(|ts| Utc.timestamp_opt(ts, 0).single()),
                mode: parsed.mode,
                uid: parsed.uid,
                gid: parsed.gid,
                symlink_target: target,
                mime_type: None,
                readable: parsed.readable,
            });
        }
        Ok(entries)
    }

    pub async fn file_properties(
        &self,
        session: &VolumePreviewSession,
        volume_name: &str,
        path: &VolumePath,
        cancellation: CancellationToken,
    ) -> Result<VolumeFileProperties, DockerError> {
        self.ensure_session_volume(session, volume_name)?;
        let entry = self.stat_path(session, path, &cancellation).await?;
        Ok(VolumeFileProperties {
            name: entry.name.clone(),
            path: entry.path.clone(),
            entry_type: entry.entry_type,
            mime_type: entry.mime_type.clone(),
            size_bytes: entry.size_bytes,
            modified_at: entry.modified_at,
            mode: entry.mode,
            uid: entry.uid,
            gid: entry.gid,
            symlink_target: entry.symlink_target.clone(),
        })
    }

    pub async fn resolve_entry(
        &self,
        session: &VolumePreviewSession,
        volume_name: &str,
        path: &VolumePath,
        cancellation: CancellationToken,
    ) -> Result<VolumeFileEntry, DockerError> {
        self.ensure_session_volume(session, volume_name)?;
        let entry = self.stat_path(session, path, &cancellation).await?;
        if entry.entry_type != VolumeFileType::SymbolicLink {
            return Ok(entry);
        }
        let resolved = self.resolve_symlink(session, path, &cancellation).await?;
        Ok(resolved)
    }

    pub async fn preview_file(
        &self,
        session: &VolumePreviewSession,
        request: &PreviewVolumeFileRequest,
        cancellation: CancellationToken,
    ) -> Result<VolumeFilePreview, DockerError> {
        self.ensure_session_volume(session, &request.volume_name)?;
        check_cancel(&cancellation)?;
        let entry = self
            .stat_path(session, &request.path, &cancellation)
            .await?;
        if entry.entry_type == VolumeFileType::Directory {
            return Err(DockerError::VolumePreviewUnsupported(
                "directories cannot be previewed".into(),
            ));
        }
        if entry.entry_type == VolumeFileType::SymbolicLink {
            let resolved = self
                .resolve_symlink(session, &request.path, &cancellation)
                .await?;
            return Box::pin(self.preview_file(
                session,
                &PreviewVolumeFileRequest {
                    volume_name: request.volume_name.clone(),
                    path: resolved.path,
                    max_bytes: request.max_bytes,
                },
                cancellation,
            ))
            .await;
        }

        let max_bytes = request.max_bytes.max(1);
        let mime = guess_mime(&entry.name, None);
        let kind = classify_preview_kind(&entry.name, mime.as_deref());
        let limit = match kind {
            FilePreviewKind::Image => self
                .config
                .image_preview_max_bytes
                .min(max_bytes.max(self.config.image_preview_max_bytes)),
            FilePreviewKind::Text | FilePreviewKind::Json => {
                self.config.text_preview_max_bytes.min(max_bytes)
            }
            FilePreviewKind::Binary | FilePreviewKind::Unsupported => 0,
        };

        if matches!(kind, FilePreviewKind::Binary | FilePreviewKind::Unsupported) {
            return Ok(VolumeFilePreview {
                path: entry.path,
                name: entry.name,
                mime_type: mime,
                size_bytes: entry.size_bytes,
                preview_kind: if kind == FilePreviewKind::Unsupported {
                    FilePreviewKind::Unsupported
                } else {
                    FilePreviewKind::Binary
                },
                content: if kind == FilePreviewKind::Unsupported {
                    FilePreviewContent::Unsupported("preview not available".into())
                } else {
                    FilePreviewContent::BinaryInfo
                },
                truncated: false,
            });
        }

        if let Some(size) = entry.size_bytes {
            if kind == FilePreviewKind::Image && size > self.config.image_preview_max_bytes {
                return Ok(VolumeFilePreview {
                    path: entry.path,
                    name: entry.name,
                    mime_type: mime,
                    size_bytes: entry.size_bytes,
                    preview_kind: FilePreviewKind::Binary,
                    content: FilePreviewContent::BinaryInfo,
                    truncated: false,
                });
            }
        }

        let helper_path = entry.path.helper_absolute();
        let bytes = self
            .exec_collect_bytes(
                session,
                vec![
                    "sh".into(),
                    "-c".into(),
                    PREVIEW_HEAD_SCRIPT.into(),
                    "head".into(),
                    helper_path,
                    limit.to_string(),
                ],
                &cancellation,
            )
            .await?;

        let truncated = entry
            .size_bytes
            .map(|size| size > bytes.len() as u64)
            .unwrap_or(bytes.len() as u64 >= limit);

        if bytes.contains(&0) && matches!(kind, FilePreviewKind::Text | FilePreviewKind::Json) {
            return Ok(VolumeFilePreview {
                path: entry.path,
                name: entry.name,
                mime_type: mime,
                size_bytes: entry.size_bytes,
                preview_kind: FilePreviewKind::Binary,
                content: FilePreviewContent::BinaryInfo,
                truncated,
            });
        }

        match kind {
            FilePreviewKind::Image => Ok(VolumeFilePreview {
                path: entry.path,
                name: entry.name,
                mime_type: mime,
                size_bytes: entry.size_bytes,
                preview_kind: FilePreviewKind::Image,
                content: FilePreviewContent::ImageBytes(bytes),
                truncated,
            }),
            FilePreviewKind::Json => {
                let text = decode_text(&bytes);
                let (pretty, parse_error) = match serde_json::from_str::<serde_json::Value>(&text) {
                    Ok(value) => (
                        Some(serde_json::to_string_pretty(&value).unwrap_or_else(|_| text.clone())),
                        None,
                    ),
                    Err(error) => (None, Some(error.to_string())),
                };
                Ok(VolumeFilePreview {
                    path: entry.path,
                    name: entry.name,
                    mime_type: mime,
                    size_bytes: entry.size_bytes,
                    preview_kind: FilePreviewKind::Json,
                    content: FilePreviewContent::Json {
                        pretty,
                        raw: text,
                        parse_error,
                    },
                    truncated,
                })
            }
            FilePreviewKind::Text => Ok(VolumeFilePreview {
                path: entry.path,
                name: entry.name,
                mime_type: mime,
                size_bytes: entry.size_bytes,
                preview_kind: FilePreviewKind::Text,
                content: FilePreviewContent::Text(decode_text(&bytes)),
                truncated,
            }),
            FilePreviewKind::Binary | FilePreviewKind::Unsupported => Ok(VolumeFilePreview {
                path: entry.path,
                name: entry.name,
                mime_type: mime,
                size_bytes: entry.size_bytes,
                preview_kind: FilePreviewKind::Binary,
                content: FilePreviewContent::BinaryInfo,
                truncated: false,
            }),
        }
    }

    pub async fn download_file(
        &self,
        session: &VolumePreviewSession,
        request: &DownloadVolumeFileRequest,
        mut progress: Option<tokio::sync::mpsc::Sender<u64>>,
        cancellation: CancellationToken,
    ) -> Result<(), DockerError> {
        self.ensure_session_volume(session, &request.volume_name)?;
        check_cancel(&cancellation)?;
        let entry = self
            .stat_path(session, &request.path, &cancellation)
            .await?;
        let path = if entry.entry_type == VolumeFileType::SymbolicLink {
            self.resolve_symlink(session, &request.path, &cancellation)
                .await?
                .path
        } else if entry.entry_type == VolumeFileType::Directory {
            return Err(DockerError::VolumeDownloadFailed(
                "directories cannot be downloaded in this phase".into(),
            ));
        } else {
            entry.path
        };

        let destination = request.destination.clone();
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|error| map_io_error(error, parent, "create download directory"))?;
        }
        let temporary = temporary_download_path(&destination);
        let mut file = fs::File::create(&temporary)
            .await
            .map_err(|error| map_io_error(error, &temporary, "create temporary download file"))?;
        let helper_path = path.helper_absolute();

        let result = self
            .stream_file_to(
                session,
                &helper_path,
                &mut file,
                progress.as_mut(),
                &cancellation,
            )
            .await;

        match result {
            Ok(()) => {
                file.sync_all().await.ok();
                drop(file);
                fs::rename(&temporary, &destination)
                    .await
                    .map_err(|error| {
                        let _ = std::fs::remove_file(&temporary);
                        map_io_error(error, &destination, "finalize download")
                    })?;
                Ok(())
            }
            Err(error) => {
                drop(file);
                let _ = fs::remove_file(&temporary).await;
                Err(error)
            }
        }
    }

    /// Remove stopped/orphaned managed volume-preview helpers.
    pub async fn cleanup_orphan_sessions(&self) -> Result<usize, DockerError> {
        let docker = self.client.inner().clone();
        let timeout = self.config.operation_timeout;
        let mut filters = std::collections::HashMap::new();
        filters.insert(
            "label".to_string(),
            vec![
                ORPHAN_LABEL_FILTER.into(),
                format!("{LABEL_PURPOSE}={PURPOSE_VALUE}"),
            ],
        );
        let containers = tokio::time::timeout(
            timeout,
            docker.list_containers(Some(ListContainersOptions {
                all: true,
                filters: Some(filters),
                ..Default::default()
            })),
        )
        .await
        .map_err(|_| DockerError::OperationTimeout)?
        .map_err(|error| classify_api_error(&error, "container"))?;

        let mut removed = 0usize;
        for container in containers {
            let id = match container.id {
                Some(id) => id,
                None => continue,
            };
            // Remove all managed preview helpers found at startup — active
            // sessions are created after cleanup in the same process.
            if self.force_remove_container(&id).await.is_ok() {
                removed += 1;
                tracing::debug!(container_id = %id, "removed orphan volume-preview helper");
            }
        }
        Ok(removed)
    }

    async fn ensure_helper_image(&self) -> Result<(), DockerError> {
        let docker = self.client.inner().clone();
        match tokio::time::timeout(
            self.config.operation_timeout,
            docker.inspect_image(&self.config.image),
        )
        .await
        {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(error)) => {
                let text = error.to_string().to_ascii_lowercase();
                if text.contains("not found") || text.contains("no such image") {
                    Err(DockerError::VolumePreviewHelperImageMissing)
                } else {
                    Err(classify_api_error(&error, "image"))
                }
            }
            Err(_) => Err(DockerError::OperationTimeout),
        }
    }

    fn ensure_session_volume(
        &self,
        session: &VolumePreviewSession,
        volume_name: &str,
    ) -> Result<(), DockerError> {
        if session.volume_name != volume_name {
            Err(DockerError::VolumePreviewSessionFailed(
                "session volume mismatch".into(),
            ))
        } else {
            Ok(())
        }
    }

    async fn stat_path(
        &self,
        session: &VolumePreviewSession,
        path: &VolumePath,
        cancellation: &CancellationToken,
    ) -> Result<VolumeFileEntry, DockerError> {
        check_cancel(cancellation)?;
        let helper_path = path.helper_absolute();
        let output = self
            .exec_collect(
                session,
                vec![
                    "sh".into(),
                    "-c".into(),
                    STAT_SCRIPT.into(),
                    "stat".into(),
                    helper_path,
                ],
                cancellation,
            )
            .await?;
        let line = output.lines().next().unwrap_or("").trim();
        if line == "MISSING" {
            return Err(DockerError::VolumeEntryNotFound(path.display()));
        }
        if line == "UNREADABLE" {
            return Err(DockerError::VolumeEntryUnreadable(path.display()));
        }
        let parsed = parse_list_line(line)
            .map_err(|e| DockerError::VolumeHelperProtocolError(format!("stat parse: {e}")))?;
        let name = if path.is_root() {
            "/".into()
        } else {
            path.components()
                .last()
                .cloned()
                .unwrap_or_else(|| "/".into())
        };
        let target = if parsed.target_b64.is_empty() {
            None
        } else {
            Some(decode_name(&parsed.target_b64).map_err(|e| {
                DockerError::VolumeHelperProtocolError(format!("symlink decode: {e}"))
            })?)
        };
        let entry_type = VolumeFileType::from_protocol(&parsed.type_code);
        Ok(VolumeFileEntry {
            hidden: name.starts_with('.'),
            name,
            path: path.clone(),
            entry_type,
            size_bytes: if entry_type.is_directory() {
                None
            } else {
                parsed.size
            },
            modified_at: parsed
                .mtime
                .and_then(|ts| Utc.timestamp_opt(ts, 0).single()),
            mode: parsed.mode,
            uid: parsed.uid,
            gid: parsed.gid,
            symlink_target: target,
            mime_type: None,
            readable: parsed.readable,
        })
    }

    async fn resolve_symlink(
        &self,
        session: &VolumePreviewSession,
        path: &VolumePath,
        cancellation: &CancellationToken,
    ) -> Result<VolumeFileEntry, DockerError> {
        check_cancel(cancellation)?;
        let helper_path = path.helper_absolute();
        // realpath -m resolves without requiring final component to exist fully;
        // alpine busybox supports realpath.
        let output = self
            .exec_collect(
                session,
                vec![
                    "sh".into(),
                    "-c".into(),
                    r#"
set -eu
target=$1
if [ ! -e "$target" ] && [ ! -L "$target" ]; then
  echo MISSING
  exit 0
fi
if ! resolved=$(realpath "$target" 2>/dev/null); then
  echo LOOP
  exit 0
fi
case "$resolved" in
  /volume|/volume/*) printf 'OK\n%s\n' "$resolved" ;;
  *) echo OUTSIDE ;;
esac
"#
                    .into(),
                    "resolve".into(),
                    helper_path,
                ],
                cancellation,
            )
            .await?;
        let mut lines = output.lines();
        let status = lines.next().unwrap_or("").trim();
        match status {
            "MISSING" => Err(DockerError::VolumeEntryNotFound(path.display())),
            "LOOP" => Err(DockerError::VolumeSymlinkLoop(path.display())),
            "OUTSIDE" => Err(DockerError::VolumeSymlinkOutsideRoot(path.display())),
            "OK" => {
                let resolved = lines.next().unwrap_or("").trim();
                let logical = helper_to_volume_path(resolved)?;
                self.stat_path(session, &logical, cancellation).await
            }
            other => Err(DockerError::VolumeHelperProtocolError(format!(
                "unexpected symlink resolve status: {other}"
            ))),
        }
    }

    async fn stream_file_to(
        &self,
        session: &VolumePreviewSession,
        helper_path: &str,
        file: &mut fs::File,
        progress: Option<&mut tokio::sync::mpsc::Sender<u64>>,
        cancellation: &CancellationToken,
    ) -> Result<(), DockerError> {
        // BusyBox `cat` with path as argv — no shell interpolation of content.
        let docker = self.client.inner().clone();
        let create = docker
            .create_exec(
                &session.container_id,
                CreateExecOptions::<String> {
                    attach_stdout: Some(true),
                    attach_stderr: Some(true),
                    cmd: Some(vec!["cat".into(), helper_path.into()]),
                    ..Default::default()
                },
            )
            .await
            .map_err(|error| classify_api_error(&error, "container"))?;

        let start = docker
            .start_exec(
                &create.id,
                Some(StartExecOptions {
                    detach: false,
                    tty: false,
                    output_capacity: Some(1024 * 1024),
                }),
            )
            .await
            .map_err(|error| classify_api_error(&error, "container"))?;

        let StartExecResults::Attached { mut output, .. } = start else {
            return Err(DockerError::VolumeDownloadFailed(
                "exec started detached".into(),
            ));
        };

        let mut written = 0u64;
        loop {
            check_cancel(cancellation)?;
            let next = tokio::select! {
                biased;
                _ = cancellation.cancelled() => return Err(DockerError::OperationCancelled),
                item = output.next() => item,
            };
            match next {
                Some(Ok(chunk)) => {
                    let bytes = chunk.into_bytes();
                    if bytes.is_empty() {
                        continue;
                    }
                    file.write_all(&bytes).await.map_err(|error| {
                        map_io_error(error, Path::new("download"), "write download chunk")
                    })?;
                    written = written.saturating_add(bytes.len() as u64);
                    if let Some(tx) = progress.as_ref() {
                        let _ = tx.try_send(written);
                    }
                }
                Some(Err(error)) => {
                    return Err(DockerError::VolumeDownloadFailed(error.to_string()));
                }
                None => break,
            }
        }
        Ok(())
    }

    async fn exec_collect(
        &self,
        session: &VolumePreviewSession,
        cmd: Vec<String>,
        cancellation: &CancellationToken,
    ) -> Result<String, DockerError> {
        let bytes = self.exec_collect_bytes(session, cmd, cancellation).await?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    async fn exec_collect_bytes(
        &self,
        session: &VolumePreviewSession,
        cmd: Vec<String>,
        cancellation: &CancellationToken,
    ) -> Result<Vec<u8>, DockerError> {
        check_cancel(cancellation)?;
        let docker = self.client.inner().clone();
        let timeout = self.config.operation_timeout;

        let create = tokio::select! {
            biased;
            _ = cancellation.cancelled() => return Err(DockerError::OperationCancelled),
            result = tokio::time::timeout(timeout, docker.create_exec(
                &session.container_id,
                CreateExecOptions::<String> {
                    attach_stdout: Some(true),
                    attach_stderr: Some(true),
                    cmd: Some(cmd),
                    ..Default::default()
                },
            )) => {
                result
                    .map_err(|_| DockerError::OperationTimeout)?
                    .map_err(|error| {
                        let text = error.to_string().to_ascii_lowercase();
                        if text.contains("no such container") || text.contains("is not running") {
                            DockerError::VolumePreviewSessionClosed
                        } else {
                            classify_api_error(&error, "container")
                        }
                    })?
            }
        };

        let start = tokio::select! {
            biased;
            _ = cancellation.cancelled() => return Err(DockerError::OperationCancelled),
            result = tokio::time::timeout(timeout, docker.start_exec(
                &create.id,
                Some(StartExecOptions {
                    detach: false,
                    tty: false,
                    output_capacity: Some(8 * 1024 * 1024),
                }),
            )) => {
                result
                    .map_err(|_| DockerError::OperationTimeout)?
                    .map_err(|error| classify_api_error(&error, "container"))?
            }
        };

        let StartExecResults::Attached { mut output, .. } = start else {
            return Err(DockerError::VolumeHelperProtocolError(
                "exec started detached".into(),
            ));
        };

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        loop {
            let next = tokio::select! {
                biased;
                _ = cancellation.cancelled() => return Err(DockerError::OperationCancelled),
                item = output.next() => item,
            };
            match next {
                Some(Ok(chunk)) => match chunk {
                    bollard::container::LogOutput::StdOut { message } => {
                        stdout.extend_from_slice(&message);
                    }
                    bollard::container::LogOutput::StdErr { message } => {
                        stderr.extend_from_slice(&message);
                    }
                    bollard::container::LogOutput::Console { message } => {
                        stdout.extend_from_slice(&message);
                    }
                    _ => {}
                },
                Some(Err(error)) => {
                    return Err(DockerError::VolumeHelperProtocolError(error.to_string()));
                }
                None => break,
            }
        }

        if stdout.is_empty() && !stderr.is_empty() {
            let message = String::from_utf8_lossy(&stderr);
            let lower = message.to_ascii_lowercase();
            if lower.contains("permission denied") {
                return Err(DockerError::VolumeEntryUnreadable(message.trim().into()));
            }
            return Err(DockerError::VolumeHelperProtocolError(
                message.trim().into(),
            ));
        }
        Ok(stdout)
    }

    async fn force_remove_container(&self, id: &str) -> Result<(), DockerError> {
        let docker = self.client.inner().clone();
        tokio::time::timeout(
            self.config.operation_timeout,
            docker.remove_container(
                id,
                Some(BollardRemoveContainerOptions {
                    force: true,
                    v: false,
                    link: false,
                }),
            ),
        )
        .await
        .map_err(|_| DockerError::OperationTimeout)?
        .map_err(|error| {
            let text = error.to_string().to_ascii_lowercase();
            if text.contains("no such container") || text.contains("not found") {
                DockerError::VolumePreviewSessionClosed
            } else {
                classify_api_error(&error, "container")
            }
        })
        .or_else(|error| match error {
            DockerError::VolumePreviewSessionClosed => Ok(()),
            other => Err(other),
        })
    }
}

fn check_cancel(token: &CancellationToken) -> Result<(), DockerError> {
    if token.is_cancelled() {
        Err(DockerError::OperationCancelled)
    } else {
        Ok(())
    }
}

fn helper_to_volume_path(helper_path: &str) -> Result<VolumePath, DockerError> {
    let path = helper_path.trim();
    if path == "/volume" {
        return Ok(VolumePath::root());
    }
    let Some(rest) = path.strip_prefix("/volume/") else {
        return Err(DockerError::VolumePathEscapesRoot);
    };
    VolumePath::parse(rest).map_err(DockerError::VolumePathInvalid)
}

fn temporary_download_path(destination: &Path) -> PathBuf {
    let file_name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("download");
    destination.with_file_name(format!(
        ".tuxstack-download-{file_name}.{}.part",
        Uuid::new_v4()
    ))
}

fn map_io_error(error: std::io::Error, path: &Path, context: &str) -> DockerError {
    match error.kind() {
        std::io::ErrorKind::PermissionDenied => {
            DockerError::DestinationPermissionDenied(path.to_path_buf())
        }
        std::io::ErrorKind::StorageFull => DockerError::DiskFull(path.to_path_buf()),
        _ => DockerError::VolumeDownloadFailed(format!("{context}: {error}")),
    }
}

fn decode_text(bytes: &[u8]) -> String {
    let mut data = bytes;
    if let Some(stripped) = data.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        data = stripped;
    }
    String::from_utf8_lossy(data).into_owned()
}

fn guess_mime(name: &str, _header: Option<&[u8]>) -> Option<String> {
    let lower = name.to_ascii_lowercase();
    let mime = if lower.ends_with(".json") {
        "application/json"
    } else if lower.ends_with(".png") {
        "image/png"
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg"
    } else if lower.ends_with(".gif") {
        "image/gif"
    } else if lower.ends_with(".webp") {
        "image/webp"
    } else if lower.ends_with(".bmp") {
        "image/bmp"
    } else if lower.ends_with(".svg") {
        "image/svg+xml"
    } else if lower.ends_with(".txt")
        || lower.ends_with(".log")
        || lower.ends_with(".conf")
        || lower.ends_with(".ini")
        || lower.ends_with(".toml")
        || lower.ends_with(".yaml")
        || lower.ends_with(".yml")
        || lower.ends_with(".xml")
        || lower.ends_with(".csv")
        || lower.ends_with(".env")
        || lower == "dockerfile"
        || lower.ends_with(".md")
        || lower.ends_with(".rs")
        || lower.ends_with(".sh")
    {
        "text/plain"
    } else {
        return None;
    };
    Some(mime.into())
}

fn classify_preview_kind(name: &str, mime: Option<&str>) -> FilePreviewKind {
    if let Some(mime) = mime {
        if mime == "application/json" || name.to_ascii_lowercase().ends_with(".json") {
            return FilePreviewKind::Json;
        }
        if mime.starts_with("image/") {
            return FilePreviewKind::Image;
        }
        if mime.starts_with("text/") {
            return FilePreviewKind::Text;
        }
    }
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".json") {
        FilePreviewKind::Json
    } else if lower.ends_with(".png")
        || lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".gif")
        || lower.ends_with(".webp")
        || lower.ends_with(".bmp")
        || lower.ends_with(".svg")
    {
        FilePreviewKind::Image
    } else if lower.ends_with(".txt")
        || lower.ends_with(".log")
        || lower.ends_with(".conf")
        || lower.ends_with(".ini")
        || lower.ends_with(".toml")
        || lower.ends_with(".yaml")
        || lower.ends_with(".yml")
        || lower.ends_with(".xml")
        || lower.ends_with(".csv")
        || lower.ends_with(".env")
        || lower == "dockerfile"
        || lower.ends_with(".md")
    {
        FilePreviewKind::Text
    } else {
        FilePreviewKind::Binary
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
    use protocol::parse_list_line;

    #[test]
    fn parse_list_protocol_line() {
        let name = B64.encode("hello world");
        let line = format!("f|12|1700000000|644|0|0|{name}|");
        let parsed = parse_list_line(&line).unwrap();
        assert_eq!(parsed.type_code, "f");
        assert_eq!(parsed.size, Some(12));
        assert_eq!(decode_name(&parsed.name_b64).unwrap(), "hello world");
    }

    #[test]
    fn helper_path_roundtrip() {
        let path = VolumePath::parse("/a/b").unwrap();
        assert_eq!(
            helper_to_volume_path(&path.helper_absolute()).unwrap(),
            path
        );
        assert!(helper_to_volume_path("/etc/passwd").is_err());
    }
}
