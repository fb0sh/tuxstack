//! Container lifecycle service.

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use bollard::models::{
    ContainerCreateBody, EndpointIpamConfig, EndpointSettings, HostConfig, Mount, MountBindOptions,
    MountBindOptionsPropagationEnum, MountTmpfsOptions, MountType, NetworkConnectRequest,
    NetworkingConfig, PortBinding as BollardPortBinding, RestartPolicy as BollardRestartPolicy,
    RestartPolicyNameEnum,
};
use bollard::query_parameters::{
    CreateContainerOptions, KillContainerOptions as BollardKillContainerOptions,
    ListContainersOptions as BollardListContainersOptions, LogsOptions,
    RemoveContainerOptions as BollardRemoveContainerOptions, RenameContainerOptions,
    RestartContainerOptions as BollardRestartContainerOptions, StatsOptions,
    StopContainerOptions as BollardStopContainerOptions,
};
use futures_util::{Stream, StreamExt};
use tokio_util::sync::CancellationToken;

use crate::client::DockerClient;
use crate::error::{DockerError, classify_api_error, classify_container_api_error};
use crate::mapping::containers::{map_container_detail, map_container_summary, map_log_output};
use crate::mapping::stats::map_container_stats;
use crate::models::{
    ContainerDetail, ContainerLogsOptions, ContainerNetworkFailure, ContainerRuntimeState,
    ContainerStats, ContainerSummary, CreateContainerMount, CreateContainerNetwork,
    CreateContainerRequest, CreateContainerResult, KillContainerOptions, LogLine,
    RemoveContainerOptions as DomainRemoveOptions, RestartContainerOptions,
    StopContainerOptions as DomainStopOptions, container_matches_search,
};

pub type LogStreamResult = Pin<Box<dyn Stream<Item = Result<LogLine, DockerError>> + Send>>;
pub type StatsStreamResult =
    Pin<Box<dyn Stream<Item = Result<ContainerStats, DockerError>> + Send>>;

#[derive(Debug, Clone, Default)]
pub struct ListContainersOptions {
    pub all: bool,
    pub limit: Option<usize>,
    pub search: Option<String>,
    pub state: Option<ContainerRuntimeState>,
}

#[derive(Clone)]
pub struct ContainerService {
    client: Arc<DockerClient>,
}

impl ContainerService {
    pub fn new(client: Arc<DockerClient>) -> Self {
        Self { client }
    }

    /// One Engine list request only: this deliberately performs no inspect or
    /// stats calls per row.
    pub async fn list_containers(
        &self,
        options: &ListContainersOptions,
    ) -> Result<Vec<ContainerSummary>, DockerError> {
        let timer = crate::instrument::Timer::start("docker.list_containers");
        let result = self.list_containers_inner(options).await;
        match &result {
            Ok(containers) => timer.finish_ok(containers.len(), "live"),
            Err(error) => timer.finish_err(&error.to_string()),
        }
        result
    }

    async fn list_containers_inner(
        &self,
        options: &ListContainersOptions,
    ) -> Result<Vec<ContainerSummary>, DockerError> {
        let mut filters = HashMap::new();
        if let Some(state) = options
            .state
            .filter(|state| *state != ContainerRuntimeState::Unknown)
        {
            filters.insert("status".to_string(), vec![state.as_str().to_string()]);
        }
        let docker = self.docker_with_timeout(self.request_timeout());
        let response = self
            .run(
                docker.list_containers(Some(BollardListContainersOptions {
                    all: options.all,
                    limit: options
                        .limit
                        .map(|value| i32::try_from(value).unwrap_or(i32::MAX)),
                    filters: (!filters.is_empty()).then_some(filters),
                    ..Default::default()
                })),
            )
            .await
            .map_err(|error| classify_api_error(&error, "container"))?;
        let mut containers: Vec<_> = response.into_iter().map(map_container_summary).collect();
        if let Some(search) = options.search.as_deref() {
            containers.retain(|container| container_matches_search(container, search));
        }
        if let Some(state) = options.state {
            containers.retain(|container| container.state == state);
        }
        Ok(containers)
    }

