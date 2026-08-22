use core_model::HeaderValue;
use reqwest::{header::{HeaderName, HeaderValue as HttpHeaderValue}, Client, Method, Url};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const MAX_REPLAY_RESPONSE_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReplayHeaderDraft {
    pub name: String,
    pub value: Option<String>,
    pub sensitive: bool,
    pub use_original: bool,
    pub enabled: bool,
    pub source_index: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReplayBodyDraft {
    pub text: Option<String>,
    pub base64: Option<String>,
    pub is_binary: bool,
    pub content_type: Option<String>,
    pub use_original: bool,
    pub source_truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReplayDraft {
    pub source_flow_id: String,
    pub method: String,
    pub url: String,
    pub headers: Vec<ReplayHeaderDraft>,
    pub body: Option<ReplayBodyDraft>,
}

#[derive(Debug, Clone)]
pub struct ReplayRequest {
    pub source_flow_id: String,
    pub method: String,
    pub url: String,
    pub headers: Vec<HeaderValue>,
    pub body: Option<Vec<u8>>,
    pub content_type: Option<String>,
    pub body_is_binary: bool,
}

#[derive(Debug, Clone)]
pub struct ReplayExecution {
    pub request: ReplayRequest,
    pub scheme: String,
    pub host: String,
    pub port: Option<u16>,
    pub path: String,
    pub query: Option<String>,
    pub status_code: u16,
    pub reason: Option<String>,
    pub response_headers: Vec<HeaderValue>,
    pub response_body: Vec<u8>,
    pub response_body_truncated: bool,
    pub response_content_type: Option<String>,
    pub started_at: String,
    pub total_ms: u64,
}

#[derive(Debug, Clone)]
pub struct ReplayEngine {
    client: Client,
}

impl ReplayEngine {
    pub fn new() -> Result<Self, ReplayError> {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(20))
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(ReplayError::request_build)?;
        Ok(Self { client })
    }

    pub async fn execute(&self, request: ReplayRequest) -> Result<ReplayExecution, ReplayError> {
        let method = Method::from_bytes(request.method.as_bytes())
            .map_err(|error| ReplayError::invalid_request(format!("invalid method: {error}")))?;
        let parsed = Url::parse(&request.url)
            .map_err(|error| ReplayError::invalid_request(format!("invalid URL: {error}")))?;
        let scheme = parsed.scheme().to_string();
        let host = parsed
            .host_str()
            .ok_or_else(|| ReplayError::invalid_request("URL must contain a host"))?
            .to_string();
        let port = parsed.port();
        let path = parsed.path().to_string();
        let query = parsed.query().map(str::to_string);

        let started_at = epoch_millis();
        let start = Instant::now();
        let mut builder = self.client.request(method, parsed);

        for header in &request.headers {
            if should_skip_header(&header.name) {
                continue;
            }
            let name = HeaderName::from_bytes(header.name.as_bytes()).map_err(|error| {
                ReplayError::invalid_request(format!("invalid header name '{}': {error}", header.name))
            })?;
            let value = HttpHeaderValue::from_str(&header.value).map_err(|error| {
                ReplayError::invalid_request(format!("invalid value for header '{}': {error}", header.name))
            })?;
            builder = builder.header(name, value);
        }

        if let Some(body) = &request.body {
            builder = builder.body(body.clone());
        }

        let mut response = builder.send().await.map_err(ReplayError::send)?;
        let status = response.status();
        let status_code = status.as_u16();
        let reason = status.canonical_reason().map(str::to_string);
        let response_content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        let response_headers = response
            .headers()
            .iter()
            .map(|(name, value)| HeaderValue {
                name: name.as_str().to_string(),
                value: value.to_str().unwrap_or("<non-utf8>").to_string(),
                sensitive: is_sensitive_header(name.as_str()),
            })
            .collect();

        let mut response_body = Vec::new();
        let mut response_body_truncated = false;
        while let Some(chunk) = response.chunk().await.map_err(ReplayError::send)? {
            let remaining = MAX_REPLAY_RESPONSE_BYTES.saturating_sub(response_body.len());
            if remaining == 0 {
                response_body_truncated = true;
                break;
            }
            if chunk.len() > remaining {
                response_body.extend_from_slice(&chunk[..remaining]);
                response_body_truncated = true;
                break;
            }
            response_body.extend_from_slice(&chunk);
        }

        Ok(ReplayExecution {
            request,
            scheme,
            host,
            port,
            path,
            query,
            status_code,
            reason,
            response_headers,
            response_body,
            response_body_truncated,
            response_content_type,
            started_at,
            total_ms: start.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReplayError {
    pub code: String,
    pub message: String,
    pub recoverable: bool,
}

impl ReplayError {
    fn invalid_request(message: impl Into<String>) -> Self {
        Self { code: "replay_invalid_request".into(), message: message.into(), recoverable: true }
    }

    fn request_build(error: reqwest::Error) -> Self {
        Self { code: "replay_client_failed".into(), message: error.to_string(), recoverable: true }
    }

    fn send(error: reqwest::Error) -> Self {
        Self { code: "replay_send_failed".into(), message: error.to_string(), recoverable: true }
    }
}

fn should_skip_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "host" | "content-length" | "transfer-encoding" | "connection" | "proxy-connection"
    )
}

pub fn is_sensitive_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "authorization"
            | "proxy-authorization"
            | "cookie"
            | "set-cookie"
            | "x-api-key"
            | "api-key"
            | "x-auth-token"
    )
}

fn epoch_millis() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().to_string())
        .unwrap_or_else(|_| "0".into())
}
