//! JSON Lines client for communicating with the `tuxstack-fs-helper`
//! running inside a preview container. Handles the hello handshake,
//! streaming list/stat/preview/hash operations, and session invalidation
//! on timeout or exec failure.

use bollard::exec::{CreateExecOptions, StartExecOptions, StartExecResults};
use futures_util::StreamExt;
use tokio_util::sync::CancellationToken;

use super::error::FilesystemError;
use super::types::*;
use tuxstack_fs_protocol::FilesystemPathToken;

/// Maximum bytes collected per exec call (8 MiB stdout, 32 MiB for preview).
const MAX_EXEC_STDOUT: usize = 8 * 1024 * 1024;
const MAX_EXEC_PREVIEW: usize = 32 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Hello handshake
// ---------------------------------------------------------------------------

/// Execute the `hello` command and verify the protocol version. Returns the
/// helper's reported version string on success.
pub async fn hello(
    client: &bollard::Docker,
    session: &FilesystemSession,
    timeout: std::time::Duration,
    cancellation: &CancellationToken,
) -> Result<String, FilesystemError> {
    let output = exec_lines(
        client,
        session,
        vec![session.helper_path.clone(), "hello".into()],
        timeout,
        MAX_EXEC_STDOUT,
        cancellation,
    )
    .await?;

    for line in &output {
        if let Ok(tuxstack_fs_protocol::HelperMessage::Hello {
            protocol,
            helper_version,
        }) = serde_json::from_str::<tuxstack_fs_protocol::HelperMessage>(line)
        {
            if protocol != tuxstack_fs_protocol::FS_HELPER_PROTOCOL_VERSION {
                return Err(FilesystemError::HelperProtocolMismatch {
                    expected: tuxstack_fs_protocol::FS_HELPER_PROTOCOL_VERSION,
                    got: protocol,
                });
            }
            return Ok(helper_version);
        }
    }
    Err(FilesystemError::HelperHandshakeFailed(
        "no hello message received".into(),
    ))
}

// ---------------------------------------------------------------------------
// List directory
// ---------------------------------------------------------------------------

pub async fn list_directory(
    client: &bollard::Docker,
    session: &FilesystemSession,
    request: &ListDirectoryRequest,
    timeout: std::time::Duration,
    cancellation: &CancellationToken,
) -> Result<ListDirectoryResult, FilesystemError> {
    let mut args: Vec<String> = vec![
        session.helper_path.clone(),
        "list".into(),
        "--root".into(),
        session.root.clone(),
        "--path-token".into(),
        request.path_token.0.clone(),
    ];
    if request.show_hidden {
        args.push("--show-hidden".into());
    }
    if let Some(limit) = &request.limit {
        args.push("--limit".into());
        args.push(limit.to_string());
    }
    if let Some(cursor) = &request.cursor {
        args.push("--cursor".into());
        args.push(cursor.clone());
    }

    let output = exec_lines(
        client,
        session,
        args,
        timeout,
        MAX_EXEC_STDOUT,
        cancellation,
    )
    .await?;

    parse_list_output(&output)
}

fn parse_list_output(lines: &[String]) -> Result<ListDirectoryResult, FilesystemError> {
    let mut entries = Vec::new();
    let mut truncated = false;
    let mut next_cursor = None;

    for line in lines {
        let message: tuxstack_fs_protocol::HelperMessage =
            serde_json::from_str(line).map_err(|error| {
                FilesystemError::HelperProtocolError(format!("JSON parse: {error}"))
            })?;
        match message {
            tuxstack_fs_protocol::HelperMessage::Entry { .. } => {
                if let Some(entry) = FilesystemEntry::from_protocol_entry(&message) {
                    entries.push(entry);
                }
            }
            tuxstack_fs_protocol::HelperMessage::End {
                truncated: t,
                next_cursor: c,
            } => {
                truncated = t;
                next_cursor = c;
            }
            tuxstack_fs_protocol::HelperMessage::Error { code, message } => {
                return Err(map_helper_error(code, &message));
            }
            _ => {}
        }
    }
    Ok(ListDirectoryResult {
        entries,
        truncated,
        next_cursor,
    })
}

// ---------------------------------------------------------------------------
// Stat
// ---------------------------------------------------------------------------

