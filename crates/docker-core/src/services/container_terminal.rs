//! Interactive Docker exec terminal backend.
//!
//! This module deliberately keeps Bollard's upgraded connection behind bounded
//! channels. A GUI task can consume terminal output while other tasks write to
//! stdin, resize the TTY, or close the session. No output is collected in full.

use std::collections::HashSet;
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use bollard::container::LogOutput;
use bollard::errors::Error as BollardError;
use bollard::exec::{CreateExecOptions, ResizeExecOptions, StartExecOptions, StartExecResults};
use futures_util::StreamExt;
use tokio::io::AsyncWriteExt;
use tokio::sync::{Mutex, mpsc, watch};
use tokio::task::JoinHandle;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::CancellationToken;

use crate::client::DockerClient;

const DEFAULT_SHELLS: [&str; 11] = [
    "/bin/bash",
    "/usr/bin/bash",
    "/bin/zsh",
    "/usr/bin/zsh",
    "/bin/fish",
    "/usr/bin/fish",
    "/bin/ash",
    "/usr/bin/ash",
    "/bin/sh",
    "/usr/bin/sh",
    "/busybox/sh",
];
const OUTPUT_CHANNEL_CAPACITY: usize = 128;
const INPUT_CHANNEL_CAPACITY: usize = 64;
const EXEC_START_PROBE_DELAY: Duration = Duration::from_millis(100);

/// Lifecycle state of an interactive exec session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ContainerTerminalState {
    #[default]
    Idle,
    Connecting,
    Ready,
    Exited,
    Error,
}

/// Stable, secret-free terminal errors suitable for presentation by the GUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ContainerTerminalError {
    #[error("the container is not running")]
    NotRunning,
    #[error("the container is paused")]
    Paused,
    #[error("no supported shell was found in the container")]
    ShellNotFound,
    #[error("Docker could not create the exec instance")]
    CreateFailed,
    #[error("Docker could not start the exec instance")]
    StartFailed,
    #[error("the terminal connection was lost")]
    Disconnected,
    #[error("Docker could not resize the terminal")]
    ResizeFailed,
    #[error("the terminal operation timed out")]
    Timeout,
    #[error("the terminal operation was cancelled")]
    Cancelled,
    #[error("terminal options are invalid")]
    InvalidOptions,
    #[error("permission to use the Docker terminal was denied")]
    Permission,
    #[error("Docker Engine is unavailable")]
    DockerUnavailable,
}

/// Origin and bytes of one streamed terminal output chunk.
///
/// Docker normally emits [`Console`](Self::Console) for a TTY. The other
/// variants are retained so consumers never have to discard stream identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContainerTerminalOutput {
    StdOut(Vec<u8>),
    StdErr(Vec<u8>),
    StdIn(Vec<u8>),
    Console(Vec<u8>),
}

/// A non-buffering, bounded stream of terminal chunks.
pub type ContainerTerminalOutputStream =
    ReceiverStream<Result<ContainerTerminalOutput, ContainerTerminalError>>;

/// Current status returned by Docker's exec inspection endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContainerTerminalExecStatus {
    pub running: bool,
    pub exit_code: Option<i64>,
}

/// Caller-controlled terminal settings.
#[derive(Debug, Clone, Default)]
pub struct ContainerTerminalOptions {
    /// Preferred shell path. It is used only when it is an absolute, safe path.
    pub shell: Option<String>,
    /// Exec working directory. When absent, the container configuration is used,
    /// then `/` as a final default.
    pub working_dir: Option<String>,
    /// Exec user. When absent, the container configuration is used. An empty
    /// value leaves Docker's container-default user unchanged.
    pub user: Option<String>,
    /// Initial TTY height and width. Zero values are rejected.
    pub rows: u16,
    pub cols: u16,
    /// Optional override for this session's Docker operation timeout.
    pub operation_timeout: Option<Duration>,
}

/// Real Docker terminal service shared by GUI/session controllers.
#[derive(Clone)]
pub struct ContainerTerminalService {
    client: Arc<DockerClient>,
}

impl ContainerTerminalService {
    pub fn new(client: Arc<DockerClient>) -> Self {
        Self { client }
    }

