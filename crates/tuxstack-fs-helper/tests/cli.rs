//! End-to-end CLI tests for the `tuxstack-fs-helper` binary, run against
//! real directories (host-native; the same binary is injected into preview
//! containers and mounted volume sessions).

use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;

use tuxstack_fs_protocol::{HelperMessage, decode_base64};

const BIN: &str = env!("CARGO_BIN_EXE_tuxstack-fs-helper");

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let mut dir = std::env::temp_dir();
        dir.push(format!("fs-helper-cli-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run(args: &[&str]) -> (i32, Vec<HelperMessage>) {
    let output = Command::new(BIN).args(args).output().expect("spawn helper");
    let mut messages = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if line.trim().is_empty() {
            continue;
        }
        messages.push(serde_json::from_str::<HelperMessage>(line).expect("parse helper line"));
    }
    (output.status.code().unwrap_or(-1), messages)
}

fn token_of(relative: &str) -> String {
    tuxstack_fs_protocol::FilesystemPathToken::from_relative(relative)
        .unwrap()
        .0
}

fn entry_names(messages: &[HelperMessage]) -> Vec<String> {
    messages
        .iter()
        .filter_map(|message| match message {
            HelperMessage::Entry { name_b64, .. } => {
                Some(String::from_utf8_lossy(&decode_base64(name_b64).unwrap()).into_owned())
            }
            _ => None,
        })
        .collect()
}

fn end_of(messages: &[HelperMessage]) -> Option<&HelperMessage> {
    messages
        .iter()
        .find(|message| matches!(message, HelperMessage::End { .. }))
}

// ---------------------------------------------------------------------------

#[test]
fn list_plain_directory() {
    let dir = TempDir::new("plain");
    for name in ["etc", "bin", "usr", "var"] {
        std::fs::create_dir(dir.0.join(name)).unwrap();
    }
    std::fs::write(dir.0.join("app.log"), b"log").unwrap();

    let root = dir.0.to_str().unwrap();
    let (code, messages) = run(&[
        "list",
        "--root",
        root,
        "--path-token",
        &token_of(""),
        "--limit",
        "1000",
    ]);
    assert_eq!(code, 0);
    assert_eq!(
        entry_names(&messages),
        vec!["app.log", "bin", "etc", "usr", "var"] // sorted by raw bytes
    );
    assert!(matches!(
        end_of(&messages),
        Some(HelperMessage::End {
            truncated: false,
            next_cursor: None
        })
    ));
    // Every entry carries a usable token.
    for message in &messages {
        if let HelperMessage::Entry { path_token, .. } = message {
            assert!(
                tuxstack_fs_protocol::FilesystemPathToken(path_token.clone())
                    .decode_relative()
                    .is_ok()
            );
        }
    }
}

#[test]
fn list_empty_directory() {
    let dir = TempDir::new("empty");
    let root = dir.0.to_str().unwrap();
    let (code, messages) = run(&["list", "--root", root, "--path-token", &token_of("")]);
    assert_eq!(code, 0);
    assert!(entry_names(&messages).is_empty());
    assert!(matches!(
        end_of(&messages),
        Some(HelperMessage::End {
            truncated: false,
            next_cursor: None
        })
    ));
}

#[test]
fn list_hidden_filtering_and_special_names() {
    let dir = TempDir::new("names");
    for name in [
        "visible",
        ".hidden",
        "with space",
        "tab\there",
        "new\nline",
        "dash-name",
    ] {
        std::fs::create_dir(dir.0.join(name)).unwrap();
    }
    // Non-UTF-8 name (raw bytes).
    std::fs::create_dir(dir.0.join(PathBuf::from(OsString::from_vec(vec![
        b'v', 0xff, b'a', b't',
    ]))))
    .unwrap();

    let root = dir.0.to_str().unwrap();
    let (code, hidden_off) = run(&[
        "list",
        "--root",
        root,
        "--path-token",
        &token_of(""),
        "--show-hidden",
    ]);
    assert_eq!(code, 0);
    let names = entry_names(&hidden_off);
    assert!(names.iter().any(|name| name == ".hidden"));
    assert!(names.iter().any(|name| name == "with space"));
    assert!(names.iter().any(|name| name == "new\nline"));
    assert!(names.iter().any(|name| name.contains('\u{fffd}'))); // lossy display

    let (_, hidden_on) = run(&["list", "--root", root, "--path-token", &token_of("")]);
    assert!(!entry_names(&hidden_on).iter().any(|name| name == ".hidden"));
}

