#!/usr/bin/env python3
"""Exercise capture-child cleanup after its owning service is forcibly killed.

Uses only loopback, a temporary data directory, and the binaries from a portable bundle.
"""
import argparse
import json
import os
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import time
import urllib.request
from pathlib import Path


def free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def listener_open(port):
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=0.3):
            return True
    except OSError:
        return False


def process_info(pid):
    result = subprocess.run(["ps", "-p", str(pid), "-o", "stat=,command="],
                            text=True, capture_output=True)
    if result.returncode:
        return None
    line = result.stdout.strip()
    if not line:
        return None
    state, _, command = line.partition(" ")
    return state, command


def wait_http(port, timeout=20):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            with urllib.request.urlopen(f"http://127.0.0.1:{port}/healthz", timeout=0.5) as response:
                if response.read() == b"ok":
                    return
        except Exception:
            time.sleep(0.1)
    raise RuntimeError("service health endpoint did not become ready")


def invoke(cli, socket_path, command, args=None, env=None, timeout=40):
    result = subprocess.run([sys.executable, str(cli), "--socket", str(socket_path), command],
                            input=json.dumps(args or {}).encode(), stdout=subprocess.PIPE,
                            stderr=subprocess.PIPE, timeout=timeout, env=env)
    if result.returncode not in (0, 1):
        raise RuntimeError(f"{command} exited unexpectedly: {result.stderr.decode(errors='replace')[:300]}")
    try:
        return json.loads(result.stdout)
    except ValueError as error:
        raise RuntimeError(f"{command} returned invalid JSON") from error


def start_service(binary, data_dir, port, log_path, env):
    log = open(log_path, "ab", buffering=0)
    proc = subprocess.Popen([str(binary), "--port", str(port), "--no-open", "--data-dir", str(data_dir)],
                            env=env, stdout=log, stderr=subprocess.STDOUT)
    proc._test_log = log
    return proc


def child_mitmdump(service_pid, addon, data_dir, port):
    result = subprocess.run(["pgrep", "-P", str(service_pid)], text=True, capture_output=True)
    for raw_pid in result.stdout.split():
        pid = int(raw_pid)
        info = process_info(pid)
        if info is None:
            continue
        state, command = info
        if (state[0] != "Z" and str(addon) in command and f"--listen-port {port}" in command
                and str(data_dir) in command):
            return pid, command
    raise RuntimeError("could not identify the owned mitmdump child")


