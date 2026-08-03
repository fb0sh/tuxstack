//! Container list model (QAbstractListModel) with page-controller
//! invokables. All Docker I/O happens on the Tokio runtime; results are
//! marshalled back with `CxxQtThread::queue`.

use std::pin::Pin;

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QModelIndex, QString, QVariant};
use tuxstack_docker_core::DockerError;
use tuxstack_docker_core::services::containers::ListContainersOptions;

use crate::app_state::{ContainerPageState, PageStatus, get_services, map_docker_error};

/// Build a QVariant from a string (String → QString → QVariant).
fn qv(s: &str) -> QVariant {
    QVariant::from(&QString::from(s))
}

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!(< QAbstractListModel >);
        type QAbstractListModel;

        include!("cxx-qt-lib/qmodelindex.h");
        type QModelIndex = cxx_qt_lib::QModelIndex;

        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;

        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!("cxx-qt-lib/qhash.h");
        type QHash_i32_QByteArray = cxx_qt_lib::QHash<cxx_qt_lib::QHashPair_i32_QByteArray>;
    }

    /// Roles exposed to QML delegates.
    #[qenum(ContainerListModel)]
    enum ContainerRoles {
        ContainerId,
        ShortId,
        Name,
        Image,
        State,
        Status,
        Ports,
        CpuPercent,
        MemoryUsage,
        MemoryLimit,
        CreatedAt,
        Running,
        Busy,
        Operation,
    }

    impl cxx_qt::Threading for ContainerListModel {}

    unsafe extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[base = QAbstractListModel]
        #[qproperty(QString, search_text)]
        #[qproperty(bool, show_all)]
        #[qproperty(i32, status)]
        #[qproperty(QString, status_text)]
        type ContainerListModel = super::ContainerListModelRust;

        #[cxx_override]
        #[rust_name = "row_count"]
        fn rowCount(&self, parent: &QModelIndex) -> i32;

        #[cxx_override]
        fn data(&self, index: &QModelIndex, role: i32) -> QVariant;

        #[cxx_override]
        #[rust_name = "role_names"]
        fn roleNames(&self) -> QHash_i32_QByteArray;

        #[inherit]
        #[rust_name = "begin_reset_model"]
        fn beginResetModel(self: Pin<&mut Self>);

        #[inherit]
        #[rust_name = "end_reset_model"]
        fn endResetModel(self: Pin<&mut Self>);

        /// Reload the container list (with search/state filters).
        #[qinvokable]
        #[rust_name = "refresh"]
        fn refresh(self: Pin<&mut Self>);

        /// Start a container.
        #[qinvokable]
        #[rust_name = "start_container"]
        fn startContainer(self: Pin<&mut Self>, id: &QString);

        /// Stop a container.
        #[qinvokable]
        #[rust_name = "stop_container"]
        fn stopContainer(self: Pin<&mut Self>, id: &QString);

        /// Restart a container.
        #[qinvokable]
        #[rust_name = "restart_container"]
        fn restartContainer(self: Pin<&mut Self>, id: &QString);

        /// Remove a container.
        #[qinvokable]
        #[rust_name = "remove_container"]
        fn removeContainer(self: Pin<&mut Self>, id: &QString);

        /// Whether the given container id is currently busy.
        #[qinvokable]
        #[rust_name = "is_busy"]
        fn isBusy(&self, id: &QString) -> bool;
    }
}

/// Rust state backing the model.
#[derive(Default)]
pub struct ContainerListModelRust {
    pub(crate) state: ContainerPageState,
    search_text: QString,
    show_all: bool,
    status: i32,
    status_text: QString,
}

impl qobject::ContainerListModel {
    fn row_count(&self, _parent: &QModelIndex) -> i32 {
        self.state.rows.len() as i32
    }

    fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        let role = qobject::ContainerRoles { repr: role };
        let Some(row) = self.state.rows.get(index.row() as usize) else {
            return QVariant::default();
        };
        let busy = self.state.busy.get(&row.id).cloned().unwrap_or_default();
        match role {
            qobject::ContainerRoles::ContainerId => qv(&row.id),
            qobject::ContainerRoles::ShortId => qv(&row.short_id),
            qobject::ContainerRoles::Name => qv(&row.name),
            qobject::ContainerRoles::Image => qv(&row.image),
            qobject::ContainerRoles::State => qv(&row.state),
            qobject::ContainerRoles::Status => qv(&row.status),
            qobject::ContainerRoles::Ports => qv(&row.ports),
            qobject::ContainerRoles::CpuPercent => QVariant::from(&row.cpu_percent),
            qobject::ContainerRoles::MemoryUsage => QVariant::from(&(row.memory_usage as i64)),
            qobject::ContainerRoles::MemoryLimit => QVariant::from(&(row.memory_limit as i64)),
            qobject::ContainerRoles::CreatedAt => qv(&row.created_at),
            qobject::ContainerRoles::Running => QVariant::from(&row.running()),
            qobject::ContainerRoles::Busy => QVariant::from(&self.state.is_busy(&row.id)),
            qobject::ContainerRoles::Operation => qv(&busy),
            _ => QVariant::default(),
        }
    }

    fn role_names(&self) -> qobject::QHash_i32_QByteArray {
        let mut hash = qobject::QHash_i32_QByteArray::default();
        hash.insert(
            qobject::ContainerRoles::ContainerId.repr,
            "containerId".into(),
        );
        hash.insert(qobject::ContainerRoles::ShortId.repr, "shortId".into());
        hash.insert(qobject::ContainerRoles::Name.repr, "name".into());
        hash.insert(qobject::ContainerRoles::Image.repr, "image".into());
        hash.insert(qobject::ContainerRoles::State.repr, "state".into());
        hash.insert(qobject::ContainerRoles::Status.repr, "status".into());
        hash.insert(qobject::ContainerRoles::Ports.repr, "ports".into());
        hash.insert(
            qobject::ContainerRoles::CpuPercent.repr,
            "cpuPercent".into(),
        );
        hash.insert(
            qobject::ContainerRoles::MemoryUsage.repr,
            "memoryUsage".into(),
        );
        hash.insert(
            qobject::ContainerRoles::MemoryLimit.repr,
            "memoryLimit".into(),
        );
        hash.insert(qobject::ContainerRoles::CreatedAt.repr, "createdAt".into());
        hash.insert(qobject::ContainerRoles::Running.repr, "running".into());
        hash.insert(qobject::ContainerRoles::Busy.repr, "busy".into());
        hash.insert(qobject::ContainerRoles::Operation.repr, "operation".into());
        hash
    }

    /// Reload the container list on the Tokio runtime.
    pub fn refresh(mut self: Pin<&mut Self>) {
        let Some(services) = get_services() else {
            let mut s = self.as_mut().rust_mut().state.clone();
            s.status = PageStatus::DockerUnavailable;
            s.status_text =
                "Not connected to Docker Engine. Connect from the Overview page.".into();
            self.as_mut().apply_state(s);
            return;
        };

        let generation = {
            let mut s = self.as_mut().rust_mut().state.clone();
            let generation = s.begin_refresh();
            self.as_mut().apply_state(s);
            generation
        };

        let search = self.search_text().to_string();
        let show_all = *self.show_all();
        let qt_thread = self.qt_thread();

        crate::runtime::spawn(async move {
            let options = ListContainersOptions {
                all: show_all,
                search: if search.is_empty() {
                    None
                } else {
                    Some(search)
                },
                state: None,
                ..Default::default()
            };
            let result = services.containers.list_containers(&options).await;
            qt_thread
                .queue(move |mut model| {
                    let mut s = model.as_mut().rust_mut().state.clone();
                    match result {
                        Ok(summaries) => {
                            let ids: Vec<String> = summaries
                                .iter()
                                .filter(|c| c.state.is_active())
                                .map(|c| c.id.clone())
                                .collect();
                            s.apply_list(generation, &summaries);
                            model.as_mut().apply_state(s);
                            // Kick off one-shot stats for running containers.
                            if !ids.is_empty() {
                                let stats_thread = model.as_mut().qt_thread();
                                crate::runtime::spawn(async move {
                                    for id in ids {
                                        let services2 = get_services();
                                        if let Some(services2) = services2 {
                                            if let Ok(stats) =
                                                services2.containers.container_stats(&id).await
                                            {
                                                let tid = id.clone();
                                                let thread = stats_thread.clone();
                                                crate::runtime::spawn(async move {
                                                    let _ = thread.queue(move |mut m| {
                                                        let mut st =
                                                            m.as_mut().rust_mut().state.clone();
                                                        st.apply_stats(
                                                            generation,
                                                            &tid,
                                                            stats.cpu_percent,
                                                            stats.memory_usage_bytes,
                                                            stats.memory_limit_bytes,
                                                        );
                                                        m.as_mut().apply_state(st);
                                                    });
                                                });
                                            }
                                        }
                                    }
                                });
                            }
                        }
                        Err(e) => {
                            s.apply_list_error(generation, &map_docker_error(&e));
                            model.as_mut().apply_state(s);
                        }
                    }
                })
                .expect("queue to Qt thread");
        });
    }

    fn start_container(mut self: Pin<&mut Self>, id: &QString) {
        self.as_mut()
            .run_operation(id, "starting", |services, id| async move {
                services.containers.start_container(&id).await?;
                Ok(())
            });
    }

    fn stop_container(mut self: Pin<&mut Self>, id: &QString) {
        self.as_mut()
            .run_operation(id, "stopping", |services, id| async move {
                services
                    .containers
                    .stop_container(
                        &id,
                        Some(&tuxstack_docker_core::StopContainerOptions {
                            timeout_seconds: Some(10),
                        }),
                    )
                    .await?;
                Ok(())
            });
    }

    fn restart_container(mut self: Pin<&mut Self>, id: &QString) {
        self.as_mut()
            .run_operation(id, "restarting", |services, id| async move {
                services.containers.restart_container(&id).await?;
                Ok(())
            });
    }

    fn remove_container(mut self: Pin<&mut Self>, id: &QString) {
        self.as_mut()
            .run_operation(id, "removing", |services, id| async move {
                services
                    .containers
                    .remove_container(
                        &id,
                        &tuxstack_docker_core::RemoveContainerOptions {
                            force: false,
                            remove_volumes: false,
                            remove_links: false,
                        },
                    )
                    .await?;
                Ok(())
            });
    }

    fn is_busy(&self, id: &QString) -> bool {
        self.state.is_busy(&id.to_string())
    }

    /// Shared operation pipeline: mark busy → run async op → refresh.
    fn run_operation<F, Fut>(mut self: Pin<&mut Self>, id: &QString, operation: &str, op: F)
    where
        F: FnOnce(std::sync::Arc<tuxstack_docker_core::DockerServices>, String) -> Fut
            + Send
            + 'static,
        Fut: std::future::Future<Output = Result<(), DockerError>> + Send + 'static,
    {
        let Some(services) = get_services() else {
            return;
        };
        let id_str = id.to_string();
        {
            let mut s = self.as_mut().rust_mut().state.clone();
            s.mark_busy(&id_str, operation);
            self.as_mut().apply_state(s);
        }

        let qt_thread = self.qt_thread();
        crate::runtime::spawn(async move {
            let result = op(services, id_str.clone()).await;
            qt_thread
                .queue(move |mut model| {
                    let mut s = model.as_mut().rust_mut().state.clone();
                    s.clear_busy(&id_str);
                    model.as_mut().apply_state(s);
                    if let Err(e) = result {
                        tracing::debug!(container = %id_str, error = %e, "container operation failed");
                    }
                    // Refresh the list to reflect the new state.
                    model.as_mut().refresh();
                })
                .expect("queue to Qt thread");
        });
    }

    /// Replace the internal page state and sync the QML-visible properties.
    fn apply_state(mut self: Pin<&mut Self>, state: ContainerPageState) {
        self.as_mut().begin_reset_model();
        self.as_mut().rust_mut().state = state;
        self.as_mut().end_reset_model();
        let st = self.as_mut().state.clone();
        self.as_mut().set_status(st.status.as_i32());
        self.as_mut()
            .set_status_text(QString::from(&st.status_text));
    }
}
