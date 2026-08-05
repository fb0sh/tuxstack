#!/usr/bin/env bash
# Build a self-contained TuxStack RPM from the current working tree.
#
# The source archive is made from the working tree rather than HEAD so a local
# icon replacement or documentation change is included before it is committed.
# User-owned files and build artifacts are deliberately excluded.
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
VERSION=$(awk -F'"' '/^version = / { print $2; exit }' "$ROOT/Cargo.toml")
NAME="tuxstack-${VERSION}"
OUT_DIR="$ROOT/packaging/rpm/RPMS"
WORK_DIR=$(mktemp -d "${TMPDIR:-/tmp}/tuxstack-rpm.XXXXXX")
TOP_DIR="$WORK_DIR/rpmbuild"
SOURCE_ROOT="$WORK_DIR/$NAME"
SOURCE_TARBALL="$WORK_DIR/${NAME}.tar.gz"

cleanup() {
    rm -rf "$WORK_DIR"
}
trap cleanup EXIT

if ! command -v rpmbuild >/dev/null 2>&1; then
    cat >&2 <<'EOF'
error: rpmbuild is not installed.
Install the RPM build toolchain first, for example on Fedora:
  sudo dnf install rpm-build rpmdevtools cargo rust cargo-vendor
Then run packaging/rpm/build-rpm.sh again.
EOF
    exit 1
fi

if ! command -v cargo >/dev/null 2>&1; then
    echo "error: cargo is required to build the RPM" >&2
    exit 1
fi

mkdir -p "$TOP_DIR"/{BUILD,BUILDROOT,RPMS,SOURCES,SPECS,SRPMS}
mkdir -p "$SOURCE_ROOT"

# Do not package repository metadata, compiler output, temporary test logs, or
# the user-owned RefactorFS planning files. The current icon and all ordinary
# project files remain in the archive.
tar -C "$ROOT" \
    --exclude='./.git' \
    --exclude='./target' \
    --exclude='./.tmp-build' \
    --exclude='./docs/PLAN.md' \
    --exclude='./docs/RefactorFS.md' \
    --exclude='./packaging/rpm/RPMS' \
    --exclude='./packaging/rpm/BUILD' \
    --exclude='./packaging/rpm/BUILDROOT' \
    --exclude='./packaging/rpm/SOURCES' \
    --exclude='./packaging/rpm/SPECS' \
    --exclude='./packaging/rpm/SRPMS' \
    --transform="s,^\./,${NAME}/," \
    -cf - . | tar -C "$WORK_DIR" -xf -

# Vendor dependencies into the source archive. The spec builds with
# --offline, which makes the RPM reproducible on builders without network
# access after the source RPM has been created.
mkdir -p "$SOURCE_ROOT/.cargo"
(
    cd "$SOURCE_ROOT"
    cargo vendor --locked vendor > .cargo/config.toml
)
tar -C "$WORK_DIR" -czf "$SOURCE_TARBALL" "$NAME"

cp "$SOURCE_TARBALL" "$TOP_DIR/SOURCES/"
cp "$ROOT/packaging/rpm/tuxstack.spec" "$TOP_DIR/SPECS/"

rpmbuild -ba \
    --define "_topdir $TOP_DIR" \
    --define "_sourcedir $TOP_DIR/SOURCES" \
    --define "_specdir $TOP_DIR/SPECS" \
    "$TOP_DIR/SPECS/tuxstack.spec"

mkdir -p "$OUT_DIR"
find "$TOP_DIR/RPMS" -type f -name '*.rpm' -exec cp -f {} "$OUT_DIR/" \;
find "$TOP_DIR/SRPMS" -type f -name '*.src.rpm' -exec cp -f {} "$OUT_DIR/" \;

echo "RPM packages written to: $OUT_DIR"
find "$OUT_DIR" -maxdepth 1 -type f -printf '  %f\n' | sort
