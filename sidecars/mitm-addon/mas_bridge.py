import json
import os

from mitmproxy import http

EVENT_PREFIX = "MAS_EVENT "
SESSION_ID = os.environ.get("MAS_SESSION_ID")


def _emit(payload: dict) -> None:
    print(EVENT_PREFIX + json.dumps(payload, separators=(",", ":")), flush=True)


def _millis(value: float | None) -> int | None:
    if value is None:
        return None
    return max(0, int(value * 1000))


def response(flow: http.HTTPFlow) -> None:
    request = flow.request
    response = flow.response
    if response is None:
        return

    duration_ms = None
    if request.timestamp_start is not None and response.timestamp_end is not None:
        duration_ms = _millis(response.timestamp_end - request.timestamp_start)

    response_size = None
    if response.raw_content is not None:
        response_size = len(response.raw_content)

    started_at = "0"
    if request.timestamp_start is not None:
        started_at = str(_millis(request.timestamp_start) or 0)

    _emit(
        {
            "type": "flow_completed",
            "id": flow.id,
            "session_id": SESSION_ID,
            "method": request.method,
            "host": request.pretty_host or request.host,
            "path": request.path,
            "status_code": response.status_code,
            "duration_ms": duration_ms,
            "response_size_bytes": response_size,
            "started_at": started_at,
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
