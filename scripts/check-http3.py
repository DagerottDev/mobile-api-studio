import json, os, pathlib, socket, subprocess, tempfile, sys

with tempfile.TemporaryDirectory(prefix='mas-http3-') as tmp:
    root = pathlib.Path(tmp)
    def port():
        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as s:
            s.bind(('127.0.0.1', 0))
            return s.getsockname()[1]
    upstream, proxy = port(), port()
    while proxy == upstream: proxy = port()
    subprocess.run(['openssl', 'req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-subj', '/CN=localhost', '-days', '1', '-keyout', str(root/'key.pem'), '-out', str(root/'cert.pem')], check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    addon = root/'h3_fixture.py'
    addon.write_text('''import asyncio, os, ssl
from aioquic.asyncio import QuicConnectionProtocol, serve, connect
from aioquic.h3.connection import H3_ALPN, H3Connection
from aioquic.h3.events import HeadersReceived, DataReceived
from aioquic.quic.configuration import QuicConfiguration

class Origin(QuicConnectionProtocol):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.http = H3Connection(self._quic)
    def quic_event_received(self, event):
        for item in self.http.handle_event(event):
            if isinstance(item, HeadersReceived):
                self.http.send_headers(item.stream_id, [(b':status', b'200'), (b'content-type', b'text/plain')])
                self.http.send_data(item.stream_id, b'http3-captured', end_stream=True)
                self.transmit()

class Client(QuicConnectionProtocol):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.http = H3Connection(self._quic)
        self.done = asyncio.get_running_loop().create_future()
        self.body = bytearray()
        self.status = None
    def quic_event_received(self, event):
        for item in self.http.handle_event(event):
            if isinstance(item, HeadersReceived): self.status = dict(item.headers).get(b':status')
            if isinstance(item, DataReceived): self.body.extend(item.data)
            if getattr(item, 'stream_ended', False) and not self.done.done(): self.done.set_result(None)

async def check():
    server = None
    try:
        config = QuicConfiguration(is_client=False, alpn_protocols=H3_ALPN)
        config.load_cert_chain(os.environ['H3_CERT'], os.environ['H3_KEY'])
        server = await serve('127.0.0.1', int(os.environ['H3_UPSTREAM']), configuration=config, create_protocol=Origin)
        await asyncio.sleep(0.2)
        client_config = QuicConfiguration(is_client=True, alpn_protocols=H3_ALPN, verify_mode=ssl.CERT_NONE)
        async with connect('127.0.0.1', int(os.environ['H3_PROXY']), configuration=client_config, create_protocol=Client) as client:
            stream = client._quic.get_next_available_stream_id()
            client.http.send_headers(stream, [(b':method', b'GET'), (b':scheme', b'https'), (b':authority', b'localhost'), (b':path', b'/h3')], end_stream=True)
            client.transmit()
            await asyncio.wait_for(client.done, 12)
            assert client.status == b'200' and client.body == b'http3-captured', (client.status, client.body)
            print('H3_CLIENT_OK', flush=True)
            await asyncio.sleep(0.3)
    except Exception as error:
        print('H3_CLIENT_FAILED', repr(error), flush=True)
        raise
    finally:
        if server: server.close()
asyncio.run(check())
''')
    env=os.environ.copy()
    env.update(H3_CERT=str(root/'cert.pem'), H3_KEY=str(root/'key.pem'), H3_UPSTREAM=str(upstream), H3_PROXY=str(proxy), MAS_SESSION_ID='h3-check')
    env.pop('MAS_RULE_SOCKET', None)
    bridge = pathlib.Path(__file__).resolve().parents[1] / 'sidecars/mitm-addon/mas_bridge.py'
    process = subprocess.Popen(['mitmdump', '--mode', f'reverse:http3://127.0.0.1:{upstream}', '--listen-host', '127.0.0.1', '--listen-port', str(proxy), '--set', f'confdir={root}/conf', '--set', 'ssl_insecure=true', '--set', 'connection_strategy=lazy', '-s', str(bridge)], env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    try:
        client = subprocess.run([os.environ.get('HTTP3_PYTHON', sys.executable), str(addon)], env=env, capture_output=True, text=True, timeout=20)
    finally:
        process.terminate()
        output, _ = process.communicate(timeout=8)
    result = subprocess.CompletedProcess(process.args, client.returncode, client.stdout + output, client.stderr)
    events = [json.loads(line[10:]) for line in result.stdout.splitlines() if line.startswith('MAS_EVENT ')]
    completed = [e for e in events if e['type']=='flow_completed']
    if result.returncode or 'H3_CLIENT_OK' not in result.stdout:
        print(result.stdout[-5000:]); print(result.stderr[-1000:])
    assert result.returncode==0 and 'H3_CLIENT_OK' in result.stdout
    assert any(e['protocol']['requestHttpVersion']=='HTTP/3' and e['protocol']['responseHttpVersion']=='HTTP/3' and e['protocol']['clientConnection']['transport']=='udp' for e in completed), completed
    print('HTTP/3 reverse capture: real QUIC request/response, payload and protocol metadata passed.')
