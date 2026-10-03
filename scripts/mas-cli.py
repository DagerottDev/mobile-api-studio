#!/usr/bin/env python3
"""Local Mobile API Studio CLI and optional MCP stdio bridge."""

import argparse
import ctypes
import json
import os
import socket
import stat
import sys
import time
from pathlib import Path

MAX_REQUEST = 128 * 1024
MAX_RESPONSE = 16 * 1024 * 1024
SOCKET_TIMEOUT = 310
MCP_VERSION = "2025-11-25"
SUPPORTED_MCP_VERSIONS = {MCP_VERSION, "2025-06-18", "2025-03-26", "2024-11-05"}
COMMANDS = [
    "capture_platform_info", "connect_capture_target", "current_connection", "delete_network_profile",
    "delete_proxy_rule", "disconnect_device", "export_interchange",
    "export_workspace", "get_flow_detail", "health",
    "list_desktop_processes", "list_devices", "list_lan_interfaces", "list_flows", "list_network_profiles", "list_proxy_rules",
    "list_sessions", "preview_proxy_rule", "search_traffic",
    "upsert_network_profile", "upsert_proxy_rule",
]


def default_socket():
    if os.name == "nt":
        return rf"\\.\pipe\mobile-api-studio-control-{current_user_sid()}"
    if sys.platform.startswith("linux"):
        data_home = Path(os.environ.get("XDG_DATA_HOME", Path.home() / ".local" / "share"))
        if not data_home.is_absolute():
            data_home = Path.home() / ".local" / "share"
        return data_home / "dev.mobileapistudio.desktop" / "control" / "socket"
    return Path.home() / "Library" / "Application Support" / "dev.mobileapistudio.desktop" / "control" / "socket"


def validate_socket(path):
    if os.name == "nt":
        return
    uid = os.geteuid()
    parent = path.parent.lstat()
    target = path.lstat()
    if not stat.S_ISDIR(parent.st_mode) or parent.st_uid != uid or stat.S_IMODE(parent.st_mode) != 0o700:
        raise RuntimeError("Control socket directory must be owned by this user with mode 0700")
    if not stat.S_ISSOCK(target.st_mode) or target.st_uid != uid or stat.S_IMODE(target.st_mode) != 0o600:
        raise RuntimeError("Control socket must be owned by this user with mode 0600")


def _windows_apis():
    if os.name != "nt":
        raise RuntimeError("Windows named pipes are available only on Windows")
    from ctypes import wintypes

    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    advapi32 = ctypes.WinDLL("advapi32", use_last_error=True)
    kernel32.GetCurrentProcess.restype = wintypes.HANDLE
    kernel32.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
    kernel32.OpenProcess.restype = wintypes.HANDLE
    kernel32.GetNamedPipeServerProcessId.argtypes = [wintypes.HANDLE, ctypes.POINTER(wintypes.ULONG)]
    kernel32.GetNamedPipeServerProcessId.restype = wintypes.BOOL
    kernel32.CloseHandle.argtypes = [wintypes.HANDLE]
    kernel32.CloseHandle.restype = wintypes.BOOL
    kernel32.WaitNamedPipeW.argtypes = [wintypes.LPCWSTR, wintypes.DWORD]
    kernel32.WaitNamedPipeW.restype = wintypes.BOOL
    kernel32.CreateFileW.argtypes = [wintypes.LPCWSTR, wintypes.DWORD, wintypes.DWORD, wintypes.LPVOID, wintypes.DWORD, wintypes.DWORD, wintypes.HANDLE]
    kernel32.CreateFileW.restype = wintypes.HANDLE
    kernel32.CreateEventW.argtypes = [wintypes.LPVOID, wintypes.BOOL, wintypes.BOOL, wintypes.LPCWSTR]
    kernel32.CreateEventW.restype = wintypes.HANDLE
    kernel32.ReadFile.argtypes = [wintypes.HANDLE, wintypes.LPVOID, wintypes.DWORD, ctypes.POINTER(wintypes.DWORD), wintypes.LPVOID]
    kernel32.ReadFile.restype = wintypes.BOOL
    kernel32.WriteFile.argtypes = [wintypes.HANDLE, ctypes.c_void_p, wintypes.DWORD, ctypes.POINTER(wintypes.DWORD), wintypes.LPVOID]
    kernel32.WriteFile.restype = wintypes.BOOL
    kernel32.GetOverlappedResult.argtypes = [wintypes.HANDLE, wintypes.LPVOID, ctypes.POINTER(wintypes.DWORD), wintypes.BOOL]
    kernel32.GetOverlappedResult.restype = wintypes.BOOL
    kernel32.WaitForSingleObject.argtypes = [wintypes.HANDLE, wintypes.DWORD]
    kernel32.WaitForSingleObject.restype = wintypes.DWORD
    kernel32.CancelIoEx.argtypes = [wintypes.HANDLE, wintypes.LPVOID]
    kernel32.CancelIoEx.restype = wintypes.BOOL
    advapi32.OpenProcessToken.argtypes = [wintypes.HANDLE, wintypes.DWORD, ctypes.POINTER(wintypes.HANDLE)]
    advapi32.OpenProcessToken.restype = wintypes.BOOL
    advapi32.GetTokenInformation.argtypes = [wintypes.HANDLE, ctypes.c_int, wintypes.LPVOID, wintypes.DWORD, ctypes.POINTER(wintypes.DWORD)]
    advapi32.GetTokenInformation.restype = wintypes.BOOL
    return kernel32, advapi32


