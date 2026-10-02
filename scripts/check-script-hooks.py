"""Run after building the server and script worker; uses only disposable loopback data."""
import base64
import hashlib
import http.client
import http.server
import json
import os
from pathlib import Path
import socket
import struct
import subprocess
import tempfile
import threading
import time
import urllib.request


def free_port():
    with socket.socket() as source:
        source.bind(("127.0.0.1", 0))
        return source.getsockname()[1]


def exact_read(stream, size):
    data = bytearray()
    while len(data) < size:
        chunk = stream.read(size - len(data))
        if not chunk:
            raise EOFError("WebSocket closed mid-frame")
        data.extend(chunk)
    return bytes(data)


def websocket_frame(opcode, payload, masked):
    first = 0x80 | opcode
    length = len(payload)
    if length < 126:
        header = bytes((first, (0x80 if masked else 0) | length))
    elif length < 65536:
        header = bytes((first, (0x80 if masked else 0) | 126)) + struct.pack("!H", length)
    else:
        header = bytes((first, (0x80 if masked else 0) | 127)) + struct.pack("!Q", length)
    if not masked:
        return header + payload
    mask = os.urandom(4)
    return header + mask + bytes(byte ^ mask[index % 4] for index, byte in enumerate(payload))


class Origin(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def do_POST(self):
        if self.headers.get("transfer-encoding", "").lower() == "chunked":
            chunks = []
            while True:
                size = int(self.rfile.readline().split(b";", 1)[0].strip(), 16)
                if not size:
                    while self.rfile.readline() not in (b"\r\n", b"\n"):
                        pass
                    break
                chunks.append(exact_read(self.rfile, size))
                exact_read(self.rfile, 2)
            body = b"".join(chunks)
        else:
            body = self.rfile.read(int(self.headers.get("content-length", 0)))
        with self.server.lock:
            self.server.requests.append((self.path, dict(self.headers), body))
        response = b"origin:" + body
        self.send_response(200)
        self.send_header("content-type", "text/plain")
        self.send_header("content-length", str(len(response)))
        self.end_headers()
        self.wfile.write(response)

    def do_GET(self):
        if self.path != "/ws":
            self.send_error(404)
            return
        key = self.headers.get("Sec-WebSocket-Key")
        accept = base64.b64encode(hashlib.sha1((key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").encode()).digest()).decode()
        self.send_response(101, "Switching Protocols")
        self.send_header("Upgrade", "websocket")
        self.send_header("Connection", "Upgrade")
        self.send_header("Sec-WebSocket-Accept", accept)
        self.end_headers()
        self.wfile.flush()
        try:
            while True:
                first, second = exact_read(self.rfile, 2)
                opcode = first & 0x0F
                length = second & 0x7F
                if length == 126:
                    length = struct.unpack("!H", exact_read(self.rfile, 2))[0]
                elif length == 127:
                    length = struct.unpack("!Q", exact_read(self.rfile, 8))[0]
                mask = exact_read(self.rfile, 4) if second & 0x80 else None
                payload = exact_read(self.rfile, length)
                if mask:
                    payload = bytes(byte ^ mask[index % 4] for index, byte in enumerate(payload))
                if opcode == 8:
                    self.connection.sendall(websocket_frame(8, payload, False))
                    return
                with self.server.lock:
                    self.server.websocket_frames.append((opcode, payload))
                self.connection.sendall(websocket_frame(opcode, payload, False))
        except (EOFError, OSError):
            return

    def log_message(self, *_):
        pass


with tempfile.TemporaryDirectory(prefix="mas-script-hooks-") as temporary:
    root = Path(__file__).resolve().parents[1]
    origin = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Origin)
    origin.daemon_threads = True
    origin.lock = threading.Lock()
    origin.requests = []
    origin.websocket_frames = []
    threading.Thread(target=origin.serve_forever, daemon=True).start()
    service_port, capture_port = free_port(), free_port()
    while capture_port == service_port:
        capture_port = free_port()
    binary = os.environ.get("MAS_SERVER_BINARY", str(root / "target/debug/mobile-api-studio-server"))
    worker = Path(binary).with_name("mobile-api-studio-script-worker")
    assert Path(binary).is_file(), f"Build the service first: {binary}"
    assert worker.is_file(), f"Build the script worker first: {worker}"

    with open(Path(temporary) / "service.log", "w+") as log:
        service = subprocess.Popen([binary, "--port", str(service_port), "--no-open", "--data-dir", temporary], cwd=root, stdout=log, stderr=subprocess.STDOUT)
        base = f"http://127.0.0.1:{service_port}"
        token = None
        for _ in range(100):
            try:
                with urllib.request.urlopen(base + "/api/session", timeout=0.2) as response:
                    token = json.load(response)["token"]
                break
            except (OSError, ValueError):
                time.sleep(0.1)
        assert token is not None, "Local service did not become ready"

        def invoke(command, **args):
            request = urllib.request.Request(base + "/api/invoke", data=json.dumps({"command": command, "args": args}).encode(), headers={"Content-Type": "application/json", "X-MAS-Token": token}, method="POST")
            with urllib.request.urlopen(request, timeout=15) as response:
                return json.load(response)

        def rule(rule_id, path, stage, script, priority=0):
            value = {
                "schemaVersion": 1,
                "id": rule_id,
                "name": rule_id,
                "enabled": True,
                "priority": priority,
                "matcher": {"method": None, "host": {"kind": "wildcard", "value": "*"}, "path": {"kind": "exact", "value": path}},
                "action": {"type": "script_hook", "stage": stage, "script": script},
                "createdAt": "1",
                "updatedAt": "1",
            }
            invoke("upsert_proxy_rule", rule=value)

        def request(path, body=b"original", timeout=15):
            client = http.client.HTTPConnection("127.0.0.1", capture_port, timeout=timeout)
            try:
                client.request("POST", path, body, {"Content-Type": "text/plain", "Connection": "close"})
                response = client.getresponse()
                return response.status, {name.lower(): value for name, value in response.getheaders()}, response.read()
            finally:
                client.close()

        def wait_for(predicate, message):
            for _ in range(60):
                if predicate():
                    return
                time.sleep(0.1)
            raise AssertionError(message)

        def script_failure(path, prior_diagnostic_count):
            diagnostics = invoke("list_proxy_rule_diagnostics")
            flows = invoke("list_flows")
            detail = next((invoke("get_flow_detail", flowId=flow["id"]) for flow in flows if flow.get("path") == path), None)
            return (
                len(diagnostics) > prior_diagnostic_count
                and any(item.get("code") == "script_hook_failed" and item.get("message") == "Proxy rule execution failed; the affected flow was stopped." for item in diagnostics)
                and detail is not None
                and detail.get("errorCode") == "script_hook_failed"
                and detail.get("errorMessage") == "Script hook failed; matching flow stopped."
            )

        connected = False
        try:
            invoke("connect_capture_target", target={"schemaVersion": 1, "type": "proxy_listener", "mode": {"type": "reverse_proxy", "url": f"http://127.0.0.1:{origin.server_port}"}, "listenPort": capture_port}, sessionName="Script hook check")
            connected = True

            request_body = base64.b64encode(b"rewritten request").decode()
            response_body = base64.b64encode(b"rewritten response").decode()
            rule("script-request", "/hook", "request", f"function transform(e) {{ e.headers.push({{name:'x-script-request',value:'yes'}}); e.body={{dataBase64:'{request_body}',contentType:'text/plain',isBinary:false,isTruncated:false}}; return e; }}")
            rule("script-response", "/hook", "response", f"function transform(e) {{ e.statusCode=202; e.headers.push({{name:'x-script-response',value:'yes'}}); e.body={{dataBase64:'{response_body}',contentType:'text/plain',isBinary:false,isTruncated:false}}; return e; }}")
            status, headers, body = request("/hook", b"before")
            with origin.lock:
                seen = next(item for item in origin.requests if item[0] == "/hook")
            assert next(value for name, value in seen[1].items() if name.lower() == "x-script-request") == "yes", seen[1]
            assert seen[2] == b"rewritten request", (seen[1], seen[2])
            assert (status, body) == (202, b"rewritten response"), (status, body)
            assert headers.get("x-script-response") == "yes", headers

            large_body = b"XYZ" * 43691
            rule("script-large", "/large", "response", "function transform(e) { e.body={dataBase64:'WFla'.repeat(43691),contentType:'application/octet-stream',isBinary:true,isTruncated:false}; return e; }")
            large_status, _, large_result = request("/large")
            assert large_status == 200 and large_result == large_body, (large_status, len(large_result))

            loop_script = "function transform(e) { while (true) {} }"
            rule("script-loop", "/loop", "request", loop_script)
            prior_diagnostics = len(invoke("list_proxy_rule_diagnostics"))
            try:
                request("/loop", timeout=5)
            except (http.client.HTTPException, OSError):
                pass
            with origin.lock:
                assert not any(item[0] == "/loop" for item in origin.requests), "Infinite-loop hook forwarded the request"
            wait_for(lambda: script_failure("/loop", prior_diagnostics), "Infinite-loop hook did not persist its generic diagnostic and flow error")

            ws_script = """function transform(e) {
                if (e.fromClient && e.opcode === 1) {
                    if (e.body.dataBase64 === 'ZHJvcC1tZQ==') e.dropped = true;
                    else e.body.dataBase64 = 'aG9va2VkLXRleHQ=';
                }
                if (e.fromClient && e.opcode === 2) e.body.dataBase64 = 'BwgJ/w==';
                return e;
            }"""
            rule("script-websocket", "/ws", "websocket", ws_script)
            key = base64.b64encode(os.urandom(16)).decode()
            ws = socket.create_connection(("127.0.0.1", capture_port), timeout=10)
            ws.settimeout(10)
            stream = ws.makefile("rb")
            ws.sendall((f"GET /ws HTTP/1.1\r\nHost: 127.0.0.1:{origin.server_port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n").encode())
            assert stream.readline().startswith(b"HTTP/1.1 101"), "WebSocket handshake failed"
            while stream.readline() not in (b"\r\n", b"\n"):
                pass

            def send_and_receive(opcode, payload, expected):
                ws.sendall(websocket_frame(opcode, payload, True))
                first, second = exact_read(stream, 2)
                length = second & 0x7F
                if length == 126:
                    length = struct.unpack("!H", exact_read(stream, 2))[0]
                elif length == 127:
                    length = struct.unpack("!Q", exact_read(stream, 8))[0]
                assert first & 0x0F == opcode
                assert exact_read(stream, length) == expected

            send_and_receive(1, b"source text", b"hooked-text")
            send_and_receive(2, b"\x00\x01\x02", b"\x07\x08\x09\xff")
            with origin.lock:
                before_drop = len(origin.websocket_frames)
            ws.sendall(websocket_frame(1, b"drop-me", True))
            time.sleep(0.3)
            with origin.lock:
                assert len(origin.websocket_frames) == before_drop, origin.websocket_frames
                assert origin.websocket_frames[:2] == [(1, b"hooked-text"), (2, b"\x07\x08\x09\xff")]
            ws.sendall(websocket_frame(8, struct.pack("!H", 1000), True))
            ws.close()

            prior_diagnostics = len(invoke("list_proxy_rule_diagnostics"))
            for index in range(17):
                rule(f"script-count-{index}", "/count", "request", "function transform(e) { return e; }", priority=index)
            try:
                request("/count", timeout=5)
            except (http.client.HTTPException, OSError):
                pass
            with origin.lock:
                assert not any(item[0] == "/count" for item in origin.requests), "More than sixteen hooks ran for one request stage"
            wait_for(lambda: script_failure("/count", prior_diagnostics), "Script stage cap did not persist its generic diagnostic and flow error")
            print(f"Script hooks passed: request/response edits, {len(large_result)}-byte worker output, timeout failure, WebSocket text/binary transforms and drop, and sixteen-hook stage cap.")
        finally:
            if connected:
                try:
                    invoke("disable_all_proxy_rules")
                    invoke("disconnect_device")
                except (OSError, ValueError):
                    pass
            service.terminate()
            try:
                service.wait(timeout=8)
            except subprocess.TimeoutExpired:
                service.kill()
                service.wait()
            origin.shutdown()
            origin.server_close()
