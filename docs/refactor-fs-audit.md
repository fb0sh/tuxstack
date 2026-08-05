# Daemon + FUSE migration audit

Date: 2026-08-05
Baseline: `v0.3.0` / `3ceb7b2`
Specification: `docs/RefactorFS.md`

## User-owned working-tree changes

The migration must not restore, overwrite, or include these pre-existing changes in migration commits:

```text
D  PLAN.md
D  fix1.md
D  tuxstack-handoff.md
?? docs/PLAN.md
?? docs/RefactorFS.md
```

## 1. Current workspace

| Package | Current responsibility |
|---|---|
| `tuxstack` (`crates/gui`) | Qt/QML/CXX-Qt GUI, Tokio runtime, Docker connection and event ownership |
| `tuxstack-docker-core` (`crates/docker-core`) | Bollard client, Docker services, caches, events, and all current file backends |
| `tuxstack-fs-helper` | Static helper injected into image/volume helper containers |
| `tuxstack-fs-protocol` | JSON-lines helper protocol and path tokens |

Current dependency direction:

```text
tuxstack GUI -> tuxstack-docker-core -> tuxstack-fs-protocol
tuxstack-fs-helper -> tuxstack-fs-protocol
```

`docker-core/build.rs` also builds and embeds the musl helper binary.

There is no daemon, daemon client, CLI, VFS, FUSE, IPC, or systemd-service crate.

## 2. GUI Docker ownership

The GUI does not import Bollard directly, but it owns the Bollard-backed client and service facade, which violates the target daemon-only ownership rule.

- `crates/gui/src/bridge/app_bridge.rs`: creates `DockerClient`/`DockerServices`, pings Docker, starts Docker Events and helper cleanup.
- `crates/gui/src/app_state.rs`: globally stores services, persistent cache, metadata/usage caches, helper pools, and event monitor.
- Direct Docker-service callers include `containers_bridge.rs`, `container_live_bridge.rs`, `container_terminal_bridge.rs`, `container_tools_bridge.rs`, `image_bridge.rs`, `image_file_bridge.rs`, `volume_bridge.rs`, `volume_file_bridge.rs`, and `network_bridge.rs`.

The final migration therefore covers every Docker page, not only Files.

## 3. CLI

No shipped CLI exists. Direct Bollard use under `tests/integration` is test-fixture code, not a product client.

## 4. Container Files today

Current chain:

```text
ContainerFileListModel
-> DockerServices.container_files
-> inspect mounts
-> Docker export stream
-> custom ustar/PAX/GNU parser
-> in-memory metadata snapshot owned by the GUI
```

Reusable strengths in `crates/docker-core/src/services/container_files.rs`:

- streaming archive parsing;
- traversal and malformed/truncated archive validation;
- resource limits;
- mount-parent synthesis and shadowed-rootfs entries;
- Archive API preview/download;
- timeout and cancellation.

Missing target behavior:

- mount destinations are actions, not seamless provider transitions;
- no longest-component-prefix `ContainerPathRouter`;
- named volumes and binds are not browsed in the same tree;
- no FUSE inode/handle model or random-read spool;
- paths are UTF-8 `String`, not raw filename bytes;
- no host-safe absolute-symlink rewriting.

## 5. Image Files today

Current provider (`services/filesystem/image_provider.rs`) creates a container from the image, injects a static helper, starts the helper as an overridden entrypoint, and uses Docker exec.

It is hardened, but it is not an immutable image-ID index. It starts a container, requires a writable layer, and can expose runtime-generated files. It must be replaced by created-never-started inspection-container export, persistent metadata index, short-lived content containers, and image-ID identity.

## 6. Volume Files today

The volume backend builds/loads a scratch helper image, mounts a named volume read-only at `/mnt/data`, and uses the static Rust helper for list/stat/readlink/preview/hash/download.

Security already includes network none, read-only helper rootfs, cap-drop all, no-new-privileges, and CPU/memory/PID/FD limits. This is the provider to extract into daemon ownership and share between top-level Volumes and container mount routes.

The GUI currently owns separate image and volume pools. Pool defaults are three sessions, 120-second idle TTL, and five-second directory TTL. Directory-cache APIs and expiry eviction are not fully wired in production.

## 7. Reusable Files UI

`VolumeFilesView.qml` contains the best existing table, toolbar, breadcrumb, search, sort, hidden-file toggle, and keyboard behavior. `ImageFilesView.qml` already wraps it. Container Files duplicates a separate table and state model.

The final GUI should retain presentation concepts but replace all three backends with one `LocalFuseFilesController`. Existing temp-file plus `xdg-open` code must be replaced with Qt desktop URL handling.

## 8. Docker Events

`docker-core/src/streams/events.rs` uses Bollard Events. `cache/events.rs` adds resource classification, 250 ms debounce, bounded actor/action batches, and reconnect backoff. Ownership currently lives in the GUI.

Move this monitor to `tuxstackd`, preserve actor/action information, and add repository, namespace, provider-cache, and FUSE notifier invalidation.

## 9. Cache and repository state

Reusable cache code:

- `cache/coordinator.rs`: generic single-flight and TTL cache;
- `cache/persistent.rs`: endpoint-keyed SQLite WAL cache;
- `cache/image_metadata.rs`;
- `cache/volume_usage.rs`;
- `cache/session_pool.rs`;
- `cache/events.rs`.

