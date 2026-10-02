#!/usr/bin/env python3
"""Focused real HTTP validation; uses an isolated temporary database and synthetic owner token."""
import hashlib
import json
import os
from pathlib import Path
import socket
import sqlite3
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request

binary = Path(sys.argv[1] if len(sys.argv) > 1 else "target/debug/mobile-api-studio-sharing-server").resolve()
owner = "1" * 64  # Synthetic check credential only; never a production bootstrap token.
with tempfile.TemporaryDirectory(prefix="mas-sharing-http-") as temp:
    os.chmod(temp, 0o700)
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        port = sock.getsockname()[1]
    env = dict(os.environ, MAS_SHARING_OWNER_TOKEN=owner)
    proc = subprocess.Popen([str(binary), "--listen", f"127.0.0.1:{port}", "--data-dir", temp,
                             "--desktop-origin", "http://localhost:1420"], env=env,
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    base = f"http://127.0.0.1:{port}"

    def call(method, path, payload=None, token=owner, status=200, extra=None):
        headers = {"Content-Type": "application/json"}
        if token:
            headers["Authorization"] = "Bearer " + token
        headers.update(extra or {})
        request = urllib.request.Request(base + path, method=method, headers=headers,
                                         data=None if payload is None else json.dumps(payload).encode())
        try:
            with urllib.request.urlopen(request, timeout=3) as response:
                code, body, result_headers = response.status, response.read(), response.headers
        except urllib.error.HTTPError as error:
            code, body, result_headers = error.code, error.read(), error.headers
        assert code == status, f"Unexpected HTTP status for {method} API operation: {code}, expected {status}"
        assert "no-store" in result_headers["Cache-Control"]
        return body

    def data(*args, **kwargs):
        return json.loads(call(*args, **kwargs))

    try:
        for attempt in range(80):
            try:
                call("GET", "/v1/me")
                break
            except (OSError, urllib.error.URLError):
                if proc.poll() is not None:
                    raise AssertionError("Service exited during startup") from None
                time.sleep(0.05)
        else:
            raise AssertionError("Service did not start")

        db = sqlite3.connect(Path(temp) / "sharing.sqlite3")
        before = db.execute("SELECT revision,rules,fixtures FROM workspace").fetchall()
        for _ in range(3):
            assert data("GET", "/v1/me")["role"] == "owner"
        assert db.execute("SELECT revision,rules,fixtures FROM workspace").fetchall() == before
        assert db.execute("SELECT count(*) FROM shares").fetchone()[0] == 0
        call("GET", "/v1/me", token=None, status=401)
        call("GET", "/v1/me", token="2" * 64, status=401)
        call("GET", "/v1/me", extra={"Origin": "https://wrong.test"}, status=403)
        assert data("GET", "/v1/me", extra={"Origin": "http://localhost:1420"})["role"] == "owner"

        editor = data("POST", "/v1/members", {"name": "Synthetic editor", "role": "editor"})
        viewer = data("POST", "/v1/members", {"name": "Synthetic viewer", "role": "viewer"})
        et, vt = editor["accessToken"], viewer["accessToken"]
        call("POST", "/v1/members", {"name": "No", "role": "viewer"}, token=et, status=403)
        call("GET", "/v1/members", token=vt, status=403)
        call("PATCH", "/v1/members/owner", {"role": "viewer"}, status=400)
        call("DELETE", "/v1/members/owner", status=400)

        rule = {"schemaVersion": 1, "id": "rule", "name": "Block synthetic", "enabled": False,
                "priority": 1, "matcher": {"method": None, "host": {"kind": "exact", "value": "example.test"},
                                           "path": {"kind": "wildcard", "value": "/*"}},
                "action": {"type": "block", "statusCode": 503}, "createdAt": "1", "updatedAt": "1"}
        fixture = {"schemaVersion": 1, "id": "fixture", "name": "Synthetic response", "statusCode": 200,
                   "responseHeaders": [], "responseBody": {"contentType": "application/json", "encoding": "text", "data": "{}"},
                   "sourceFlowId": None, "createdAt": "1", "updatedAt": "1"}
        workspace = {"schemaVersion": 1, "expectedRevision": 0, "rules": [rule], "fixtures": [fixture]}
        call("PUT", "/v1/workspace", workspace, token=vt, status=403)
        assert data("PUT", "/v1/workspace", workspace, token=et)["revision"] == 1
        assert data("GET", "/v1/workspace", token=vt)["fixtures"] == [fixture]
        call("PUT", "/v1/workspace", workspace, token=et, status=409)
        unsafe = json.loads(json.dumps(workspace))
        unsafe["expectedRevision"] = 1
        unsafe["rules"][0]["action"] = {"type": "script_hook", "stage": "request", "script": "print('bad')"}
        call("PUT", "/v1/workspace", unsafe, status=400)
        unsafe["rules"][0]["action"] = {"type": "block", "statusCode": 503}
        unsafe["rules"][0]["matcher"]["path"] = {"kind": "regex", "value": "["}
        call("PUT", "/v1/workspace", unsafe, status=400)
        assert data("GET", "/v1/workspace")["revision"] == 1

        har = {"log": {"version": "1.2", "creator": {"name": "Mobile API Studio", "version": "0.5"}, "entries": [{
            "startedDateTime": "2026-10-02T00:00:00Z", "time": 1,
            "request": {"method": "GET", "url": "https://example.test/path", "httpVersion": "HTTP/1.1",
                        "headers": [{"name": "Authorization", "value": "<redacted>"}], "cookies": [], "queryString": [],
                        "headersSize": -1, "bodySize": 0},
            "response": {"status": 200, "statusText": "OK", "httpVersion": "HTTP/1.1", "headers": [], "cookies": [],
                         "content": {"size": 2, "mimeType": "application/json", "text": "{}"},
                         "redirectURL": "", "headersSize": -1, "bodySize": 2},
            "cache": {}, "timings": {"send": 0, "wait": 1, "receive": 0}}]}}
        artifact = json.dumps(har, indent=2) + "\n"
        digest = hashlib.sha256(artifact.encode()).hexdigest()
        payload = {"artifact": artifact, "expiresInSeconds": 60, "sha256": digest}
        call("POST", "/v1/shares", payload, token=vt, status=403)
        shared = data("POST", "/v1/shares", payload, token=et)
        assert shared["sha256"] == digest
        downloaded = call("GET", shared["urlPath"], token=None)
        assert downloaded == artifact.encode()
        assert hashlib.sha256(downloaded).hexdigest() == digest
        call("DELETE", "/v1/shares/" + shared["id"], token=vt, status=403)
        call("DELETE", "/v1/shares/" + shared["id"], token=et, status=204)
        call("GET", shared["urlPath"], token=None, status=404)
        expired = data("POST", "/v1/shares", {"artifact": artifact, "expiresInSeconds": 1})
        time.sleep(1.1)
        call("GET", expired["urlPath"], token=None, status=404)
        count = db.execute("SELECT count(*) FROM shares").fetchone()[0]
        for malformed in ["{}", '{"log":', artifact.replace("<redacted>", "raw-secret"),
                          artifact.replace('"name": "Authorization"', '"name": "Authorization", "name": "X-Test"'),
                          artifact.replace("Authorization", "X-Mobile-API-Studio-Request-ID"),
                          artifact.replace("https://example.test/path", "https://user:pass@example.test/path"),
                          artifact.replace("https://example.test/path", "https://example.test/path?token=secret")]:
            call("POST", "/v1/shares", {"artifact": malformed, "expiresInSeconds": 60}, status=400)
        call("POST", "/v1/shares", dict(payload, sha256="0" * 64), status=400)
        assert db.execute("SELECT count(*) FROM shares").fetchone()[0] == count
        assert all(len(row[0]) == 64 and row[0] not in [owner, et, vt] for row in db.execute("SELECT token_hash FROM members"))
        assert all(len(row[0]) == 64 for row in db.execute("SELECT token_hash FROM shares"))
        et2 = data("POST", "/v1/members/" + editor["member"]["userId"] + "/token")["accessToken"]
        call("GET", "/v1/me", token=et, status=401)
        assert data("GET", "/v1/me", token=et2)["role"] == "editor"
        member_share = data("POST", "/v1/shares", payload, token=et2)
        call("DELETE", "/v1/members/" + editor["member"]["userId"], status=204)
        call("GET", "/v1/me", token=et2, status=401)
        call("GET", member_share["urlPath"], token=None, status=404)
        db.close()
        # Inspect only permissions/metadata of generated bootstrap credentials, never their contents.
        bootstrap_dir = Path(temp) / "bootstrap"
        bootstrap_env = dict(os.environ)
        bootstrap_env.pop("MAS_SHARING_OWNER_TOKEN", None)
        bootstrap_args = [str(binary), "--listen", "127.0.0.1:0", "--data-dir", str(bootstrap_dir)]
        modification = None
        for restart in range(2):
            bootstrap = subprocess.Popen(bootstrap_args, env=bootstrap_env, stdout=subprocess.PIPE,
                                         stderr=subprocess.DEVNULL)
            try:
                assert bootstrap.stdout.readline().startswith(b"Sharing service listening on "), "Bootstrap service failed to start"
                token_file = bootstrap_dir / "owner-access-token"
                assert bootstrap_dir.stat().st_mode & 0o777 == 0o700
                assert token_file.stat().st_mode & 0o777 == 0o600
                assert (bootstrap_dir / "sharing.sqlite3").stat().st_mode & 0o777 == 0o600
                assert token_file.stat().st_size == 64
                if restart:
                    assert token_file.stat().st_mtime_ns == modification
                modification = token_file.stat().st_mtime_ns
            finally:
                bootstrap.terminate()
                bootstrap.wait(timeout=5)
        unsafe_dir = Path(temp) / "dangling-database"
        unsafe_dir.mkdir(mode=0o700)
        outside = Path(temp) / "outside-database-must-stay-absent"
        (unsafe_dir / "sharing.sqlite3").symlink_to(outside)
        refused = subprocess.run([str(binary), "--listen", "127.0.0.1:0", "--data-dir", str(unsafe_dir)],
                                 env=bootstrap_env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=5)
        assert refused.returncode != 0
        assert not outside.exists()
        assert not (unsafe_dir / "owner-access-token").exists()
        print("PASS: exact bytes/digest, identity-only sign-in, roles, CAS, expiry/revocation, token rotation, redaction rejection, origin isolation, private one-time bootstrap")
    finally:
        proc.terminate()
        proc.wait(timeout=5)
