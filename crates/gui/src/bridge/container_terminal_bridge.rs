//! CXX-Qt bridge for a real interactive Docker container terminal.
//!
//! The Rust side owns the Docker exec session and a `vt100` parser. QML sees
//! only rendered rows and cursor metadata; raw terminal output is never
//! exposed, logged, cached, or converted to a transcript.

use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QModelIndex, QString, QVariant};
use futures_util::StreamExt;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tuxstack_docker_core::{
    ContainerTerminalError, ContainerTerminalOptions, ContainerTerminalOutput,
    ContainerTerminalOutputStream, ContainerTerminalSession, ContainerTerminalState,
};

use crate::app_state::get_services;
use crate::controllers::container_terminal::{
    ContainerTerminalControllerState, ContainerTerminalRenderer, TerminalSelectionAction,
    terminal_key_bytes, terminal_paste_bytes,
};

const ROLE_TEXT: i32 = 257;
const INPUT_QUEUE_CAPACITY: usize = 256;
const OUTPUT_PUBLISH_INTERVAL: Duration = Duration::from_millis(16);
const OUTPUT_PUBLISH_BYTES: usize = 64 * 1024;

pub struct ContainerTerminalModelRust {
    pub(crate) state: ContainerTerminalControllerState,
    pub(crate) renderer: ContainerTerminalRenderer,
    pub(crate) rows: Vec<String>,
    pub(crate) session: Option<Arc<ContainerTerminalSession>>,
    pub(crate) connection_cancel: Option<CancellationToken>,
    pub(crate) output_cancel: Option<CancellationToken>,
    pub(crate) resize_cancel: Option<CancellationToken>,
    pub(crate) input_tx: Option<mpsc::Sender<Vec<u8>>>,

    pub(crate) terminal_state: QString,
    pub(crate) error_kind: QString,
    pub(crate) error_message: QString,
    pub(crate) container_id: QString,
    pub(crate) shell: QString,
    pub(crate) count: i32,
    pub(crate) column_count: i32,
    pub(crate) cursor_row: i32,
    pub(crate) cursor_column: i32,
    pub(crate) cursor_visible: bool,
    pub(crate) alternate_screen: bool,
    pub(crate) scrollback_offset: i32,
    pub(crate) running: bool,
    pub(crate) active: bool,
}

impl Default for ContainerTerminalModelRust {
    fn default() -> Self {
        let state = ContainerTerminalControllerState::default();
        let renderer = ContainerTerminalRenderer::new(0, state.rows, state.columns);
        let snapshot = renderer.snapshot();
        Self {
            state,
            renderer,
            rows: snapshot.rows,
            session: None,
            connection_cancel: None,
            output_cancel: None,
            resize_cancel: None,
            input_tx: None,
            terminal_state: QString::from("idle"),
            error_kind: QString::default(),
            error_message: QString::default(),
            container_id: QString::default(),
            shell: QString::default(),
            count: i32::from(snapshot.row_count),
            column_count: i32::from(snapshot.column_count),
            cursor_row: i32::from(snapshot.cursor_row),
            cursor_column: i32::from(snapshot.cursor_column),
            cursor_visible: snapshot.cursor_visible,
            alternate_screen: snapshot.alternate_screen,
            scrollback_offset: saturating_i32(snapshot.scrollback),
            running: false,
            active: false,
        }
    }
}

impl Drop for ContainerTerminalModelRust {
    fn drop(&mut self) {
        cancel(&mut self.connection_cancel);
        cancel(&mut self.output_cancel);
        cancel(&mut self.resize_cancel);
        self.input_tx.take();
        // Dropping the final Arc cancels and aborts the backend pumps. Normal
        // application shutdown calls `shutdown`, which closes asynchronously.
        self.session.take();
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
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qhash.h");
        type QHash_i32_QByteArray = cxx_qt_lib::QHash<cxx_qt_lib::QHashPair_i32_QByteArray>;
    }

    impl cxx_qt::Threading for ContainerTerminalModel {}

