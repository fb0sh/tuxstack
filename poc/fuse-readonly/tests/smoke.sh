#!/usr/bin/env bash
set -euo pipefail

# Mount-level Phase 0 smoke test. Pass the built binary as $1; an optional $2
# selects the test directory. The delayed node is lookup-only, so normal
# readdir remains exactly '.', '..', and 'hello.txt'. Reading
# .tuxstack-slow-read sleeps three seconds inside one FUSE callback. The
# simultaneous hello.txt read must still complete promptly with four workers.

binary=${1:?"usage: $0 BINARY [TEST_DIRECTORY]"}
base=${2:-"${TMPDIR:?TMPDIR must be project-local}/fuse-smoke"}
mnt="$base/mnt"
log="$base/poc.log"
pid=

cleanup() {
    if [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null; then
        kill -TERM "$pid" 2>/dev/null || true
        wait "$pid" 2>/dev/null || true
    fi
    if mountpoint -q "$mnt" 2>/dev/null; then
        fusermount3 -u -z "$mnt" 2>/dev/null || true
    fi
}
trap cleanup EXIT INT TERM

rm -rf -- "$base"
mkdir -p -- "$mnt"
"$binary" "$mnt" >"$log" 2>&1 &
pid=$!

for _ in $(seq 1 100); do
    mountpoint -q "$mnt" && break
    kill -0 "$pid" 2>/dev/null || {
        cat "$log" >&2
        exit 1
    }
    sleep 0.05
done
mountpoint -q "$mnt"

[[ "$(cat "$mnt/hello.txt")" == tuxstack-fuse-poc ]]
[[ "$(find "$mnt" -mindepth 1 -maxdepth 1 -printf '%f\n')" == hello.txt ]]
[[ "$(stat -c '%a %u %g' "$mnt")" == "555 $(id -u) $(id -g)" ]]
[[ "$(stat -c '%a %u %g' "$mnt/hello.txt")" == "444 $(id -u) $(id -g)" ]]
if printf x >"$mnt/hello.txt" 2>/dev/null; then
    echo "write unexpectedly succeeded" >&2
    exit 1
fi

cat "$mnt/.tuxstack-slow-read" >"$base/slow.out" &
slow_pid=$!
sleep 0.25
start_ns=$(date +%s%N)
[[ "$(cat "$mnt/hello.txt")" == tuxstack-fuse-poc ]]
elapsed_ms=$(( ($(date +%s%N) - start_ns) / 1000000 ))
wait "$slow_pid"
[[ "$(cat "$base/slow.out")" == "delayed read complete" ]]
if (( elapsed_ms >= 1500 )); then
    echo "unrelated read took ${elapsed_ms}ms during delayed callback" >&2
    exit 1
fi

kill -TERM "$pid"
wait "$pid"
pid=
! mountpoint -q "$mnt"
printf 'smoke test passed; unrelated read during 3s callback: %dms\n' "$elapsed_ms"
