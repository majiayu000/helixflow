#!/usr/bin/env bash
set -euo pipefail

SMOKE_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SMOKE_DIR="$(mktemp -d)"
SMOKE_PORT="${HELIXFLOW_SMOKE_PORT:-0}"
SMOKE_TOKEN="release-smoke-token"
SMOKE_PID=""
SMOKE_READY=0
SMOKE_BASE_URL=""

cleanup() {
  if [[ -n "$SMOKE_PID" ]] && kill -0 "$SMOKE_PID" 2>/dev/null; then
    kill -TERM "$SMOKE_PID"
    wait "$SMOKE_PID"
  fi
  rm -rf -- "${SMOKE_DIR:?}"
}
trap cleanup EXIT INT TERM

cd "$SMOKE_ROOT"
cargo build --locked -p helixflow-server
HELIXFLOW_DATA_DIR="$SMOKE_DIR/data" \
HELIXFLOW_RUNTIME_PROVIDER=mock \
HELIXFLOW_ENABLE_MOCK_PROVIDER=1 \
HELIXFLOW_BIND_ADDR="127.0.0.1:$SMOKE_PORT" \
HELIXFLOW_AUTH_TOKEN="$SMOKE_TOKEN" \
HELIXFLOW_WEB_DIST="$SMOKE_ROOT/web/dist" \
"$SMOKE_ROOT/target/debug/helixflow-server" >"$SMOKE_DIR/server.log" 2>&1 &
SMOKE_PID="$!"

for _ in $(seq 1 120); do
  if ! kill -0 "$SMOKE_PID" 2>/dev/null; then
    sed -n '1,240p' "$SMOKE_DIR/server.log" >&2
    exit 1
  fi
  if [[ -z "$SMOKE_BASE_URL" ]]; then
    BOUND_PORT="$(sed -n 's/^helixflow listening on 127\.0\.0\.1:\([0-9][0-9]*\)$/\1/p' "$SMOKE_DIR/server.log" | tail -n 1)"
    if [[ -n "$BOUND_PORT" ]]; then
      SMOKE_BASE_URL="http://127.0.0.1:$BOUND_PORT"
    fi
  fi
  if [[ -n "$SMOKE_BASE_URL" ]] && curl --fail --silent \
    --header "Authorization: Bearer $SMOKE_TOKEN" \
    "$SMOKE_BASE_URL/api/ready" \
    >"$SMOKE_DIR/ready.json"; then
    SMOKE_READY=1
    break
  fi
  sleep 0.25
done

if [[ "$SMOKE_READY" -ne 1 ]]; then
  sed -n '1,240p' "$SMOKE_DIR/server.log" >&2
  exit 1
fi

curl --fail --silent --show-error \
  --header "Authorization: Bearer $SMOKE_TOKEN" \
  "$SMOKE_BASE_URL/api/health" \
  >"$SMOKE_DIR/health.json"
curl --fail --silent --show-error \
  --header "Authorization: Bearer $SMOKE_TOKEN" \
  "$SMOKE_BASE_URL/" \
  >"$SMOKE_DIR/index.html"
LOGIN_STATUS="$(curl --silent --show-error \
  --output /dev/null \
  --write-out '%{http_code}' \
  --cookie-jar "$SMOKE_DIR/browser.cookies" \
  --data-urlencode "token=$SMOKE_TOKEN" \
  "$SMOKE_BASE_URL/api/auth/session")"
if [[ "$LOGIN_STATUS" != "303" ]]; then
  printf 'browser login returned HTTP %s, expected 303\n' "$LOGIN_STATUS" >&2
  exit 1
fi
curl --fail --silent --show-error \
  --cookie "$SMOKE_DIR/browser.cookies" \
  "$SMOKE_BASE_URL/" \
  >"$SMOKE_DIR/browser-index.html"
grep -q '"ok"[[:space:]]*:[[:space:]]*true' "$SMOKE_DIR/ready.json"
grep -q '"database"[[:space:]]*:[[:space:]]*true' "$SMOKE_DIR/ready.json"
grep -q '"storage"[[:space:]]*:[[:space:]]*true' "$SMOKE_DIR/ready.json"
grep -q '"ok"[[:space:]]*:[[:space:]]*true' "$SMOKE_DIR/health.json"
grep -q '"service"[[:space:]]*:[[:space:]]*"helixflow"' "$SMOKE_DIR/health.json"
grep -q '<div id="root"></div>' "$SMOKE_DIR/index.html"
grep -q '<div id="root"></div>' "$SMOKE_DIR/browser-index.html"

kill -TERM "$SMOKE_PID"
wait "$SMOKE_PID"
SMOKE_PID=""
grep -q 'shutdown requested' "$SMOKE_DIR/server.log"
