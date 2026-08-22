import asyncio
import base64
import json
import os
from urllib.parse import urlsplit

from mitmproxy import ctx, http

EVENT_PREFIX = "MAS_EVENT "
SESSION_ID = os.environ.get("MAS_SESSION_ID")
MAX_BODY_BYTES = int(os.environ.get("MAS_MAX_BODY_BYTES", str(2 * 1024 * 1024)))
_RULES_MTIME_NS: int | None = None
_RULES_DOCUMENT: dict = {"enabled": True, "rules": []}


def _emit(payload: dict) -> None:
    print(EVENT_PREFIX + json.dumps(payload, separators=(",", ":")), flush=True)


def _millis(value: float | None) -> int | None:
    if value is None:
        return None
    return max(0, int(value * 1000))


def _headers(headers) -> list[dict]:
    return [
        {"name": name, "value": value}
        for name, value in headers.items(multi=True)
    ]


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


def _duration_ms(start: float | None, end: float | None) -> int | None:
    if start is None or end is None or end < start:
        return None
    return _millis(end - start)


def _rules_path() -> str:
    override = os.environ.get("MAS_MOCK_RULES_PATH")
    if override:
        return override
    return os.path.join(str(ctx.options.confdir), "mock-rules.json")


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
            _emit({
                "type": "mock_rules_failed",
                "code": "mock_rules_reload_failed",
                "message": str(exc),
            })
            return []

    if not _RULES_DOCUMENT.get("enabled", True):
        return []
    return _RULES_DOCUMENT.get("rules", [])


def _normalized_path(path: str) -> str:
    segments = []
    for segment in path.split("?")[0].split("/"):
        if not segment:
            continue
        if _dynamic_segment(segment):
            segments.append(":id")
        else:
            segments.append(segment)
    return "/" + "/".join(segments)


def _dynamic_segment(segment: str) -> bool:
    if segment.isdigit():
        return True
    lower = segment.lower()
    if len(lower) == 36:
        hyphens = {8, 13, 18, 23}
        if all((char == "-" if index in hyphens else char in "0123456789abcdef") for index, char in enumerate(lower)):
            return True
    return len(lower) >= 20 and all(char.isalnum() or char in "-_" for char in lower)


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
        path_match = rule.get("pathMatch", "exact")
        candidate = _normalized_path(path) if path_match == "normalized" else path
        if pattern != candidate:
            continue
        return rule
    return None


def _mark_mock(flow: http.HTTPFlow, rule: dict) -> None:
    flow.metadata["mas_mock_rule_id"] = rule.get("id")
    flow.metadata["mas_mock_rule_name"] = rule.get("name")


async def request(flow: http.HTTPFlow) -> None:
    rule = _matching_rule(flow)
    if rule is None:
        return
    _mark_mock(flow, rule)

    failure_mode = rule.get("failureMode", "none")
    if failure_mode == "drop":
        flow.kill()
        return
    if failure_mode == "timeout":
        timeout_ms = int(rule.get("latencyMs") or 30000)
        await asyncio.sleep(max(0, timeout_ms) / 1000.0)
        flow.kill()


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
            _set_json_pointer(
                document,
                str(mutation.get("pointer") or ""),
                mutation.get("value"),
                bool(mutation.get("remove", False)),
            )
        flow.response.raw_content = json.dumps(document, separators=(",", ":")).encode("utf-8")
        if not flow.response.headers.get("content-type"):
            flow.response.headers["content-type"] = "application/json"
    except (UnicodeDecodeError, json.JSONDecodeError, ValueError, KeyError, IndexError) as exc:
        _emit({
            "type": "mock_rules_failed",
            "code": "mock_json_mutation_failed",
            "message": str(exc),
            "rule_id": flow.metadata.get("mas_mock_rule_id"),
        })


def _apply_body_override(response: http.Response, override: dict) -> None:
    encoding = override.get("encoding", "text")
    data = override.get("data", "")
    if encoding == "base64":
        response.raw_content = base64.b64decode(data)
    else:
        response.raw_content = str(data).encode("utf-8")
    content_type = override.get("contentType")
    if content_type:
        response.headers["content-type"] = content_type
    response.headers.pop("content-encoding", None)


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

    rule = _matching_rule(flow)
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
                _emit({
                    "type": "mock_rules_failed",
                    "code": "mock_body_override_failed",
                    "message": str(exc),
                    "rule_id": rule.get("id"),
                })
        _apply_header_mutations(response, rule.get("responseHeaders") or [])
        _apply_json_mutations(flow, rule.get("jsonMutations") or [])

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
        "headers": _headers(request.headers),
        "body": _body(
            request.raw_content,
            request_content_type,
            request.headers.get("content-encoding"),
        ),
    }

    response_payload = {
        "status_code": response.status_code,
        "reason": response.reason or None,
        "headers": _headers(response.headers),
        "body": _body(
            response.raw_content,
            response_content_type,
            response.headers.get("content-encoding"),
        ),
    }

    timing_payload = {
        "request_ms": _duration_ms(request.timestamp_start, request.timestamp_end),
        "server_ms": _duration_ms(request.timestamp_end, response.timestamp_start),
        "download_ms": _duration_ms(response.timestamp_start, response.timestamp_end),
        "total_ms": _duration_ms(request.timestamp_start, response.timestamp_end),
    }

    _emit(
        {
            "type": "flow_completed",
            "id": flow.id,
            "session_id": SESSION_ID,
            "started_at": started_at,
            "response_size_bytes": response_size,
            "request": request_payload,
            "response": response_payload,
            "timing": timing_payload,
            "mock_rule_id": flow.metadata.get("mas_mock_rule_id"),
            "mock_rule_name": flow.metadata.get("mas_mock_rule_name"),
        }
    )


def error(flow: http.HTTPFlow) -> None:
    error_message = "Unknown proxy error"
    if flow.error is not None:
        error_message = flow.error.msg

    _emit(
        {
            "type": "flow_failed",
            "id": flow.id,
            "code": "proxy_flow_failed",
            "message": error_message,
            "mock_rule_id": flow.metadata.get("mas_mock_rule_id"),
            "mock_rule_name": flow.metadata.get("mas_mock_rule_name"),
        }
    )
