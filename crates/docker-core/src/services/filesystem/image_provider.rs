//! Image session provider: creates a preview container from the target image,
//! injects the static helper binary via the Docker Archive API, starts it,
//! and validates the hello handshake.

use bollard::models::{ContainerCreateBody, HostConfig, ResourcesUlimits};
use bollard::query_parameters::{CreateContainerOptions, UploadToContainerOptions};
use bollard::Docker;
use chrono::Utc;
use tokio_util::sync::CancellationToken;

use super::error::FilesystemError;
use super::types::*;

use super::client;
use super::session;

const LABEL_MANAGED: &str = "io.github.tuxstack.managed";
const LABEL_PURPOSE: &str = "io.github.tuxstack.purpose";
const PURPOSE_VALUE: &str = "filesystem-helper";

/// Helper binary path inside the container.
const HELPER_DIR: &str = "/.tuxstack";
const HELPER_PATH: &str = "/.tuxstack/tuxstack-fs-helper";
const HELPER_BUNDLE_PATH: &str = "/.tuxstack";

/// Create a filesystem session for an image. The preview container is
/// created from the image itself, with the static helper injected via
/// put_archive before starting.
pub async fn create_session(
    client: &bollard::Docker,
    image_id: &str,
    timeout: std::time::Duration,
    cancellation: &CancellationToken,
) -> Result<FilesystemSession, FilesystemError> {
    // 1. Inspect image → get immutable ID, architecture, OS.
    let inspect = tokio::time::timeout(
        timeout,
        client.inspect_image(image_id),
    )
    .await
    .map_err(|_| FilesystemError::Timeout)?
    .map_err(|error| {
        let text = error.to_string().to_ascii_lowercase();
        if text.contains("not found") || text.contains("no such image") {
            FilesystemError::ImageNotFound(image_id.to_string())
        } else {
            FilesystemError::ExecFailed(error.to_string())
        }
    })?;

    let immutable_id = inspect
        .id
        .clone()
        .unwrap_or_else(|| image_id.to_string());
    let os = inspect.os.as_deref().unwrap_or("linux");
    if os != "linux" {
        return Err(FilesystemError::UnsupportedPlatform(format!(
            "image operating system is {os}"
        )));
    }
    let arch = inspect
        .architecture
        .as_deref()
        .unwrap_or("amd64")
        .to_string();
    let platform = format!("{os}/{arch}");

    // 2. Select the correct helper binary for this architecture.
    let (helper_bytes, helper_arch) = helper_bytes_for_arch(&arch)
        .ok_or_else(|| FilesystemError::HelperBinaryUnavailable(format!(
            "no helper binary for architecture {arch}"
        )))?;

    // 3. Build the tar archive with the helper binary.
    let tar = build_helper_archive(helper_bytes);

    // 4. Create the preview container.
    let session_id = uuid::Uuid::new_v4();
    let container_name = format!("tuxstack-fs-helper-{session_id}");
    let labels = [
        (LABEL_MANAGED.to_string(), "true".into()),
        (LABEL_PURPOSE.to_string(), PURPOSE_VALUE.into()),
        (image_id.to_string(), "true".into()),
    ]
    .into_iter()
    .collect();

    let response = tokio::time::timeout(
        timeout,
        client.create_container(
            Some(CreateContainerOptions {
                name: Some(container_name.clone()),
                platform: String::new(),
            }),
            ContainerCreateBody {
                image: Some(image_id.to_string()),
                entrypoint: Some(vec![HELPER_PATH.into()]),
                cmd: Some(vec!["hold".into()]),
                user: Some("0".into()),
                working_dir: Some("/".into()),
                labels: Some(labels),
                host_config: Some(HostConfig {
                    network_mode: Some("none".into()),
                    memory: Some(128 * 1024 * 1024),
                    nano_cpus: Some(250_000_000),
                    pids_limit: Some(32),
                    readonly_rootfs: Some(false), // helper needs to write the injection layer
                    security_opt: Some(vec!["no-new-privileges:true".into()]),
                    cap_drop: Some(vec!["ALL".into()]),
                    auto_remove: Some(false),
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
    .map_err(|error| {
        let text = error.to_string();
        if text.to_ascii_lowercase().contains("no such image") {
            FilesystemError::ImageNotFound(image_id.to_string())
        } else {
            FilesystemError::HelperContainerCreateFailed(text)
        }
    })?;

    // 5. Inject the helper binary via the Docker Archive API.
    use bollard::body_full;
    client
        .upload_to_container(
            &response.id,
            Some(UploadToContainerOptions {
                path: "/".to_string(),
                ..Default::default()
            }),
            body_full(bytes::Bytes::from(tar)),
        )
        .await
        .map_err(|error| FilesystemError::HelperImageLoadFailed(error.to_string()))?;

    // 6. Start the container.
    tokio::time::timeout(
        timeout,
        client.start_container(&response.id, None),
    )
    .await
    .map_err(|_| FilesystemError::Timeout)?
    .map_err(|error| FilesystemError::HelperContainerStartFailed(error.to_string()))?;

    // 7. Hello handshake.
    let session = FilesystemSession {
        container_id: response.id,
        container_name,
        source: FilesystemSource::Image {
            image_id: image_id.to_string(),
            platform: platform.clone(),
        },
        root: "/".into(),
        helper_path: HELPER_PATH.into(),
        protocol_version: tuxstack_fs_protocol::FS_HELPER_PROTOCOL_VERSION,
        helper_version: String::new(),
        read_only: false,
        created_at: Utc::now(),
    };

    let helper_version = client::hello(
        client,
        &session,
        timeout,
        cancellation,
    )
    .await
    .map_err(|error| {
        // Handshake failed → invalidate the session.
        let client_clone = client.clone();
        let session_clone = session.clone();
        tokio::spawn(async move {
            let _ = session::invalidate_session(&client_clone, &session_clone).await;
        });
        error
    })?;

    Ok(FilesystemSession {
        helper_version,
        ..session
    })
}

// ---------------------------------------------------------------------------
// Helper binary selection
// ---------------------------------------------------------------------------

#[cfg(helper_x86_64)]
const HELPER_X86_64_BYTES: &[u8] = include_bytes!(env!("IMAGEFS_HELPER_X86_64"));

#[cfg(helper_aarch64)]
const HELPER_AARCH64_BYTES: &[u8] = include_bytes!(env!("IMAGEFS_HELPER_AARCH64"));

fn helper_bytes_for_arch(arch: &str) -> Option<(&'static [u8], &'static str)> {
    match arch {
        "x86_64" | "amd64" => {
            #[cfg(helper_x86_64)]
            {
                return Some((HELPER_X86_64_BYTES, "x86_64"));
            }
            #[cfg(not(helper_x86_64))]
            return None;
        }
        "aarch64" | "arm64" => {
            #[cfg(helper_aarch64)]
            {
                return Some((HELPER_AARCH64_BYTES, "aarch64"));
            }
            #[cfg(not(helper_aarch64))]
            return None;
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Tar archive builder (hand-rolled, no dependencies)
// ---------------------------------------------------------------------------

/// Build a minimal tar archive containing the helper binary at
/// `.tuxstack/tuxstack-fs-helper` with mode 0755.
fn build_helper_archive(helper_binary: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(512 + 512 + helper_binary.len() + 1024 + 512);

    // .tuxstack/ directory entry
    write_ustar_header(&mut out, ".tuxstack/", 0o755, 0, true);

    // .tuxstack/tuxstack-fs-helper file entry
    write_ustar_header(
        &mut out,
        ".tuxstack/tuxstack-fs-helper",
        0o755,
        helper_binary.len() as u64,
        false,
    );
    out.extend_from_slice(helper_binary);
    // Pad to 512-byte boundary.
    let padding = (512 - (helper_binary.len() % 512)) % 512;
    out.extend(std::iter::repeat(0u8).take(padding));

    // End-of-archive: two 512-byte zero blocks.
    out.extend(std::iter::repeat(0u8).take(1024));
    out
}

/// Write a 512-byte ustar tar header.
fn write_ustar_header(out: &mut Vec<u8>, name: &str, mode: u32, size: u64, is_dir: bool) {
    let mut header = [0u8; 512];

    // name[0..100]
    let name_bytes = name.as_bytes();
    let name_len = name_bytes.len().min(100);
    header[..name_len].copy_from_slice(&name_bytes[..name_len]);

    // mode[100..108] - octal
    write_octal(&mut header[100..108], mode as u64);

    // uid[108..116] - 0
    write_octal(&mut header[108..116], 0);
    // gid[116..124] - 0
    write_octal(&mut header[116..124], 0);

    // size[124..136] - octal
    write_octal(&mut header[124..136], if is_dir { 0 } else { size });

    // mtime[136..148] - 0
    write_octal(&mut header[136..148], 0);

    // chksum placeholder (spaces)
    header[148..156].copy_from_slice(b"        ");

    // typeflag[156]
    header[156] = if is_dir { b'5' } else { b'0' };

    // linkname[157..257] - zeros
    // magic[257..263]
    header[257..263].copy_from_slice(b"ustar\0");
    // version[263..265]
    header[263..265].copy_from_slice(b"00");

    // Compute checksum.
    let checksum: u32 = header.iter().map(|&b| b as u32).sum();
    write_octal(&mut header[148..156], checksum as u64);

    out.extend_from_slice(&header);
}

/// Write an octal number into a field, terminated with NUL and space-padded.
fn write_octal(field: &mut [u8], value: u64) {
    let octal = format!("{value:0width$o}", width = field.len() - 1);
    let bytes = octal.as_bytes();
    field[..bytes.len()].copy_from_slice(bytes);
    field[bytes.len()] = 0;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_helper_archive_produces_valid_tar_structure() {
        let helper = b"fake-helper-binary-content";
        let tar = build_helper_archive(helper);
        // Should contain: dir header (512) + file header (512) + content + padding + end (1024)
        assert!(tar.len() > 512 + 512 + helper.len());
        // First header name should be .tuxstack/
        assert_eq!(&tar[..10], b".tuxstack/");
        // Second header name should be .tuxstack/tuxstack-fs-helper
        assert_eq!(&tar[512..512 + 29], b".tuxstack/tuxstack-fs-helper\0");
        // End-of-archive zeros.
        let end = &tar[tar.len() - 1024..];
        assert!(end.iter().all(|&b| b == 0));
    }
}
