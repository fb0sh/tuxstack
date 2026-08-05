//! CXX-Qt bridge for the live Stats and Logs container tabs.
//!
//! Both models own their cancellation tokens. Selection changes, tab
//! deactivation, and shutdown synchronously cancel old Docker streams and
//! invalidate their generations before a late queued result can be applied.

use std::pin::Pin;
use std::time::Duration;

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QList, QMap, QModelIndex, QString, QVariant};
use futures_util::StreamExt;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tuxstack_docker_core::{DockerError, LogLine};

use crate::app_state::get_services;
use crate::controllers::container_logs::{ContainerLogsState, DISCARDED_NOTICE, LogViewportLine};
use crate::controllers::container_stats::{
    ContainerStatsState, MAX_CONCURRENT_STATS_REQUESTS, StatsHistoryPoint, StatsReading,
};

const STATS_ROLE_ID: i32 = 257;
const STATS_ROLE_NAME: i32 = 258;
const STATS_ROLE_CPU: i32 = 259;
const STATS_ROLE_MEMORY_RAW: i32 = 260;
const STATS_ROLE_MEMORY_WORKING: i32 = 261;
const STATS_ROLE_MEMORY_WORKING_KNOWN: i32 = 262;
const STATS_ROLE_MEMORY_LIMIT: i32 = 263;
const STATS_ROLE_NETWORK_RX: i32 = 264;
const STATS_ROLE_NETWORK_TX: i32 = 265;
const STATS_ROLE_BLOCK_READ: i32 = 266;
const STATS_ROLE_BLOCK_WRITE: i32 = 267;
const STATS_ROLE_PIDS: i32 = 268;

const LOG_ROLE_SEQUENCE: i32 = 257;
const LOG_ROLE_CONTAINER_ID: i32 = 258;
const LOG_ROLE_CONTAINER_NAME: i32 = 259;
const LOG_ROLE_STREAM: i32 = 260;
const LOG_ROLE_TIMESTAMP: i32 = 261;
const LOG_ROLE_MESSAGE: i32 = 262;
const LOG_ROLE_DISPLAY: i32 = 263;

type QVariantList = QList<QVariant>;
type QVariantMap = QMap<cxx_qt_lib::QMapPair_QString_QVariant>;

#[derive(Default)]
pub struct ContainerStatsModelRust {
    pub(crate) state: ContainerStatsState,
    pub(crate) rows: Vec<(String, String, StatsReading)>,
    pub(crate) stream_cancel: Option<CancellationToken>,
    pub(crate) status: QString,
    pub(crate) error_message: QString,
    pub(crate) active: bool,
    pub(crate) count: i32,
    pub(crate) running_count: i32,
    pub(crate) reporting_count: i32,
    pub(crate) cpu_percent: f64,
    pub(crate) memory_raw_bytes: i64,
    pub(crate) memory_working_set_bytes: i64,
    pub(crate) memory_working_set_known: bool,
    pub(crate) memory_limit_bytes: i64,
    pub(crate) memory_percent: f64,
    pub(crate) network_rx_bytes: i64,
    pub(crate) network_tx_bytes: i64,
    pub(crate) block_read_bytes: i64,
    pub(crate) block_write_bytes: i64,
    pub(crate) pids: i64,
    pub(crate) history_model: QVariantList,
}

impl Drop for ContainerStatsModelRust {
    fn drop(&mut self) {
        cancel(&mut self.stream_cancel);
    }
}

#[derive(Default)]
pub struct ContainerLogsModelRust {
    pub(crate) state: ContainerLogsState,
    pub(crate) rows: Vec<LogViewportLine>,
    pub(crate) stream_cancel: Option<CancellationToken>,
    pub(crate) status: QString,
    pub(crate) error_message: QString,
    pub(crate) validation_error: QString,
    pub(crate) active: bool,
    pub(crate) count: i32,
    pub(crate) stdout: bool,
    pub(crate) stderr: bool,
    pub(crate) follow: bool,
    pub(crate) timestamps: bool,
    pub(crate) paused: bool,
    pub(crate) wrap: bool,
    pub(crate) tail: QString,
    pub(crate) since: QString,
    pub(crate) search_query: QString,
    pub(crate) member_filter_id: QString,
    pub(crate) member_model: QVariantList,
    pub(crate) group_selection: bool,
    pub(crate) discarded: bool,
    pub(crate) discarded_message: QString,
    pub(crate) pending_count: i32,
}

