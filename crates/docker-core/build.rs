//! Build script for docker-core: compiles the `tuxstack-fs-helper` binary
//! for the host architecture (and optionally a cross-arch target) and emits
//! env vars so the service crate can `include_bytes!` the result.

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let workspace_root = manifest_dir.parent().unwrap().parent().unwrap().to_path_buf();
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let target_dir = PathBuf::from(&out_dir).join("tuxstack-fs-helper-build");

    println!("cargo:rerun-if-changed=../tuxstack-fs-helper/src/main.rs");
    println!("cargo:rerun-if-changed=../tuxstack-fs-helper/src/lib.rs");
    println!("cargo:rerun-if-changed=../tuxstack-fs-helper/Cargo.toml");
    println!("cargo:rerun-if-changed=../tuxstack-fs-protocol/src/lib.rs");

    let host_arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let host_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if host_os != "linux" {
        println!("cargo:warning=tuxstack-fs-helper: only building for linux targets");
        return;
    }

    // Host-architecture helper (always built).
    match host_arch.as_str() {
        "x86_64" => {
            if build_for_triple(&cargo, &workspace_root, &target_dir, "x86_64-unknown-linux-musl") {
                let bin = target_dir.join("x86_64-unknown-linux-musl/release/tuxstack-fs-helper");
                println!("cargo:rustc-env=IMAGEFS_HELPER_X86_64={}", bin.display());
                println!("cargo:rustc-cfg=helper_x86_64");
            }
        }
        "aarch64" => {
            if build_for_triple(&cargo, &workspace_root, &target_dir, "aarch64-unknown-linux-musl") {
                let bin = target_dir.join("aarch64-unknown-linux-musl/release/tuxstack-fs-helper");
                println!("cargo:rustc-env=IMAGEFS_HELPER_AARCH64={}", bin.display());
                println!("cargo:rustc-cfg=helper_aarch64");
            }
        }
        other => {
            println!("cargo:warning=tuxstack-fs-helper: unsupported host arch {other}");
            return;
        }
    }

    // Cross-architecture helper (best-effort: target must be installed).
    let cross_triple = match host_arch.as_str() {
        "x86_64" => Some("aarch64-unknown-linux-musl"),
        "aarch64" => Some("x86_64-unknown-linux-musl"),
        _ => None,
    };
    if let Some(triple) = cross_triple {
        if build_for_triple(&cargo, &workspace_root, &target_dir, triple) {
            let bin = target_dir.join(format!("{triple}/release/tuxstack-fs-helper"));
            let env = match triple {
                "x86_64-unknown-linux-musl" => "IMAGEFS_HELPER_X86_64",
                "aarch64-unknown-linux-musl" => "IMAGEFS_HELPER_AARCH64",
                _ => unreachable!(),
            };
            let cfg = match triple {
                "x86_64-unknown-linux-musl" => "helper_x86_64",
                "aarch64-unknown-linux-musl" => "helper_aarch64",
                _ => unreachable!(),
            };
            println!("cargo:rustc-env={env}={}", bin.display());
            println!("cargo:rustc-cfg={cfg}");
        }
    }
}

fn build_for_triple(
    cargo: &str,
    workspace: &Path,
    target_dir: &Path,
    triple: &str,
) -> bool {
    Command::new(cargo)
        .current_dir(workspace)
        .args([
            "build",
            "-p",
            "tuxstack-fs-helper",
            "--release",
            "--target",
            triple,
            "--target-dir",
        ])
        .arg(target_dir)
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}
