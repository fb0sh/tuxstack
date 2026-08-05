use std::collections::HashMap;
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use futures_util::{StreamExt, stream::FuturesUnordered};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{Mutex, mpsc};
use tokio_util::sync::CancellationToken;
use tuxstack_docker_core::services::{
    ComposeGroupAction, ListContainersOptions, ListImagesOptions, ListNetworksOptions,
    ListVolumesOptions,
};
use tuxstack_docker_core::{
    ContainerError, ContainerTerminalError, ContainerTerminalOptions, ContainerTerminalOutput,
    ContainerTerminalSession, DockerError,
};
use tuxstack_protocol::{
    ComposeAction, DockerRequest, DockerResponse, FeatureFlags, FrameError, HandshakeRejection,
    HandshakeRejectionCode, MAX_FRAME_SIZE, MountAction, PROTOCOL_VERSION, ProtocolBody,
    ProtocolEnvelope, ProtocolError, ProtocolErrorCode, Request, RequestId, Response, ServerEvent,
    ServerHello, SubscriptionAccepted, SubscriptionEndReason, SubscriptionId, SubscriptionRequest,
    TerminalState, decode_payload, encode_frame_with_limit, validate_frame_length,
};

use crate::state::{DaemonEvent, DaemonState};

const OUTBOUND_QUEUE: usize = 256;
const REQUEST_DONE_QUEUE: usize = 64;
const SUBSCRIPTION_DONE_QUEUE: usize = 32;
const MAX_CLIENT_REQUESTS: usize = 64;
const MAX_CLIENT_SUBSCRIPTIONS: usize = 32;
const MAX_CLIENT_STREAM_TASKS: usize = 32;
const MAX_SUBSCRIPTION_MEMBERS: usize = 16;
const MAX_TERMINAL_INPUT_BYTES: usize = 64 * 1024;
const MAX_TERMINAL_DIMENSION: u16 = 4096;
const OUTBOUND_SEND_TIMEOUT: Duration = Duration::from_secs(5);
const WRITER_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);

type TerminalRegistry = Arc<Mutex<HashMap<SubscriptionId, Arc<ContainerTerminalSession>>>>;