impl Drop for ContainerLogsModelRust {
    fn drop(&mut self) {
        cancel(&mut self.stream_cancel);
    }
}

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!(<QAbstractListModel>);
        type QAbstractListModel;
        include!("cxx-qt-lib/qmodelindex.h");
        type QModelIndex = cxx_qt_lib::QModelIndex;
        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;
        include!("cxx-qt-lib/core/qlist/qlist_QVariant.h");
        type QList_QVariant = cxx_qt_lib::QList<cxx_qt_lib::QVariant>;
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qhash.h");
        type QHash_i32_QByteArray = cxx_qt_lib::QHash<cxx_qt_lib::QHashPair_i32_QByteArray>;
    }

    impl cxx_qt::Threading for ContainerStatsModel {}

    unsafe extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[base = QAbstractListModel]
        #[qproperty(QString, status)]
        #[qproperty(QString, error_message, cxx_name = "errorMessage")]
        #[qproperty(i32, count)]
        #[qproperty(i32, running_count, cxx_name = "runningCount")]
        #[qproperty(i32, reporting_count, cxx_name = "reportingCount")]
        #[qproperty(f64, cpu_percent, cxx_name = "cpuPercent")]
        #[qproperty(i64, memory_raw_bytes, cxx_name = "memoryRawBytes")]
        #[qproperty(i64, memory_working_set_bytes, cxx_name = "memoryWorkingSetBytes")]
        #[qproperty(bool, memory_working_set_known, cxx_name = "memoryWorkingSetKnown")]
        #[qproperty(i64, memory_limit_bytes, cxx_name = "memoryLimitBytes")]
        #[qproperty(f64, memory_percent, cxx_name = "memoryPercent")]
        #[qproperty(i64, network_rx_bytes, cxx_name = "networkRxBytes")]
        #[qproperty(i64, network_tx_bytes, cxx_name = "networkTxBytes")]
        #[qproperty(i64, block_read_bytes, cxx_name = "blockReadBytes")]
        #[qproperty(i64, block_write_bytes, cxx_name = "blockWriteBytes")]
        #[qproperty(i64, pids)]
        #[qproperty(QList_QVariant, history_model, cxx_name = "historyModel")]
        type ContainerStatsModel = super::ContainerStatsModelRust;

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

        #[qinvokable]
        #[cxx_name = "setSelection"]
        fn set_selection(
            self: Pin<&mut Self>,
            kind: &QString,
            id: &QString,
            container_ids: &QList_QVariant,
            states: &QList_QVariant,
            names: &QList_QVariant,
        );
        #[qinvokable]
        #[cxx_name = "setActive"]
        fn update_active(self: Pin<&mut Self>, active: bool);
        #[qinvokable]
        fn shutdown(self: Pin<&mut Self>);
    }

    impl cxx_qt::Threading for ContainerLogsModel {}

    unsafe extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[base = QAbstractListModel]
        #[qproperty(QString, status)]
        #[qproperty(QString, error_message, cxx_name = "errorMessage")]
        #[qproperty(QString, validation_error, cxx_name = "validationError")]
        #[qproperty(i32, count)]
        #[qproperty(bool, stdout)]
        #[qproperty(bool, stderr)]
        #[qproperty(bool, follow)]
        #[qproperty(bool, timestamps)]
        #[qproperty(bool, paused)]
        #[qproperty(bool, wrap)]
        #[qproperty(QString, tail)]
        #[qproperty(QString, since)]
        #[qproperty(QString, search_query, cxx_name = "searchQuery")]
        #[qproperty(QString, member_filter_id, cxx_name = "memberFilterId")]
        #[qproperty(QList_QVariant, member_model, cxx_name = "memberModel")]
        #[qproperty(bool, group_selection, cxx_name = "groupSelection")]
        #[qproperty(bool, discarded)]
        #[qproperty(QString, discarded_message, cxx_name = "discardedMessage")]
        #[qproperty(i32, pending_count, cxx_name = "pendingCount")]
        type ContainerLogsModel = super::ContainerLogsModelRust;

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

        #[qsignal]
        #[cxx_name = "saveCompleted"]
        fn save_completed(self: Pin<&mut Self>, path: QString);
        #[qsignal]
        #[cxx_name = "saveFailed"]
        fn save_failed(self: Pin<&mut Self>, message: QString);

        #[qinvokable]
        #[cxx_name = "setSelection"]
        fn set_selection(
            self: Pin<&mut Self>,
            kind: &QString,
            id: &QString,
            container_ids: &QList_QVariant,
            states: &QList_QVariant,
            names: &QList_QVariant,
        );
        #[qinvokable]
        #[cxx_name = "setActive"]
        fn update_active(self: Pin<&mut Self>, active: bool);
        #[qinvokable]
        #[cxx_name = "updateStdout"]
        fn update_stdout(self: Pin<&mut Self>, enabled: bool);
        #[qinvokable]
        #[cxx_name = "updateStderr"]
        fn update_stderr(self: Pin<&mut Self>, enabled: bool);
        #[qinvokable]
        #[cxx_name = "updateFollow"]
        fn update_follow(self: Pin<&mut Self>, follow: bool);
        #[qinvokable]
        #[cxx_name = "updateTimestamps"]
        fn update_timestamps(self: Pin<&mut Self>, timestamps: bool);
        #[qinvokable]
        #[cxx_name = "updateTail"]
        fn update_tail(self: Pin<&mut Self>, tail: &QString);
        #[qinvokable]
        #[cxx_name = "updateSince"]
        fn update_since(self: Pin<&mut Self>, since: &QString);
        #[qinvokable]
        #[cxx_name = "setMemberFilter"]
        fn set_member_filter(self: Pin<&mut Self>, container_id: &QString);
        #[qinvokable]
        #[cxx_name = "updatePaused"]
        fn update_paused(self: Pin<&mut Self>, paused: bool);
        #[qinvokable]
        #[cxx_name = "updateWrap"]
        fn update_wrap(self: Pin<&mut Self>, wrap: bool);
        #[qinvokable]
        #[cxx_name = "setSearch"]
        fn set_search(self: Pin<&mut Self>, query: &QString);
        #[qinvokable]
        #[cxx_name = "clearViewport"]
        fn clear_viewport(self: Pin<&mut Self>);
        #[qinvokable]
        #[cxx_name = "saveViewport"]
        fn save_viewport(self: Pin<&mut Self>, destination: &QString);
        #[qinvokable]
        #[cxx_name = "viewportText"]
        fn viewport_text(&self) -> QString;
        #[qinvokable]
        fn shutdown(self: Pin<&mut Self>);
    }
}