    unsafe extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[base = QAbstractListModel]
        #[qproperty(QString, terminal_state, cxx_name = "terminalState")]
        #[qproperty(QString, error_kind, cxx_name = "errorKind")]
        #[qproperty(QString, error_message, cxx_name = "errorMessage")]
        #[qproperty(QString, container_id, cxx_name = "containerId")]
        #[qproperty(QString, shell)]
        #[qproperty(i32, count)]
        #[qproperty(i32, column_count, cxx_name = "columnCount")]
        #[qproperty(i32, cursor_row, cxx_name = "cursorRow")]
        #[qproperty(i32, cursor_column, cxx_name = "cursorColumn")]
        #[qproperty(bool, cursor_visible, cxx_name = "cursorVisible")]
        #[qproperty(bool, alternate_screen, cxx_name = "alternateScreen")]
        #[qproperty(i32, scrollback_offset, cxx_name = "scrollbackOffset")]
        #[qproperty(bool, running)]
        #[qproperty(bool, active, READ, NOTIFY)]
        type ContainerTerminalModel = super::ContainerTerminalModelRust;

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
        fn set_selection(self: Pin<&mut Self>, container_id: &QString, running: bool);
        #[qinvokable]
        #[cxx_name = "clearSelection"]
        fn clear_selection(self: Pin<&mut Self>);
        #[qinvokable]
        #[cxx_name = "setActive"]
        fn update_active(self: Pin<&mut Self>, active: bool);
        #[qinvokable]
        #[cxx_name = "sendText"]
        fn send_text(self: Pin<&mut Self>, text: &QString);
        #[qinvokable]
        #[cxx_name = "sendKey"]
        fn send_key(self: Pin<&mut Self>, key: &QString);
        #[qinvokable]
        fn paste(self: Pin<&mut Self>, text: &QString);
        #[qinvokable]
        fn resize(self: Pin<&mut Self>, rows: i32, columns: i32);
        #[qinvokable]
        #[cxx_name = "scrollLines"]
        fn scroll_lines(self: Pin<&mut Self>, lines: i32);
        #[qinvokable]
        fn retry(self: Pin<&mut Self>);
        #[qinvokable]
        #[cxx_name = "invalidateContainer"]
        fn invalidate_container(self: Pin<&mut Self>, container_id: &QString);
        #[qinvokable]
        fn shutdown(self: Pin<&mut Self>);
    }
}

impl qobject::ContainerTerminalModel {
    pub(crate) fn row_count(&self, _parent: &QModelIndex) -> i32 {
        saturating_i32(self.rows.len())
    }

