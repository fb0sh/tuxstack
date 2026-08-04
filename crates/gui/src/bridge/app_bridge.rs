//! App-level bridge: connection state and overview data.
//!
//! This QObject owns the connection lifecycle. On success it stores the
//! shared `DockerServices` in the global registry so every page model
//! can use them.

use std::pin::Pin;

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;
use tuxstack_docker_core::DockerServices;

use crate::app_state;
use crate::settings;

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        /// An alias to the QString type
        type QString = cxx_qt_lib::QString;
    }

    impl cxx_qt::Threading for AppController {}

    extern "RustQt" {
        /// Global app controller: docker connection + overview.
        #[qobject]
        #[qml_element]
        #[qproperty(i32, docker_status, cxx_name = "dockerStatus")]
        #[qproperty(QString, docker_status_text, cxx_name = "dockerStatusText")]
        #[qproperty(QString, docker_host, cxx_name = "dockerHost")]
        #[qproperty(QString, engine_info_json, cxx_name = "engineInfoJson")]
        #[qproperty(bool, overview_loading, cxx_name = "overviewLoading")]
        #[qproperty(QString, overview_json, cxx_name = "overviewJson")]
        type AppController = super::AppControllerRust;

        /// Connect to Docker and load the overview (called from QML).
        #[qinvokable]
        #[cxx_name = "startup"]
        fn startup(self: Pin<&mut Self>);

        /// Re-run the overview aggregation.
        #[qinvokable]
        #[cxx_name = "refreshOverview"]
        fn refresh_overview(self: Pin<&mut Self>);

        /// Emitted when the Docker event monitor publishes a debounced change
        /// batch; `kind` is one of images/containers/volumes/networks/daemon.
        #[qsignal]
        #[cxx_name = "dockerChanged"]
        fn docker_changed(self: Pin<&mut Self>, kind: QString);
    }
}

