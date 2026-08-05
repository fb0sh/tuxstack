//! App-level bridge for the tuxstackd connection, status, overview, and
//! daemon-owned Docker resource events.

use std::pin::Pin;

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;
use tuxstack_protocol::{
    DaemonStatus, DockerConnectionStatus, DockerResourceRef, MountState, Request, ResourceChange,
    ResourceKind, Response, ServerEvent, SubscriptionRequest,
};

use crate::app_state;
use crate::error::AppError;

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    impl cxx_qt::Threading for AppController {}

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(i32, docker_status, cxx_name = "dockerStatus")]
        #[qproperty(QString, docker_status_text, cxx_name = "dockerStatusText")]
        #[qproperty(QString, docker_host, cxx_name = "dockerHost")]
        #[qproperty(QString, engine_info_json, cxx_name = "engineInfoJson")]
        #[qproperty(bool, overview_loading, cxx_name = "overviewLoading")]
        #[qproperty(QString, overview_json, cxx_name = "overviewJson")]
        type AppController = super::AppControllerRust;

        #[qinvokable]
        #[cxx_name = "startup"]
        fn startup(self: Pin<&mut Self>);
        #[qinvokable]
        #[cxx_name = "refreshOverview"]
        fn refresh_overview(self: Pin<&mut Self>);

        #[qsignal]
        #[cxx_name = "dockerChanged"]
        fn docker_changed(self: Pin<&mut Self>, kind: QString);
        #[qsignal]
        #[cxx_name = "containerChanged"]
        fn container_changed(self: Pin<&mut Self>, actor_id: QString, action: QString);
    }
}

#[derive(Default)]
pub struct AppControllerRust {
    connection_generation: u64,
    docker_status: i32,
    docker_status_text: QString,
    docker_host: QString,
    engine_info_json: QString,
    overview_loading: bool,
    overview_json: QString,
}

impl qobject::AppController {
    pub fn startup(mut self: Pin<&mut Self>) {
        app_state::clear_client();
        let generation = {
            let mut state = self.as_mut().rust_mut();
            state.connection_generation = state.connection_generation.wrapping_add(1);
            state.connection_generation
        };
        self.as_mut().set_docker_status(0);
        self.as_mut()
            .set_docker_status_text(QString::from("Connecting to TuxStack service…"));
        self.as_mut()
            .set_docker_host(QString::from("$XDG_RUNTIME_DIR/tuxstack/control.sock"));

        let qt = self.qt_thread();
        crate::runtime::spawn(async move {
            let result = async {
                let config = tuxstack_client::ClientConfig::from_env(env!("CARGO_PKG_VERSION"))?;
                let client = tuxstack_client::Client::connect(config).await?;
                let status = match client.request(Request::GetDaemonStatus).await? {
                    Response::DaemonStatus(status) => status,
                    Response::Error(error) => {
                        return Err(tuxstack_client::DaemonError::Internal(error.message));
                    }
                    _ => {
                        return Err(tuxstack_client::DaemonError::Internal(
                            "daemon returned an unexpected status response".into(),
                        ));
                    }
                };
                Ok::<_, tuxstack_client::DaemonError>((client, status))
            }
            .await;

            qt.queue(move |mut controller| {
                if controller.connection_generation != generation {
                    return;
                }
                match result {
                    Ok((client, status)) => {
                        app_state::set_client(client);
                        controller.as_mut().apply_daemon_status(&status);
                        controller.as_mut().start_daemon_subscriptions(generation);
                        if matches!(status.docker, DockerConnectionStatus::Connected { .. }) {
                            controller.as_mut().refresh_overview();
                        }
                    }
                    Err(error) => {
                        let app_error = AppError::from(error);
                        controller
                            .as_mut()
                            .set_docker_status(app_error.status_code());
                        controller
                            .as_mut()
                            .set_docker_status_text(QString::from(app_error.user_message()));
                        controller.as_mut().schedule_reconnect(generation);
                    }
                }
            })
            .ok();
        });
    }

    fn apply_daemon_status(mut self: Pin<&mut Self>, status: &DaemonStatus) {
        let (code, text) = daemon_status_text(status);
        self.as_mut().set_docker_status(code);
        self.as_mut().set_docker_status_text(QString::from(text));
        self.as_mut().set_engine_info_json(QString::from(
            serde_json::to_string(status).unwrap_or_else(|_| "{}".into()),
        ));
    }