struct SubscriptionControl {
    cancellation: CancellationToken,
    task_cost: usize,
}

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
    let mut writer_task = tokio::spawn(async move {
        while let Some(envelope) = outbound_rx.recv().await {
            write_envelope(&mut writer, &envelope, maximum).await?;
        }
        Ok::<_, IpcError>(())
    });
    let (request_done_tx, mut request_done_rx) = mpsc::channel(REQUEST_DONE_QUEUE);
    let (subscription_done_tx, mut subscription_done_rx) = mpsc::channel(SUBSCRIPTION_DONE_QUEUE);
    let terminals: TerminalRegistry = Arc::new(Mutex::new(HashMap::new()));
    let mut requests: HashMap<RequestId, CancellationToken> = HashMap::new();
    let mut subscriptions: HashMap<SubscriptionId, SubscriptionControl> = HashMap::new();
    let mut subscription_tasks = 0_usize;
    let mut next_subscription_id = 1_u64;
    let shutdown = state.shutdown_token();

    let connection_result: Result<()> = async {
        loop {
            let envelope = tokio::select! {
                result = read_envelope(&mut reader, maximum) => Some(result?),
                Some(request_id) = request_done_rx.recv() => {
                    requests.remove(&request_id);
                    None
                }
                Some(subscription_id) = subscription_done_rx.recv() => {
                    if let Some(control) = subscriptions.remove(&subscription_id) {
                        subscription_tasks = subscription_tasks.saturating_sub(control.task_cost);
                    }
                    None
                }
                _ = outbound_tx.closed() => return Err(IpcError::Closed.into()),
                _ = shutdown.cancelled() => break,
            };
            let Some(envelope) = envelope else { continue };
            match envelope.body {
                ProtocolBody::Request(request) => {
                    let request_id = envelope.request_id;
                    if request_id == 0
                        || requests.contains_key(&request_id)
                        || requests.len() >= MAX_CLIENT_REQUESTS
                    {
                        let (code, message, retryable) =
                            if requests.len() >= MAX_CLIENT_REQUESTS {
                                (
                                    ProtocolErrorCode::ResourceBusy,
                                    "per-client request limit reached",
                                    true,
                                )
                            } else {
                                (
                                    ProtocolErrorCode::InvalidRequest,
                                    "request ID must be nonzero and unique while active",
                                    false,
                                )
                            };
                        send_response(
                            &outbound_tx,
                            request_id,
                            Response::Error(protocol_error(code, message, retryable)),
                        )
                        .await?;
                        continue;
                    }
                    let token = CancellationToken::new();
                    requests.insert(request_id, token.clone());
                    let state = Arc::clone(&state);
                    let tx = outbound_tx.clone();
                    let done = request_done_tx.clone();
                    let terminals = Arc::clone(&terminals);
                    tokio::spawn(async move {
                        let response = tokio::select! {
                            _ = token.cancelled() => Response::Error(protocol_error(
                                ProtocolErrorCode::Cancelled,
                                "request cancelled",
                                true,
                            )),
                            response = dispatch_request(state, *request, token.clone(), terminals) => response,
                        };
                        let _ = send_response(&tx, request_id, response).await;
                        let _ = done.send(request_id).await;
                    });
                }
                ProtocolBody::CancelRequest { request_id } => {
                    if let Some(token) = requests.get(&request_id) {
                        token.cancel();
                    }
                }
                ProtocolBody::Subscribe(subscription) => {
                    let task_cost = match subscription_task_cost(&subscription) {
                        Ok(task_cost) => task_cost,
                        Err(error) => {
                            send_response(
                                &outbound_tx,
                                envelope.request_id,
                                Response::Error(error),
                            )
                            .await?;
                            continue;
                        }
                    };
                    if subscriptions.len() >= MAX_CLIENT_SUBSCRIPTIONS
                        || subscription_tasks.saturating_add(task_cost) > MAX_CLIENT_STREAM_TASKS
                    {
                        send_response(
                            &outbound_tx,
                            envelope.request_id,
                            Response::Error(protocol_error(
                                ProtocolErrorCode::ResourceBusy,
                                "per-client subscription limit reached",
                                true,
                            )),
                        )
                        .await?;
                        continue;
                    }
                    while subscriptions.contains_key(&next_subscription_id) {
                        next_subscription_id = next_subscription_id.wrapping_add(1).max(1);
                    }
                    let id = next_subscription_id;
                    next_subscription_id = next_subscription_id.wrapping_add(1).max(1);
                    let token = CancellationToken::new();
                    subscriptions.insert(
                        id,
                        SubscriptionControl {
                            cancellation: token.clone(),
                            task_cost,
                        },
                    );
                    subscription_tasks += task_cost;
                    send_envelope(
                        &outbound_tx,
                        ProtocolEnvelope::new(
                            envelope.request_id,
                            ProtocolBody::Subscribed(SubscriptionAccepted {
                                subscription_id: id,
                            }),
                        ),
                    )
                    .await?;
                    spawn_subscription(
                        id,
                        subscription,
                        Arc::clone(&state),
                        outbound_tx.clone(),
                        token,
                        Arc::clone(&terminals),
                        subscription_done_tx.clone(),
                    );
                }
                ProtocolBody::Unsubscribe { subscription_id } => {
                    if let Some(control) = subscriptions.remove(&subscription_id) {
                        subscription_tasks = subscription_tasks.saturating_sub(control.task_cost);
                        control.cancellation.cancel();
                    }
                    close_terminal(&terminals, subscription_id).await;
                    send_response(
                        &outbound_tx,
                        envelope.request_id,
                        Response::Acknowledged,
                    )
                    .await?;
                }
                ProtocolBody::Ping => {
                    send_envelope(
                        &outbound_tx,
                        ProtocolEnvelope::new(envelope.request_id, ProtocolBody::Pong),
                    )
                    .await?;
                }
                _ => bail!("unexpected client protocol message"),
            }
        }
        Ok(())
    }
    .await;

    for (_, token) in requests {
        token.cancel();
    }
    for (_, control) in subscriptions {
        control.cancellation.cancel();
    }
    close_all_terminals(&terminals).await;
    drop(outbound_tx);
    match tokio::time::timeout(WRITER_SHUTDOWN_TIMEOUT, &mut writer_task).await {
        Ok(result) => result.context("join IPC writer")??,
        Err(_) => {
            writer_task.abort();
            let _ = writer_task.await;
        }
    }
    connection_result
}