    pub async fn inspect_container(
        &self,
        id_or_name: &str,
    ) -> Result<ContainerDetail, DockerError> {
        let docker = self.docker_with_timeout(self.request_timeout());
        let response = self
            .run(docker.inspect_container(id_or_name, None))
            .await
            .map_err(|error| classify_api_error(&error, "container"))?;
        map_container_detail(response)
    }

    pub async fn start_container(&self, id_or_name: &str) -> Result<(), DockerError> {
        let timeout = self.request_timeout();
        let docker = self.docker_with_timeout(timeout);
        self.action("start", docker.start_container(id_or_name, None), timeout)
            .await
    }

    pub async fn stop_container(
        &self,
        id_or_name: &str,
        options: Option<&DomainStopOptions>,
    ) -> Result<(), DockerError> {
        let grace = options.and_then(|options| options.timeout_seconds);
        let api_options = match options {
            Some(options) => Some(BollardStopContainerOptions {
                t: checked_timeout(options.timeout_seconds)?,
                ..Default::default()
            }),
            None => None,
        };
        let timeout = lifecycle_timeout(self.request_timeout(), grace);
        let docker = self.docker_with_timeout(timeout);
        self.action(
            "stop",
            docker.stop_container(id_or_name, api_options),
            timeout,
        )
        .await
    }

    /// Compatibility restart using Docker's configured stop timeout.
    pub async fn restart_container(&self, id_or_name: &str) -> Result<(), DockerError> {
        self.restart_container_with_options(id_or_name, &RestartContainerOptions::default())
            .await
    }

    pub async fn restart_container_with_options(
        &self,
        id_or_name: &str,
        options: &RestartContainerOptions,
    ) -> Result<(), DockerError> {
        let api_options = BollardRestartContainerOptions {
            signal: None,
            t: checked_timeout(options.timeout_seconds)?,
        };
        let timeout = lifecycle_timeout(self.request_timeout(), options.timeout_seconds);
        let docker = self.docker_with_timeout(timeout);
        self.action(
            "restart",
            docker.restart_container(id_or_name, Some(api_options)),
            timeout,
        )
        .await
    }

    pub async fn pause_container(&self, id_or_name: &str) -> Result<(), DockerError> {
        let timeout = self.request_timeout();
        let docker = self.docker_with_timeout(timeout);
        self.action("pause", docker.pause_container(id_or_name), timeout)
            .await
    }

    pub async fn unpause_container(&self, id_or_name: &str) -> Result<(), DockerError> {
        let timeout = self.request_timeout();
        let docker = self.docker_with_timeout(timeout);
        self.action("unpause", docker.unpause_container(id_or_name), timeout)
            .await
    }

    /// Compatibility kill using SIGKILL.
    pub async fn kill_container(&self, id_or_name: &str) -> Result<(), DockerError> {
        self.kill_container_with_options(id_or_name, &KillContainerOptions::default())
            .await
    }

