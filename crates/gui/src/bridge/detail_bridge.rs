//! Container detail controller + log model.
//!
//! Owns the logs/stats streams for the detail page. Streams are started
//! and stopped by the page; the page always cancels them when it closes.

use std::pin::Pin;

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QModelIndex, QString, QVariant};
use tokio_util::sync::CancellationToken;

use crate::app_state::get_services;

/// Build a QVariant from a string (String → QString → QVariant).
fn qv(s: &str) -> QVariant {
    QVariant::from(&QString::from(s))
}

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!(< QAbstractListModel >);
        type QAbstractListModel;

        include!("cxx-qt-lib/qmodelindex.h");
        type QModelIndex = cxx_qt_lib::QModelIndex;

        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;

        include!("cxx-qt-lib/qhash.h");
        type QHash_i32_QByteArray = cxx_qt_lib::QHash<cxx_qt_lib::QHashPair_i32_QByteArray>;
    }

    /// Roles for the log line model.
    #[qenum(LogListModel)]
    enum LogRoles {
        Message,
        Stream,
        Timestamp,
    }

    impl cxx_qt::Threading for LogListModel {}

    unsafe extern "RustQt" {
        /// A capped, filterable log line model.
        #[qobject]
        #[qml_element]
        #[base = QAbstractListModel]
        #[qproperty(QString, search_text)]
        type LogListModel = super::LogListModelRust;

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

        /// Append raw text lines (timestamp-prefixed chunks from Rust).
        #[qinvokable]
        #[rust_name = "append_text"]
        fn appendText(self: Pin<&mut Self>, text: &QString);

        /// Clear all lines.
        #[qinvokable]
        #[rust_name = "clear"]
        fn clear(self: Pin<&mut Self>);
    }

    impl cxx_qt::Threading for ContainerDetailController {}

    extern "RustQt" {
        /// Detail page controller.
        #[qobject]
        #[qml_element]
        #[qproperty(QString, container_id)]
        #[qproperty(QString, container_name)]
        #[qproperty(QString, detail_json)]
        #[qproperty(bool, detail_loading)]
        #[qproperty(bool, logs_active)]
        #[qproperty(bool, stats_active)]
        #[qproperty(f64, cpu_percent)]
        #[qproperty(f64, memory_percent)]
        #[qproperty(QString, memory_usage)]
        #[qproperty(QString, memory_limit)]
        #[qproperty(QString, network_rx)]
        #[qproperty(QString, network_tx)]
        #[qproperty(QString, block_read)]
        #[qproperty(QString, block_write)]
        #[qproperty(QString, pids)]
        #[qproperty(QString, cpu_history)]
        type ContainerDetailController = super::ContainerDetailControllerRust;

        /// A chunk of log text produced by the follow stream.
        #[qsignal]
        #[cxx_name = "logChunk"]
        fn log_chunk(self: Pin<&mut Self>, chunk: QString);

        /// Load details for the given container.
        #[qinvokable]
        #[rust_name = "open"]
        fn open(self: Pin<&mut Self>, id: &QString);

        /// Reload the detail JSON.
        #[qinvokable]
        #[rust_name = "refresh_detail"]
        fn refreshDetail(self: Pin<&mut Self>);

        /// Start following logs (emits `logChunk`).
        #[qinvokable]
        #[rust_name = "start_logs"]
        fn startLogs(self: Pin<&mut Self>);

        /// Stop the log stream.
        #[qinvokable]
        #[rust_name = "stop_logs"]
        fn stopLogs(self: Pin<&mut Self>);

        /// Start the stats polling loop.
        #[qinvokable]
        #[rust_name = "start_stats"]
        fn startStats(self: Pin<&mut Self>);

        /// Stop the stats loop.
        #[qinvokable]
        #[rust_name = "stop_stats"]
        fn stopStats(self: Pin<&mut Self>);
    }
}

/// A log line held in the model.
#[derive(Debug, Clone)]
pub struct LogLineView {
    pub timestamp: String,
    pub stream: String,
    pub message: String,
}

/// Rust state for [`qobject::LogListModel`].
#[derive(Default)]
pub struct LogListModelRust {
    pub(crate) lines: Vec<LogLineView>,
    pub(crate) filtered: Vec<usize>,
    search_text: QString,
}

impl qobject::LogListModel {
    fn row_count(&self, _parent: &QModelIndex) -> i32 {
        self.filtered.len() as i32
    }

    fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        let role = qobject::LogRoles { repr: role };
        let Some(&line_idx) = self.filtered.get(index.row() as usize) else {
            return QVariant::default();
        };
        let Some(line) = self.lines.get(line_idx) else {
            return QVariant::default();
        };
        match role {
            qobject::LogRoles::Message => qv(&line.message),
            qobject::LogRoles::Stream => qv(&line.stream),
            qobject::LogRoles::Timestamp => qv(&line.timestamp),
            _ => QVariant::default(),
        }
    }

    fn role_names(&self) -> qobject::QHash_i32_QByteArray {
        let mut hash = qobject::QHash_i32_QByteArray::default();
        hash.insert(qobject::LogRoles::Message.repr, "message".into());
        hash.insert(qobject::LogRoles::Stream.repr, "stream".into());
        hash.insert(qobject::LogRoles::Timestamp.repr, "timestamp".into());
        hash
    }

    /// Append a chunk of text; splits lines, parses optional RFC3339
    /// timestamps, and caps the buffer at the configured limit.
    pub fn append_text(mut self: Pin<&mut Self>, text: &QString) {
        let limit = crate::app_state::settings().log_line_limit.max(1);
        let text = text.to_string();
        for raw in text.split('\n') {
            if raw.is_empty() {
                continue;
            }
            let (stream, message) = if let Some(stripped) = raw.strip_prefix('\u{1}') {
                ("stderr".to_string(), stripped.to_string())
            } else {
                ("stdout".to_string(), raw.to_string())
            };
            let (timestamp, message) = if let Some((ts, rest)) = message.split_once(' ') {
                (ts.to_string(), rest.to_string())
            } else {
                (String::new(), message)
            };
            self.as_mut().rust_mut().lines.push(LogLineView {
                timestamp,
                stream,
                message,
            });
        }

        let mut lines = std::mem::take(&mut self.as_mut().rust_mut().lines);
        if lines.len() > limit {
            let drop = lines.len() - limit;
            lines.drain(..drop);
        }
        self.as_mut().rust_mut().lines = lines;
        let search = self.search_text().to_string().to_lowercase();
        let filtered = self
            .as_mut()
            .rust_mut()
            .lines
            .iter()
            .enumerate()
            .filter(|(_, l)| search.is_empty() || l.message.to_lowercase().contains(&search))
            .map(|(i, _)| i)
            .collect();
        self.as_mut().rust_mut().filtered = filtered;
        self.as_mut().begin_reset_model();
        self.as_mut().end_reset_model();
    }

    pub fn clear(mut self: Pin<&mut Self>) {
        self.as_mut().rust_mut().lines.clear();
        self.as_mut().rust_mut().filtered.clear();
        self.as_mut().begin_reset_model();
        self.as_mut().end_reset_model();
    }
}

/// Rust state for [`qobject::ContainerDetailController`].
#[derive(Default)]
pub struct ContainerDetailControllerRust {
    container_id: QString,
    container_name: QString,
    detail_json: QString,
    detail_loading: bool,
    logs_active: bool,
    stats_active: bool,
    cpu_percent: f64,
    memory_percent: f64,
    memory_usage: QString,
    memory_limit: QString,
    network_rx: QString,
    network_tx: QString,
    block_read: QString,
    block_write: QString,
    pids: QString,
    cpu_history: QString,

    logs_cancel: Option<CancellationToken>,
    stats_cancel: Option<CancellationToken>,
}

impl qobject::ContainerDetailController {
    /// Open details for a container id.
    pub fn open(mut self: Pin<&mut Self>, id: &QString) {
        self.as_mut().stop_logs();
        self.as_mut().stop_stats();
        self.as_mut().set_container_id(id.clone());
        self.as_mut().set_logs_active(false);
        self.as_mut().set_stats_active(false);
        self.as_mut().refresh_detail();
    }

    /// Load and display the inspect JSON.
    pub fn refresh_detail(mut self: Pin<&mut Self>) {
        let Some(services) = get_services() else {
            return;
        };
        let id = self.container_id().to_string();
        self.as_mut().set_detail_loading(true);
        let qt_thread = self.qt_thread();
        crate::runtime::spawn(async move {
            let result = services.containers.inspect_container(&id).await;
            qt_thread
                .queue(move |mut controller| {
                    controller.as_mut().set_detail_loading(false);
                    match result {
                        Ok(detail) => {
                            let name = detail.summary.name.clone();
                            let json = serde_json::to_string_pretty(&detail)
                                .unwrap_or_else(|_| "{}".to_string());
                            controller.as_mut().set_container_name(QString::from(name));
                            controller.as_mut().set_detail_json(QString::from(json));
                        }
                        Err(e) => {
                            tracing::debug!(container = %id, error = %e, "inspect failed");
                            controller.as_mut().set_detail_json(QString::from(format!(
                                "{{ \"error\": {} }}",
                                serde_json::to_string(&e.to_string()).unwrap_or_default()
                            )));
                        }
                    }
                })
                .unwrap_or_else(|error| tracing::debug!(%error, "Qt object destroyed before async result delivery"));
        });
    }

