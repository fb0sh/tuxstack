# Roadmap

Current state: **alpha** — a native Linux desktop application for Docker,
with Incus planned as a separate backend. The GUI calls the internal Rust
core directly through CXX-Qt; Docker operations use Bollard rather than the
Docker CLI.

## Implemented

### Docker Containers

The Containers page now provides a single real implementation for:

- label-only Compose project grouping and bounded, per-container group actions;
- structured container and group details;
- start, stop, restart, pause, resume, kill, rename, remove, and create;
- live per-container and aggregate group statistics;
- bounded, cancellable individual and merged group logs;
- a real interactive Docker TTY terminal interpreted by Rust `vt100`;
- merged-rootfs filesystem snapshots with mount overlays, pagination, preview,
  and streaming Save As;
- actor-aware Docker Events refresh and tool invalidation;
- persistent container-summary cache hydration followed by live refresh.

Container Files intentionally uses point-in-time snapshot semantics. Mounted
paths are inspect-derived navigation overlays; exported shadow content is not
presented as live volume or bind-mount data.

### Image operations

Pull, remove, inspect, streaming export, and read-only image file browsing are
implemented. Image browsing injects the bundled static Rust filesystem helper
into a hardened temporary container, so scratch and distroless images are not
dependent on an in-image shell. Unsupported platforms and helper failures are
reported explicitly. Build, tag, push, and prune remain planned.

### Volume operations

List, inspect, usage association, create, remove, prune, export, clone, and
read-only file browsing/preview/download are implemented against the real
Docker Engine. Volume browsing uses the same bundled static filesystem helper
through an internal scratch helper image; it does not pull a network helper
image.

Editing volume files, uploads, scheduled backups/snapshots, encryption, and
volume-plugin administration remain future work.

## Planned

### Dedicated Compose projects

The Containers page already groups official Compose-labelled containers and
supports real group lifecycle actions. A separate Compose project page with
project discovery, `up`/`down`, configuration views, and project-scoped create
workflows remains planned. It will use Docker APIs and typed Compose models,
never shell-command splicing.

### Image build and registry workflows

Build, tag, push, prune, registry login, and persistent credential handling.
Secrets must remain redacted from logs, debug output, caches, and IPC errors.

### Docker contexts and remote engines

List and switch Docker contexts, preserve endpoint/daemon cache isolation, and
complete remote `tcp://`/`ssh://` workflows. Connection plumbing already
exists, but context lifecycle and remote-path UX still require dedicated work.

## Future consideration

### Incus

Incus will be added as a **separate** crate, `crates/incus-core/`, alongside
docker-core:

```text
gui
├── docker-core
└── incus-core
```

A shared `WorkloadBackend`-style abstraction will only be extracted after real
duplication between Docker and Incus code exists. The GUI keeps backend-native
terminology and does not force Docker and Incus into a premature universal
resource model.

### Podman

Not currently planned; revisit if demand appears.

## Explicitly out of scope for now

- generic backend plugin systems;
- universal resource models;
- macOS/Windows desktop support.
