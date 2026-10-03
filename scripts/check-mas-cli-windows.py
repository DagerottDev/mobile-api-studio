#!/usr/bin/env python3
"""Exercise Windows pipe peer checks with mocked Win32 calls on any host."""

import ctypes
import importlib.util
import sys
from ctypes import wintypes
from pathlib import Path
from unittest.mock import patch

cli_path = Path(__file__).with_name("mas-cli.py")
spec = importlib.util.spec_from_file_location("mas_cli_windows_check", cli_path)
cli = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = cli
spec.loader.exec_module(cli)


def fake_kernel32():
    api = type("Kernel32", (), {})()
    def set_process_id(_handle, pid):
        ctypes.cast(pid, ctypes.POINTER(wintypes.ULONG))[0] = 42
        return True

    api.GetCurrentProcess = lambda: 1
    api.OpenProcess = lambda *_: 222
    api.GetNamedPipeServerProcessId = set_process_id
    api.WaitNamedPipeW = lambda *_: True
    api.CreateFileW = lambda *_: 111
    api.CreateEventW = lambda *_: 444
    api.ReadFile = lambda *_: True
    api.WriteFile = lambda *_: True
    api.GetOverlappedResult = lambda *_: True
    api.WaitForSingleObject = lambda *_: 0
    api.CancelIoEx = lambda *_: True
    api.CloseHandle = lambda *_: True
    return api


def fake_advapi32():
    api = type("Advapi32", (), {})()

    def open_process_token(_process, _access, token):
        ctypes.cast(token, ctypes.POINTER(wintypes.HANDLE))[0] = wintypes.HANDLE(333)
        return True

    api.OpenProcessToken = open_process_token
    api.GetTokenInformation = lambda *_: True
    return api


sent = []
request = b'{"command":"health","args":{}}\n'
kernel = fake_kernel32()
advapi = fake_advapi32()
loaded = {}

def load_dll(name, **_kwargs):
    loaded[name] = kernel if name == "kernel32" else advapi
    return loaded[name]

with patch.object(cli.os, "name", "nt"), patch.object(cli.ctypes, "WinDLL", side_effect=load_dll, create=True):
    actual_kernel, actual_advapi = cli._windows_apis()
assert actual_kernel is kernel and actual_advapi is advapi
assert "OpenProcessToken" not in vars(kernel) and "GetTokenInformation" not in vars(kernel)
assert "OpenProcessToken" in vars(advapi) and "GetTokenInformation" in vars(advapi)

with patch.object(cli, "_windows_apis", return_value=(kernel, advapi)), \
     patch.object(cli, "current_user_sid", return_value="S-1-current"), \
     patch.object(cli, "_token_sid", return_value="S-1-other"), \
     patch.object(cli, "_pipe_io", side_effect=lambda _api, _handle, *, data=None, size=0, deadline: sent.append(data) or (len(data) if data is not None else b"{}\n")):
    try:
        cli.windows_pipe_call(r"\\.\pipe\explicit-user-pipe", request)
    except RuntimeError as error:
        assert "another user" in str(error)
    else:
        raise AssertionError("a different user's pipe was accepted")
assert sent == [], "request bytes were written before verifying the server SID"
with patch.object(cli.os, "name", "nt"), patch.object(cli, "current_user_sid", return_value="S-1-current"):
    assert cli.default_socket() == r"\\.\pipe\mobile-api-studio-control-S-1-current"

kernel = fake_kernel32()
advapi = fake_advapi32()
sids = iter(["S-1-current", "S-1-current"])
sent.clear()
with patch.object(cli, "_windows_apis", return_value=(kernel, advapi)), \
     patch.object(cli, "current_user_sid", side_effect=lambda: next(sids)), \
     patch.object(cli, "_token_sid", return_value="S-1-current"), \
     patch.object(cli, "_pipe_io", side_effect=lambda _api, _handle, *, data=None, size=0, deadline: sent.append(data) or (len(data) if data is not None else b'{"ok":true}\n')):
    assert cli.windows_pipe_call(r"\\.\pipe\explicit-user-pipe", request) == b'{"ok":true}\n'
assert sent == [request, None]

print("mas-cli Windows peer authentication checks passed")