    pub async fn kill_container_with_options(
        &self,
        id_or_name: &str,
        options: &KillContainerOptions,
    ) -> Result<(), DockerError> {
        let signal = options.signal.trim();
        if signal.is_empty() || !signal.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
            return Err(DockerError::InvalidContainerConfig(
                "invalid kill signal".into(),
            ));
        }
        let timeout = self.request_timeout();
        let docker = self.docker_with_timeout(timeout);
        self.action(
            "kill",
            docker.kill_container(
                id_or_name,
                Some(BollardKillContainerOptions {
                    signal: signal.into(),
                }),
            ),
            timeout,
        )
        .await
    }

    pub async fn remove_container(
        &self,
        id_or_name: &str,
        options: &DomainRemoveOptions,
    ) -> Result<(), DockerError> {
        let timeout = self.request_timeout();
        let docker = self.docker_with_timeout(timeout);
        self.action(
            "remove",
            docker.remove_container(
                id_or_name,
                Some(BollardRemoveContainerOptions {
                    force: options.force,
                    v: options.remove_volumes,
                    link: options.remove_links,
                }),
            ),
            timeout,
        )
        .await
    }

    pub async fn rename_container(
        &self,
        id_or_name: &str,
        new_name: &str,
    ) -> Result<(), DockerError> {
        if new_name.trim().is_empty() {
            return Err(DockerError::InvalidContainerConfig(
                "container name is empty".into(),
            ));
        }
        let timeout = self.request_timeout();
        let docker = self.docker_with_timeout(timeout);
        self.action(
            "rename",
            docker.rename_container(
                id_or_name,
                RenameContainerOptions {
                    name: new_name.into(),
                },
            ),
            timeout,
        )
        .await
    }

    /// Create from the domain request. The first network is attached atomically
    /// by create; additional networks are attempted afterwards and reported as
    /// partial failures without discarding the successfully-created container.
    pub async fn create_container(
        &self,
        request: &CreateContainerRequest,
    ) -> Result<CreateContainerResult, DockerError> {
        request
            .validate()
            .map_err(|error| DockerError::InvalidContainerConfig(error.to_string()))?;
        let first_network = request.networks.first();
        let body = create_body(request, first_network)?;
        let docker = self.docker_with_timeout(self.request_timeout());
        let created = self
            .run(docker.create_container(
                Some(CreateContainerOptions {
                    name: request.name.clone(),
                    platform: request.platform.clone().unwrap_or_default(),
                }),
                body,
            ))
            .await
            .map_err(|error| DockerError::from(classify_container_api_error(&error, "create")))?;

        let mut result = CreateContainerResult {
            id: created.id,
            warnings: created.warnings,
            network_failures: Vec::new(),
            started: false,
            start_error: None,
        };
        for network in request.networks.iter().skip(1) {
            let connect = docker.connect_network(
                &network.name,
                NetworkConnectRequest {
                    container: result.id.clone(),
                    endpoint_config: Some(endpoint_settings(network)),
                },
            );
            if let Err(error) = self.run(connect).await {
                result.network_failures.push(ContainerNetworkFailure {
                    network: network.name.clone(),
                    error: classify_container_api_error(&error, "connect network").to_string(),
                });
            }
        }
        if request.create_and_start {
            match self.start_container(&result.id).await {
                Ok(()) => result.started = true,
                Err(error) => result.start_error = Some(error.to_string()),
            }
        }
        Ok(result)
    }

    pub async fn container_logs(
        &self,
        id_or_name: &str,
        options: &ContainerLogsOptions,
    ) -> Result<Vec<LogLine>, DockerError> {
        let docker = self.docker_with_timeout(self.request_timeout());
        let mut stream = Box::pin(docker.logs(id_or_name, Some(bollard_logs_options(options))));
        let timestamps = options.timestamps;
        tokio::time::timeout(self.request_timeout(), async {
            let mut lines = Vec::new();
            while let Some(item) = stream.next().await {
                lines.push(map_log_output(
                    item.map_err(|error| classify_api_error(&error, "container"))?,
                    timestamps,
                ));
            }
            Ok(lines)
        })
        .await
        .map_err(|_| DockerError::OperationTimeout)?
    }

    pub async fn container_stats(&self, id_or_name: &str) -> Result<ContainerStats, DockerError> {
        let docker = self.docker_with_timeout(self.request_timeout());
        let mut stream = Box::pin(docker.stats(
            id_or_name,
            Some(StatsOptions {
                stream: false,
                one_shot: true,
            }),
        ));
        tokio::time::timeout(self.request_timeout(), async {
            let raw = stream
                .next()
                .await
                .ok_or_else(|| DockerError::InvalidResponse("empty stats response".into()))?
                .map_err(|error| classify_api_error(&error, "container"))?;
            Ok(map_container_stats(raw, None))
        })
        .await
        .map_err(|_| DockerError::OperationTimeout)?
    }

    pub fn watch_logs(
        &self,
        id_or_name: &str,
        options: &ContainerLogsOptions,
        cancel: CancellationToken,
    ) -> LogStreamResult {
        let timestamps = options.timestamps;
        let inner = self
            .docker_with_timeout(self.request_timeout())
            .logs(id_or_name, Some(bollard_logs_options(options)))
            .map(move |item| {
                item.map(|output| map_log_output(output, timestamps))
                    .map_err(|error| classify_api_error(&error, "container"))
            });
        Box::pin(inner.take_until(cancel.cancelled_owned()))
    }

    pub fn watch_stats(&self, id_or_name: &str, cancel: CancellationToken) -> StatsStreamResult {
        let mut previous = None;
        let inner = self
            .docker_with_timeout(self.request_timeout())
            .stats(
                id_or_name,
                Some(StatsOptions {
                    stream: true,
                    one_shot: false,
                }),
            )
            .map(move |item| {
                item.map(|raw| {
                    let mapped = map_container_stats(raw, previous.as_ref());
                    previous = Some(mapped.clone());
                    mapped
                })
                .map_err(|error| classify_api_error(&error, "container"))
            });
        Box::pin(inner.take_until(cancel.cancelled_owned()))
    }

    fn request_timeout(&self) -> Duration {
        self.client.config().request_timeout
    }

    fn docker_with_timeout(&self, timeout: Duration) -> bollard::Docker {
        self.client.inner().clone().with_timeout(timeout)
    }

    async fn run<T>(
        &self,
        future: impl std::future::Future<Output = Result<T, bollard::errors::Error>>,
    ) -> Result<T, bollard::errors::Error> {
        tokio::time::timeout(self.request_timeout(), future)
            .await
            .map_err(|_| bollard::errors::Error::RequestTimeoutError)?
    }

    async fn action<T>(
        &self,
        operation: &str,
        future: impl std::future::Future<Output = Result<T, bollard::errors::Error>>,
        timeout: Duration,
    ) -> Result<T, DockerError> {
        tokio::time::timeout(timeout, future)
            .await
            .map_err(|_| DockerError::OperationTimeout)?
            .map_err(|error| DockerError::from(classify_container_api_error(&error, operation)))
    }
}

