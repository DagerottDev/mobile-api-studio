"""Focused loopback check: python3 scripts/check-proxy-rules.py (mitmdump + openssl)."""
import json
import os
from pathlib import Path
import queue
import shutil
import socket
import socketserver
import ssl
import subprocess
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


def main():
    assert shutil.which("mitmdump") and shutil.which("openssl"), "Install mitmproxy and OpenSSL first"
    events = queue.Queue()
    observed = []
    policy = {"rules": []}

    class Rules(socketserver.StreamRequestHandler):
        def handle(self):
            request = json.loads(self.rfile.readline(8193))
            rules = policy["rules"] if request["method"] == "TLS" else []
            if request["method"] != "TLS" and policy.get("breakpoint"):
                if request["path"] == "/public" and not policy.get("legacy"):
                    rules = [{"id": "edit-url", "action": {"type": "breakpoint", "stage": "request"}}]
                elif request["path"] == "/admin":
                    rules = [{"id": "block-admin", "priority": 0, "action": {"type": "block", "statusCode": 403}}]
            self.wfile.write(b"invalid\n" if policy.get("fail") else json.dumps({"rules": rules}).encode() + b"\n")

    class Origin(BaseHTTPRequestHandler):
        def do_GET(self):
            observed.append(self.path)
            self.send_response(200)
            self.send_header("Content-Length", "2")
            self.end_headers()
            self.wfile.write(b"ok")

        def log_message(self, *_):
            pass

    def wait_event(kind, timeout=10):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            event = events.get(timeout=max(0.01, deadline - time.monotonic()))
            if event.get("type") == kind:
                return event
        raise AssertionError(f"Missing {kind} event")

    with tempfile.TemporaryDirectory(prefix="mas-rule-check-", dir="/private/tmp" if Path("/private/tmp").exists() else None) as directory:
        root = Path(directory)
        cert, key = root / "origin.pem", root / "origin.key"
        subprocess.run(["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "1",
                        "-subj", "/CN=localhost", "-addext", "subjectAltName=DNS:localhost,IP:127.0.0.1",
                        "-keyout", str(key), "-out", str(cert)], check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        origin = ThreadingHTTPServer(("127.0.0.1", 0), Origin)
        origin.daemon_threads = True
        server_tls = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        server_tls.load_cert_chain(cert, key)
        origin.socket = server_tls.wrap_socket(origin.socket, server_side=True)
        threading.Thread(target=origin.serve_forever, daemon=True).start()
        rule_socket = root / "rules.sock"
        rules = socketserver.ThreadingUnixStreamServer(str(rule_socket), Rules)
        rules.daemon_threads = True
        os.chmod(rule_socket, 0o600)
        threading.Thread(target=rules.serve_forever, daemon=True).start()
        with socket.socket() as reservation:
            reservation.bind(("127.0.0.1", 0))
            proxy_port = reservation.getsockname()[1]
        addon = Path(__file__).resolve().parents[1] / "sidecars/mitm-addon/mas_bridge.py"
        process = subprocess.Popen(["mitmdump", "--quiet", "--mode", "regular", "--listen-host", "127.0.0.1",
                                    "--listen-port", str(proxy_port), "--set", f"confdir={root / 'conf'}",
                                    "--set", "connection_strategy=lazy", "--set", f"ssl_verify_upstream_trusted_ca={cert}",
                                    "-s", str(addon)], env={**os.environ, "MAS_RULE_SOCKET": str(rule_socket)},
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        def read_events():
            for line in process.stdout:
                if line.startswith("MAS_EVENT "):
                    events.put(json.loads(line[len("MAS_EVENT "):]))
        threading.Thread(target=read_events, daemon=True).start()
        errors = []
        def drain_errors():
            for line in process.stderr:
                errors.append(line.strip())
        threading.Thread(target=drain_errors, daemon=True).start()
        try:
            wait_event("engine_started")
            client_tls = ssl.create_default_context(cafile=str(cert))
            client_tls.load_verify_locations(cafile=str(root / "conf/mitmproxy-ca-cert.pem"))
            origin_der = ssl.PEM_cert_to_DER_cert(cert.read_text())
            def fetch(path, expected=200):
                with socket.create_connection(("127.0.0.1", proxy_port), timeout=5) as client:
                    authority = f"127.0.0.1:{origin.server_port}"
                    client.sendall(f"CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n\r\n".encode())
                    header = b""
                    while not header.endswith(b"\r\n\r\n"):
                        chunk = client.recv(1)
                        assert chunk, "CONNECT closed"
                        header += chunk
                    assert b"200" in header.split(b"\r\n", 1)[0], header
                    with client_tls.wrap_socket(client, server_hostname="localhost") as secure:
                        certificate = secure.getpeercert(binary_form=True)
                        secure.sendall(f"GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n".encode())
                        response = b""
                        while chunk := secure.recv(4096):
                            response += chunk
                        assert str(expected).encode() in response.split(b"\r\n", 1)[0], response
                        if expected == 200:
                            assert response.endswith(b"ok"), response
                        return certificate
            policy["rules"] = [{"id": "tls-inspect", "action": {"type": "inspect_https", "enabled": True}}]
            assert fetch("/inspect") != origin_der, "HTTPS was not intercepted"
            captured = wait_event("flow_completed")
            assert "tls-inspect" in captured["proxy_rule_ids"], captured
            if "HTTP2" in subprocess.check_output(["curl", "-V"], text=True):
                version = subprocess.check_output(["curl", "--http2", "--noproxy", "", "--proxy", f"http://127.0.0.1:{proxy_port}",
                                                   "--cacert", str(root / "conf/mitmproxy-ca-cert.pem"), "--max-time", "5",
                                                   "--silent", "--show-error", "--output", os.devnull, "--write-out", "%{http_version}",
                                                   f"https://127.0.0.1:{origin.server_port}/http2"], text=True)
                assert version == "2", f"Expected HTTP/2 client negotiation, got {version}"
                wait_event("flow_completed")
                print("PASS: HTTP/2 client negotiation with lazy upstream connection")
            else:
                print("SKIP: curl has no HTTP/2 support; validate HTTP/2 with an enabled client")
            policy["rules"] = [{"id": "tls-bypass", "action": {"type": "inspect_https", "enabled": False}},
                               {"id": "lower-inspect", "action": {"type": "inspect_https", "enabled": True}}]
            assert fetch("/bypass") == origin_der, "First TLS rule did not preserve the origin certificate"
            policy["rules"] = []
            assert fetch("/disabled") != origin_der, "Disabled TLS rules did not restore ordinary inspection"
            wait_event("flow_completed")
            stop_decisions = threading.Event()
            def edit_breakpoints():
                while not stop_decisions.wait(0.02):
                    for pending in (root / "conf/breakpoints/pending").glob("*.json"):
                        envelope = json.loads(pending.read_text())
                        decision = {"action": "continue", "url": f"https://127.0.0.1:{origin.server_port}/admin"}
                        if policy.get("invalid_header"):
                            decision["headers"] = [{"name": "bad header", "value": "value"}]
                        destination = root / "conf/breakpoints/decisions" / pending.name
                        destination.parent.mkdir(parents=True, exist_ok=True)
                        temporary = destination.with_suffix(".tmp")
                        temporary.write_text(json.dumps(decision))
                        temporary.replace(destination)
            decisions = threading.Thread(target=edit_breakpoints, daemon=True)
            decisions.start()
            try:
                policy["breakpoint"] = True
                fetch("/public", expected=403)
                edited = wait_event("flow_completed")
                assert "block-admin" in edited["proxy_rule_ids"], edited
                assert "/admin" not in observed, "Edited request bypassed the terminal block"
                policy["legacy"] = True
                (root / "conf/mock-rules.json").write_text(json.dumps({"enabled": True, "rules": [{"id": "legacy-edit", "name": "Legacy edit", "enabled": True, "priority": 5, "createdAt": "1", "pathPattern": "/public", "requestBreakpoint": True}]}))
                fetch("/public", expected=403)
                edited = wait_event("flow_completed")
                assert "block-admin" in edited["proxy_rule_ids"] and not edited.get("mock_rule_id"), edited
                policy["invalid_header"] = True
                try:
                    fetch("/public")
                except (ssl.SSLError, OSError, AssertionError):
                    pass
                else:
                    raise AssertionError("Invalid breakpoint header was accepted")
                invalid = wait_event("mock_rules_failed")
                assert invalid["code"] == "breakpoint_decision_invalid", invalid
                print("PASS: proxy/mock breakpoint edits re-match terminal rules; invalid decisions stop the flow")
            finally:
                stop_decisions.set()
                decisions.join(timeout=2)
                policy.pop("breakpoint", None)
                policy.pop("legacy", None)
                policy.pop("invalid_header", None)
            before = len(observed)
            policy["fail"] = True
            try:
                fetch("/must-stop")
            except (ssl.SSLError, OSError):
                pass
            else:
                raise AssertionError("Malformed rule IPC did not stop TLS")
            assert len(observed) == before, "Failed policy reached the origin HTTP handler"
            failure = wait_event("proxy_rules_failed")
            assert failure["code"] == "proxy_rule_tls_failed", failure
            assert process.poll() is None, "One policy failure stopped the whole listener"
            print("PASS: live TLS inspection, ordered passthrough, disable restoration, fail-closed IPC, listener readiness")
        finally:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
            rules.shutdown()
            rules.server_close()
            origin.shutdown()
            origin.server_close()
            if errors:
                print("mitmdump diagnostics:", "\n".join(errors[-5:]))


if __name__ == "__main__":
    main()
