#!/usr/bin/env python3
"""Check Windows owner-monitor identity/lifetime branches without a Windows runtime."""
import ast
import ctypes
from ctypes import wintypes
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

source = Path(__file__).resolve().parents[1] / 'sidecars/mitm-addon/mas_bridge.py'
names = {'_shutdown_if_service_gone', '_watch_service_parent', '_start_service_parent_watchdog'}
tree = ast.parse(source.read_text())
functions = ast.Module(body=[item for item in tree.body if isinstance(item, ast.FunctionDef) and item.name in names], type_ignores=[])


class Function:
    def __init__(self, call):
        self.call = call

    def __call__(self, *args):
        return self.call(*args)


def check(owner_created, should_start):
    closed, shutdown, threads = [], [], []

    def times(handle, created, *_):
        value = ctypes.cast(created, ctypes.POINTER(wintypes.FILETIME)).contents
        value.dwLowDateTime = owner_created if handle == 10 else 200
        value.dwHighDateTime = 0
        return True

    kernel = SimpleNamespace(
        OpenProcess=Function(lambda *_: 10), GetCurrentProcess=Function(lambda: 11),
        GetProcessTimes=Function(times), WaitForSingleObject=Function(lambda *_: 0),
        CloseHandle=Function(lambda handle: closed.append(handle) or True))

    class Thread:
        def __init__(self, target, args, **_):
            self.target, self.args = target, args

        def start(self):
            threads.append(self)

    environment = SimpleNamespace(name='nt', environ={'MAS_SERVICE_PID': '42'},
                                  getppid=lambda: (_ for _ in ()).throw(AssertionError('Windows launchers may be intermediate parents')))
    namespace = {'os': environment, 'ctx': SimpleNamespace(master=SimpleNamespace(shutdown=lambda: shutdown.append(True))),
                 'threading': SimpleNamespace(Thread=Thread), 'time': SimpleNamespace(sleep=lambda _: None)}
    exec(compile(functions, str(source), 'exec'), namespace)
    with patch.object(ctypes, 'WinDLL', return_value=kernel, create=True):
        namespace['_start_service_parent_watchdog']()
    assert bool(threads) == should_start
    if should_start:
        assert closed == [] and shutdown == []
        threads[0].target(*threads[0].args)
    assert closed == [10] and shutdown == [True]


check(100, True)  # Retained owner handle works even with an intermediate console launcher.
check(300, False)  # A recycled service PID created after this child is rejected.
print('Windows parent-monitor identity/lifetime mock checks passed')
