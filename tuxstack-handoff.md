# TuxStack Handoff — Docker Volumes complete, next: roadmap continuation

**Repo:** `/home/fb0sh/Projects/tuxstack`
**HEAD:** `f4b1ad5` (feat: implement Docker volume management) — working tree **clean**
**Branch:** `main` ahead of `origin/main` by 5 — **never pushed** (user has never asked to push)

This doc is the working summary for continuing the conversation. The authoritative requirements live in the committed spec files; read those before starting any new phase.

## Current state (verified at handoff)

- All work in this conversation's most recent task — **Docker Volumes** (spec: `DockerVolumeUI.md`, committed in `f4b1ad5`) — is complete, validated, and committed. 34 files, +10,869/−196.
- Prior phases also complete and committed in `67200c3` ("feat: deliver GUI-only Docker management app", 106 files): localization (`I18n.qml` + ki18n singleton), QML smoke diagnostics, sidebar/UI skeleton, Docker **Images** (spec `DockerImageUI.md`), Docker **Networks** (spec `DockerNetworkUI.md`), CLI removal, `tuxstack-gui`→`tuxstack` rename, release-size optimization.
- Validation numbers at handoff: **81 docker-core tests + 80 GUI tests pass**; `cargo clippy --workspace --all-targets --all-features -- -D warnings` passes; QML smoke (`smoke_test::all_qml_components_load_without_errors`) passes; real-Docker integration tests are `#[ignore]`d and pass when run explicitly (see Validation section).

## Architecture (do not deviate)

Qt 6 / QML / Kirigami → **CXX-Qt 0.9.1** → `tuxstack-docker-core` (internal library) → **Bollard 0.21.0** → Docker Engine (API v1.47). GUI-only product; no CLI, daemon, REST, JSON-RPC, or shell-out paths (user decision, documented in `docs/architecture.md`). No mock data anywhere.

### Hard-won conventions (from earlier phases; do not regress)

- **CXX-Qt multiword `qproperty`/`qinvokable` export as snake_case unless explicitly given `cxx_name`** — QML reads camelCase. Every public multiword member must export its camelCase name explicitly. A regression here silently breaks Docker connection-state delivery to models.
- **Model roles start at `Qt::UserRole + 1` (257)** — the old resource roles began at 0 and collided with Qt standard roles; cxx-qt 0.9.1 `qenum` cannot assign explicit discriminants.
- **CXX-Qt 0.9.1 `QVariant` does not auto-convert Rust `Vec`/maps/`Option`/domain structs/`serde_json::Value`** — bridges must build supported `QStringList`/`QVariantList`/`QVariantMap`/`QUrl` explicitly. File-export destinations cross the bridge as `QUrl` (do not strip `file://` in JS).
- **QML i18n**: bare `i18nd(...)` is undefined; use the registered singleton `I18n.i18nd("tuxstack", ...)` (see `crates/gui/qml/I18n.qml`). No bare calls remain.
- **App shell wiring**: `Main.qml` owns `AppController` + one long-lived model per resource (ImageListModel, NetworkListModel, VolumeListModel). Models initialize before Docker connects; Main.qml must **replay AppController's stored Docker status** to each model on page entry and forward connection-state changes (`setConnectionState`), or real data never loads. Each model also needs shutdown wiring, retry, passive notifications, and container navigation.
- **Lazy-load trap**: `Main.qml` must not assign a `RowLayout` to the read-only `Kirigami.ApplicationWindow.contentItem`. Pages eager-instantiating StackLayout children must gate init on page activity.
- **Permanent three-column detail layout**: list panel + separator + detail panel are permanently instantiated; only the detail *content* hides when nothing is selected. Never reintroduce a conditional `Loader` (a static smoke regression test forbids it).
- **Independent states**: per-resource list state (Loading/Ready/Empty/Error) is separate from detail state (None/Loading/Ready/Error); detail failures stay in the right pane with Retry, never disturbing list state.
- **Generation guards + cancellation tokens** for every async operation (list, detail, create, remove, prune, export, clone) so stale results can't overwrite newer state; cancel on disconnect/shutdown/drop.
- **Sensitive data**: registry credentials (password/identity_token/registry_token) are redacted as `<redacted>` in Debug; never log volume/container/image environment values, label values, or driver-option values. Lifecycle logs are generic (`Docker returned N volumes`, `Volume detail loaded`). A pre-commit scan pattern for `credential-assignment` will false-positive on the redaction test fixtures (`models/volume.rs`, `services/volumes.rs`) — verify before flagging.
- **KDE-native styling**: theme-derived colors only, no fixed hex, no cards/shadows; Freedesktop icon names via `Kirigami.Icon` (e.g. `drive-harddisk`); Breeze-style ItemDelegate interaction rules for list rows.

## Recent task details — Docker Volumes (`f4b1ad5`)