impl qobject::ContainerStatsModel {
    pub(crate) fn row_count(&self, _parent: &QModelIndex) -> i32 {
        saturating_i32(self.rows.len())
    }

    pub(crate) fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        let Some((id, name, row)) = self.rows.get(index.row() as usize) else {
            return QVariant::default();
        };
        match role {
            STATS_ROLE_ID => qv(id),
            STATS_ROLE_NAME => qv(name),
            STATS_ROLE_CPU => QVariant::from(&row.cpu_percent),
            STATS_ROLE_MEMORY_RAW => QVariant::from(&saturating_i64(row.memory_raw_bytes)),
            STATS_ROLE_MEMORY_WORKING => {
                QVariant::from(&saturating_i64(row.memory_working_set_bytes.unwrap_or(0)))
            }
            STATS_ROLE_MEMORY_WORKING_KNOWN => {
                QVariant::from(&row.memory_working_set_bytes.is_some())
            }
            STATS_ROLE_MEMORY_LIMIT => QVariant::from(&saturating_i64(row.memory_limit_bytes)),
            STATS_ROLE_NETWORK_RX => QVariant::from(&saturating_i64(row.network_rx_bytes)),
            STATS_ROLE_NETWORK_TX => QVariant::from(&saturating_i64(row.network_tx_bytes)),
            STATS_ROLE_BLOCK_READ => QVariant::from(&saturating_i64(row.block_read_bytes)),
            STATS_ROLE_BLOCK_WRITE => QVariant::from(&saturating_i64(row.block_write_bytes)),
            STATS_ROLE_PIDS => QVariant::from(&saturating_i64(row.pids)),
            _ => QVariant::default(),
        }
    }

    pub(crate) fn role_names(&self) -> qobject::QHash_i32_QByteArray {
        role_hash(&[
            (STATS_ROLE_ID, "containerId"),
            (STATS_ROLE_NAME, "containerName"),
            (STATS_ROLE_CPU, "cpuPercent"),
            (STATS_ROLE_MEMORY_RAW, "memoryRawBytes"),
            (STATS_ROLE_MEMORY_WORKING, "memoryWorkingSetBytes"),
            (STATS_ROLE_MEMORY_WORKING_KNOWN, "memoryWorkingSetKnown"),
            (STATS_ROLE_MEMORY_LIMIT, "memoryLimitBytes"),
            (STATS_ROLE_NETWORK_RX, "networkRxBytes"),
            (STATS_ROLE_NETWORK_TX, "networkTxBytes"),
            (STATS_ROLE_BLOCK_READ, "blockReadBytes"),
            (STATS_ROLE_BLOCK_WRITE, "blockWriteBytes"),
            (STATS_ROLE_PIDS, "pids"),
        ])
    }

    pub fn set_selection(
        mut self: Pin<&mut Self>,
        kind: &QString,
        id: &QString,
        container_ids: &QVariantList,
        states: &QVariantList,
        names: &QVariantList,
    ) {
        let ids = strings(container_ids);
        let states = strings(states);
        let names = strings(names);
        if self.as_mut().rust_mut().state.set_selection(
            &kind.to_string(),
            &id.to_string(),
            &ids,
            &states,
            &names,
        ) {
            self.as_mut().cancel_stream();
            self.as_mut().publish_stats();
            self.as_mut().restart_stats();
        }
    }

    pub fn update_active(mut self: Pin<&mut Self>, active: bool) {
        if !active {
            self.as_mut().cancel_stream();
        }
        if self.as_mut().rust_mut().state.set_active(active) {
            self.as_mut().publish_stats();
            if active {
                self.as_mut().restart_stats();
            }
        }
    }

    pub fn shutdown(mut self: Pin<&mut Self>) {
        self.as_mut().cancel_stream();
        self.as_mut().rust_mut().state.shutdown();
        self.as_mut().publish_stats();
    }

    fn cancel_stream(mut self: Pin<&mut Self>) {
        cancel(&mut self.as_mut().rust_mut().stream_cancel);
    }

    fn restart_stats(mut self: Pin<&mut Self>) {
        self.as_mut().cancel_stream();
        let Some((generation, targets)) = self.as_mut().rust_mut().state.begin_stream() else {
            self.as_mut().publish_stats();
            return;
        };
        let Some(services) = get_services() else {
            self.as_mut()
                .rust_mut()
                .state
                .apply_error(generation, "Docker Engine is unavailable.");
            self.as_mut().publish_stats();
            return;
        };
        let cancel = CancellationToken::new();
        self.as_mut().rust_mut().stream_cancel = Some(cancel.clone());
        self.as_mut().publish_stats();
        let qt = self.qt_thread();
        if targets.len() > MAX_CONCURRENT_STATS_REQUESTS {
            // Long-lived streams would permanently occupy all permits and
            // starve later group members. Poll every running member instead,
            // with at most eight concurrent Docker requests per round.
            crate::runtime::spawn(async move {
                loop {
                    let requests =
                        futures_util::stream::iter(targets.clone().into_iter().map(|target| {
                            let services = services.clone();
                            async move {
                                let result = services.containers.container_stats(&target.id).await;
                                (target.id, result)
                            }
                        }))
                        .buffer_unordered(MAX_CONCURRENT_STATS_REQUESTS);
                    tokio::pin!(requests);
                    let mut samples = Vec::with_capacity(targets.len());
                    let mut last_error = None;
                    loop {
                        tokio::select! {
                            _ = cancel.cancelled() => return,
                            item = requests.next() => match item {
                                Some((id, Ok(sample))) => samples.push((id, StatsReading::from(sample))),
                                Some((_, Err(error))) => last_error = Some(live_error(&error)),
                                None => break,
                            }
                        }
                    }
                    if qt
                        .queue(move |mut model| {
                            let mut changed = false;
                            for (id, sample) in samples {
                                changed |= model
                                    .as_mut()
                                    .rust_mut()
                                    .state
                                    .apply_sample(generation, &id, sample);
                            }
                            if changed {
                                model.as_mut().publish_stats();
                            } else if let Some(message) = last_error {
                                if model
                                    .as_mut()
                                    .rust_mut()
                                    .state
                                    .apply_error(generation, &message)
                                {
                                    model.as_mut().publish_stats();
                                }
                            }
                        })
                        .is_err()
                    {
                        return;
                    }
                    tokio::select! {
                        _ = cancel.cancelled() => return,
                        _ = tokio::time::sleep(Duration::from_secs(1)) => {}
                    }
                }
            });
        } else {
            for target in targets {
                let services = services.clone();
                let task_cancel = cancel.child_token();
                let qt = qt.clone();
                crate::runtime::spawn(async move {
                    let mut stream = services
                        .containers
                        .watch_stats(&target.id, task_cancel.clone());
                    while let Some(item) = stream.next().await {
                        if task_cancel.is_cancelled() {
                            break;
                        }
                        let id = target.id.clone();
                        match item {
                            Ok(sample) => {
                                if qt
                                    .queue(move |mut model| {
                                        if model.as_mut().rust_mut().state.apply_sample(
                                            generation,
                                            &id,
                                            StatsReading::from(sample),
                                        ) {
                                            model.as_mut().publish_stats();
                                        }
                                    })
                                    .is_err()
                                {
                                    break;
                                }
                            }
                            Err(error) => {
                                let message = live_error(&error);
                                qt.queue(move |mut model| {
                                    if model
                                        .as_mut()
                                        .rust_mut()
                                        .state
                                        .apply_error(generation, &message)
                                    {
                                        model.as_mut().publish_stats();
                                    }
                                })
                                .ok();
                                break;
                            }
                        }
                    }
                });
            }
        }
    }

    fn publish_stats(mut self: Pin<&mut Self>) {
        let state = self.state.clone();
        let aggregate = state.aggregate();
        let rows = state
            .targets
            .iter()
            .filter_map(|target| {
                state
                    .latest
                    .get(&target.id)
                    .cloned()
                    .map(|sample| (target.id.clone(), target.name.clone(), sample))
            })
            .collect::<Vec<_>>();
        let count = saturating_i32(rows.len());
        self.as_mut().begin_reset_model();
        self.as_mut().rust_mut().rows = rows;
        self.as_mut().end_reset_model();
        self.as_mut()
            .set_status(QString::from(state.status.as_str()));
        self.as_mut()
            .set_error_message(QString::from(&state.error_message));
        self.as_mut().set_count(count);
        self.as_mut()
            .set_running_count(saturating_i32(state.targets.len()));
        self.as_mut()
            .set_reporting_count(saturating_i32(aggregate.reporting_count));
        self.as_mut().set_cpu_percent(aggregate.cpu_percent);
        self.as_mut()
            .set_memory_raw_bytes(saturating_i64(aggregate.memory_raw_bytes));
        self.as_mut().set_memory_working_set_bytes(saturating_i64(
            aggregate.memory_working_set_bytes.unwrap_or(0),
        ));
        self.as_mut()
            .set_memory_working_set_known(aggregate.memory_working_set_bytes.is_some());
        self.as_mut()
            .set_memory_limit_bytes(saturating_i64(aggregate.memory_limit_bytes));
        self.as_mut().set_memory_percent(state.memory_percent());
        self.as_mut()
            .set_network_rx_bytes(saturating_i64(aggregate.network_rx_bytes));
        self.as_mut()
            .set_network_tx_bytes(saturating_i64(aggregate.network_tx_bytes));
        self.as_mut()
            .set_block_read_bytes(saturating_i64(aggregate.block_read_bytes));
        self.as_mut()
            .set_block_write_bytes(saturating_i64(aggregate.block_write_bytes));
        self.as_mut().set_pids(saturating_i64(aggregate.pids));
        self.as_mut()
            .set_history_model(history_variants(state.history.iter()));
    }
}