    /// Create, start, and probe an attached TTY exec session.
    ///
    /// Shell selection is per call and is not persisted globally. The supplied
    /// cancellation token may therefore be tied directly to a GUI selection or
    /// tab generation.
    pub async fn connect(
        &self,
        container_id: &str,
        options: ContainerTerminalOptions,
        cancellation: CancellationToken,
    ) -> Result<ContainerTerminalSession, ContainerTerminalError> {
        if cancellation.is_cancelled() {
            return Err(ContainerTerminalError::Cancelled);
        }
        if container_id.trim().is_empty()
            || container_id.as_bytes().contains(&0)
            || options.rows == 0
            || options.cols == 0
            || options
                .working_dir
                .as_deref()
                .is_some_and(|path| !is_safe_working_directory(path))
            || options
                .user
                .as_deref()
                .is_some_and(|user| user.as_bytes().contains(&0))
        {
            return Err(ContainerTerminalError::InvalidOptions);
        }

        let timeout = options
            .operation_timeout
            .filter(|duration| !duration.is_zero())
            .unwrap_or(self.client.config().request_timeout);
        let docker = self.client.inner().clone().with_timeout(timeout);

        let inspect = operation(
            timeout,
            &cancellation,
            docker.inspect_container(container_id, None),
        )
        .await
        .map_err(|error| classify_bollard_error(&error, ContainerTerminalError::NotRunning))?;
        let state = inspect.state.as_ref();
        if state.and_then(|state| state.paused).unwrap_or(false) {
            return Err(ContainerTerminalError::Paused);
        }
        if !state.and_then(|state| state.running).unwrap_or(false) {
            return Err(ContainerTerminalError::NotRunning);
        }

        let configured_working_dir = inspect
            .config
            .as_ref()
            .and_then(|config| non_empty(config.working_dir.as_deref()));
        let configured_user = inspect
            .config
            .as_ref()
            .and_then(|config| non_empty(config.user.as_deref()));
        let working_dir = non_empty(options.working_dir.as_deref())
            .or(configured_working_dir)
            .unwrap_or("/")
            .to_owned();
        let user = non_empty(options.user.as_deref())
            .or(configured_user)
            .map(str::to_owned);

        let (state_tx, _) = watch::channel(ContainerTerminalState::Idle);
        transition_state(&state_tx, ContainerTerminalState::Connecting);

        let configured_shell = inspect
            .config
            .as_ref()
            .and_then(|config| config.labels.as_ref())
            .and_then(|labels| labels.get("io.tuxstack.shell").cloned());
        let environment_shell = inspect.config.as_ref().and_then(|config| {
            config.env.as_ref().and_then(|environment| {
                environment
                    .iter()
                    .find_map(|value| value.strip_prefix("SHELL=").map(str::to_owned))
            })
        });
        for shell in shell_candidates(
            options.shell.as_deref(),
            configured_shell.as_deref(),
            environment_shell.as_deref(),
        ) {
            if cancellation.is_cancelled() {
                transition_state(&state_tx, ContainerTerminalState::Error);
                return Err(ContainerTerminalError::Cancelled);
            }

            let create_options = CreateExecOptions::<String> {
                attach_stdin: Some(true),
                attach_stdout: Some(true),
                attach_stderr: Some(true),
                tty: Some(true),
                env: Some(vec![
                    "TERM=xterm-256color".to_owned(),
                    "COLORTERM=truecolor".to_owned(),
                ]),
                cmd: Some(vec![shell.clone(), "-i".to_owned()]),
                user: user.clone(),
                working_dir: Some(working_dir.clone()),
                ..Default::default()
            };

            let created = match operation(
                timeout,
                &cancellation,
                docker.create_exec(container_id, create_options),
            )
            .await
            {
                Ok(created) => created,
                Err(error) => {
                    let classified =
                        classify_bollard_error(&error, ContainerTerminalError::CreateFailed);
                    if is_fatal_candidate_error(classified) {
                        transition_state(&state_tx, ContainerTerminalState::Error);
                        return Err(classified);
                    }
                    continue;
                }
            };

            let started = match operation(
                timeout,
                &cancellation,
                docker.start_exec(
                    &created.id,
                    Some(StartExecOptions {
                        detach: false,
                        tty: true,
                        output_capacity: None,
                    }),
                ),
            )
            .await
            {
                Ok(StartExecResults::Attached { output, input }) => (output, input),
                Ok(StartExecResults::Detached) => continue,
                Err(error) => {
                    let classified =
                        classify_bollard_error(&error, ContainerTerminalError::StartFailed);
                    if is_fatal_candidate_error(classified) {
                        transition_state(&state_tx, ContainerTerminalState::Error);
                        return Err(classified);
                    }
                    continue;
                }
            };

            // Starting an absent or unusable executable can race with the HTTP
            // upgrade. Give Docker a short, bounded opportunity to publish its
            // terminal exit status before accepting this shell.
            if let Err(error) = cancellable_sleep(EXEC_START_PROBE_DELAY, &cancellation).await {
                transition_state(&state_tx, ContainerTerminalState::Error);
                return Err(error);
            }
            let probe =
                match operation(timeout, &cancellation, docker.inspect_exec(&created.id)).await {
                    Ok(probe) => probe,
                    Err(error) => {
                        let classified =
                            classify_bollard_error(&error, ContainerTerminalError::StartFailed);
                        if is_fatal_candidate_error(classified) {
                            transition_state(&state_tx, ContainerTerminalState::Error);
                            return Err(classified);
                        }
                        continue;
                    }
                };
            if !probe.running.unwrap_or(false) {
                // Any immediate exit means this candidate cannot back an
                // interactive session. This includes exit codes 126/127 and a
                // configured shell that exits successfully without interacting.
                drop(started);
                continue;
            }
            if let Err(error) = operation(
                timeout,
                &cancellation,
                docker.resize_exec(
                    &created.id,
                    ResizeExecOptions {
                        height: options.rows,
                        width: options.cols,
                    },
                ),
            )
            .await
            {
                let error = classify_bollard_error(&error, ContainerTerminalError::ResizeFailed);
                transition_state(&state_tx, ContainerTerminalState::Error);
                return Err(error);
            }

            let session_cancel = cancellation.child_token();
            let (output_tx, output_rx) = mpsc::channel(OUTPUT_CHANNEL_CAPACITY);
            let (input_tx, input_rx) = mpsc::channel(INPUT_CHANNEL_CAPACITY);
            let closed = Arc::new(AtomicBool::new(false));
            let tasks = spawn_session_tasks(
                docker.clone(),
                created.id.clone(),
                started.0,
                started.1,
                input_rx,
                output_tx,
                state_tx.clone(),
                session_cancel.clone(),
                timeout,
            );
            transition_state(&state_tx, ContainerTerminalState::Ready);

            return Ok(ContainerTerminalSession {
                exec_id: created.id,
                container_id: container_id.to_owned(),
                shell,
                client: self.client.clone(),
                timeout,
                input_tx,
                output_rx: Mutex::new(Some(output_rx)),
                state_tx,
                cancellation: session_cancel,
                closed,
                tasks: Mutex::new(tasks),
            });
        }

        transition_state(&state_tx, ContainerTerminalState::Error);
        Err(ContainerTerminalError::ShellNotFound)
    }
}