- **Core** (`crates/docker-core/src/`): typed `VolumeSummary/Detail/Usage/ContainerReference/Create/Remove/Prune/Export/Clone` domains; `models/volume.rs`, `mapping/volumes.rs`, `services/volumes.rs` (list is one request, no N+1); volume-specific error classification in `error.rs`; `DockerClient::is_local()` in `client.rs` for export restriction.
- **Usage association**: from raw container `Mounts` `Name` fields across ALL container states (bind/tmpfs excluded); bounded-concurrency inspect fallback (8) when summary mounts missing. Stopped containers still count as In Use.
- **Sizes**: `/system/df` preferred (degrades safely under Bollard 1.53 schema vs API 1.47 incompatibility), else inspect `UsageData`; missing/negative → `None` (never fake `0 B`); totals sum only known sizes, with known/unknown counts shown.
- **GUI**: `controllers/volumes.rs` (pure state), `models/volume_model.rs` (view model), `bridge/volume_bridge.rs` (new CXX-Qt facade, replaces legacy model in `resource_bridges.rs`); 11 new QML files (list/detail/used-by/key-value-editor + 5 dialogs); 10 sorts, local 200 ms search, In Use/Unused grouping; keyboard accessibility.
- **Export/Clone**: constrained `alpine:3.20` helper containers — unique `tuxstack-helper-<uuid>` name, read-only `/source`, no network, no Docker socket, non-privileged, dropped capabilities, resource limits, direct argv (no shell), unconditional cleanup. Export: unique staging dir bind + atomic rename, tar/tar.gz only, **local engine only** (`is_local()`). Clone: `cp -a /source/. /target/` (dotfiles), rejects pre-existing target, removes only incomplete targets it created.
- Full final report (files, APIs, algorithms, validation, limitations, commit hash) was delivered in the conversation immediately before this handoff; see also `docs/docker-core.md` / `docs/gui.md` updates committed in `f4b1ad5`.

## Known limitations (documented in report; keep accurate)

- `.tar.zst` → typed `UnsupportedVolumeCompression` (only `.tar`/`.tar.gz`).
- Helper image `alpine:3.20` must already exist locally (no implicit pull).
- Export requires a local Docker Engine (host bind-mount path); clone has no such restriction.
- Bollard 0.21 models plugin `Status` as `Option<Vec<String>>`; keys survive, values lost — never fabricate a status map.
- Incompatible `/system/df` schema → sizes honestly reported Unknown.
- `cp -a` preservation depends on driver/filesystem/user-namespace.
- Cross-module limitation: Volume "Used By" can switch to the placeholder Containers page, but true select-and-open needs the Containers module (out of scope of every spec so far).

## Validation workflow (proven, reuse)

```bash
cargo fmt --all -- --check
cargo check --workspace
cargo test --workspace                                   # 81 core + 80 GUI
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo build -p tuxstack
cargo test -p tuxstack -- --exact smoke_test::all_qml_components_load_without_errors
git diff --check
```

- **Runtime smoke** (offscreen, no display): `QT_QPA_PLATFORM=offscreen QT_QUICK_BACKEND=software QT_QUICK_CONTROLS_STYLE=org.kde.desktop RUST_LOG=tuxstack=debug target/debug/tuxstack` for ~10 s; success = stays alive, `root_objects=1`, expected lifecycle logs, no ReferenceError/TypeError/panic. Run matrix: default, `KDE_COLOR_SCHEME=BreezeDark`, `QT_SCALE_FACTOR=2`, and `DOCKER_HOST=unix:///tmp/tuxstack-missing.sock` (unavailable path).
- **Real-Docker integration**: environment has working (read-only) Docker access against server **29.7.1** via default unix socket — e.g. `cargo test -p tuxstack-docker-core --test volumes -- --ignored --nocapture` passes (create/usage/export/clone/cleanup verified). All `#[ignore]`d suites runnable here.
- **QML contract audit**: a script cross-checking every `volumesModel.<member>` usage in QML against the bridge declaration passed (73/73); regenerate after any bridge change.
- Rebuild `target/debug/tuxstack` explicitly before runtime checks — `cargo test` does not always rebuild the binary, and stale binaries caused false "nothing happens" diagnoses twice.
- Pre-existing benign warnings only: Qt/GCC `QChar` SFINAE header warnings, `gold linker is deprecated`.

## Inconclusive artifact (do not re-run blindly)

Two review subagents (GUI + docker-core) were launched mid-task and produced **no final findings** (transcripts exist under `~/.pi/agent/sessions/.../tasks/2026-08-04T03-11-*.jsonl`). They hit max_turns without a summary. The areas they were told to check (QML contract vs generated declarations, state separation, dialog flows, cancellation) were subsequently covered by my own automated cross-checks, the full validation chain, and real-Docker runtime runs — all passing.

## Where to continue next (likely)

Per `docs/roadmap.md`, Docker Volumes closes the resource-management streak; remaining roadmap items include: **Containers page** (currently a placeholder; Images/Networks/Volumes all emit `navigateToContainer(id)` and Main.qml stores `pendingContainerId` for it — this is the natural next phase), Activity Monitor, Commands, Devices, then packaging/release. **Ask the user which phase to start**; each phase so far was driven by a committed spec (UI.md, DockerImageUI.md, DockerNetworkUI.md, DockerVolumeUI.md) that defines scope, forbidden work, and validation.

## Suggested skills

- **to-spec** / **to-tickets** — when the user describes the next phase (e.g. Containers): convert requirements into the structured spec/tickets style used by the existing committed specs before implementing.
- **code-review** — run a read-only review pass over new GUI/bridge/core code against the phase spec before the full validation chain; a fresh agent should reproduce the QML-contract cross-check script.
- **tdd** — follow the established test-first pattern (pure state/controller tests, mapping tests, then QML smoke) used in every phase.
- **diagnosing-bugs** — if a new phase hits the CXX-Qt snake_case bridge regression or connection-replay blocker, this is the same failure family as before; check generated headers (`target/debug/build/tuxstack-*/out/cxxqtgen/src/bridge/*.cxx.h`) first.
- **implement** — for the bulk QML + bridge implementation once the spec exists.
- **resolving-merge-conflicts** — only if origin/main has moved; otherwise the tree is clean at `f4b1ad5` and no conflict work is pending.
