#!/usr/bin/env bash
# The live checks of the archive portals (docs/archives.md §9.1), for every
# catalogued archive with an adapter and live checks, or for those named:
# `scripts/archives-live.sh [archive id…]`, run by `just archives-live` and by
# the Archive portals workflow.
#
# For each archive, one at a time:
#
# 1. The ignored Rust test runs steps 1 to 3 of the collections any client
#    may reach, over the native transport, into native.json.
# 2. The Playwright check (e2e/playwright.archives.config.ts) runs steps 1 to
#    3 of the browser-only collections through the bridge binary, then opens
#    every resolved target, and writes report.json.
#
# Archives whose portals share an address — several services hosted by one
# vendor, which rate-limits them together — are checked one after another
# with a pause between them (OXIDGENE_LIVE_HOST_PAUSE seconds, 120 by
# default; docs/archives.md §9.2).
#
# Both reports land in OXIDGENE_LIVE_REPORT_DIR (default
# target/archives-live) for one archive, and in a directory per archive
# within it for several; the Playwright traces of each archive of several in
# e2e/test-results/archives-<archive id>. The script fails on a drift, or
# when a check could not run; an unreachable or challenged portal is
# reported without failing.
set -Eeuo pipefail
cd "$(dirname "$0")/.."

report_dir="${OXIDGENE_LIVE_REPORT_DIR:-$PWD/target/archives-live}"
pause="${OXIDGENE_LIVE_HOST_PAUSE:-120}"
export OXIDGENE_LIVE_BRIDGE="$PWD/target/debug/archives-live-bridge"

cargo build --locked -p oxidgene-archives --features live --bin archives-live-bridge
# Every archive checked, or those named: an unknown archive, or one whose
# catalogue entry opts out, stops here.
if [ "$#" -eq 0 ]; then
    mapfile -t archives < <("$OXIDGENE_LIVE_BRIDGE" --list)
else
    archives=("$@")
    for archive in "${archives[@]}"; do
        "$OXIDGENE_LIVE_BRIDGE" --list "$archive" >/dev/null
    done
fi
(cd e2e && npm ci --no-audit --no-fund && npx playwright install ${CI:+--with-deps} chromium)

# The address an archive's portal answers on: its first collection's origin,
# resolved, or its host name when it does not resolve.
address() {
    local origin host resolved
    origin=$(jq -r --arg id "$1" 'select(.id == $id) | .collections[0].portal.origin' assets/archives/*/*.json)
    host=${origin#*://}
    host=${host%%[:/]*}
    resolved=$(getent ahostsv4 "$host" | awk 'NR == 1 { print $1 }' || true)
    echo "${resolved:-$host}"
}

# Steps 1 to 5 of one archive, its reports in the directory given.
check() {
    local status=0
    export OXIDGENE_LIVE_ARCHIVE="$1" OXIDGENE_LIVE_REPORT_DIR="$2"
    rm -rf "$OXIDGENE_LIVE_REPORT_DIR"
    mkdir -p "$OXIDGENE_LIVE_REPORT_DIR"
    cargo nextest run --locked -p oxidgene-archives --features native,live --test live \
        --run-ignored only --no-capture || status=$?
    (cd e2e && npx playwright test --config playwright.archives.config.ts) || status=$?
    return "$status"
}

if [ "${#archives[@]}" -eq 1 ]; then
    check "${archives[0]}" "$report_dir"
    exit
fi

status=0
previous=""
rm -rf "$report_dir"
# Archives sharing an address one after another, in catalogue order within.
while read -r at archive; do
    if [ "$at" = "$previous" ]; then
        echo "Pausing ${pause}s before $archive, whose portal shares the address of the previous one"
        sleep "$pause"
    fi
    previous="$at"
    check "$archive" "$report_dir/$archive" || status=$?
    rm -rf "e2e/test-results/archives-$archive"
    if [ -d e2e/test-results/archives ]; then
        mv e2e/test-results/archives "e2e/test-results/archives-$archive"
    fi
done < <(for archive in "${archives[@]}"; do echo "$(address "$archive") $archive"; done | sort -s -k1,1)
exit "$status"