async fn dispatch_request(
    state: Arc<DaemonState>,
    request: Request,
    cancellation: CancellationToken,
    terminals: TerminalRegistry,
) -> Response {
    let result: Result<Response, ProtocolError> = async {
        Ok(match request {
            Request::GetDaemonStatus => Response::DaemonStatus(state.status()),
            Request::GetMountStatus => Response::MountStatus(state.mount_status()),
            Request::SetMountState(action) => {
                let status = match action {
                    MountAction::Mount => state.mount().await,
                    MountAction::Unmount => state.unmount().await,
                    MountAction::Remount => state.remount().await,
                    _ => {
                        return Err(protocol_error(
                            ProtocolErrorCode::InvalidRequest,
                            "unsupported mount action",
                            false,
                        ));
                    }
                }
                .map_err(|error| {
                    protocol_error(ProtocolErrorCode::Internal, &error.to_string(), false)
                })?;
                Response::MountStatus(status)
            }
            Request::GetResourceFusePath(resource) => Response::ResourceFusePath(
                state
                    .resource_path(resource)
                    .await
                    .map_err(|error| map_resource_error(&error))?,
            ),
            Request::GetProviderDescriptor(path) => Response::ProviderDescriptor(
                state
                    .provider_descriptor(path)
                    .await
                    .map_err(|error| map_resource_error(&error))?,
            ),
            Request::PerformResourceOperation(_) => Response::Acknowledged,
            Request::Docker(request) => Response::Docker(Box::new(
                dispatch_docker(&state, *request, cancellation).await?,
            )),
            Request::ContainerTerminalInput {
                subscription_id,
                bytes,
            } => {
                if bytes.len() > MAX_TERMINAL_INPUT_BYTES {
                    return Err(protocol_error(
                        ProtocolErrorCode::InvalidRequest,
                        "terminal input exceeds the 64 KiB request limit",
                        false,
                    ));
                }
                let session = terminal_session(&terminals, subscription_id).await?;
                session
                    .write_input(bytes)
                    .await
                    .map_err(terminal_protocol_error)?;
                Response::Acknowledged
            }
            Request::ContainerTerminalResize {
                subscription_id,
                rows,
                cols,
            } => {
                validate_terminal_size(rows, cols)?;
                let session = terminal_session(&terminals, subscription_id).await?;
                session
                    .resize(rows, cols)
                    .await
                    .map_err(terminal_protocol_error)?;
                Response::Acknowledged
            }
            Request::ContainerTerminalClose { subscription_id } => {
                let session = {
                    terminals
                        .lock()
                        .await
                        .remove(&subscription_id)
                        .ok_or_else(|| {
                            protocol_error(
                                ProtocolErrorCode::NotFound,
                                "terminal subscription is not active",
                                false,
                            )
                        })?
                };
                session.close().await;
                Response::Acknowledged
            }
            _ => {
                return Err(protocol_error(
                    ProtocolErrorCode::InvalidRequest,
                    "unsupported request variant",
                    false,
                ));
            }
        })
    }
    .await;
    result.unwrap_or_else(Response::Error)
}

async fn dispatch_docker(
    state: &DaemonState,
    request: DockerRequest,
    cancellation: CancellationToken,
) -> Result<DockerResponse, ProtocolError> {
    let services = &state.services;
    macro_rules! docker {
        ($future:expr) => {
            $future.await.map_err(docker_protocol_error)?
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
                _ => {
                    return Err(protocol_error(
                        ProtocolErrorCode::InvalidRequest,
                        "unsupported compose action",
                        false,
                    ));
                }
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
                _ = cancellation.cancelled() => return Err(protocol_error(
                    ProtocolErrorCode::Cancelled,
                    "image pull cancelled",
                    true,
                )),
                item = stream.next() => item,
            } {
                progress.map_err(docker_protocol_error)?;
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
        _ => {
            return Err(protocol_error(
                ProtocolErrorCode::InvalidRequest,
                "unsupported Docker request variant",
                false,
            ));
        }
    })
}

