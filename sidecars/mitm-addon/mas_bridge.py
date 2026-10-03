import asyncio
import base64
import ipaddress
import json
import mimetypes
import os
import random
import stat
import threading
import time
import weakref
from urllib.parse import urlsplit

from mitmproxy import ctx, dns, exceptions, http, tls
from mitmproxy.proxy.mode_specs import DnsMode

EVENT_PREFIX = "MAS_EVENT "
SESSION_ID = os.environ.get("MAS_SESSION_ID")
MAX_BODY_BYTES = int(os.environ.get("MAS_MAX_BODY_BYTES", str(2 * 1024 * 1024)))
MAX_DECISION_BYTES = 3 * 1024 * 1024
MAX_RULE_RESPONSE_BYTES = 8 * 1024 * 1024
BREAKPOINT_TIMEOUT_MS = int(os.environ.get("MAS_BREAKPOINT_TIMEOUT_MS", "60000"))
BREAKPOINT_POLL_MS = max(25, int(os.environ.get("MAS_BREAKPOINT_POLL_MS", "100")))
SDK_CORRELATION_HEADER = "X-Mobile-API-Studio-Request-Id"
RULE_SOCKET = os.environ.get("MAS_RULE_SOCKET")
RULE_TCP_PORT = os.environ.get("MAS_RULE_TCP_PORT")
RULE_TCP_TOKEN = os.environ.get("MAS_RULE_TCP_TOKEN")
MAX_RULE_REQUEST_BYTES = 8 * 1024
_RULES_MTIME_NS: int | None = None
_RULES_DOCUMENT: dict = {"enabled": True, "rules": []}
_TLS_POLICIES = weakref.WeakKeyDictionary()


def _emit(payload: dict) -> None:
    print(EVENT_PREFIX + json.dumps(payload, separators=(",", ":")), flush=True)


def _shutdown_if_service_gone() -> None:
    try:
        ctx.master.shutdown()
    except BaseException:
        os._exit(1)


def _watch_service_parent(parent_pid: int, windows_handle=None, kernel32=None) -> None:
    if os.name == "nt":
        should_shutdown = False
        try:
            while True:
                result = kernel32.WaitForSingleObject(windows_handle, 0)
                if result == 0:  # WAIT_OBJECT_0
                    should_shutdown = True
                    break
                if result != 0x102:  # WAIT_TIMEOUT
                    should_shutdown = True
                    break
                time.sleep(0.25)
        except BaseException:
            should_shutdown = True
        finally:
            try:
                if not kernel32.CloseHandle(windows_handle):
                    should_shutdown = True
            except BaseException:
                should_shutdown = True
        if should_shutdown:
            _shutdown_if_service_gone()
        return

    try:
        while os.getppid() == parent_pid:
            time.sleep(0.25)
    except BaseException:
        pass
    _shutdown_if_service_gone()


def _start_service_parent_watchdog() -> None:
    raw_pid = os.environ.get("MAS_SERVICE_PID")
    if raw_pid is None:
        return
    windows_handle = None
    kernel32 = None
    try:
        parent_pid = int(raw_pid)
        if parent_pid <= 0:
            raise ValueError("invalid service pid")
        if os.name == "nt":
            import ctypes
            from ctypes import wintypes

            kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
            kernel32.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
            kernel32.OpenProcess.restype = wintypes.HANDLE
            kernel32.WaitForSingleObject.argtypes = [wintypes.HANDLE, wintypes.DWORD]
            kernel32.WaitForSingleObject.restype = wintypes.DWORD
            kernel32.CloseHandle.argtypes = [wintypes.HANDLE]
            kernel32.CloseHandle.restype = wintypes.BOOL
            kernel32.GetCurrentProcess.restype = wintypes.HANDLE
            kernel32.GetProcessTimes.argtypes = [wintypes.HANDLE] + [ctypes.POINTER(wintypes.FILETIME)] * 4
            kernel32.GetProcessTimes.restype = wintypes.BOOL
            windows_handle = kernel32.OpenProcess(0x00101000, False, parent_pid)  # SYNCHRONIZE | QUERY_LIMITED_INFORMATION
            if not windows_handle:
                raise OSError(ctypes.get_last_error(), "cannot monitor service process")

            def created_at(handle):
                values = [wintypes.FILETIME() for _ in range(4)]
                if not kernel32.GetProcessTimes(handle, *(ctypes.byref(value) for value in values)):
                    raise OSError("cannot verify service process identity")
                return (values[0].dwHighDateTime << 32) | values[0].dwLowDateTime

            # Windows console launchers may be intermediate parents. The retained service
            # handle identifies the owner; creation times reject a recycled startup PID.
            if created_at(windows_handle) > created_at(kernel32.GetCurrentProcess()):
                raise RuntimeError("capture service process identity changed")
        elif os.getppid() != parent_pid:
            raise RuntimeError("capture service parent does not match")

        threading.Thread(
            target=_watch_service_parent,
            args=(parent_pid, windows_handle, kernel32),
            name="mas-service-parent-watch",
            daemon=True,
        ).start()
    except BaseException:
        if windows_handle and kernel32:
            kernel32.CloseHandle(windows_handle)
        _shutdown_if_service_gone()


def running() -> None:
    # This hook runs after mitmproxy has successfully started its configured servers.
    _start_service_parent_watchdog()
    _emit({"type": "engine_started"})


async def tls_clienthello(data: tls.ClientHelloData) -> None:
    _TLS_POLICIES.pop(data.context.client, None)
    address = data.context.server.address
    if not address:
        # Outer HTTPS-to-proxy TLS has no destination policy to evaluate yet.
        return
    mode = data.context.client.proxy_mode.type_name
    host = (data.client_hello.sni if mode in ("local", "transparent", "wireguard") else None) or (address[0] if address else data.client_hello.sni) or ""
    try:
        rules = await _proxy_rules_for("TLS", host, "/")
        rule = next((item for item in rules if item["action"].get("type") == "inspect_https"), None)
        if rule is None:
            return
        enabled = rule["action"].get("enabled")
        if not isinstance(enabled, bool):
            raise ValueError("Invalid HTTPS inspection decision")
        if not enabled and data.context.client.transport_protocol != "tcp":
            raise ValueError("Encrypted passthrough requires TCP TLS in this capture engine")
        _TLS_POLICIES[data.context.client] = {"rule": rule}
        data.ignore_connection = not enabled
    except (OSError, ValueError, UnicodeDecodeError, asyncio.TimeoutError, asyncio.LimitOverrunError) as exc:
        _TLS_POLICIES[data.context.client] = {"failed": True}
        data.ignore_connection = False
        data.establish_server_tls_first = False
        _emit({"type": "proxy_rules_failed", "code": "proxy_rule_tls_failed", "message": str(exc)})
        # Stop later TLS addons from changing this failed policy decision.
        raise exceptions.AddonHalt()


