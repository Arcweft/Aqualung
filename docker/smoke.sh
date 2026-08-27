#!/usr/bin/env bash
set -euo pipefail

IMAGE=${1:?usage: docker/smoke.sh <image> [grok-version]}
WANT_GROK=${2:-}
TOKEN=smoke-ci
NAME=aqualung-smoke-$$
INIT='{"jsonrpc":"2.0","id":0,"method":"initialize","params":{}}'

pick_port() {
  python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()'
}

PHONE_PORT=$(pick_port)
SNORKEL_PORT=$(pick_port)

phone() {
  python3 - "$PHONE_PORT" "$1" "$INIT" <<'PY'
import base64, hashlib, os, socket, struct, sys

host, port, token, payload = "127.0.0.1", int(sys.argv[1]), sys.argv[2], sys.argv[3]


def recv_exact(sock, n):
    buf = b""
    while len(buf) < n:
        chunk = sock.recv(n - len(buf))
        if not chunk:
            raise ConnectionError("closed")
        buf += chunk
    return buf


sock = None
try:
    sock = socket.create_connection((host, port), timeout=5)
    sock.settimeout(5)
    key = base64.b64encode(os.urandom(16)).decode("ascii")
    sock.sendall(
        (
            f"GET / HTTP/1.1\r\nHost: {host}:{port}\r\nUpgrade: websocket\r\n"
            f"Connection: Upgrade\r\nSec-WebSocket-Key: {key}\r\n"
            f"Sec-WebSocket-Version: 13\r\nAuthorization: Bearer {token}\r\n\r\n"
        ).encode("ascii")
    )
    buf = b""
    while b"\r\n\r\n" not in buf:
        chunk = sock.recv(4096)
        if not chunk:
            raise ConnectionError("closed during handshake")
        buf += chunk
    status = buf.split(b"\r\n", 1)[0]
    if b" 101 " not in status:
        raise ConnectionError(status.decode("ascii", "replace"))
    mask = os.urandom(4)
    data = payload.encode("utf-8")
    masked = bytes(b ^ mask[i % 4] for i, b in enumerate(data))
    header = bytearray([0x81, 0x80 | len(data)])
    sock.sendall(bytes(header) + mask + masked)
    hdr = recv_exact(sock, 2)
    n = hdr[1] & 0x7F
    if n == 126:
        n = struct.unpack("!H", recv_exact(sock, 2))[0]
    elif n == 127:
        n = struct.unpack("!Q", recv_exact(sock, 8))[0]
    if hdr[1] & 0x80:
        recv_exact(sock, 4)
    sys.stdout.write(recv_exact(sock, n).decode("utf-8"))
except OSError as exc:
    print(str(exc), file=sys.stderr)
    sys.exit(1)
finally:
    if sock is not None:
        sock.close()
PY
}

cleanup() {
  docker rm -f "$NAME" >/dev/null 2>&1 || true
}
trap cleanup EXIT

docker run -d --name "$NAME" --init \
  -p "127.0.0.1:${PHONE_PORT}:7678" \
  -p "127.0.0.1:${SNORKEL_PORT}:1943" \
  -e TOPSIDE_TOKEN="$TOKEN" \
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

if [[ $(wait_running) != true ]]; then
  fail_logs
  echo "container exited after phone attach" >&2
  exit 1
fi

printf 'ok %s\n' "$IMAGE"
