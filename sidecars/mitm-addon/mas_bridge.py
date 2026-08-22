import base64
import json
import os
from urllib.parse import urlsplit

from mitmproxy import http

EVENT_PREFIX = "MAS_EVENT "
SESSION_ID = os.environ.get("MAS_SESSION_ID")
MAX_BODY_BYTES = int(os.environ.get("MAS_MAX_BODY_BYTES", str(2 * 1024 * 1024)))


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


def _is_binary(content_type: str | None) -> bool:
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
        "is_binary": _is_binary(content_type),
        "is_truncated": is_truncated,
    }


def _duration_ms(start: float | None, end: float | None) -> int | None:
    if start is None or end is None or end < start:
        return None
    return _millis(end - start)


def response(flow: http.HTTPFlow) -> None:
    request = flow.request
    response = flow.response
    if response is None:
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
        }
    )
