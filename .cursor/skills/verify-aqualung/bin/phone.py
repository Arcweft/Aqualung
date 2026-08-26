#!/usr/bin/env python3
"""One WebSocket text-frame round trip. Stdlib only."""
from __future__ import annotations

import argparse
import base64
import hashlib
import os
import socket
import struct
import sys


def _handshake(sock: socket.socket, host: str, port: int, token: str) -> None:
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
    sock.sendall(req.encode("ascii"))
    buf = b""
    while b"\r\n\r\n" not in buf:
        chunk = sock.recv(4096)
        if not chunk:
            raise ConnectionError("topside closed during WebSocket handshake")
        buf += chunk
    header, _, _rest = buf.partition(b"\r\n\r\n")
    status = header.split(b"\r\n", 1)[0]
    if b" 101 " not in status:
        raise ConnectionError(f"WebSocket upgrade failed: {status.decode('ascii', 'replace')}")
    accept = None
    for line in header.split(b"\r\n")[1:]:
        if line.lower().startswith(b"sec-websocket-accept:"):
            accept = line.split(b":", 1)[1].strip()
    expected = base64.b64encode(
        hashlib.sha1(key.encode("ascii") + b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11").digest()
    )
    if accept != expected:
        raise ConnectionError("Sec-WebSocket-Accept mismatch")


def _mask_frame(payload: bytes) -> bytes:
    mask = os.urandom(4)
    masked = bytes(b ^ mask[i % 4] for i, b in enumerate(payload))
    header = bytearray([0x81])
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


def _recv_text(sock: socket.socket) -> str:
    hdr = b""
    while len(hdr) < 2:
        chunk = sock.recv(2 - len(hdr))
        if not chunk:
            raise ConnectionError("topside closed before a JSON-RPC reply")
        hdr += chunk
    opcode = hdr[0] & 0x0F
    masked = bool(hdr[1] & 0x80)
    n = hdr[1] & 0x7F
    if n == 126:
        n = struct.unpack("!H", _recv_exact(sock, 2))[0]
    elif n == 127:
        n = struct.unpack("!Q", _recv_exact(sock, 8))[0]
    mask = _recv_exact(sock, 4) if masked else b""
    payload = _recv_exact(sock, n)
    if masked:
        payload = bytes(b ^ mask[i % 4] for i, b in enumerate(payload))
    if opcode == 0x8:
        raise ConnectionError("topside sent close")
    if opcode != 0x1:
        raise ConnectionError(f"expected text frame, got opcode {opcode}")
    return payload.decode("utf-8")


def _recv_exact(sock: socket.socket, n: int) -> bytes:
    buf = b""
    while len(buf) < n:
        chunk = sock.recv(n - len(buf))
        if not chunk:
            raise ConnectionError("topside closed mid-frame")
        buf += chunk
    return buf


def main() -> int:
    p = argparse.ArgumentParser()
    p.add_argument("--host", default="127.0.0.1")
    p.add_argument("--port", type=int, default=7678)
    p.add_argument("--token", required=True)
    p.add_argument("--send", required=True)
    args = p.parse_args()
    sock = None
    try:
        sock = socket.create_connection((args.host, args.port), timeout=5)
        sock.settimeout(5)
        _handshake(sock, args.host, args.port, args.token)
        sock.sendall(_mask_frame(args.send.encode("utf-8")))
        print(_recv_text(sock))
        return 0
    except OSError as exc:
        print(str(exc), file=sys.stderr)
        return 1
    finally:
        if sock is not None:
            sock.close()


if __name__ == "__main__":
    raise SystemExit(main())
