# TuxStack architecture

## Design goals

- Native Docker management for KDE Plasma desktops.
- One unprivileged user daemon owns Docker; the GUI stays presentation-only.
- Simple, testable architecture with typed IPC and a read-only FUSE namespace.

## Daemon-first design

`tuxstackd` is the sole Docker Engine client. The GUI and `tuxstackctl` talk
to it over an authenticated local Unix socket
(`$XDG_RUNTIME_DIR/tuxstack/control.sock`, owner/mode-checked, typed CBOR
frames). The daemon also owns a persistent read-only FUSE namespace at
`~/TuxStack/docker` exposing containers, images, and volumes:

```
tuxstack / tuxstackctl ─► tuxstack-client ─► tuxstackd ─► Docker Engine
                                                    └─► FUSE (~/TuxStack/docker)
```

Neither the GUI nor the CLI imports Bollard or owns Docker services, events,
caches, or helper sessions. Old file-browsing backends (helper sessions,
export-tar parsing) were deleted rather than retained in parallel.

## Crate layout

| Crate                  | Role                                                |
| ---------------------- | --------------------------------------------------- |
| `tuxstack-domain`      | Protocol-neutral Docker domain models (serde)       |
| `tuxstack-protocol`    | Typed IPC frames, requests, subscriptions, events   |
| `tuxstack-client`      | GUI/CLI Unix-socket client with typed facade        |
| `tuxstack-vfs`         | Read-only VFS core: paths, inodes, providers, FUSE  |
| `tuxstack-daemon`      | Sole Docker owner: IPC server, providers, FUSE      |
| `tuxstack-docker-core` | Bollard client, services, streams, VFS providers    |
| `tuxstack-cli`         | `tuxstackctl` daemon control client                 |
| `tuxstack`             | Qt 6 / QML / Kirigami application via CXX-Qt        |

## Qt ⇄ Tokio boundary

The GUI runs two cooperating runtimes:

- **Qt event loop** (main thread): QML, QObjects, models, and UI state.
- **Tokio runtime** (multi-threaded): `tuxstack-client` IPC I/O.

The flow for every operation:

```
QML action
  → CXX-Qt invokable (Qt thread)
  → tuxstack-client request over the daemon socket
  → CxxQtThread::queue closure (Qt thread) updates the QObject/model
```

Rules enforced by the design:

- The Qt main thread never blocks on Docker.
- No Tokio runtime is created per-click; one shared runtime exists.
- Background tasks never hold Qt object pointers; they hold a
  `CxxQtThread<T>` handle which is safe to use from any thread.
- UI updates always return to the Qt event loop via queued closures.
- Streams (logs/stats/events/image pull/image export) and terminal sessions
  are daemon subscriptions; resource refresh/detail requests carry a
  `CancellationToken`; app shutdown cancels everything.
- Refresh operations carry a generation id; only the newest generation
  may update the UI, so stale responses cannot overwrite fresh data.

## CXX-Qt boundary

CXX-Qt bridge modules live in `crates/gui/src/bridge/`. Each bridge
declares QObjects with `#[qobject]`, properties with `#[qproperty]`,
invokables with `#[qinvokable]`, signals with `#[qsignal]`, and
`impl cxx_qt::Threading` for cross-thread marshalling. QAbstractListModel
subclasses override `rowCount`/`data`/`roleNames` and use
`begin/endResetModel`.

Bridges are thin: they delegate state transitions to pure Rust page-state
machines. Shared/container state lives in `crates/gui/src/app_state.rs`;
Docker Images state and view mapping live in `controllers/images.rs` and
`models/image_model.rs`; Docker Volumes use `controllers/volumes.rs` and
`models/volume_model.rs`. These modules are unit tested without Qt.

## Bollard data mapping

`tuxstack-docker-core` never exposes Bollard types outside the crate.
`crates/docker-core/src/mapping/` converts every Docker DTO to a domain
model (e.g. `bollard::models::ContainerSummary` →
`ContainerSummary`). The mapping layer is pure functions with unit
tests, so API drift in Bollard is caught by the test suite.

## Stream lifecycle

- `watch_logs` / `watch_stats` / `watch_events` return boxed streams
  that end when cancelled or when the engine disconnects (final error).
- Cancellation is cooperative via `tokio_util::sync::CancellationToken`.
- Logs are capped at `ui.log_line_limit` (default 5000) lines; pausing
  the view only pauses UI consumption, it does not accumulate memory.
- Stats polling uses the configured interval and stops when the page
  closes.
- Image pull maps Docker's real layer status/current/total stream without
  inventing progress. Image export forwards TAR bytes without buffering the
  complete image.
- GUI image export writes a sibling temporary file asynchronously, removes it
  on cancellation/failure, and atomically renames it after flush/sync.
- Volume export and clone use restricted temporary helper containers with the
  source mounted read-only. Cancellation and application shutdown trigger task,
  temporary-file, and helper-container cleanup.

## Error propagation

`DaemonError`/`ProtocolError` (tuxstack-client) distinguish daemon, Docker,
and FUSE failures: service offline, socket permission, Docker unavailable,
timeout, per-resource not-found, conflict, invalid response. The GUI converts
them to `AppError` and only ever displays safe, concise user-facing text;
full details go to debug logs.

## Configuration

XDG config at `~/.config/tuxstack/config.toml` (see README). The daemon
resolves Docker settings through `tuxstack-docker-core::config`. On parse
failure: report the error, use safe defaults, never overwrite.
