# TuxStack GUI

Qt 6 + QML + Kirigami, bridged with CXX-Qt.

## QML page structure

```text
Kirigami.ApplicationWindow (Main.qml)
├── AppSidebar                 responsive KDE navigation
├── Kirigami.Separator
└── StackLayout
    ├── ContainersPage        current phase placeholder
    ├── ImagesPage            real Docker image management
    │   ├── ImageListPanel
    │   │   ├── search / sort / refresh / pull
    │   │   └── In Use / Unused image sections
    │   └── ImageDetailPanel
    │       ├── basic information / export
    │       ├── config
    │       ├── environment
    │       ├── labels
    │       └── used-by containers
    ├── VolumesPage           real Docker volume management
    │   ├── VolumeListPanel
    │   │   ├── search / sort / refresh / create / prune
    │   │   └── In Use / Unused volume sections
    │   └── VolumeDetailPanel
    │       ├── Info / Files tabs
    │       ├── Info: metadata / export / clone / used-by / labels
    │       └── Files: read-only helper-session browser + preview
    ├── NetworksPage          real Docker network management
    ├── ActivityMonitorPage   future phase placeholder
    ├── CommandsPage          future phase placeholder
    ├── DevicesPage           future phase placeholder
    └── SettingsPage          future phase placeholder
```

The Images detail panel starts directly with image metadata. It does not
contain Info, Terminal, or Files tabs.

Image workflows use `PullImageDialog`, `RemoveImageDialog`, and
`ExportImageDialog`. The save location is selected through Qt's native
save-file dialog.

Volume workflows use `CreateVolumeDialog`, `RemoveVolumeDialog`,
`PruneVolumesDialog`, `ExportVolumeDialog`, and `CloneVolumeDialog`. The
application shell owns one long-lived `VolumeListModel`, forwards Docker
connection status into it, cancels it during shutdown, and routes page retries
through `AppController.startup()`. The page initializes the model exactly once
and reuses its inventory and selection when navigation changes.

## Images controller and model

`ImageListModel` is a `QAbstractListModel` and the controller for the page.
Its roles start at `Qt::UserRole + 1` and expose scalar image-list data.
The typed detail snapshot is exposed as a QVariant map; environment,
labels, and used-by containers are structured QVariant lists rather than
JSON strings.

Pure, Qt-free state lives in:

- `src/controllers/images.rs` — loading/error transitions, local filtering,
  eight sort modes, selection preservation, generation guards, busy/pull/
  export state.
- `src/models/image_model.rs` — list and detail view mapping, IEC sizes,
  relative timestamps, config array formatting.

`src/bridge/image_bridge.rs` performs Docker and file operations on Tokio:

- list images and all containers, then load the selected image detail;
- remove an image with force/prune options and refresh from Docker;
- consume real pull progress with cancellation;
- stream image TAR bytes to a sibling temporary file, flush/sync, then
  atomically rename it to the selected destination;
- cancel refresh, detail, pull, export, and remove work during shutdown.

Refresh and detail requests have independent generation IDs, so stale
responses cannot overwrite newer state. Search and sorting operate only on
the in-memory inventory and never issue another Docker request.

## Volumes controller and model

`VolumeListModel` combines the volume `QAbstractListModel`, detail state, and
operation controller. The list and detail state machines are independent, and
the detail panel is a permanent `RowLayout` child rather than a conditional
`Loader`. Unknown Docker usage values remain unknown; aggregate text reports
known bytes and unknown-volume counts without presenting unknown data as zero.

The model associates volumes with all existing containers, including stopped
containers. Search and all ten sort modes are local. Refresh/detail generations
reject stale results, selection survives refresh where possible, and removing
the selected volume chooses an adjacent row. Used-by rows request navigation
through the model; `Main.qml` switches to Containers and records the full
container ID, matching Images navigation.

Export and clone run asynchronously through restricted helper containers and
support cooperative cancellation. Create, remove, prune, export, and clone
completion signals drive passive notifications and dialog lifecycle in QML.

## Volume files browser

`VolumeFileListModel` owns a separate read-only preview session per selected
volume. Switching to the Files tab starts a constrained helper container
(`alpine:3.20`, volume mounted at `/volume:ro`, no network, no privileges,
dropped capabilities). Directory listings, bounded text/JSON/image previews,
properties, and streaming Save As use Docker exec with validated
`VolumePath` values. Leaving Files or changing volumes tears the session down;
application startup also cleans orphan helpers labeled
`io.github.tuxstack.purpose=volume-preview`.

## Image/container association

`docker-core` requests all containers, including stopped and created ones.
It normalizes full and short `sha256` IDs, prefers exact image-ID matches,
accepts only unambiguous short-ID prefixes, and uses exact tag/digest aliases
only as a fallback. Docker's image-summary container count is not trusted for
the In Use/Unused grouping.

The total shown by the page is the logical sum of unique image IDs. It is not
presented as exclusive on-disk layer usage.

## Runtime and security

One shared Tokio runtime handles all Docker and file I/O. Results return to
the Qt thread through `CxxQtThread::queue`; the Qt event loop never blocks.
Pull/export streams and outstanding requests use `CancellationToken`.

Registry credentials exist only for the active pull request. Password/token
fields are cleared immediately after submission and credentials are never
logged or persisted. Image environment variables and labels are shown because
they are real image metadata, but their values are never written to tracing.

## Kirigami theming

Colors come from `Kirigami.Theme.*`, spacing and animation durations from
`Kirigami.Units.*`, and icons from the active FreeDesktop/KDE icon theme.
The sidebar and image rows use Breeze-style normal, hover, pressed, selected,
and keyboard-focus states. There are no fixed Light/Dark colors or custom
accent colors.

The sidebar automatically collapses on compact windows. The Images page uses
a two-panel list/detail layout on desktop widths and stacks the panels when
space is constrained.

## Testing

- docker-core mapping/service/stream unit tests cover image IDs, tags,
  dangling images, details, environment parsing, usage association, unique
  sizes, error classes, progress, and cancellation.
- GUI pure-state tests cover loading/error states, local search and sorting,
  selection/race behavior, busy cleanup, pull progress, image export, and all
  volume-operation state.
- `smoke_test.rs` loads every registered QML component and Rust QML type with
  the offscreen Qt platform, binds each model through its real camelCase API,
  exercises selected/loading and populated image/network/volume details, loads
  populated volume dialogs, and validates complete `Main.qml` creation.
- Real Docker image lifecycle tests are marked ignored and cover pull, list,
  inspect, container usage association, export, remove, and cleanup.
