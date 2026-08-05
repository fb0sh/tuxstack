# tuxstack-docker-core

The internal Docker core library used by `tuxstackd`. Keeping Docker
I/O and domain mapping independent of Qt makes it testable without a product
frontend; the GUI never imports it and only uses `tuxstack-client`.

## Module structure

```
src/
├── client.rs     DockerClient + DockerConfig, connection resolution
├── config.rs     XDG config file loading (TOML)
├── error.rs      DockerError and error classification
├── models/       Domain models (no Bollard types leak out)
│   ├── container.rs  ContainerSummary/Detail, states, ports, logs
│   ├── image.rs      ImageSummary
│   ├── network.rs    NetworkSummary/Detail
│   ├── volume.rs     VolumeSummary/Detail, usage, container references,
│   │                  create/remove/prune/export/clone requests
│   ├── volume_file.rs VolumePath, file entries, preview/download models
│   ├── stats.rs      ContainerStats
│   ├── event.rs      DockerEvent
│   ├── system.rs     DockerSystemInfo, OverviewData
│   ├── options.rs    Operation option structs
│   └── compose.rs    Compose model (planned feature)
├── services/     Per-resource services sharing one client
│   ├── containers.rs  list/inspect/start/stop/restart/pause/unpause/
│   │                  kill/remove/rename/logs/stats + log/stats streams
│   ├── images.rs      list/inspect/remove
│   ├── networks.rs    list/inspect
│   ├── volumes.rs     list/inspect/create/remove/prune/export/clone
│   ├── container_terminal.rs  interactive TTY exec sessions
│   ├── filesystem/    unified injected static Rust helper (volume + bind)
│   ├── system.rs      ping/info/overview
│   └── compose.rs     Compose project actions
├── mapping/      Bollard DTO → domain model (pure, tested)
│   ├── containers.rs
│   ├── images.rs
│   ├── networks.rs
│   ├── volumes.rs
│   ├── stats.rs       CPU/memory math
│   └── system.rs
├── streams/      event/pull/export stream services
└── vfs_providers/ Docker-backed read-only FUSE providers (snapshot,
                   named volume, local/helper bind, archive, image index)
```

## DockerClient

```rust
DockerClient::connect_default() -> Result<Self, DockerError>
DockerClient::connect_with_config(DockerConfig) -> Result<Self, DockerError>
client.ping()               -> Result<(), DockerError>
client.system_info()        -> Result<DockerSystemInfo, DockerError>
```

Connection resolution order:

1. `DockerConfig.host` (accepts `unix://`, `tcp://`, `http://`,
   `https://`, `ssh://`; anything else is `UnsupportedConnection`).
2. `DOCKER_HOST` environment variable.
3. Local default Unix socket `/var/run/docker.sock`.

## Services

`DockerServices { system, containers, images, networks, volumes, compose, container_terminal, filesystem }`
all share one `Arc<DockerClient>` and are cheap to clone. There is no
generic backend trait — Docker is modeled directly (Incus will get its
own crate later).

## Docker volumes

The volume service combines Docker's volume inventory, all-container mount
references, and system disk usage. A stopped/paused/created container still
counts as a volume user. Bind and tmpfs mounts do not. Docker `-1` or missing
usage values map to `None`, never `0` or a wrapped unsigned value.

Volume export and clone are explicit helper-container operations because Docker
has no native volume-export endpoint. Helpers have no Docker socket, privileged
mode, or network access; source volumes are mounted read-only. Export writes a
temporary sibling and renames it after success. Clone refuses an existing
target and can clean up an incomplete target. Both operations support
`CancellationToken` and always attempt helper cleanup.

## Filesystem helper

`services/filesystem/` runs the bundled static Rust helper (`tuxstack-fs-helper`,
JSON-lines protocol from `tuxstack-fs-protocol`) in restricted scratch helper
containers for named-volume and fallback bind providers. It no longer serves
the GUI directly: the GUI browses the daemon's FUSE namespace, and the old
`volume_files/` shell-script service was deleted.

## Timeouts and cancellation

- Every request is wrapped in `tokio::time::timeout` with the
  configured `request_timeout`.
- Streams end cooperatively via `CancellationToken`; they also end on
  container removal or engine disconnect with a final typed error.

## Errors

`DockerError` distinguishes: `SocketNotFound`, `PermissionDenied`,
`EngineUnavailable`, `ConnectionTimeout`, `OperationTimeout`,
`Container/Image/Network/VolumeNotFound`, `Conflict`, `InvalidResponse`,
`Api`, `UnsupportedConnection`, `Internal`.

Bollard HTTP status codes map precisely: 404 → per-resource not-found,
409 → Conflict, 401/403 → PermissionDenied, 408 → OperationTimeout.
Connection errors are inspected for socket/permission/refused/timeout
signatures.