There is no explicit repository layer. GUI bridges currently orchestrate cache hydration/writeback. Known defects include an invalid generic persistent point-read key, no production user of `RequestCoordinator`, and incomplete helper-pool eviction cleanup.

## 10. Final deletion/replacement scope

After equivalent daemon/FUSE behavior is operational, delete the old GUI Files controllers/bridges/models and their old QObject registrations:

```text
controllers/container_files.rs
controllers/image_files.rs
controllers/volume_files.rs
bridge/container_tools_bridge.rs
bridge/image_file_bridge.rs
bridge/volume_file_bridge.rs
models/volume_file_model.rs
```

Replace old Files QML with a unified local-FUSE view/controller, removing container/image/volume-specific Docker-backed Files models.

Extract then remove old Docker-core APIs at:

```text
services/container_files.rs
services/filesystem/*
cache/session_pool.rs
```

The static helper and helper protocol remain reusable daemon-internal infrastructure for named-volume and fallback bind providers.

Finally remove GUI Docker service/event/cache/helper ownership and all GUI `get_services()` calls. No feature-flagged legacy backend remains.

## 11. Rust and host baseline

- Project MSRV: Rust 1.85, edition 2024.
- Active toolchain: Rust/Cargo 1.97.1.
- Host tested: Arch Linux, kernel 7.1.5, x86_64.
- Product target: Fedora/KDE Plasma/Wayland, Linux-only.

Rust 1.85 compilation still requires an installed 1.85 toolchain; the current host only has 1.97.1.

## 12. FUSE/system dependencies

Host observations:

```text
/dev/fuse: readable and writable by the current user
fusermount3: 3.18.2 (setuid root)
libfuse3/pkg-config: 3.18.2
findmnt and mountpoint: available
systemd --user: running
XDG_RUNTIME_DIR: /run/user/1000
```

Current source has no Rust FUSE dependency. Fedora runtime requirements are expected to include `fuse3`, `util-linux`, and `systemd`; a libfuse-linked build additionally needs `fuse3-devel` and `pkgconf-pkg-config`. Fedora/SELinux behavior must be tested on Fedora rather than inferred from this Arch host.

## 13. FUSE candidate and risks

Candidate: exact `fuser 0.18.0`, stable synchronous `Filesystem` API, initially with default features disabled and ABI 7.31 enabled.

Reasons:

- stable Linux FUSE API;
- required read-only operations and notifier invalidation are available;
- no experimental async API in the architecture core;
- compatible in principle with the project MSRV, subject to an actual Rust 1.85 build.

Phase-0 blockers to prove with a PoC:

1. unprivileged mount/read/unmount;
2. exact `EROFS` mutation behavior;
3. repeated clean mount/unmount;
4. user-service lifecycle;
5. whether `NoNewPrivileges=yes` prevents setuid `fusermount3`;
6. whether a slow callback blocks unrelated cached reads;
7. startup recovery after forced termination.

## 14. Phase-0 PoC results

The isolated PoC lives under `poc/fuse-readonly/` and does not participate in the root workspace. It uses exact `fuser 0.18.0`, `default-features = false`, and a local `abi-7-31` compatibility marker. `fuser 0.18.0` no longer publishes the historical upstream ABI Cargo features; it negotiates a maximum ABI of 7.40 with the kernel. The PoC implementation deliberately stays within the ABI 7.31 operation surface.

Verified on the current host:

- unit tests: 2 passed;
- strict Clippy: passed;
- static `hello.txt` mount/read/stat: passed;
- root mode 0555 and file mode 0444 with current UID/GID: passed;
- write open and mutation attempts: `EROFS`;
- FUSE mount flags: `ro,nosuid,nodev,noexec,default_permissions`;
- SIGTERM clean unmount: passed;
- repeated mount/unmount: passed;
- four FUSE workers with cloned `/dev/fuse` descriptors: a normal read completed in 2 ms while another callback slept for 3 seconds;
- transient systemd user unit with `NoNewPrivileges=no`: mount/read/clean stop passed;
- transient systemd user unit with `NoNewPrivileges=yes`: mount failed with `fusermount3: mount failed: Operation not permitted`;
- SIGKILL leaves a mounted stale FUSE connection; startup recovery using `fusermount3 -u -z`, remount, read, and final clean stop passed.

Consequences for production:

1. `tuxstackd.service` cannot use `NoNewPrivileges=yes` while it relies on the distribution's setuid `fusermount3`; the daemon still runs unprivileged as the session user.
2. Startup must identify and clean only its own stale mount before remounting.
3. Production FUSE must configure a bounded worker count and cloned descriptors; one default synchronous worker is insufficient.
4. Slow provider work still requires timeout, cancellation, and provider semaphores even with multiple FUSE workers.

Rust 1.85 compilation was attempted, but installing the 1.85 toolchain timed out after 30 minutes and left an incomplete toolchain. Current-toolchain compilation and runtime behavior are proven; MSRV compilation remains an explicit release gate rather than being reported as passed.

## Phase-0 decision

The functional PoC gate is passed. Large-scale migration may proceed with the following non-negotiable constraints:

- owner-only, read-only FUSE;
- no `NoNewPrivileges=yes` on the FUSE-owning service;
- bounded multi-worker FUSE dispatch;
- stale-mount recovery at daemon startup;
- Rust 1.85 build verification before release.
