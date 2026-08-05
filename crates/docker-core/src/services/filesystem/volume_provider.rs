//! Volume session provider: loads a scratch-based helper image containing the
//! static `tuxstack-fs-helper` binary, creates a container from it with the
//! volume mounted at `/mnt/data`, and validates the hello handshake.

use bollard::models::{ContainerCreateBody, HostConfig, Mount, MountType, ResourcesUlimits};
use bollard::query_parameters::{CreateContainerOptions, ImportImageOptions};
use chrono::Utc;
use tokio_util::sync::CancellationToken;

use super::error::FilesystemError;
use super::types::*;

use super::client;
use super::session;

const LABEL_MANAGED: &str = "io.github.tuxstack.managed";
const LABEL_PURPOSE: &str = "io.github.tuxstack.purpose";
const PURPOSE_VALUE: &str = "filesystem-helper";
const HELPER_PATH: &str = "/usr/bin/tuxstack-fs-helper";
const MOUNT_PATH: &str = "/mnt/data";

/// Helper image tag (internal, never pushed to a registry).
fn helper_image_tag() -> String {
    let arch = std::env::consts::ARCH;
    format!("tuxstack.internal/fs-helper:1-{arch}")
}

/// Ensure the helper image is loaded into the Docker daemon. If it is not
/// present, generates a minimal scratch-based image from the embedded helper
/// binary and loads it via `docker load`.
pub async fn ensure_helper_image(
    client: &bollard::Docker,
    timeout: std::time::Duration,
) -> Result<(), FilesystemError> {
    let tag = helper_image_tag();
    // Check if the image already exists.
    match tokio::time::timeout(timeout, client.inspect_image(&tag)).await {
        Ok(Ok(_)) => return Ok(()),
        Ok(Err(_)) => {} // Not found → load.
        Err(_) => return Err(FilesystemError::Timeout),
    }

    let helper_bytes = helper_binary_for_host().ok_or_else(|| {
        FilesystemError::HelperBinaryUnavailable("host architecture not supported".into())
    })?;

    let image_tar = build_load_tar(helper_bytes);

    use bollard::body_full;
    use futures_util::TryStreamExt;
    let opts = ImportImageOptions::default();
    client
        .import_image(opts, body_full(bytes::Bytes::from(image_tar)), None)
        .try_collect::<Vec<_>>()
        .await
        .map_err(|error| FilesystemError::HelperImageLoadFailed(error.to_string()))?;

    Ok(())
}

/// Create a filesystem session for a volume. The helper image must already
/// be loaded (call [`ensure_helper_image`] first).
pub async fn create_session(
    client: &bollard::Docker,
    volume_name: &str,
    timeout: std::time::Duration,
    cancellation: &CancellationToken,
) -> Result<FilesystemSession, FilesystemError> {
    // 1. Ensure the helper image is loaded.
    ensure_helper_image(client, timeout).await?;

    // 2. Create the preview container with the volume mounted.
    let session_id = uuid::Uuid::new_v4();
    let container_name = format!("tuxstack-fs-helper-{session_id}");
    let tag = helper_image_tag();

    let labels = [
        (LABEL_MANAGED.to_string(), "true".into()),
        (LABEL_PURPOSE.to_string(), PURPOSE_VALUE.into()),
        (volume_name.to_string(), "true".into()),
    ]
    .into_iter()
    .collect();

    let mounts = vec![Mount {
        typ: Some(MountType::VOLUME),
        source: Some(volume_name.into()),
        target: Some(MOUNT_PATH.into()),
        read_only: Some(true),
        ..Default::default()
    }];

    let response = tokio::time::timeout(
        timeout,
        client.create_container(
            Some(CreateContainerOptions {
                name: Some(container_name.clone()),
                platform: String::new(),
            }),
            ContainerCreateBody {
                image: Some(tag),
                entrypoint: Some(vec![HELPER_PATH.into()]),
                cmd: Some(vec!["hold".into()]),
                user: Some("0".into()),
                labels: Some(labels),
                host_config: Some(HostConfig {
                    network_mode: Some("none".into()),
                    memory: Some(128 * 1024 * 1024),
                    nano_cpus: Some(250_000_000),
                    pids_limit: Some(32),
                    readonly_rootfs: Some(true),
                    security_opt: Some(vec!["no-new-privileges:true".into()]),
                    cap_drop: Some(vec!["ALL".into()]),
                    auto_remove: Some(false),
                    mounts: Some(mounts),
                    ulimits: Some(vec![ResourcesUlimits {
                        name: Some("nofile".into()),
                        soft: Some(1024),
                        hard: Some(1024),
                    }]),
                    ..Default::default()
                }),
                ..Default::default()
            },
        ),
    )
    .await
    .map_err(|_| FilesystemError::Timeout)?
    .map_err(|error| FilesystemError::HelperContainerCreateFailed(error.to_string()))?;

    // 3. Start the container.
    tokio::time::timeout(timeout, client.start_container(&response.id, None))
        .await
        .map_err(|_| FilesystemError::Timeout)?
        .map_err(|error| FilesystemError::HelperContainerStartFailed(error.to_string()))?;

    // 4. Hello handshake.
    let session = FilesystemSession {
        container_id: response.id,
        container_name,
        source: FilesystemSource::Volume {
            volume_name: volume_name.to_string(),
        },
        root: MOUNT_PATH.into(),
        helper_path: HELPER_PATH.into(),
        protocol_version: tuxstack_fs_protocol::FS_HELPER_PROTOCOL_VERSION,
        helper_version: String::new(),
        read_only: true,
        created_at: Utc::now(),
    };

    let helper_version = client::hello(client, &session, timeout, cancellation)
        .await
        .inspect_err(|_| {
            let client_clone = client.clone();
            let session_clone = session.clone();
            tokio::spawn(async move {
                let _ = session::invalidate_session(&client_clone, &session_clone).await;
            });
        })?;

    Ok(FilesystemSession {
        helper_version,
        ..session
    })
}

