# RPM packaging

The RPM package installs the three product binaries, desktop integration, icons,
and the per-user daemon unit:

- `/usr/bin/tuxstack`
- `/usr/bin/tuxstackd`
- `/usr/bin/tuxstackctl`
- `/usr/lib/systemd/user/tuxstackd.service` (the path follows `%{_userunitdir}`)
- hicolor icons, desktop entry, and AppStream metadata

## Build

On Fedora/RHEL-like systems install the build toolchain first:

```bash
sudo dnf install rpm-build rpmdevtools cargo rust rustup \
  gcc-c++ cmake make pkgconf-pkg-config \
  qt6-qtbase-devel qt6-qtdeclarative-devel qt6-qtquickcontrols2-devel \
  kf6-kirigami kf6-kirigami-addons fuse3-devel musl-gcc
```

Then run from the repository root:

```bash
./packaging/rpm/build-rpm.sh
```

The script creates a vendored source archive and places the resulting RPM and
source RPM in `packaging/rpm/RPMS/`. It uses the current working tree, so a
replacement `tuxstack.png` and local README changes are included. User-owned
planning files under `docs/` and local build output are intentionally excluded.

The build script cannot run until `rpmbuild` is installed. The repository's
Arch development environment does not provide that tool by default; the RPM
spec is intended to be built on Fedora/RHEL or another RPM-based build host.

## Install and start the daemon

The daemon must be a **systemd user service**, not a system-wide root service:
it needs the user's `HOME`, `XDG_RUNTIME_DIR`, Docker-group access, and permission
to own the user's FUSE mount at `~/TuxStack/docker`.

```bash
sudo dnf install ./packaging/rpm/RPMS/tuxstack-0.3.1-1.*.rpm
systemctl --user daemon-reload
systemctl --user enable --now tuxstackd.service
systemctl --user status tuxstackd.service
journalctl --user -u tuxstackd.service -f
```

To stop it:

```bash
systemctl --user disable --now tuxstackd.service
```

For a daemon that survives logout, enable user lingering according to local
system policy:

```bash
loginctl enable-linger "$USER"
```

The GUI and `tuxstackctl` connect through:

```text
$XDG_RUNTIME_DIR/tuxstack/control.sock
```
