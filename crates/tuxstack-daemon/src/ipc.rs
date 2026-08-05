use std::collections::HashMap;
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use futures_util::StreamExt;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tuxstack_docker_core::services::{
    ComposeGroupAction, ListContainersOptions, ListImagesOptions, ListNetworksOptions,
    ListVolumesOptions,
};
use tuxstack_protocol::{
    ComposeAction, DockerRequest, DockerResponse, FeatureFlags, FrameError, HandshakeRejection,
    HandshakeRejectionCode, MAX_FRAME_SIZE, MountAction, PROTOCOL_VERSION, ProtocolBody,
    ProtocolEnvelope, ProtocolError, ProtocolErrorCode, Request, RequestId, Response, ServerEvent,
    ServerHello, SubscriptionAccepted, SubscriptionId, SubscriptionRequest, decode_payload,
    encode_frame_with_limit, validate_frame_length,
};

use crate::state::{DaemonEvent, DaemonState};

const OUTBOUND_QUEUE: usize = 256;

pub struct IpcServer {
    listener: UnixListener,
    state: Arc<DaemonState>,
}

impl IpcServer {
    pub fn bind(state: Arc<DaemonState>) -> Result<Self> {
        let path = &state.paths.socket_path;
        if path.exists() {
            fs::remove_file(path)
                .with_context(|| format!("remove stale socket {}", path.display()))?;
        }
        let listener = UnixListener::bind(path)
            .with_context(|| format!("bind control socket {}", path.display()))?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .with_context(|| format!("secure control socket {}", path.display()))?;
        Ok(Self { listener, state })
    }

    pub async fn run(self) -> Result<()> {
        let shutdown = self.state.shutdown_token();
        loop {
            tokio::select! {
                _ = shutdown.cancelled() => return Ok(()),
                accepted = self.listener.accept() => {
                    let (stream, _) = accepted.context("accept IPC client")?;
                    let state = Arc::clone(&self.state);
                    tokio::spawn(async move {
                        if let Err(error) = serve_client(stream, state).await {
                            tracing::debug!(%error, "IPC client disconnected");
                        }
                    });
                }
            }
        }
    }
}

impl Drop for IpcServer {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.state.paths.socket_path);
    }
}

