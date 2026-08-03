//! Container lifecycle service.

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;

use bollard::query_parameters::{
    ListContainersOptions as BollardListContainersOptions, LogsOptions,
    RemoveContainerOptions as BollardRemoveContainerOptions, RenameContainerOptions, StatsOptions,
    StopContainerOptions as BollardStopContainerOptions,
};
use futures_util::{Stream, StreamExt};
use tokio_util::sync::CancellationToken;

use crate::client::DockerClient;
use crate::error::{DockerError, classify_api_error};
use crate::mapping::containers::{map_container_detail, map_container_summary, map_log_output};
use crate::mapping::stats::map_container_stats;
use crate::models::{
    ContainerDetail, ContainerLogsOptions, ContainerState, ContainerStats, ContainerSummary,
    LogLine, RemoveContainerOptions as DomainRemoveOptions,
    StopContainerOptions as DomainStopOptions,
};

pub type LogStreamResult = Pin<Box<dyn Stream<Item = Result<LogLine, DockerError>> + Send>>;
pub type StatsStreamResult =
    Pin<Box<dyn Stream<Item = Result<ContainerStats, DockerError>> + Send>>;

/// Options for listing containers.
#[derive(Debug, Clone, Default)]
pub struct ListContainersOptions {
    /// Include stopped containers (default false).
    pub all: bool,
    /// Limit the number of entries returned.
    pub limit: Option<usize>,
    /// Local name substring filter.
    pub search: Option<String>,
    /// Local state filter.
    pub state: Option<ContainerState>,
}

/// Container service backed by the shared Docker client.
#[derive(Clone)]
pub struct ContainerService {
    client: Arc<DockerClient>,
}

impl ContainerService {
    pub fn new(client: Arc<DockerClient>) -> Self {
        Self { client }
    }

    /// List containers, applying local search/state filtering on top of
    /// the API-level `all` flag.
    pub async fn list_containers(
        &self,
        options: &ListContainersOptions,
    ) -> Result<Vec<ContainerSummary>, DockerError> {
        let mut filters: HashMap<String, Vec<String>> = HashMap::new();
        if let Some(state) = &options.state {
            filters.insert("status".to_string(), vec![state.as_str().to_string()]);
        }

        let bollard_opts = BollardListContainersOptions {
            all: options.all,
            limit: options.limit.map(|l| l as i32),
            filters: if filters.is_empty() {
                None
            } else {
                Some(filters)
            },
            ..Default::default()
        };

        let docker = self.client.inner().clone();
        let containers = tokio::time::timeout(
            self.client.config().request_timeout,
            docker.list_containers(Some(bollard_opts)),
        )
        .await
        .map_err(|_| DockerError::OperationTimeout)?
        .map_err(|e| classify_api_error(&e, "container"))?;

        let mut mapped: Vec<ContainerSummary> =
            containers.into_iter().map(map_container_summary).collect();

        if let Some(search) = options.search.as_deref().map(str::to_lowercase) {
            mapped.retain(|c| {
                c.name.to_lowercase().contains(&search)
                    || c.image.to_lowercase().contains(&search)
                    || c.short_id.contains(&search)
                    || c.id.contains(&search)
            });
        }
        if let Some(state) = options.state {
            mapped.retain(|c| c.state == state);
        }

        Ok(mapped)
    }

    /// Fetch detailed information about one container.
    pub async fn inspect_container(
        &self,
        id_or_name: &str,
    ) -> Result<ContainerDetail, DockerError> {
        let docker = self.client.inner().clone();
        let detail = tokio::time::timeout(
            self.client.config().request_timeout,
            docker.inspect_container(id_or_name, None),
        )
        .await
        .map_err(|_| DockerError::OperationTimeout)?
        .map_err(|e| classify_api_error(&e, "container"))?;
        map_container_detail(detail)
    }