    pub(crate) fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        if role != ROLE_TEXT || index.row() < 0 {
            return QVariant::default();
        }
        self.rows
            .get(index.row() as usize)
            .map(|row| QVariant::from(&QString::from(row)))
            .unwrap_or_default()
    }

    pub(crate) fn role_names(&self) -> qobject::QHash_i32_QByteArray {
        let mut roles = qobject::QHash_i32_QByteArray::default();
        roles.insert(ROLE_TEXT, "text".into());
        roles
    }

    pub(crate) fn set_selection(mut self: Pin<&mut Self>, container_id: &QString, running: bool) {
        let container_id = container_id.to_string();
        let selection_changed = self.state.container_id != container_id;
        let action = self
            .as_mut()
            .rust_mut()
            .state
            .select_container(&container_id, running);
        if selection_changed && !matches!(action, TerminalSelectionAction::Connect { .. }) {
            self.as_mut().close_resources();
            let generation = self.state.generation;
            let rows = self.state.rows;
            let columns = self.state.columns;
            self.as_mut()
                .rust_mut()
                .renderer
                .reset(generation, rows, columns);
        }
        self.as_mut().apply_action(action);
    }

    pub(crate) fn clear_selection(mut self: Pin<&mut Self>) {
        let action = self.as_mut().rust_mut().state.clear_selection();
        self.as_mut().close_resources();
        let generation = self.state.generation;
        let rows = self.state.rows;
        let columns = self.state.columns;
        self.as_mut()
            .rust_mut()
            .renderer
            .reset(generation, rows, columns);
        debug_assert!(matches!(
            action,
            TerminalSelectionAction::None | TerminalSelectionAction::Close { .. }
        ));
        self.as_mut().publish_all();
    }

    pub(crate) fn update_active(mut self: Pin<&mut Self>, active: bool) {
        let action = self.as_mut().rust_mut().state.set_active(active);
        self.as_mut().apply_action(action);
    }

    pub(crate) fn send_text(mut self: Pin<&mut Self>, text: &QString) {
        self.as_mut().queue_input(text.to_string().into_bytes());
    }

    pub(crate) fn send_key(mut self: Pin<&mut Self>, key: &QString) {
        let application_cursor = self.renderer.application_cursor();
        if let Some(bytes) = terminal_key_bytes(&key.to_string(), application_cursor) {
            self.as_mut().queue_input(bytes);
        }
    }

    pub(crate) fn paste(mut self: Pin<&mut Self>, text: &QString) {
        let text = text.to_string();
        let bytes = terminal_paste_bytes(&text, self.renderer.bracketed_paste());
        self.as_mut().queue_input(bytes);
    }

    pub(crate) fn resize(mut self: Pin<&mut Self>, rows: i32, columns: i32) {
        let Ok(rows) = u16::try_from(rows) else {
            return;
        };
        let Ok(columns) = u16::try_from(columns) else {
            return;
        };
        if rows == 0 || columns == 0 {
            return;
        }

        let generation = self.state.generation;
        let parser_changed = self
            .as_mut()
            .rust_mut()
            .renderer
            .resize(generation, rows, columns);
        let pending = self
            .as_mut()
            .rust_mut()
            .state
            .request_resize(rows, columns)
            .and_then(|resize_generation| {
                self.as_mut()
                    .rust_mut()
                    .state
                    .take_pending_resize(resize_generation)
            });
        if parser_changed {
            self.as_mut().publish_screen();
        }
        let Some(pending) = pending else {
            return;
        };
        let Some(session) = self.session.clone() else {
            return;
        };
        cancel(&mut self.as_mut().rust_mut().resize_cancel);
        let token = CancellationToken::new();
        self.as_mut().rust_mut().resize_cancel = Some(token.clone());
        let qt = self.qt_thread();
        crate::runtime::spawn(async move {
            let result = tokio::select! {
                _ = token.cancelled() => return,
                result = session.resize(pending.rows, pending.columns) => result,
            };
            if let Err(error) = result {
                qt.queue(move |mut model| {
                    if generation == model.state.generation
                        && model.state.state == ContainerTerminalState::Ready
                    {
                        model
                            .as_mut()
                            .rust_mut()
                            .state
                            .apply_stream_error(generation, error);
                        model.as_mut().close_resources();
                        model.as_mut().publish_all();
                    }
                })
                .ok();
            }
        });
    }

    pub(crate) fn scroll_lines(mut self: Pin<&mut Self>, lines: i32) {
        let generation = self.state.generation;
        if self
            .as_mut()
            .rust_mut()
            .renderer
            .scroll_lines(generation, lines)
        {
            self.as_mut().publish_screen();
        }
    }

    pub(crate) fn retry(mut self: Pin<&mut Self>) {
        self.as_mut().close_resources();
        let action = self.as_mut().rust_mut().state.begin_connect();
        self.as_mut().apply_action(action);
    }

    pub(crate) fn invalidate_container(mut self: Pin<&mut Self>, container_id: &QString) {
        let action = self
            .as_mut()
            .rust_mut()
            .state
            .container_event_invalidated(&container_id.to_string());
        if matches!(action, TerminalSelectionAction::Close { .. }) {
            self.as_mut().close_resources();
        }
        self.as_mut().publish_all();
    }

    pub(crate) fn shutdown(mut self: Pin<&mut Self>) {
        let action = self.as_mut().rust_mut().state.close();
        debug_assert!(matches!(
            action,
            TerminalSelectionAction::None | TerminalSelectionAction::Close { .. }
        ));
        self.as_mut().close_resources();
        self.as_mut().publish_all();
    }

    fn apply_action(mut self: Pin<&mut Self>, action: TerminalSelectionAction) {
        match action {
            TerminalSelectionAction::Connect { generation } => {
                self.as_mut().start_connect(generation)
            }
            TerminalSelectionAction::Close { .. } => {
                self.as_mut().close_resources();
                self.as_mut().publish_all();
            }
            TerminalSelectionAction::None => self.as_mut().publish_all(),
        }
    }

    fn start_connect(mut self: Pin<&mut Self>, generation: u64) {
        self.as_mut().close_resources();
        let rows = self.state.rows;
        let columns = self.state.columns;
        self.as_mut()
            .rust_mut()
            .renderer
            .reset(generation, rows, columns);
        self.as_mut().publish_all();

        let Some(services) = get_services() else {
            self.as_mut()
                .rust_mut()
                .state
                .apply_connect_error(generation, ContainerTerminalError::DockerUnavailable);
            self.as_mut().publish_all();
            return;
        };
        let cancellation = CancellationToken::new();
        self.as_mut().rust_mut().connection_cancel = Some(cancellation.clone());
        let container_id = self.state.container_id.clone();
        let qt = self.qt_thread();
        crate::runtime::spawn(async move {
            let result = services
                .container_terminal
                .connect(
                    &container_id,
                    ContainerTerminalOptions::default(),
                    cancellation.clone(),
                )
                .await;
            let result = match result {
                Ok(session) => {
                    let session = Arc::new(session);
                    if let Err(error) = session.resize(rows, columns).await {
                        session.close().await;
                        Err(error)
                    } else {
                        match session.take_output().await {
                            Ok(output) => Ok((session, output)),
                            Err(error) => {
                                session.close().await;
                                Err(error)
                            }
                        }
                    }
                }
                Err(error) => Err(error),
            };
            if cancellation.is_cancelled() {
                if let Ok((session, _)) = result {
                    session.close().await;
                }
                return;
            }
            qt.queue(move |mut model| {
                if generation != model.state.generation
                    || model.state.state != ContainerTerminalState::Connecting
                {
                    if let Ok((session, _)) = result {
                        crate::runtime::spawn(async move { session.close().await });
                    }
                    return;
                }
                model.as_mut().rust_mut().connection_cancel = None;
                match result {
                    Ok((session, output)) => {
                        let shell = session.shell().to_owned();
                        if model
                            .as_mut()
                            .rust_mut()
                            .state
                            .apply_connected(generation, &shell)
                        {
                            model.as_mut().start_io(generation, session, output);
                        }
                    }
                    Err(error) => {
                        model
                            .as_mut()
                            .rust_mut()
                            .state
                            .apply_connect_error(generation, error);
                    }
                }
                model.as_mut().publish_all();
            })
            .ok();
        });
    }

    fn start_io(
        mut self: Pin<&mut Self>,
        generation: u64,
        session: Arc<ContainerTerminalSession>,
        output: ContainerTerminalOutputStream,
    ) {
        let cancellation = CancellationToken::new();
        self.as_mut().rust_mut().output_cancel = Some(cancellation.clone());
        self.as_mut().rust_mut().session = Some(session.clone());

        let (input_tx, mut input_rx) = mpsc::channel::<Vec<u8>>(INPUT_QUEUE_CAPACITY);
        self.as_mut().rust_mut().input_tx = Some(input_tx);
        let input_session = session.clone();
        let input_cancel = cancellation.clone();
        let input_qt = self.qt_thread();
        crate::runtime::spawn(async move {
            loop {
                let bytes = tokio::select! {
                    _ = input_cancel.cancelled() => break,
                    bytes = input_rx.recv() => match bytes {
                        Some(bytes) => bytes,
                        None => break,
                    },
                };
                if let Err(error) = input_session.write_input(bytes).await {
                    if !input_cancel.is_cancelled() {
                        let exited = input_session.state() == ContainerTerminalState::Exited;
                        input_qt
                            .queue(move |mut model| {
                                if generation != model.state.generation {
                                    return;
                                }
                                if exited {
                                    model
                                        .as_mut()
                                        .rust_mut()
                                        .state
                                        .apply_exited(generation, None);
                                } else {
                                    model
                                        .as_mut()
                                        .rust_mut()
                                        .state
                                        .apply_stream_error(generation, error);
                                }
                                model.as_mut().close_resources();
                                model.as_mut().publish_all();
                            })
                            .ok();
                    }
                    break;
                }
            }
        });

        let output_qt = self.qt_thread();
        crate::runtime::spawn(run_output(
            output_qt,
            generation,
            session,
            output,
            cancellation,
        ));
    }

    fn queue_input(mut self: Pin<&mut Self>, bytes: Vec<u8>) {
        if bytes.is_empty() || self.as_mut().rust_mut().state.accept_input().is_none() {
            return;
        }
        let generation = self.state.generation;
        let Some(sender) = self.input_tx.clone() else {
            return;
        };
        if sender.try_send(bytes).is_err() {
            self.as_mut()
                .rust_mut()
                .state
                .apply_stream_error(generation, ContainerTerminalError::Disconnected);
            self.as_mut().close_resources();
            self.as_mut().publish_all();
            return;
        }
        if self.as_mut().rust_mut().renderer.snap_to_bottom(generation) {
            self.as_mut().publish_screen();
        }
    }

    fn close_resources(mut self: Pin<&mut Self>) {
        cancel(&mut self.as_mut().rust_mut().connection_cancel);
        cancel(&mut self.as_mut().rust_mut().output_cancel);
        cancel(&mut self.as_mut().rust_mut().resize_cancel);
        self.as_mut().rust_mut().input_tx.take();
        if let Some(session) = self.as_mut().rust_mut().session.take() {
            crate::runtime::spawn(async move { session.close().await });
        }
    }

    fn publish_all(mut self: Pin<&mut Self>) {
        let state = self.state.clone();
        self.as_mut()
            .set_terminal_state(QString::from(state.state_name()));
        self.as_mut()
            .set_error_kind(QString::from(&state.error_kind));
        self.as_mut()
            .set_error_message(QString::from(&state.error_message));
        self.as_mut()
            .set_container_id(QString::from(&state.container_id));
        self.as_mut().set_shell(QString::from(&state.shell));
        self.as_mut().set_running(state.selected_running);
        if self.active != state.active {
            self.as_mut().rust_mut().active = state.active;
            self.as_mut().active_changed();
        }
        self.as_mut().publish_screen();
    }

    fn publish_screen(mut self: Pin<&mut Self>) {
        let snapshot = self.renderer.snapshot();
        let count = saturating_i32(snapshot.rows.len());
        self.as_mut().begin_reset_model();
        self.as_mut().rust_mut().rows = snapshot.rows;
        self.as_mut().end_reset_model();
        self.as_mut().set_count(count);
        self.as_mut()
            .set_column_count(i32::from(snapshot.column_count));
        self.as_mut().set_cursor_row(i32::from(snapshot.cursor_row));
        self.as_mut()
            .set_cursor_column(i32::from(snapshot.cursor_column));
        self.as_mut().set_cursor_visible(snapshot.cursor_visible);
        self.as_mut()
            .set_alternate_screen(snapshot.alternate_screen);
        self.as_mut()
            .set_scrollback_offset(saturating_i32(snapshot.scrollback));
    }
}