def tls_start_client(data: tls.TlsData) -> None:
    if _TLS_POLICIES.get(data.context.client, {}).get("failed"):
        data.ssl_conn = None
        # ScriptLoader precedes TlsConfig. Halting leaves no TLS context, which closes the connection.
        raise exceptions.AddonHalt()


def quic_start_client(data) -> None:
    if _TLS_POLICIES.get(data.context.client, {}).get("failed"):
        data.settings = None
        raise exceptions.AddonHalt()


def _millis(value: float | None) -> int | None:
    if value is None:
        return None
    return max(0, int(value * 1000))


def _headers(headers) -> list[dict]:
    return [{"name": name, "value": value} for name, value in headers.items(multi=True)]


def _bounded(value, limit: int = 1024) -> str | None:
    if value is None:
        return None
    if isinstance(value, bytes):
        return value.decode("utf-8", "replace")[:limit]
    return str(value)[:limit]


def _address(value) -> str | None:
    if not value:
        return None
    host, port = value[:2]
    host = str(host)
    return _bounded(f"[{host}]:{port}" if ":" in host else f"{host}:{port}")


def _certificate(cert) -> dict:
    return {
        "subject": _bounded(", ".join(f"{key}={value}" for key, value in cert.subject)),
        "issuer": _bounded(", ".join(f"{key}={value}" for key, value in cert.issuer)),
        "notBefore": _bounded(cert.notbefore.isoformat()),
        "notAfter": _bounded(cert.notafter.isoformat()),
        "sha256": cert.fingerprint().hex(),
        "subjectAlternativeNames": [_bounded(name.value) for name in list(cert.altnames)[:64]],
    }


def _connection(conn) -> dict | None:
    if conn is None:
        return None
    return {
        "id": _bounded(conn.id),
        "transport": _bounded(conn.transport_protocol),
        "peerAddress": _address(conn.peername),
        "localAddress": _address(conn.sockname),
        "serverAddress": _address(getattr(conn, "address", None)),
        "tlsVersion": _bounded(conn.tls_version),
        "cipher": _bounded(conn.cipher),
        "alpn": _bounded(conn.alpn),
        "sni": _bounded(conn.sni),
        "tlsEstablished": conn.tls_established,
        "startedAt": str(_millis(conn.timestamp_start)) if conn.timestamp_start is not None else None,
        "tlsEstablishedAt": str(_millis(conn.timestamp_tls_setup)) if conn.timestamp_tls_setup is not None else None,
        "endedAt": str(_millis(conn.timestamp_end)) if conn.timestamp_end is not None else None,
        "peerCertificates": [_certificate(cert) for cert in conn.certificate_list[:16]],
    }


def _trailers(headers) -> list[dict]:
    if headers is None:
        return []
    sensitive = {"authorization", "proxy-authorization", "cookie", "set-cookie", "x-api-key", "api-key", "x-auth-token"}
    return [{"name": _bounded(name), "value": _bounded(value), "sensitive": name.lower() in sensitive}
            for name, value in list(headers.items(multi=True))[:64]]


def _is_binary(content_type: str | None, encoding: str | None) -> bool:
    if encoding and encoding.lower() not in ("identity", ""):
        return True
    if not content_type:
        return True
    normalized = content_type.lower()
    return not (
        normalized.startswith("text/")
        or "json" in normalized
        or "xml" in normalized
        or "javascript" in normalized
        or "x-www-form-urlencoded" in normalized
        or "graphql" in normalized
    )


def _body(raw_content: bytes | None, content_type: str | None, encoding: str | None) -> dict | None:
    if raw_content is None:
        return None
    is_truncated = len(raw_content) > MAX_BODY_BYTES
    captured = raw_content[:MAX_BODY_BYTES]
    return {
        "data_base64": base64.b64encode(captured).decode("ascii"),
        "content_type": content_type,
        "encoding": encoding,
        "is_binary": _is_binary(content_type, encoding),
        "is_truncated": is_truncated,
    }


def _breakpoint_body(raw_content: bytes | None, content_type: str | None, encoding: str | None) -> dict | None:
    if raw_content is None:
        return None
    is_truncated = len(raw_content) > MAX_BODY_BYTES
    captured = raw_content[:MAX_BODY_BYTES]
    return {
        "dataBase64": base64.b64encode(captured).decode("ascii"),
        "contentType": content_type,
        "isBinary": _is_binary(content_type, encoding),
        "isTruncated": is_truncated,
    }


def _duration_ms(start: float | None, end: float | None) -> int | None:
    if start is None or end is None or end < start:
        return None
    return _millis(end - start)


def _rules_path() -> str:
    override = os.environ.get("MAS_MOCK_RULES_PATH")
    if override:
        return override
    return os.path.join(str(ctx.options.confdir), "mock-rules.json")


def _breakpoint_root() -> str:
    return os.path.join(str(ctx.options.confdir), "breakpoints")


def _breakpoint_pending_dir() -> str:
    return os.path.join(_breakpoint_root(), "pending")


def _breakpoint_decision_dir() -> str:
    return os.path.join(_breakpoint_root(), "decisions")


def _atomic_json(path: str, payload: dict) -> None:
    os.makedirs(os.path.dirname(path), exist_ok=True)
    temporary = path + ".tmp"
    with open(temporary, "w", encoding="utf-8") as handle:
        json.dump(payload, handle, separators=(",", ":"))
    os.replace(temporary, path)


def _rules() -> list[dict]:
    global _RULES_MTIME_NS, _RULES_DOCUMENT
    path = _rules_path()
    try:
        stat = os.stat(path)
    except FileNotFoundError:
        _RULES_MTIME_NS = None
        _RULES_DOCUMENT = {"enabled": True, "rules": []}
        return []

    if _RULES_MTIME_NS != stat.st_mtime_ns:
        try:
            with open(path, "r", encoding="utf-8") as handle:
                document = json.load(handle)
            if isinstance(document, dict) and isinstance(document.get("rules"), list):
                _RULES_DOCUMENT = document
                _RULES_MTIME_NS = stat.st_mtime_ns
        except (OSError, json.JSONDecodeError) as exc:
            _emit({"type": "mock_rules_failed", "code": "mock_rules_reload_failed", "message": str(exc)})
            return []

    if not _RULES_DOCUMENT.get("enabled", True):
        return []
    return _RULES_DOCUMENT.get("rules", [])


def _normalized_segment(segment: str) -> str:
    if segment.isdigit():
        return ":id"
    lower = segment.lower()
    if len(lower) == 36:
        hyphens = {8, 13, 18, 23}
        if all((char == "-" if index in hyphens else char in "0123456789abcdef") for index, char in enumerate(lower)):
            return ":uuid"
    if len(lower) >= 16 and all(char in "0123456789abcdef" for char in lower):
        return ":hex"
    return segment


def _normalized_path(path: str) -> str:
    segments = [_normalized_segment(segment) for segment in path.split("?")[0].split("/") if segment]
    return "/" + "/".join(segments)


