#!/bin/sh
# Runs target/debug/<binary> in the development overlay
# (docker-compose.dev.yml), and runs it again whenever `backend-build`
# replaces it, or when it exits. The binary is built by another container
# into the shared target volume, so this only waits for it: nothing here
# compiles.
set -eu

binary="/app/target/debug/$1"
pid=""
trap 'if [ -n "$pid" ]; then kill "$pid" 2>/dev/null || true; fi; exit 0' INT TERM

until [ -x "$binary" ]; do
    echo "waiting for the first build of $1…"
    sleep 2
done

while true; do
    # Cargo links each build in as a new file: its inode changes even when
    # two builds fall within the same second.
    built="$(stat -c '%i %Y' "$binary")"
    "$binary" &
    pid=$!
    while kill -0 "$pid" 2>/dev/null && [ "$(stat -c '%i %Y' "$binary" 2>/dev/null || echo "$built")" = "$built" ]; do
        sleep 1
    done
    if kill -0 "$pid" 2>/dev/null; then
        echo "$1 was rebuilt: restarting"
        kill "$pid" 2>/dev/null || true
    fi
    wait "$pid" 2>/dev/null || true
    pid=""
    # A crashed binary is not restarted in a loop faster than a rebuild.
    sleep 1
done
