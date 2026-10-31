#!/usr/bin/env bash
set -Eeuo pipefail

watch_backend=false
if [[ "${1:-}" == "--watch-backend" ]]; then
    watch_backend=true
elif [[ $# -gt 0 ]]; then
    echo "Usage: $0 [--watch-backend]" >&2
    exit 2
fi

if ! command -v dx >/dev/null 2>&1; then
    echo "Dioxus CLI is required: cargo install dioxus-cli --version 0.7.10 --locked" >&2
    exit 1
fi

if $watch_backend && ! command -v cargo-watch >/dev/null 2>&1; then
    echo "Backend hot reload requires cargo-watch: cargo install cargo-watch --locked" >&2
    exit 1
fi

export OXIDGENE_HOST="${OXIDGENE_HOST:-127.0.0.1}"
export OXIDGENE_PORT="${OXIDGENE_PORT:-8080}"
# A development server: GraphiQL, which loads from a CDN, is on here only.
export OXIDGENE_GRAPHIQL="${OXIDGENE_GRAPHIQL:-true}"
backend_url="http://127.0.0.1:${OXIDGENE_PORT}"
frontend_port="${OXIDGENE_WEB_PORT:-8081}"
frontend_host=127.0.0.1

# Authentication stays off unless asked (docs/authentication.md §8.5). With
# OXIDGENE_AUTH_MODE=oidc the server signs in against the Compose Rauthy
# (`just dev-idp-up`) with the development settings, each one kept
# when already set, and the page calls the API on its own origin, which the
# Dioxus development server forwards to the backend (Dioxus.toml's
# [[web.proxy]], which names port 8080): the browser opens
# http://localhost:8081, the origin the provider redirects to.
export OXIDGENE_AUTH_MODE="${OXIDGENE_AUTH_MODE:-none}"
if [[ "$OXIDGENE_AUTH_MODE" == "oidc" ]]; then
    while IFS='=' read -r name value; do
        [[ -z "$name" || "$name" == \#* ]] && continue
        [[ -v "$name" ]] || export "$name=$value"
    done < docker/rauthy/oxidgene-dev-auth.env
    export OXIDGENE_API_URL=""
    frontend_host=localhost
    if ! curl --silent --fail --output /dev/null "${OXIDGENE_OIDC_ISSUER%/}/.well-known/openid-configuration"; then
        echo "The identity provider at ${OXIDGENE_OIDC_ISSUER} does not answer: run \`just dev-idp-up\` first." >&2
        exit 1
    fi
else
    export OXIDGENE_API_URL="${OXIDGENE_API_URL:-$backend_url}"
fi
startup_timeout="${OXIDGENE_DEV_STARTUP_TIMEOUT:-300}"
startup_attempts=$((startup_timeout * 2))

children=()
use_process_groups=false
if command -v setsid >/dev/null 2>&1; then
    use_process_groups=true
fi

start_process() {
    if $use_process_groups; then
        setsid "$@" &
    else
        "$@" &
    fi
    started_pid=$!
    children+=("$started_pid")
}

cleanup() {
    local child
    trap - EXIT INT TERM
    for child in "${children[@]}"; do
        if $use_process_groups; then
            kill -- "-$child" 2>/dev/null || true
        else
            kill "$child" 2>/dev/null || true
        fi
    done
    for child in "${children[@]}"; do
        wait "$child" 2>/dev/null || true
    done
}
trap cleanup EXIT INT TERM

if $watch_backend; then
    start_process cargo watch -x "run --package oxidgene-server"
else
    start_process cargo run --package oxidgene-server
fi
backend_pid=$started_pid

echo "Waiting for the backend at ${backend_url}/healthz..."
for ((attempt = 1; attempt <= startup_attempts; attempt++)); do
    if curl --silent --fail --output /dev/null "${backend_url}/healthz"; then
        break
    fi
    if ! kill -0 "$backend_pid" 2>/dev/null; then
        if wait "$backend_pid"; then
            exit 0
        else
            exit $?
        fi
    fi
    if [[ $attempt -eq $startup_attempts ]]; then
        echo "Backend did not become healthy within ${startup_timeout} seconds." >&2
        exit 1
    fi
    sleep 0.5
done

echo "Backend ready at ${backend_url}"
echo "Frontend available at http://${frontend_host}:${frontend_port}"
start_process env OXIDGENE_API_URL="$OXIDGENE_API_URL" scripts/dx.sh serve \
    --package oxidgene-web \
    --web \
    --addr 127.0.0.1 \
    --port "$frontend_port" \
    --open false \
    --interactive false
frontend_pid=$started_pid

set +e
wait -n "$backend_pid" "$frontend_pid"
exit_code=$?
set -e
exit "$exit_code"
