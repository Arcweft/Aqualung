#!/usr/bin/env python3
"""Tests for docker/phone.py. Stdlib only. No topside required."""
from __future__ import annotations

import base64
import hashlib
import json
import os
import socket
import struct
import sys
import threading
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import phone as client


def _unmasked_frame(payload: bytes, opcode: int = 0x1) -> bytes:
    header = bytearray([0x80 | opcode])
    n = len(payload)
    if n < 126:
        header.append(n)
    elif n < 65536:
        header.append(126)
        header.extend(struct.pack("!H", n))
    else:
        header.append(127)
        header.extend(struct.pack("!Q", n))
    return bytes(header) + payload


def _read_http(sock: socket.socket) -> tuple[bytes, bytes]:
    buf = b""
    while b"\r\n\r\n" not in buf:
        chunk = sock.recv(4096)
        if not chunk:
            raise ConnectionError("client closed during handshake")
        buf += chunk
    header, _, rest = buf.partition(b"\r\n\r\n")
    return header, rest


def _client_key(header: bytes) -> str:
    for line in header.split(b"\r\n")[1:]:
        if line.lower().startswith(b"sec-websocket-key:"):
            return line.split(b":", 1)[1].strip().decode("ascii")
    raise AssertionError("missing Sec-WebSocket-Key")


def _accept_header(key: str) -> bytes:
    digest = hashlib.sha1(
        key.encode("ascii") + b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11"
    ).digest()
    accept = base64.b64encode(digest).decode("ascii")
    return (
        "HTTP/1.1 101 Switching Protocols\r\n"
        "Upgrade: websocket\r\n"
        "Connection: Upgrade\r\n"
        f"Sec-WebSocket-Accept: {accept}\r\n"
        "\r\n"
    ).encode("ascii")


def _recv_masked_text(sock: socket.socket, leftover: bytes) -> tuple[str, bytes]:
    buf = leftover
    while len(buf) < 2:
        chunk = sock.recv(4096)
        if not chunk:
            raise ConnectionError("closed")
        buf += chunk
    n = buf[1] & 0x7F
    offset = 2
    if n == 126:
        while len(buf) < 4:
            buf += sock.recv(4096)
        n = struct.unpack("!H", buf[2:4])[0]
        offset = 4
    mask_at = offset
    payload_at = offset + 4
    while len(buf) < payload_at + n:
        buf += sock.recv(4096)
    mask = buf[mask_at:payload_at]
    payload = bytes(b ^ mask[i % 4] for i, b in enumerate(buf[payload_at : payload_at + n]))
    return payload.decode("utf-8"), buf[payload_at + n :]


class ScriptedServer(threading.Thread):
    def __init__(self, replies_for: dict[str, list[bytes]]):
        super().__init__(daemon=True)
        self.replies_for = replies_for
        self.error = None
        self._sock = socket.socket()
        self._sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self._sock.bind(("127.0.0.1", 0))
        self._sock.listen(1)
        self.port = self._sock.getsockname()[1]
        self.ready = threading.Event()

    def run(self) -> None:
        self.ready.set()
        try:
            sock, _ = self._sock.accept()
            with sock:
                header, leftover = _read_http(sock)
                key = _client_key(header)
                sock.sendall(_accept_header(key))
                while True:
                    text, leftover = _recv_masked_text(sock, leftover)
                    obj = json.loads(text)
                    method = obj.get("method", "")
                    frames = self.replies_for.get(method)
                    if frames is None:
                        raise AssertionError(f"unexpected method {method}: {text}")
                    for frame in frames:
                        sock.sendall(frame)
        except (ConnectionError, OSError):
            pass
        except Exception as exc:
            self.error = exc
        finally:
            self._sock.close()