// ---------------------------------------------------------------------------
// Helper binary selection (host architecture)
// ---------------------------------------------------------------------------

#[cfg(helper_x86_64)]
const HELPER_X86_64: &[u8] = include_bytes!(env!("IMAGEFS_HELPER_X86_64"));

#[cfg(helper_aarch64)]
const HELPER_AARCH64: &[u8] = include_bytes!(env!("IMAGEFS_HELPER_AARCH64"));

fn helper_binary_for_host() -> Option<&'static [u8]> {
    match std::env::consts::ARCH {
        #[cfg(helper_x86_64)]
        "x86_64" | "amd64" => Some(HELPER_X86_64),
        #[cfg(helper_aarch64)]
        "aarch64" | "arm64" => Some(HELPER_AARCH64),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Docker-loadable image tar generation
// ---------------------------------------------------------------------------

/// Build a Docker-loadable image tar containing a scratch image with just
/// the `tuxstack-fs-helper` binary. The tar is a valid input for
/// `docker load`.
fn build_load_tar(helper_binary: &[u8]) -> Vec<u8> {
    // Layer: single tar containing /usr/bin/tuxstack-fs-helper
    let layer = build_layer_tar(helper_binary);
    let layer_sha = sha256_hex(&layer);

    // Config JSON
    let tag = helper_image_tag();
    let arch = std::env::consts::ARCH;
    let config = format!(
        r#"{{"architecture":"{arch}","os":"linux","config":{{"Entrypoint":["{HELPER_PATH}"],"Cmd":["hold"]}},"rootfs":{{"type":"layers","diff_ids":["sha256:{layer_sha}"]}},"history":[{{"created":"1970-01-01T00:00:00Z","comment":"tuxstack filesystem helper v{version}"}}],"config_env":["PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"]}}"#,
        version = env!("CARGO_PKG_VERSION"),
    );
    let config_sha = sha256_hex(config.as_bytes());

    // Manifest
    let manifest = format!(
        r#"[{{"Config":"{config_sha}.json","RepoTags":["{tag}"],"Layers":["{layer_sha}.tar"]}}]"#
    );

    // Repositories
    let repositories = format!(r#"{{"{tag}":["{config_sha}"]}}"#);

    // Assemble the outer tar
    let mut out = Vec::new();

    // manifest.json
    write_tar_entry(&mut out, "manifest.json", manifest.as_bytes());
    // config
    write_tar_entry(&mut out, &format!("{config_sha}.json"), config.as_bytes());
    // layer
    write_tar_entry(&mut out, &format!("{layer_sha}.tar"), &layer);
    // repositories
    write_tar_entry(&mut out, "repositories", repositories.as_bytes());

    // End-of-archive
    out.extend(std::iter::repeat_n(0u8, 1024));
    out
}

/// Build the inner layer tar: just `/usr/bin/tuxstack-fs-helper`.
fn build_layer_tar(helper_binary: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(512 + helper_binary.len() + 1024 + 512);
    write_ustar_header(&mut out, "usr/bin/", 0o755, 0, true);
    write_ustar_header(
        &mut out,
        "usr/bin/tuxstack-fs-helper",
        0o755,
        helper_binary.len() as u64,
        false,
    );
    out.extend_from_slice(helper_binary);
    let padding = (512 - (helper_binary.len() % 512)) % 512;
    out.extend(std::iter::repeat_n(0u8, padding));
    out.extend(std::iter::repeat_n(0u8, 1024));
    out
}

/// Write a file into a tar archive.
fn write_tar_entry(out: &mut Vec<u8>, name: &str, data: &[u8]) {
    write_ustar_header(out, name, 0o644, data.len() as u64, false);
    out.extend_from_slice(data);
    let padding = (512 - (data.len() % 512)) % 512;
    out.extend(std::iter::repeat_n(0u8, padding));
}

/// Write a 512-byte ustar tar header.
fn write_ustar_header(out: &mut Vec<u8>, name: &str, mode: u32, size: u64, is_dir: bool) {
    let mut header = [0u8; 512];
    let name_bytes = name.as_bytes();
    let name_len = name_bytes.len().min(100);
    header[..name_len].copy_from_slice(&name_bytes[..name_len]);
    write_octal(&mut header[100..108], mode as u64);
    write_octal(&mut header[108..116], 0);
    write_octal(&mut header[116..124], 0);
    write_octal(&mut header[124..136], if is_dir { 0 } else { size });
    write_octal(&mut header[136..148], 0);
    header[148..156].copy_from_slice(b"        ");
    header[156] = if is_dir { b'5' } else { b'0' };
    header[257..263].copy_from_slice(b"ustar\0");
    header[263..265].copy_from_slice(b"00");
    let checksum: u32 = header.iter().map(|&b| b as u32).sum();
    write_octal(&mut header[148..156], checksum as u64);
    out.extend_from_slice(&header);
}

fn write_octal(field: &mut [u8], value: u64) {
    let octal = format!("{value:0width$o}", width = field.len() - 1);
    let bytes = octal.as_bytes();
    field[..bytes.len()].copy_from_slice(bytes);
    field[bytes.len()] = 0;
}

/// Hand-rolled SHA-256 (the volume provider has no serde_json dep path to
/// crypto, and we only hash small in-memory blobs).
fn sha256_hex(data: &[u8]) -> String {
    // Use the same algorithm as the helper crate, inlined here to keep the
    // docker-core crate dependency-light for this module.
    struct Sha256 {
        state: [u32; 8],
        buffer: [u8; 64],
        buffer_len: usize,
        total: u64,
    }

    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    impl Sha256 {
        fn new() -> Self {
            Self {
                state: [
                    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c,
                    0x1f83d9ab, 0x5be0cd19,
                ],
                buffer: [0; 64],
                buffer_len: 0,
                total: 0,
            }
        }
        fn update(&mut self, mut data: &[u8]) {
            self.total = self.total.wrapping_add(data.len() as u64);
            if self.buffer_len > 0 {
                let space = 64 - self.buffer_len;
                let take = space.min(data.len());
                self.buffer[self.buffer_len..self.buffer_len + take].copy_from_slice(&data[..take]);
                self.buffer_len += take;
                data = &data[take..];
                if self.buffer_len == 64 {
                    let block = self.buffer;
                    self.compress(&block);
                    self.buffer_len = 0;
                }
            }
            while data.len() >= 64 {
                let mut arr = [0u8; 64];
                arr.copy_from_slice(&data[..64]);
                self.compress(&arr);
                data = &data[64..];
            }
            if !data.is_empty() {
                self.buffer[..data.len()].copy_from_slice(data);
                self.buffer_len = data.len();
            }
        }
        fn compress(&mut self, block: &[u8; 64]) {
            let mut w = [0u32; 64];
            for (i, chunk) in block.chunks_exact(4).enumerate() {
                w[i] = u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            }
            for i in 16..64 {
                let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
                let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
                w[i] = w[i - 16]
                    .wrapping_add(s0)
                    .wrapping_add(w[i - 7])
                    .wrapping_add(s1);
            }
            let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = self.state;
            for i in 0..64 {
                let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
                let ch = (e & f) ^ ((!e) & g);
                let t1 = h
                    .wrapping_add(s1)
                    .wrapping_add(ch)
                    .wrapping_add(K[i])
                    .wrapping_add(w[i]);
                let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
                let maj = (a & b) ^ (a & c) ^ (b & c);
                let t2 = s0.wrapping_add(maj);
                h = g;
                g = f;
                f = e;
                e = d.wrapping_add(t1);
                d = c;
                c = b;
                b = a;
                a = t1.wrapping_add(t2);
            }
            self.state[0] = self.state[0].wrapping_add(a);
            self.state[1] = self.state[1].wrapping_add(b);
            self.state[2] = self.state[2].wrapping_add(c);
            self.state[3] = self.state[3].wrapping_add(d);
            self.state[4] = self.state[4].wrapping_add(e);
            self.state[5] = self.state[5].wrapping_add(f);
            self.state[6] = self.state[6].wrapping_add(g);
            self.state[7] = self.state[7].wrapping_add(h);
        }
        fn finish(mut self) -> [u8; 32] {
            let bit_len = self.total.wrapping_mul(8);
            self.update(&[0x80]);
            while self.buffer_len != 56 {
                self.update(&[0]);
            }
            self.update(&bit_len.to_be_bytes());
            let mut out = [0u8; 32];
            for (i, word) in self.state.iter().enumerate() {
                out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
            }
            out
        }
    }

    let mut h = Sha256::new();
    h.update(data);
    let digest = h.finish();
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_load_tar_produces_loadable_structure() {
        let tar = build_load_tar(b"test-binary");
        // Contains manifest.json, config.json, layer.tar, repositories, end-of-archive
        assert!(tar.len() > 2048);
        // First entry should be manifest.json
        assert_eq!(&tar[..13], b"manifest.json");
    }

    #[test]
    fn helper_image_tag_includes_arch() {
        let tag = helper_image_tag();
        assert!(tag.contains("tuxstack.internal/fs-helper:1-"));
        assert!(tag.contains(std::env::consts::ARCH));
    }
}