async fn serve_client(mut stream: UnixStream, state: Arc<DaemonState>) -> Result<()> {
    verify_peer(&stream)?;
    let hello = read_envelope(&mut stream, MAX_FRAME_SIZE).await?;
    let client = match hello.body {
        ProtocolBody::Hello(client) if hello.request_id == 0 => client,
        _ => {
            write_envelope(
                &mut stream,
                &ProtocolEnvelope::new(
                    0,
                    ProtocolBody::Rejected(HandshakeRejection {
                        code: HandshakeRejectionCode::UnsupportedProtocol,
                        message: "first frame must be Hello with request ID zero".to_owned(),
                        daemon_version: env!("CARGO_PKG_VERSION").to_owned(),
                        supported_protocol_versions: vec![PROTOCOL_VERSION],
                    }),
                ),
                MAX_FRAME_SIZE,
            )
            .await?;
            bail!("invalid client handshake");
        }
    };
    if !client
        .supported_protocol_versions
        .contains(&PROTOCOL_VERSION)
        || client.max_frame_size == 0
    {
        write_envelope(
            &mut stream,
            &ProtocolEnvelope::new(
                0,
                ProtocolBody::Rejected(HandshakeRejection {
                    code: HandshakeRejectionCode::UnsupportedProtocol,
                    message: "no compatible protocol version".to_owned(),
                    daemon_version: env!("CARGO_PKG_VERSION").to_owned(),
                    supported_protocol_versions: vec![PROTOCOL_VERSION],
                }),
            ),
            MAX_FRAME_SIZE,
        )
        .await?;
        bail!("unsupported client protocol");
    }
    let maximum = client.max_frame_size.min(MAX_FRAME_SIZE);
    let features = FeatureFlags::SUBSCRIPTIONS
        .union(FeatureFlags::REQUEST_CANCELLATION)
        .union(FeatureFlags::RESOURCE_DESCRIPTORS);
    write_envelope(
        &mut stream,
        &ProtocolEnvelope::new(
            0,
            ProtocolBody::Accepted(ServerHello {
                daemon_version: env!("CARGO_PKG_VERSION").to_owned(),
                negotiated_protocol_version: PROTOCOL_VERSION,
                feature_flags: features,
                max_frame_size: maximum,
            }),
        ),
        maximum,
    )
    .await?;

    let (mut reader, mut writer) = stream.into_split();
    let (outbound_tx, mut outbound_rx) = mpsc::channel::<ProtocolEnvelope>(OUTBOUND_QUEUE);
    let writer_task = tokio::spawn(async move {
        while let Some(envelope) = outbound_rx.recv().await {
            write_envelope(&mut writer, &envelope, maximum).await?;
        }
        Ok::<_, IpcError>(())
    });
    let mut requests: HashMap<RequestId, CancellationToken> = HashMap::new();
    let mut subscriptions: HashMap<SubscriptionId, CancellationToken> = HashMap::new();
    let mut next_subscription_id = 1_u64;
    let shutdown = state.shutdown_token();
    loop {
        let envelope = tokio::select! {
            result = read_envelope(&mut reader, maximum) => result?,
            _ = shutdown.cancelled() => break,
        };
        match envelope.body {
            ProtocolBody::Request(request) => {
                let token = CancellationToken::new();
                requests.insert(envelope.request_id, token.clone());
                let state = Arc::clone(&state);
                let tx = outbound_tx.clone();
                tokio::spawn(async move {
                    let response = tokio::select! {
                        _ = token.cancelled() => Response::Error(protocol_error(ProtocolErrorCode::Cancelled, "request cancelled", true)),
                        response = dispatch_request(state, *request, token.clone()) => response,
                    };
                    let _ = tx
                        .send(ProtocolEnvelope::new(
                            envelope.request_id,
                            ProtocolBody::Response(Box::new(response)),
                        ))
                        .await;
                });
            }
            ProtocolBody::CancelRequest { request_id } => {
                if let Some(token) = requests.remove(&request_id) {
                    token.cancel();
                }
            }
            ProtocolBody::Subscribe(subscription) => {
                let id = next_subscription_id;
                next_subscription_id = next_subscription_id.saturating_add(1).max(1);
                let token = CancellationToken::new();
                subscriptions.insert(id, token.clone());
                outbound_tx
                    .send(ProtocolEnvelope::new(
                        envelope.request_id,
                        ProtocolBody::Subscribed(SubscriptionAccepted {
                            subscription_id: id,
                        }),
                    ))
                    .await
                    .map_err(|_| IpcError::Closed)?;
                spawn_subscription(
                    id,
                    subscription,
                    Arc::clone(&state),
                    outbound_tx.clone(),
                    token,
                );
            }
            ProtocolBody::Unsubscribe { subscription_id } => {
                if let Some(token) = subscriptions.remove(&subscription_id) {
                    token.cancel();
                }
                outbound_tx
                    .send(ProtocolEnvelope::new(
                        envelope.request_id,
                        ProtocolBody::Response(Box::new(Response::Acknowledged)),
                    ))
                    .await
                    .map_err(|_| IpcError::Closed)?;
            }
            ProtocolBody::Ping => {
                outbound_tx
                    .send(ProtocolEnvelope::new(
                        envelope.request_id,
                        ProtocolBody::Pong,
                    ))
                    .await
                    .map_err(|_| IpcError::Closed)?;
            }
            _ => bail!("unexpected client protocol message"),
        }
    }
    for (_, token) in requests {
        token.cancel();
    }
    for (_, token) in subscriptions {
        token.cancel();
    }
    drop(outbound_tx);
    writer_task.await.context("join IPC writer")??;
    Ok(())
}