#[test]
fn list_pagination_and_cursor() {
    let dir = TempDir::new("pages");
    for i in 0..10 {
        std::fs::create_dir(dir.0.join(format!("entry-{i:02}"))).unwrap();
    }
    let root = dir.0.to_str().unwrap();

    let (code, page1) = run(&[
        "list",
        "--root",
        root,
        "--path-token",
        &token_of(""),
        "--limit",
        "4",
    ]);
    assert_eq!(code, 0);
    let names1 = entry_names(&page1);
    assert_eq!(names1, vec!["entry-00", "entry-01", "entry-02", "entry-03"]);
    let HelperMessage::End {
        truncated,
        next_cursor,
    } = end_of(&page1).unwrap()
    else {
        panic!("no end");
    };
    assert!(truncated);
    let cursor = next_cursor.clone().expect("cursor");

    let (code, page2) = run(&[
        "list",
        "--root",
        root,
        "--path-token",
        &token_of(""),
        "--limit",
        "4",
        "--cursor",
        &cursor,
    ]);
    assert_eq!(code, 0);
    assert_eq!(
        entry_names(&page2),
        vec!["entry-04", "entry-05", "entry-06", "entry-07"]
    );

    let HelperMessage::End { next_cursor, .. } = end_of(&page2).unwrap() else {
        panic!("no end");
    };
    let (code, page3) = run(&[
        "list",
        "--root",
        root,
        "--path-token",
        &token_of(""),
        "--limit",
        "4",
        "--cursor",
        &next_cursor.clone().unwrap(),
    ]);
    assert_eq!(code, 0);
    assert_eq!(entry_names(&page3), vec!["entry-08", "entry-09"]);
    assert!(matches!(
        end_of(&page3),
        Some(HelperMessage::End {
            truncated: false,
            next_cursor: None
        })
    ));
}

#[test]
fn list_symlinks_and_dangling() {
    let dir = TempDir::new("links");
    std::fs::write(dir.0.join("real.txt"), b"data").unwrap();
    std::os::unix::fs::symlink("real.txt", dir.0.join("good")).unwrap();
    std::os::unix::fs::symlink("missing", dir.0.join("dangling")).unwrap();

    let root = dir.0.to_str().unwrap();
    let (code, messages) = run(&["list", "--root", root, "--path-token", &token_of("")]);
    assert_eq!(code, 0);
    let good = messages
        .iter()
        .find_map(|message| match message {
            HelperMessage::Entry {
                name_b64,
                file_type,
                symlink_target_b64,
                ..
            } if decode_base64(name_b64).unwrap() == b"good" => {
                Some((*file_type, symlink_target_b64.clone()))
            }
            _ => None,
        })
        .unwrap();
    assert_eq!(good.0, tuxstack_fs_protocol::HelperFileType::Symlink);
    assert_eq!(decode_base64(&good.1.unwrap()).unwrap(), b"real.txt");

    let dangling = messages.iter().any(|message| match message {
        HelperMessage::Entry {
            name_b64,
            file_type,
            ..
        } => {
            decode_base64(name_b64).unwrap() == b"dangling"
                && *file_type == tuxstack_fs_protocol::HelperFileType::Symlink
        }
        _ => false,
    });
    assert!(dangling);
}

#[test]
fn list_large_directory_truncates_at_limit() {
    let dir = TempDir::new("big");
    for i in 0..500 {
        std::fs::create_dir(dir.0.join(format!("item-{i:04}"))).unwrap();
    }
    let root = dir.0.to_str().unwrap();
    let (code, messages) = run(&[
        "list",
        "--root",
        root,
        "--path-token",
        &token_of(""),
        "--limit",
        "100",
    ]);
    assert_eq!(code, 0);
    assert_eq!(entry_names(&messages).len(), 100);
    assert!(matches!(
        end_of(&messages),
        Some(HelperMessage::End {
            truncated: true,
            next_cursor: Some(_)
        })
    ));
}

#[test]
fn list_missing_and_not_directory() {
    let dir = TempDir::new("missing");
    let root = dir.0.to_str().unwrap();
    let (code, messages) = run(&["list", "--root", root, "--path-token", &token_of("nope")]);
    assert_ne!(code, 0);
    assert!(matches!(
        messages.last(),
        Some(HelperMessage::Error {
            code: tuxstack_fs_protocol::HelperErrorCode::NotFound,
            ..
        })
    ));

    std::fs::write(dir.0.join("file.txt"), b"x").unwrap();
    let (code, messages) = run(&[
        "list",
        "--root",
        root,
        "--path-token",
        &token_of("file.txt"),
    ]);
    assert_ne!(code, 0);
    assert!(matches!(
        messages.last(),
        Some(HelperMessage::Error {
            code: tuxstack_fs_protocol::HelperErrorCode::NotDirectory,
            ..
        })
    ));
}