impl qobject::ContainerLogsModel {
    pub(crate) fn row_count(&self, _parent: &QModelIndex) -> i32 {
        saturating_i32(self.rows.len())
    }

    pub(crate) fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        let Some(row) = self.rows.get(index.row() as usize) else {
            return QVariant::default();
        };
        match role {
            LOG_ROLE_SEQUENCE => QVariant::from(&saturating_i64(row.sequence)),
            LOG_ROLE_CONTAINER_ID => qv(&row.container_id),
            LOG_ROLE_CONTAINER_NAME => qv(&row.container_name),
            LOG_ROLE_STREAM => qv(&row.stream),
            LOG_ROLE_TIMESTAMP => qv(&row.timestamp),
            LOG_ROLE_MESSAGE => qv(&row.message),
            LOG_ROLE_DISPLAY => qv(&row.display),
            _ => QVariant::default(),
        }
    }

    pub(crate) fn role_names(&self) -> qobject::QHash_i32_QByteArray {
        role_hash(&[
            (LOG_ROLE_SEQUENCE, "sequence"),
            (LOG_ROLE_CONTAINER_ID, "containerId"),
            (LOG_ROLE_CONTAINER_NAME, "containerName"),
            (LOG_ROLE_STREAM, "stream"),
            (LOG_ROLE_TIMESTAMP, "timestamp"),
            (LOG_ROLE_MESSAGE, "message"),
            (LOG_ROLE_DISPLAY, "displayText"),
        ])
    }

    pub fn set_selection(
        mut self: Pin<&mut Self>,
        kind: &QString,
        id: &QString,
        container_ids: &QVariantList,
        states: &QVariantList,
        names: &QVariantList,
    ) {
        let ids = strings(container_ids);
        let states = strings(states);
        let names = strings(names);
        if self.as_mut().rust_mut().state.set_selection(
            &kind.to_string(),
            &id.to_string(),
            &ids,
            &states,
            &names,
        ) {
            self.as_mut().cancel_stream();
            self.as_mut().publish_logs();
            self.as_mut().restart_logs();
        }
    }

    pub fn update_active(mut self: Pin<&mut Self>, active: bool) {
        if !active {
            self.as_mut().cancel_stream();
        }
        if self.as_mut().rust_mut().state.set_active(active) {
            self.as_mut().publish_logs();
            if active {
                self.as_mut().restart_logs();
            }
        }
    }

    pub fn update_stdout(mut self: Pin<&mut Self>, enabled: bool) {
        self.as_mut()
            .update_stream_option(|state| state.set_stdout(enabled), true);
    }

    pub fn update_stderr(mut self: Pin<&mut Self>, enabled: bool) {
        self.as_mut()
            .update_stream_option(|state| state.set_stderr(enabled), true);
    }

    pub fn update_follow(mut self: Pin<&mut Self>, follow: bool) {
        self.as_mut()
            .update_stream_option(|state| state.set_follow(follow), false);
    }

    pub fn update_timestamps(mut self: Pin<&mut Self>, timestamps: bool) {
        self.as_mut()
            .update_stream_option(|state| state.set_timestamps(timestamps), true);
    }

    pub fn update_tail(mut self: Pin<&mut Self>, tail: &QString) {
        let tail = tail.to_string();
        self.as_mut()
            .update_stream_option(|state| state.set_tail(&tail), true);
    }

    pub fn update_since(mut self: Pin<&mut Self>, since: &QString) {
        let since = since.to_string();
        self.as_mut()
            .update_stream_option(|state| state.set_since(&since), true);
    }

    pub fn set_member_filter(mut self: Pin<&mut Self>, container_id: &QString) {
        if self
            .as_mut()
            .rust_mut()
            .state
            .set_member_filter(&container_id.to_string())
        {
            self.as_mut().publish_logs();
        }
    }

    pub fn update_paused(mut self: Pin<&mut Self>, paused: bool) {
        if self.as_mut().rust_mut().state.set_paused(paused) {
            self.as_mut().publish_logs();
        }
    }

    pub fn update_wrap(mut self: Pin<&mut Self>, wrap: bool) {
        if self.as_mut().rust_mut().state.set_wrap(wrap) {
            self.as_mut().publish_logs();
        }
    }

    pub fn set_search(mut self: Pin<&mut Self>, query: &QString) {
        if self
            .as_mut()
            .rust_mut()
            .state
            .set_search(&query.to_string())
        {
            self.as_mut().publish_logs();
        }
    }

    pub fn clear_viewport(mut self: Pin<&mut Self>) {
        self.as_mut().rust_mut().state.clear_viewport();
        self.as_mut().publish_logs();
    }

    pub fn save_viewport(mut self: Pin<&mut Self>, destination: &QString) {
        let path = match local_path(&destination.to_string()) {
            Ok(path) => path,
            Err(message) => {
                self.as_mut().save_failed(QString::from(message));
                return;
            }
        };
        let text = self.state.save_text();
        let qt = self.qt_thread();
        crate::runtime::spawn(async move {
            let result = tokio::fs::write(&path, text.as_bytes()).await;
            qt.queue(move |mut model| match result {
                Ok(()) => model
                    .as_mut()
                    .save_completed(QString::from(path.display().to_string())),
                Err(error) => model.as_mut().save_failed(QString::from(format!(
                    "Could not save the current log viewport: {error}"
                ))),
            })
            .ok();
        });
    }

    pub fn viewport_text(&self) -> QString {
        QString::from(self.state.save_text())
    }

    pub fn shutdown(mut self: Pin<&mut Self>) {
        self.as_mut().cancel_stream();
        self.as_mut().rust_mut().state.shutdown();
        self.as_mut().publish_logs();
    }

    fn cancel_stream(mut self: Pin<&mut Self>) {
        cancel(&mut self.as_mut().rust_mut().stream_cancel);
    }

    fn update_stream_option(
        mut self: Pin<&mut Self>,
        update: impl FnOnce(&mut ContainerLogsState) -> bool,
        clear_viewport: bool,
    ) {
        let old_generation = self.state.generation;
        if !update(&mut self.as_mut().rust_mut().state) {
            return;
        }
        let restart = self.state.generation != old_generation;
        if restart {
            self.as_mut().cancel_stream();
            if clear_viewport {
                self.as_mut().rust_mut().state.clear_viewport();
            }
        }
        self.as_mut().publish_logs();
        if restart {
            self.as_mut().restart_logs();
        }
    }

    fn restart_logs(mut self: Pin<&mut Self>) {
        self.as_mut().cancel_stream();
        let Some((generation, targets)) = self.as_mut().rust_mut().state.begin_stream() else {
            self.as_mut().publish_logs();
            return;
        };
        let Some(services) = get_services() else {
            self.as_mut()
                .rust_mut()
                .state
                .apply_error(generation, "Docker Engine is unavailable.");
            self.as_mut().publish_logs();
            return;
        };
        let include_history = self.state.entries.is_empty();
        let options_state = self.state.clone();
        let cancel = CancellationToken::new();
        self.as_mut().rust_mut().stream_cancel = Some(cancel.clone());
        self.as_mut().publish_logs();

        // Stream tasks feed one bounded queue. A single flusher crosses to Qt
        // at most every 50 ms or 128 entries, instead of spawning per line.
        let (sender, mut receiver) = mpsc::channel::<(String, LogLine)>(4096);
        let qt = self.qt_thread();
        for target in targets {
            let services = services.clone();
            let options = options_state.docker_options(target.running, include_history);
            let sender = sender.clone();
            let task_cancel = cancel.child_token();
            let target_qt = qt.clone();
            crate::runtime::spawn(async move {
                let mut stream =
                    services
                        .containers
                        .watch_logs(&target.id, &options, task_cancel.clone());
                while let Some(item) = stream.next().await {
                    if task_cancel.is_cancelled() {
                        break;
                    }
                    match item {
                        Ok(line) => {
                            if sender.send((target.id.clone(), line)).await.is_err() {
                                break;
                            }
                        }
                        Err(error) => {
                            let message = live_error(&error);
                            target_qt
                                .queue(move |mut model| {
                                    if model
                                        .as_mut()
                                        .rust_mut()
                                        .state
                                        .apply_error(generation, &message)
                                    {
                                        model.as_mut().publish_logs();
                                    }
                                })
                                .ok();
                            break;
                        }
                    }
                }
            });
        }
        drop(sender);
        crate::runtime::spawn(async move {
            let mut batch = Vec::with_capacity(128);
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    item = receiver.recv() => {
                        match item {
                            Some(item) => batch.push(item),
                            None => {
                                flush_log_batch(&qt, generation, &mut batch);
                                break;
                            }
                        }
                    }
                    _ = tokio::time::sleep(Duration::from_millis(50)), if !batch.is_empty() => {
                        flush_log_batch(&qt, generation, &mut batch);
                    }
                }
                if batch.len() >= 128 {
                    flush_log_batch(&qt, generation, &mut batch);
                }
            }
        });
    }

    fn publish_logs(mut self: Pin<&mut Self>) {
        let state = self.state.clone();
        let rows = state
            .visible_entries()
            .into_iter()
            .cloned()
            .collect::<Vec<_>>();
        let count = saturating_i32(rows.len());
        self.as_mut().begin_reset_model();
        self.as_mut().rust_mut().rows = rows;
        self.as_mut().end_reset_model();
        self.as_mut()
            .set_status(QString::from(state.status.as_str()));
        self.as_mut()
            .set_error_message(QString::from(&state.error_message));
        self.as_mut()
            .set_validation_error(QString::from(&state.validation_error));
        self.as_mut().set_count(count);
        self.as_mut().set_stdout(state.stdout);
        self.as_mut().set_stderr(state.stderr);
        self.as_mut().set_follow(state.follow);
        self.as_mut().set_timestamps(state.timestamps);
        self.as_mut().set_paused(state.paused);
        self.as_mut().set_wrap(state.wrap);
        self.as_mut().set_tail(QString::from(state.tail.as_str()));
        self.as_mut().set_since(QString::from(&state.since_input));
        self.as_mut()
            .set_search_query(QString::from(&state.search_query));
        self.as_mut()
            .set_member_filter_id(QString::from(&state.member_filter_id));
        self.as_mut().set_member_model(log_member_variants(&state));
        self.as_mut()
            .set_group_selection(state.selection_kind == "group");
        self.as_mut().set_discarded(state.discarded);
        self.as_mut()
            .set_discarded_message(QString::from(if state.discarded {
                DISCARDED_NOTICE
            } else {
                ""
            }));
        self.as_mut()
            .set_pending_count(saturating_i32(state.pending.len()));
    }
}