def _matching_rule(flow: http.HTTPFlow) -> dict | None:
    request = flow.request
    parsed = urlsplit(request.url)
    path = parsed.path or "/"
    for rule in _rules():
        if not rule.get("enabled", True):
            continue
        method = rule.get("method")
        if method and method.lower() != request.method.lower():
            continue
        host = rule.get("host")
        request_host = request.pretty_host or request.host
        if host and host.lower() != request_host.lower():
            continue
        pattern = rule.get("pathPattern") or "/"
        candidate = _normalized_path(path) if rule.get("pathMatch", "exact") == "normalized" else path
        if pattern != candidate:
            continue
        return rule
    return None


async def _proxy_rules(flow: http.HTTPFlow) -> list[dict]:
    request = flow.request
    return await _proxy_rules_for(request.method, request.pretty_host or request.host, urlsplit(request.url).path or "/")


def _rule_service_available() -> bool:
    return bool(RULE_TCP_PORT or RULE_TCP_TOKEN) if os.name == "nt" else bool(RULE_SOCKET)


async def _rule_document(payload: dict) -> dict:
    payload = dict(payload)
    if os.name == "nt":
        if not RULE_TCP_PORT or not RULE_TCP_TOKEN or len(RULE_TCP_TOKEN) != 64 or any(char not in "0123456789abcdef" for char in RULE_TCP_TOKEN):
            raise ValueError("Private rule transport configuration is invalid")
        port = int(RULE_TCP_PORT)
        if not 1 <= port <= 65535:
            raise ValueError("Private rule transport port is invalid")
        payload["token"] = RULE_TCP_TOKEN
    encoded = json.dumps(payload, separators=(",", ":")).encode("utf-8") + b"\n"
    if len(encoded) > MAX_RULE_REQUEST_BYTES:
        raise ValueError("Rule lookup request exceeds 8 KiB")
    if os.name == "nt":
        reader, writer = await asyncio.wait_for(asyncio.open_connection("127.0.0.1", port, limit=MAX_RULE_RESPONSE_BYTES), 2)
    else:
        reader, writer = await asyncio.wait_for(asyncio.open_unix_connection(RULE_SOCKET, limit=MAX_RULE_RESPONSE_BYTES), 2)
    try:
        writer.write(encoded)
        await asyncio.wait_for(writer.drain(), 2)
        line = await asyncio.wait_for(reader.readline(), 2)
        if not line or len(line) > MAX_RULE_RESPONSE_BYTES + 1:
            raise ValueError("Rule service returned no bounded response")
        document = json.loads(line)
        if not isinstance(document, dict):
            raise ValueError("Rule service returned an invalid document")
        return document
    finally:
        writer.close()
        await writer.wait_closed()


async def _proxy_rules_for(method: str, host: str, path: str) -> list[dict]:
    if not _rule_service_available():
        return []
    document = await _rule_document({"method": method, "host": host, "path": path})
    if not isinstance(document.get("rules"), list) or any(not isinstance(rule, dict) or not isinstance(rule.get("action"), dict) for rule in document["rules"]):
        raise ValueError("Rule service returned invalid rules")
    return document["rules"]


def _rule_order(rule: dict) -> tuple:
    return (int(rule.get("priority") or 0), str(rule.get("createdAt") or ""), str(rule.get("id") or ""))


def _record_proxy_rule(flow: http.HTTPFlow, rule: dict) -> None:
    flow.metadata.setdefault("mas_proxy_rule_ids", []).append(str(rule["id"]))


def _record_change(flow: http.HTTPFlow, rule: dict, field: str, before, after) -> None:
    changes = flow.metadata.setdefault("mas_proxy_rule_changes", [])
    if len(changes) < 32:
        changes.append({"ruleId": str(rule["id"])[:120], "field": field[:80], "before": str(before)[:256], "after": str(after)[:256]})


def _safe_url(url: str) -> str:
    parsed = urlsplit(url)
    return f"{parsed.scheme}://{parsed.hostname or ''}{parsed.path}"[:256]


def _record_breakpoint_changes(flow: http.HTTPFlow, rule: dict, stage: str, decision: dict, before: tuple) -> None:
    message = flow.request if stage == "request" else flow.response
    if stage == "request":
        if before[0] != flow.request.method:
            _record_change(flow, rule, "method", before[0], flow.request.method)
        if before[1] != _safe_url(flow.request.url):
            _record_change(flow, rule, "url", before[1], _safe_url(flow.request.url))
    elif message is not None and before[0] != message.status_code:
        _record_change(flow, rule, "status", before[0], message.status_code)
    if message is not None and before[2] != len(message.raw_content or b""):
        _record_change(flow, rule, "bodyBytes", before[2], len(message.raw_content or b""))
    for row in decision.get("headers") or []:
        _record_change(flow, rule, f"header:{str(row.get('name') or '')[:64]}", "prior value redacted", "set")


def _replace_body(message, body: bytes) -> None:
    # Changed payloads are unencoded bytes; let mitmproxy set their wire length.
    message.headers.pop("content-encoding", None)
    message.headers.pop("transfer-encoding", None)
    message.content = body


def _rewrite(message, action: dict, flow: http.HTTPFlow, rule: dict) -> None:
    for mutation in action.get("headers") or []:
        name = str(mutation["name"]).strip()
        if not name or "\r" in name or "\n" in name:
            raise ValueError("Invalid rewrite header name")
        if mutation.get("remove"):
            message.headers.pop(name, None)
            _record_change(flow, rule, f"header:{name}", "present", "removed")
        else:
            value = str(mutation.get("value") or "")
            if "\r" in value or "\n" in value:
                raise ValueError("Invalid rewrite header value")
            message.headers[name] = value
            _record_change(flow, rule, f"header:{name}", "prior value redacted", "set")
    if action.get("body") is not None:
        body = action["body"].encode("utf-8")
        if len(body) > MAX_BODY_BYTES:
            raise ValueError("Rewrite body exceeds capture limit")
        before_size = len(message.raw_content or b"")
        _replace_body(message, body)
        _record_change(flow, rule, "bodyBytes", before_size, len(body))


def _local_body(path: str) -> bytes:
    if not path or path in (".", "..") or os.path.basename(path) != path:
        raise ValueError("Map Local requires a relative filename")
    root = os.path.join(os.path.realpath(str(ctx.options.confdir)), "proxy-maps")
    if os.path.realpath(root) != root:
        raise ValueError("Map Local root must not be a symlink")
    filename = os.path.join(root, path)
    if os.path.realpath(filename) != filename:
        raise ValueError("Map Local path must be inside proxy-maps without symlinks")
    descriptor = os.open(filename, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0))
    try:
        info = os.fstat(descriptor)
        if not stat.S_ISREG(info.st_mode) or info.st_size > MAX_BODY_BYTES:
            raise ValueError("Map Local requires a bounded regular file")
        with os.fdopen(descriptor, "rb", closefd=False) as handle:
            body = handle.read(MAX_BODY_BYTES + 1)
        if len(body) > MAX_BODY_BYTES:
            raise ValueError("Map Local file exceeds capture limit")
        return body
    finally:
        os.close(descriptor)


