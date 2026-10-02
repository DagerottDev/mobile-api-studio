use crate::{ApiResult, bad};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::Value;
use std::collections::HashSet;

// Reject duplicate object keys: otherwise the validator and a HAR reader could see different values.
pub struct StrictValue(pub Value);
impl<'de> serde::Deserialize<'de> for StrictValue {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = StrictValue;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                write!(f, "JSON without duplicate fields")
            }
            fn visit_bool<E: serde::de::Error>(self, v: bool) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::Bool(v)))
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Self::Value, E> {
                Ok(StrictValue(v.into()))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(StrictValue(v.into()))
            }
            fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<Self::Value, E> {
                serde_json::Number::from_f64(v)
                    .map(|n| StrictValue(Value::Number(n)))
                    .ok_or_else(|| E::custom("Nonfinite number"))
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::String(v.into())))
            }
            fn visit_string<E: serde::de::Error>(self, v: String) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::String(v)))
            }
            fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::Null))
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::Null))
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<Self::Value, A::Error> {
                let mut items = Vec::new();
                while let Some(StrictValue(v)) = seq.next_element()? {
                    items.push(v);
                }
                Ok(StrictValue(Value::Array(items)))
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Self::Value, A::Error> {
                let mut object = serde_json::Map::new();
                while let Some((key, StrictValue(v))) = map.next_entry::<String, StrictValue>()? {
                    if object.insert(key, v).is_some() {
                        return Err(serde::de::Error::custom("Duplicate JSON field"));
                    }
                }
                Ok(StrictValue(Value::Object(object)))
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}

const MAX_ARTIFACT: usize = 4 * 1024 * 1024;
fn object<'a>(v: &'a Value, keys: &[&str]) -> ApiResult<&'a serde_json::Map<String, Value>> {
    let o = v.as_object().ok_or_else(|| bad("Expected object"))?;
    if o.keys().any(|k| !keys.contains(&k.as_str())) {
        return Err(bad(
            "Unsupported field; only portable redacted definitions are accepted",
        ));
    }
    Ok(o)
}
fn text<'a>(v: &'a Value, max: usize) -> ApiResult<&'a str> {
    let s = v.as_str().ok_or_else(|| bad("Expected text"))?;
    if s.len() > max {
        return Err(bad("Text limit exceeded"));
    }
    if s.chars().any(|c| c == '\0') {
        return Err(bad("NUL text is not supported"));
    }
    material(s.as_bytes())?;
    Ok(s)
}
fn arr(v: &Value, max: usize) -> ApiResult<&Vec<Value>> {
    let a = v.as_array().ok_or_else(|| bad("Expected array"))?;
    if a.len() > max {
        return Err(bad("Item limit exceeded"));
    }
    Ok(a)
}
fn number(v: &Value, min: i64, max: i64) -> ApiResult<()> {
    if v.as_i64().is_some_and(|n| (min..=max).contains(&n)) {
        Ok(())
    } else {
        Err(bad("Invalid number"))
    }
}
fn redacted(s: &str) -> bool {
    ["<redacted>", "[redacted]", "redacted", ""].contains(&s.to_ascii_lowercase().as_str())
}
fn secret(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    [
        "authorization",
        "proxy-authorization",
        "cookie",
        "set-cookie",
        "x-api-key",
        "api-key",
        "x-auth-token",
        "x-access-token",
        "x-csrf-token",
        "x-xsrf-token",
        "x-amz-security-token",
    ]
    .contains(&name.as_str())
        || name.contains("secret")
        || name.ends_with("-token")
        || name.ends_with("-api-key")
}
fn material(bytes: &[u8]) -> ApiResult<()> {
    let lower = String::from_utf8_lossy(bytes).to_ascii_lowercase();
    if [
        "-----begin ",
        "private_key",
        "privatekey",
        "client_secret",
        "aws_secret_access_key",
    ]
    .iter()
    .any(|s| lower.contains(s))
    {
        return Err(bad("Private or credential material cannot be shared"));
    }
    Ok(())
}
fn url(value: &Value) -> ApiResult<()> {
    let s = text(value, 8192)?;
    let parsed = url::Url::parse(s).map_err(|_| bad("Invalid URL"))?;
    if !["http", "https"].contains(&parsed.scheme())
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.fragment().is_some()
    {
        return Err(bad(
            "Only HTTP(S) URLs without credentials or fragments are allowed",
        ));
    }
    for (key, value) in parsed.query_pairs() {
        if secret(&key)
            || [
                "password",
                "passwd",
                "access_token",
                "refresh_token",
                "token",
                "api_key",
                "apikey",
            ]
            .contains(&key.to_ascii_lowercase().as_str())
        {
            if !redacted(&value) {
                return Err(bad("Known secret query parameters must be redacted"));
            }
        }
    }
    Ok(())
}
fn headers(value: &Value, mutation: bool) -> ApiResult<()> {
    for h in arr(value, 256)? {
        let keys = if mutation {
            vec!["name", "value", "remove"]
        } else {
            vec!["name", "value"]
        };
        object(h, &keys)?;
        let name = text(&h["name"], 256)?;
        if name.is_empty()
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
        {
            return Err(bad("Invalid header name"));
        }
        if name.eq_ignore_ascii_case("x-mobile-api-studio-request-id")
            || name
                .to_ascii_lowercase()
                .starts_with("x-mobile-api-studio-")
        {
            return Err(bad("SDK correlation must be removed"));
        }
        let value = if mutation && h["value"].is_null() {
            ""
        } else {
            text(&h["value"], 16384)?
        };
        if value.contains(['\r', '\n']) {
            return Err(bad("Invalid header value"));
        }
        if secret(name) && !redacted(value) {
            return Err(bad("Known secret headers must be redacted"));
        }
        if mutation && !h["remove"].is_boolean() {
            return Err(bad("Header mutation requires remove boolean"));
        }
    }
    Ok(())
}
fn content(v: &Value, response: bool) -> ApiResult<()> {
    object(
        v,
        if response {
            &["size", "mimeType", "text", "encoding"]
        } else {
            &["mimeType", "text", "encoding"]
        },
    )?;
    text(&v["mimeType"], 256)?;
    if response {
        number(&v["size"], 0, MAX_ARTIFACT as i64)?;
    }
    if !v["text"].is_null() {
        let s = text(&v["text"], MAX_ARTIFACT)?;
        if !v["encoding"].is_null() {
            if v["encoding"] != "base64" {
                return Err(bad("Only base64 content encoding is supported"));
            }
            material(
                &STANDARD
                    .decode(s)
                    .map_err(|_| bad("Invalid base64 content"))?,
            )?;
        }
    } else if !v["encoding"].is_null() {
        return Err(bad("Encoding requires text"));
    }
    Ok(())
}
pub fn har(artifact: &str) -> ApiResult<()> {
    if artifact.len() > MAX_ARTIFACT {
        return Err(bad("HAR exceeds 4 MiB"));
    }
    material(artifact.as_bytes())?;
    let StrictValue(v) =
        serde_json::from_str(artifact).map_err(|_| bad("Malformed or ambiguous HAR JSON"))?;
    object(&v, &["log"])?;
    let log = &v["log"];
    object(log, &["version", "creator", "entries"])?;
    if log["version"] != "1.2" {
        return Err(bad("HAR 1.2 required"));
    }
    object(&log["creator"], &["name", "version"])?;
    text(&log["creator"]["name"], 128)?;
    text(&log["creator"]["version"], 64)?;
    let entries = arr(&log["entries"], 500)?;
    if entries.is_empty() {
        return Err(bad("Select at least one flow"));
    }
    for entry in entries {
        object(
            entry,
            &[
                "startedDateTime",
                "time",
                "request",
                "response",
                "cache",
                "timings",
            ],
        )?;
        text(&entry["startedDateTime"], 64)?;
        number(&entry["time"], 0, 604800000)?;
        object(&entry["cache"], &[])?;
        let request = &entry["request"];
        object(
            request,
            &[
                "method",
                "url",
                "httpVersion",
                "headers",
                "cookies",
                "queryString",
                "headersSize",
                "bodySize",
                "postData",
            ],
        )?;
        let method = text(&request["method"], 32)?;
        if method.is_empty() || !method.bytes().all(|b| b.is_ascii_uppercase() || b == b'-') {
            return Err(bad("Invalid method"));
        }
        url(&request["url"])?;
        text(&request["httpVersion"], 32)?;
        headers(&request["headers"], false)?;
        if !arr(&request["cookies"], 0)?.is_empty() {
            return Err(bad("Cookies must be removed"));
        }
        for pair in arr(&request["queryString"], 256)? {
            object(pair, &["name", "value"])?;
            let name = text(&pair["name"], 1024)?;
            let value = text(&pair["value"], 16384)?;
            if (secret(name)
                || [
                    "password",
                    "token",
                    "access_token",
                    "refresh_token",
                    "api_key",
                ]
                .contains(&name.to_ascii_lowercase().as_str()))
                && !redacted(value)
            {
                return Err(bad("Secret query values must be redacted"));
            }
        }
        number(&request["headersSize"], -1, MAX_ARTIFACT as i64)?;
        number(&request["bodySize"], -1, MAX_ARTIFACT as i64)?;
        if !request["postData"].is_null() {
            content(&request["postData"], false)?;
        }
        let response = &entry["response"];
        object(
            response,
            &[
                "status",
                "statusText",
                "httpVersion",
                "headers",
                "cookies",
                "content",
                "redirectURL",
                "headersSize",
                "bodySize",
            ],
        )?;
        number(&response["status"], 0, 599)?;
        text(&response["statusText"], 128)?;
        text(&response["httpVersion"], 32)?;
        headers(&response["headers"], false)?;
        arr(&response["cookies"], 0)?;
        content(&response["content"], true)?;
        let redirect = text(&response["redirectURL"], 8192)?;
        if !redirect.is_empty() {
            url(&response["redirectURL"])?;
        }
        number(&response["headersSize"], -1, MAX_ARTIFACT as i64)?;
        number(&response["bodySize"], -1, MAX_ARTIFACT as i64)?;
        object(
            &entry["timings"],
            &[
                "blocked", "dns", "connect", "ssl", "send", "wait", "receive",
            ],
        )?;
        for (_, value) in entry["timings"].as_object().unwrap() {
            number(value, -1, 604800000)?;
        }
        for key in ["send", "wait", "receive"] {
            number(&entry["timings"][key], 0, 604800000)?;
        }
    }
    Ok(())
}
fn optional_text(v: &Value, max: usize) -> ApiResult<()> {
    if !v.is_null() {
        text(v, max)?;
    }
    Ok(())
}
fn pattern(v: &Value) -> ApiResult<()> {
    object(v, &["kind", "value"])?;
    let value = text(&v["value"], 256)?;
    if value.is_empty() {
        return Err(bad("Pattern cannot be empty"));
    }
    if !["exact", "wildcard", "regex"].contains(&v["kind"].as_str().unwrap_or_default()) {
        return Err(bad("Invalid pattern type"));
    }
    if v["kind"] == "regex" {
        regex::RegexBuilder::new(value)
            .size_limit(2 * 1024 * 1024)
            .build()
            .map_err(|_| bad("Invalid or oversized regex"))?;
    }
    Ok(())
}
fn identity(v: &Value) -> ApiResult<()> {
    let id = text(&v["id"], 120)?;
    let name = text(&v["name"], 120)?;
    if id.is_empty()
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
        || name.trim().is_empty()
    {
        return Err(bad("Invalid definition identity"));
    }
    for key in ["createdAt", "updatedAt"] {
        let timestamp = text(&v[key], 40)?;
        if timestamp.is_empty() || !timestamp.bytes().all(|b| b.is_ascii_digit()) {
            return Err(bad("Timestamps must be decimal epoch milliseconds"));
        }
    }
    Ok(())
}
fn rule(v: &Value) -> ApiResult<()> {
    object(
        v,
        &[
            "schemaVersion",
            "id",
            "name",
            "enabled",
            "priority",
            "matcher",
            "action",
            "createdAt",
            "updatedAt",
        ],
    )?;
    number(&v["schemaVersion"], 1, 1)?;
    identity(v)?;
    if !v["enabled"].is_boolean() {
        return Err(bad("Rule enabled must be boolean"));
    }
    number(&v["priority"], -1000000, 1000000)?;
    text(&v["createdAt"], 64)?;
    text(&v["updatedAt"], 64)?;
    let matcher = &v["matcher"];
    object(matcher, &["method", "host", "path"])?;
    optional_text(&matcher["method"], 32)?;
    if let Some(method) = matcher["method"].as_str() {
        if method.is_empty() || !method.bytes().all(|b| b.is_ascii_alphabetic()) {
            return Err(bad("Invalid rule method"));
        }
    }
    pattern(&matcher["host"])?;
    pattern(&matcher["path"])?;
    let a = &v["action"];
    let kind = a["type"]
        .as_str()
        .ok_or_else(|| bad("Action type required"))?;
    match kind {
        "allow" | "no_cache" | "block_cookies" => {
            object(a, &["type"])?;
        }
        "block" => {
            object(a, &["type", "statusCode"])?;
            number(&a["statusCode"], 400, 599)?;
        }
        "map_remote" | "reverse_proxy" | "upstream_proxy" => {
            object(a, &["type", "url"])?;
            url(&a["url"])?;
        }
        "rewrite_request" | "rewrite_response" => {
            object(a, &["type", "headers", "body"])?;
            headers(&a["headers"], true)?;
            arr(&a["headers"], 64)?;
            optional_text(&a["body"], 2 * 1024 * 1024)?;
        }
        "breakpoint" => {
            object(a, &["type", "stage"])?;
            if !["request", "response"].contains(&a["stage"].as_str().unwrap_or_default()) {
                return Err(bad("Invalid breakpoint stage"));
            }
        }
        "inspect_https" => {
            object(a, &["type", "enabled"])?;
            if !a["enabled"].is_boolean() {
                return Err(bad("Invalid inspection flag"));
            }
            if matcher["method"] != "TLS"
                || matcher["path"]["kind"] != "wildcard"
                || matcher["path"]["value"] != "*"
            {
                return Err(bad("Inspection requires TLS method and wildcard path"));
            }
        }
        "dns_override" | "socks_proxy" => {
            object(a, &["type", "address"])?;
            let s = text(&a["address"], 256)?;
            if s.contains(['@', '/', '\\']) || s.is_empty() {
                return Err(bad("Invalid address"));
            }
            if kind == "dns_override"
                && (s.parse::<std::net::IpAddr>().is_err()
                    || matcher["method"] != "DNS"
                    || matcher["path"]["kind"] != "wildcard"
                    || matcher["path"]["value"] != "*")
            {
                return Err(bad(
                    "DNS override requires IP address, DNS method and wildcard path",
                ));
            }
            if kind == "socks_proxy" && s.parse::<std::net::SocketAddr>().is_err() {
                return Err(bad("SOCKS address must be IP:port"));
            }
        }
        _ => {
            return Err(bad(
                "Scripts and local-file actions are not portable shared definitions",
            ));
        }
    }
    Ok(())
}
fn fixture(v: &Value) -> ApiResult<()> {
    object(
        v,
        &[
            "schemaVersion",
            "id",
            "name",
            "statusCode",
            "responseHeaders",
            "responseBody",
            "sourceFlowId",
            "createdAt",
            "updatedAt",
        ],
    )?;
    number(&v["schemaVersion"], 1, 1)?;
    identity(v)?;
    number(&v["statusCode"], 100, 599)?;
    headers(&v["responseHeaders"], true)?;
    text(&v["createdAt"], 64)?;
    text(&v["updatedAt"], 64)?;
    if !v["sourceFlowId"].is_null() {
        return Err(bad("Fixture source correlation must be removed"));
    }
    if !v["responseBody"].is_null() {
        let b = &v["responseBody"];
        object(b, &["contentType", "encoding", "data"])?;
        optional_text(&b["contentType"], 256)?;
        let s = text(&b["data"], MAX_ARTIFACT)?;
        match b["encoding"].as_str() {
            Some("text") => {}
            Some("base64") => material(
                &STANDARD
                    .decode(s)
                    .map_err(|_| bad("Invalid base64 fixture"))?,
            )?,
            _ => return Err(bad("Invalid fixture encoding")),
        }
    }
    Ok(())
}
pub fn workspace(rules: &[Value], fixtures: &[Value]) -> ApiResult<()> {
    if rules.len() > 500 || fixtures.len() > 500 {
        return Err(bad("At most 500 rules and 500 fixtures"));
    }
    for (items, check) in [
        (rules, rule as fn(&Value) -> ApiResult<()>),
        (fixtures, fixture as fn(&Value) -> ApiResult<()>),
    ] {
        let mut ids = HashSet::new();
        for item in items {
            check(item)?;
            let id = text(&item["id"], 128)?;
            if id.is_empty() || !ids.insert(id) {
                return Err(bad("Definition IDs must be nonempty and unique"));
            }
        }
    }
    Ok(())
}
