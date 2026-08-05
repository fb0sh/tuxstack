# Roadmap

Current state: **alpha** — a native Linux desktop application for Docker,
with Incus planned as a separate backend. `tuxstackd` is the sole Docker
Engine client (Bollard) and serves typed IPC over an authenticated local Unix
socket; the GUI depends only on `tuxstack-client` and browses container/image/
volume files through a persistent read-only FUSE namespace at `~/TuxStack/docker`.

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
- actor-aware Docker Events refresh and tool invalidation.

### Unified read-only Files browsing (FUSE)

Container, Image, and Volume Files pages all browse one local FUSE namespace
maintained by `tuxstackd`; the GUI performs local directory I/O over the mount
and never starts helper containers, parses Docker export tars, or owns a
filesystem backend.

- Container rootfs is a labeled point-in-time Snapshot (10-second export-index
  cache); named volumes are Live through one shared volume provider; safe local
  bind mounts are Live with a helper-bind fallback; tmpfs and runtime mounts
  use operation-time Docker Container Archive reads.
- Image rootfs is an immutable index built from a never-started inspection
  container; the whole images subtree is read-only.
- tmpfs/runtime paths that the Docker Engine Archive API cannot expose return
  an accurate unsupported error (`EOPNOTSUPP`) instead of leaking exported
  shadow content.
- Preview, Save As, properties, and “Open in File Manager” operate on local
  FUSE paths; saves use a unique sibling `.part` and atomic rename.

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