    /// Start a container.
    pub async fn start_container(&self, id_or_name: &str) -> Result<(), DockerError> {
        let docker = self.client.inner().clone();
        tokio::time::timeout(
            self.client.config().request_timeout,
            docker.start_container(id_or_name, None),
        )
        .await
        .map_err(|_| DockerError::OperationTimeout)?
        .map_err(|e| classify_api_error(&e, "container"))
    }

    /// Stop a container with an optional grace period.
    pub async fn stop_container(
        &self,
        id_or_name: &str,
        options: Option<&DomainStopOptions>,
    ) -> Result<(), DockerError> {
        let opts = options.map(|o| BollardStopContainerOptions {
            t: o.timeout_seconds.map(|t| t as i32),
            ..Default::default()
        });
        let docker = self.client.inner().clone();
        tokio::time::timeout(
            self.client.config().request_timeout,
            docker.stop_container(id_or_name, opts),
        )
        .await
        .map_err(|_| DockerError::OperationTimeout)?
        .map_err(|e| classify_api_error(&e, "container"))
    }

    /// Restart a container.
    pub async fn restart_container(&self, id_or_name: &str) -> Result<(), DockerError> {
        let docker = self.client.inner().clone();
        tokio::time::timeout(
            self.client.config().request_timeout,
            docker.restart_container(id_or_name, None),
        )
        .await
        .map_err(|_| DockerError::OperationTimeout)?
        .map_err(|e| classify_api_error(&e, "container"))
    }

    /// Pause a container.
    pub async fn pause_container(&self, id_or_name: &str) -> Result<(), DockerError> {
        let docker = self.client.inner().clone();
        tokio::time::timeout(
            self.client.config().request_timeout,
            docker.pause_container(id_or_name),
        )
        .await
        .map_err(|_| DockerError::OperationTimeout)?
        .map_err(|e| classify_api_error(&e, "container"))
    }

    /// Unpause a container.
    pub async fn unpause_container(&self, id_or_name: &str) -> Result<(), DockerError> {
        let docker = self.client.inner().clone();
        tokio::time::timeout(
            self.client.config().request_timeout,
            docker.unpause_container(id_or_name),
        )
        .await
        .map_err(|_| DockerError::OperationTimeout)?
        .map_err(|e| classify_api_error(&e, "container"))
    }

    /// Kill a container.
    pub async fn kill_container(&self, id_or_name: &str) -> Result<(), DockerError> {
        let docker = self.client.inner().clone();
        tokio::time::timeout(
            self.client.config().request_timeout,
            docker.kill_container(id_or_name, None),
        )
        .await
        .map_err(|_| DockerError::OperationTimeout)?
        .map_err(|e| classify_api_error(&e, "container"))
    }

    /// Remove a container.
    pub async fn remove_container(
        &self,
        id_or_name: &str,
        options: &DomainRemoveOptions,
    ) -> Result<(), DockerError> {
        let opts = BollardRemoveContainerOptions {
            force: options.force,
            v: options.remove_volumes,
            link: options.remove_links,
        };
        let docker = self.client.inner().clone();
        tokio::time::timeout(
            self.client.config().request_timeout,
            docker.remove_container(id_or_name, Some(opts)),
        )
        .await
        .map_err(|_| DockerError::OperationTimeout)?
        .map_err(|e| classify_api_error(&e, "container"))
    }

    /// Rename a container.
    pub async fn rename_container(
        &self,
        id_or_name: &str,
        new_name: &str,
    ) -> Result<(), DockerError> {
        let opts = RenameContainerOptions {
            name: new_name.to_string(),
        };
        let docker = self.client.inner().clone();
        tokio::time::timeout(
            self.client.config().request_timeout,
            docker.rename_container(id_or_name, opts),
        )
        .await
        .map_err(|_| DockerError::OperationTimeout)?
        .map_err(|e| classify_api_error(&e, "container"))
    }

