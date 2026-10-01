use crate::settings_commands::{PortableFlow, PortableSession, PortableWorkspaceBundle};
use crate::{AppState, State};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use chrono::{DateTime, Utc};
use core_model::{
    AppError, BodyRef, CaptureSession, FlowDetail, FlowSource, FlowSummary, HeaderValue,
    ProtocolDetails, RequestDetail, ResponseDetail, SessionStatus, Timing, SCHEMA_VERSION,
};
use replay::{ReplayBodyDraft, ReplayDraft, ReplayHeaderDraft};
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use url::Url;

const MAX_INPUT: usize = 16 * 1024 * 1024;
const MAX_BODY: usize = 2 * 1024 * 1024;
const MAX_REQUESTS: usize = 1000;
const POSTMAN_SCHEMA: &str = "https://schema.getpostman.com/json/collection/v2.1.0/collection.json";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InterchangePreview {
    pub requests: Vec<ReplayDraft>,
    pub bundle: Option<PortableWorkspaceBundle>,
    pub warnings: Vec<String>,
}
fn invalid(message: impl Into<String>) -> AppError {
    AppError::new("interchange_invalid", message, true)
}
fn unsupported(message: impl Into<String>) -> AppError {
    AppError::new("interchange_unsupported", message, true)
}
fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str, AppError> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| invalid(format!("Missing or invalid {key}.")))
}
fn array<'a>(value: &'a Value, key: &str) -> Result<&'a Vec<Value>, AppError> {
    value
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| invalid(format!("Missing or invalid {key}.")))
}
fn validate(draft: &ReplayDraft) -> Result<(), AppError> {
    crate::replay_commands::validate_draft(draft)?;
    if draft.url.chars().any(char::is_control) {
        return Err(invalid("Invalid or oversized URL."));
    }
    let url = Url::parse(&draft.url).map_err(|_| {
        invalid("Expected an absolute HTTP(S) URL; resolve URL variables before importing.")
    })?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(invalid(
            "Expected an HTTP(S) URL without embedded credentials.",
        ));
    }
    if draft.headers.len() > 100 {
        return Err(invalid("At most 100 headers per request."));
    }
    for h in &draft.headers {
        let value = h.value.as_deref().unwrap_or("");
        if value.chars().any(|c| c.is_control() && c != '\t') {
            return Err(invalid("Invalid or oversized HTTP header."));
        }
    }
    if let Some(b) = &draft.body {
        body_bytes(b)?;
    }
    Ok(())
}
fn body_bytes(body: &ReplayBodyDraft) -> Result<Vec<u8>, AppError> {
    let bytes = if body.is_binary {
        let encoded = body
            .base64
            .as_deref()
            .ok_or_else(|| invalid("Missing binary body."))?;
        if encoded.len() > MAX_BODY.div_ceil(3) * 4 {
            return Err(invalid("Body exceeds 2 MiB."));
        }
        BASE64
            .decode(encoded)
            .map_err(|_| invalid("Invalid base64 body."))?
    } else {
        body.text.as_deref().unwrap_or("").as_bytes().to_vec()
    };
    if bytes.len() > MAX_BODY {
        return Err(invalid("Body exceeds 2 MiB."));
    }
    Ok(bytes)
}
fn body(
    bytes: Vec<u8>,
    content_type: Option<String>,
    binary: bool,
) -> Result<ReplayBodyDraft, AppError> {
    if bytes.len() > MAX_BODY {
        return Err(invalid("Body exceeds 2 MiB."));
    }
    if content_type
        .as_ref()
        .is_some_and(|s| s.len() > 1024 || s.chars().any(char::is_control))
    {
        return Err(invalid("Invalid content type."));
    }
    let text = if binary {
        None
    } else {
        String::from_utf8(bytes.clone()).ok()
    };
    Ok(ReplayBodyDraft {
        base64: text.is_none().then(|| BASE64.encode(&bytes)),
        is_binary: text.is_none(),
        text,
        content_type,
        use_original: false,
        source_truncated: false,
    })
}
fn header(name: String, value: String) -> ReplayHeaderDraft {
    ReplayHeaderDraft {
        sensitive: replay::is_sensitive_header(&name),
        name,
        value: Some(value),
        enabled: true,
        use_original: false,
        source_index: None,
    }
}
fn headers(value: &Value, key: &str, name_key: &str) -> Result<Vec<ReplayHeaderDraft>, AppError> {
    let items = array(value, key)?;
    if items.len() > 100 {
        return Err(invalid("At most 100 headers per request."));
    }
    items
        .iter()
        .filter(|h| h.get("disabled") != Some(&Value::Bool(true)))
        .map(|h| {
            Ok(header(
                string(h, name_key)?.into(),
                string(h, "value")?.into(),
            ))
        })
        .collect()
}
fn draft(
    method: String,
    url: String,
    headers: Vec<ReplayHeaderDraft>,
    body: Option<ReplayBodyDraft>,
) -> ReplayDraft {
    ReplayDraft {
        source_flow_id: "draft:import".into(),
        method,
        url,
        headers,
        body,
    }
}
pub fn preview_interchange(
    format: String,
    text: String,
    _state: State<'_, AppState>,
) -> Result<InterchangePreview, AppError> {
    parse(&format, &text)
}
fn parse(format: &str, text: &str) -> Result<InterchangePreview, AppError> {
    if text.len() > MAX_INPUT {
        return Err(invalid("Import exceeds 16 MiB."));
    }
    let mut result = InterchangePreview {
        requests: vec![],
        bundle: None,
        warnings: vec![],
    };
    match format {
        "curl" => result.requests.push(parse_curl(text)?),
        "har" => parse_har(&serde_json::from_str(text).map_err(|_| invalid("Invalid HAR JSON."))?, &mut result)?,
        "postman" => {
            let value: Value = serde_json::from_str(text).map_err(|_| invalid("Invalid Postman JSON."))?;
            let schema = value.pointer("/info/schema").and_then(Value::as_str).ok_or_else(|| invalid("Postman collection requires info.schema."))?;
            if schema != POSTMAN_SCHEMA && schema != "https://schema.getpostman.com/json/draft-07/collection/v2.1.0/" { return Err(unsupported("Only Postman collection v2.1 JSON is supported; export v2.1 JSON.")); }
            result.warnings.push("Postman scripts, auth helpers, variables and examples are not executed or imported.".into());
            let mut nodes = 0;
            postman_items(array(&value, "item")?, 0, &mut nodes, &mut result)?;
        }
        "csv" => parse_csv(text, &mut result)?,
        "charles" | "chls" | "proxyman" => return Err(unsupported("Native Charles/Proxyman session files are unsupported. Export HAR 1.2 and import it here.")),
        _ => return Err(unsupported("Supported formats: curl, har, postman, csv.")),
    }
    if result.requests.len() > MAX_REQUESTS {
        return Err(invalid("At most 1000 requests per import."));
    }
    for request in &result.requests {
        validate(request)?;
    }
    if result.requests.iter().any(|r| {
        r.url.contains("{{")
            || r.headers
                .iter()
                .any(|h| h.value.as_ref().is_some_and(|s| s.contains("{{")))
            || r.body
                .as_ref()
                .and_then(|b| b.text.as_ref())
                .is_some_and(|s| s.contains("{{"))
    }) {
        result
            .warnings
            .push("Unresolved variables remain; review and resolve them before sending.".into());
    }
    Ok(result)
}
fn parse_curl(text: &str) -> Result<ReplayDraft, AppError> {
    // Inspect shell syntax before tokenizing; no shell or subprocess receives the input.
    let mut quote = None;
    let mut escape = false;
    for c in text.chars() {
        if escape {
            escape = false;
            continue;
        }
        if quote == Some('\'') {
            if c == '\'' {
                quote = None;
            }
            continue;
        }
        if c == '\\' {
            escape = true;
            continue;
        }
        if quote == Some('"') {
            if c == '"' {
                quote = None;
            } else if c == '$' || c == '`' {
                return Err(unsupported("Shell expansion is unsupported."));
            }
            continue;
        }
        match c {
            '\'' | '"' => quote = Some(c),
            '$' | '`' | ';' | '|' | '&' | '<' | '>' | '(' | ')' | '\n' | '\r' | '#' | '~' => return Err(unsupported("Only one literal curl command is supported; shell operations and expansions are unsupported.")),
            _ => {}
        }
    }
    let args = shlex::split(text).ok_or_else(|| invalid("Unclosed curl quoting."))?;
    if args.first().map(String::as_str) != Some("curl") {
        return Err(invalid("Expected one curl command."));
    }
    let mut method = "GET".to_string();
    let mut explicit_method = false;
    let mut url = None;
    let mut hs = vec![];
    let mut payload: Option<String> = None;
    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];
        let (flag, inline) = arg
            .split_once('=')
            .map_or((arg.as_str(), None), |(a, b)| (a, Some(b)));
        match flag {
            "-X" | "--request" | "-H" | "--header" | "-d" | "--data" | "--data-raw"
            | "--data-binary" | "--url" => {
                let value = if let Some(v) = inline {
                    v
                } else {
                    i += 1;
                    args.get(i)
                        .ok_or_else(|| invalid("curl option requires a value."))?
                };
                match flag {
                    "-X" | "--request" => {
                        method = value.into();
                        explicit_method = true;
                    }
                    "-H" | "--header" => {
                        let (name, value) = value
                            .split_once(':')
                            .ok_or_else(|| invalid("curl header requires name:value."))?;
                        hs.push(header(name.trim().into(), value.trim_start().into()));
                    }
                    "--url" => {
                        if url.replace(value.to_string()).is_some() {
                            return Err(invalid("Exactly one URL is required."));
                        }
                    }
                    _ => {
                        if flag != "--data-raw" && value.starts_with('@') {
                            return Err(unsupported("curl file/stdin input is unsupported."));
                        }
                        if let Some(p) = &mut payload {
                            p.push('&');
                            p.push_str(value);
                        } else {
                            payload = Some(value.into());
                        }
                    }
                }
            }
            "-I" | "--head" => {
                method = "HEAD".into();
                explicit_method = true;
            }
            _ if arg.starts_with('-') => {
                return Err(unsupported(format!("Unsupported curl option: {flag}")))
            }
            _ => {
                if url.replace(arg.clone()).is_some() {
                    return Err(invalid("Exactly one URL is required."));
                }
            }
        }
        i += 1;
    }
    if payload.is_some() && !explicit_method {
        method = "POST".into();
    }
    if payload.is_some()
        && !hs
            .iter()
            .any(|h| h.name.eq_ignore_ascii_case("content-type"))
    {
        hs.push(header(
            "Content-Type".into(),
            "application/x-www-form-urlencoded".into(),
        ));
    }
    let b = payload
        .map(|p| body(p.into_bytes(), None, false))
        .transpose()?;
    Ok(draft(
        method,
        url.ok_or_else(|| invalid("curl requires a URL."))?,
        hs,
        b,
    ))
}
fn postman_items(
    items: &[Value],
    depth: usize,
    nodes: &mut usize,
    preview: &mut InterchangePreview,
) -> Result<(), AppError> {
    if depth > 32 {
        return Err(invalid("Postman nesting exceeds 32 levels."));
    }
    for item in items {
        *nodes += 1;
        if *nodes > 2000 || preview.requests.len() >= MAX_REQUESTS {
            return Err(invalid("Postman collection exceeds item limits."));
        }
        if let Some(nested) = item.get("item") {
            postman_items(
                nested
                    .as_array()
                    .ok_or_else(|| invalid("Invalid Postman item group."))?,
                depth + 1,
                nodes,
                preview,
            )?;
            continue;
        }
        let request = item
            .get("request")
            .ok_or_else(|| invalid("Postman item requires request or item."))?;
        if let Some(url) = request.as_str() {
            preview
                .requests
                .push(draft("GET".into(), url.into(), vec![], None));
            continue;
        }
        let url = request
            .get("url")
            .and_then(|v| v.as_str().or_else(|| v.get("raw").and_then(Value::as_str)))
            .ok_or_else(|| invalid("Postman URL requires a string or raw URL."))?;
        let mut hs = if request.get("header").is_some() {
            headers(request, "header", "key")?
        } else {
            vec![]
        };
        let mut b = None;
        if let Some(v) = request.get("body").filter(|v| !v.is_null()) {
            match string(v, "mode")? {
                "raw" => b = Some(body(string(v, "raw")?.as_bytes().to_vec(), None, false)?),
                "urlencoded" => {
                    let entries = array(v, "urlencoded")?;
                    if entries.len() > 1000 {
                        return Err(invalid("Too many form fields."));
                    }
                    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
                    for e in entries
                        .iter()
                        .filter(|e| e.get("disabled") != Some(&Value::Bool(true)))
                    {
                        serializer.append_pair(string(e, "key")?, string(e, "value")?);
                    }
                    b = Some(body(
                        serializer.finish().into_bytes(),
                        Some("application/x-www-form-urlencoded".into()),
                        false,
                    )?);
                }
                "formdata" => {
                    let entries = array(v, "formdata")?;
                    if entries.len() > 1000 {
                        return Err(invalid("Too many multipart fields."));
                    }
                    let boundary = "mas-import-form-boundary";
                    let mut content = String::new();
                    for e in entries
                        .iter()
                        .filter(|e| e.get("disabled") != Some(&Value::Bool(true)))
                    {
                        if e.get("type").and_then(Value::as_str).unwrap_or("text") != "text" {
                            return Err(unsupported(
                                "Postman file fields are unsupported; use inline text or HAR.",
                            ));
                        }
                        let name = string(e, "key")?;
                        let value = string(e, "value")?;
                        if name.contains(['\r', '\n', '"']) || value.contains(boundary) {
                            return Err(invalid("Invalid multipart field."));
                        }
                        content.push_str(&format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"));
                    }
                    content.push_str(&format!("--{boundary}--\r\n"));
                    b = Some(body(
                        content.into_bytes(),
                        Some(format!("multipart/form-data; boundary={boundary}")),
                        false,
                    )?);
                    hs.retain(|h| !h.name.eq_ignore_ascii_case("content-type"));
                }
                mode => preview.warnings.push(format!(
                    "Unsupported Postman body mode '{mode}' omitted; review before sending."
                )),
            }
        }
        if let Some(ct) = b.as_ref().and_then(|b| b.content_type.as_ref()) {
            if !hs
                .iter()
                .any(|h| h.name.eq_ignore_ascii_case("content-type"))
            {
                hs.push(header("Content-Type".into(), ct.clone()));
            }
        }
        preview
            .requests
            .push(draft(string(request, "method")?.into(), url.into(), hs, b));
    }
    Ok(())
}
fn har_body(value: &Value) -> Result<Option<ReplayBodyDraft>, AppError> {
    let Some(text) = value.get("text") else {
        if value
            .get("params")
            .and_then(Value::as_array)
            .is_some_and(|p| !p.is_empty())
        {
            return Err(unsupported(
                "HAR postData params require exported text to preserve the body.",
            ));
        }
        return Ok(None);
    };
    let text = text
        .as_str()
        .ok_or_else(|| invalid("HAR body text must be a string."))?;
    let ct = value
        .get("mimeType")
        .and_then(Value::as_str)
        .map(str::to_string);
    let encoding = value.get("encoding").and_then(Value::as_str);
    if encoding.is_some_and(|e| e != "base64") {
        return Err(unsupported("Unsupported HAR body encoding."));
    }
    if encoding == Some("base64") {
        if text.len() > MAX_BODY.div_ceil(3) * 4 {
            return Err(invalid("HAR body exceeds 2 MiB."));
        }
        return Ok(Some(body(
            BASE64
                .decode(text)
                .map_err(|_| invalid("Invalid HAR base64."))?,
            ct,
            true,
        )?));
    }
    Ok(Some(body(text.as_bytes().to_vec(), ct, false)?))
}
fn refs(body: Option<&ReplayBodyDraft>) -> Result<(Option<BodyRef>, Option<String>), AppError> {
    let Some(b) = body else {
        return Ok((None, None));
    };
    let bytes = body_bytes(b)?;
    Ok((
        Some(BodyRef {
            sha256: format!("{:x}", Sha256::digest(&bytes)),
            byte_size: bytes.len() as u64,
            content_type: b.content_type.clone(),
            encoding: None,
            is_binary: b.is_binary,
            is_truncated: false,
        }),
        Some(BASE64.encode(bytes)),
    ))
}
fn values(hs: &[ReplayHeaderDraft]) -> Vec<HeaderValue> {
    hs.iter()
        .filter(|h| h.enabled)
        .map(|h| HeaderValue {
            name: h.name.clone(),
            value: h.value.clone().unwrap_or_default(),
            sensitive: h.sensitive,
        })
        .collect()
}
fn request_detail(r: &ReplayDraft, body: Option<BodyRef>) -> Result<RequestDetail, AppError> {
    validate(r)?;
    let url = Url::parse(&r.url).map_err(|_| invalid("Invalid URL."))?;
    Ok(RequestDetail {
        method: r.method.clone(),
        url: r.url.clone(),
        scheme: url.scheme().into(),
        host: url.host_str().unwrap_or_default().into(),
        port: url.port(),
        path: url.path().into(),
        query: url.query().map(str::to_string),
        headers: values(&r.headers),
        body,
    })
}
fn time(value: &str) -> Result<String, AppError> {
    if let Ok(ms) = value.parse::<i64>() {
        return DateTime::<Utc>::from_timestamp_millis(ms)
            .map(|d| d.to_rfc3339())
            .ok_or_else(|| invalid("Timestamp out of range."));
    }
    DateTime::parse_from_rfc3339(value)
        .map(|d| d.to_rfc3339())
        .map_err(|_| invalid("Invalid ISO timestamp."))
}
fn har_duration(v: &Value) -> Result<Option<u64>, AppError> {
    if v.is_null() {
        return Ok(None);
    }
    let n = v.as_f64().ok_or_else(|| invalid("Invalid HAR timing."))?;
    if !n.is_finite() || n < -1.0 || n > 86_400_000.0 {
        return Err(invalid("HAR timing out of bounds."));
    }
    Ok((n >= 0.0).then_some(n.round() as u64))
}
fn parse_har(value: &Value, preview: &mut InterchangePreview) -> Result<(), AppError> {
    let log = value
        .get("log")
        .ok_or_else(|| invalid("HAR requires log."))?;
    if string(log, "version")? != "1.2" {
        return Err(unsupported("Only HAR 1.2 is supported."));
    }
    let entries = array(log, "entries")?;
    if entries.len() > MAX_REQUESTS {
        return Err(invalid("HAR exceeds 1000 entries."));
    }
    let id = format!("har-import-{}", crate::now_epoch_millis()?);
    let mut flows = vec![];
    for (i, e) in entries.iter().enumerate() {
        let req = e
            .get("request")
            .ok_or_else(|| invalid("Missing HAR request."))?;
        let rb = req.get("postData").map(har_body).transpose()?.flatten();
        let r = draft(
            string(req, "method")?.into(),
            string(req, "url")?.into(),
            headers(req, "headers", "name")?,
            rb,
        );
        let (body_ref, request_body_base64) = refs(r.body.as_ref())?;
        let request = request_detail(&r, body_ref)?;
        let resp = e
            .get("response")
            .ok_or_else(|| invalid("Missing HAR response."))?;
        let code = resp
            .get("status")
            .and_then(Value::as_u64)
            .filter(|c| *c <= 599)
            .ok_or_else(|| invalid("Invalid HAR response status."))? as u16;
        let response_headers = headers(resp, "headers", "name")?;
        validate(&draft(
            "GET".into(),
            r.url.clone(),
            response_headers.clone(),
            None,
        ))?;
        let content = resp
            .get("content")
            .ok_or_else(|| invalid("Missing HAR response content."))?;
        let response_body = har_body(content)?;
        let (response_body_ref, response_body_base64) = refs(response_body.as_ref())?;
        let started_at = time(string(e, "startedDateTime")?)?;
        let duration_ms = har_duration(&e["time"])?;
        let timings = &e["timings"];
        let timing = Timing {
            total_ms: duration_ms,
            dns_ms: har_duration(&timings["dns"])?,
            connect_ms: har_duration(&timings["connect"])?,
            tls_ms: har_duration(&timings["ssl"])?,
            request_ms: har_duration(&timings["send"])?,
            server_ms: har_duration(&timings["wait"])?,
            download_ms: har_duration(&timings["receive"])?,
        };
        let summary = FlowSummary {
            schema_version: SCHEMA_VERSION,
            id: format!("{id}-{i}"),
            session_id: Some(id.clone()),
            source: FlowSource::Proxy,
            method: r.method.clone(),
            host: request.host.clone(),
            path: request.path.clone(),
            status_code: Some(code),
            duration_ms,
            response_size_bytes: response_body_ref.as_ref().map(|b| b.byte_size),
            started_at,
        };
        let req_version = string(req, "httpVersion")?;
        let resp_version = string(resp, "httpVersion")?;
        if req_version.len() > 32 || resp_version.len() > 32 {
            return Err(invalid("Invalid HTTP version."));
        }
        let detail = FlowDetail {
            summary: summary.clone(),
            request: Some(request),
            response: Some(ResponseDetail {
                status_code: code,
                reason: Some(string(resp, "statusText")?.into()),
                headers: values(&response_headers),
                body: response_body_ref,
            }),
            timing,
            error_code: None,
            error_message: None,
            proxy_rule_ids: vec![],
            proxy_rule_changes: vec![],
            protocol: Some(ProtocolDetails {
                request_http_version: Some(req_version.into()),
                response_http_version: Some(resp_version.into()),
                ..Default::default()
            }),
        };
        flows.push(PortableFlow {
            summary,
            detail: Some(detail),
            request_body_base64,
            response_body_base64,
        });
        preview.requests.push(r);
    }
    let timestamp = flows
        .first()
        .map(|f| f.summary.started_at.clone())
        .unwrap_or_else(|| "1970-01-01T00:00:00+00:00".into());
    preview.bundle = Some(PortableWorkspaceBundle {
        bundle_version: 6,
        exported_at: crate::now_epoch_millis()?,
        sessions: vec![PortableSession {
            session: CaptureSession {
                schema_version: SCHEMA_VERSION,
                id,
                name: "Imported HAR".into(),
                status: SessionStatus::Archived,
                started_at: timestamp.clone(),
                ended_at: Some(timestamp),
                device_id: None,
                app_id: None,
                connection_strategy: None,
                capture_engine: Some("har".into()),
                notes: None,
                capture_target: None,
                capture_mode: None,
            },
            flows,
        }],
        collections: vec![],
        saved_requests: vec![],
        environments: vec![],
        environment_variables: vec![],
        proxy_rules: vec![],
        network_profiles: vec![],
        websocket_messages: vec![],
    });
    preview.warnings.push("HAR cookies, cache metadata and unsupported extensions are omitted; request/response headers, bodies, HTTP versions and timings are preserved.".into());
    Ok(())
}
fn csv_rows(text: &str) -> Result<Vec<Vec<String>>, AppError> {
    let mut rows = vec![];
    let mut row = vec![];
    let mut field = String::new();
    let mut quoted = false;
    let mut closed = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    quoted = false;
                    closed = true;
                }
            } else {
                field.push(c);
            }
            continue;
        }
        match c {
            '"' if field.is_empty() && !closed => quoted = true,
            ',' => {
                row.push(std::mem::take(&mut field));
                closed = false;
            }
            '\n' | '\r' => {
                if c == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
                closed = false;
                if rows.len() > MAX_REQUESTS + 1 {
                    return Err(invalid("CSV exceeds 1000 requests."));
                }
            }
            '"' => return Err(invalid("Invalid CSV quote.")),
            _ if closed => return Err(invalid("Unexpected text after CSV closing quote.")),
            _ => field.push(c),
        }
        if row.len() > 100 {
            return Err(invalid("Too many CSV columns."));
        }
    }
    if quoted {
        return Err(invalid("Unclosed CSV quote."));
    }
    if !field.is_empty() || !row.is_empty() || closed {
        row.push(field);
        rows.push(row);
    }
    Ok(rows)
}
fn parse_csv(text: &str, result: &mut InterchangePreview) -> Result<(), AppError> {
    let rows = csv_rows(text)?;
    let Some(columns) = rows.first() else {
        return Err(invalid("CSV requires a header row."));
    };
    let field = |row: &[String], key: &str| -> Option<String> {
        columns
            .iter()
            .position(|c| c == key)
            .and_then(|i| row.get(i))
            .cloned()
    };
    if !columns.iter().any(|c| c == "method") || !columns.iter().any(|c| c == "url") {
        return Err(invalid("CSV requires method and url columns."));
    }
    if columns.iter().collect::<HashSet<_>>().len() != columns.len() {
        return Err(invalid("CSV has duplicate columns."));
    }
    for row in rows.iter().skip(1) {
        if row.len() != columns.len() {
            return Err(invalid("CSV row width does not match header."));
        }
        let hs: Vec<HeaderValue> =
            serde_json::from_str(&field(row, "headers").unwrap_or_else(|| "[]".into()))
                .map_err(|_| invalid("CSV headers must be a JSON array of header values."))?;
        let text = field(row, "bodyText").unwrap_or_default();
        let encoded = field(row, "bodyBase64").unwrap_or_default();
        let ct = field(row, "contentType").unwrap_or_default();
        if !encoded.is_empty() && !text.is_empty() {
            return Err(invalid("CSV body must use text or base64, not both."));
        }
        let b = if !encoded.is_empty() {
            if encoded.len() > MAX_BODY.div_ceil(3) * 4 {
                return Err(invalid("CSV body exceeds 2 MiB."));
            }
            Some(body(
                BASE64
                    .decode(&encoded)
                    .map_err(|_| invalid("Invalid CSV base64."))?,
                (!ct.is_empty()).then_some(ct),
                true,
            )?)
        } else if !text.is_empty() {
            Some(body(
                text.into_bytes(),
                (!ct.is_empty()).then_some(ct),
                false,
            )?)
        } else {
            None
        };
        result.requests.push(draft(
            field(row, "method").unwrap_or_default(),
            field(row, "url").unwrap_or_default(),
            hs.into_iter().map(|h| header(h.name, h.value)).collect(),
            b,
        ));
    }
    Ok(())
}
fn redact(hs: &[ReplayHeaderDraft]) -> Vec<ReplayHeaderDraft> {
    hs.iter()
        .filter(|h| {
            !h.name
                .eq_ignore_ascii_case("x-mobile-api-studio-request-id")
        })
        .map(|h| {
            let mut h = h.clone();
            if h.sensitive || replay::is_sensitive_header(&h.name) {
                h.value = Some("<redacted>".into());
                h.sensitive = true;
            }
            h.use_original = false;
            h.source_index = None;
            h
        })
        .collect()
}
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
pub fn export_interchange(
    format: String,
    flow_ids: Vec<String>,
    saved_request_ids: Vec<String>,
    state: State<'_, AppState>,
) -> Result<String, AppError> {
    if flow_ids.len() + saved_request_ids.len() > MAX_REQUESTS {
        return Err(invalid("Select at most 1000 requests."));
    }
    let mut seen = HashSet::new();
    let mut requests = vec![];
    let mut details = vec![];
    for id in flow_ids {
        if !seen.insert(format!("flow:{id}")) {
            continue;
        }
        let detail = state
            .database
            .get_flow_detail(&id)
            .map_err(|e| AppError::storage(e.to_string()))?
            .ok_or_else(|| invalid("Selected flow was not found."))?;
        let r = detail
            .request
            .as_ref()
            .ok_or_else(|| invalid("Selected flow has no request details."))?;
        let b = r
            .body
            .as_ref()
            .map(|b| {
                state
                    .body_store
                    .read(&b.sha256)
                    .map_err(|e| AppError::storage(e.to_string()))
                    .and_then(|bytes| body(bytes, b.content_type.clone(), b.is_binary))
            })
            .transpose()?;
        requests.push(draft(
            r.method.clone(),
            r.url.clone(),
            r.headers
                .iter()
                .map(|h| {
                    let mut x = header(h.name.clone(), h.value.clone());
                    x.sensitive |= h.sensitive;
                    x
                })
                .collect(),
            b,
        ));
        details.push(Some(detail));
    }
    let saved = state
        .database
        .list_saved_requests(None)
        .map_err(|e| AppError::storage(e.to_string()))?;
    for id in saved_request_ids {
        if !seen.insert(format!("saved:{id}")) {
            continue;
        }
        let r = saved
            .iter()
            .find(|r| r.id == id)
            .ok_or_else(|| invalid("Selected saved request was not found."))?;
        requests.push(draft(
            r.method.clone(),
            r.url.clone(),
            r.headers
                .iter()
                .map(|h| {
                    let mut x = header(h.name.clone(), h.value.clone());
                    x.sensitive |= h.sensitive;
                    x
                })
                .collect(),
            r.body.as_ref().map(|b| ReplayBodyDraft {
                text: b.text.clone(),
                base64: b.base64.clone(),
                content_type: b.content_type.clone(),
                is_binary: b.is_binary,
                use_original: false,
                source_truncated: false,
            }),
        ));
        details.push(None);
    }
    for r in &mut requests {
        validate(r)?;
        r.headers = redact(&r.headers);
    }
    let result = match format.as_str() {
        "curl" => {
            if requests.len() != 1 {
                return Err(unsupported(
                    "curl export requires exactly one selected request.",
                ));
            }
            let r = &requests[0];
            let mut command = format!(
                "curl --request {} --url {}",
                shell_quote(&r.method),
                shell_quote(&r.url)
            );
            for h in &r.headers {
                command.push_str(&format!(
                    " --header {}",
                    shell_quote(&format!(
                        "{}: {}",
                        h.name,
                        h.value.as_deref().unwrap_or_default()
                    ))
                ));
            }
            if let Some(b) = &r.body {
                if b.is_binary {
                    return Err(unsupported("Binary bodies cannot be represented by a literal curl command; export HAR or CSV."));
                }
                command.push_str(&format!(
                    " --data-raw {}",
                    shell_quote(b.text.as_deref().unwrap_or_default())
                ));
            }
            command
        }
        "postman" => {
            let mut items = vec![];
            for r in &requests {
                let mut req = json!({"method":r.method,"url":r.url,"header":r.headers.iter().map(|h| json!({"key":h.name,"value":h.value.as_deref().unwrap_or_default(),"type":"text"})).collect::<Vec<_>>()});
                if let Some(b) = &r.body {
                    if b.is_binary {
                        return Err(unsupported(
                            "Postman raw export cannot preserve binary bodies; export HAR or CSV.",
                        ));
                    }
                    req["body"] = json!({"mode":"raw","raw":b.text.as_deref().unwrap_or_default()});
                }
                items.push(json!({"name":format!("{} {}",r.method,r.url),"request":req}));
            }
            serde_json::to_string_pretty(&json!({"info":{"name":"Mobile API Studio export","schema":POSTMAN_SCHEMA},"item":items})).map_err(|e| invalid(e.to_string()))?
        }
        "csv" => {
            let mut rows = vec![vec![
                "method",
                "url",
                "headers",
                "bodyText",
                "bodyBase64",
                "contentType",
                "statusCode",
                "startedAt",
                "durationMs",
            ]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>()];
            for (r, d) in requests.iter().zip(&details) {
                rows.push(vec![
                    r.method.clone(),
                    r.url.clone(),
                    serde_json::to_string(&values(&r.headers))
                        .map_err(|e| invalid(e.to_string()))?,
                    r.body
                        .as_ref()
                        .and_then(|b| b.text.clone())
                        .unwrap_or_default(),
                    r.body
                        .as_ref()
                        .and_then(|b| b.base64.clone())
                        .unwrap_or_default(),
                    r.body
                        .as_ref()
                        .and_then(|b| b.content_type.clone())
                        .unwrap_or_default(),
                    d.as_ref()
                        .and_then(|d| d.summary.status_code)
                        .map(|c| c.to_string())
                        .unwrap_or_default(),
                    d.as_ref()
                        .map(|d| d.summary.started_at.clone())
                        .unwrap_or_default(),
                    d.as_ref()
                        .and_then(|d| d.summary.duration_ms)
                        .map(|n| n.to_string())
                        .unwrap_or_default(),
                ]);
            }
            rows.iter()
                .map(|row| {
                    row.iter()
                        .map(|s| format!("\"{}\"", s.replace('"', "\"\"")))
                        .collect::<Vec<_>>()
                        .join(",")
                })
                .collect::<Vec<_>>()
                .join("\r\n")
        }
        "har" => export_har(&requests, &details, &state)?,
        "charles" | "chls" | "proxyman" => {
            return Err(unsupported(
                "Native session export unsupported; use HAR 1.2.",
            ))
        }
        _ => return Err(unsupported("Supported formats: curl, har, postman, csv.")),
    };
    if result.len() > MAX_INPUT {
        return Err(invalid("Export exceeds 16 MiB; select fewer requests."));
    }
    Ok(result)
}
fn export_har(
    requests: &[ReplayDraft],
    details: &[Option<FlowDetail>],
    state: &State<'_, AppState>,
) -> Result<String, AppError> {
    let mut entries = vec![];
    for (r, d) in requests.iter().zip(details) {
        let parsed = Url::parse(&r.url).map_err(|_| invalid("Invalid URL."))?;
        let mut req = json!({"method":r.method,"url":r.url,"httpVersion":d.as_ref().and_then(|d| d.protocol.as_ref()).and_then(|p| p.request_http_version.clone()).unwrap_or_else(|| "HTTP/1.1".into()),"headers":r.headers.iter().map(|h| json!({"name":h.name,"value":h.value.as_deref().unwrap_or_default()})).collect::<Vec<_>>(),"cookies":[],"queryString":parsed.query_pairs().map(|(n,v)| json!({"name":n,"value":v})).collect::<Vec<_>>(),"headersSize":-1,"bodySize":r.body.as_ref().map(body_bytes).transpose()?.map(|b|b.len()).unwrap_or(0)});
        if let Some(b) = &r.body {
            req["postData"] = har_content(b, false)?;
        }
        let response = d.as_ref().and_then(|d| d.response.as_ref());
        let response_body = response
            .and_then(|r| r.body.as_ref())
            .map(|b| {
                state
                    .body_store
                    .read(&b.sha256)
                    .map_err(|e| AppError::storage(e.to_string()))
                    .and_then(|bytes| body(bytes, b.content_type.clone(), b.is_binary))
            })
            .transpose()?;
        let rh = response
            .map(|r| {
                r.headers
                    .iter()
                    .map(|h| {
                        let mut x = header(h.name.clone(), h.value.clone());
                        x.sensitive |= h.sensitive;
                        x
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let rh = redact(&rh);
        validate(&draft("GET".into(), r.url.clone(), rh.clone(), None))?;
        let content = response_body
            .as_ref()
            .map(|b| har_content(b, true))
            .transpose()?
            .unwrap_or_else(|| json!({"size":0,"mimeType":"application/octet-stream"}));
        let timing = d.as_ref().map(|d| d.timing.clone()).unwrap_or_default();
        let number = |n: Option<u64>| n.map(|n| json!(n)).unwrap_or(json!(-1));
        entries.push(json!({"startedDateTime":time(d.as_ref().map(|d|d.summary.started_at.as_str()).unwrap_or("1970-01-01T00:00:00Z"))?,"time":timing.total_ms.unwrap_or(0),"request":req,"response":{"status":response.map(|r|r.status_code).unwrap_or(0),"statusText":response.and_then(|r|r.reason.clone()).unwrap_or_default(),"httpVersion":d.as_ref().and_then(|d|d.protocol.as_ref()).and_then(|p|p.response_http_version.clone()).unwrap_or_else(||"HTTP/1.1".into()),"headers":rh.iter().map(|h|json!({"name":h.name,"value":h.value.as_deref().unwrap_or_default()})).collect::<Vec<_>>(),"cookies":[],"content":content,"redirectURL":"","headersSize":-1,"bodySize":response_body.as_ref().map(body_bytes).transpose()?.map(|b|b.len()).unwrap_or(0)},"cache":{},"timings":{"blocked":-1,"dns":number(timing.dns_ms),"connect":number(timing.connect_ms),"ssl":number(timing.tls_ms),"send":timing.request_ms.unwrap_or(0),"wait":timing.server_ms.unwrap_or(0),"receive":timing.download_ms.unwrap_or(0)}}));
    }
    serde_json::to_string_pretty(&json!({"log":{"version":"1.2","creator":{"name":"Mobile API Studio","version":"0.5"},"entries":entries}})).map_err(|e|invalid(e.to_string()))
}
fn har_content(b: &ReplayBodyDraft, response: bool) -> Result<Value, AppError> {
    let bytes = body_bytes(b)?;
    let mut v = json!({"mimeType":b.content_type.as_deref().unwrap_or("application/octet-stream"),"text":if b.is_binary {b.base64.as_deref().unwrap_or_default()} else {b.text.as_deref().unwrap_or_default()}});
    if b.is_binary {
        v["encoding"] = json!("base64");
    }
    if response {
        v["size"] = json!(bytes.len());
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selected_export_roundtrips_binary_and_preview_is_read_only() {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
            let root = std::env::temp_dir().join(format!("mas-interchange-{}-{}", std::process::id(), crate::now_epoch_millis().unwrap()));
            let state = crate::initialize_state(root.clone(), crate::resolve_addon_path().unwrap()).unwrap();
            let before = state.database.list_sessions(100).unwrap();
            assert!(preview_interchange("har".into(), "{\"log\":{\"version\":\"1.2\",\"entries\":[{}]}}".into(), State(&state)).is_err());
            assert_eq!(state.database.list_sessions(100).unwrap(), before);
            assert!(state.database.list_flows(100).unwrap().is_empty());
            let source = json!({"log":{"version":"1.2","entries":[{"startedDateTime":"2026-10-01T00:00:00Z","time":3,"request":{"method":"POST","url":"https://example.test","httpVersion":"HTTP/2","headers":[{"name":"Authorization","value":"top-secret"},{"name":"X-Mobile-API-Studio-Request-ID","value":"sdk-id"}],"postData":{"mimeType":"application/octet-stream","text":"AP8=","encoding":"base64"}},"response":{"status":200,"statusText":"OK","httpVersion":"HTTP/2","headers":[{"name":"Set-Cookie","value":"secret-cookie"}],"content":{"mimeType":"application/octet-stream","text":"/wA=","encoding":"base64"}},"timings":{"send":1,"wait":1,"receive":1}}]}});
            let preview = preview_interchange("har".into(), source.to_string(), State(&state)).unwrap();
            assert!(state.database.list_flows(100).unwrap().is_empty());
            let bundle = preview.bundle.unwrap();
            let session = &bundle.sessions[0];
            state.database.create_session(&session.session).unwrap();
            let flow = &session.flows[0];
            for encoded in [&flow.request_body_base64,&flow.response_body_base64].into_iter().flatten() {
                state.body_store.put(&BASE64.decode(encoded).unwrap()).unwrap();
            }
            let detail = flow.detail.as_ref().unwrap();
            state.database.upsert_flow_detail(detail).unwrap();
            let mut other = detail.clone(); other.summary.id = "unselected".into(); other.request.as_mut().unwrap().url = "https://unselected.test".into();
            state.database.upsert_flow_detail(&other).unwrap();
            let exported = export_interchange("har".into(),vec![detail.summary.id.clone()],vec![],State(&state)).unwrap();
            assert!(!exported.contains("top-secret") && !exported.contains("secret-cookie") && !exported.contains("sdk-id") && !exported.contains("unselected.test"));
            let preview = preview_interchange("har".into(),exported,State(&state)).unwrap();
            assert_eq!(preview.requests.len(),1);
            assert_eq!(body_bytes(preview.requests[0].body.as_ref().unwrap()).unwrap(),vec![0,255]);
            assert_eq!(preview.bundle.unwrap().sessions[0].flows[0].response_body_base64.as_deref(),Some("/wA="));
            let csv = export_interchange("csv".into(),vec![detail.summary.id.clone()],vec![],State(&state)).unwrap();
            assert_eq!(body_bytes(parse("csv",&csv).unwrap().requests[0].body.as_ref().unwrap()).unwrap(),vec![0,255]);
            assert!(export_interchange("curl".into(),vec![detail.summary.id.clone()],vec![],State(&state)).is_err());
            assert_eq!(state.database.list_flows(100).unwrap().len(),2);
            drop(state); std::fs::remove_dir_all(root).unwrap();
        });
    }
    #[test]
    fn interchange_bounds_quotes_nested_items_and_binary_har() {
        let r=parse("curl","curl -X POST -H 'X-Test: two words' --data-raw 'it'\\''s $literal' 'https://example.test/a'").unwrap();
        assert_eq!(
            r.requests[0].body.as_ref().unwrap().text.as_deref(),
            Some("it's $literal")
        );
        for c in [
            "curl https://example.test; touch /tmp/no",
            "curl --data @/etc/passwd https://example.test",
            "curl \"https://example.test/$TOKEN\"",
            "curl -k https://example.test",
        ] {
            assert!(parse("curl", c).is_err());
        }
        assert!(parse("curl", "curl https://user:pass@example.test").is_err());
        assert!(parse("curl", &"x".repeat(MAX_INPUT + 1)).is_err());
        assert_eq!(
            csv_rows("method,url\r\n\"P,OST\",\"a\"\"b\nnext\"\r\n").unwrap()[1],
            vec!["P,OST", "a\"b\nnext"]
        );
        assert!(csv_rows("x\n\"unterminated").is_err());
        let p = json!({"info":{"schema":POSTMAN_SCHEMA},"item":[{"item":[{"request":{"method":"POST","url":{"raw":"https://example.test"},"header":[],"body":{"mode":"urlencoded","urlencoded":[{"key":"a b","value":"x&y"}]}}}]}]});
        let r = parse("postman", &p.to_string()).unwrap();
        assert_eq!(r.requests.len(), 1);
        assert_eq!(
            r.requests[0].body.as_ref().unwrap().text.as_deref(),
            Some("a+b=x%26y")
        );
        let h = json!({"log":{"version":"1.2","entries":[{"startedDateTime":"2026-10-01T00:00:00Z","time":3.5,"request":{"method":"POST","url":"https://example.test","httpVersion":"HTTP/2","headers":[],"postData":{"mimeType":"application/octet-stream","text":"AP8=","encoding":"base64"}},"response":{"status":200,"statusText":"OK","httpVersion":"HTTP/2","headers":[],"content":{"mimeType":"application/octet-stream","text":"/wA=","encoding":"base64"}},"timings":{"send":1,"wait":1,"receive":1}}]}});
        let r = parse("har", &h.to_string()).unwrap();
        assert_eq!(
            body_bytes(r.requests[0].body.as_ref().unwrap()).unwrap(),
            vec![0, 255]
        );
        let bundle = r.bundle.unwrap();
        let f = &bundle.sessions[0].flows[0];
        assert_eq!(f.response_body_base64.as_deref(), Some("/wA="));
        assert_eq!(
            f.detail
                .as_ref()
                .unwrap()
                .protocol
                .as_ref()
                .unwrap()
                .request_http_version
                .as_deref(),
            Some("HTTP/2")
        );
        assert!(r.requests[0]
            .headers
            .iter()
            .all(|h| !h.use_original && h.source_index.is_none()));
        assert!(parse("charles", "anything").unwrap_err().code == "interchange_unsupported");
    }
}