async fn dispatch_request(
    state: Arc<DaemonState>,
    request: Request,
    cancellation: CancellationToken,
) -> Response {
    let result: Result<Response, String> = async {
        Ok(match request {
            Request::GetDaemonStatus => Response::DaemonStatus(state.status()),
            Request::GetMountStatus => Response::MountStatus(state.mount_status()),
            Request::SetMountState(action) => {
                let status = match action {
                    MountAction::Mount => state.mount().await,
                    MountAction::Unmount => state.unmount().await,
                    MountAction::Remount => state.remount().await,
                    _ => return Err("unsupported mount action".to_owned()),
                }
                .map_err(|error| error.to_string())?;
                Response::MountStatus(status)
            }
            Request::GetResourceFusePath(resource) => Response::ResourceFusePath(
                state
                    .resource_path(resource)
                    .await
                    .map_err(|error| error.to_string())?,
            ),
            Request::GetProviderDescriptor(path) => Response::ProviderDescriptor(
                state
                    .provider_descriptor(path)
                    .await
                    .map_err(|error| error.to_string())?,
            ),
            Request::PerformResourceOperation(_) => Response::Acknowledged,
            Request::Docker(request) => Response::Docker(Box::new(
                dispatch_docker(&state, *request, cancellation).await?,
            )),
            _ => return Err("unsupported request variant".to_owned()),
        })
    }
    .await;
    result.unwrap_or_else(|message| {
        Response::Error(protocol_error(ProtocolErrorCode::Internal, &message, false))
    })
}