#[test]
fn list_permission_denied() {
    if unsafe { libc::geteuid() } == 0 {
        eprintln!("running as root; skipping permission-denied check");
        return;
    }
    let dir = TempDir::new("perm");
    let locked = dir.0.join("locked");
    std::fs::create_dir(&locked).unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    let root = dir.0.to_str().unwrap();
    let (code, messages) = run(&["list", "--root", root, "--path-token", &token_of("locked")]);
    let _ = std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755));
    assert_ne!(code, 0);
    assert!(matches!(
        messages.last(),
        Some(HelperMessage::Error {
            code: tuxstack_fs_protocol::HelperErrorCode::PermissionDenied,
            ..
        })
    ));
}

#[test]
fn path_token_escape_is_rejected() {
    let dir = TempDir::new("escape");
    std::fs::create_dir_all(dir.0.join("sub")).unwrap();
    std::fs::write(dir.0.join("secret.txt"), b"secret").unwrap();
    let root = dir.0.to_str().unwrap();

    // A forged token with ".." must be refused by the helper (the token
    // decoder rejects it before any filesystem access).
    let forged = format!(
        "v1:{}",
        tuxstack_fs_protocol::encode_base64(b"../secret.txt")
    );
    let (code, messages) = run(&["stat", "--root", root, "--path-token", &forged]);
    assert_ne!(code, 0);
    assert!(matches!(
        messages.last(),
        Some(HelperMessage::Error {
            code: tuxstack_fs_protocol::HelperErrorCode::InvalidToken,
            ..
        })
    ));

    // Absolute-path token.
    let absolute = format!("v1:{}", tuxstack_fs_protocol::encode_base64(b"/etc/passwd"));
    let (code, _) = run(&["stat", "--root", root, "--path-token", &absolute]);
    assert_ne!(code, 0);
}

#[test]
fn symlink_escape_is_rejected_for_reads() {
    let dir = TempDir::new("linkescape");
    std::fs::create_dir(dir.0.join("inside")).unwrap();
    std::fs::write(dir.0.join("inside/ok.txt"), b"ok").unwrap();
    // Symlink pointing outside the browse root.
    std::os::unix::fs::symlink("/etc/hostname", dir.0.join("inside/out")).unwrap();
    let root = dir.0.to_str().unwrap();

    let (code, messages) = run(&[
        "preview",
        "--root",
        root,
        "--path-token",
        &token_of("inside/out"),
    ]);
    assert_ne!(code, 0);
    assert!(matches!(
        messages.last(),
        Some(HelperMessage::Error {
            code: tuxstack_fs_protocol::HelperErrorCode::PathEscapeRejected,
            ..
        })
    ));

    // In-root symlink is fine.
    std::os::unix::fs::symlink("ok.txt", dir.0.join("inside/good")).unwrap();
    let (code, messages) = run(&[
        "preview",
        "--root",
        root,
        "--path-token",
        &token_of("inside/good"),
        "--limit-bytes",
        "8",
    ]);
    assert_eq!(code, 0);
    assert!(messages.iter().any(|m| matches!(
        m,
        HelperMessage::PreviewChunk { data_b64, eof: true, .. } if decode_base64(data_b64).unwrap() == b"ok"
    )));
}

#[test]
fn stat_regular_file_and_missing() {
    let dir = TempDir::new("stat");
    std::fs::write(dir.0.join("data.bin"), b"0123456789").unwrap();
    let root = dir.0.to_str().unwrap();

    let (code, messages) = run(&[
        "stat",
        "--root",
        root,
        "--path-token",
        &token_of("data.bin"),
    ]);
    assert_eq!(code, 0);
    assert!(matches!(
        messages.last(),
        Some(HelperMessage::Stat {
            file_type: tuxstack_fs_protocol::HelperFileType::File,
            size: Some(10),
            ..
        })
    ));

    let (code, messages) = run(&["stat", "--root", root, "--path-token", &token_of("missing")]);
    assert_ne!(code, 0);
    assert!(matches!(
        messages.last(),
        Some(HelperMessage::Error {
            code: tuxstack_fs_protocol::HelperErrorCode::NotFound,
            ..
        })
    ));
}