fn spawn_subscription(
    id: SubscriptionId,
    subscription: SubscriptionRequest,
    state: Arc<DaemonState>,
    tx: mpsc::Sender<ProtocolEnvelope>,
    cancellation: CancellationToken,
    terminals: TerminalRegistry,
    done: mpsc::Sender<SubscriptionId>,
) {
    tokio::spawn(async move {
        let reason = match subscription {
            SubscriptionRequest::DaemonStatus
            | SubscriptionRequest::MountStatus
            | SubscriptionRequest::ResourceChanges { .. }
            | SubscriptionRequest::ProviderStatus { .. } => {
                run_daemon_subscription(id, &subscription, &state, &tx, &cancellation).await
            }
            SubscriptionRequest::ContainerStats { container_ids } => {
                run_stats_subscription(id, container_ids, &state, &tx, &cancellation).await
            }
            SubscriptionRequest::ContainerLogs {
                container_ids,
                options,
            } => {
                run_logs_subscription(id, container_ids, options, &state, &tx, &cancellation).await
            }
            SubscriptionRequest::ImagePull { options } => {
                run_image_pull_subscription(id, options, &state, &tx, &cancellation).await
            }
            SubscriptionRequest::ContainerTerminal {
                container_id,
                rows,
                cols,
            } => {
                run_terminal_subscription(
                    id,
                    container_id,
                    rows,
                    cols,
                    &state,
                    &tx,
                    &cancellation,
                    &terminals,
                )
                .await
            }
            _ => SubscriptionEndReason::Error(protocol_error(
                ProtocolErrorCode::InvalidRequest,
                "subscription is not implemented",
                false,
            )),
        };

        let reason = if state.shutdown_token().is_cancelled() {
            SubscriptionEndReason::ServerShutdown
        } else {
            reason
        };
        close_terminal(&terminals, id).await;
        let _ = send_event(
            &tx,
            ServerEvent::SubscriptionEnded {
                subscription_id: id,
                reason,
            },
            None,
        )
        .await;
        let _ = done.send(id).await;
    });
}

async fn run_daemon_subscription(
    id: SubscriptionId,
    subscription: &SubscriptionRequest,
    state: &DaemonState,
    tx: &mpsc::Sender<ProtocolEnvelope>,
    cancellation: &CancellationToken,
) -> SubscriptionEndReason {
    let mut events = state.subscribe();
    loop {
        let event = tokio::select! {
            _ = cancellation.cancelled() => return SubscriptionEndReason::Unsubscribed,
            event = events.recv() => match event {
                Ok(event) => event,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    return SubscriptionEndReason::ServerShutdown;
                }
            },
        };
        let event = match (subscription, event) {
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
            && !send_event(tx, event, Some(cancellation)).await
        {
            return SubscriptionEndReason::Unsubscribed;
        }
    }
}

async fn run_stats_subscription(
    id: SubscriptionId,
    container_ids: Vec<String>,
    state: &DaemonState,
    tx: &mpsc::Sender<ProtocolEnvelope>,
    cancellation: &CancellationToken,
) -> SubscriptionEndReason {
    let members = FuturesUnordered::new();
    for container_id in container_ids {
        let service = state.services.containers.clone();
        let tx = tx.clone();
        let cancel = cancellation.clone();
        members.push(async move {
            let mut stream = service.watch_stats(&container_id, cancel.clone());
            loop {
                let item = tokio::select! {
                    _ = cancel.cancelled() => return Ok(()),
                    item = stream.next() => item,
                };
                let Some(item) = item else { return Ok(()) };
                let stats = item?;
                if !send_event(
                    &tx,
                    ServerEvent::ContainerStats {
                        subscription_id: id,
                        container_id: container_id.clone(),
                        stats,
                    },
                    Some(&cancel),
                )
                .await
                {
                    cancel.cancel();
                    return Ok(());
                }
            }
        });
    }
    wait_for_members(members, cancellation).await
}