    /// Fetch a bounded number of historical log lines.
    pub async fn container_logs(
        &self,
        id_or_name: &str,
        options: &ContainerLogsOptions,
    ) -> Result<Vec<LogLine>, DockerError> {
        let opts = bollard_logs_options(options);
        let docker = self.client.inner().clone();
        let timestamps = options.timestamps;
        let timeout = self.client.config().request_timeout;

        let logs = docker.logs(id_or_name, Some(opts));
        let mut lines = Vec::new();
        let mut stream = Box::pin(logs);
        let result = tokio::time::timeout(timeout, async {
            while let Some(entry) = stream.next().await {
                let entry = entry.map_err(|e| classify_api_error(&e, "container"))?;
                lines.push(map_log_output(entry, timestamps));
            }
            Ok::<_, DockerError>(lines)
        })
        .await;

        match result {
            Ok(Ok(lines)) => Ok(lines),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(DockerError::OperationTimeout),
        }
    }

    /// Fetch a single stats sample (non-streaming).
    pub async fn container_stats(&self, id_or_name: &str) -> Result<ContainerStats, DockerError> {
        let opts = StatsOptions {
            stream: false,
            one_shot: true,
        };
        let docker = self.client.inner().clone();
        let timeout = self.client.config().request_timeout;

        let stats = docker.stats(id_or_name, Some(opts));
        let mut stream = Box::pin(stats);
        let result = tokio::time::timeout(timeout, async {
            let mut previous: Option<ContainerStats> = None;
            while let Some(entry) = stream.next().await {
                let entry = entry.map_err(|e| classify_api_error(&e, "container"))?;
                let mapped = map_container_stats(entry, previous.as_ref());
                previous = Some(mapped);
            }
            previous.ok_or_else(|| DockerError::InvalidResponse("empty stats response".into()))
        })
        .await;

        match result {
            Ok(Ok(stats)) => Ok(stats),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(DockerError::OperationTimeout),
        }
    }

    /// Stream container logs with follow support.
    ///
    /// The returned stream ends when the container is removed or the
    /// token is cancelled.
    pub fn watch_logs(
        &self,
        id_or_name: &str,
        options: &ContainerLogsOptions,
        cancel: CancellationToken,
    ) -> LogStreamResult {
        let opts = bollard_logs_options(options);
        let docker = self.client.inner().clone();
        let timestamps = options.timestamps;

        let inner = docker.logs(id_or_name, Some(opts));
        Box::pin(inner.map(move |item| match item {
            Ok(output) => Ok(map_log_output(output, timestamps)),
            Err(e) => Err(classify_api_error(&e, "container")),
        }))
        .take_until(cancel.cancelled_owned())
        .boxed()
    }

    /// Stream container stats at the Docker Engine's natural rate.
    pub fn watch_stats(&self, id_or_name: &str, cancel: CancellationToken) -> StatsStreamResult {
        let opts = StatsOptions {
            stream: true,
            one_shot: false,
        };
        let docker = self.client.inner().clone();

        let mut previous: Option<ContainerStats> = None;
        let inner = docker
            .stats(id_or_name, Some(opts))
            .map(move |item| match item {
                Ok(raw) => {
                    let mapped = map_container_stats(raw, previous.as_ref());
                    previous = Some(mapped.clone());
                    Ok(mapped)
                }
                Err(e) => Err(classify_api_error(&e, "container")),
            });
        Box::pin(inner.take_until(cancel.cancelled_owned())).boxed()
    }
}

fn bollard_logs_options(options: &ContainerLogsOptions) -> LogsOptions {
    LogsOptions {
        stdout: options.stdout,
        stderr: options.stderr,
        follow: options.follow,
        timestamps: options.timestamps,
        tail: options
            .tail
            .map(|t| t.to_string())
            .unwrap_or_else(|| "all".to_string()),
        since: options.since.map(|t| t.timestamp() as i32).unwrap_or(0),
        until: options.until.map(|t| t.timestamp() as i32).unwrap_or(0),
    }
}
