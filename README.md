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
| Image list              | Implemented   |
| Network list            | Implemented   |
| Volume list             | Implemented   |
| Compose                 | Planned       |
| Terminal                | Planned       |
| Files                   | Planned       |
| Incus                   | Future consideration |

What works today:

- **GUI** (`tuxstack-gui`): KDE Plasma style (Breeze, system icons,
  system fonts and colors). Pages for Overview, Containers (search,
  state filter, start/stop/restart/remove, details, logs, stats,
  inspect), Images, Networks, Volumes, plus an honest "planned" Compose
  page. Live log following with search and a capped line buffer; live
  stats with a CPU sparkline.
- **CLI** (`tuxstack`): `info`, `ps`, `inspect`, `logs`, `start`,
  `stop`, `restart`, `pause`, `unpause`, `rm`, `images`, `networks`,
  `volumes`, with `--json` output and documented exit codes.

What is deliberately **not** included yet: Compose projects, container
terminal, file browser, image pull/build/tag/push, registry login,
remote engines, Incus, Podman. No mock data is used for any of these.

## Screenshots

Screenshots will be added once the UI stabilizes.

## Architecture

```
┌─────────────────────────────────────┐
│            tuxstack-gui             │
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

There is **no daemon** and **no REST/JSON-RPC layer**. The GUI and the
CLI both link directly against `tuxstack-docker-core`, which talks to
the Docker Engine through [Bollard](https://docs.rs/bollard).

## Technology stack

- Rust (edition 2024)
- Qt 6 / Qt Quick / QML
- Kirigami (KDE Frameworks 6)
- CXX-Qt for the Rust ⇄ Qt bridge
- Bollard (Docker API client), Tokio (async runtime)
- Clap (CLI), Serde (serialization), thiserror, tracing

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
cargo run -p tuxstack-gui
```

The GUI connects to the local Docker socket (or `DOCKER_HOST`) on
startup. If Docker is unavailable or permissions are missing, the
Overview page explains the problem and offers a retry button.

## Using the CLI

```bash
cargo run -p tuxstack-cli -- info
cargo run -p tuxstack-cli -- ps --all
cargo run -p tuxstack-cli -- inspect <container>
cargo run -p tuxstack-cli -- logs <container> --tail 100
cargo run -p tuxstack-cli -- start <container...>
cargo run -p tuxstack-cli -- stop <container...>
cargo run -p tuxstack-cli -- restart <container...>
cargo run -p tuxstack-cli -- rm --force <container...>
cargo run -p tuxstack-cli -- images
cargo run -p tuxstack-cli -- networks
cargo run -p tuxstack-cli -- volumes
```

Global options: `--host <host>` (e.g. `unix:///var/run/docker.sock`,
`tcp://127.0.0.1:2375`), `--timeout <seconds>`, `--json`, `--debug`.

Exit codes:

| Code | Meaning                |
| ---- | ---------------------- |
| 0    | Success                |
| 1    | General error          |
| 2    | Argument error         |
| 3    | Docker unavailable     |
| 4    | Permission denied      |
| 5    | Resource not found     |
| 6    | Operation conflict     |
| 7    | Operation timeout      |

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
cargo test -p tuxstack-cli --test cli -- --ignored --nocapture
```

Test containers use the unique prefix `tuxstack-test-<uuid>` and are
removed even on failure.

## Security notes

- Docker socket access = host control. Be careful with group
  membership and never expose the socket over a network.
- TuxStack logs container/short IDs, operation types and error kinds.
  It never logs secrets, tokens, environment variables, registry
  passwords, or full container log contents.
- The GUI only shows safe, concise user-facing error messages; the
  full error chain goes to debug logs.

## Roadmap

See [docs/roadmap.md](docs/roadmap.md) for the planned direction:
Compose, Terminal, Files, image pull/build, registry login, Docker
contexts, remote engines, and a future Incus integration.

## Documentation

- [Architecture](docs/architecture.md)
- [docker-core](docs/docker-core.md)
- [GUI](docs/gui.md)
- [Development setup](docs/development.md)
- [Roadmap](docs/roadmap.md)

## License

MIT
