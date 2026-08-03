# tuxstack-docker-core

The shared Docker core library. GUI and CLI depend on it directly.

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
│   ├── volume.rs     VolumeSummary
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
│   ├── volumes.rs     list/remove
│   ├── system.rs      ping/info/overview
│   └── compose.rs     planned placeholder (returns an explicit error)
├── mapping/      Bollard DTO → domain model (pure, tested)
│   ├── containers.rs
│   ├── images.rs
│   ├── networks.rs
│   ├── volumes.rs
│   ├── stats.rs       CPU/memory math
│   └── system.rs
└── streams/      event stream service
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

`DockerServices { system, containers, images, networks, volumes, compose }`
all share one `Arc<DockerClient>` and are cheap to clone. There is no
generic backend trait — Docker is modeled directly (Incus will get its
own crate later).

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