fn flush_log_batch(
    qt: &cxx_qt::CxxQtThread<qobject::ContainerLogsModel>,
    generation: u64,
    batch: &mut Vec<(String, LogLine)>,
) {
    if batch.is_empty() {
        return;
    }
    let batch = std::mem::take(batch);
    qt.queue(move |mut model| {
        if model
            .as_mut()
            .rust_mut()
            .state
            .apply_batch(generation, batch)
        {
            model.as_mut().publish_logs();
        }
    })
    .ok();
}

fn strings(values: &QVariantList) -> Vec<String> {
    values
        .iter()
        .filter_map(|value| value.value::<QString>())
        .map(|value| value.to_string())
        .collect()
}

fn log_member_variants(state: &ContainerLogsState) -> QVariantList {
    let mut result = QVariantList::default();
    for option in state.member_options() {
        let mut map = QVariantMap::default();
        map.insert(QString::from("id"), qv(&option.id));
        map.insert(QString::from("name"), qv(&option.name));
        map.insert(QString::from("label"), qv(&option.label));
        result.append(QVariant::from(&map));
    }
    result
}

fn history_variants<'a>(history: impl Iterator<Item = &'a StatsHistoryPoint>) -> QVariantList {
    let mut result = QVariantList::default();
    for point in history {
        let mut map = QVariantMap::default();
        map.insert(
            QString::from("sampledAt"),
            qv(&point.sampled_at.to_rfc3339()),
        );
        map.insert(
            QString::from("cpuPercent"),
            QVariant::from(&point.cpu_percent),
        );
        map.insert(
            QString::from("memoryRawBytes"),
            QVariant::from(&saturating_i64(point.memory_raw_bytes)),
        );
        map.insert(
            QString::from("memoryWorkingSetBytes"),
            QVariant::from(&saturating_i64(point.memory_working_set_bytes.unwrap_or(0))),
        );
        map.insert(
            QString::from("memoryWorkingSetKnown"),
            QVariant::from(&point.memory_working_set_bytes.is_some()),
        );
        map.insert(
            QString::from("networkRxBytes"),
            QVariant::from(&saturating_i64(point.network_rx_bytes)),
        );
        map.insert(
            QString::from("networkTxBytes"),
            QVariant::from(&saturating_i64(point.network_tx_bytes)),
        );
        result.append(QVariant::from(&map));
    }
    result
}

