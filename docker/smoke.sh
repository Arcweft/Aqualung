#!/usr/bin/env bash
# Prove a GHCR image starts grok, Registers through snorkel, answers initialize
# locally, then forwards session/new and session/load on one 7678 connection.
#
# Lives under docker/ so CI does not import .agents/. The phone client is
# docker/phone.py, also used by control-aqualung.
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
PHONE_PY="$ROOT/docker/phone.py"
IMAGE=${1:?usage: docker/smoke.sh <image> [grok-version]}
WANT_GROK=${2:-}
TOKEN=smoke-ci
NAME=aqualung-smoke-$$
SMOKE_KEY=${XAI_API_KEY:-aqualung-smoke-not-a-key}
AUTH_TOML="$ROOT/docker/grok-smoke-auth.toml"
INIT='{"jsonrpc":"2.0","id":0,"method":"initialize","params":{}}'

if ! python3 "$ROOT/docker/phone_test.py" >/dev/null 2>&1; then
  python3 "$ROOT/docker/phone_test.py" >&2
  exit 1
fi

pick_port() {
  python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()'
}

PHONE_PORT=$(pick_port)
SNORKEL_PORT=$(pick_port)

phone() {
  python3 "$PHONE_PY" --host 127.0.0.1 --port "$PHONE_PORT" --token "$1" --timeout 5 --send "$INIT"
}

cleanup() {
  docker rm -f "$NAME" >/dev/null 2>&1 || true
}
trap cleanup EXIT

docker run -d --name "$NAME" --init \
  -p "127.0.0.1:${PHONE_PORT}:7678" \
  -p "127.0.0.1:${SNORKEL_PORT}:1943" \
  -e TOPSIDE_TOKEN="$TOKEN" \
  -e XAI_API_KEY="$SMOKE_KEY" \
  -v "$AUTH_TOML:/var/lib/grok/config.toml:ro" \
  "$IMAGE" >/dev/null

wait_running() {
  docker inspect -f '{{.State.Running}}' "$NAME" 2>/dev/null || echo false
}

fail_logs() {
  docker logs "$NAME" >&2 || true
  docker exec "$NAME" sh -c 'cat /var/lib/grok/logs/unified.jsonl' >&2 || true
}

deadline=$((SECONDS + 30))
while ! docker exec "$NAME" sh -c 'test -S /var/lib/grok/leader.sock' 2>/dev/null; do
  if [[ $(wait_running) != true ]]; then
    fail_logs
    echo "container exited before leader socket" >&2
    exit 1
  fi
  if (( SECONDS >= deadline )); then
    fail_logs
    echo "leader socket did not appear" >&2
    exit 1
  fi
  sleep 0.2
done

if [[ -n "$WANT_GROK" ]]; then
  ver=$(docker exec "$NAME" grok --version)
  if [[ "$ver" != *"$WANT_GROK"* ]]; then
    echo "grok version want ${WANT_GROK}, got ${ver}" >&2
    exit 1
  fi
fi

deadline=$((SECONDS + 20))
while ! docker exec "$NAME" sh -c 'grep -q leader.client.registered /var/lib/grok/logs/unified.jsonl' 2>/dev/null; do
  if [[ $(wait_running) != true ]]; then
    fail_logs
    echo "container exited before leader register" >&2
    exit 1
  fi
  if (( SECONDS >= deadline )); then
    fail_logs
    echo "topside did not register with leader" >&2
    exit 1
  fi
  sleep 0.2
done

deadline=$((SECONDS + 20))
out=""
while (( SECONDS < deadline )); do
  if out=$(phone "$TOKEN"); then
    break
  fi
  if [[ $(wait_running) != true ]]; then
    fail_logs
    echo "container exited during initialize" >&2
    exit 1
  fi
  out=""
  sleep 0.3
done
if [[ -z "$out" ]]; then
  fail_logs
  echo "initialize did not return a JSON-RPC body" >&2
  exit 1
fi
python3 -c 'import json, sys
r = json.loads(sys.argv[1])
assert r.get("jsonrpc") == "2.0" and r.get("id") == 0 and "result" in r, r' "$out"
printf '%s\n' "$out"

reject_err=0
reject_out=$(phone wrong 2>&1) || reject_err=$?
if [[ "$reject_err" -eq 0 ]]; then
  echo "bad token was accepted" >&2
  exit 1
fi
if [[ "$reject_out" != *401* ]]; then
  printf '%s\n' "$reject_out" >&2
  echo "bad token did not return 401" >&2
  exit 1
fi

# Hub may still be BringingUp after Register. Retry until session/new is
# forwarded, then session/load on the same connection.
deadline=$((SECONDS + 120))
probe_code=1
probe_out=""
while (( SECONDS < deadline )); do
  probe_code=0
  probe_out=$(python3 "$PHONE_PY" --host 127.0.0.1 --port "$PHONE_PORT" --token "$TOKEN" --timeout 30 --probe-session) || probe_code=$?
  if [[ "$probe_code" -eq 0 ]]; then
    break
  fi
  if [[ $(wait_running) != true ]]; then
    fail_logs
    printf '%s\n' "$probe_out" >&2
    echo "container exited during session/load probe" >&2
    exit 1
  fi
  sleep 0.4
done

if [[ "$probe_code" -eq 0 ]]; then
  printf '%s\n' "$probe_out"
else
  fail_logs
  printf '%s\n' "$probe_out" >&2
  echo "session/new+load did not complete on 7678" >&2
  exit 1
fi

if [[ $(wait_running) != true ]]; then
  fail_logs
  echo "container exited after session probe" >&2
  exit 1
fi

printf 'ok %s\n' "$IMAGE"
