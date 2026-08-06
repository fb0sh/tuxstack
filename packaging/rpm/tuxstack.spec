Name:           tuxstack
Version:        0.3.2
Release:        1%{?dist}
Summary:        Docker and Incus desktop manager for Linux

License:        MIT
URL:            https://github.com/fb0sh/tuxstack
Source0:        %{name}-%{version}.tar.gz

# The user service is installed below /usr/lib/systemd/user on Fedora and
# openSUSE. Keep a fallback for RPM implementations without _userunitdir.
%{!?_userunitdir:%global _userunitdir %{_prefix}/lib/systemd/user}

BuildRequires:  cargo
BuildRequires:  rust
BuildRequires:  gcc-c++
BuildRequires:  cmake
BuildRequires:  make
BuildRequires:  pkgconfig
BuildRequires:  systemd-rpm-macros
BuildRequires:  qt6-qtbase-devel
BuildRequires:  qt6-qtdeclarative-devel
BuildRequires:  qt6-qtquickcontrols2-devel
BuildRequires:  kf6-kirigami
BuildRequires:  kf6-kirigami-addons
BuildRequires:  musl-gcc
BuildRequires:  rustup

Requires:       systemd
Requires:       qt6-qtbase
Requires:       qt6-qtdeclarative
Requires:       qt6-qtquickcontrols2
Requires:       kf6-kirigami
Requires:       kf6-kirigami-addons
Requires:       fuse3
Requires:       fuse3-libs
Requires:       util-linux

%description
TuxStack is a native Linux desktop manager for Docker and Incus. The
unprivileged tuxstackd user service is the sole Docker Engine client. The GUI
and tuxstackctl communicate with it over authenticated local Unix-socket IPC,
and tuxstackd exposes containers, images, and volumes through a persistent
read-only FUSE namespace under ~/TuxStack/docker.

%prep
%setup -q -n %{name}-%{version}

%build
# Fedora's rust-std-static package contains only the host standard library;
# it does not provide the linux-musl target package required by the embedded
# filesystem helper. Bootstrap an isolated stable rustup toolchain instead of
# referring to the nonexistent rust-std-static-* RPM capabilities.
export RUSTUP_HOME="%{_builddir}/.rustup"
export CARGO_HOME="%{_builddir}/.cargo-rustup"
rustup-init -y --profile minimal --default-toolchain stable --no-modify-path
export PATH="$CARGO_HOME/bin:$PATH"
case "$(uname -m)" in
    x86_64) rustup target add x86_64-unknown-linux-musl ;;
    aarch64) rustup target add aarch64-unknown-linux-musl ;;
    *) echo "Unsupported RPM architecture: $(uname -m)" >&2; exit 1 ;;
esac

# docker-core's build script also builds the static filesystem helper for the
# host architecture. cargo is intentionally used directly here so this spec
# remains usable outside Fedora's cargo-rpm macro set.
cargo build --offline --release --locked \
    -p tuxstack -p tuxstack-daemon -p tuxstack-cli

%install
install -Dpm0755 target/release/tuxstack \
    %{buildroot}%{_bindir}/tuxstack
install -Dpm0755 target/release/tuxstackd \
    %{buildroot}%{_bindir}/tuxstackd
install -Dpm0755 target/release/tuxstackctl \
    %{buildroot}%{_bindir}/tuxstackctl
install -Dpm0755 target/release/tuxstack-cli \
    %{buildroot}%{_bindir}/tuxstack-cli

install -Dpm0644 packaging/systemd/tuxstackd.service \
    %{buildroot}%{_userunitdir}/tuxstackd.service
install -Dpm0644 packaging/desktop/io.github.tuxstack.TuxStack.desktop \
    %{buildroot}%{_datadir}/applications/io.github.tuxstack.TuxStack.desktop
install -Dpm0644 packaging/desktop/io.github.tuxstack.TuxStack.metainfo.xml \
    %{buildroot}%{_datadir}/metainfo/io.github.tuxstack.TuxStack.metainfo.xml

for size in 16 22 24 32 48 64 128 256 512; do
    install -Dpm0644 \
        packaging/icons/hicolor/${size}x${size}/apps/io.github.tuxstack.TuxStack.png \
        %{buildroot}%{_datadir}/icons/hicolor/${size}x${size}/apps/io.github.tuxstack.TuxStack.png
done

install -Dpm0644 LICENSE %{buildroot}%{_licensedir}/%{name}/LICENSE

%check
# Keep the package build fast while still checking the binaries selected by
# this spec. Docker/FUSE integration tests are opt-in and are not run here.
cargo test --offline --locked -p tuxstack-domain -p tuxstack-protocol -p tuxstack-client

%post
%systemd_user_post tuxstackd.service
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -q -f %{_datadir}/icons/hicolor || :
fi

%preun
%systemd_user_preun tuxstackd.service

%postun
%systemd_user_postun_with_restart tuxstackd.service
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -q -f %{_datadir}/icons/hicolor || :
fi

%files
%license %{_licensedir}/%{name}/LICENSE
%doc README.md
%{_bindir}/tuxstack
%{_bindir}/tuxstackd
%{_bindir}/tuxstackctl
%{_bindir}/tuxstack-cli
%{_userunitdir}/tuxstackd.service
%{_datadir}/applications/io.github.tuxstack.TuxStack.desktop
%{_datadir}/metainfo/io.github.tuxstack.TuxStack.metainfo.xml
%{_datadir}/icons/hicolor/*/apps/io.github.tuxstack.TuxStack.png

%changelog
* Thu Aug 06 2026 TuxStack Maintainers <maintainers@tuxstack.internal> - 0.3.2-1
- Package the Docker/Incus desktop application, daemon, CLI, icons, and user service.
