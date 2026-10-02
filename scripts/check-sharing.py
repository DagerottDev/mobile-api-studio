#!/usr/bin/env python3
"""Exercise sharing through the local UI command boundary using disposable loopback services."""
import base64
import hashlib
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import time
import urllib.error
import urllib.request
import uuid

OWNER = "1" * 64


def free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def stop(process):
    if process is None:
        return
    process.terminate()
    try:
        process.wait(timeout=8)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait()


def get_json(url, timeout=2):
    with urllib.request.urlopen(url, timeout=timeout) as response:
        return response.status, response.read()


def main():
    root = Path(__file__).resolve().parents[1]
    local_binary = Path(os.environ.get("MAS_SERVER_BINARY", root / "target/debug/mobile-api-studio-server"))
    sharing_binary = Path(os.environ.get("MAS_SHARING_SERVER_BINARY", root / "target/debug/mobile-api-studio-sharing-server"))
    assert local_binary.is_file(), f"Missing local service binary: {local_binary}"
    assert sharing_binary.is_file(), f"Missing sharing service binary: {sharing_binary}"

    with tempfile.TemporaryDirectory(prefix="mas-sharing-check-") as temporary:
        local_data = Path(temporary) / "local"
        sharing_data = Path(temporary) / "sharing"
        local_data.mkdir(mode=0o700)
        sharing_data.mkdir(mode=0o700)
        local_port, sharing_port = free_port(), free_port()
        while sharing_port == local_port:
            sharing_port = free_port()
        local_base = f"http://127.0.0.1:{local_port}"
        sharing_origin = f"http://127.0.0.1:{sharing_port}"
        local_log = open(Path(temporary) / "local.log", "w+")
        sharing_log = open(Path(temporary) / "sharing.log", "w+")
        local = sharing = None
        try:
            local = subprocess.Popen(
                [str(local_binary), "--port", str(local_port), "--no-open", "--data-dir", str(local_data)],
                cwd=root, stdout=local_log, stderr=subprocess.STDOUT,
            )
            sharing_env = dict(os.environ, MAS_SHARING_OWNER_TOKEN=OWNER)
            sharing = subprocess.Popen(
                [str(sharing_binary), "--listen", f"127.0.0.1:{sharing_port}", "--data-dir", str(sharing_data)],
                cwd=root, env=sharing_env, stdout=sharing_log, stderr=subprocess.STDOUT,
            )
            local_token = None
            for _ in range(120):
                try:
                    _, payload = get_json(local_base + "/api/session", 0.2)
                    local_token = json.loads(payload)["token"]
                    break
                except (OSError, ValueError, KeyError):
                    if local.poll() is not None:
                        raise AssertionError("Local service exited before becoming ready")
                    time.sleep(0.1)
            assert local_token, "Local service did not become ready"
            for _ in range(120):
                try:
                    get_json(sharing_origin + "/v1/me", 0.2)
                except urllib.error.HTTPError as error:
                    if error.code == 401:
                        break
                    raise
                except OSError:
                    pass
                if sharing.poll() is not None:
                    raise AssertionError("Sharing service exited before becoming ready")
                time.sleep(0.1)

            def invoke(command, **args):
                request = urllib.request.Request(
                    local_base + "/api/invoke",
                    data=json.dumps({"command": command, "args": args}).encode(),
                    headers={"Content-Type": "application/json", "X-MAS-Token": local_token},
                    method="POST",
                )
                try:
                    with urllib.request.urlopen(request, timeout=25) as response:
                        return json.load(response)
                except urllib.error.HTTPError as error:
                    try:
                        detail = json.loads(error.read())
                    except ValueError:
                        detail = {"status": error.code}
                    raise AssertionError(f"{command} failed: {detail}") from error

            def share_request(token, path, method="GET", body=None):
                return invoke("sharing_request", origin=sharing_origin, accessToken=token,
                              path=path, method=method, body=body)

            baseline_flows = invoke("list_flows")
            me = share_request(OWNER, "/v1/me")
            assert me["role"] == "owner"
            assert invoke("list_flows") == baseline_flows, "Signing in changed captured flows"
            assert share_request(OWNER, "/v1/shares")["shares"] == [], "Sign-in uploaded a share"

            request_body = b"synthetic-request-body"
            response_body = b"synthetic-response-body"
            har = {"log": {"version": "1.2", "creator": {"name": "check", "version": "1"}, "entries": [{
                "startedDateTime": "2026-01-01T00:00:00Z", "time": 1,
                "request": {"method": "POST", "url": "https://example.test/path?api_key=synthetic-secret&visible=ok",
                            "httpVersion": "HTTP/1.1", "headers": [], "queryString": [], "cookies": [],
                            "headersSize": -1, "bodySize": len(request_body),
                            "postData": {"mimeType": "text/plain", "text": request_body.decode()}},
                "response": {"status": 200, "statusText": "OK", "httpVersion": "HTTP/1.1", "headers": [],
                             "cookies": [], "content": {"size": len(response_body), "mimeType": "text/plain", "text": response_body.decode()},
                             "redirectURL": "", "headersSize": -1, "bodySize": len(response_body)},
                "cache": {}, "timings": {"send": 0, "wait": 1, "receive": 0},
            }]}}
            interchange = invoke("preview_interchange", format="har", text=json.dumps(har))
            draft = interchange["requests"][0]
            assert draft["method"] == "POST" and "synthetic-secret" in draft["url"]
            parsed_url = urllib.parse.urlsplit(draft["url"])
            request_ref = {"sha256": hashlib.sha256(request_body).hexdigest(), "byteSize": len(request_body),
                           "contentType": "text/plain", "encoding": None, "isBinary": False, "isTruncated": False}
            response_ref = {"sha256": hashlib.sha256(response_body).hexdigest(), "byteSize": len(response_body),
                            "contentType": "text/plain", "encoding": None, "isBinary": False, "isTruncated": False}
            timestamp = str(int(time.time() * 1000))
            session_id, flow_id = "synthetic-session", "synthetic-flow"
            summary = {"schemaVersion": 1, "id": flow_id, "sessionId": None, "source": "fixture",
                       "method": draft["method"], "host": parsed_url.hostname, "path": parsed_url.path,
                       "statusCode": 200, "durationMs": 1, "responseSizeBytes": len(response_body), "startedAt": timestamp}
            detail = {"summary": summary, "request": {"method": draft["method"], "url": draft["url"],
                      "scheme": parsed_url.scheme, "host": parsed_url.hostname, "port": parsed_url.port,
                      "path": parsed_url.path, "query": parsed_url.query, "headers": [], "body": request_ref},
                      "response": {"statusCode": 200, "reason": "OK", "headers": [], "body": response_ref},
                      "timing": {}, "errorCode": None, "errorMessage": None, "proxyRuleIds": [],
                      "proxyRuleChanges": [], "protocol": None}
            workspace = invoke("export_workspace")
            workspace["sessions"].append({"session": {"schemaVersion": 1, "id": session_id, "name": "Synthetic import",
                "status": "completed", "startedAt": timestamp, "endedAt": timestamp, "deviceId": None,
                "appId": None, "connectionStrategy": None, "captureEngine": "har", "notes": None,
                "captureTarget": None, "captureMode": None}, "flows": [{"summary": summary, "detail": detail,
                "requestBodyBase64": base64.b64encode(request_body).decode(),
                "responseBodyBase64": base64.b64encode(response_body).decode()}]})
            imported = invoke("import_workspace", bundle=workspace, mode="merge")
            assert imported["flows"] == 1
            assert len(invoke("list_flows")) == len(baseline_flows) + 1

            flow_ids = [flow_id]
            default_preview = invoke("preview_share_har", flowIds=flow_ids, includeQuery=False, includeBodies=False)
            default_har = json.loads(default_preview["artifact"])
            default_entry = default_har["log"]["entries"][0]
            assert "api_key" not in default_entry["request"]["url"]
            assert "postData" not in default_entry["request"]
            assert "synthetic-response-body" not in default_preview["artifact"]
            explicit = invoke("preview_share_har", flowIds=flow_ids, includeQuery=True, includeBodies=True)
            explicit_har = json.loads(explicit["artifact"])
            explicit_entry = explicit_har["log"]["entries"][0]
            assert "api_key=%3Credacted%3E" in explicit_entry["request"]["url"]
            assert "synthetic-secret" not in explicit["artifact"]
            assert "synthetic-request-body" in explicit["artifact"] and "synthetic-response-body" in explicit["artifact"]
            assert explicit["sha256"] == hashlib.sha256(explicit["artifact"].encode()).hexdigest()

            created = share_request(OWNER, "/v1/shares", "POST", {
                "artifact": explicit["artifact"], "sha256": explicit["sha256"], "expiresInSeconds": 60,
            })
            assert created["sha256"] == explicit["sha256"]
            public_status, public_bytes = get_json(sharing_origin + created["urlPath"])
            assert public_status == 200 and public_bytes == explicit["artifact"].encode()
            assert hashlib.sha256(public_bytes).hexdigest() == created["sha256"]
            short = share_request(OWNER, "/v1/shares", "POST", {
                "artifact": explicit["artifact"], "sha256": explicit["sha256"], "expiresInSeconds": 1,
            })
            time.sleep(1.2)
            try:
                get_json(sharing_origin + short["urlPath"])
                raise AssertionError("Expired share remained publicly accessible")
            except urllib.error.HTTPError as error:
                assert error.code == 404
            share_request(OWNER, f"/v1/shares/{created['id']}", "DELETE")
            try:
                get_json(sharing_origin + created["urlPath"])
                raise AssertionError("Revoked share remained publicly accessible")
            except urllib.error.HTTPError as error:
                assert error.code == 404

            rule = {"schemaVersion": 1, "id": "synthetic-rule", "name": "Synthetic rule", "enabled": True,
                    "priority": 1, "matcher": {"method": "GET", "host": {"kind": "exact", "value": "example.test"},
                    "path": {"kind": "wildcard", "value": "/team/*"}},
                    "action": {"type": "rewrite_request", "headers": [{"name": "X-Team-Test", "value": "safe", "remove": False}], "body": None},
                    "createdAt": timestamp, "updatedAt": timestamp}
            fixture = {"schemaVersion": 1, "id": "synthetic-fixture", "name": "Synthetic fixture", "statusCode": 200,
                       "responseHeaders": [{"name": "Content-Type", "value": "text/plain", "remove": False}],
                       "responseBody": {"contentType": "text/plain", "encoding": "text", "data": "synthetic fixture body"},
                       "sourceFlowId": flow_id, "createdAt": timestamp, "updatedAt": timestamp}
            invoke("upsert_proxy_rule", rule=rule)
            invoke("upsert_mock_fixture", fixture=fixture)
            exported = invoke("export_team_workspace", ruleIds=[rule["id"]], fixtureIds=[fixture["id"]], includeBodies=False)
            assert len(exported["rules"]) == 1 and len(exported["fixtures"]) == 1
            remote = share_request(OWNER, "/v1/workspace")
            payload = {"schemaVersion": 1, "expectedRevision": remote["revision"],
                       "rules": exported["rules"], "fixtures": exported["fixtures"]}
            pushed = share_request(OWNER, "/v1/workspace", "PUT", payload)
            assert pushed["revision"] == remote["revision"] + 1
            assert len(pushed["rules"]) == 1 and len(pushed["fixtures"]) == 1

            member = share_request(OWNER, "/v1/members", "POST", {"name": "Synthetic viewer", "role": "viewer"})
            viewer_token = member["accessToken"]
            try:
                share_request(viewer_token, "/v1/workspace", "PUT", {
                    "schemaVersion": 1, "expectedRevision": pushed["revision"], "rules": pushed["rules"], "fixtures": pushed["fixtures"],
                })
                raise AssertionError("Viewer unexpectedly wrote the shared workspace")
            except AssertionError as error:
                if str(error).startswith("Viewer unexpectedly"):
                    raise
                assert "sharing_service_error" in str(error), error
            try:
                share_request(OWNER, "/v1/workspace", "PUT", {
                    "schemaVersion": 1, "expectedRevision": remote["revision"], "rules": [], "fixtures": [],
                })
                raise AssertionError("Stale workspace revision unexpectedly succeeded")
            except AssertionError as error:
                if str(error).startswith("Stale workspace"):
                    raise
                assert "sharing_service_error" in str(error), error

            pulled = share_request(OWNER, "/v1/workspace")
            reviewed = invoke("preview_team_workspace", artifact=json.dumps(pulled))
            assert reviewed["rules"][0]["id"] == rule["id"]
            invoke("import_team_workspace", artifact=json.dumps(reviewed))
            local_rules = invoke("list_proxy_rules")
            imported_rule = next(item for item in local_rules if item["id"].startswith("team-") and item["id"].endswith("-rule-0"))
            assert imported_rule["id"] != rule["id"] and imported_rule["enabled"] is False
            assert next(item for item in local_rules if item["id"] == rule["id"])["enabled"] is True
            local_fixtures = invoke("list_mock_fixtures")
            imported_fixture = next(item for item in local_fixtures if item["id"].startswith("team-") and item["id"].endswith("-fixture-0"))
            assert imported_fixture["id"] != fixture["id"] and imported_fixture["sourceFlowId"] is None
            assert next(item for item in local_fixtures if item["id"] == fixture["id"])
            print("Sharing check passed: HAR preview/import, redaction choices, explicit publish and download, expiry/revoke, team push/viewer denial/CAS, and disabled fresh-ID import.")
        finally:
            stop(local)
            stop(sharing)
            local_log.close()
            sharing_log.close()


if __name__ == "__main__":
    import urllib.parse
    main()