def _token_sid(advapi32, token):
    from ctypes import wintypes

    class SidAndAttributes(ctypes.Structure):
        _fields_ = [("Sid", wintypes.LPVOID), ("Attributes", wintypes.DWORD)]

    class TokenUser(ctypes.Structure):
        _fields_ = [("User", SidAndAttributes)]

    class Sid(ctypes.Structure):
        _fields_ = [("Revision", ctypes.c_ubyte), ("SubAuthorityCount", ctypes.c_ubyte), ("IdentifierAuthority", ctypes.c_ubyte * 6), ("SubAuthority", wintypes.DWORD * 1)]

    required = wintypes.DWORD()
    advapi32.GetTokenInformation(token, 1, None, 0, ctypes.byref(required))
    if not required.value:
        raise RuntimeError("Cannot verify the local control peer")
    buffer = ctypes.create_string_buffer(required.value)
    if not advapi32.GetTokenInformation(token, 1, buffer, required.value, ctypes.byref(required)):
        raise RuntimeError("Cannot verify the local control peer")
    user = ctypes.cast(buffer, ctypes.POINTER(TokenUser)).contents
    sid = ctypes.cast(user.User.Sid, ctypes.POINTER(Sid)).contents
    count = sid.SubAuthorityCount
    if count == 0 or count > 15:
        raise RuntimeError("Cannot verify the local control peer")
    authority = int.from_bytes(bytes(sid.IdentifierAuthority), "big")
    subauthorities = (wintypes.DWORD * count).from_address(ctypes.addressof(sid) + 8)
    return "S-{}-{}-{}".format(sid.Revision, authority, "-".join(str(value) for value in subauthorities))


def current_user_sid():
    from ctypes import wintypes

    kernel32, advapi32 = _windows_apis()
    token = wintypes.HANDLE()
    if not advapi32.OpenProcessToken(kernel32.GetCurrentProcess(), 0x0008, ctypes.byref(token)):
        raise RuntimeError("Cannot read the current Windows user")
    try:
        return _token_sid(advapi32, token)
    finally:
        kernel32.CloseHandle(token)


class _Overlapped(ctypes.Structure):
    from ctypes import wintypes

    _fields_ = [("Internal", ctypes.c_size_t), ("InternalHigh", ctypes.c_size_t), ("Offset", wintypes.DWORD), ("OffsetHigh", wintypes.DWORD), ("hEvent", wintypes.HANDLE)]


def _pipe_io(kernel32, handle, *, data=None, size=0, deadline):
    from ctypes import wintypes

    event = kernel32.CreateEventW(None, True, False, None)
    if not event:
        raise RuntimeError("Cannot communicate with the local control service")
    operation = _Overlapped()
    operation.hEvent = event
    buffer = ctypes.create_string_buffer(data if data is not None else size)
    transferred = wintypes.DWORD()
    try:
        if data is None:
            ok = kernel32.ReadFile(handle, buffer, size, ctypes.byref(transferred), ctypes.byref(operation))
        else:
            ok = kernel32.WriteFile(handle, buffer, len(data), ctypes.byref(transferred), ctypes.byref(operation))
        if not ok:
            error = ctypes.get_last_error()
            if error in (109, 232) and data is None:
                return b""
            if error != 997:
                raise RuntimeError("Cannot communicate with the local control service")
            remaining = max(0, int((deadline - time.monotonic()) * 1000))
            if kernel32.WaitForSingleObject(event, remaining) != 0:
                kernel32.CancelIoEx(handle, ctypes.byref(operation))
                kernel32.WaitForSingleObject(event, 0xFFFFFFFF)
                raise TimeoutError("Local control service timed out")
            if not kernel32.GetOverlappedResult(handle, ctypes.byref(operation), ctypes.byref(transferred), False):
                error = ctypes.get_last_error()
                if error in (109, 232) and data is None:
                    return b""
                raise RuntimeError("Cannot communicate with the local control service")
        return buffer.raw[:transferred.value] if data is None else transferred.value
    finally:
        kernel32.CloseHandle(event)