def wait_child_stopped(pid, port, timeout=8):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        info = process_info(pid)
        if (info is None or info[0].startswith("Z")) and not listener_open(port):
            return
        time.sleep(0.1)
    info = process_info(pid)
    raise RuntimeError(f"owned capture child remained active; state={info[0] if info else 'gone'} listener={listener_open(port)}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bundle", type=Path, required=True, help="path to the unpacked portable bundle")
    args = parser.parse_args()
    bundle = args.bundle.resolve()
    binary = bundle / "bin" / "mobile-api-studio-server"
    addon = bundle / "sidecars" / "mitm-addon" / "mas_bridge.py"
    cli = bundle / "cli" / "mas-cli.py"
    for path in (binary, addon, cli):
        if not path.is_file():
            raise SystemExit(f"bundle is missing {path}")

    env = os.environ.copy()
    env["MAS_UI_DIR"] = str(bundle / "ui")
    env["MAS_ADDON_PATH"] = str(addon)
    root = Path(tempfile.mkdtemp(prefix="mas-capture-watchdog-"))
    (root / "upstream").mkdir()
    (root / "upstream" / "fixture.txt").write_text("synthetic loopback upstream\n")
    data_dir = root / "data"
    upstream_port, proxy_port, service_port = free_port(), free_port(), free_port()
    while proxy_port in (upstream_port, service_port):
        proxy_port = free_port()
    while service_port == upstream_port:
        service_port = free_port()
    socket_path = data_dir / "control" / "socket"
    upstream_log = open(root / "upstream.log", "ab", buffering=0)
    upstream = subprocess.Popen([sys.executable, "-m", "http.server", str(upstream_port), "--bind", "127.0.0.1",
                                 "--directory", str(root / "upstream")], stdout=upstream_log,
                                stderr=subprocess.STDOUT)
    services = []
    owned_child = None
    try:
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline and not listener_open(upstream_port):
            time.sleep(0.05)
        if not listener_open(upstream_port):
            raise RuntimeError("synthetic loopback upstream did not start")

        first = start_service(binary, data_dir, service_port, root / "service-1.log", env)
        services.append(first)
        wait_http(service_port)
        target = {"target": {"schemaVersion": 1, "type": "proxy_listener",
                  "mode": {"type": "reverse_proxy", "url": f"http://127.0.0.1:{upstream_port}"},
                  "listenPort": proxy_port}, "sessionName": "Disposable parent-watchdog probe"}
        response = invoke(cli, socket_path, "connect_capture_target", target, env=env)
        if not response.get("connection", {}).get("connected"):
            raise RuntimeError("synthetic capture did not become active")
        owned_child, command = child_mitmdump(first.pid, addon, data_dir, proxy_port)
        with urllib.request.urlopen(f"http://127.0.0.1:{proxy_port}/fixture.txt", timeout=5) as response:
            if response.read() != b"synthetic loopback upstream\n":
                raise RuntimeError("reverse proxy returned unexpected fixture")
        print(f"active_capture=ok service_pid={first.pid} owned_child_pid={owned_child}")

        os.kill(first.pid, signal.SIGKILL)
        first.wait(timeout=5)
        first._test_log.close()
        wait_child_stopped(owned_child, proxy_port)
        print("after_service_sigkill=owned_child_stopped proxy_listener_closed")

        second = start_service(binary, data_dir, service_port, root / "service-2.log", env)
        services.append(second)
        wait_http(service_port)
        snapshot = invoke(cli, socket_path, "current_connection", env=env)
        if snapshot.get("connected") or snapshot.get("capture_running"):
            raise RuntimeError("service restart unexpectedly reports prior capture active")
        response = invoke(cli, socket_path, "connect_capture_target", target, env=env)
        if not response.get("connection", {}).get("connected"):
            raise RuntimeError(f"capture did not reconnect after restart: {response!r}")
        with urllib.request.urlopen(f"http://127.0.0.1:{proxy_port}/fixture.txt", timeout=5) as response:
            if response.read() != b"synthetic loopback upstream\n":
                raise RuntimeError("reconnected proxy returned unexpected fixture")
        invoke(cli, socket_path, "disconnect_device", env=env)
        print("restart_reconnect=ok reverse_proxy=ok disconnect=ok")
    finally:
        for service in reversed(services):
            if service.poll() is None:
                service.terminate()
                try:
                    service.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    service.kill()
                    service.wait(timeout=5)
            service._test_log.close()
        # Emergency cleanup is limited to the child identified beneath our service and
        # matching this test's addon, private data directory, and selected proxy port.
        if owned_child is not None:
            info = process_info(owned_child)
            if info is not None and not info[0].startswith("Z"):
                command = info[1]
                if str(addon) in command and str(data_dir) in command and f"--listen-port {proxy_port}" in command:
                    os.kill(owned_child, signal.SIGTERM)
                    deadline = time.monotonic() + 3
                    while time.monotonic() < deadline and process_info(owned_child):
                        time.sleep(0.1)
                    info = process_info(owned_child)
                    if info is not None and not info[0].startswith("Z"):
                        os.kill(owned_child, signal.SIGKILL)
        if upstream.poll() is None:
            upstream.terminate()
            try:
                upstream.wait(timeout=3)
            except subprocess.TimeoutExpired:
                upstream.kill()
                upstream.wait(timeout=3)
        upstream_log.close()
        if listener_open(proxy_port):
            raise RuntimeError("test proxy port remains open after exact-owned-child cleanup")
        shutil.rmtree(root)
        print("cleanup=complete disposable_data_removed=true")


if __name__ == "__main__":
    main()