async fn run_logs_subscription(
    id: SubscriptionId,
    container_ids: Vec<String>,
    options: tuxstack_domain::ContainerLogsOptions,
    state: &DaemonState,
    tx: &mpsc::Sender<ProtocolEnvelope>,
    cancellation: &CancellationToken,
) -> SubscriptionEndReason {
    let members = FuturesUnordered::new();
    for container_id in container_ids {
        let service = state.services.containers.clone();
        let options = options.clone();
        let tx = tx.clone();
        let cancel = cancellation.clone();
        members.push(async move {
            let mut stream = service.watch_logs(&container_id, &options, cancel.clone());
            loop {
                let item = tokio::select! {
                    _ = cancel.cancelled() => return Ok(()),
                    item = stream.next() => item,
                };
                let Some(item) = item else { return Ok(()) };
                let line = item?;
                if !send_event(
                    &tx,
                    ServerEvent::ContainerLog {
                        subscription_id: id,
                        container_id: container_id.clone(),
                        line,
                    },
                    Some(&cancel),
                )
                .await
                {
                    cancel.cancel();
                    return Ok(());
                }
            }
        });
    }
    wait_for_members(members, cancellation).await
}

async fn wait_for_members<F>(
    mut members: FuturesUnordered<F>,
    cancellation: &CancellationToken,
) -> SubscriptionEndReason
where
    F: std::future::Future<Output = Result<(), DockerError>>,
{
    loop {
        let result = tokio::select! {
            _ = cancellation.cancelled() => return SubscriptionEndReason::Unsubscribed,
            result = members.next() => result,
        };
        match result {
            Some(Ok(())) => {}
            Some(Err(error)) => {
                cancellation.cancel();
                return SubscriptionEndReason::Error(docker_protocol_error(error));
            }
            None => return SubscriptionEndReason::Completed,
        }
    }
}

