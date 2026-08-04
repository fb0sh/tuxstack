# TuxStack

TuxStack is a native Docker management application for Linux desktops,
designed for KDE Plasma and built with Rust, Qt, Kirigami, and Bollard.

**Status: alpha.** The project is under active development; features are
still being added and the API may change.

## Features

| Feature                 | Status        |
| ----------------------- | ------------- |
| Container list          | Implemented   |
| Container inspect       | Implemented   |
| Start/stop/restart      | Implemented   |
| Container logs          | Implemented   |
| Container stats         | Implemented   |
| Image management        | Implemented   |
| Network list            | Implemented   |
| Volume list             | Implemented   |
| Compose                 | Planned       |
| Terminal                | Planned       |
| Files                   | Planned       |
| Incus                   | Future consideration |

What works today:

- **Application** (`tuxstack`): KDE Plasma style (Breeze, system icons,
  system fonts and colors). Pages for Overview, Containers (search,
  state filter, start/stop/restart/remove, details, logs, stats,
  inspect), Docker Images (usage grouping, search, sorting, typed details,
  remove, pull progress, and streaming export), Networks, Volumes, plus
  honest placeholders for later phases. Live log following uses a capped
  line buffer; image operations never use mock progress or mock data.

TuxStack is a GUI-only application. It does not install or maintain a
separate command-line frontend.

What is deliberately **not** included yet: Compose projects, container
terminal, file browser, image build/tag/push/prune, persistent registry
accounts, remote-engine UI, Incus, and Podman. No mock data is used for
any of these.

## Screenshots

Screenshots will be added once the UI stabilizes.

## Architecture

```
┌─────────────────────────────────────┐
│              tuxstack               │
│  QML + Kirigami                     │
│  CXX-Qt QObject / Qt Models         │
│  GUI Controllers / App State        │
└───────┬─────────────────────────────┘
        │ Rust crate API
        ▼
┌─────────────────────────────────────┐
│          tuxstack-docker-core       │
│  Application services               │
│  Docker models / operations         │
│  Stats/logs/event streams           │
│  Bollard type mapping               │
└───────┬─────────────────────────────┘
        │ Bollard
        ▼
┌─────────────────────────────────────┐
│           Docker Engine             │
│ /var/run/docker.sock or DOCKER_HOST │
└─────────────────────────────────────┘
```

There is **no daemon**, CLI frontend, or REST/JSON-RPC layer. The GUI
links directly against `tuxstack-docker-core`, which talks to the Docker
Engine through [Bollard](https://docs.rs/bollard).

## Technology stack

- Rust (edition 2024)
- Qt 6 / Qt Quick / QML
- Kirigami (KDE Frameworks 6)
- CXX-Qt for the Rust ⇄ Qt bridge
- Bollard (Docker API client), Tokio (async runtime)
- Serde (serialization), thiserror, tracing

## System requirements

- Linux (KDE Plasma preferred; Wayland first, X11 compatible)
- Rust 1.85+ (MSRV) and Cargo
- Qt 6 (Core, QML, Quick, QuickControls2, QuickLayouts) with C++ compiler
- Kirigami (KF6) and `kirigami-addons` QML modules
- A running Docker Engine with an accessible socket

### Docker permissions

- The local default Docker socket is usually `/var/run/docker.sock`.
- Your user needs permission to access it (typically membership in the
  `docker` group, applied after logout/login).
- Docker socket access is equivalent to high-privilege control of the
  host. Manage `docker` group membership carefully.
- TuxStack never runs `sudo`, never changes your groups, and never asks
  for a root password. On permission errors it explains what to do.

### Dependency notes by distribution

See [docs/development.md](docs/development.md) for verified package
names on Fedora, Arch Linux, and Ubuntu.

## Building

```bash
cargo build --workspace
```

## Running the GUI

```bash
cargo run
```

The workspace defaults to the `tuxstack` application package. The explicit
package form is `cargo run -p tuxstack`.

The GUI connects to the local Docker socket (or `DOCKER_HOST`) on
startup. If Docker is unavailable or permissions are missing, the
Overview page explains the problem and offers a retry button.

## Configuration

Configuration lives at `$XDG_CONFIG_HOME/tuxstack/config.toml`
(default `~/.config/tuxstack/config.toml`):

```toml
[docker]
host = ""
connect_timeout_seconds = 5
operation_timeout_seconds = 30

[ui]
auto_refresh_seconds = 5
stats_refresh_seconds = 2
log_line_limit = 5000
confirm_remove = true

[logging]
level = "info"
```

The theme always follows the system (Breeze Light / Breeze Dark). If
the config file is unreadable, safe defaults are used and the problem
is reported; the file is never overwritten.

## Testing

```bash
cargo test --workspace
```

Unit tests cover docker-core mapping, error classification, stats
math, config parsing, and the GUI page-state machine (loading→ready,
loading→error, stale refresh rejection, busy/operation states). A
QML smoke test loads the full UI headless.

### Docker integration tests

Real-Docker tests are gated behind `#[ignore]` and need an accessible
Engine:

```bash
cargo test -p tuxstack-docker-core --test docker -- --ignored --nocapture
cargo test -p tuxstack-docker-core --test containers -- --ignored --nocapture
cargo test -p tuxstack-docker-core --test images -- --ignored --nocapture
cargo test -p tuxstack-docker-core --test networks -- --ignored --nocapture
```

Test containers use the unique prefix `tuxstack-test-<uuid>` and are
removed even on failure.

## Security notes

- Docker socket access = host control. Be careful with group
  membership and never expose the socket over a network.
- TuxStack logs container/short IDs, operation types and error kinds.
  It never logs secrets, tokens, image environment-variable values,
  sensitive labels, registry passwords, or full container log contents.
- Image environment variables are stored in the image metadata and may
  themselves contain secrets. The details page displays the real metadata
  on request, but TuxStack never writes those values to its logs.
- The GUI only shows safe, concise user-facing error messages; the
  full error chain goes to debug logs.

## Roadmap

See [docs/roadmap.md](docs/roadmap.md) for the planned direction:
Compose, Terminal, Files, image build/tag/push/prune, persistent registry
accounts, Docker contexts, remote engines, and a future Incus integration.

## Documentation

- [Architecture](docs/architecture.md)
- [docker-core](docs/docker-core.md)
- [GUI](docs/gui.md)
- [Development setup](docs/development.md)
- [Roadmap](docs/roadmap.md)

## License

MIT