def _remote_url(url: str) -> str:
    if not isinstance(url, str):
        raise ValueError("Map Remote requires a URL string")
    parsed = urlsplit(url)
    if parsed.scheme not in ("http", "https") or not parsed.hostname or parsed.username or parsed.password or parsed.fragment or len(url.encode("utf-8")) > 2048 or any(ord(char) < 32 for char in url):
        raise ValueError("Map Remote requires a bounded HTTP(S) URL without credentials")
    return url


def _apply_terminal_rule(flow: http.HTTPFlow, rules: list[dict], mock: dict | None) -> bool:
    terminal = next((item for item in rules if item.get("action", {}).get("type") in ("allow", "block", "map_local", "map_remote")), None)
    if terminal is None or (mock is not None and _rule_order(terminal) >= _rule_order(mock)):
        return False
    action = terminal["action"]
    _record_proxy_rule(flow, terminal)
    if action["type"] == "map_local":
        flow.response = http.Response.make(200, _local_body(action["path"]), {"content-type": mimetypes.guess_type(action["path"])[0] or "application/octet-stream"})
        _record_change(flow, terminal, "status", "upstream", 200)
    elif action["type"] == "map_remote":
        before = _safe_url(flow.request.url)
        flow.request.url = _remote_url(action["url"])
        _record_change(flow, terminal, "url", before, _safe_url(flow.request.url))
    elif action["type"] == "block":
        flow.response = http.Response.make(int(action.get("statusCode") or 403), b"", {"content-type": "text/plain"})
        _record_change(flow, terminal, "status", "upstream", flow.response.status_code)
    flow.metadata["mas_skip_mock"] = True
    flow.metadata.pop("mas_mock_rule_id", None)
    flow.metadata.pop("mas_mock_rule_name", None)
    return True


def _rule_by_id(rule_id: str | None) -> dict | None:
    if not rule_id:
        return None
    for rule in _rules():
        if str(rule.get("id") or "") == rule_id and rule.get("enabled", True):
            return rule
    return None


def _capture_sdk_request_id(flow: http.HTTPFlow) -> None:
    request_id = flow.request.headers.get(SDK_CORRELATION_HEADER)
    if not request_id:
        return
    normalized = str(request_id).strip()
    if not normalized:
        return
    flow.metadata["mas_sdk_request_id"] = normalized
    # Correlation is local debug metadata. Never forward it to the real backend.
    flow.request.headers.pop(SDK_CORRELATION_HEADER, None)


def _captured_request_headers(flow: http.HTTPFlow) -> list[dict]:
    headers = _headers(flow.request.headers)
    request_id = flow.metadata.get("mas_sdk_request_id")
    if request_id:
        headers.append({"name": SDK_CORRELATION_HEADER, "value": str(request_id)})
    return headers


def _mark_mock(flow: http.HTTPFlow, rule: dict) -> None:
    flow.metadata["mas_mock_rule_id"] = str(rule.get("id") or "")
    flow.metadata["mas_mock_rule_name"] = str(rule.get("name") or "Mock rule")


def _breakpoint_envelope(flow: http.HTTPFlow, rule: dict, stage: str, breakpoint_id: str) -> dict:
    request = flow.request
    response = flow.response
    now_ms = int(time.time() * 1000)
    if stage == "response" and response is not None:
        headers = _headers(response.headers)
        body = _breakpoint_body(response.raw_content, response.headers.get("content-type"), response.headers.get("content-encoding"))
        status_code = response.status_code
    else:
        headers = _headers(request.headers)
        body = _breakpoint_body(request.raw_content, request.headers.get("content-type"), request.headers.get("content-encoding"))
        status_code = None

    return {
        "schemaVersion": 1,
        "id": breakpoint_id,
        "flowId": flow.id,
        "ruleId": str(rule.get("id") or ""),
        "ruleName": str(rule.get("name") or "Mock rule"),
        "stage": stage,
        "createdAt": str(now_ms),
        "deadlineAt": str(now_ms + BREAKPOINT_TIMEOUT_MS),
        "method": request.method,
        "url": request.url,
        "headers": headers,
        "body": body,
        "statusCode": status_code,
    }


async def _wait_for_breakpoint(flow: http.HTTPFlow, rule: dict, stage: str) -> dict | None:
    breakpoint_id = f"{flow.id}-{stage}-{int(time.time() * 1000)}"
    pending_path = os.path.join(_breakpoint_pending_dir(), breakpoint_id + ".json")
    decision_path = os.path.join(_breakpoint_decision_dir(), breakpoint_id + ".json")
    try:
        _atomic_json(pending_path, _breakpoint_envelope(flow, rule, stage, breakpoint_id))
    except OSError as exc:
        _emit({"type": "mock_rules_failed", "code": "breakpoint_publish_failed", "message": str(exc), "rule_id": rule.get("id")})
        return None

    deadline = time.monotonic() + max(1, BREAKPOINT_TIMEOUT_MS) / 1000.0
    try:
        while time.monotonic() < deadline:
            if os.path.isfile(decision_path):
                try:
                    descriptor = os.open(decision_path, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0))
                    with os.fdopen(descriptor, "rb") as handle:
                        info = os.fstat(handle.fileno())
                        if not stat.S_ISREG(info.st_mode) or info.st_size > MAX_DECISION_BYTES:
                            raise ValueError("Breakpoint decision exceeds the supported size")
                        payload = handle.read(MAX_DECISION_BYTES + 1)
                    if len(payload) > MAX_DECISION_BYTES:
                        raise ValueError("Breakpoint decision exceeds the supported size")
                    decision = json.loads(payload)
                    _validate_breakpoint_decision(decision)
                    return decision
                except (OSError, ValueError, TypeError, UnicodeDecodeError) as exc:
                    _emit({"type": "mock_rules_failed", "code": "breakpoint_decision_invalid", "message": str(exc), "rule_id": rule.get("id")})
                    return {"action": "cancel"}
                finally:
                    try:
                        os.remove(decision_path)
                    except FileNotFoundError:
                        pass
            await asyncio.sleep(BREAKPOINT_POLL_MS / 1000.0)
        return None
    finally:
        try:
            os.remove(pending_path)
        except FileNotFoundError:
            pass