async fn run_output(
    qt: cxx_qt::CxxQtThread<qobject::ContainerTerminalModel>,
    generation: u64,
    session: Arc<ContainerTerminalSession>,
    mut output: ContainerTerminalOutputStream,
    cancellation: CancellationToken,
) {
    let mut pending = Vec::with_capacity(OUTPUT_PUBLISH_BYTES);
    let mut error = None;
    loop {
        tokio::select! {
            _ = cancellation.cancelled() => return,
            item = output.next() => match item {
                Some(Ok(chunk)) => {
                    pending.extend_from_slice(&into_output_bytes(chunk));
                    if pending.len() >= OUTPUT_PUBLISH_BYTES {
                        flush_output(&qt, generation, &mut pending);
                    }
                }
                Some(Err(stream_error)) => {
                    error = Some(stream_error);
                    break;
                }
                None => break,
            },
            _ = tokio::time::sleep(OUTPUT_PUBLISH_INTERVAL), if !pending.is_empty() => {
                flush_output(&qt, generation, &mut pending);
            }
        }
    }
    flush_output(&qt, generation, &mut pending);
    if cancellation.is_cancelled() {
        return;
    }

    let outcome = if let Some(error) = error {
        TerminalEnd::Error(error)
    } else {
        match session.inspect().await {
            Ok(status) if !status.running => TerminalEnd::Exited(status.exit_code),
            Ok(_) => TerminalEnd::Error(ContainerTerminalError::Disconnected),
            Err(_) if session.state() == ContainerTerminalState::Exited => {
                TerminalEnd::Exited(None)
            }
            Err(error) => TerminalEnd::Error(error),
        }
    };
    qt.queue(move |mut model| {
        if generation != model.state.generation {
            return;
        }
        match outcome {
            TerminalEnd::Exited(exit_code) => {
                model
                    .as_mut()
                    .rust_mut()
                    .state
                    .apply_exited(generation, exit_code);
            }
            TerminalEnd::Error(error) => {
                model
                    .as_mut()
                    .rust_mut()
                    .state
                    .apply_stream_error(generation, error);
            }
        }
        model.as_mut().close_resources();
        model.as_mut().publish_all();
    })
    .ok();
}

fn flush_output(
    qt: &cxx_qt::CxxQtThread<qobject::ContainerTerminalModel>,
    generation: u64,
    pending: &mut Vec<u8>,
) {
    if pending.is_empty() {
        return;
    }
    let bytes = std::mem::take(pending);
    qt.queue(move |mut model| {
        if model
            .as_mut()
            .rust_mut()
            .renderer
            .process(generation, &bytes)
        {
            model.as_mut().publish_screen();
        }
    })
    .ok();
}

enum TerminalEnd {
    Exited(Option<i64>),
    Error(ContainerTerminalError),
}

fn into_output_bytes(output: ContainerTerminalOutput) -> Vec<u8> {
    match output {
        ContainerTerminalOutput::StdOut(bytes)
        | ContainerTerminalOutput::StdErr(bytes)
        | ContainerTerminalOutput::StdIn(bytes)
        | ContainerTerminalOutput::Console(bytes) => bytes,
    }
}

fn cancel(slot: &mut Option<CancellationToken>) {
    if let Some(token) = slot.take() {
        token.cancel();
    }
}

fn saturating_i32(value: usize) -> i32 {
    value.min(i32::MAX as usize) as i32
}
