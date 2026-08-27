#!/usr/bin/env python3
"""ACP WebSocket client for topside on 7678. Stdlib only.

Smoke and control-aqualung both call this file. One connection can send several
JSON-RPC requests. Replies are matched by id so a following host-away
notification cannot steal the initialize result.

Stdout is one JSON object per request, in send order. Notifications go to stderr.
"""
from __future__ import annotations

import argparse
import base64
import hashlib
import json
import os
import socket
import struct
import sys
from typing import Any


INIT = '{"jsonrpc":"2.0","id":0,"method":"initialize","params":{}}'
NEW = '{"jsonrpc":"2.0","id":1,"method":"session/new","params":{"cwd":"/tmp","mcpServers":[]}}'
DEFAULT_PROMPT = "Reply with the single word PONG and nothing else."

# Host is still BringingUp. Caller should reconnect later.
EXIT_AWAY = 4
# session/new reached grok, but there is no sessionId to load.
EXIT_NO_SESSION = 3


def ids_equal(left: Any, right: Any) -> bool:
    return left == right


def session_id_from(obj: dict[str, Any]) -> str | None:
    params = obj.get("params")
    if isinstance(params, dict):
        value = params.get("sessionId")
        if isinstance(value, str) and value:
            return value
    result = obj.get("result")
    if isinstance(result, dict):
        value = result.get("sessionId")
        if isinstance(value, str) and value:
            return value
    return None


def error_code(obj: dict[str, Any]) -> int | None:
    err = obj.get("error")
    if not isinstance(err, dict):
        return None
    code = err.get("code")
    return code if isinstance(code, int) else None


def error_message(obj: dict[str, Any]) -> str:
    err = obj.get("error")
    if not isinstance(err, dict):
        return ""
    return str(err.get("message") or "")


def is_away_error(obj: dict[str, Any]) -> bool:
    return error_message(obj).startswith("host is away")


def is_uninitialized(obj: dict[str, Any]) -> bool:
    return error_message(obj) == "phone has not initialized"


def parse_object(text: str) -> dict[str, Any]:
    obj = json.loads(text)
    if not isinstance(obj, dict):
        raise ValueError("JSON-RPC body is not an object")
    return obj


def request_id_of(text: str) -> Any:
    obj = parse_object(text)
    if "id" not in obj:
        raise ValueError("request has no id")
    return obj["id"]


class Conn:
    def __init__(self, sock: socket.socket):
        self.sock = sock
        self.buf = b""

    def recv_exact(self, n: int) -> bytes:
        while len(self.buf) < n:
            chunk = self.sock.recv(max(4096, n - len(self.buf)))
            if not chunk:
                raise ConnectionError("topside closed mid-frame")
            self.buf += chunk
        out, self.buf = self.buf[:n], self.buf[n:]
        return out

    def recv_some(self) -> bytes:
        if self.buf:
            out, self.buf = self.buf, b""
            return out
        chunk = self.sock.recv(4096)
        if not chunk:
            raise ConnectionError("topside closed")
        return chunk


