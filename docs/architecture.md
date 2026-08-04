# TuxStack architecture

## Design goals

- Native Docker management for KDE Plasma desktops.
- One GUI process backed by a reusable Rust Docker core library.
- Simple, testable architecture with no CLI frontend or hidden daemons.

## No-daemon design

TuxStack has no background daemon, CLI frontend, REST API, JSON-RPC, or
extra Unix socket. The GUI links `tuxstack-docker-core` directly:

```
tuxstack ─► tuxstack-docker-core ─► Bollard ─► Docker Engine
```

This keeps the process model to one GUI process, removes a whole class of
IPC bugs, and keeps Docker behavior easy to unit test independently of Qt.

## Crate layout

| Crate                  | Role                                                |
| ---------------------- | --------------------------------------------------- |
| `tuxstack-docker-core` | Docker connection, domain models, services, streams |
| `tuxstack`             | Qt 6 / QML / Kirigami application via CXX-Qt        |

## Qt ⇄ Tokio boundary

The GUI runs two cooperating runtimes:

- **Qt event loop** (main thread): QML, QObjects, models, and UI state.
- **Tokio runtime** (multi-threaded): all Docker I/O.

The flow for every operation:

```
QML action
  → CXX-Qt invokable (Qt thread)
  → tokio task (docker-core call)
  → CxxQtThread::queue closure (Qt thread) updates the QObject/model
```

Rules enforced by the design:

- The Qt main thread never blocks on Docker.
- No Tokio runtime is created per-click; one shared runtime exists.
- Background tasks never hold Qt object pointers; they hold a
  `CxxQtThread<T>` handle which is safe to use from any thread.
- UI updates always return to the Qt event loop via queued closures.
- Streams (logs/stats/events/image pull/image export) and outstanding image
  refresh/detail requests own a `CancellationToken`; app shutdown cancels
  everything.
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
`models/image_model.rs`. These modules are unit tested without Qt.

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
- GUI export writes a sibling temporary file asynchronously, removes it on
  cancellation/failure, and atomically renames it after flush/sync.

## Error propagation

`DockerError` (docker-core) classifies failures precisely: socket
missing, permission denied, engine unavailable, connection/operation
timeout, per-resource not-found, conflict, invalid response, API error.
The GUI converts it to `AppError` and only ever displays safe,
concise user-facing text; full details go to debug logs.

## Configuration

XDG config at `~/.config/tuxstack/config.toml` (see README). The GUI
resolves it through `tuxstack-docker-core::config`. On parse failure:
report the error, use safe defaults, never overwrite.
