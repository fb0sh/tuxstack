# Development setup

TuxStack targets Linux (KDE Plasma preferred, Wayland first, X11
compatible). Package names below were verified against the respective
distributions; on Fedora and Ubuntu the Kirigami/Qt QML modules live in
the `universe` / non-default repos.

## Toolchain

- Rust 1.85+ (MSRV; project uses edition 2024)
- Cargo
- A C++ compiler (GCC or Clang)
- CMake and Make (used by the cxx-qt build)
- Qt 6 (Core, QML, Quick, QuickControls2, QuickLayouts)
- Kirigami (KF6) and kirigami-addons QML modules
- Docker Engine (for running the app and the integration tests)

## Arch Linux

```bash
sudo pacman -S rust gcc cmake make docker \
  qt6-base qt6-declarative qt6-quickcontrols2 \
  kirigami kirigami-addons
sudo systemctl enable --now docker
sudo usermod -aG docker "$USER"   # then log out and back in
```

## Fedora

```bash
sudo dnf install rust gcc-c++ cmake make docker \
  qt6-qtdeclarative qt6-qtquickcontrols qt6-qtquickcontrols2 \
  kf6-kirigami kf6-kirigami-addons
sudo systemctl enable --now docker
sudo usermod -aG docker "$USER"
```

## Ubuntu 24.04 (noble)

```bash
sudo apt install rustc cargo build-essential cmake make docker.io \
  qt6-base-dev qt6-declarative-dev \
  qml6-module-qtquick qml6-module-qtquick-controls \
  qml6-module-qtquick-layouts qml6-module-org-kde-kirigami
sudo systemctl enable --now docker
sudo usermod -aG docker "$USER"
```

Note: `qml6-module-org-kde-kirigami` is in `universe`; other
distributions may name it differently (e.g. Debian uses
`qml6-module-org-kde-kirigami`, older releases `qml-module-org-kde-kirigami2`
for KF5).

## Building

```bash
cargo build --workspace
```

## Running

```bash
cargo run -p tuxstack-gui        # GUI
cargo run -p tuxstack-cli -- info  # CLI
```

If the GUI cannot find Kirigami at runtime, point QML at the right
module path, e.g.:

```bash
export QML_IMPORT_PATH=/usr/lib/qt6/qml   # distro-dependent
```

## Tests

```bash
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

Docker integration tests are opt-in (`--ignored`) and require the user
to have socket access:

```bash
cargo test -p tuxstack-docker-core --test docker -- --ignored --nocapture
cargo test -p tuxstack-docker-core --test containers -- --ignored --nocapture
cargo test -p tuxstack-cli --test cli -- --ignored --nocapture
```

## Known toolchain quirks

- cxx-qt requires a C++ compiler and links Qt libraries through
  `qmake`/pkg-config; `QMAKE` can be set explicitly if needed.
- Qt 6.11 headers with GCC 16 emit benign `-Wsfinae-incomplete`
  warnings during the cxx-qt C++ build; they are upstream and harmless.
