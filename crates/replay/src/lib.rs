use capture_core::{CapturedBody, CapturedFlow, CapturedRequest, CapturedResponse};
use core_model::{FlowSource, FlowSummary, HeaderValue, Timing, SCHEMA_VERSION};
use reqwest::{header::{HeaderName, HeaderValue as ReqwestHeaderValue}, redirect::Policy, Client, Method, Url};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReplayDraft {
    pub source_flow_id: Option<String>,
    pub session_id: Option<String>,
    pub method: String,
    pub url: String,
    pub headers: Vec<HeaderValue>,
    pub body_text: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ReplayEngine { client: Client }

impl ReplayEngine {
    pub fn new() -> Result<Self, ReplayError> {
        let client = Client::builder()
            .redirect(Policy::none())
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|error| ReplayError::new("replay_client_failed", error.to_string(), false))?;
        Ok(Self { client })
    }

    pub async fn execute(&self, draft: ReplayDraft) -> Result<CapturedFlow, ReplayError> {
        let method = Method::from_bytes(draft.method.trim().to_uppercase().as_bytes())
            .map_err(|error| ReplayError::new("replay_method_invalid", error.to_string(), true))?;
        let url = Url::parse(draft.url.trim())
            .map_err(|error| ReplayError::new("replay_url_invalid", error.to_string(), true))?;
        let started_at = epoch_millis_string();
        let started = Instant::now();
        let mut builder = self.client.request(method.clone(), url.clone());
        let mut normalized_request_headers = Vec::new();

        for header in draft.headers {
            if !is_replayable_header(&header.name) { continue; }
            let name = HeaderName::from_bytes(header.name.trim().as_bytes())
                .map_err(|error| ReplayError::new("replay_header_name_invalid", error.to_string(), true))?;
            let value = ReqwestHeaderValue::from_str(&header.value)
                .map_err(|error| ReplayError::new("replay_header_value_invalid", error.to_string(), true))?;
            builder = builder.header(name, value);
            normalized_request_headers.push(header);
        }

        let request_body = if let Some(text) = draft.body_text {
            let bytes = text.into_bytes();
            builder = builder.body(bytes.clone());
            Some(CapturedBody {
                bytes,
                content_type: content_type(&normalized_request_headers),
                encoding: Some("utf-8".into()),
                is_binary: false,
                is_truncated: false,
            })
        } else { None };

        let response = builder.send().await
            .map_err(|error| ReplayError::new("replay_request_failed", error.to_string(), true))?;
        let response_received = Instant::now();
        let status = response.status();
        let response_headers = response.headers().iter().map(|(name, value)| HeaderValue {
            name: name.as_str().to_string(),
            value: value.to_str().unwrap_or("<non-utf8>").to_string(),
            sensitive: is_sensitive_header(name.as_str()),
        }).collect::<Vec<_>>();
        let response_content_type = content_type(&response_headers);
        let response_bytes = response.bytes().await
            .map_err(|error| ReplayError::new("replay_body_read_failed", error.to_string(), true))?.to_vec();
        let finished = Instant::now();

        let total_ms = millis(finished.duration_since(started));
        let server_ms = millis(response_received.duration_since(started));
        let download_ms = millis(finished.duration_since(response_received));
        let response_size = response_bytes.len() as u64;
        let host = url.host_str().unwrap_or_default().to_string();
        let path = if url.path().is_empty() { "/".to_string() } else { url.path().to_string() };
        let id = format!("replay-{}", epoch_millis_u128());

        Ok(CapturedFlow {
            summary: FlowSummary {
                schema_version: SCHEMA_VERSION,
                id,
                session_id: draft.session_id,
                source: FlowSource::Replay,
                method: method.as_str().to_string(),
                host: host.clone(),
                path: path.clone(),
                status_code: Some(status.as_u16()),
                duration_ms: Some(total_ms),
                response_size_bytes: Some(response_size),
                started_at,
            },
            request: CapturedRequest {
                method: method.as_str().to_string(),
                url: url.as_str().to_string(),
                scheme: url.scheme().to_string(),
                host,
                port: url.port_or_known_default(),
                path,
                query: url.query().map(str::to_string),
                headers: normalized_request_headers,
                body: request_body,
            },
            response: Some(CapturedResponse {
                status_code: status.as_u16(),
                reason: status.canonical_reason().map(str::to_string),
                headers: response_headers,
                body: Some(CapturedBody {
                    bytes: response_bytes,
                    content_type: response_content_type,
                    encoding: None,
                    is_binary: false,
                    is_truncated: false,
                }),
            }),
            timing: Timing { server_ms: Some(server_ms), download_ms: Some(download_ms), total_ms: Some(total_ms), ..Timing::default() },
            error_code: None,
            error_message: None,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReplayError { pub code: String, pub message: String, pub recoverable: bool }
impl ReplayError { fn new(code: impl Into<String>, message: impl Into<String>, recoverable: bool) -> Self { Self { code: code.into(), message: message.into(), recoverable } } }

fn is_replayable_header(name: &str) -> bool { !matches!(name.to_ascii_lowercase().as_str(), "host" | "content-length" | "transfer-encoding" | "connection") }
fn is_sensitive_header(name: &str) -> bool { matches!(name.to_ascii_lowercase().as_str(), "authorization" | "proxy-authorization" | "cookie" | "set-cookie" | "x-api-key" | "api-key" | "x-auth-token") }
fn content_type(headers: &[HeaderValue]) -> Option<String> { headers.iter().find(|header| header.name.eq_ignore_ascii_case("content-type")).map(|header| header.value.clone()) }
fn millis(duration: Duration) -> u64 { duration.as_millis().min(u128::from(u64::MAX)) as u64 }
fn epoch_millis_u128() -> u128 { SystemTime::now().duration_since(UNIX_EPOCH).map(|duration| duration.as_millis()).unwrap_or_default() }
fn epoch_millis_string() -> String { epoch_millis_u128().to_string() }