fn lifecycle_timeout(request_timeout: Duration, daemon_grace_seconds: Option<i64>) -> Duration {
    // The Engine response is intentionally held until its stop phase finishes.
    // Keep the transport budget separate from (and in addition to) that daemon
    // grace period. Docker's default stop grace is 10 seconds when no override
    // is sent.
    let grace = daemon_grace_seconds
        .and_then(|value| u64::try_from(value).ok())
        .map(Duration::from_secs)
        .unwrap_or(Duration::from_secs(10));
    request_timeout
        .saturating_add(grace)
        .saturating_add(Duration::from_secs(2))
}

fn checked_timeout(value: Option<i64>) -> Result<Option<i32>, DockerError> {
    value
        .map(|value| {
            if value < 0 {
                return Err(DockerError::InvalidContainerConfig(
                    "container timeout must not be negative".into(),
                ));
            }
            i32::try_from(value).map_err(|_| {
                DockerError::InvalidContainerConfig("container timeout is too large".into())
            })
        })
        .transpose()
}

fn create_body(
    request: &CreateContainerRequest,
    first_network: Option<&CreateContainerNetwork>,
) -> Result<ContainerCreateBody, DockerError> {
    let exposed_ports: Vec<_> = request
        .ports
        .iter()
        .map(|port| format!("{}/{}", port.container_port, port.protocol.as_str()))
        .collect();
    let mut port_bindings: HashMap<String, Option<Vec<BollardPortBinding>>> = HashMap::new();
    for port in &request.ports {
        let key = format!("{}/{}", port.container_port, port.protocol.as_str());
        if port.host_ip.is_some() || port.host_port.is_some() {
            port_bindings
                .entry(key)
                .or_default()
                .get_or_insert_with(Vec::new)
                .push(BollardPortBinding {
                    host_ip: port.host_ip.clone(),
                    host_port: port.host_port.map(|value| value.to_string()),
                });
        }
    }
    let mounts = request
        .mounts
        .iter()
        .map(map_create_mount)
        .collect::<Result<Vec<_>, _>>()?;
    let restart_name = match request.restart_policy.name {
        crate::models::ContainerRestartPolicyName::No => RestartPolicyNameEnum::NO,
        crate::models::ContainerRestartPolicyName::Always => RestartPolicyNameEnum::ALWAYS,
        crate::models::ContainerRestartPolicyName::UnlessStopped => {
            RestartPolicyNameEnum::UNLESS_STOPPED
        }
        crate::models::ContainerRestartPolicyName::OnFailure => RestartPolicyNameEnum::ON_FAILURE,
    };
    let networking_config = first_network.map(|network| NetworkingConfig {
        endpoints_config: Some(HashMap::from([(
            network.name.clone(),
            endpoint_settings(network),
        )])),
    });
    Ok(ContainerCreateBody {
        hostname: request.hostname.clone(),
        domainname: request.domain_name.clone(),
        user: request.user.clone(),
        attach_stdin: Some(request.open_stdin),
        tty: Some(request.tty),
        open_stdin: Some(request.open_stdin),
        exposed_ports: (!exposed_ports.is_empty()).then_some(exposed_ports),
        env: (!request.environment.is_empty()).then(|| {
            request
                .environment
                .iter()
                .map(|item| format!("{}={}", item.key, item.value))
                .collect()
        }),
        cmd: (!request.command.is_empty()).then_some(request.command.clone()),
        image: Some(request.image.clone()),
        working_dir: request.working_directory.clone(),
        entrypoint: (!request.entrypoint.is_empty()).then_some(request.entrypoint.clone()),
        labels: (!request.labels.is_empty()).then(|| request.labels.clone().into_iter().collect()),
        host_config: Some(HostConfig {
            memory: request
                .resources
                .memory_bytes
                .map(i64::try_from)
                .transpose()
                .map_err(|_| {
                    DockerError::InvalidContainerConfig("memory limit is too large".into())
                })?,
            nano_cpus: request
                .resources
                .cpu_cores_millis
                .map(|value| i64::from(value) * 1_000_000),
            pids_limit: request.resources.pids_limit,
            port_bindings: (!port_bindings.is_empty()).then_some(port_bindings),
            restart_policy: Some(BollardRestartPolicy {
                name: Some(restart_name),
                maximum_retry_count: request
                    .restart_policy
                    .maximum_retry_count
                    .map(i64::try_from)
                    .transpose()
                    .map_err(|_| {
                        DockerError::InvalidContainerConfig("restart count is too large".into())
                    })?,
            }),
            mounts: (!mounts.is_empty()).then_some(mounts),
            auto_remove: Some(request.auto_remove),
            privileged: Some(request.privileged),
            readonly_rootfs: Some(request.read_only_rootfs),
            ..Default::default()
        }),
        networking_config,
        ..Default::default()
    })
}

