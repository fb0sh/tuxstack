# TuxStack architecture

## Design goals

- Native Docker management for KDE Plasma desktops.
- One shared Rust core library used by both the GUI and the CLI.
- Simple, testable architecture with no hidden daemons.

## No-daemon design

TuxStack has no background daemon, no REST API, no JSON-RPC, and no
extra Unix socket. Both binaries link `tuxstack-docker-core` directly:

```
tuxstack-gui ─┐
              ├─► tuxstack-docker-core ─► Bollard ─► Docker Engine
tuxstack-cli ─┘
```

This keeps the process model trivial (one GUI process, one CLI process),
removes a whole class of IPC bugs, and makes the core easy to unit test.

## Crate layout

| Crate                | Role                                              |
| -------------------- | ------------------------------------------------- |
| `tuxstack-docker-core` | Docker connection, domain models, services, streams |
| `tuxstack-gui`       | Qt 6 / QML / Kirigami UI via CXX-Qt               |
| `tuxstack-cli`       | Clap-based command line tool                      |

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
- Streams (logs/stats/events) own a `CancellationToken`; pages cancel
  them on close, and app shutdown cancels everything.
- Refresh operations carry a generation id; only the newest generation
  may update the UI, so stale responses cannot overwrite fresh data.

## CXX-Qt boundary

CXX-Qt bridge modules live in `crates/gui/src/bridge/`. Each bridge
declares QObjects with `#[qobject]`, properties with `#[qproperty]`,
invokables with `#[qinvokable]`, signals with `#[qsignal]`, and
`impl cxx_qt::Threading` for cross-thread marshalling. QAbstractListModel
subclasses override `rowCount`/`data`/`roleNames` and use
`begin/endResetModel`.

Bridges are thin: they delegate all state transitions to pure Rust
page-state machines in `crates/gui/src/app_state.rs`, which are unit
tested without Qt.

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

## Error propagation

`DockerError` (docker-core) classifies failures precisely: socket
missing, permission denied, engine unavailable, connection/operation
timeout, per-resource not-found, conflict, invalid response, API error.
The GUI converts it to `AppError` and only ever displays safe,
concise user-facing text; full details go to debug logs.

## Configuration

XDG config at `~/.config/tuxstack/config.toml` (see README). The GUI
and CLI both resolve it through `tuxstack-docker-core::config`. On
parse failure: report the error, use safe defaults, never overwrite.