    /// Start following container logs; each chunk is emitted as a signal.
    pub fn start_logs(mut self: Pin<&mut Self>) {
        if *self.logs_active() {
            return;
        }
        let Some(services) = get_services() else {
            return;
        };
        let id = self.container_id().to_string();
        let cancel = CancellationToken::new();
        self.as_mut().rust_mut().logs_cancel = Some(cancel.clone());
        self.as_mut().set_logs_active(true);

        let options = tuxstack_docker_core::ContainerLogsOptions::follow();
        let qt_thread = self.qt_thread();
        crate::runtime::spawn(async move {
            let mut stream = services
                .containers
                .watch_logs(&id, &options, cancel.clone());
            use futures_util::StreamExt;
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    item = stream.next() => {
                        match item {
                            Some(Ok(line)) => {
                                let chunk = match line.stream {
                                    tuxstack_docker_core::LogStream::Stderr => {
                                        format!("\u{1}{}\n", line.message)
                                    }
                                    _ => format!("{}\n", line.message),
                                };
                                let qs = QString::from(&chunk);
                                let thread = qt_thread.clone();
                                crate::runtime::spawn(async move {
                                    if let Err(e) = thread.queue(move |mut c| {
                                        c.as_mut().log_chunk(qs);
                                    }) {
                                        tracing::debug!(error = %e, "log chunk queue failed");
                                    }
                                });
                            }
                            Some(Err(e)) => {
                                tracing::debug!(container = %id, error = %e, "log stream ended");
                                let msg = format!("\n[log stream ended: {e}]\n");
                                let qs = QString::from(&msg);
                                let thread = qt_thread.clone();
                                crate::runtime::spawn(async move {
                                    if let Err(e) = thread.queue(move |mut c| {
                                        c.as_mut().log_chunk(qs);
                                    }) {
                                        tracing::debug!(error = %e, "log chunk queue failed");
                                    }
                                });
                                break;
                            }
                            None => break,
                        }
                    }
                }
            }
        });
    }

    /// Stop the log stream.
    pub fn stop_logs(mut self: Pin<&mut Self>) {
        if let Some(cancel) = self.as_mut().rust_mut().logs_cancel.take() {
            cancel.cancel();
        }
        self.as_mut().set_logs_active(false);
    }

    /// Start the stats polling loop (interval from settings).
    pub fn start_stats(mut self: Pin<&mut Self>) {
        if *self.stats_active() {
            return;
        }
        let Some(services) = get_services() else {
            return;
        };
        let id = self.container_id().to_string();
        let cancel = CancellationToken::new();
        self.as_mut().rust_mut().stats_cancel = Some(cancel.clone());
        self.as_mut().set_stats_active(true);
        self.as_mut().set_cpu_history(QString::from(""));

        let interval = crate::app_state::settings().stats_refresh_seconds.max(1);
        let qt_thread = self.qt_thread();
        crate::runtime::spawn(async move {
            let mut ticker = tokio::time::interval(std::time::Duration::from_secs(interval));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let mut first = true;
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    _ = ticker.tick() => {
                        if first { first = false; continue; }
                        match services.containers.container_stats(&id).await {
                            Ok(stats) => {
                                let thread = qt_thread.clone();
                                crate::runtime::spawn(async move {
                                    let _ = thread.queue(move |mut c| {
                                        c.as_mut().apply_stats_sample(stats);
                                    });
                                });
                            }
                            Err(e) => {
                                tracing::debug!(container = %id, error = %e, "stats sample failed");
                            }
                        }
                    }
                }
            }
        });
    }

    /// Stop the stats loop.
    pub fn stop_stats(mut self: Pin<&mut Self>) {
        if let Some(cancel) = self.as_mut().rust_mut().stats_cancel.take() {
            cancel.cancel();
        }
        self.as_mut().set_stats_active(false);
    }
}

impl qobject::ContainerDetailController {
    /// Apply one stats sample to the properties (runs on the Qt thread).
    pub(crate) fn apply_stats_sample(
        mut self: Pin<&mut Self>,
        stats: tuxstack_docker_core::ContainerStats,
    ) {
        use tuxstack_docker_core::format::bytes;

        self.as_mut().set_cpu_percent(stats.cpu_percent);
        self.as_mut().set_memory_percent(stats.memory_percent);
        self.as_mut()
            .set_memory_usage(QString::from(bytes(stats.memory_usage_bytes)));
        self.as_mut()
            .set_memory_limit(QString::from(bytes(stats.memory_limit_bytes)));
        self.as_mut()
            .set_network_rx(QString::from(bytes(stats.network_rx_bytes)));
        self.as_mut()
            .set_network_tx(QString::from(bytes(stats.network_tx_bytes)));
        self.as_mut()
            .set_block_read(QString::from(bytes(stats.block_read_bytes)));
        self.as_mut()
            .set_block_write(QString::from(bytes(stats.block_write_bytes)));
        self.as_mut().set_pids(QString::from(
            stats
                .pids
                .map(|p| p.to_string())
                .unwrap_or_else(|| "-".to_string()),
        ));

        // Keep a rolling history of the last 60 cpu samples as CSV text.
        let history: Vec<String> = self
            .cpu_history()
            .to_string()
            .split(',')
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect();
        let mut history = history;
        history.push(format!("{:.1}", stats.cpu_percent));
        if history.len() > 60 {
            history.remove(0);
        }
        self.as_mut()
            .set_cpu_history(QString::from(&history.join(",")));
    }
}