fn endpoint_settings(network: &CreateContainerNetwork) -> EndpointSettings {
    EndpointSettings {
        aliases: (!network.aliases.is_empty()).then_some(network.aliases.clone()),
        ipam_config: (network.ipv4_address.is_some() || network.ipv6_address.is_some()).then(
            || EndpointIpamConfig {
                ipv4_address: network.ipv4_address.clone(),
                ipv6_address: network.ipv6_address.clone(),
                ..Default::default()
            },
        ),
        ..Default::default()
    }
}

fn map_create_mount(mount: &CreateContainerMount) -> Result<Mount, DockerError> {
    Ok(match mount {
        CreateContainerMount::Volume {
            source,
            destination,
            read_only,
        } => Mount {
            target: Some(destination.clone()),
            source: Some(source.clone()),
            typ: Some(MountType::VOLUME),
            read_only: Some(*read_only),
            ..Default::default()
        },
        CreateContainerMount::Bind {
            source,
            destination,
            read_only,
            propagation,
        } => Mount {
            target: Some(destination.clone()),
            source: Some(source.clone()),
            typ: Some(MountType::BIND),
            read_only: Some(*read_only),
            bind_options: propagation
                .as_deref()
                .map(|value| {
                    Ok(MountBindOptions {
                        propagation: Some(match value {
                            "private" => MountBindOptionsPropagationEnum::PRIVATE,
                            "rprivate" => MountBindOptionsPropagationEnum::RPRIVATE,
                            "shared" => MountBindOptionsPropagationEnum::SHARED,
                            "rshared" => MountBindOptionsPropagationEnum::RSHARED,
                            "slave" => MountBindOptionsPropagationEnum::SLAVE,
                            "rslave" => MountBindOptionsPropagationEnum::RSLAVE,
                            _ => {
                                return Err(DockerError::InvalidContainerConfig(format!(
                                    "invalid bind propagation: {value}"
                                )));
                            }
                        }),
                        ..Default::default()
                    })
                })
                .transpose()?,
            ..Default::default()
        },
        CreateContainerMount::Tmpfs {
            destination,
            size_bytes,
            mode,
        } => Mount {
            target: Some(destination.clone()),
            typ: Some(MountType::TMPFS),
            tmpfs_options: Some(MountTmpfsOptions {
                size_bytes: size_bytes.map(i64::try_from).transpose().map_err(|_| {
                    DockerError::InvalidContainerConfig("tmpfs size is too large".into())
                })?,
                mode: mode.map(i64::from),
                ..Default::default()
            }),
            ..Default::default()
        },
    })
}