pub async fn stat(
    client: &bollard::Docker,
    session: &FilesystemSession,
    request: &StatRequest,
    timeout: std::time::Duration,
    cancellation: &CancellationToken,
) -> Result<FilesystemEntry, FilesystemError> {
    let args = vec![
        session.helper_path.clone(),
        "stat".into(),
        "--root".into(),
        session.root.clone(),
        "--path-token".into(),
        request.path_token.0.clone(),
    ];
    let output = exec_lines(
        client,
        session,
        args,
        timeout,
        MAX_EXEC_STDOUT,
        cancellation,
    )
    .await?;

    for line in &output {
        if let Ok(message) = serde_json::from_str::<tuxstack_fs_protocol::HelperMessage>(line) {
            match &message {
                tuxstack_fs_protocol::HelperMessage::Stat { .. } => {
                    // Stat messages share the same fields as Entry without name_b64;
                    // reconstruct an Entry-compatible view by synthesizing name_b64.
                    if let tuxstack_fs_protocol::HelperMessage::Stat {
                        path_token,
                        file_type,
                        size,
                        mtime,
                        mode,
                        uid,
                        gid,
                        symlink_target_b64,
                        readable,
                    } = &message
                    {
                        let name_b64 = tuxstack_fs_protocol::encode_base64(path_token.as_bytes());
                        let synthetic = tuxstack_fs_protocol::HelperMessage::Entry {
                            name_b64,
                            path_token: path_token.clone(),
                            file_type: *file_type,
                            size: *size,
                            mtime: *mtime,
                            mode: *mode,
                            uid: *uid,
                            gid: *gid,
                            symlink_target_b64: symlink_target_b64.clone(),
                            readable: *readable,
                        };
                        if let Some(entry) = FilesystemEntry::from_protocol_entry(&synthetic) {
                            return Ok(entry);
                        }
                    }
                }
                tuxstack_fs_protocol::HelperMessage::Error { code, message } => {
                    return Err(map_helper_error(*code, message));
                }
                _ => {}
            }
        }
    }
    Err(FilesystemError::HelperProtocolError(
        "no stat message received".into(),
    ))
}

// ---------------------------------------------------------------------------
// Preview (bounded byte-range read)
// ---------------------------------------------------------------------------

pub async fn preview(
    client: &bollard::Docker,
    session: &FilesystemSession,
    request: &PreviewRequest,
    timeout: std::time::Duration,
    cancellation: &CancellationToken,
) -> Result<PreviewResult, FilesystemError> {
    let args = vec![
        session.helper_path.clone(),
        "preview".into(),
        "--root".into(),
        session.root.clone(),
        "--path-token".into(),
        request.path_token.0.clone(),
        "--offset".into(),
        request.offset.to_string(),
        "--limit-bytes".into(),
        request.limit.to_string(),
    ];
    let output = exec_lines(
        client,
        session,
        args,
        timeout,
        MAX_EXEC_PREVIEW,
        cancellation,
    )
    .await?;

    let mut chunks = Vec::new();
    let mut total_length = 0u64;
    let mut truncated = false;

    for line in &output {
        if let Ok(message) = serde_json::from_str::<tuxstack_fs_protocol::HelperMessage>(line) {
            match message {
                tuxstack_fs_protocol::HelperMessage::PreviewChunk {
                    data_b64,
                    offset,
                    eof,
                    truncated: t,
                } => {
                    total_length = offset + data_b64.len() as u64; // approximate
                    truncated = t;
                    chunks.push(PreviewChunk {
                        data_b64,
                        offset,
                        eof,
                        truncated: t,
                    });
                }
                tuxstack_fs_protocol::HelperMessage::Error { code, message } => {
                    return Err(map_helper_error(code, &message));
                }
                _ => {}
            }
        }
    }
    Ok(PreviewResult {
        chunks,
        total_length,
        truncated,
    })
}

// ---------------------------------------------------------------------------
// Hash
// ---------------------------------------------------------------------------

pub async fn hash(
    client: &bollard::Docker,
    session: &FilesystemSession,
    request: &HashRequest,
    timeout: std::time::Duration,
    cancellation: &CancellationToken,
) -> Result<(String, String), FilesystemError> {
    let args = vec![
        session.helper_path.clone(),
        "hash".into(),
        "--root".into(),
        session.root.clone(),
        "--path-token".into(),
        request.path_token.0.clone(),
        "--algorithm".into(),
        request.algorithm.clone(),
    ];
    let output = exec_lines(
        client,
        session,
        args,
        timeout,
        MAX_EXEC_STDOUT,
        cancellation,
    )
    .await?;

    for line in &output {
        if let Ok(message) = serde_json::from_str::<tuxstack_fs_protocol::HelperMessage>(line) {
            match &message {
                tuxstack_fs_protocol::HelperMessage::Hash { algorithm, value } => {
                    return Ok((algorithm.clone(), value.clone()));
                }
                tuxstack_fs_protocol::HelperMessage::Error { code, message } => {
                    return Err(map_helper_error(*code, message));
                }
                _ => {}
            }
        }
    }
    Err(FilesystemError::HelperProtocolError(
        "no hash message received".into(),
    ))
}

