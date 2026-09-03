#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"

usage() {
    printf '%s\n' \
        'Start the local web app and Rust backend for access through Docker.' \
        '' \
        'Usage:' \
        '  pnpm run dev:container' \
        '  ./scripts/start-container-dev.sh' \
        '' \
        'Environment variables:' \
        '  PORT                Base/frontend port (default: 4020)' \
        '  FRONTEND_PORT       Vite port; overrides PORT' \
        '  BACKEND_PORT        API port (default: FRONTEND_PORT + 1)' \
        '  PREVIEW_PROXY_PORT  Preview proxy port (default: FRONTEND_PORT + 2)' \
        '  HOST                Bind address (default: 0.0.0.0)' \
        '  VK_ALLOWED_ORIGINS  Comma-separated browser origins' \
        '' \
        'Publish FRONTEND_PORT and PREVIEW_PROXY_PORT from Docker, then open:' \
        '  http://localhost:<FRONTEND_PORT>'
}

if [[ "${1:-}" == "--" ]]; then
    shift
fi

if [[ "${1:-}" == "--help" || "${1:-}" == "-h" ]]; then
    usage
    exit 0
fi

if (( $# > 0 )); then
    printf 'Unknown argument: %s\n\n' "$1" >&2
    usage >&2
    exit 2
fi

require_port() {
    local name="$1"
    local value="$2"

    if [[ ! "$value" =~ ^[0-9]+$ ]] || (( 10#$value < 1 || 10#$value > 65535 )); then
        printf '%s must be an integer between 1 and 65535 (received: %s)\n' \
            "$name" "$value" >&2
        exit 2
    fi
}

frontend_port="${FRONTEND_PORT:-${PORT:-4020}}"
require_port FRONTEND_PORT "$frontend_port"

backend_port="${BACKEND_PORT:-$((10#$frontend_port + 1))}"
preview_proxy_port="${PREVIEW_PROXY_PORT:-$((10#$frontend_port + 2))}"
require_port BACKEND_PORT "$backend_port"
require_port PREVIEW_PROXY_PORT "$preview_proxy_port"

if [[ "$frontend_port" == "$backend_port" || \
      "$frontend_port" == "$preview_proxy_port" || \
      "$backend_port" == "$preview_proxy_port" ]]; then
    printf '%s\n' 'FRONTEND_PORT, BACKEND_PORT, and PREVIEW_PROXY_PORT must be distinct.' >&2
    exit 2
fi

if ! command -v pnpm >/dev/null 2>&1; then
    printf '%s\n' 'pnpm is required but was not found in PATH.' >&2
    exit 1
fi

if ! cargo watch --version >/dev/null 2>&1; then
    printf '%s\n' 'cargo-watch is required. Install it with: cargo install cargo-watch' >&2
    exit 1
fi

if [[ ! -d "${REPO_ROOT}/node_modules" ]]; then
    printf '%s\n' 'Dependencies are not installed. Run `pnpm i` first.' >&2
    exit 1
fi

export FRONTEND_PORT="$frontend_port"
export BACKEND_PORT="$backend_port"
export PREVIEW_PROXY_PORT="$preview_proxy_port"
export HOST="${HOST:-0.0.0.0}"
export VK_ALLOWED_ORIGINS="${VK_ALLOWED_ORIGINS:-http://localhost:${FRONTEND_PORT}}"
export VITE_VK_SHARED_API_BASE="${VK_SHARED_API_BASE:-}"

printf '%s\n' \
    'Starting easy-vibe-kanban for Docker access:' \
    "  Frontend:       http://localhost:${FRONTEND_PORT}" \
    "  Backend API:    ${HOST}:${BACKEND_PORT} (proxied through the frontend)" \
    "  Preview proxy:  ${HOST}:${PREVIEW_PROXY_PORT}" \
    "  Allowed origin: ${VK_ALLOWED_ORIGINS}" \
    '' \
    'Docker must publish the frontend and preview proxy ports.' \
    'Press Ctrl+C to stop both processes.'

cd -- "$REPO_ROOT"

exec pnpm exec concurrently \
    --kill-others \
    --names backend,frontend \
    --prefix-colors cyan,magenta \
    'pnpm run backend:dev:watch' \
    'pnpm --dir packages/local-web run dev --host "$HOST" --port "$FRONTEND_PORT" --strictPort'