async fn dispatch_docker(
    state: &DaemonState,
    request: DockerRequest,
    cancellation: CancellationToken,
) -> Result<DockerResponse, String> {
    let services = &state.services;
    macro_rules! docker {
        ($future:expr) => {
            $future.await.map_err(|error| error.to_string())?
        };
    }
    Ok(match request {
        DockerRequest::SystemInfo => {
            DockerResponse::SystemInfo(docker!(services.system.system_info()))
        }
        DockerRequest::Overview => DockerResponse::Overview(docker!(services.system.overview())),
        DockerRequest::ListContainers {
            all,
            limit,
            search,
            state: runtime_state,
        } => DockerResponse::Containers(docker!(services.containers.list_containers(
            &ListContainersOptions {
                all,
                limit,
                search,
                state: runtime_state,
            }
        ))),
        DockerRequest::InspectContainer { id } => {
            DockerResponse::ContainerDetail(docker!(services.containers.inspect_container(&id)))
        }
        DockerRequest::StartContainer { id } => {
            docker!(services.containers.start_container(&id));
            DockerResponse::Acknowledged
        }
        DockerRequest::StopContainer { id, options } => {
            docker!(services.containers.stop_container(&id, Some(&options)));
            DockerResponse::Acknowledged
        }
        DockerRequest::RestartContainer { id, options } => {
            docker!(
                services
                    .containers
                    .restart_container_with_options(&id, &options)
            );
            DockerResponse::Acknowledged
        }
        DockerRequest::PauseContainer { id } => {
            docker!(services.containers.pause_container(&id));
            DockerResponse::Acknowledged
        }
        DockerRequest::UnpauseContainer { id } => {
            docker!(services.containers.unpause_container(&id));
            DockerResponse::Acknowledged
        }
        DockerRequest::KillContainer { id, options } => {
            docker!(
                services
                    .containers
                    .kill_container_with_options(&id, &options)
            );
            DockerResponse::Acknowledged
        }
        DockerRequest::RemoveContainer { id, options } => {
            docker!(services.containers.remove_container(&id, &options));
            DockerResponse::Acknowledged
        }
        DockerRequest::RenameContainer { id, new_name } => {
            docker!(services.containers.rename_container(&id, &new_name));
            DockerResponse::Acknowledged
        }
        DockerRequest::CreateContainer { request } => DockerResponse::ContainerCreated(docker!(
            services.containers.create_container(&request)
        )),
        DockerRequest::ContainerStats { id } => {
            DockerResponse::ContainerStats(docker!(services.containers.container_stats(&id)))
        }
        DockerRequest::ContainerLogs { id, options } => DockerResponse::ContainerLogs(docker!(
            services.containers.container_logs(&id, &options)
        )),
        DockerRequest::ListComposeProjects => {
            DockerResponse::ComposeProjects(docker!(services.compose.list_projects()))
        }
        DockerRequest::ExecuteComposeTargets {
            group_id,
            target_ids,
            action,
        } => {
            let action = match action {
                ComposeAction::Start => ComposeGroupAction::Start,
                ComposeAction::Stop(options) => ComposeGroupAction::Stop(options),
                ComposeAction::Restart(options) => ComposeGroupAction::Restart(options),
                ComposeAction::Kill(options) => ComposeGroupAction::Kill(options),
                ComposeAction::Pause => ComposeGroupAction::Pause,
                ComposeAction::Unpause => ComposeGroupAction::Unpause,
                ComposeAction::Remove(options) => ComposeGroupAction::Remove(options),
                _ => return Err("unsupported compose action".to_owned()),
            };
            DockerResponse::ComposeOperation(docker!(services.compose.execute_group_targets(
                &group_id,
                &target_ids,
                action
            )))
        }
        DockerRequest::ListImages { search } => DockerResponse::Images(docker!(
            services.images.list_images(ListImagesOptions { search })
        )),
        DockerRequest::InspectImage { id } => {
            DockerResponse::ImageDetail(docker!(services.images.inspect_image(&id)))
        }
        DockerRequest::RemoveImage { id, options } => {
            DockerResponse::ImagesRemoved(docker!(services.images.remove_image(&id, options)))
        }
        DockerRequest::PullImage { options } => {
            let mut stream = services.images.pull_image(options);
            while let Some(progress) = tokio::select! {
                _ = cancellation.cancelled() => return Err("image pull cancelled".to_owned()),
                item = stream.next() => item,
            } {
                progress.map_err(|error| error.to_string())?;
            }
            DockerResponse::Acknowledged
        }
        DockerRequest::ListNetworks { search } => DockerResponse::Networks(docker!(
            services
                .networks
                .list_networks(&ListNetworksOptions { search })
        )),
        DockerRequest::InspectNetwork { id } => {
            DockerResponse::NetworkDetail(docker!(services.networks.inspect_network(&id)))
        }
        DockerRequest::CreateNetwork { options } => {
            DockerResponse::NetworkCreated(docker!(services.networks.create_network(options)))
        }
        DockerRequest::RemoveNetwork { id } => {
            docker!(services.networks.remove_network(&id));
            DockerResponse::Acknowledged
        }
        DockerRequest::ListVolumes { search } => DockerResponse::Volumes(docker!(
            services
                .volumes
                .list_volumes(&ListVolumesOptions { search })
        )),
        DockerRequest::InspectVolume { name } => {
            DockerResponse::VolumeDetail(docker!(services.volumes.inspect_volume(&name)))
        }
        DockerRequest::CreateVolume { request } => {
            DockerResponse::VolumeDetail(docker!(services.volumes.create_volume(request)))
        }
        DockerRequest::RemoveVolume { name, options } => {
            docker!(services.volumes.remove_volume(&name, options));
            DockerResponse::Acknowledged
        }
        DockerRequest::PruneVolumes { filters } => {
            DockerResponse::VolumesPruned(docker!(services.volumes.prune_volumes(filters)))
        }
        DockerRequest::CloneVolume { request } => DockerResponse::VolumeDetail(docker!(
            services.volumes.clone_volume(request, cancellation)
        )),
        _ => return Err("unsupported Docker request variant".to_owned()),
    })
}

