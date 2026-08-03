# TuxStack GUI

Qt 6 + QML + Kirigami, bridged with CXX-Qt.

## QML page structure

```
Kirigami.ApplicationWindow (Main.qml)
├── AppSidebar          fixed left navigation
└── Kirigami.PageRow    main pages + pushed detail pages
    ├── OverviewPage        engine status + resource counts
    ├── ContainersPage      searchable/filterable container list
    ├── ContainerDetailsPage (pushed on demand)
    │   ├── Overview tab    inspect summary fields
    │   ├── Stats tab       live stats + CPU sparkline
    │   ├── Inspect tab     pretty JSON
    │   ├── Terminal tab    planned
    │   └── Files tab       planned
    ├── ImagesPage
    ├── NetworksPage
    ├── VolumesPage
    ├── ComposePage         honest "planned" state
    └── SettingsPage        connection + config info
```

Dialog components: `ConfirmRemoveDialog`, `ContainerLogsDialog`,
`ContainerInspectDialog`, `ErrorDetailsDialog`.

## Controllers and models

All page logic lives in Rust:

- `bridge/app_bridge.rs` — `AppController`: connection lifecycle,
  overview aggregation, shared `DockerServices` registry.
- `bridge/container_bridge.rs` — `ContainerListModel` (roles:
  containerId, shortId, name, image, state, status, ports, cpuPercent,
  memoryUsage, memoryLimit, createdAt, running, busy, operation) plus
  start/stop/restart/remove invokables.
- `bridge/detail_bridge.rs` — `ContainerDetailController` (inspect
  JSON, log follow with `logChunk` signal, stats polling with
  cancellation) and `LogListModel` (capped, searchable log lines).
- `bridge/resource_bridges.rs` — `ImageListModel`, `NetworkListModel`,
  `VolumeListModel`.

The bridges are thin; `app_state.rs` holds pure page-state machines
(`ContainerPageState`, `LoadState`, `PageStatus`) with unit tests for
loading→ready/empty/error transitions, stale-generation rejection, and
busy/operation gating.

## Runtime

One shared Tokio runtime (`runtime.rs`). Invokables spawn tasks;
results return to the Qt thread via `CxxQtThread::queue`. Streams use
`CancellationToken`s cancelled by page close handlers.

## Kirigami theming

Colors come only from `Kirigami.Theme.*` (backgroundColor,
alternateBackgroundColor, textColor, disabledTextColor, highlightColor,
negativeTextColor, positiveTextColor, neutralTextColor), spacing from
`Kirigami.Units.*` (smallSpacing, mediumSpacing, largeSpacing,
gridUnit), and icons are FreeDesktop/KDE system icon names. There are
no hardcoded colors and no custom Light/Dark toggle — the app follows
the system theme (Breeze Light / Breeze Dark) automatically.

## Responsive layout

Wide windows: fixed sidebar + page list + optional pushed detail page.
Narrow windows: the `PageRow` stack pushes details as a new page and
the sidebar remains available. Window resizing is supported; minimum
size is 720×480.

## Testing

- Unit tests for `app_state` (controller transitions), `error`
  (message mapping), `settings` (defaults).
- `smoke_test.rs` loads the real `Main.qml` headless
  (`QT_QPA_PLATFORM=offscreen`) and asserts root objects are created.
