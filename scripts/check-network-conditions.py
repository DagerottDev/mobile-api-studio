"""Run after cargo build -p mobile-api-studio-server; disposable loopback data only."""
import concurrent.futures
import http.client
import http.server
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import threading
import time
import urllib.request


def free_port():
    with socket.socket() as source:
        source.bind(('127.0.0.1', 0))
        return source.getsockname()[1]


class Origin(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        self.rfile.read(int(self.headers.get('content-length', 0)))
        self.send_response(200)
        self.send_header('content-length', '32768')
        self.end_headers()
        self.wfile.write(b'x' * 32768)

    def log_message(self, *_):
        pass


with tempfile.TemporaryDirectory(prefix='mas-network-check-') as temporary:
    root = Path(__file__).resolve().parents[1]
    origin = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Origin)
    threading.Thread(target=origin.serve_forever, daemon=True).start()
    service_port, capture_port = free_port(), free_port()
    while capture_port == service_port: capture_port = free_port()
    binary = os.environ.get('MAS_SERVER_BINARY', str(root / 'target/debug/mobile-api-studio-server'))
    with open(Path(temporary) / 'service.log', 'w+') as log:
        service = subprocess.Popen([binary, '--port', str(service_port), '--no-open', '--data-dir', temporary], cwd=root, stdout=log, stderr=subprocess.STDOUT)
        token = None
        base = f'http://127.0.0.1:{service_port}'
        for _ in range(100):
            try:
                with urllib.request.urlopen(base + '/api/session', timeout=0.2) as response:
                    token = json.load(response)['token']
                break
            except (OSError, ValueError):
                time.sleep(0.1)
        assert token is not None, 'Local service did not become ready'

        with urllib.request.urlopen(base + '/network', timeout=2) as response:
            assert response.status == 200

        def invoke(command, **args):
            request = urllib.request.Request(base + '/api/invoke', data=json.dumps({'command': command, 'args': args}).encode(), headers={'Content-Type': 'application/json', 'X-MAS-Token': token}, method='POST')
            with urllib.request.urlopen(request, timeout=15) as response:
                return json.load(response)

        def profile(**changes):
            value = dict(schemaVersion=1, id='measured', name='Measured loopback', enabled=True, priority=0, scope={'type': 'global'}, latencyMs=200, jitterMs=0, uploadBytesPerSecond=65536, downloadBytesPerSecond=65536, offline=False, failurePercent=0, createdAt='1', updatedAt='1')
            value.update(changes)
            invoke('upsert_network_profile', profile=value)
            return value

        def transfer():
            started = time.monotonic()
            client = http.client.HTTPConnection('127.0.0.1', capture_port, timeout=15)
            try:
                client.request('POST', '/measure', b'u' * 32768, {'Content-Type': 'application/octet-stream'})
                response = client.getresponse()
                assert response.status == 200 and response.read() == b'x' * 32768
                return time.monotonic() - started
            finally:
                client.close()

        connected = False
        try:
            invoke('connect_capture_target', target={'schemaVersion': 1, 'type': 'proxy_listener', 'mode': {'type': 'reverse_proxy', 'url': f'http://127.0.0.1:{origin.server_port}'}, 'listenPort': capture_port}, sessionName='Network measurement')
            connected = True
            baseline = transfer()
            profile()
            measured = transfer()
            assert 1.05 <= measured - baseline <= 1.65, (baseline, measured)
            # Jitter is additive around latency, with a nonnegative floor.
            profile(latencyMs=300, jitterMs=50, uploadBytesPerSecond=None, downloadBytesPerSecond=None)
            samples = [transfer() - baseline for _ in range(3)]
            assert all(0.18 <= sample <= 0.55 for sample in samples), samples
            # Disable-all releases an already waiting request and prevents new delay.
            profile(latencyMs=10000, jitterMs=0, uploadBytesPerSecond=None, downloadBytesPerSecond=None)
            with concurrent.futures.ThreadPoolExecutor(max_workers=1) as pool:
                pending = pool.submit(transfer)
                time.sleep(0.3)
                assert invoke('disable_all_network_profiles') == 1
                released = pending.result(timeout=3)
            assert released < 1.5, released
            assert transfer() < baseline + 0.3
            # Both kinds of failure stop forwarding and persist an explicit diagnostic.
            for settings, code in [({'offline': True}, 'network_profile_offline'), ({'failurePercent': 100}, 'network_profile_failure')]:
                profile(latencyMs=0, uploadBytesPerSecond=None, downloadBytesPerSecond=None, **settings)
                try:
                    transfer()
                    raise AssertionError('Expected request failure')
                except (http.client.HTTPException, ConnectionError, OSError):
                    pass
                for _ in range(30):
                    flows = invoke('list_flows')
                    details = [invoke('get_flow_detail', flowId=flow['id']) for flow in flows]
                    if any(detail and detail.get('errorCode') == code for detail in details): break
                    time.sleep(0.1)
                assert any(detail and detail.get('errorCode') == code for detail in details), code
            invoke('disable_all_network_profiles')
            print(f'Network conditions passed: baseline={baseline:.3f}s; 64 KiB/s up/down +200ms={measured:.3f}s; jitter samples={[round(value, 3) for value in samples]}; disable released={released:.3f}s; offline/failure diagnostics persisted.')
        finally:
            if connected:
                invoke('disconnect_device')
            service.terminate()
            try: service.wait(timeout=8)
            except subprocess.TimeoutExpired:
                service.kill(); service.wait()
            origin.shutdown(); origin.server_close()