// ---------------------------------------------------------------------------
// Readlink (protocol extension)
// ---------------------------------------------------------------------------

pub async fn readlink(
    client: &bollard::Docker,
    session: &FilesystemSession,
    request: &StatRequest,
    timeout: std::time::Duration,
    cancellation: &CancellationToken,
) -> Result<FilesystemPathToken, FilesystemError> {
    let args = vec![
        session.helper_path.clone(),
        "readlink".into(),
        "--root".into(),
        session.root.clone(),
        "--path-token".into(),
        request.path_token.0.clone(),
    ];
    let output = exec_lines(
        client,
        session,
        args,
        timeout,
        MAX_EXEC_STDOUT,
        cancellation,
    )
    .await?;

    for line in &output {
        if let Ok(message) = serde_json::from_str::<tuxstack_fs_protocol::HelperMessage>(line) {
            match &message {
                tuxstack_fs_protocol::HelperMessage::Resolved { path_token } => {
                    return Ok(FilesystemPathToken(path_token.clone()));
                }
                tuxstack_fs_protocol::HelperMessage::Error { code, message } => {
                    return Err(map_helper_error(*code, message));
                }
                _ => {}
            }
        }
    }
    Err(FilesystemError::HelperProtocolError(
        "no resolved message received".into(),
    ))
}

// ---------------------------------------------------------------------------
// Exec infrastructure
// ---------------------------------------------------------------------------

/// Execute the helper and collect stdout lines (JSONL). On timeout or cancel,
/// invalidates the session (removes the container).
async fn exec_lines(
    client: &bollard::Docker,
    session: &FilesystemSession,
    cmd: Vec<String>,
    timeout: std::time::Duration,
    max_output: usize,
    cancellation: &CancellationToken,
) -> Result<Vec<String>, FilesystemError> {
    check_cancel(cancellation)?;

    let create = client
        .create_exec(
            &session.container_id,
            CreateExecOptions::<String> {
                attach_stdout: Some(true),
                attach_stderr: Some(true),
                cmd: Some(cmd),
                ..Default::default()
            },
        )
        .await
        .map_err(map_exec_error)?;

    let start = tokio::time::timeout(
        timeout,
        client.start_exec(
            &create.id,
            Some(StartExecOptions {
                detach: false,
                tty: false,
                output_capacity: Some(max_output),
            }),
        ),
    )
    .await
    .map_err(|_| FilesystemError::Timeout)?
    .map_err(map_exec_error)?;

    let StartExecResults::Attached { mut output, .. } = start else {
        return Err(FilesystemError::ExecFailed("exec started detached".into()));
    };

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    loop {
        let next = tokio::select! {
            biased;
            _ = cancellation.cancelled() => return Err(FilesystemError::Cancelled),
            item = output.next() => item,
        };
        match next {
            Some(Ok(chunk)) => match chunk {
                bollard::container::LogOutput::StdOut { message } => {
                    stdout.extend_from_slice(&message);
                }
                bollard::container::LogOutput::StdErr { message } => {
                    stderr.extend_from_slice(&message);
                }
                bollard::container::LogOutput::Console { message } => {
                    stdout.extend_from_slice(&message);
                }
                _ => {}
            },
            Some(Err(error)) => {
                return Err(FilesystemError::ExecFailed(error.to_string()));
            }
            None => break,
        }
    }

    if stdout.is_empty() && !stderr.is_empty() {
        let msg = String::from_utf8_lossy(&stderr);
        let lower = msg.to_ascii_lowercase();
        if lower.contains("permission denied") {
            return Err(FilesystemError::PermissionDenied(msg.trim().into()));
        }
        return Err(FilesystemError::ExecFailed(msg.trim().into()));
    }

    // Split stdout into lines, filtering empty lines.
    let text = String::from_utf8_lossy(&stdout);
    Ok(text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(String::from)
        .collect())
}