async fn run_image_pull_subscription(
    id: SubscriptionId,
    options: tuxstack_domain::PullImageOptions,
    state: &DaemonState,
    tx: &mpsc::Sender<ProtocolEnvelope>,
    cancellation: &CancellationToken,
) -> SubscriptionEndReason {
    let mut stream = state.services.images.pull_image(options);
    loop {
        let item = tokio::select! {
            _ = cancellation.cancelled() => return SubscriptionEndReason::Unsubscribed,
            item = stream.next() => item,
        };
        match item {
            Some(Ok(progress)) => {
                if !send_event(
                    tx,
                    ServerEvent::ImagePullProgress {
                        subscription_id: id,
                        progress,
                    },
                    Some(cancellation),
                )
                .await
                {
                    return SubscriptionEndReason::Unsubscribed;
                }
            }
            Some(Err(error)) => {
                return SubscriptionEndReason::Error(docker_protocol_error(error));
            }
            None => return SubscriptionEndReason::Completed,
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_terminal_subscription(
    id: SubscriptionId,
    container_id: String,
    rows: u16,
    cols: u16,
    state: &DaemonState,
    tx: &mpsc::Sender<ProtocolEnvelope>,
    cancellation: &CancellationToken,
    terminals: &TerminalRegistry,
) -> SubscriptionEndReason {
    if !send_event(
        tx,
        ServerEvent::TerminalState {
            subscription_id: id,
            state: TerminalState::Connecting,
        },
        Some(cancellation),
    )
    .await
    {
        return SubscriptionEndReason::Unsubscribed;
    }
    let session = match state
        .services
        .container_terminal
        .connect(
            &container_id,
            ContainerTerminalOptions::default(),
            cancellation.child_token(),
        )
        .await
    {
        Ok(session) => Arc::new(session),
        Err(error) => {
            let message = error.to_string();
            let _ = send_event(
                tx,
                ServerEvent::TerminalState {
                    subscription_id: id,
                    state: TerminalState::Failed {
                        reason: message.clone(),
                    },
                },
                Some(cancellation),
            )
            .await;
            return SubscriptionEndReason::Error(terminal_protocol_error(error));
        }
    };
    if let Err(error) = session.resize(rows, cols).await {
        let message = error.to_string();
        session.close().await;
        let _ = send_event(
            tx,
            ServerEvent::TerminalState {
                subscription_id: id,
                state: TerminalState::Failed { reason: message },
            },
            Some(cancellation),
        )
        .await;
        return SubscriptionEndReason::Error(terminal_protocol_error(error));
    }
    let mut output = match session.take_output().await {
        Ok(output) => output,
        Err(error) => {
            session.close().await;
            return SubscriptionEndReason::Error(terminal_protocol_error(error));
        }
    };
    terminals.lock().await.insert(id, Arc::clone(&session));
    if !send_event(
        tx,
        ServerEvent::TerminalState {
            subscription_id: id,
            state: TerminalState::Running {
                shell: session.shell().to_owned(),
            },
        },
        Some(cancellation),
    )
    .await
    {
        return SubscriptionEndReason::Unsubscribed;
    }

    loop {
        let item = tokio::select! {
            _ = cancellation.cancelled() => return SubscriptionEndReason::Unsubscribed,
            item = output.next() => item,
        };
        match item {
            Some(Ok(output)) => {
                let bytes = terminal_output_bytes(output);
                if !send_event(
                    tx,
                    ServerEvent::TerminalOutput {
                        subscription_id: id,
                        bytes,
                    },
                    Some(cancellation),
                )
                .await
                {
                    return SubscriptionEndReason::Unsubscribed;
                }
            }
            Some(Err(error)) => {
                let message = error.to_string();
                let _ = send_event(
                    tx,
                    ServerEvent::TerminalState {
                        subscription_id: id,
                        state: TerminalState::Failed {
                            reason: message.clone(),
                        },
                    },
                    Some(cancellation),
                )
                .await;
                return SubscriptionEndReason::Error(terminal_protocol_error(error));
            }
            None => {
                let status = session.inspect().await.ok();
                let exit_code = status.and_then(|status| status.exit_code);
                let _ = send_event(
                    tx,
                    ServerEvent::TerminalState {
                        subscription_id: id,
                        state: TerminalState::Exited { exit_code },
                    },
                    Some(cancellation),
                )
                .await;
                return SubscriptionEndReason::Completed;
            }
        }
    }
}

fn subscription_task_cost(subscription: &SubscriptionRequest) -> Result<usize, ProtocolError> {
    let members = match subscription {
        SubscriptionRequest::ContainerStats { container_ids }
        | SubscriptionRequest::ContainerLogs { container_ids, .. } => {
            if container_ids.is_empty() {
                return Err(protocol_error(
                    ProtocolErrorCode::InvalidRequest,
                    "at least one container ID is required",
                    false,
                ));
            }
            if container_ids.len() > MAX_SUBSCRIPTION_MEMBERS {
                return Err(protocol_error(
                    ProtocolErrorCode::InvalidRequest,
                    "too many container IDs in one subscription",
                    false,
                ));
            }
            if container_ids.iter().any(|id| id.trim().is_empty()) {
                return Err(protocol_error(
                    ProtocolErrorCode::InvalidRequest,
                    "container IDs must not be empty",
                    false,
                ));
            }
            container_ids.len()
        }
        SubscriptionRequest::ContainerTerminal {
            container_id,
            rows,
            cols,
        } => {
            if container_id.trim().is_empty() {
                return Err(protocol_error(
                    ProtocolErrorCode::InvalidRequest,
                    "container ID must not be empty",
                    false,
                ));
            }
            validate_terminal_size(*rows, *cols)?;
            1
        }
        _ => 1,
    };
    Ok(members)
}

fn validate_terminal_size(rows: u16, cols: u16) -> Result<(), ProtocolError> {
    if rows == 0 || cols == 0 || rows > MAX_TERMINAL_DIMENSION || cols > MAX_TERMINAL_DIMENSION {
        return Err(protocol_error(
            ProtocolErrorCode::InvalidRequest,
            "terminal rows and columns must be between 1 and 4096",
            false,
        ));
    }
    Ok(())
}

async fn terminal_session(
    terminals: &TerminalRegistry,
    subscription_id: SubscriptionId,
) -> Result<Arc<ContainerTerminalSession>, ProtocolError> {
    terminals
        .lock()
        .await
        .get(&subscription_id)
        .cloned()
        .ok_or_else(|| {
            protocol_error(
                ProtocolErrorCode::NotFound,
                "terminal subscription is not active",
                false,
            )
        })
}

async fn close_terminal(terminals: &TerminalRegistry, subscription_id: SubscriptionId) {
    let session = terminals.lock().await.remove(&subscription_id);
    if let Some(session) = session {
        session.close().await;
    }
}

async fn close_all_terminals(terminals: &TerminalRegistry) {
    let sessions = std::mem::take(&mut *terminals.lock().await);
    let closes = FuturesUnordered::new();
    for session in sessions.into_values() {
        closes.push(async move { session.close().await });
    }
    closes.collect::<Vec<()>>().await;
}

fn terminal_output_bytes(output: ContainerTerminalOutput) -> Vec<u8> {
    match output {
        ContainerTerminalOutput::StdOut(bytes)
        | ContainerTerminalOutput::StdErr(bytes)
        | ContainerTerminalOutput::StdIn(bytes)
        | ContainerTerminalOutput::Console(bytes) => bytes,
    }
}

fn docker_protocol_error(error: DockerError) -> ProtocolError {
    let (code, retryable) = match &error {
        DockerError::SocketNotFound(_)
        | DockerError::EngineUnavailable
        | DockerError::ConnectionTimeout => (ProtocolErrorCode::DockerUnavailable, true),
        DockerError::OperationTimeout => (ProtocolErrorCode::OperationTimedOut, true),
        DockerError::OperationCancelled => (ProtocolErrorCode::Cancelled, true),
        DockerError::PermissionDenied
        | DockerError::DestinationPermissionDenied(_)
        | DockerError::RegistryAuthenticationFailed => (ProtocolErrorCode::PermissionDenied, false),
        DockerError::ContainerNotFound(_)
        | DockerError::ImageNotFound(_)
        | DockerError::NetworkNotFound(_)
        | DockerError::VolumeNotFound(_)
        | DockerError::Container(
            ContainerError::NotFound(_)
            | ContainerError::ImageNotFound(_)
            | ContainerError::VolumeNotFound(_)
            | ContainerError::NetworkNotFound(_),
        ) => (ProtocolErrorCode::NotFound, false),
        DockerError::Conflict(_)
        | DockerError::NetworkProtected(_)
        | DockerError::NetworkInUse(_)
        | DockerError::VolumeInUse(_)
        | DockerError::VolumeAlreadyExists(_)
        | DockerError::Container(
            ContainerError::AlreadyRunning(_)
            | ContainerError::NotRunning(_)
            | ContainerError::Paused(_)
            | ContainerError::RemovalInProgress(_)
            | ContainerError::PortAlreadyAllocated(_)
            | ContainerError::NameAlreadyInUse(_),
        ) => (ProtocolErrorCode::Conflict, false),
        DockerError::InvalidContainerConfig(_)
        | DockerError::InvalidImageReference(_)
        | DockerError::InvalidNetworkConfig(_)
        | DockerError::InvalidVolumeName(_)
        | DockerError::UnsupportedConnection(_)
        | DockerError::UnsupportedVolumeCompression(_)
        | DockerError::Container(ContainerError::InvalidConfiguration(_)) => {
            (ProtocolErrorCode::InvalidRequest, false)
        }
        DockerError::DiskFull(_) => (ProtocolErrorCode::ResourceBusy, false),
        DockerError::RegistryUnavailable(_)
        | DockerError::VolumeDriverUnavailable(_)
        | DockerError::VolumePluginError(_) => (ProtocolErrorCode::DockerUnavailable, true),
        DockerError::Container(ContainerError::PermissionDenied) => {
            (ProtocolErrorCode::PermissionDenied, false)
        }
        DockerError::Container(ContainerError::DockerUnavailable) => {
            (ProtocolErrorCode::DockerUnavailable, true)
        }
        DockerError::Container(ContainerError::OperationTimeout) => {
            (ProtocolErrorCode::OperationTimedOut, true)
        }
        DockerError::Container(ContainerError::OperationCancelled) => {
            (ProtocolErrorCode::Cancelled, true)
        }
        _ => (ProtocolErrorCode::Internal, false),
    };
    protocol_error(code, &error.to_string(), retryable)
}

fn terminal_protocol_error(error: ContainerTerminalError) -> ProtocolError {
    let (code, retryable) = match error {
        ContainerTerminalError::InvalidOptions => (ProtocolErrorCode::InvalidRequest, false),
        ContainerTerminalError::NotRunning
        | ContainerTerminalError::Paused
        | ContainerTerminalError::ShellNotFound => (ProtocolErrorCode::Conflict, false),
        ContainerTerminalError::Permission => (ProtocolErrorCode::PermissionDenied, false),
        ContainerTerminalError::DockerUnavailable => (ProtocolErrorCode::DockerUnavailable, true),
        ContainerTerminalError::Timeout => (ProtocolErrorCode::OperationTimedOut, true),
        ContainerTerminalError::Cancelled => (ProtocolErrorCode::Cancelled, true),
        ContainerTerminalError::CreateFailed
        | ContainerTerminalError::StartFailed
        | ContainerTerminalError::Disconnected
        | ContainerTerminalError::ResizeFailed => (ProtocolErrorCode::DockerUnavailable, true),
    };
    protocol_error(code, &error.to_string(), retryable)
}

async fn send_response(
    tx: &mpsc::Sender<ProtocolEnvelope>,
    request_id: RequestId,
    response: Response,
) -> Result<(), IpcError> {
    send_envelope(
        tx,
        ProtocolEnvelope::new(request_id, ProtocolBody::Response(Box::new(response))),
    )
    .await
}

async fn send_envelope(
    tx: &mpsc::Sender<ProtocolEnvelope>,
    envelope: ProtocolEnvelope,
) -> Result<(), IpcError> {
    tokio::time::timeout(OUTBOUND_SEND_TIMEOUT, tx.send(envelope))
        .await
        .map_err(|_| IpcError::Backpressure)?
        .map_err(|_| IpcError::Closed)
}

async fn send_event(
    tx: &mpsc::Sender<ProtocolEnvelope>,
    event: ServerEvent,
    cancellation: Option<&CancellationToken>,
) -> bool {
    let envelope = ProtocolEnvelope::new(0, ProtocolBody::Event(event));
    match cancellation {
        Some(cancellation) => tokio::select! {
            biased;
            _ = cancellation.cancelled() => false,
            result = send_envelope(tx, envelope) => result.is_ok(),
        },
        None => send_envelope(tx, envelope).await.is_ok(),
    }
}

fn verify_peer(stream: &UnixStream) -> Result<()> {
    let credentials = stream.peer_cred().context("read Unix peer credentials")?;
    let current_uid = unsafe { libc::geteuid() };
    if credentials.uid() != current_uid {
        bail!("IPC peer UID {} is not authorized", credentials.uid());
    }
    Ok(())
}

fn map_resource_error(error: &anyhow::Error) -> ProtocolError {
    let message = error.to_string();
    let code = if message.contains("not attached to the VFS namespace") {
        ProtocolErrorCode::NotFound
    } else if message.contains("permission") {
        ProtocolErrorCode::PermissionDenied
    } else if message.contains("timed out") {
        ProtocolErrorCode::OperationTimedOut
    } else {
        ProtocolErrorCode::ProviderUnavailable
    };
    protocol_error(
        code,
        &message,
        matches!(
            code,
            ProtocolErrorCode::OperationTimedOut | ProtocolErrorCode::ProviderUnavailable
        ),
    )
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
    #[error("outbound client queue remained full")]
    Backpressure,
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

    #[test]
    fn docker_error_classes_map_to_stable_protocol_codes() {
        let cases = [
            (
                DockerError::ContainerNotFound("missing".into()),
                ProtocolErrorCode::NotFound,
            ),
            (
                DockerError::PermissionDenied,
                ProtocolErrorCode::PermissionDenied,
            ),
            (
                DockerError::EngineUnavailable,
                ProtocolErrorCode::DockerUnavailable,
            ),
            (
                DockerError::OperationTimeout,
                ProtocolErrorCode::OperationTimedOut,
            ),
            (
                DockerError::Conflict("busy".into()),
                ProtocolErrorCode::Conflict,
            ),
            (
                DockerError::InvalidImageReference("bad".into()),
                ProtocolErrorCode::InvalidRequest,
            ),
        ];
        for (docker, expected) in cases {
            assert_eq!(docker_protocol_error(docker).code, expected);
        }
    }

    #[test]
    fn stream_member_and_terminal_bounds_are_validated() {
        assert!(matches!(
            subscription_task_cost(&SubscriptionRequest::ContainerStats {
                container_ids: Vec::new(),
            }),
            Err(ProtocolError {
                code: ProtocolErrorCode::InvalidRequest,
                ..
            })
        ));
        assert!(matches!(
            validate_terminal_size(0, 80),
            Err(ProtocolError {
                code: ProtocolErrorCode::InvalidRequest,
                ..
            })
        ));
        assert!(validate_terminal_size(24, 80).is_ok());
    }
}