def windows_pipe_call(path, request):
    from ctypes import wintypes

    pipe_name = os.fspath(path)
    if not pipe_name.casefold().startswith("\\\\.\\pipe\\"):
        raise RuntimeError("Windows control endpoint must be a named pipe")
    kernel32, advapi32 = _windows_apis()
    deadline = time.monotonic() + SOCKET_TIMEOUT
    connect_deadline = min(deadline, time.monotonic() + 5)
    handle = None
    while time.monotonic() < connect_deadline:
        if not kernel32.WaitNamedPipeW(pipe_name, 250):
            error = ctypes.get_last_error()
            if error not in (2, 121, 231):
                raise RuntimeError("Cannot connect to the local control service")
            time.sleep(0.05)
            continue
        handle = kernel32.CreateFileW(pipe_name, 0xC0000000, 0, None, 3, 0x40000000, None)
        if handle != wintypes.HANDLE(-1).value:
            break
        if ctypes.get_last_error() not in (2, 121, 231):
            raise RuntimeError("Cannot connect to the local control service")
    if handle is None or handle == wintypes.HANDLE(-1).value:
        raise TimeoutError("Local control service did not accept a connection")

    try:
        process_id = wintypes.ULONG()
        if not kernel32.GetNamedPipeServerProcessId(handle, ctypes.byref(process_id)):
            raise RuntimeError("Cannot verify the local control peer")
        process = kernel32.OpenProcess(0x1000, False, process_id.value)
        if not process:
            raise RuntimeError("Cannot verify the local control peer")
        try:
            token = wintypes.HANDLE()
            if not advapi32.OpenProcessToken(process, 0x0008, ctypes.byref(token)):
                raise RuntimeError("Cannot verify the local control peer")
            try:
                if _token_sid(advapi32, token) != current_user_sid():
                    raise RuntimeError("The local control endpoint belongs to another user")
            finally:
                kernel32.CloseHandle(token)
        finally:
            kernel32.CloseHandle(process)

        offset = 0
        while offset < len(request):
            written = _pipe_io(kernel32, handle, data=request[offset:], deadline=deadline)
            if written <= 0:
                raise RuntimeError("Cannot communicate with the local control service")
            offset += written

        response = bytearray()
        while len(response) <= MAX_RESPONSE:
            chunk = _pipe_io(kernel32, handle, size=64 * 1024, deadline=deadline)
            if not chunk:
                break
            response.extend(chunk)
            if b"\n" in chunk:
                break
            if len(response) > MAX_RESPONSE:
                raise RuntimeError("Control response exceeds 16 MiB")
        if b"\n" not in response and len(response) > MAX_RESPONSE:
            raise RuntimeError("Control response exceeds 16 MiB")
        return response
    finally:
        kernel32.CloseHandle(handle)


def socket_call(path, command, args):
    request = json.dumps({"command": command, "args": args}, separators=(",", ":")).encode() + b"\n"
    if len(request) > MAX_REQUEST:
        raise RuntimeError("Control request exceeds 128 KiB")
    validate_socket(path)
    if os.name == "nt":
        response = windows_pipe_call(path, request)
    else:
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
                if b"\n" in response:
                    break
    response = response.split(b"\n", 1)[0]
    if len(response) > MAX_RESPONSE:
        raise RuntimeError("Control response exceeds 16 MiB")
    try:
        return json.loads(response)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise RuntimeError("Control service returned invalid JSON") from error


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
    parser.add_argument("--socket", help="Use this explicit local control endpoint")
    parser.add_argument("--mcp", action="store_true", help="Bridge MCP JSON-RPC over stdio")
    parser.add_argument("command", nargs="?", help="Allowlisted command; pass its JSON args on stdin")
    options = parser.parse_args()
    path = Path(options.socket) if options.socket else default_socket()
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