/// Rust state backing [`qobject::AppController`].
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
    /// Start connecting to Docker asynchronously.
    pub fn startup(mut self: Pin<&mut Self>) {
        app_state::clear_services();
        let generation = {
            let mut state = self.as_mut().rust_mut();
            state.connection_generation = state.connection_generation.wrapping_add(1);
            state.connection_generation
        };
        self.as_mut().set_docker_status(0); // loading
        self.as_mut()
            .set_docker_status_text(QString::from("Connecting to Docker Engine..."));

        let (settings, warning) = settings::load_settings();
        app_state::set_settings(settings.clone());
        self.as_mut().set_docker_host(QString::from(
            settings
                .docker_host
                .clone()
                .unwrap_or_else(|| "default (DOCKER_HOST or /var/run/docker.sock)".to_string()),
        ));

        if let Some(w) = warning {
            self.as_mut().set_docker_status_text(QString::from(w));
        }

        let qt_thread = self.qt_thread();
        crate::runtime::spawn(async move {
            let config = tuxstack_docker_core::DockerConfig {
                host: settings.docker_host.clone(),
                connect_timeout: std::time::Duration::from_secs(settings.connect_timeout_seconds),
                request_timeout: std::time::Duration::from_secs(settings.operation_timeout_seconds),
            };

            let result = match tuxstack_docker_core::DockerClient::connect_with_config(config) {
                Ok(client) => {
                    let services = DockerServices::new(std::sync::Arc::new(client));
                    match services.system.ping().await {
                        Ok(()) => Ok(services),
                        Err(e) => Err(e),
                    }
                }
                Err(e) => Err(e),
            };

            qt_thread
                .queue(move |mut controller| {
                    if controller.rust().connection_generation != generation {
                        return;
                    }
                    match result {
                        Ok(services) => {
                            app_state::set_services(services.clone());
                            // Start the global Docker event monitor: it
                            // debounces /events bursts and emits dockerChanged
                            // so pages refresh only what changed.
                            let monitor = app_state::get_store().events.clone();
                            monitor.rebind_client(services.client());
                            let watch_loop = monitor.clone();
                            crate::runtime::spawn(async move {
                                let stream = watch_loop.start();
                                tuxstack_docker_core::cache::run_monitor(
                                    &watch_loop,
                                    stream,
                                    tuxstack_docker_core::cache::DefaultEventClassifier,
                                )
                                .await;
                            });
                            let mut rx = monitor.subscribe();
                            let qt_thread = controller.as_mut().qt_thread();
                            crate::runtime::spawn(async move {
                                while rx.changed().await.is_ok() {
                                    let Some(notification) = rx.borrow_and_update().clone()
                                    else {
                                        continue;
                                    };
                                    tracing::debug!(
                                        burst = notification.burst,
                                        kinds = ?notification.kinds,
                                        reconnected = notification.reconnected,
                                        "dockerChanged notification forwarded"
                                    );
                                    let kinds = notification.kinds.clone();
                                    qt_thread
                                        .queue(move |mut controller| {
                                            for kind in kinds {
                                                let name = match kind {
                                                    tuxstack_docker_core::cache::ChangeKind::Images => "images",
                                                    tuxstack_docker_core::cache::ChangeKind::Containers => "containers",
                                                    tuxstack_docker_core::cache::ChangeKind::Volumes => "volumes",
                                                    tuxstack_docker_core::cache::ChangeKind::Networks => "networks",
                                                    tuxstack_docker_core::cache::ChangeKind::Daemon => "daemon",
                                                };
                                                controller.as_mut().docker_changed(
                                                    QString::from(name),
                                                );
                                            }
                                        })
                                        .unwrap_or_else(|error| {
                                            tracing::debug!(
                                                %error,
                                                "Qt object destroyed before dockerChanged delivery"
                                            )
                                        });
                                }
                            });
                            // Best-effort cleanup of orphaned volume-preview helpers
                            // left by previous crashes. Never blocks the UI path.
                            let cleanup_services = services.clone();
                            crate::runtime::spawn(async move {
                                match cleanup_services
                                    .volume_files
                                    .cleanup_orphan_sessions()
                                    .await
                                {
                                    Ok(count) if count > 0 => {
                                        tracing::debug!(
                                            removed = count,
                                            "cleaned orphan volume-preview helpers"
                                        );
                                    }
                                    Ok(_) => {}
                                    Err(error) => {
                                        tracing::debug!(
                                            error = %error,
                                            "orphan volume-preview cleanup failed"
                                        );
                                    }
                                }
                            });
                            controller.as_mut().set_docker_status(1); // ready
                            controller
                                .as_mut()
                                .set_docker_status_text(QString::from("Connected"));
                            controller.as_mut().refresh_overview();
                        }
                        Err(e) => {
                            tracing::debug!(error = %e, "docker connect failed");
                            let app_err = app_state::map_docker_error(&e);
                            controller.as_mut().set_docker_status(match app_err.kind() {
                                "permission_denied" => 3,
                                "docker_unavailable" => 2,
                                _ => 4,
                            });
                            controller
                                .as_mut()
                                .set_docker_status_text(QString::from(app_err.user_message()));
                        }
                    }
                })
                .unwrap_or_else(|error| tracing::debug!(%error, "Qt object destroyed before async result delivery"));
        });
    }

    /// Refresh the overview aggregation from the engine.
    pub fn refresh_overview(mut self: Pin<&mut Self>) {
        let Some(services) = app_state::get_services() else {
            return;
        };
        self.as_mut().set_overview_loading(true);
        let qt_thread = self.qt_thread();
        crate::runtime::spawn(async move {
            let result = services.system.overview().await;
            qt_thread
                .queue(move |mut controller| {
                    controller.as_mut().set_overview_loading(false);
                    match result {
                        Ok(overview) => {
                            let json = serde_json::to_string(&overview)
                                .unwrap_or_else(|_| "{}".to_string());
                            controller.as_mut().set_overview_json(QString::from(json));
                        }
                        Err(e) => {
                            tracing::debug!(error = %e, "overview refresh failed");
                            controller.as_mut().set_overview_json(QString::from("{}"));
                        }
                    }
                })
                .unwrap_or_else(|error| tracing::debug!(%error, "Qt object destroyed before async result delivery"));
        });
    }
}