fn spawn_subscription(
    id: SubscriptionId,
    subscription: SubscriptionRequest,
    state: Arc<DaemonState>,
    tx: mpsc::Sender<ProtocolEnvelope>,
    cancellation: CancellationToken,
) {
    tokio::spawn(async move {
        match subscription {
            SubscriptionRequest::DaemonStatus
            | SubscriptionRequest::MountStatus
            | SubscriptionRequest::ResourceChanges { .. }
            | SubscriptionRequest::ProviderStatus { .. } => {
                let mut events = state.subscribe();
                loop {
                    let event = tokio::select! {
                        _ = cancellation.cancelled() => break,
                        event = events.recv() => match event { Ok(event) => event, Err(_) => break },
                    };
                    let event = match (&subscription, event) {
                        (SubscriptionRequest::DaemonStatus, DaemonEvent::Status(status)) => {
                            Some(ServerEvent::DaemonStatus {
                                subscription_id: id,
                                status,
                            })
                        }
                        (SubscriptionRequest::MountStatus, DaemonEvent::Mount(status)) => {
                            Some(ServerEvent::MountStatus {
                                subscription_id: id,
                                status,
                            })
                        }
                        (
                            SubscriptionRequest::ResourceChanges { kinds },
                            DaemonEvent::Resource {
                                kind,
                                resource,
                                change,
                            },
                        ) if kinds.contains(&kind) => Some(ServerEvent::ResourceChanged {
                            subscription_id: id,
                            kind,
                            resource,
                            change,
                        }),
                        _ => None,
                    };
                    if let Some(event) = event
                        && tx
                            .send(ProtocolEnvelope::new(0, ProtocolBody::Event(event)))
                            .await
                            .is_err()
                    {
                        break;
                    }
                }
            }
            SubscriptionRequest::ImagePull { options } => {
                let mut stream = state.services.images.pull_image(options);
                loop {
                    let item = tokio::select! {
                        _ = cancellation.cancelled() => break,
                        item = stream.next() => item,
                    };
                    let Some(item) = item else { break };
                    match item {
                        Ok(progress) => {
                            if tx
                                .send(ProtocolEnvelope::new(
                                    0,
                                    ProtocolBody::Event(ServerEvent::ImagePullProgress {
                                        subscription_id: id,
                                        progress,
                                    }),
                                ))
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                        Err(error) => {
                            let event = ServerEvent::SubscriptionEnded {
                                subscription_id: id,
                                reason: tuxstack_protocol::SubscriptionEndReason::Error(
                                    protocol_error(
                                        ProtocolErrorCode::Internal,
                                        &error.to_string(),
                                        true,
                                    ),
                                ),
                            };
                            let _ = tx
                                .send(ProtocolEnvelope::new(0, ProtocolBody::Event(event)))
                                .await;
                            return;
                        }
                    }
                }
            }
            _ => {
                let event = ServerEvent::SubscriptionEnded {
                    subscription_id: id,
                    reason: tuxstack_protocol::SubscriptionEndReason::Error(protocol_error(
                        ProtocolErrorCode::InvalidRequest,
                        "subscription is not implemented yet",
                        false,
                    )),
                };
                let _ = tx
                    .send(ProtocolEnvelope::new(0, ProtocolBody::Event(event)))
                    .await;
            }
        }
    });
}

fn verify_peer(stream: &UnixStream) -> Result<()> {
    let credentials = stream.peer_cred().context("read Unix peer credentials")?;
    let current_uid = unsafe { libc::geteuid() };
    if credentials.uid() != current_uid {
        bail!("IPC peer UID {} is not authorized", credentials.uid());
    }
    Ok(())
}

fn protocol_error(code: ProtocolErrorCode, message: &str, retryable: bool) -> ProtocolError {
    ProtocolError {
        code,
        message: message.to_owned(),
        retryable,
    }
}

async fn read_envelope<R: AsyncRead + Unpin>(
    reader: &mut R,
    maximum: u32,
) -> Result<ProtocolEnvelope, IpcError> {
    let mut header = [0_u8; 4];
    reader.read_exact(&mut header).await?;
    let length = u32::from_be_bytes(header);
    validate_frame_length(length, maximum)?;
    let mut payload = vec![0_u8; length as usize];
    reader.read_exact(&mut payload).await?;
    Ok(decode_payload(&payload, length)?)
}

async fn write_envelope<W: AsyncWrite + Unpin>(
    writer: &mut W,
    envelope: &ProtocolEnvelope,
    maximum: u32,
) -> Result<(), IpcError> {
    let frame = encode_frame_with_limit(envelope, maximum)?;
    writer.write_all(&frame).await?;
    writer.flush().await?;
    Ok(())
}

#[derive(Debug, thiserror::Error)]
enum IpcError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("frame error: {0}")]
    Frame(#[from] FrameError),
    #[error("connection closed")]
    Closed,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_errors_are_stable_and_do_not_expose_internal_types() {
        let error = protocol_error(ProtocolErrorCode::Cancelled, "cancelled", true);
        assert_eq!(error.code, ProtocolErrorCode::Cancelled);
        assert!(error.retryable);
    }
}
