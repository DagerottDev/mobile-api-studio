#!/usr/bin/env python3
import json
import os
import socket
import stat
import subprocess
import sys
import tempfile
import threading
from pathlib import Path

cli = Path(__file__).with_name("mas-cli.py")
with tempfile.TemporaryDirectory(prefix="mas-cli-check-") as temporary:
    control = Path(temporary) / "control"
    control.mkdir(mode=0o700)
    control.chmod(0o700)
    endpoint = control / "socket"
    server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    server.bind(str(endpoint))
    endpoint.chmod(0o600)
    server.listen(4)
    received = []

    def serve_once():
        connection, _ = server.accept()
        with connection:
            data = bytearray()
            while chunk := connection.recv(65536):
                data.extend(chunk)
            received.append(json.loads(data))
            connection.sendall(b'{"ok":true}')

    thread = threading.Thread(target=serve_once)
    thread.start()
    result = subprocess.run([sys.executable, str(cli), "--socket", str(endpoint), "health"], input=b'{"check":1}', capture_output=True)
    thread.join(5)
    assert result.returncode == 0, result.stderr.decode()
    assert json.loads(result.stdout) == {"ok": True}
    assert received == [{"command": "health", "args": {"check": 1}}]

    def serve_array():
        connection, _ = server.accept()
        with connection:
            request = bytearray()
            while chunk := connection.recv(65536):
                request.extend(chunk)
            assert json.loads(request) == {"command": "health", "args": {}}
            connection.sendall(b'[{"id":1}]')

    thread = threading.Thread(target=serve_array)
    thread.start()
    messages = [
        {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}},
        {"jsonrpc": "2.0", "method": "notifications/initialized"},
        {"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "mobile_api_studio", "arguments": {"command": "health", "args": {}}}},
    ]
    mcp = subprocess.run([sys.executable, str(cli), "--socket", str(endpoint), "--mcp"], input=b"\n".join(json.dumps(message).encode() for message in messages) + b"\n", capture_output=True)
    thread.join(5)
    assert mcp.returncode == 0, mcp.stderr.decode()
    tool_result = json.loads(mcp.stdout.splitlines()[-1])["result"]
    assert tool_result["structuredContent"] == {"result": [{"id": 1}]}

    oversized = subprocess.run([sys.executable, str(cli), "--socket", str(endpoint), "--mcp"], input=b"x" * (128 * 1024 + 1) + b"\n", capture_output=True)
    assert json.loads(oversized.stdout)["error"]["message"] == "MCP message exceeds 128 KiB"

    endpoint.chmod(0o666)
    denied = subprocess.run([sys.executable, str(cli), "--socket", str(endpoint), "health"], input=b"{}", capture_output=True)
    assert denied.returncode == 2 and b"mode 0600" in denied.stderr

    endpoint.chmod(0o600)
    alias = control / "alias"
    alias.symlink_to(endpoint)
    symlink_denied = subprocess.run([sys.executable, str(cli), "--socket", str(alias), "health"], input=b"{}", capture_output=True)
    assert symlink_denied.returncode == 2 and b"mode 0600" in symlink_denied.stderr
    server.close()

print("mas-cli socket authentication checks passed")