fn role_hash(pairs: &[(i32, &'static str)]) -> qobject::QHash_i32_QByteArray {
    let mut roles = qobject::QHash_i32_QByteArray::default();
    for (role, name) in pairs {
        roles.insert(*role, (*name).into());
    }
    roles
}

fn live_error(error: &DockerError) -> String {
    match error {
        DockerError::EngineUnavailable | DockerError::SocketNotFound(_) => {
            "Docker Engine is unavailable.".into()
        }
        DockerError::PermissionDenied => "Permission denied while reading container data.".into(),
        DockerError::ContainerNotFound(_) => "The selected container no longer exists.".into(),
        DockerError::OperationCancelled => "The live stream was cancelled.".into(),
        other => format!("Docker live data failed: {other}"),
    }
}

fn local_path(value: &str) -> Result<std::path::PathBuf, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("A save destination is required.".into());
    }
    let path = if let Some(rest) = value.strip_prefix("file://") {
        let encoded = if rest.starts_with('/') {
            rest
        } else if let Some(rest) = rest.strip_prefix("localhost/") {
            return decode_file_url_path(&format!("/{rest}"));
        } else {
            return Err("The destination must be a local file URL.".into());
        };
        percent_decode(encoded)?
    } else if let Some(encoded) = value.strip_prefix("file:") {
        percent_decode(encoded)?
    } else if value.contains("://") {
        return Err("The destination must be a local path.".into());
    } else {
        value.to_string()
    };
    if path.contains('\0') {
        return Err("The destination is invalid.".into());
    }
    Ok(path.into())
}

