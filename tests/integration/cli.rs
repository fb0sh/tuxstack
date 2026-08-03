//! CLI end-to-end integration tests (require a reachable Docker Engine).
//!
//! Run with:
//!
//! ```bash
//! cargo test -p tuxstack-cli --test cli -- --ignored --nocapture
//! ```

use std::process::Command;

/// Path to the built `tuxstack` binary (set by Cargo for integration tests).
const BIN: &str = env!("CARGO_BIN_EXE_tuxstack");

fn run(args: &[&str]) -> std::process::Output {
    Command::new(BIN).args(args).output().expect("run tuxstack")
}

#[test]
#[ignore = "requires a reachable Docker Engine"]
fn info_prints_engine_details() {
    let out = run(&["info"]);
    assert!(out.status.success(), "info must succeed");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("Docker version"),
        "info must show version: {stdout}"
    );
}

#[test]
#[ignore = "requires a reachable Docker Engine"]
fn info_json_is_valid() {
    let out = run(&["--json", "info"]);
    assert!(out.status.success());
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).expect("valid JSON");
    assert!(value.get("server_version").is_some());
}

#[test]
#[ignore = "requires a reachable Docker Engine"]
fn ps_lists_containers() {
    let out = run(&["ps", "--all"]);
    assert!(out.status.success(), "ps must succeed");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("CONTAINER ID"),
        "ps must have a header: {stdout}"
    );
}

#[test]
#[ignore = "requires a reachable Docker Engine"]
fn images_and_networks_and_volumes() {
    for args in [&["images"][..], &["networks"][..], &["volumes"][..]] {
        let out = run(args);
        assert!(out.status.success(), "{args:?} must succeed");
    }
}

#[test]
#[ignore = "requires a reachable Docker Engine"]
fn missing_docker_uses_exit_code_3() {
    // Point at a non-existent socket.
    let out = run(&["--host", "unix:///tmp/tuxstack-does-not-exist.sock", "info"]);
    assert_eq!(
        out.status.code(),
        Some(3),
        "exit code must be 3 (docker unavailable)"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("socket"),
        "stderr must explain the failure: {stderr}"
    );
}
