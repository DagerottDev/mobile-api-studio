#!/usr/bin/env python3
"""Check a portable bundle from outside its source checkout using disposable data."""
import argparse
import json
import os
from pathlib import Path
import re
import socket
import subprocess
import tempfile
import time
import urllib.request


def stop(process):
    if process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=12)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('bundle', type=Path)
    bundle = parser.parse_args().bundle.resolve()
    assert (bundle / 'sidecars/mitm-addon/mas_bridge.py').is_file()
    assert not list(bundle.rglob('*.pyc')), 'Package must not include Python caches'
    with socket.socket() as port_socket:
        port_socket.bind(('127.0.0.1', 0))
        port = port_socket.getsockname()[1]
    temp_parent = os.environ.get('LOCALAPPDATA') if os.name == 'nt' else None
    with tempfile.TemporaryDirectory(prefix='mas-bundle-check-', dir=temp_parent) as directory:
        temporary = Path(directory)
        data = temporary / 'data'
        data.mkdir(mode=0o700)
        launcher = ['powershell', '-NoProfile', '-File', str(bundle / 'start.ps1')] if os.name == 'nt' else [str(bundle / 'start.sh')]
        command = launcher + ['--no-open', '--port', str(port), '--data-dir', str(data)]
        environment = os.environ.copy()
        environment.pop('MAS_UI_DIR', None)
        environment.pop('MAS_ADDON_PATH', None)
        base = f'http://127.0.0.1:{port}'
        with (temporary / 'service.log').open('w+') as log:
            def start():
                process = subprocess.Popen(command, cwd=temporary, env=environment, stdout=log, stderr=subprocess.STDOUT)
                for _ in range(150):
                    try:
                        with urllib.request.urlopen(base + '/api/session', timeout=0.2) as response:
                            token = json.load(response)['token']
                        return process, token
                    except (OSError, ValueError):
                        if process.poll() is not None:
                            raise AssertionError('Packaged service exited before becoming ready')
                        time.sleep(0.1)
                stop(process)
                raise AssertionError('Packaged service did not become ready')

            process, token = start()
            try:
                with urllib.request.urlopen(base + '/', timeout=2) as response:
                    html = response.read().decode()
                assert '<div id="root"></div>' in html
                assets = re.findall(r'(?:src|href)="(/assets/[^\"]+)"', html)
                assert assets
                for asset in assets:
                    with urllib.request.urlopen(base + asset, timeout=2) as response:
                        assert response.read() == (bundle / 'ui' / asset.lstrip('/')).read_bytes()

                cli = ['python3' if os.name != 'nt' else 'python', str(bundle / 'cli/mas-cli.py')]
                if os.name != 'nt':
                    cli += ['--socket', str(data / 'control/socket')]
                for name in ['health', 'capture_platform_info', 'list_desktop_processes', 'list_lan_interfaces']:
                    result = subprocess.run(cli + [name], input=b'{}', capture_output=True, check=True, timeout=15, cwd=temporary)
                    value = json.loads(result.stdout)
                    assert not (isinstance(value, dict) and 'error' in value)
                    if name == 'capture_platform_info':
                        assert value['localCapture'] is True

                # A second service cannot take this workspace or its active control endpoint.
                duplicate = subprocess.Popen(command, cwd=temporary, env=environment, stdout=log, stderr=subprocess.STDOUT)
                try:
                    assert duplicate.wait(timeout=10) != 0
                finally:
                    stop(duplicate)
                process.terminate()
                assert process.wait(timeout=12) == 0
                if os.name != 'nt':
                    assert not (data / 'control/socket').exists()

                # Idle forced-stop recovery exercises kernel lock release and stale endpoint handling.
                process, token = start()
                process.kill()
                process.wait(timeout=5)
                process, token = start()
                print('Portable bundle passed: external working directory, exact UI assets, CLI discovery/authentication, exclusive workspace, graceful shutdown, and idle forced-stop restart.')
            finally:
                stop(process)


if __name__ == '__main__':
    main()