def _decode_breakpoint_body(body: dict | None) -> bytes | None:
    if body is None:
        return None
    if not isinstance(body, dict) or not isinstance(body.get("dataBase64"), str) or len(body["dataBase64"]) > ((MAX_BODY_BYTES + 2) // 3) * 4 or body.get("isTruncated"):
        raise ValueError("Breakpoint body is invalid, truncated, or too large")
    decoded = base64.b64decode(body["dataBase64"], validate=True)
    if len(decoded) > MAX_BODY_BYTES:
        raise ValueError("Breakpoint body exceeds capture limit")
    content_type = body.get("contentType")
    if content_type is not None and (not isinstance(content_type, str) or len(content_type) > 8192 or "\r" in content_type or "\n" in content_type):
        raise ValueError("Breakpoint body has an invalid content type")
    return decoded


def _validate_breakpoint_decision(decision: dict) -> None:
    if not isinstance(decision, dict) or decision.get("action") not in ("continue", "cancel"):
        raise ValueError("Breakpoint decision has an invalid action")
    method = decision.get("method")
    if method is not None and (not isinstance(method, str) or not method or len(method) > 32 or any(not (char.isascii() and (char.isalnum() or char in "!#$%&'*+-.^_`|~")) for char in method)):
        raise ValueError("Breakpoint decision has an invalid method")
    if decision.get("url") is not None:
        _remote_url(decision["url"])
    _validate_breakpoint_headers(decision.get("headers"))
    _decode_breakpoint_body(decision.get("body"))
    status = decision.get("statusCode")
    if status is not None and (type(status) is not int or not 100 <= status <= 599):
        raise ValueError("Breakpoint decision has an invalid status")


def _validate_breakpoint_headers(rows) -> None:
    if rows is None:
        return
    if not isinstance(rows, list) or len(rows) > 64:
        raise ValueError("Breakpoint decision exceeds 64 headers")
    for row in rows:
        if not isinstance(row, dict):
            raise ValueError("Invalid breakpoint header")
        name, value = row.get("name"), row.get("value")
        if not isinstance(name, str) or not name or len(name) > 256 or any(not (char.isascii() and (char.isalnum() or char in "!#$%&'*+-.^_`|~")) for char in name) or not isinstance(value, str) or len(value.encode("utf-8")) > 8192 or "\r" in value or "\n" in value:
            raise ValueError("Invalid breakpoint header")


def _replace_headers(headers, rows: list[dict] | None) -> None:
    if rows is None:
        return
    _validate_breakpoint_headers(rows)
    headers.clear()
    for row in rows:
        name = str(row.get("name") or "").strip()
        if name:
            headers.add(name, str(row.get("value") or ""))


def _apply_decision_body(message, decision: dict) -> None:
    if decision.get("clearBody", False):
        _replace_body(message, b"")
        return
    body = decision.get("body")
    if body is None:
        return
    _replace_body(message, _decode_breakpoint_body(body))
    content_type = body.get("contentType")
    if content_type:
        message.headers["content-type"] = str(content_type)


def _apply_request_breakpoint_decision(flow: http.HTTPFlow, decision: dict | None, strict: bool = False) -> bool:
    if not decision:
        return True
    if decision.get("action") == "cancel":
        flow.kill()
        return False
    method = decision.get("method")
    if method:
        flow.request.method = str(method).upper()
    url = decision.get("url")
    if url:
        flow.request.url = str(url)
    _replace_headers(flow.request.headers, decision.get("headers"))
    try:
        _apply_decision_body(flow.request, decision)
    except (ValueError, TypeError) as exc:
        if strict:
            raise ValueError("Invalid request breakpoint body") from exc
        _emit({"type": "mock_rules_failed", "code": "breakpoint_request_body_invalid", "message": str(exc), "rule_id": flow.metadata.get("mas_mock_rule_id")})
    # A breakpoint edit can reintroduce the local-only SDK header. Capture then strip again.
    _capture_sdk_request_id(flow)
    return True


def _apply_response_breakpoint_decision(flow: http.HTTPFlow, decision: dict | None, strict: bool = False) -> bool:
    if not decision:
        return True
    if decision.get("action") == "cancel":
        flow.kill()
        return False
    if flow.response is None:
        return True
    status_code = decision.get("statusCode")
    if status_code is not None:
        flow.response.status_code = int(status_code)
    _replace_headers(flow.response.headers, decision.get("headers"))
    try:
        _apply_decision_body(flow.response, decision)
    except (ValueError, TypeError) as exc:
        if strict:
            raise ValueError("Invalid response breakpoint body") from exc
        _emit({"type": "mock_rules_failed", "code": "breakpoint_response_body_invalid", "message": str(exc), "rule_id": flow.metadata.get("mas_mock_rule_id")})
    return True


async def dns_request(flow: dns.DNSFlow) -> None:
    if not isinstance(flow.client_conn.proxy_mode, DnsMode):
        return
    question = flow.request.question
    if question is None or question.class_ != dns.classes.IN or question.type not in (dns.types.A, dns.types.AAAA):
        return
    try:
        if not _rule_service_available():
            raise ValueError("Rule service is unavailable")
        rules = await _proxy_rules_for("DNS", question.name, "/")
        rule = next((item for item in rules if item["action"].get("type") == "dns_override"), None)
        if rule is None:
            return
        address = ipaddress.ip_address(rule["action"]["address"])
        if question.type == dns.types.A and isinstance(address, ipaddress.IPv4Address):
            answers = [dns.ResourceRecord.A(question.name, address)]
        elif question.type == dns.types.AAAA and isinstance(address, ipaddress.IPv6Address):
            answers = [dns.ResourceRecord.AAAA(question.name, address)]
        else:
            answers = []
        flow.response = flow.request.succeed(answers)
        _record_proxy_rule(flow, rule)
    except (OSError, ValueError, KeyError, TypeError, UnicodeDecodeError, asyncio.TimeoutError, asyncio.LimitOverrunError) as exc:
        _emit({"type": "proxy_rules_failed", "code": "proxy_rule_dns_override_failed", "message": str(exc)})
        flow.response = flow.request.fail(dns.response_codes.SERVFAIL)



_SCRIPT_SLOTS = asyncio.Semaphore(4)

async def _script_action(flow, item, stage, message=None):
    worker = os.environ.get("MAS_SCRIPT_WORKER")
    if not worker:
        raise ValueError("Script worker is unavailable; build and install the worker alongside the local service")
    target = flow.response if stage == "response" else flow.request
    raw = message.content if message is not None else (target.raw_content or b"")
    if len(raw) > MAX_BODY_BYTES:
        raise ValueError("Script body exceeds the supported limit")
    event = {"stage": stage, "method": flow.request.method, "url": flow.request.url,
        "headers": [] if message is not None else _headers(target.headers),
        "body": {"dataBase64": base64.b64encode(raw).decode("ascii"),
            "contentType": None if message is not None else target.headers.get("content-type"),
            "isTruncated": False}}
    if stage == "response": event["statusCode"] = target.status_code
    if message is not None:
        event.update(opcode=int(message.type), fromClient=message.from_client, dropped=message.dropped)
    payload = json.dumps({"script": item["action"]["script"], "event": event}, ensure_ascii=False, separators=(",", ":")).encode()
    if len(json.dumps(event, ensure_ascii=False).encode()) > 2 * 1024 * 1024 or len(payload) > 3 * 1024 * 1024:
        raise ValueError("Script event exceeds the 2 MiB worker input limit")
    async with _SCRIPT_SLOTS:
        process = await asyncio.create_subprocess_exec(worker, stdin=asyncio.subprocess.PIPE,
            stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.DEVNULL, env={}, cwd="/")
        async def exchange():
            process.stdin.write(payload)
            await process.stdin.drain()
            process.stdin.close()
            result = bytearray()
            while True:
                chunk = await process.stdout.read(65536)
                if not chunk: break
                result.extend(chunk)
                if len(result) > 2 * 1024 * 1024 + 1: raise ValueError("Script output exceeded its limit")
            await process.wait()
            if process.returncode != 0: raise ValueError("Script worker failed")
            return json.loads(result)
        try:
            output = await asyncio.wait_for(exchange(), 1)
        finally:
            if process.returncode is None:
                process.kill()
                await process.wait()
    if not isinstance(output, dict) or "error" in output or output.get("stage") != stage or set(output) - set(event):
        raise ValueError("Script failed or returned an invalid event")
    if message is not None:
        if any(output.get(key) != event[key] for key in ("method", "url", "opcode", "fromClient", "headers")) or type(output.get("dropped")) is not bool:
            raise ValueError("Script changed read-only WebSocket metadata")
        content = _decode_breakpoint_body(output.get("body"))
        if content is None: raise ValueError("Script must return a complete WebSocket body")
        if int(message.type) == 1: content.decode("utf-8")
        message.content = content
        if output["dropped"]: message.drop()
    else:
        decision = {key: value for key, value in output.items() if key in ("method", "url", "headers", "body", "statusCode")}
        decision["action"] = "continue"
        _validate_breakpoint_decision(decision)
        if output.get("body") == event["body"]: decision.pop("body", None)
        if stage == "response":
            if output.get("method") != event["method"] or output.get("url") != event["url"]:
                raise ValueError("Response scripts cannot rewrite request identity")
            _apply_response_breakpoint_decision(flow, decision, strict=True)
        else: _apply_request_breakpoint_decision(flow, decision, strict=True)
    _record_proxy_rule(flow, item)
    _record_change(flow, item, "script:" + stage, "local event", "transformed")

async def _script_or_stop(flow, item, stage, message=None):
    try:
        key = "mas_script_budget_" + stage
        budget = flow.metadata.setdefault(key, {"count": 0, "deadline": time.monotonic() + 2})
        budget["count"] += 1
        remaining = budget["deadline"] - time.monotonic()
        if budget["count"] > 16 or remaining <= 0:
            raise ValueError("Script stage exceeds sixteen hooks or its two-second deadline")
        await asyncio.wait_for(_script_action(flow, item, stage, message), remaining)
        return True
    except (OSError, ValueError, KeyError, TypeError, UnicodeDecodeError, asyncio.TimeoutError, BrokenPipeError):
        if message is not None: message.drop()
        _emit({"type": "proxy_rules_failed", "code": "script_hook_failed", "message": "Script hook failed; matching flow stopped.", "rule_id": item.get("id")})
        _network_error(flow, "script_hook_failed", "Script hook failed; matching flow stopped.")
        return False

async def _request_rules(flow: http.HTTPFlow) -> None:
    _capture_sdk_request_id(flow)
    tls_rule = _TLS_POLICIES.get(flow.client_conn, {}).get("rule")
    if tls_rule:
        _record_proxy_rule(flow, tls_rule)
        _record_change(flow, tls_rule, "tlsInspection", "connection policy", "inspect")
    try:
        proxy_rules = await _proxy_rules(flow)
    except (OSError, ValueError, UnicodeDecodeError, asyncio.TimeoutError, asyncio.LimitOverrunError) as exc:
        _emit({"type": "proxy_rules_failed", "code": "proxy_rules_unavailable", "message": str(exc)})
        flow.kill()
        return
    original_identity = (flow.request.method, flow.request.url, flow.request.pretty_host)
    try:
        supported = {"allow", "block", "map_local", "map_remote", "rewrite_request", "rewrite_response", "breakpoint", "no_cache", "block_cookies", "script_hook"}
        for item in proxy_rules:
            if item["action"].get("type") not in supported:
                raise ValueError(f"Rule action {item['action'].get('type')} is not yet supported")
        for item in proxy_rules:
            action = item.get("action") or {}
            kind = action.get("type")
            if kind == "script_hook" and action.get("stage") == "request":
                if not await _script_or_stop(flow, item, "request"): return
            elif kind == "rewrite_request":
                _rewrite(flow.request, action, flow, item)
                _record_proxy_rule(flow, item)
            elif kind == "no_cache":
                flow.request.headers["cache-control"] = "no-cache"
                _record_change(flow, item, "header:cache-control", "prior value redacted", "set")
                _record_proxy_rule(flow, item)
            elif kind == "block_cookies":
                flow.request.headers.pop("cookie", None)
                _record_change(flow, item, "header:cookie", "present", "removed")
                _record_proxy_rule(flow, item)
            elif kind == "breakpoint" and action.get("stage") == "request":
                _record_proxy_rule(flow, item)
                decision = await _wait_for_breakpoint(flow, item, "request")
                if not isinstance(decision, dict):
                    raise ValueError("Request breakpoint ended without a decision")
                before = (flow.request.method, _safe_url(flow.request.url), len(flow.request.raw_content or b""))
                if not _apply_request_breakpoint_decision(flow, decision, strict=True):
                    return
                _record_breakpoint_changes(flow, item, "request", decision, before)
        _capture_sdk_request_id(flow)
        if (flow.request.method, flow.request.url, flow.request.pretty_host) != original_identity:
            # Re-match the edited request once; request mutations must not run twice.
            proxy_rules = await _proxy_rules(flow)
        flow.metadata["mas_response_proxy_rules"] = [item for item in proxy_rules if item.get("action", {}).get("type") in ("rewrite_response", "breakpoint", "no_cache", "block_cookies", "script_hook")]
        rule = _matching_rule(flow)
        if _apply_terminal_rule(flow, proxy_rules, rule):
            return
    except (OSError, ValueError, KeyError, TypeError, UnicodeDecodeError, asyncio.TimeoutError, asyncio.LimitOverrunError) as exc:
        _emit({"type": "proxy_rules_failed", "code": "proxy_rule_action_failed", "message": str(exc)})
        flow.kill()
        return
    if rule is None:
        return
    _mark_mock(flow, rule)

    if rule.get("requestBreakpoint", False):
        before_identity = (flow.request.method, flow.request.url, flow.request.pretty_host)
        decision = await _wait_for_breakpoint(flow, rule, "request")
        if not _apply_request_breakpoint_decision(flow, decision):
            return
        if (flow.request.method, flow.request.url, flow.request.pretty_host) != before_identity:
            try:
                rematched = await _proxy_rules(flow)
                flow.metadata["mas_response_proxy_rules"] = [item for item in rematched if item.get("action", {}).get("type") in ("rewrite_response", "breakpoint", "no_cache", "block_cookies", "script_hook")]
                if _apply_terminal_rule(flow, rematched, rule):
                    return
            except (OSError, ValueError, KeyError, TypeError, UnicodeDecodeError, asyncio.TimeoutError, asyncio.LimitOverrunError) as exc:
                _emit({"type": "proxy_rules_failed", "code": "proxy_rule_rematch_failed", "message": str(exc)})
                flow.kill()
                return

    failure_mode = rule.get("failureMode", "none")
    if failure_mode == "drop":
        flow.kill()
        return
    if failure_mode == "timeout":
        timeout_ms = int(rule.get("latencyMs") or 30000)
        await asyncio.sleep(max(0, timeout_ms) / 1000.0)
        flow.kill()



async def _network_document(flow: http.HTTPFlow, profile_id: str | None = None) -> dict:
    if not _rule_service_available():
        return {"networkProfile": None, "networkProfileEnabled": False}
    request = flow.request
    return await _rule_document({"method": request.method, "host": request.pretty_host or request.host,
        "path": urlsplit(request.url).path or "/", "request_id": flow.metadata.get("mas_sdk_request_id"),
        "network_profile_id": profile_id})


async def _network_wait(flow: http.HTTPFlow, profile: dict, seconds: float) -> None:
    # ponytail: buffered message delay simulates end-to-end body rate, not smooth packet pacing.
    if seconds > 120:
        raise ValueError("Network profile transfer delay exceeds the 120-second per-stage limit")
    deadline = time.monotonic() + max(0, seconds)
    while flow.live and time.monotonic() < deadline:
        if not (await _network_document(flow, str(profile["id"]))).get("networkProfileEnabled", False):
            return
        await asyncio.sleep(min(0.1, max(0, deadline - time.monotonic())))


def _network_error(flow: http.HTTPFlow, code: str, message: str) -> None:
    request = flow.request
    parsed = urlsplit(request.url)
    _emit({"type": "flow_failed", "id": flow.id, "code": code, "message": message, "recoverable": True,
        "started_at": str(_millis(request.timestamp_start) or 0),
        "request": {"method": request.method, "url": request.url, "scheme": request.scheme,
            "host": request.pretty_host or request.host, "port": request.port, "path": parsed.path or "/",
            "query": parsed.query or None, "headers": _captured_request_headers(flow),
            "body": _body(request.raw_content, request.headers.get("content-type"), request.headers.get("content-encoding"))}})
    flow.metadata["mas_network_failed"] = True
    flow.kill()


async def request(flow: http.HTTPFlow) -> None:
    await _request_rules(flow)
    if not flow.live: return
    try:
        profile = (await _network_document(flow)).get("networkProfile")
        flow.metadata["mas_network_profile"] = profile
        if profile is None: return
        if profile["offline"] or random.random() * 100 < profile["failurePercent"]:
            reason = "offline simulation" if profile["offline"] else "simulated request failure"
            _network_error(flow, "network_profile_offline" if profile["offline"] else "network_profile_failure", f"{profile['name']}: {reason}. This is not transport packet loss.")
            return
        delay = max(0, profile["latencyMs"] + random.uniform(-profile["jitterMs"], profile["jitterMs"])) / 1000
        rate = profile.get("uploadBytesPerSecond")
        await _network_wait(flow, profile, delay + (len(flow.request.raw_content or b"") / rate if rate else 0))
    except (OSError, ValueError, KeyError, TypeError, UnicodeDecodeError, asyncio.TimeoutError, asyncio.LimitOverrunError) as error:
        _network_error(flow, "network_profile_failed", str(error))


def _set_json_pointer(document, pointer: str, value, remove: bool) -> None:
    if pointer == "":
        return
    if not pointer.startswith("/"):
        raise ValueError(f"JSON mutation pointer must start with '/': {pointer}")
    parts = [part.replace("~1", "/").replace("~0", "~") for part in pointer[1:].split("/")]
    target = document
    for part in parts[:-1]:
        if isinstance(target, list):
            target = target[int(part)]
        elif isinstance(target, dict):
            target = target[part]
        else:
            raise ValueError(f"JSON pointer traversed non-container at {part}")
    leaf = parts[-1]
    if isinstance(target, list):
        index = int(leaf)
        if remove:
            target.pop(index)
        else:
            target[index] = value
    elif isinstance(target, dict):
        if remove:
            target.pop(leaf, None)
        else:
            target[leaf] = value
    else:
        raise ValueError(f"JSON pointer target is not a container: {pointer}")


def _apply_json_mutations(flow: http.HTTPFlow, mutations: list[dict]) -> None:
    if not mutations or flow.response is None:
        return
    raw = flow.response.raw_content or b""
    try:
        document = json.loads(raw.decode("utf-8"))
        for mutation in mutations:
            _set_json_pointer(document, str(mutation.get("pointer") or ""), mutation.get("value"), bool(mutation.get("remove", False)))
        _replace_body(flow.response, json.dumps(document, separators=(",", ":")).encode("utf-8"))
        if not flow.response.headers.get("content-type"):
            flow.response.headers["content-type"] = "application/json"
    except (UnicodeDecodeError, json.JSONDecodeError, ValueError, KeyError, IndexError) as exc:
        _emit({"type": "mock_rules_failed", "code": "mock_json_mutation_failed", "message": str(exc), "rule_id": flow.metadata.get("mas_mock_rule_id")})


def _apply_body_override(response: http.Response, override: dict) -> None:
    encoding = override.get("encoding", "text")
    data = override.get("data", "")
    _replace_body(response, base64.b64decode(data) if encoding == "base64" else str(data).encode("utf-8"))
    content_type = override.get("contentType")
    if content_type:
        response.headers["content-type"] = content_type


def _apply_header_mutations(response: http.Response, mutations: list[dict]) -> None:
    for mutation in mutations:
        name = str(mutation.get("name") or "").strip()
        if not name:
            continue
        if mutation.get("remove", False):
            response.headers.pop(name, None)
        else:
            response.headers[name] = str(mutation.get("value") or "")


async def response(flow: http.HTTPFlow) -> None:
    request = flow.request
    response = flow.response
    if response is None:
        return

    rule = None if flow.metadata.get("mas_skip_mock") else (_rule_by_id(flow.metadata.get("mas_mock_rule_id")) or _matching_rule(flow))
    if rule is not None:
        _mark_mock(flow, rule)
        latency_ms = int(rule.get("latencyMs") or 0)
        if latency_ms > 0 and rule.get("failureMode", "none") == "none":
            await asyncio.sleep(latency_ms / 1000.0)
        status_code = rule.get("statusCode")
        if status_code is not None:
            response.status_code = int(status_code)
        body_override = rule.get("responseBody")
        if isinstance(body_override, dict):
            try:
                _apply_body_override(response, body_override)
            except (ValueError, TypeError) as exc:
                _emit({"type": "mock_rules_failed", "code": "mock_body_override_failed", "message": str(exc), "rule_id": rule.get("id")})
        _apply_header_mutations(response, rule.get("responseHeaders") or [])
        _apply_json_mutations(flow, rule.get("jsonMutations") or [])

        if rule.get("responseBreakpoint", False):
            decision = await _wait_for_breakpoint(flow, rule, "response")
            if not _apply_response_breakpoint_decision(flow, decision):
                return
            response = flow.response
            if response is None:
                return

    try:
        for item in flow.metadata.get("mas_response_proxy_rules", []):
            action = item["action"]
            if action["type"] == "script_hook" and action.get("stage") == "response":
                if not await _script_or_stop(flow, item, "response"): return
            elif action["type"] == "rewrite_response":
                _rewrite(response, action, flow, item)
                _record_proxy_rule(flow, item)
            elif action["type"] == "no_cache":
                response.headers["cache-control"] = "no-store"
                response.headers.pop("expires", None)
                _record_change(flow, item, "header:cache-control", "prior value redacted", "set")
                _record_change(flow, item, "header:expires", "present", "removed")
            elif action["type"] == "block_cookies":
                response.headers.pop("set-cookie", None)
                _record_change(flow, item, "header:set-cookie", "present", "removed")
            elif action.get("stage") == "response":
                _record_proxy_rule(flow, item)
                decision = await _wait_for_breakpoint(flow, item, "response")
                if not isinstance(decision, dict):
                    raise ValueError("Response breakpoint ended without a decision")
                before = (response.status_code, None, len(response.raw_content or b""))
                if not _apply_response_breakpoint_decision(flow, decision, strict=True):
                    return
                _record_breakpoint_changes(flow, item, "response", decision, before)
                response = flow.response
                if response is None:
                    return
    except (OSError, ValueError, KeyError, TypeError) as exc:
        _emit({"type": "proxy_rules_failed", "code": "proxy_rule_action_failed", "message": str(exc)})
        flow.kill()
        return

    profile = flow.metadata.get("mas_network_profile")
    if profile is not None:
        try:
            rate = profile.get("downloadBytesPerSecond")
            await _network_wait(flow, profile, len(response.raw_content or b"") / rate if rate else 0)
        except (OSError, ValueError, KeyError, TypeError, UnicodeDecodeError, asyncio.TimeoutError, asyncio.LimitOverrunError) as error:
            _network_error(flow, "network_profile_failed", str(error))
            return

    parsed = urlsplit(request.url)
    response_size = len(response.raw_content) if response.raw_content is not None else None
    started_at = str(_millis(request.timestamp_start) or 0)
    request_content_type = request.headers.get("content-type")
    response_content_type = response.headers.get("content-type")

    request_payload = {
        "method": request.method,
        "url": request.url,
        "scheme": request.scheme,
        "host": request.pretty_host or request.host,
        "port": request.port,
        "path": parsed.path or "/",
        "query": parsed.query or None,
        "headers": _captured_request_headers(flow),
        "body": _body(request.raw_content, request_content_type, request.headers.get("content-encoding")),
    }
    response_payload = {
        "status_code": response.status_code,
        "reason": response.reason or None,
        "headers": _headers(response.headers),
        "body": _body(response.raw_content, response_content_type, response.headers.get("content-encoding")),
    }
    timing_payload = {
        "request_ms": _duration_ms(request.timestamp_start, request.timestamp_end),
        "server_ms": _duration_ms(request.timestamp_end, response.timestamp_start),
        "download_ms": _duration_ms(response.timestamp_start, response.timestamp_end),
        "total_ms": _duration_ms(request.timestamp_start, response.timestamp_end),
    }
    protocol_payload = {
        "requestHttpVersion": _bounded(request.http_version),
        "responseHttpVersion": _bounded(response.http_version),
        "requestTrailers": _trailers(request.trailers),
        "responseTrailers": _trailers(response.trailers),
        "clientConnection": _connection(flow.client_conn),
        "serverConnection": _connection(flow.server_conn),
        "websocket": flow.websocket is not None,
    }

    _emit({
        "type": "flow_completed",
        "id": flow.id,
        "session_id": SESSION_ID,
        "started_at": started_at,
        "response_size_bytes": response_size,
        "request": request_payload,
        "response": response_payload,
        "timing": timing_payload,
        "protocol": protocol_payload,
        "mock_rule_id": flow.metadata.get("mas_mock_rule_id"),
        "mock_rule_name": flow.metadata.get("mas_mock_rule_name"),
        "proxy_rule_ids": flow.metadata.get("mas_proxy_rule_ids", []),
        "proxy_rule_changes": flow.metadata.get("mas_proxy_rule_changes", []),
    })


async def websocket_message(flow: http.HTTPFlow) -> None:
    ws = flow.websocket
    if ws is None or not ws.messages:
        return
    message = ws.messages[-1]
    flow.metadata.pop("mas_script_budget_websocket", None)
    try:
        for item in await _proxy_rules(flow):
            if item.get("action", {}).get("type") == "script_hook" and item["action"].get("stage") == "websocket":
                if not await _script_or_stop(flow, item, "websocket", message): return
    except (OSError, ValueError, UnicodeDecodeError, asyncio.TimeoutError, asyncio.LimitOverrunError):
        _network_error(flow, "script_hook_failed", "WebSocket rule lookup failed; matching flow stopped.")
        return
    sequence = int(flow.metadata.get("mas_websocket_sequence", 0)) + 1
    flow.metadata["mas_websocket_sequence"] = sequence
    opcode = int(message.type)
    try:
        valid_text = opcode == 1 and message.content.decode("utf-8") is not None
    except UnicodeDecodeError:
        valid_text = False
    body = _body(message.content, "text/plain" if valid_text else "application/octet-stream", None)
    if body is not None:
        body["is_binary"] = not valid_text
    _emit({
        "type": "websocket_message",
        "id": f"{flow.id}:{sequence}",
        "flow_id": flow.id,
        "session_id": SESSION_ID,
        "sequence": sequence,
        "from_client": message.from_client,
        "opcode": opcode,
        "timestamp": str(_millis(message.timestamp) or 0),
        "dropped": message.dropped,
        "injected": message.injected,
        "body": body,
    })
    # ponytail: mitmproxy retains only the latest message here; one in-flight message may exceed 2 MiB before our capture truncates it.
    # The WebSocket layer forwards from its own message reference after this hook (mitmproxy 12.2.3).
    del ws.messages[:-1]


def websocket_end(flow: http.HTTPFlow) -> None:
    ws = flow.websocket
    if ws is None:
        return
    _emit({
        "type": "websocket_closed",
        "flow_id": flow.id,
        "close_code": ws.close_code,
        "close_reason": _bounded(ws.close_reason),
        "closed_by_client": ws.closed_by_client,
    })


def error(flow: http.HTTPFlow) -> None:
    if flow.metadata.get("mas_network_failed"): return
    error_message = flow.error.msg if flow.error is not None else "Unknown proxy error"
    _emit({
        "type": "flow_failed",
        "id": flow.id,
        "code": "proxy_flow_failed",
        "message": error_message,
        "mock_rule_id": flow.metadata.get("mas_mock_rule_id"),
        "mock_rule_name": flow.metadata.get("mas_mock_rule_name"),
        "proxy_rule_ids": flow.metadata.get("mas_proxy_rule_ids", []),
    })