class HelperTests(unittest.TestCase):
    def test_session_id_from_result(self) -> None:
        self.assertEqual(
            client.session_id_from({"result": {"sessionId": "sess_1"}}),
            "sess_1",
        )

    def test_session_id_from_params(self) -> None:
        self.assertEqual(
            client.session_id_from({"params": {"sessionId": "sess_2"}}),
            "sess_2",
        )

    def test_load_request_includes_grok_required_fields(self) -> None:
        obj = json.loads(client.load_request(2, "sess_1"))
        self.assertEqual(obj["params"]["sessionId"], "sess_1")
        self.assertEqual(obj["params"]["cwd"], "/tmp")
        self.assertEqual(obj["params"]["mcpServers"], [])

    def test_away_error(self) -> None:
        self.assertTrue(
            client.is_away_error({"error": {"code": -32003, "message": "host is away"}})
        )
        self.assertTrue(
            client.is_away_error(
                {
                    "error": {
                        "code": -32003,
                        "message": "host is away (unsupported leader protocol version 2)",
                    }
                }
            )
        )
        self.assertFalse(
            client.is_away_error({"error": {"code": -32000, "message": "Authentication required"}})
        )


class ExchangeTests(unittest.TestCase):
    def test_id_match_skips_host_away_notification(self) -> None:
        init_result = json.dumps(
            {"jsonrpc": "2.0", "id": 0, "result": {"protocolVersion": 1, "authMethods": []}}
        ).encode()
        away = json.dumps(
            {"jsonrpc": "2.0", "method": "aqualung/host_away", "params": {"away": True}}
        ).encode()
        created = json.dumps(
            {"jsonrpc": "2.0", "id": 1, "result": {"sessionId": "sess_live"}}
        ).encode()
        loaded = json.dumps({"jsonrpc": "2.0", "id": 2, "result": None}).encode()
        server = ScriptedServer(
            {
                "initialize": [_unmasked_frame(init_result), _unmasked_frame(away)],
                "session/new": [_unmasked_frame(created)],
                "session/load": [_unmasked_frame(loaded)],
            }
        )
        server.start()
        self.assertTrue(server.ready.wait(1))
        conn = client.connect("127.0.0.1", server.port, "tok", 2)
        try:
            code = client.probe_session(conn)
        finally:
            conn.sock.close()
            server.join(1)
        self.assertEqual(code, 0)
        if server.error:
            raise server.error

    def test_probe_away_on_session_new(self) -> None:
        init_result = json.dumps(
            {"jsonrpc": "2.0", "id": 0, "result": {"protocolVersion": 1, "authMethods": []}}
        ).encode()
        away_err = json.dumps(
            {
                "jsonrpc": "2.0",
                "id": 1,
                "error": {"code": -32003, "message": "host is away"},
            }
        ).encode()
        server = ScriptedServer(
            {
                "initialize": [_unmasked_frame(init_result)],
                "session/new": [_unmasked_frame(away_err)],
            }
        )
        server.start()
        self.assertTrue(server.ready.wait(1))
        conn = client.connect("127.0.0.1", server.port, "tok", 2)
        try:
            code = client.probe_session(conn)
        finally:
            conn.sock.close()
            server.join(1)
        self.assertEqual(code, client.EXIT_AWAY)

    def test_probe_auth_required_is_no_session(self) -> None:
        init_result = json.dumps(
            {"jsonrpc": "2.0", "id": 0, "result": {"protocolVersion": 1, "authMethods": []}}
        ).encode()
        auth_err = json.dumps(
            {
                "jsonrpc": "2.0",
                "id": 1,
                "error": {
                    "code": -32000,
                    "message": "Authentication required",
                    "data": "no auth method id provided",
                },
            }
        ).encode()
        server = ScriptedServer(
            {
                "initialize": [_unmasked_frame(init_result)],
                "session/new": [_unmasked_frame(auth_err)],
            }
        )
        server.start()
        self.assertTrue(server.ready.wait(1))
        conn = client.connect("127.0.0.1", server.port, "tok", 2)
        try:
            code = client.probe_session(conn)
        finally:
            conn.sock.close()
            server.join(1)
        self.assertEqual(code, client.EXIT_NO_SESSION)


if __name__ == "__main__":
    os.chdir(os.path.dirname(os.path.abspath(__file__)))
    unittest.main()
