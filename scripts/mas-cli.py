#!/usr/bin/env python3
"""Local Mobile API Studio CLI and optional MCP stdio bridge."""

import argparse
import json
import os
import socket
import stat
import sys
from pathlib import Path

MAX_REQUEST = 128 * 1024
MAX_RESPONSE = 16 * 1024 * 1024
SOCKET_TIMEOUT = 310
MCP_VERSION = "2025-11-25"
SUPPORTED_MCP_VERSIONS = {MCP_VERSION, "2025-06-18", "2025-03-26", "2024-11-05"}
COMMANDS = [
    "connect_capture_target", "current_connection", "delete_network_profile",
    "delete_proxy_rule", "disconnect_device", "export_interchange",
    "export_workspace", "get_flow_detail", "health",
    "list_devices", "list_flows", "list_network_profiles", "list_proxy_rules",
    "list_sessions", "preview_proxy_rule", "search_traffic",
    "upsert_network_profile", "upsert_proxy_rule",
]


def default_socket():
    return Path.home() / "Library" / "Application Support" / "dev.mobileapistudio.desktop" / "control" / "socket"


def validate_socket(path):
    uid = os.geteuid()
    parent = path.parent.lstat()
    target = path.lstat()
    if not stat.S_ISDIR(parent.st_mode) or parent.st_uid != uid or stat.S_IMODE(parent.st_mode) != 0o700:
        raise RuntimeError("Control socket directory must be owned by this user with mode 0700")
    if not stat.S_ISSOCK(target.st_mode) or target.st_uid != uid or stat.S_IMODE(target.st_mode) != 0o600:
        raise RuntimeError("Control socket must be owned by this user with mode 0600")


def socket_call(path, command, args):
    request = json.dumps({"command": command, "args": args}, separators=(",", ":")).encode()
    if len(request) > MAX_REQUEST:
        raise RuntimeError("Control request exceeds 128 KiB")
    validate_socket(path)
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as connection:
        connection.settimeout(SOCKET_TIMEOUT)
        connection.connect(str(path))
        connection.sendall(request)
        connection.shutdown(socket.SHUT_WR)
        response = bytearray()
        while len(response) <= MAX_RESPONSE:
            chunk = connection.recv(min(64 * 1024, MAX_RESPONSE + 1 - len(response)))
            if not chunk:
                break
            response.extend(chunk)
        if len(response) > MAX_RESPONSE:
            raise RuntimeError("Control response exceeds 16 MiB")
    try:
        return json.loads(response)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise RuntimeError("Control socket returned invalid JSON") from error


def read_json(stream, limit=MAX_REQUEST):
    data = stream.read(limit + 1)
    if len(data) > limit:
        raise ValueError(f"Input exceeds {limit} bytes")
    value = json.loads(data or b"{}")
    if not isinstance(value, dict):
        raise ValueError("Input must be a JSON object")
    return value


def rpc_error(request_id, code, message):
    return {"jsonrpc": "2.0", "id": request_id, "error": {"code": code, "message": message}}


def mcp_stdio(path):
    initialized = False
    while line := sys.stdin.buffer.readline(MAX_REQUEST + 1):
        request_id = None
        try:
            if len(line) > MAX_REQUEST:
                while line and not line.endswith(b"\n"):
                    line = sys.stdin.buffer.readline(MAX_REQUEST + 1)
                response = rpc_error(None, -32600, "MCP message exceeds 128 KiB")
                sys.stdout.buffer.write(json.dumps(response).encode() + b"\n")
                sys.stdout.buffer.flush()
                continue
            message = json.loads(line)
            if not isinstance(message, dict) or message.get("jsonrpc") != "2.0" or not isinstance(message.get("method"), str):
                raise ValueError("Invalid JSON-RPC request")
            method = message["method"]
            request_id = message.get("id")
            params = message.get("params") or {}
            if not isinstance(params, dict):
                raise ValueError("JSON-RPC params must be an object")

            if method == "notifications/initialized":
                initialized = True
                continue
            if method == "initialize":
                requested = params.get("protocolVersion", MCP_VERSION)
                version = requested if isinstance(requested, str) and requested in SUPPORTED_MCP_VERSIONS else MCP_VERSION
                result = {"protocolVersion": version, "capabilities": {"tools": {}}, "serverInfo": {"name": "mobile-api-studio", "version": "0.1.0"}}
            elif method == "ping":
                result = {}
            elif not initialized:
                result = rpc_error(request_id, -32002, "Server not initialized")
            elif method == "tools/list":
                result = {"tools": [{"name": "mobile_api_studio", "description": "Run one allowlisted Mobile API Studio command through the current-user local control socket.", "inputSchema": {"type": "object", "properties": {"command": {"type": "string", "enum": COMMANDS}, "args": {"type": "object"}}, "required": ["command", "args"], "additionalProperties": False}}]}
            elif method == "tools/call":
                if params.get("name") != "mobile_api_studio":
                    result = rpc_error(request_id, -32602, "Unknown tool")
                else:
                    arguments = params.get("arguments")
                    if not isinstance(arguments, dict) or set(arguments) != {"command", "args"} or not isinstance(arguments["command"], str) or not isinstance(arguments["args"], dict):
                        result = rpc_error(request_id, -32602, "Tool arguments require command and object args")
                    else:
                        output = socket_call(path, arguments["command"], arguments["args"])
                        failed = isinstance(output, dict) and "error" in output
                        structured = output if isinstance(output, dict) else {"result": output}
                        result = {"content": [{"type": "text", "text": json.dumps(output, ensure_ascii=False)}], "structuredContent": structured, "isError": failed}
            else:
                result = rpc_error(request_id, -32601, "Method not found")

            if request_id is not None:
                response = result if isinstance(result, dict) and result.get("jsonrpc") == "2.0" and "error" in result else {"jsonrpc": "2.0", "id": request_id, "result": result}
                sys.stdout.buffer.write(json.dumps(response, ensure_ascii=False, separators=(",", ":")).encode() + b"\n")
                sys.stdout.buffer.flush()
        except (ValueError, json.JSONDecodeError) as error:
            sys.stdout.buffer.write(json.dumps(rpc_error(request_id, -32600, str(error))).encode() + b"\n")
            sys.stdout.buffer.flush()
        except (OSError, RuntimeError) as error:
            if request_id is not None:
                result = {"content": [{"type": "text", "text": str(error)}], "isError": True}
                sys.stdout.buffer.write(json.dumps({"jsonrpc": "2.0", "id": request_id, "result": result}).encode() + b"\n")
                sys.stdout.buffer.flush()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--socket", type=Path, help="Use this explicit local control socket")
    parser.add_argument("--mcp", action="store_true", help="Bridge MCP JSON-RPC over stdio")
    parser.add_argument("command", nargs="?", help="Allowlisted command; pass its JSON args on stdin")
    options = parser.parse_args()
    path = options.socket or default_socket()
    try:
        if options.mcp:
            if options.command:
                parser.error("--mcp does not take a command")
            mcp_stdio(path)
            return 0
        if not options.command:
            parser.error("command is required unless --mcp is used")
        args = read_json(sys.stdin.buffer)
        result = socket_call(path, options.command, args)
        sys.stdout.write(json.dumps(result, ensure_ascii=False, indent=2) + "\n")
        return 1 if isinstance(result, dict) and "error" in result else 0
    except (OSError, RuntimeError, ValueError, json.JSONDecodeError) as error:
        print(str(error), file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