def handshake(conn: Conn, host: str, port: int, token: str) -> None:
    key = base64.b64encode(os.urandom(16)).decode("ascii")
    req = (
        f"GET / HTTP/1.1\r\n"
        f"Host: {host}:{port}\r\n"
        "Upgrade: websocket\r\n"
        "Connection: Upgrade\r\n"
        f"Sec-WebSocket-Key: {key}\r\n"
        "Sec-WebSocket-Version: 13\r\n"
        f"Authorization: Bearer {token}\r\n"
        "\r\n"
    )
    conn.sock.sendall(req.encode("ascii"))
    buf = b""
    while b"\r\n\r\n" not in buf:
        buf += conn.recv_some()
    header, _, rest = buf.partition(b"\r\n\r\n")
    conn.buf = rest + conn.buf
    status = header.split(b"\r\n", 1)[0]
    if b" 101 " not in status:
        raise ConnectionError(
            f"WebSocket upgrade failed: {status.decode('ascii', 'replace')}"
        )
    accept = None
    for line in header.split(b"\r\n")[1:]:
        if line.lower().startswith(b"sec-websocket-accept:"):
            accept = line.split(b":", 1)[1].strip()
    expected = base64.b64encode(
        hashlib.sha1(key.encode("ascii") + b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11").digest()
    )
    if accept != expected:
        raise ConnectionError("Sec-WebSocket-Accept mismatch")


def mask_frame(payload: bytes, opcode: int = 0x1) -> bytes:
    mask = os.urandom(4)
    masked = bytes(b ^ mask[i % 4] for i, b in enumerate(payload))
    header = bytearray([0x80 | opcode])
    n = len(payload)
    if n < 126:
        header.append(0x80 | n)
    elif n < 65536:
        header.append(0x80 | 126)
        header.extend(struct.pack("!H", n))
    else:
        header.append(0x80 | 127)
        header.extend(struct.pack("!Q", n))
    return bytes(header) + mask + masked


def recv_message(conn: Conn) -> tuple[int, bytes]:
    hdr = conn.recv_exact(2)
    opcode = hdr[0] & 0x0F
    masked = bool(hdr[1] & 0x80)
    n = hdr[1] & 0x7F
    if n == 126:
        n = struct.unpack("!H", conn.recv_exact(2))[0]
    elif n == 127:
        n = struct.unpack("!Q", conn.recv_exact(8))[0]
    mask = conn.recv_exact(4) if masked else b""
    payload = conn.recv_exact(n)
    if masked:
        payload = bytes(b ^ mask[i % 4] for i, b in enumerate(payload))
    return opcode, payload


def recv_text(conn: Conn) -> str:
    while True:
        opcode, payload = recv_message(conn)
        if opcode == 0x8:
            raise ConnectionError("topside sent close")
        if opcode == 0x9:
            conn.sock.sendall(mask_frame(payload, opcode=0xA))
            continue
        if opcode == 0xA:
            continue
        if opcode != 0x1:
            raise ConnectionError(f"expected text frame, got opcode {opcode}")
        return payload.decode("utf-8")


def wait_reply(conn: Conn, rpc_id: Any, notes: list[str]) -> dict[str, Any]:
    while True:
        text = recv_text(conn)
        obj = parse_object(text)
        if "id" in obj and ids_equal(obj["id"], rpc_id):
            return obj
        notes.append(text)


def load_request(rpc_id: Any, session: str) -> str:
    return json.dumps(
        {
            "jsonrpc": "2.0",
            "id": rpc_id,
            "method": "session/load",
            "params": {"sessionId": session, "cwd": "/tmp", "mcpServers": []},
        },
        separators=(",", ":"),
    )


def prompt_request(rpc_id: Any, session: str, text: str) -> str:
    return json.dumps(
        {
            "jsonrpc": "2.0",
            "id": rpc_id,
            "method": "session/prompt",
            "params": {
                "sessionId": session,
                "prompt": [{"type": "text", "text": text}],
            },
        },
        separators=(",", ":"),
    )


def stop_reason(obj: dict[str, Any]) -> str | None:
    result = obj.get("result")
    if isinstance(result, dict):
        value = result.get("stopReason")
        if isinstance(value, str) and value:
            return value
    return None


def new_session_outcome(created: dict[str, Any]) -> tuple[int, str | None]:
    if is_away_error(created) or is_uninitialized(created):
        return EXIT_AWAY, None
    session = session_id_from(created)
    if session:
        return 0, session
    if error_code(created) == -32602:
        return 1, None
    return EXIT_NO_SESSION, None


def exchange(
    conn: Conn,
    sends: list[str],
    load_id: Any | None = None,
) -> tuple[list[dict[str, Any]], list[str]]:
    replies: list[dict[str, Any]] = []
    notes: list[str] = []
    for raw in sends:
        rpc_id = request_id_of(raw)
        conn.sock.sendall(mask_frame(raw.encode("utf-8")))
        replies.append(wait_reply(conn, rpc_id, notes))
    if load_id is not None:
        session = next((sid for obj in replies if (sid := session_id_from(obj))), None)
        if not session:
            return replies, notes
        raw = load_request(load_id, session)
        conn.sock.sendall(mask_frame(raw.encode("utf-8")))
        replies.append(wait_reply(conn, load_id, notes))
    return replies, notes


def connect(host: str, port: int, token: str, timeout: float) -> Conn:
    sock = socket.create_connection((host, port), timeout=timeout)
    sock.settimeout(timeout)
    conn = Conn(sock)
    handshake(conn, host, port, token)
    return conn


def write_replies(replies: list[dict[str, Any]], notes: list[str]) -> None:
    for obj in replies:
        sys.stdout.write(json.dumps(obj, separators=(",", ":")) + "\n")
    for note in notes:
        sys.stderr.write(note + "\n")


def probe_session(conn: Conn) -> int:
    replies, notes = exchange(conn, [INIT, NEW], load_id=2)
    write_replies(replies, notes)
    if len(replies) < 2:
        print("probe: missing session/new reply", file=sys.stderr)
        return 1
    code, session = new_session_outcome(replies[1])
    if code != 0:
        if code == EXIT_AWAY:
            print("probe: host is away", file=sys.stderr)
        else:
            print("probe: session/new has no sessionId", file=sys.stderr)
        return code
    if len(replies) < 3:
        print("probe: missing session/load reply", file=sys.stderr)
        return 1
    loaded = replies[2]
    if is_away_error(loaded) or is_uninitialized(loaded):
        print("probe: host is away on session/load", file=sys.stderr)
        return EXIT_AWAY
    print(f"probe: session/load replied sessionId={session}", file=sys.stderr)
    return 0


def probe_prompt(conn: Conn, text: str) -> int:
    replies, notes = exchange(conn, [INIT, NEW])
    if len(replies) < 2:
        write_replies(replies, notes)
        print("probe: missing session/new reply", file=sys.stderr)
        return 1
    code, session = new_session_outcome(replies[1])
    if code != 0 or not session:
        write_replies(replies, notes)
        if code == EXIT_AWAY:
            print("probe: host is away", file=sys.stderr)
        else:
            print("probe: session/new has no sessionId", file=sys.stderr)
        return code
    raw = prompt_request(3, session, text)
    conn.sock.sendall(mask_frame(raw.encode("utf-8")))
    replies.append(wait_reply(conn, 3, notes))
    write_replies(replies, notes)
    prompted = replies[2]
    if is_away_error(prompted) or is_uninitialized(prompted):
        print("probe: host is away on session/prompt", file=sys.stderr)
        return EXIT_AWAY
    if prompted.get("error") is not None:
        print("probe: session/prompt error", file=sys.stderr)
        return 1
    reason = stop_reason(prompted)
    if not reason:
        print("probe: session/prompt has no stopReason", file=sys.stderr)
        return 1
    print(f"probe: session/prompt stopReason={reason} sessionId={session}", file=sys.stderr)
    return 0


def main() -> int:
    p = argparse.ArgumentParser()
    p.add_argument("--host", default="127.0.0.1")
    p.add_argument("--port", type=int, default=7678)
    p.add_argument("--token", required=True)
    p.add_argument("--timeout", type=float, default=10)
    p.add_argument("--send", action="append", default=[])
    p.add_argument("--load-id", type=json.loads, default=None)
    p.add_argument(
        "--probe-session",
        action="store_true",
        help="initialize, session/new, then session/load on this connection",
    )
    p.add_argument(
        "--probe-prompt",
        action="store_true",
        help="initialize, session/new, then session/prompt on this connection",
    )
    p.add_argument("--prompt-text", default=DEFAULT_PROMPT)
    args = p.parse_args()
    probes = int(args.probe_session) + int(args.probe_prompt)
    if probes and args.send:
        print("probe flags cannot be combined with --send", file=sys.stderr)
        return 1
    if probes > 1:
        print("--probe-session cannot be combined with --probe-prompt", file=sys.stderr)
        return 1
    if probes == 0 and not args.send:
        print("phone requires --send, --probe-session, or --probe-prompt", file=sys.stderr)
        return 1
    conn = None
    try:
        conn = connect(args.host, args.port, args.token, args.timeout)
        if args.probe_session:
            return probe_session(conn)
        if args.probe_prompt:
            return probe_prompt(conn, args.prompt_text)
        replies, notes = exchange(conn, args.send, load_id=args.load_id)
        write_replies(replies, notes)
        if args.load_id is not None and not any(session_id_from(obj) for obj in replies[:-1]):
            print("no sessionId in replies; session/load not sent", file=sys.stderr)
            return EXIT_NO_SESSION
        return 0
    except OSError as exc:
        print(str(exc), file=sys.stderr)
        return 1
    finally:
        if conn is not None:
            conn.sock.close()


if __name__ == "__main__":
    raise SystemExit(main())