/// Stream raw bytes from the helper (for download or direct file reads).
pub async fn exec_raw_stream<F>(
    client: &bollard::Docker,
    session: &FilesystemSession,
    cmd: Vec<String>,
    timeout: std::time::Duration,
    cancellation: &CancellationToken,
    mut on_data: F,
) -> Result<u64, FilesystemError>
where
    F: FnMut(&[u8]) -> Result<(), FilesystemError>,
{
    check_cancel(cancellation)?;

    let create = client
        .create_exec(
            &session.container_id,
            CreateExecOptions::<String> {
                attach_stdout: Some(true),
                attach_stderr: Some(false),
                cmd: Some(cmd),
                ..Default::default()
            },
        )
        .await
        .map_err(map_exec_error)?;

    let start = tokio::time::timeout(
        timeout,
        client.start_exec(
            &create.id,
            Some(StartExecOptions {
                detach: false,
                tty: false,
                output_capacity: Some(MAX_EXEC_PREVIEW),
            }),
        ),
    )
    .await
    .map_err(|_| FilesystemError::Timeout)?
    .map_err(map_exec_error)?;

    let StartExecResults::Attached { mut output, .. } = start else {
        return Err(FilesystemError::ExecFailed("exec started detached".into()));
    };

    let mut header_buf = Vec::new();
    let mut header_done = false;
    let mut total_bytes = 0u64;

    loop {
        let next = tokio::select! {
            biased;
            _ = cancellation.cancelled() => return Err(FilesystemError::Cancelled),
            item = output.next() => item,
        };
        match next {
            Some(Ok(chunk)) => {
                let bytes = match chunk {
                    bollard::container::LogOutput::StdOut { message } => message,
                    bollard::container::LogOutput::Console { message } => message,
                    _ => continue,
                };
                if !header_done {
                    // Accumulate until first newline (the header line).
                    if let Some(pos) = bytes.iter().position(|b| *b == b'\n') {
                        header_buf.extend_from_slice(&bytes[..pos]);
                        header_done = true;
                        let header_text = String::from_utf8_lossy(&header_buf);
                        let _message: tuxstack_fs_protocol::HelperMessage =
                            serde_json::from_str(&header_text).map_err(|error| {
                                FilesystemError::HelperProtocolError(format!(
                                    "header parse: {error}"
                                ))
                            })?;
                        // Forward remaining bytes after the newline.
                        let rest = &bytes[pos + 1..];
                        if !rest.is_empty() {
                            on_data(rest)?;
                            total_bytes += rest.len() as u64;
                        }
                    } else {
                        header_buf.extend_from_slice(&bytes);
                    }
                } else {
                    on_data(&bytes)?;
                    total_bytes += bytes.len() as u64;
                }
            }
            Some(Err(error)) => {
                return Err(FilesystemError::ExecFailed(error.to_string()));
            }
            None => break,
        }
    }
    Ok(total_bytes)
}

fn check_cancel(token: &CancellationToken) -> Result<(), FilesystemError> {
    if token.is_cancelled() {
        Err(FilesystemError::Cancelled)
    } else {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Error mapping
// ---------------------------------------------------------------------------

fn map_helper_error(code: tuxstack_fs_protocol::HelperErrorCode, message: &str) -> FilesystemError {
    use tuxstack_fs_protocol::HelperErrorCode;
    match code {
        HelperErrorCode::NotFound => FilesystemError::PathNotFound(message.into()),
        HelperErrorCode::PermissionDenied => FilesystemError::PermissionDenied(message.into()),
        HelperErrorCode::IsDirectory => FilesystemError::IsDirectory(message.into()),
        HelperErrorCode::NotDirectory => FilesystemError::NotDirectory(message.into()),
        HelperErrorCode::InvalidToken => FilesystemError::InvalidPathToken(message.into()),
        HelperErrorCode::PathEscapeRejected => FilesystemError::PathEscapeRejected(message.into()),
        HelperErrorCode::SymlinkLoop => FilesystemError::UnsupportedFileType(message.into()),
        HelperErrorCode::UnsupportedFileType => {
            FilesystemError::UnsupportedFileType(message.into())
        }
        HelperErrorCode::Io => FilesystemError::ExecFailed(message.into()),
        HelperErrorCode::InvalidArgs => FilesystemError::HelperProtocolError(message.into()),
    }
}

fn map_exec_error(error: bollard::errors::Error) -> FilesystemError {
    let text = error.to_string();
    let lower = text.to_ascii_lowercase();
    if lower.contains("no such container") || lower.contains("is not running") {
        FilesystemError::SessionClosed
    } else if lower.contains("exec format error") || lower.contains("cannot execute") {
        FilesystemError::UnsupportedPlatform(text)
    } else {
        FilesystemError::ExecFailed(text)
    }
}