fn decode_file_url_path(value: &str) -> Result<std::path::PathBuf, String> {
    let path = percent_decode(value)?;
    if path.contains('\0') {
        Err("The destination is invalid.".into())
    } else {
        Ok(path.into())
    }
}

fn percent_decode(value: &str) -> Result<String, String> {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return Err("The destination contains an invalid URL escape.".into());
            }
            let (Some(high), Some(low)) = (hex(bytes[index + 1]), hex(bytes[index + 2])) else {
                return Err("The destination contains an invalid URL escape.".into());
            };
            output.push(high * 16 + low);
            index += 3;
        } else {
            output.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(output).map_err(|_| "The destination is not valid UTF-8.".to_string())
}

fn hex(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn qv(value: &str) -> QVariant {
    QVariant::from(&QString::from(value))
}

fn cancel(slot: &mut Option<CancellationToken>) {
    if let Some(token) = slot.take() {
        token.cancel();
    }
}

fn saturating_i32(value: usize) -> i32 {
    value.min(i32::MAX as usize) as i32
}

fn saturating_i64(value: u64) -> i64 {
    value.min(i64::MAX as u64) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_destinations_decode_local_urls_and_reject_remote_urls() {
        assert_eq!(
            local_path("file:///tmp/My%20Logs.log").unwrap(),
            std::path::PathBuf::from("/tmp/My Logs.log")
        );
        assert_eq!(
            local_path("file://localhost/tmp/logs.log").unwrap(),
            std::path::PathBuf::from("/tmp/logs.log")
        );
        assert!(local_path("file://example.com/tmp/logs.log").is_err());
        assert!(local_path("file:///tmp/bad%2").is_err());
    }
}