fn bollard_logs_options(options: &ContainerLogsOptions) -> LogsOptions {
    LogsOptions {
        stdout: options.stdout,
        stderr: options.stderr,
        follow: options.follow,
        timestamps: options.timestamps,
        tail: options
            .tail
            .map(|value| value.to_string())
            .unwrap_or_else(|| "all".into()),
        since: options
            .since
            .map(|value| value.timestamp() as i32)
            .unwrap_or(0),
        until: options
            .until
            .map(|value| value.timestamp() as i32)
            .unwrap_or(0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{
        ContainerPortProtocol, CreateContainerPort, CreateContainerResources,
        CreateEnvironmentVariable,
    };

    #[test]
    fn create_body_maps_ports_environment_resources_and_first_network() {
        let request = CreateContainerRequest {
            image: "busybox:latest".into(),
            ports: vec![CreateContainerPort {
                container_port: 80,
                protocol: ContainerPortProtocol::Tcp,
                host_ip: Some("127.0.0.1".into()),
                host_port: Some(8080),
            }],
            environment: vec![CreateEnvironmentVariable {
                key: "MODE".into(),
                value: "test".into(),
            }],
            networks: vec![CreateContainerNetwork {
                name: "front".into(),
                aliases: vec!["web".into()],
                ipv4_address: Some("172.20.0.2".into()),
                ipv6_address: None,
            }],
            resources: CreateContainerResources {
                cpu_cores_millis: Some(1500),
                memory_bytes: Some(128 * 1024 * 1024),
                pids_limit: Some(64),
            },
            ..Default::default()
        };
        request.validate().unwrap();
        let body = create_body(&request, request.networks.first()).unwrap();
        assert_eq!(body.exposed_ports, Some(vec!["80/tcp".into()]));
        assert_eq!(body.env, Some(vec!["MODE=test".into()]));
        let host = body.host_config.unwrap();
        assert_eq!(host.nano_cpus, Some(1_500_000_000));
        assert_eq!(host.memory, Some(128 * 1024 * 1024));
        assert_eq!(host.pids_limit, Some(64));
        let port_bindings = host.port_bindings.unwrap();
        let binding = &port_bindings["80/tcp"].as_ref().unwrap()[0];
        assert_eq!(binding.host_ip.as_deref(), Some("127.0.0.1"));
        assert_eq!(binding.host_port.as_deref(), Some("8080"));
        let endpoint = &body.networking_config.unwrap().endpoints_config.unwrap()["front"];
        assert_eq!(endpoint.aliases.as_deref(), Some(&["web".into()][..]));
    }

    #[test]
    fn lifecycle_timeout_accounts_for_daemon_grace_and_validates_range() {
        assert_eq!(
            lifecycle_timeout(Duration::from_secs(30), Some(5)),
            Duration::from_secs(37)
        );
        assert_eq!(
            lifecycle_timeout(Duration::from_secs(30), None),
            Duration::from_secs(42)
        );
        assert!(checked_timeout(Some(-1)).is_err());
        assert!(checked_timeout(Some(i64::from(i32::MAX) + 1)).is_err());
        assert_eq!(checked_timeout(Some(5)).unwrap(), Some(5));
    }
}