    fn start_daemon_subscriptions(self: Pin<&mut Self>, generation: u64) {
        let Some(client) = app_state::get_client() else {
            return;
        };
        let qt = self.qt_thread();
        crate::runtime::spawn(async move {
            let status = client.subscribe(SubscriptionRequest::DaemonStatus).await;
            let resources = client
                .subscribe(SubscriptionRequest::ResourceChanges {
                    kinds: vec![
                        ResourceKind::Container,
                        ResourceKind::Image,
                        ResourceKind::Network,
                        ResourceKind::Volume,
                    ],
                })
                .await;
            let (mut status, mut resources) = match (status, resources) {
                (Ok(status), Ok(resources)) => (status, resources),
                (status, resources) => {
                    tracing::debug!(
                        status_ok = status.is_ok(),
                        resources_ok = resources.is_ok(),
                        "daemon event subscription failed"
                    );
                    return;
                }
            };
            loop {
                let event = tokio::select! {
                    event = status.recv() => event,
                    event = resources.recv() => event,
                };
                let Some(event) = event else {
                    break;
                };
                let thread = qt.clone();
                if thread
                    .queue(move |mut controller| {
                        if controller.connection_generation != generation {
                            return;
                        }
                        match event {
                            ServerEvent::DaemonStatus { status, .. } => {
                                controller.as_mut().apply_daemon_status(&status);
                            }
                            ServerEvent::ResourceChanged {
                                kind,
                                resource,
                                change,
                                ..
                            } => emit_resource_change(controller.as_mut(), kind, resource, change),
                            _ => {}
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
            qt.queue(move |mut controller| {
                if controller.connection_generation == generation {
                    app_state::clear_client();
                    controller.as_mut().set_docker_status(5);
                    controller.as_mut().set_docker_status_text(QString::from(
                        "TuxStack service connection was lost. Reconnecting…",
                    ));
                    controller.as_mut().schedule_reconnect(generation);
                }
            })
            .ok();
        });
    }

    fn schedule_reconnect(self: Pin<&mut Self>, generation: u64) {
        let qt = self.qt_thread();
        crate::runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            qt.queue(move |mut controller| {
                if controller.connection_generation == generation {
                    controller.as_mut().startup();
                }
            })
            .ok();
        });
    }

    pub fn refresh_overview(mut self: Pin<&mut Self>) {
        let Some(services) = app_state::daemon_services() else {
            return;
        };
        self.as_mut().set_overview_loading(true);
        let qt = self.qt_thread();
        crate::runtime::spawn(async move {
            let result = services.system.overview().await;
            qt.queue(move |mut controller| {
                controller.as_mut().set_overview_loading(false);
                match result {
                    Ok(overview) => controller.as_mut().set_overview_json(QString::from(
                        serde_json::to_string(&overview).unwrap_or_else(|_| "{}".into()),
                    )),
                    Err(error) => {
                        tracing::debug!(%error, "overview refresh failed");
                        controller.as_mut().set_overview_json(QString::from("{}"));
                    }
                }
            })
            .ok();
        });
    }
}

fn daemon_status_text(status: &DaemonStatus) -> (i32, String) {
    match &status.docker {
        DockerConnectionStatus::Unavailable { reason } => {
            return (2, format!("Docker Engine is unavailable: {reason}"));
        }
        DockerConnectionStatus::Reconnecting => {
            return (2, "Docker Engine is reconnecting…".into());
        }
        DockerConnectionStatus::Connected { .. } => {}
        _ => return (4, "Docker Engine status is unavailable.".into()),
    }
    match &status.mount.state {
        MountState::Mounted => (1, "Connected".into()),
        MountState::Mounting => (6, "Docker filesystem is mounting…".into()),
        MountState::Unmounted => (6, "Docker filesystem is unavailable.".into()),
        MountState::Unmounting => (6, "Docker filesystem is unmounting…".into()),
        MountState::Failed { reason } => (6, format!("Docker filesystem is unavailable: {reason}")),
        _ => (6, "Docker filesystem status is unavailable.".into()),
    }
}

fn emit_resource_change(
    mut controller: Pin<&mut qobject::AppController>,
    kind: ResourceKind,
    resource: Option<DockerResourceRef>,
    change: ResourceChange,
) {
    let action = match change {
        ResourceChange::Created => "create",
        ResourceChange::Updated => "update",
        ResourceChange::Removed => "destroy",
        ResourceChange::Renamed => "rename",
        ResourceChange::Invalidated => "",
        _ => "",
    };
    if kind == ResourceKind::Container {
        let actor = match resource {
            Some(DockerResourceRef::Container { container_id }) => container_id,
            _ => String::new(),
        };
        controller
            .as_mut()
            .container_changed(QString::from(actor), QString::from(action));
    } else {
        let name = match kind {
            ResourceKind::Image => "images",
            ResourceKind::Network => "networks",
            ResourceKind::Volume => "volumes",
            ResourceKind::Container => return,
            _ => "daemon",
        };
        controller.as_mut().docker_changed(QString::from(name));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tuxstack_protocol::{DaemonLifecycle, MountStatus};

    fn status(docker: DockerConnectionStatus, mount: MountState) -> DaemonStatus {
        DaemonStatus {
            daemon_version: "test".into(),
            lifecycle: DaemonLifecycle::Ready,
            docker,
            mount: MountStatus {
                state: mount,
                mount_point: None,
                read_only: true,
            },
            uptime_seconds: 1,
        }
    }

    #[test]
    fn daemon_docker_and_fuse_outages_are_distinct() {
        assert_eq!(
            daemon_status_text(&status(
                DockerConnectionStatus::Unavailable {
                    reason: "down".into()
                },
                MountState::Mounted,
            ))
            .0,
            2
        );
        assert_eq!(
            daemon_status_text(&status(
                DockerConnectionStatus::Connected { daemon_id: None },
                MountState::Unmounted,
            ))
            .0,
            6
        );
    }
}