/// A concurrently manageable attached Docker exec session.
///
/// Wrap this value in an [`Arc`] when the GUI output task and input/event task
/// need independent ownership. The output receiver may be taken exactly once.
pub struct ContainerTerminalSession {
    exec_id: String,
    container_id: String,
    shell: String,
    client: Arc<DockerClient>,
    timeout: Duration,
    input_tx: mpsc::Sender<Vec<u8>>,
    output_rx:
        Mutex<Option<mpsc::Receiver<Result<ContainerTerminalOutput, ContainerTerminalError>>>>,
    state_tx: watch::Sender<ContainerTerminalState>,
    cancellation: CancellationToken,
    closed: Arc<AtomicBool>,
    tasks: Mutex<Vec<JoinHandle<()>>>,
}

impl ContainerTerminalSession {
    pub fn exec_id(&self) -> &str {
        &self.exec_id
    }

    pub fn container_id(&self) -> &str {
        &self.container_id
    }

    pub fn shell(&self) -> &str {
        &self.shell
    }

    pub fn state(&self) -> ContainerTerminalState {
        *self.state_tx.borrow()
    }

    pub fn subscribe_state(&self) -> watch::Receiver<ContainerTerminalState> {
        self.state_tx.subscribe()
    }

    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancellation.clone()
    }

    /// Take the bounded output stream. A second call returns `Disconnected`.
    pub async fn take_output(
        &self,
    ) -> Result<ContainerTerminalOutputStream, ContainerTerminalError> {
        self.output_rx
            .lock()
            .await
            .take()
            .map(ReceiverStream::new)
            .ok_or(ContainerTerminalError::Disconnected)
    }

    /// Queue raw terminal input without blocking the Docker upgraded writer.
    pub async fn write_input(&self, bytes: Vec<u8>) -> Result<(), ContainerTerminalError> {
        if self.closed.load(Ordering::Acquire) || self.cancellation.is_cancelled() {
            return Err(ContainerTerminalError::Cancelled);
        }
        if self.state() != ContainerTerminalState::Ready {
            return Err(ContainerTerminalError::Disconnected);
        }
        tokio::select! {
            biased;
            _ = self.cancellation.cancelled() => Err(ContainerTerminalError::Cancelled),
            result = tokio::time::timeout(self.timeout, self.input_tx.send(bytes)) => {
                result
                    .map_err(|_| ContainerTerminalError::Timeout)?
                    .map_err(|_| ContainerTerminalError::Disconnected)
            }
        }
    }

    /// Resize the attached TTY. Debouncing intentionally belongs to the GUI.
    pub async fn resize(&self, rows: u16, cols: u16) -> Result<(), ContainerTerminalError> {
        if self.closed.load(Ordering::Acquire) || self.cancellation.is_cancelled() {
            return Err(ContainerTerminalError::Cancelled);
        }
        let docker = self.client.inner().clone().with_timeout(self.timeout);
        operation(
            self.timeout,
            &self.cancellation,
            docker.resize_exec(
                &self.exec_id,
                ResizeExecOptions {
                    height: rows,
                    width: cols,
                },
            ),
        )
        .await
        .map_err(|error| classify_bollard_error(&error, ContainerTerminalError::ResizeFailed))
    }

    /// Inspect the real exec process status without consuming its output.
    pub async fn inspect(&self) -> Result<ContainerTerminalExecStatus, ContainerTerminalError> {
        let docker = self.client.inner().clone().with_timeout(self.timeout);
        let inspect = operation(
            self.timeout,
            &self.cancellation,
            docker.inspect_exec(&self.exec_id),
        )
        .await
        .map_err(|error| classify_bollard_error(&error, ContainerTerminalError::Disconnected))?;
        Ok(ContainerTerminalExecStatus {
            running: inspect.running.unwrap_or(false),
            exit_code: inspect.exit_code,
        })
    }

    /// Close stdin/the upgraded connection, cancel pumps, and await their exit.
    pub async fn close(&self) {
        if self.closed.swap(true, Ordering::AcqRel) {
            return;
        }
        self.cancellation.cancel();
        transition_state(&self.state_tx, ContainerTerminalState::Exited);

        let tasks = std::mem::take(&mut *self.tasks.lock().await);
        for mut task in tasks {
            if tokio::time::timeout(self.timeout, &mut task).await.is_err() {
                task.abort();
            }
        }
    }
}