#[test]
fn preview_bounded_offset_and_directory_refusal() {
    let dir = TempDir::new("preview");
    let content: Vec<u8> = (0..=255u8).collect();
    std::fs::write(dir.0.join("blob.bin"), &content).unwrap();
    let root = dir.0.to_str().unwrap();

    let (code, messages) = run(&[
        "preview",
        "--root",
        root,
        "--path-token",
        &token_of("blob.bin"),
        "--offset",
        "100",
        "--limit-bytes",
        "100",
    ]);
    assert_eq!(code, 0);
    let mut collected = Vec::new();
    let mut eof = false;
    let mut truncated = false;
    for message in &messages {
        if let HelperMessage::PreviewChunk {
            data_b64,
            eof: e,
            truncated: t,
            ..
        } = message
        {
            collected.extend_from_slice(&decode_base64(data_b64).unwrap());
            eof = *e;
            truncated = *t;
        }
    }
    assert_eq!(collected, content[100..200]);
    assert!(eof);
    assert!(truncated); // 256 total, read 100 from offset 100 -> end of range < size
    assert_eq!(collected.len(), 100);

    // Directory preview is refused.
    let (code, messages) = run(&["preview", "--root", root, "--path-token", &token_of("")]);
    assert_ne!(code, 0);
    assert!(matches!(
        messages.last(),
        Some(HelperMessage::Error {
            code: tuxstack_fs_protocol::HelperErrorCode::IsDirectory,
            ..
        })
    ));
}

#[test]
fn preview_refuses_fifo() {
    let dir = TempDir::new("fifo");
    let fifo = dir.0.join("pipe");
    let c_string = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
    unsafe {
        libc::mkfifo(c_string.as_ptr(), 0o644);
    }
    let root = dir.0.to_str().unwrap();
    let (code, messages) = run(&["preview", "--root", root, "--path-token", &token_of("pipe")]);
    assert_ne!(code, 0);
    assert!(matches!(
        messages.last(),
        Some(HelperMessage::Error {
            code: tuxstack_fs_protocol::HelperErrorCode::UnsupportedFileType,
            ..
        })
    ));
}

#[test]
fn hash_streams_sha256() {
    let dir = TempDir::new("hash");
    let file = dir.0.join("payload.bin");
    std::fs::write(&file, b"abc").unwrap();
    let root = dir.0.to_str().unwrap();
    let (code, messages) = run(&[
        "hash",
        "--root",
        root,
        "--path-token",
        &token_of("payload.bin"),
    ]);
    assert_eq!(code, 0);
    assert!(matches!(
        messages.last(),
        Some(HelperMessage::Hash { algorithm, value }) if algorithm == "sha256"
            && value == "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    ));
}

#[test]
fn readlink_resolves_chain_within_root() {
    let dir = TempDir::new("readlink");
    std::fs::create_dir_all(dir.0.join("usr")).unwrap();
    std::fs::write(dir.0.join("usr/tool"), b"x").unwrap();
    std::os::unix::fs::symlink("usr", dir.0.join("bin")).unwrap();
    std::os::unix::fs::symlink("bin/tool", dir.0.join("alias")).unwrap();
    let root = dir.0.to_str().unwrap();

    let (code, messages) = run(&[
        "readlink",
        "--root",
        root,
        "--path-token",
        &token_of("alias"),
    ]);
    assert_eq!(code, 0);
    let HelperMessage::Resolved { path_token } = messages.last().unwrap() else {
        panic!("expected resolved");
    };
    let token = tuxstack_fs_protocol::FilesystemPathToken(path_token.clone());
    assert_eq!(token.decode_relative().unwrap(), b"usr/tool");
}

#[test]
fn hello_and_version_handshake() {
    let (code, messages) = run(&["hello"]);
    assert_eq!(code, 0);
    assert!(matches!(
        messages.last(),
        Some(HelperMessage::Hello { protocol, .. }) if *protocol == tuxstack_fs_protocol::FS_HELPER_PROTOCOL_VERSION
    ));
}

#[test]
fn unknown_command_fails_cleanly() {
    let (code, messages) = run(&["frobnicate"]);
    assert_ne!(code, 0);
    assert!(matches!(
        messages.last(),
        Some(HelperMessage::Error {
            code: tuxstack_fs_protocol::HelperErrorCode::InvalidArgs,
            ..
        })
    ));
}
