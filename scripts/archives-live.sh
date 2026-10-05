#!/usr/bin/env bash
# The live checks of the archive portals (docs/archives.md §9.1), for every
# catalogued archive with an adapter and live checks, or for the one named:
# `scripts/archives-live.sh [archive id]`, run by `just archives-live` and by
# the Archive portals workflow.
#
# 1. The ignored Rust test runs steps 1 to 3 of the collections any client
#    may reach, over the native transport, into native.json.
# 2. The Playwright check (e2e/playwright.archives.config.ts) runs steps 1 to
#    3 of the browser-only collections through the bridge binary, then opens
#    every resolved target, and writes report.json.
#
# Both land in OXIDGENE_LIVE_REPORT_DIR (default target/archives-live). The
# script fails on a drift, or when a check could not run; an unreachable or
# challenged portal is reported without failing.
set -Eeuo pipefail
cd "$(dirname "$0")/.."

archive="${1:-}"
export OXIDGENE_LIVE_ARCHIVE="$archive"
export OXIDGENE_LIVE_REPORT_DIR="${OXIDGENE_LIVE_REPORT_DIR:-$PWD/target/archives-live}"
export OXIDGENE_LIVE_BRIDGE="$PWD/target/debug/archives-live-bridge"

rm -rf "$OXIDGENE_LIVE_REPORT_DIR"
mkdir -p "$OXIDGENE_LIVE_REPORT_DIR"
cargo build --locked -p oxidgene-archives --features live --bin archives-live-bridge
# An unknown archive, or one whose catalogue entry opts out, stops here.
"$OXIDGENE_LIVE_BRIDGE" --list ${archive:+"$archive"} >/dev/null
(cd e2e && npm ci --no-audit --no-fund && npx playwright install ${CI:+--with-deps} chromium)

status=0
cargo nextest run --locked -p oxidgene-archives --features native,live --test live \
    --run-ignored only --no-capture || status=$?
(cd e2e && npx playwright test --config playwright.archives.config.ts) || status=$?
exit "$status"