impl Drop for ContainerTerminalSession {
    fn drop(&mut self) {
        self.closed.store(true, Ordering::Release);
        self.cancellation.cancel();
        transition_state(&self.state_tx, ContainerTerminalState::Exited);
        if let Ok(mut tasks) = self.tasks.try_lock() {
            for task in tasks.drain(..) {
                task.abort();
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_session_tasks(
    docker: bollard::Docker,
    exec_id: String,
    mut output: std::pin::Pin<
        Box<dyn futures_util::Stream<Item = Result<LogOutput, BollardError>> + Send>,
    >,
    mut input: std::pin::Pin<Box<dyn tokio::io::AsyncWrite + Send>>,
    mut input_rx: mpsc::Receiver<Vec<u8>>,
    output_tx: mpsc::Sender<Result<ContainerTerminalOutput, ContainerTerminalError>>,
    state_tx: watch::Sender<ContainerTerminalState>,
    cancellation: CancellationToken,
    timeout: Duration,
) -> Vec<JoinHandle<()>> {
    let output_cancel = cancellation.clone();
    let output_errors = output_tx.clone();
    let input_errors = output_tx.clone();
    let output_state = state_tx.clone();
    let output_task = tokio::spawn(async move {
        loop {
            let item = tokio::select! {
                biased;
                _ = output_cancel.cancelled() => {
                    transition_state(&output_state, ContainerTerminalState::Exited);
                    break;
                },
                item = output.next() => item,
            };
            match item {
                Some(Ok(chunk)) => {
                    let chunk = map_output(chunk);
                    let sent = tokio::select! {
                        biased;
                        _ = output_cancel.cancelled() => false,
                        result = output_tx.send(Ok(chunk)) => result.is_ok(),
                    };
                    if !sent {
                        transition_state(&output_state, ContainerTerminalState::Exited);
                        output_cancel.cancel();
                        break;
                    }
                }
                Some(Err(_)) => {
                    transition_state(&output_state, ContainerTerminalState::Error);
                    let _ = output_errors.try_send(Err(ContainerTerminalError::Disconnected));
                    output_cancel.cancel();
                    break;
                }
                None => {
                    let status = tokio::time::timeout(timeout, docker.inspect_exec(&exec_id)).await;
                    match status {
                        Ok(Ok(status)) if !status.running.unwrap_or(false) => {
                            transition_state(&output_state, ContainerTerminalState::Exited);
                        }
                        Ok(Ok(_)) => {
                            transition_state(&output_state, ContainerTerminalState::Error);
                            let _ =
                                output_errors.try_send(Err(ContainerTerminalError::Disconnected));
                        }
                        Ok(Err(error)) => {
                            transition_state(&output_state, ContainerTerminalState::Error);
                            let error = classify_bollard_error(
                                &error,
                                ContainerTerminalError::Disconnected,
                            );
                            let _ = output_errors.try_send(Err(error));
                        }
                        Err(_) => {
                            transition_state(&output_state, ContainerTerminalState::Error);
                            let _ = output_errors.try_send(Err(ContainerTerminalError::Timeout));
                        }
                    }
                    output_cancel.cancel();
                    break;
                }
            }
        }
    });

    let input_cancel = cancellation.clone();
    let input_state = state_tx;
    let input_task = tokio::spawn(async move {
        loop {
            let next = tokio::select! {
                biased;
                _ = input_cancel.cancelled() => None,
                bytes = input_rx.recv() => bytes,
            };
            let Some(bytes) = next else {
                let _ = tokio::time::timeout(timeout, input.shutdown()).await;
                break;
            };
            if bytes.is_empty() {
                continue;
            }
            let write = async {
                input.write_all(&bytes).await?;
                input.flush().await
            };
            let result = tokio::select! {
                biased;
                _ = input_cancel.cancelled() => break,
                result = tokio::time::timeout(timeout, write) => result,
            };
            if !matches!(result, Ok(Ok(()))) {
                transition_state(&input_state, ContainerTerminalState::Error);
                let error = if result.is_err() {
                    ContainerTerminalError::Timeout
                } else {
                    ContainerTerminalError::Disconnected
                };
                let _ = input_errors.try_send(Err(error));
                input_cancel.cancel();
                break;
            }
        }
    });

    vec![output_task, input_task]
}

fn map_output(output: LogOutput) -> ContainerTerminalOutput {
    match output {
        LogOutput::StdOut { message } => ContainerTerminalOutput::StdOut(message.to_vec()),
        LogOutput::StdErr { message } => ContainerTerminalOutput::StdErr(message.to_vec()),
        LogOutput::StdIn { message } => ContainerTerminalOutput::StdIn(message.to_vec()),
        LogOutput::Console { message } => ContainerTerminalOutput::Console(message.to_vec()),
    }
}

async fn operation<T>(
    timeout: Duration,
    cancellation: &CancellationToken,
    future: impl Future<Output = Result<T, BollardError>>,
) -> Result<T, BollardError> {
    tokio::select! {
        biased;
        _ = cancellation.cancelled() => Err(BollardError::IOError {
            err: std::io::Error::new(std::io::ErrorKind::Interrupted, "cancelled"),
        }),
        result = tokio::time::timeout(timeout, future) => match result {
            Ok(result) => result,
            Err(_) => Err(BollardError::RequestTimeoutError),
        },
    }
}

async fn cancellable_sleep(
    duration: Duration,
    cancellation: &CancellationToken,
) -> Result<(), ContainerTerminalError> {
    tokio::select! {
        biased;
        _ = cancellation.cancelled() => Err(ContainerTerminalError::Cancelled),
        _ = tokio::time::sleep(duration) => Ok(()),
    }
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn shell_candidates(
    explicit: Option<&str>,
    configured: Option<&str>,
    environment: Option<&str>,
) -> Vec<String> {
    let mut seen = HashSet::new();
    let candidates: Box<dyn Iterator<Item = &str>> = match explicit {
        Some(shell) => Box::new(std::iter::once(shell)),
        None => Box::new(
            configured
                .into_iter()
                .chain(environment)
                .chain(DEFAULT_SHELLS.iter().copied()),
        ),
    };
    candidates
        .filter(|shell| is_safe_absolute_shell(shell))
        .filter_map(|shell| {
            let shell = shell.to_owned();
            seen.insert(shell.clone()).then_some(shell)
        })
        .collect()
}

fn is_safe_absolute_shell(shell: &str) -> bool {
    if shell.len() < 2 || shell.len() > 4096 || !shell.starts_with('/') {
        return false;
    }
    shell.split('/').skip(1).all(|component| {
        !component.is_empty()
            && component != "."
            && component != ".."
            && component.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'+')
            })
    })
}

fn is_safe_working_directory(path: &str) -> bool {
    path == "/"
        || (path.starts_with('/')
            && path.len() <= 4096
            && !path.as_bytes().contains(&0)
            && path
                .split('/')
                .skip(1)
                .all(|component| !component.is_empty() && component != "." && component != ".."))
}

fn transition_state(state: &watch::Sender<ContainerTerminalState>, next: ContainerTerminalState) {
    let current = *state.borrow();
    if state_transition_allowed(current, next) {
        state.send_replace(next);
    }
}

fn state_transition_allowed(current: ContainerTerminalState, next: ContainerTerminalState) -> bool {
    use ContainerTerminalState::{Connecting, Error, Exited, Idle, Ready};
    current == next
        || matches!(
            (current, next),
            (Idle, Connecting)
                | (Connecting, Ready | Error | Exited)
                | (Ready, Exited | Error)
                | (Error, Exited)
        )
}

fn is_fatal_candidate_error(error: ContainerTerminalError) -> bool {
    matches!(
        error,
        ContainerTerminalError::NotRunning
            | ContainerTerminalError::Paused
            | ContainerTerminalError::Timeout
            | ContainerTerminalError::Cancelled
            | ContainerTerminalError::InvalidOptions
            | ContainerTerminalError::Permission
            | ContainerTerminalError::DockerUnavailable
    )
}

fn classify_bollard_error(
    error: &BollardError,
    fallback: ContainerTerminalError,
) -> ContainerTerminalError {
    match error {
        BollardError::DockerResponseServerError {
            status_code: 401 | 403,
            ..
        } => ContainerTerminalError::Permission,
        BollardError::DockerResponseServerError { message, .. } => {
            classify_error_text(message, fallback)
        }
        BollardError::RequestTimeoutError => ContainerTerminalError::Timeout,
        BollardError::SocketNotFoundError(_) => ContainerTerminalError::DockerUnavailable,
        BollardError::IOError { err } if err.kind() == std::io::ErrorKind::Interrupted => {
            ContainerTerminalError::Cancelled
        }
        _ => classify_error_text(&error.to_string(), fallback),
    }
}

fn classify_error_text(message: &str, fallback: ContainerTerminalError) -> ContainerTerminalError {
    let message = message.to_ascii_lowercase();
    if message.contains("cancelled") || message.contains("canceled") {
        ContainerTerminalError::Cancelled
    } else if message.contains("permission denied")
        || message.contains("access denied")
        || message.contains("authorization denied")
    {
        ContainerTerminalError::Permission
    } else if message.contains("container is paused") || message.contains("paused container") {
        ContainerTerminalError::Paused
    } else if message.contains("container is not running")
        || message.contains("is not running")
        || message.contains("not running container")
    {
        ContainerTerminalError::NotRunning
    } else if message.contains("timed out") || message.contains("timeout") {
        ContainerTerminalError::Timeout
    } else if message.contains("connection refused")
        || message.contains("connection reset")
        || message.contains("broken pipe")
        || message.contains("no route to host")
        || message.contains("socket not found")
        || (fallback == ContainerTerminalError::NotRunning
            && message.contains("no such file or directory"))
    {
        ContainerTerminalError::DockerUnavailable
    } else {
        fallback
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_candidates_validate_and_deduplicate() {
        assert_eq!(
            shell_candidates(Some("/usr/local/bin/fish"), None, None),
            vec!["/usr/local/bin/fish"]
        );
        assert_eq!(
            shell_candidates(Some("/bin/sh"), None, None),
            vec!["/bin/sh"]
        );
        assert_eq!(
            shell_candidates(None, Some("relative"), None),
            DEFAULT_SHELLS
                .iter()
                .map(|shell| (*shell).to_owned())
                .collect::<Vec<_>>()
        );

        for unsafe_shell in [
            "sh",
            "/bin/sh -l",
            "/bin/../bin/sh",
            "/bin//sh",
            "/bin/$SHELL",
            "/",
        ] {
            assert!(shell_candidates(Some(unsafe_shell), None, None).is_empty());
        }
        assert_eq!(
            shell_candidates(None, Some("/custom/shell"), None),
            vec![
                "/custom/shell",
                "/bin/bash",
                "/usr/bin/bash",
                "/bin/zsh",
                "/usr/bin/zsh",
                "/bin/fish",
                "/usr/bin/fish",
                "/bin/ash",
                "/usr/bin/ash",
                "/bin/sh",
                "/usr/bin/sh",
                "/busybox/sh",
            ]
        );
        assert!(is_safe_working_directory("/"));
        assert!(is_safe_working_directory("/workspace/src"));
        assert!(!is_safe_working_directory("relative"));
        assert!(!is_safe_working_directory("/workspace/../secret"));
        assert!(!is_safe_working_directory("/workspace//src"));
    }

    #[test]
    fn terminal_state_transitions_are_explicit() {
        use ContainerTerminalState::{Connecting, Error, Exited, Idle, Ready};
        assert!(state_transition_allowed(Idle, Connecting));
        assert!(state_transition_allowed(Connecting, Ready));
        assert!(state_transition_allowed(Connecting, Error));
        assert!(state_transition_allowed(Ready, Exited));
        assert!(state_transition_allowed(Ready, Error));
        assert!(state_transition_allowed(Error, Exited));
        assert!(!state_transition_allowed(Idle, Ready));
        assert!(!state_transition_allowed(Exited, Ready));
        assert!(!state_transition_allowed(Error, Ready));
    }

    #[tokio::test]
    async fn input_channel_close_and_cancel_stop_the_pump() {
        let (tx, mut rx) = mpsc::channel::<Vec<u8>>(1);
        let cancellation = CancellationToken::new();
        let child = cancellation.clone();
        let pump = tokio::spawn(async move {
            tokio::select! {
                _ = child.cancelled() => None,
                input = rx.recv() => input,
            }
        });
        tx.send(b"input".to_vec()).await.unwrap();
        assert_eq!(pump.await.unwrap(), Some(b"input".to_vec()));

        let (tx, mut rx) = mpsc::channel::<Vec<u8>>(1);
        let child = cancellation.clone();
        let pump = tokio::spawn(async move {
            tokio::select! {
                biased;
                _ = child.cancelled() => None,
                input = rx.recv() => input,
            }
        });
        cancellation.cancel();
        assert_eq!(pump.await.unwrap(), None);
        assert!(tx.send(b"closed".to_vec()).await.is_err());
    }

    #[test]
    fn errors_are_classified_without_exposing_source_text() {
        assert_eq!(
            classify_error_text("container is paused", ContainerTerminalError::StartFailed),
            ContainerTerminalError::Paused
        );
        assert_eq!(
            classify_error_text(
                "permission denied: token=super-secret",
                ContainerTerminalError::CreateFailed
            ),
            ContainerTerminalError::Permission
        );
        assert_eq!(
            classify_error_text("connection refused", ContainerTerminalError::StartFailed),
            ContainerTerminalError::DockerUnavailable
        );
        assert_eq!(
            classify_error_text(
                "unknown daemon detail",
                ContainerTerminalError::CreateFailed
            ),
            ContainerTerminalError::CreateFailed
        );
        assert!(
            !ContainerTerminalError::Permission
                .to_string()
                .contains("super-secret")
        );
    }
}
