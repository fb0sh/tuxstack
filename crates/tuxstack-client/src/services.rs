//! High-level typed facade over the daemon protocol.
//!
//! This module intentionally mirrors resource-oriented frontend operations,
//! but every call is encoded as a typed [`tuxstack_protocol::Request`]. It
//! contains no Docker client, cache, event monitor, helper pool, archive body,
//! or Files backend.

use std::collections::HashMap;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use futures_util::Stream;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tuxstack_domain::*;
use tuxstack_protocol::{
    ComposeAction, DockerRequest, DockerResponse, ProtocolError, ProtocolErrorCode,
    PullImageRequest, RegistryAuthRequest, Request, ResourceKind, Response, ServerEvent,
    SubscriptionEndReason, SubscriptionRequest, TerminalState,
};

use crate::{Client, ClientError, Subscription};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum DaemonError {
    #[error("TuxStack service is unavailable: {0}")]
    DaemonUnavailable(String),
    #[error("control socket is unavailable: {0}")]
    SocketNotFound(PathBuf),
    #[error("Docker Engine is unavailable")]
    EngineUnavailable,
    #[error("connection timed out")]
    ConnectionTimeout,
    #[error("permission denied")]
    PermissionDenied,
    #[error("operation timed out")]
    OperationTimeout,
    #[error("operation cancelled")]
    OperationCancelled,
    #[error("container not found: {0}")]
    ContainerNotFound(String),
    #[error("image not found: {0}")]
    ImageNotFound(String),
    #[error("network not found: {0}")]
    NetworkNotFound(String),
    #[error("volume not found: {0}")]
    VolumeNotFound(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    #[error("invalid container configuration: {0}")]
    InvalidContainerConfig(String),
    #[error("invalid image reference: {0}")]
    InvalidImageReference(String),
    #[error("invalid network configuration: {0}")]
    InvalidNetworkConfig(String),
    #[error("invalid volume name: {0}")]
    InvalidVolumeName(String),
    #[error("invalid daemon response: {0}")]
    InvalidResponse(String),
    #[error("network is protected: {0}")]
    NetworkProtected(String),
    #[error("network is in use: {0}")]
    NetworkInUse(String),
    #[error("volume is in use: {0}")]
    VolumeInUse(String),
    #[error("volume already exists: {0}")]
    VolumeAlreadyExists(String),
    #[error("volume driver is unavailable: {0}")]
    VolumeDriverUnavailable(String),
    #[error("volume plugin failed: {0}")]
    VolumePluginError(String),
    #[error("registry authentication failed")]
    RegistryAuthenticationFailed,
    #[error("registry is unavailable: {0}")]
    RegistryUnavailable(String),
    #[error("image pull failed: {0}")]
    PullFailed(String),
    #[error("volume export failed: {0}")]
    ExportFailed(String),
    #[error("volume clone failed: {0}")]
    CloneFailed(String),
    #[error("operation cleanup failed: {0}")]
    CleanupFailed(String),
    #[error("destination permission denied: {0}")]
    DestinationPermissionDenied(PathBuf),
    #[error("destination is full: {0}")]
    DiskFull(PathBuf),
    #[error("daemon API failed: {0}")]
    Api(String),
    #[error("FUSE filesystem is unavailable: {0}")]
    FuseUnavailable(String),
    #[error("provider is unavailable: {0}")]
    ProviderUnavailable(String),
    #[error("daemon request failed: {0}")]
    Internal(String),
}

impl DaemonError {
    #[must_use]
    pub fn code(&self) -> ProtocolErrorCode {
        match self {
            Self::DaemonUnavailable(_) | Self::SocketNotFound(_) => {
                ProtocolErrorCode::DaemonUnavailable
            }
            Self::EngineUnavailable | Self::ConnectionTimeout => {
                ProtocolErrorCode::DockerUnavailable
            }
            Self::PermissionDenied => ProtocolErrorCode::PermissionDenied,
            Self::OperationTimeout => ProtocolErrorCode::OperationTimedOut,
            Self::OperationCancelled => ProtocolErrorCode::Cancelled,
            Self::ContainerNotFound(_)
            | Self::ImageNotFound(_)
            | Self::NetworkNotFound(_)
            | Self::VolumeNotFound(_) => ProtocolErrorCode::NotFound,
            Self::Conflict(_)
            | Self::NetworkProtected(_)
            | Self::NetworkInUse(_)
            | Self::VolumeInUse(_)
            | Self::VolumeAlreadyExists(_) => ProtocolErrorCode::Conflict,
            Self::InvalidRequest(_)
            | Self::InvalidContainerConfig(_)
            | Self::InvalidImageReference(_)
            | Self::InvalidNetworkConfig(_)
            | Self::InvalidVolumeName(_)
            | Self::InvalidResponse(_) => ProtocolErrorCode::InvalidRequest,
            Self::FuseUnavailable(_) => ProtocolErrorCode::FuseUnavailable,
            Self::ProviderUnavailable(_) => ProtocolErrorCode::ProviderUnavailable,
            Self::RegistryAuthenticationFailed | Self::DestinationPermissionDenied(_) => {
                ProtocolErrorCode::PermissionDenied
            }
            Self::RegistryUnavailable(_)
            | Self::VolumeDriverUnavailable(_)
            | Self::VolumePluginError(_) => ProtocolErrorCode::DockerUnavailable,
            Self::PullFailed(_)
            | Self::ExportFailed(_)
            | Self::CloneFailed(_)
            | Self::CleanupFailed(_)
            | Self::DiskFull(_)
            | Self::Api(_)
            | Self::Internal(_) => ProtocolErrorCode::Internal,
        }
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        matches!(self, Self::OperationCancelled)
    }
}

impl From<ClientError> for DaemonError {
    fn from(error: ClientError) -> Self {
        Self::DaemonUnavailable(error.to_string())
    }
}

#[derive(Debug, Clone, Copy)]
enum MissingResource {
    Container,
    Image,
    Network,
    Volume,
    Generic,
}

fn protocol_error(error: ProtocolError, missing: MissingResource) -> DaemonError {
    match error.code {
        ProtocolErrorCode::DaemonUnavailable => DaemonError::DaemonUnavailable(error.message),
        ProtocolErrorCode::DockerUnavailable => DaemonError::EngineUnavailable,
        ProtocolErrorCode::FuseUnavailable => DaemonError::FuseUnavailable(error.message),
        ProtocolErrorCode::ProviderUnavailable => DaemonError::ProviderUnavailable(error.message),
        ProtocolErrorCode::PermissionDenied => DaemonError::PermissionDenied,
        ProtocolErrorCode::OperationTimedOut => DaemonError::OperationTimeout,
        ProtocolErrorCode::Cancelled => DaemonError::OperationCancelled,
        ProtocolErrorCode::NotFound => match missing {
            MissingResource::Container => DaemonError::ContainerNotFound(error.message),
            MissingResource::Image => DaemonError::ImageNotFound(error.message),
            MissingResource::Network => DaemonError::NetworkNotFound(error.message),
            MissingResource::Volume => DaemonError::VolumeNotFound(error.message),
            MissingResource::Generic => DaemonError::Internal(error.message),
        },
        ProtocolErrorCode::Conflict | ProtocolErrorCode::ResourceBusy => {
            DaemonError::Conflict(error.message)
        }
        ProtocolErrorCode::InvalidRequest => DaemonError::InvalidRequest(error.message),
        _ => DaemonError::Internal(error.message),
    }
}

async fn docker_request(
    client: &Client,
    request: DockerRequest,
    missing: MissingResource,
) -> Result<DockerResponse, DaemonError> {
    match client.request(Request::Docker(Box::new(request))).await? {
        Response::Docker(response) => Ok(*response),
        Response::Error(error) => Err(protocol_error(error, missing)),
        response => Err(DaemonError::Internal(format!(
            "daemon returned an unexpected response: {response:?}"
        ))),
    }
}

async fn acknowledged(
    client: &Client,
    request: DockerRequest,
    missing: MissingResource,
) -> Result<(), DaemonError> {
    match docker_request(client, request, missing).await? {
        DockerResponse::Acknowledged => Ok(()),
        response => Err(DaemonError::Internal(format!(
            "daemon returned an unexpected acknowledgement: {response:?}"
        ))),
    }
}

#[derive(Clone)]
pub struct DaemonServices {
    pub system: SystemService,
    pub containers: ContainerService,
    pub images: ImageService,
    pub networks: NetworkService,
    pub volumes: VolumeService,
    pub compose: ComposeService,
    pub container_terminal: ContainerTerminalService,
    client: Arc<Client>,
}

impl DaemonServices {
    #[must_use]
    pub fn new(client: Arc<Client>) -> Self {
        Self {
            system: SystemService(client.clone()),
            containers: ContainerService(client.clone()),
            images: ImageService(client.clone()),
            networks: NetworkService(client.clone()),
            volumes: VolumeService(client.clone()),
            compose: ComposeService(client.clone()),
            container_terminal: ContainerTerminalService(client.clone()),
            client,
        }
    }

    #[must_use]
    pub fn client(&self) -> Arc<Client> {
        self.client.clone()
    }

    pub async fn resource_events(&self) -> Result<Subscription, DaemonError> {
        self.client
            .subscribe(SubscriptionRequest::ResourceChanges {
                kinds: vec![
                    ResourceKind::Container,
                    ResourceKind::Image,
                    ResourceKind::Network,
                    ResourceKind::Volume,
                ],
            })
            .await
            .map_err(Into::into)
    }
}

#[derive(Clone)]
pub struct SystemService(Arc<Client>);

impl SystemService {
    pub async fn ping(&self) -> Result<(), DaemonError> {
        self.0.ping().await.map_err(Into::into)
    }

    pub async fn system_info(&self) -> Result<DockerSystemInfo, DaemonError> {
        match docker_request(&self.0, DockerRequest::SystemInfo, MissingResource::Generic).await? {
            DockerResponse::SystemInfo(info) => Ok(info),
            other => unexpected(other),
        }
    }

    pub async fn overview(&self) -> Result<OverviewData, DaemonError> {
        match docker_request(&self.0, DockerRequest::Overview, MissingResource::Generic).await? {
            DockerResponse::Overview(overview) => Ok(overview),
            other => unexpected(other),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ListContainersOptions {
    pub all: bool,
    pub limit: Option<usize>,
    pub search: Option<String>,
    pub state: Option<ContainerRuntimeState>,
}

#[derive(Clone)]
pub struct ContainerService(Arc<Client>);

impl ContainerService {
    pub async fn list_containers(
        &self,
        options: &ListContainersOptions,
    ) -> Result<Vec<ContainerSummary>, DaemonError> {
        match docker_request(
            &self.0,
            DockerRequest::ListContainers {
                all: options.all,
                limit: options.limit,
                search: options.search.clone(),
                state: options.state,
            },
            MissingResource::Container,
        )
        .await?
        {
            DockerResponse::Containers(values) => Ok(values),
            other => unexpected(other),
        }
    }

    pub async fn inspect_container(&self, id: &str) -> Result<ContainerDetail, DaemonError> {
        match docker_request(
            &self.0,
            DockerRequest::InspectContainer { id: id.into() },
            MissingResource::Container,
        )
        .await?
        {
            DockerResponse::ContainerDetail(value) => Ok(value),
            other => unexpected(other),
        }
    }

    pub async fn create_container(
        &self,
        request: &CreateContainerRequest,
    ) -> Result<CreateContainerResult, DaemonError> {
        match docker_request(
            &self.0,
            DockerRequest::CreateContainer {
                request: Box::new(request.clone()),
            },
            MissingResource::Container,
        )
        .await?
        {
            DockerResponse::ContainerCreated(value) => Ok(value),
            other => unexpected(other),
        }
    }

    pub async fn start_container(&self, id: &str) -> Result<(), DaemonError> {
        acknowledged(
            &self.0,
            DockerRequest::StartContainer { id: id.into() },
            MissingResource::Container,
        )
        .await
    }

    pub async fn stop_container(
        &self,
        id: &str,
        options: Option<&StopContainerOptions>,
    ) -> Result<(), DaemonError> {
        acknowledged(
            &self.0,
            DockerRequest::StopContainer {
                id: id.into(),
                options: options.cloned().unwrap_or_default(),
            },
            MissingResource::Container,
        )
        .await
    }

    pub async fn restart_container_with_options(
        &self,
        id: &str,
        options: &RestartContainerOptions,
    ) -> Result<(), DaemonError> {
        acknowledged(
            &self.0,
            DockerRequest::RestartContainer {
                id: id.into(),
                options: options.clone(),
            },
            MissingResource::Container,
        )
        .await
    }

    pub async fn kill_container(&self, id: &str) -> Result<(), DaemonError> {
        acknowledged(
            &self.0,
            DockerRequest::KillContainer {
                id: id.into(),
                options: KillContainerOptions::default(),
            },
            MissingResource::Container,
        )
        .await
    }

    pub async fn pause_container(&self, id: &str) -> Result<(), DaemonError> {
        acknowledged(
            &self.0,
            DockerRequest::PauseContainer { id: id.into() },
            MissingResource::Container,
        )
        .await
    }

    pub async fn unpause_container(&self, id: &str) -> Result<(), DaemonError> {
        acknowledged(
            &self.0,
            DockerRequest::UnpauseContainer { id: id.into() },
            MissingResource::Container,
        )
        .await
    }

    pub async fn remove_container(
        &self,
        id: &str,
        options: &RemoveContainerOptions,
    ) -> Result<(), DaemonError> {
        acknowledged(
            &self.0,
            DockerRequest::RemoveContainer {
                id: id.into(),
                options: *options,
            },
            MissingResource::Container,
        )
        .await
    }

    pub async fn rename_container(&self, id: &str, new_name: &str) -> Result<(), DaemonError> {
        acknowledged(
            &self.0,
            DockerRequest::RenameContainer {
                id: id.into(),
                new_name: new_name.into(),
            },
            MissingResource::Container,
        )
        .await
    }

    pub async fn container_stats(&self, id: &str) -> Result<ContainerStats, DaemonError> {
        match docker_request(
            &self.0,
            DockerRequest::ContainerStats { id: id.into() },
            MissingResource::Container,
        )
        .await?
        {
            DockerResponse::ContainerStats(value) => Ok(value),
            other => unexpected(other),
        }
    }

    pub fn watch_stats(
        &self,
        id: &str,
        cancellation: CancellationToken,
    ) -> DaemonStream<ContainerStats> {
        let id = id.to_owned();
        subscription_stream(
            self.0.clone(),
            SubscriptionRequest::ContainerStats {
                container_ids: vec![id.clone()],
            },
            cancellation,
            move |event| match event {
                ServerEvent::ContainerStats {
                    container_id,
                    stats,
                    ..
                } if container_id == id => Some(Ok(stats)),
                _ => None,
            },
        )
    }

    pub fn watch_logs(
        &self,
        id: &str,
        options: &ContainerLogsOptions,
        cancellation: CancellationToken,
    ) -> DaemonStream<LogLine> {
        let id = id.to_owned();
        subscription_stream(
            self.0.clone(),
            SubscriptionRequest::ContainerLogs {
                container_ids: vec![id.clone()],
                options: options.clone(),
            },
            cancellation,
            move |event| match event {
                ServerEvent::ContainerLog {
                    container_id, line, ..
                } if container_id == id => Some(Ok(line)),
                _ => None,
            },
        )
    }
}

#[derive(Debug, Clone, Default)]
pub struct ListImagesOptions {
    pub search: Option<String>,
}

#[derive(Clone)]
pub struct ImageService(Arc<Client>);

impl ImageService {
    pub async fn list_images<O>(&self, options: O) -> Result<Vec<ImageSummary>, DaemonError>
    where
        O: std::borrow::Borrow<ListImagesOptions>,
    {
        match docker_request(
            &self.0,
            DockerRequest::ListImages {
                search: options.borrow().search.clone(),
            },
            MissingResource::Image,
        )
        .await?
        {
            DockerResponse::Images(values) => Ok(values),
            other => unexpected(other),
        }
    }

    pub async fn inspect_image(&self, id: &str) -> Result<ImageDetail, DaemonError> {
        match docker_request(
            &self.0,
            DockerRequest::InspectImage { id: id.into() },
            MissingResource::Image,
        )
        .await?
        {
            DockerResponse::ImageDetail(value) => Ok(value),
            other => unexpected(other),
        }
    }

    pub async fn remove_image(
        &self,
        id: &str,
        options: RemoveImageOptions,
    ) -> Result<Vec<ImageDeleteResult>, DaemonError> {
        match docker_request(
            &self.0,
            DockerRequest::RemoveImage {
                id: id.into(),
                options,
            },
            MissingResource::Image,
        )
        .await?
        {
            DockerResponse::ImagesRemoved(values) => Ok(values),
            other => unexpected(other),
        }
    }

    pub fn pull_image(&self, options: PullImageOptions) -> DaemonStream<ImagePullProgress> {
        subscription_stream(
            self.0.clone(),
            SubscriptionRequest::ImagePullAuthenticated {
                request: pull_request(options),
            },
            CancellationToken::new(),
            |event| match event {
                ServerEvent::ImagePullProgress { progress, .. } => Some(Ok(progress)),
                _ => None,
            },
        )
    }

    pub async fn export_image(&self, id: &str, destination: PathBuf) -> Result<u64, DaemonError> {
        match docker_request(
            &self.0,
            DockerRequest::ExportImage {
                id: id.into(),
                destination,
            },
            MissingResource::Image,
        )
        .await?
        {
            DockerResponse::ExportCompleted { bytes_written, .. } => Ok(bytes_written.unwrap_or(0)),
            other => unexpected(other),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ListNetworksOptions {
    pub search: Option<String>,
}

#[derive(Clone)]
pub struct NetworkService(Arc<Client>);

impl NetworkService {
    pub async fn list_networks(
        &self,
        options: &ListNetworksOptions,
    ) -> Result<Vec<NetworkSummary>, DaemonError> {
        match docker_request(
            &self.0,
            DockerRequest::ListNetworks {
                search: options.search.clone(),
            },
            MissingResource::Network,
        )
        .await?
        {
            DockerResponse::Networks(values) => Ok(values),
            other => unexpected(other),
        }
    }

    pub async fn inspect_network(&self, id: &str) -> Result<NetworkDetail, DaemonError> {
        match docker_request(
            &self.0,
            DockerRequest::InspectNetwork { id: id.into() },
            MissingResource::Network,
        )
        .await?
        {
            DockerResponse::NetworkDetail(value) => Ok(value),
            other => unexpected(other),
        }
    }

    pub async fn create_network(
        &self,
        options: CreateNetworkOptions,
    ) -> Result<CreateNetworkResult, DaemonError> {
        match docker_request(
            &self.0,
            DockerRequest::CreateNetwork { options },
            MissingResource::Network,
        )
        .await?
        {
            DockerResponse::NetworkCreated(value) => Ok(value),
            other => unexpected(other),
        }
    }

    pub async fn remove_network(&self, id: &str) -> Result<(), DaemonError> {
        acknowledged(
            &self.0,
            DockerRequest::RemoveNetwork { id: id.into() },
            MissingResource::Network,
        )
        .await
    }
}

#[derive(Debug, Clone, Default)]
pub struct ListVolumesOptions {
    pub search: Option<String>,
}

#[derive(Clone)]
pub struct VolumeService(Arc<Client>);

impl VolumeService {
    pub async fn list_volumes(
        &self,
        options: &ListVolumesOptions,
    ) -> Result<Vec<VolumeSummary>, DaemonError> {
        match docker_request(
            &self.0,
            DockerRequest::ListVolumes {
                search: options.search.clone(),
            },
            MissingResource::Volume,
        )
        .await?
        {
            DockerResponse::Volumes(values) => Ok(values),
            other => unexpected(other),
        }
    }

    pub async fn list_all_volumes(&self) -> Result<Vec<VolumeSummary>, DaemonError> {
        self.list_volumes(&ListVolumesOptions::default()).await
    }

    pub async fn list_volume_summaries(&self) -> Result<Vec<VolumeSummary>, DaemonError> {
        self.list_all_volumes().await
    }

    pub async fn enrich_usage(
        &self,
        volume_names: &[String],
    ) -> (
        HashMap<String, Vec<VolumeContainerReference>>,
        HashMap<String, VolumeUsage>,
    ) {
        match docker_request(
            &self.0,
            DockerRequest::EnrichVolumes {
                names: volume_names.to_vec(),
            },
            MissingResource::Volume,
        )
        .await
        {
            Ok(DockerResponse::VolumesEnriched(value)) => (
                value.references.into_iter().collect(),
                value.usage.into_iter().collect(),
            ),
            _ => (HashMap::new(), HashMap::new()),
        }
    }

    pub async fn inspect_volume(&self, name: &str) -> Result<VolumeDetail, DaemonError> {
        match docker_request(
            &self.0,
            DockerRequest::InspectVolume { name: name.into() },
            MissingResource::Volume,
        )
        .await?
        {
            DockerResponse::VolumeDetail(value) => Ok(value),
            other => unexpected(other),
        }
    }

    pub async fn create_volume(
        &self,
        request: CreateVolumeRequest,
    ) -> Result<VolumeDetail, DaemonError> {
        match docker_request(
            &self.0,
            DockerRequest::CreateVolume { request },
            MissingResource::Volume,
        )
        .await?
        {
            DockerResponse::VolumeDetail(value) => Ok(value),
            other => unexpected(other),
        }
    }

    pub async fn remove_volume(
        &self,
        name: &str,
        options: RemoveVolumeOptions,
    ) -> Result<(), DaemonError> {
        acknowledged(
            &self.0,
            DockerRequest::RemoveVolume {
                name: name.into(),
                options,
            },
            MissingResource::Volume,
        )
        .await
    }

    pub async fn prune_volumes(
        &self,
        filters: PruneVolumeFilters,
    ) -> Result<VolumePruneResult, DaemonError> {
        match docker_request(
            &self.0,
            DockerRequest::PruneVolumes { filters },
            MissingResource::Volume,
        )
        .await?
        {
            DockerResponse::VolumesPruned(value) => Ok(value),
            other => unexpected(other),
        }
    }

    pub async fn clone_volume(
        &self,
        request: CloneVolumeRequest,
        cancellation: CancellationToken,
    ) -> Result<VolumeDetail, DaemonError> {
        tokio::select! {
            _ = cancellation.cancelled() => Err(DaemonError::OperationCancelled),
            response = docker_request(
                &self.0,
                DockerRequest::CloneVolume { request },
                MissingResource::Volume,
            ) => match response? {
                DockerResponse::VolumeDetail(value) => Ok(value),
                other => unexpected(other),
            }
        }
    }

    pub async fn export_volume(
        &self,
        request: ExportVolumeRequest,
        cancellation: CancellationToken,
    ) -> Result<(), DaemonError> {
        tokio::select! {
            _ = cancellation.cancelled() => Err(DaemonError::OperationCancelled),
            response = docker_request(
                &self.0,
                DockerRequest::ExportVolume { request },
                MissingResource::Volume,
            ) => match response? {
                DockerResponse::ExportCompleted { .. } => Ok(()),
                other => unexpected(other),
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComposeGroupAction {
    Start,
    Stop(StopContainerOptions),
    Restart(RestartContainerOptions),
    Kill(KillContainerOptions),
    Pause,
    Unpause,
    Remove(RemoveContainerOptions),
}

impl From<ComposeGroupAction> for ComposeAction {
    fn from(value: ComposeGroupAction) -> Self {
        match value {
            ComposeGroupAction::Start => Self::Start,
            ComposeGroupAction::Stop(options) => Self::Stop(options),
            ComposeGroupAction::Restart(options) => Self::Restart(options),
            ComposeGroupAction::Kill(options) => Self::Kill(options),
            ComposeGroupAction::Pause => Self::Pause,
            ComposeGroupAction::Unpause => Self::Unpause,
            ComposeGroupAction::Remove(options) => Self::Remove(options),
        }
    }
}

#[derive(Clone)]
pub struct ComposeService(Arc<Client>);

impl ComposeService {
    pub async fn list_projects(&self) -> Result<Vec<ContainerGroupSummary>, DaemonError> {
        match docker_request(
            &self.0,
            DockerRequest::ListComposeProjects,
            MissingResource::Container,
        )
        .await?
        {
            DockerResponse::ComposeProjects(values) => Ok(values),
            other => unexpected(other),
        }
    }

    pub async fn execute_group_targets(
        &self,
        group_id: &ContainerGroupId,
        target_ids: &[String],
        action: ComposeGroupAction,
    ) -> Result<ContainerGroupOperationResult, DaemonError> {
        match docker_request(
            &self.0,
            DockerRequest::ExecuteComposeTargets {
                group_id: group_id.clone(),
                target_ids: target_ids.to_vec(),
                action: action.into(),
            },
            MissingResource::Container,
        )
        .await?
        {
            DockerResponse::ComposeOperation(value) => Ok(value),
            other => unexpected(other),
        }
    }
}

pub struct DaemonStream<T> {
    receiver: mpsc::Receiver<Result<T, DaemonError>>,
    cancellation: CancellationToken,
}

impl<T> DaemonStream<T> {
    #[must_use]
    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancellation.clone()
    }
}

impl<T> Stream for DaemonStream<T> {
    type Item = Result<T, DaemonError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.receiver.poll_recv(cx)
    }
}

impl<T> Drop for DaemonStream<T> {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}

fn subscription_stream<T, F>(
    client: Arc<Client>,
    request: SubscriptionRequest,
    cancellation: CancellationToken,
    mut convert: F,
) -> DaemonStream<T>
where
    T: Send + 'static,
    F: FnMut(ServerEvent) -> Option<Result<T, DaemonError>> + Send + 'static,
{
    let (sender, receiver) = mpsc::channel(256);
    let task_cancel = cancellation.clone();
    tokio::spawn(async move {
        let mut subscription = match client.subscribe(request).await {
            Ok(subscription) => subscription,
            Err(error) => {
                let _ = sender.send(Err(error.into())).await;
                return;
            }
        };
        loop {
            let event = tokio::select! {
                _ = task_cancel.cancelled() => break,
                event = subscription.recv() => event,
            };
            let Some(event) = event else {
                let _ = sender
                    .send(Err(DaemonError::DaemonUnavailable(
                        "subscription transport closed".into(),
                    )))
                    .await;
                break;
            };
            if let ServerEvent::SubscriptionEnded { reason, .. } = &event {
                if let Some(error) = subscription_end_error(reason.clone()) {
                    let _ = sender.send(Err(error)).await;
                }
                break;
            }
            if let Some(value) = convert(event)
                && sender.send(value).await.is_err()
            {
                break;
            }
        }
        drop(subscription);
    });
    DaemonStream {
        receiver,
        cancellation,
    }
}

fn pull_request(options: PullImageOptions) -> PullImageRequest {
    PullImageRequest {
        reference: options.reference,
        platform: options.platform,
        registry_auth: options.registry_auth.map(|auth| RegistryAuthRequest {
            username: auth.username,
            password: auth.password,
            server_address: auth.server_address,
            identity_token: auth.identity_token,
            registry_token: auth.registry_token,
        }),
    }
}

fn subscription_end_error(reason: SubscriptionEndReason) -> Option<DaemonError> {
    match reason {
        SubscriptionEndReason::Completed | SubscriptionEndReason::Unsubscribed => None,
        SubscriptionEndReason::ServerShutdown => {
            Some(DaemonError::DaemonUnavailable("daemon stopped".into()))
        }
        SubscriptionEndReason::ResourceRemoved => {
            Some(DaemonError::Internal("resource was removed".into()))
        }
        SubscriptionEndReason::Error(error) => {
            Some(protocol_error(error, MissingResource::Generic))
        }
        _ => Some(DaemonError::Internal("subscription ended".into())),
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ContainerTerminalOptions;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerTerminalState {
    Idle,
    Connecting,
    Ready,
    Exited,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ContainerTerminalError {
    #[error("container is not running")]
    NotRunning,
    #[error("container is paused")]
    Paused,
    #[error("no shell was found")]
    ShellNotFound,
    #[error("terminal create failed")]
    CreateFailed,
    #[error("terminal start failed")]
    StartFailed,
    #[error("terminal disconnected")]
    Disconnected,
    #[error("terminal resize failed")]
    ResizeFailed,
    #[error("terminal operation timed out")]
    Timeout,
    #[error("terminal operation cancelled")]
    Cancelled,
    #[error("invalid terminal options")]
    InvalidOptions,
    #[error("terminal permission denied")]
    Permission,
    #[error("Docker Engine is unavailable")]
    DockerUnavailable,
}

fn terminal_error(error: DaemonError) -> ContainerTerminalError {
    match error {
        DaemonError::PermissionDenied => ContainerTerminalError::Permission,
        DaemonError::OperationTimeout => ContainerTerminalError::Timeout,
        DaemonError::OperationCancelled => ContainerTerminalError::Cancelled,
        DaemonError::InvalidRequest(_) => ContainerTerminalError::InvalidOptions,
        DaemonError::Conflict(message) if message.to_ascii_lowercase().contains("paused") => {
            ContainerTerminalError::Paused
        }
        DaemonError::Conflict(message) if message.to_ascii_lowercase().contains("running") => {
            ContainerTerminalError::NotRunning
        }
        DaemonError::EngineUnavailable | DaemonError::DaemonUnavailable(_) => {
            ContainerTerminalError::DockerUnavailable
        }
        _ => ContainerTerminalError::Disconnected,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContainerTerminalOutput {
    Console(Vec<u8>),
}

pub struct ContainerTerminalOutputStream {
    receiver: mpsc::Receiver<Result<ContainerTerminalOutput, ContainerTerminalError>>,
    cancellation: CancellationToken,
}

impl Stream for ContainerTerminalOutputStream {
    type Item = Result<ContainerTerminalOutput, ContainerTerminalError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.receiver.poll_recv(cx)
    }
}

impl Drop for ContainerTerminalOutputStream {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContainerTerminalStatus {
    pub running: bool,
    pub exit_code: Option<i64>,
}

#[derive(Clone)]
pub struct ContainerTerminalService(Arc<Client>);

impl ContainerTerminalService {
    pub async fn connect(
        &self,
        container_id: &str,
        _options: ContainerTerminalOptions,
        cancellation: CancellationToken,
    ) -> Result<ContainerTerminalSession, ContainerTerminalError> {
        let mut subscription = self
            .0
            .subscribe(SubscriptionRequest::ContainerTerminal {
                container_id: container_id.into(),
                rows: 24,
                cols: 80,
            })
            .await
            .map_err(|error| terminal_error(error.into()))?;
        let subscription_id = subscription.id();
        let mut early_output = Vec::new();
        let shell = loop {
            let event = tokio::select! {
                _ = cancellation.cancelled() => return Err(ContainerTerminalError::Cancelled),
                event = subscription.recv() => event.ok_or(ContainerTerminalError::Disconnected)?,
            };
            match event {
                ServerEvent::TerminalState {
                    state: TerminalState::Running { shell },
                    ..
                } => break shell,
                ServerEvent::TerminalState {
                    state: TerminalState::Failed { reason },
                    ..
                } => {
                    return Err(terminal_error(DaemonError::Conflict(reason)));
                }
                ServerEvent::TerminalOutput { bytes, .. } => early_output.push(bytes),
                ServerEvent::SubscriptionEnded { reason, .. } => {
                    return Err(subscription_end_error(reason)
                        .map(terminal_error)
                        .unwrap_or(ContainerTerminalError::Disconnected));
                }
                _ => {}
            }
        };
        let (sender, receiver) = mpsc::channel(256);
        for bytes in early_output {
            let _ = sender
                .send(Ok(ContainerTerminalOutput::Console(bytes)))
                .await;
        }
        let state = Arc::new(AtomicU8::new(2));
        let status = Arc::new(Mutex::new(ContainerTerminalStatus {
            running: true,
            exit_code: None,
        }));
        let task_state = state.clone();
        let task_status = status.clone();
        let task_cancel = cancellation.clone();
        tokio::spawn(async move {
            loop {
                let event = tokio::select! {
                    _ = task_cancel.cancelled() => break,
                    event = subscription.recv() => event,
                };
                match event {
                    Some(ServerEvent::TerminalOutput { bytes, .. }) => {
                        if sender
                            .send(Ok(ContainerTerminalOutput::Console(bytes)))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    Some(ServerEvent::TerminalState {
                        state: TerminalState::Exited { exit_code },
                        ..
                    }) => {
                        task_state.store(3, Ordering::Release);
                        *lock(&task_status) = ContainerTerminalStatus {
                            running: false,
                            exit_code,
                        };
                    }
                    Some(ServerEvent::TerminalState {
                        state: TerminalState::Failed { reason },
                        ..
                    }) => {
                        task_state.store(4, Ordering::Release);
                        let _ = sender
                            .send(Err(terminal_error(DaemonError::Conflict(reason))))
                            .await;
                    }
                    Some(ServerEvent::SubscriptionEnded { reason, .. }) => {
                        if let Some(error) = subscription_end_error(reason) {
                            task_state.store(4, Ordering::Release);
                            let _ = sender.send(Err(terminal_error(error))).await;
                        }
                        break;
                    }
                    Some(_) => {}
                    None => {
                        task_state.store(4, Ordering::Release);
                        let _ = sender.send(Err(ContainerTerminalError::Disconnected)).await;
                        break;
                    }
                }
            }
        });
        Ok(ContainerTerminalSession {
            client: self.0.clone(),
            subscription_id,
            shell,
            state,
            status,
            cancellation,
            output: tokio::sync::Mutex::new(Some(ContainerTerminalOutputStream {
                receiver,
                cancellation: CancellationToken::new(),
            })),
        })
    }
}

pub struct ContainerTerminalSession {
    client: Arc<Client>,
    subscription_id: u64,
    shell: String,
    state: Arc<AtomicU8>,
    status: Arc<Mutex<ContainerTerminalStatus>>,
    cancellation: CancellationToken,
    output: tokio::sync::Mutex<Option<ContainerTerminalOutputStream>>,
}

impl ContainerTerminalSession {
    #[must_use]
    pub fn shell(&self) -> &str {
        &self.shell
    }

    #[must_use]
    pub fn state(&self) -> ContainerTerminalState {
        match self.state.load(Ordering::Acquire) {
            1 => ContainerTerminalState::Connecting,
            2 => ContainerTerminalState::Ready,
            3 => ContainerTerminalState::Exited,
            4 => ContainerTerminalState::Error,
            _ => ContainerTerminalState::Idle,
        }
    }

    pub async fn take_output(
        &self,
    ) -> Result<ContainerTerminalOutputStream, ContainerTerminalError> {
        self.output
            .lock()
            .await
            .take()
            .ok_or(ContainerTerminalError::Disconnected)
    }

    pub async fn write_input(&self, bytes: Vec<u8>) -> Result<(), ContainerTerminalError> {
        request_ack(
            &self.client,
            Request::ContainerTerminalInput {
                subscription_id: self.subscription_id,
                bytes,
            },
        )
        .await
    }

    pub async fn resize(&self, rows: u16, cols: u16) -> Result<(), ContainerTerminalError> {
        request_ack(
            &self.client,
            Request::ContainerTerminalResize {
                subscription_id: self.subscription_id,
                rows,
                cols,
            },
        )
        .await
        .map_err(|error| match error {
            ContainerTerminalError::Disconnected => ContainerTerminalError::ResizeFailed,
            other => other,
        })
    }

    pub async fn inspect(&self) -> Result<ContainerTerminalStatus, ContainerTerminalError> {
        Ok(*lock(&self.status))
    }

    pub async fn close(&self) {
        self.cancellation.cancel();
        let _ = request_ack(
            &self.client,
            Request::ContainerTerminalClose {
                subscription_id: self.subscription_id,
            },
        )
        .await;
        self.state.store(3, Ordering::Release);
        lock(&self.status).running = false;
    }
}

async fn request_ack(client: &Client, request: Request) -> Result<(), ContainerTerminalError> {
    match client.request(request).await {
        Ok(Response::Acknowledged) => Ok(()),
        Ok(Response::Error(error)) => Err(terminal_error(protocol_error(
            error,
            MissingResource::Container,
        ))),
        Ok(_) => Err(ContainerTerminalError::Disconnected),
        Err(error) => Err(terminal_error(error.into())),
    }
}

fn unexpected<T>(response: DockerResponse) -> Result<T, DaemonError> {
    Err(DaemonError::Internal(format!(
        "daemon returned an unexpected Docker response: {response:?}"
    )))
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_codes_map_without_docker_adapter_types() {
        let error = protocol_error(
            ProtocolError {
                code: ProtocolErrorCode::NotFound,
                message: "gone".into(),
                retryable: false,
            },
            MissingResource::Image,
        );
        assert!(matches!(error, DaemonError::ImageNotFound(_)));
        assert_eq!(error.code(), ProtocolErrorCode::NotFound);
    }

    #[test]
    fn compose_actions_remain_typed() {
        assert_eq!(
            ComposeAction::from(ComposeGroupAction::Pause),
            ComposeAction::Pause
        );
    }
}
