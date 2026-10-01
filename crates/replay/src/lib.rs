use core_model::HeaderValue;
use reqwest::{header::{HeaderName, HeaderValue as HttpHeaderValue}, redirect::Policy, Client, Method, Url};
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
            .redirect(Policy::custom(|attempt| {
                let Some(previous) = attempt.previous().last() else {
                    return attempt.stop();
                };
                let same_origin = previous.scheme() == attempt.url().scheme()
                    && previous.host_str() == attempt.url().host_str()
                    && previous.port_or_known_default() == attempt.url().port_or_known_default();
                if same_origin && attempt.previous().len() < 10 {
                    attempt.follow()
                } else {
                    attempt.stop()
                }
            }))
            .build()
            .map_err(ReplayError::request_build)?;
        Ok(Self { client })
    }

    pub async fn execute(&self, request: ReplayRequest) -> Result<ReplayExecution, ReplayError> {
        let method = Method::from_bytes(request.method.as_bytes())
            .map_err(|error| ReplayError::invalid_request(format!("invalid method: {error}")))?;
        let parsed = Url::parse(&request.url)
            .map_err(|error| ReplayError::invalid_request(format!("invalid URL: {error}")))?;
        if !matches!(parsed.scheme(), "http" | "https") || !parsed.username().is_empty() || parsed.password().is_some() || parsed.fragment().is_some()
            || request.url.len() > 8192 || request.headers.len() > 100 || request.body.as_ref().is_some_and(|body| body.len() > 2 * 1024 * 1024) {
            return Err(ReplayError::invalid_request("Use a bounded HTTP(S) URL without credentials or a fragment, at most 100 headers and a body of 2 MiB or smaller"));
        }
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

        if !request.headers.iter().any(|header| header.name.eq_ignore_ascii_case("content-type")) {
            if let Some(content_type) = &request.content_type {
                builder = builder.header(reqwest::header::CONTENT_TYPE, HttpHeaderValue::from_str(content_type).map_err(|_| ReplayError::invalid_request("Invalid body content type"))?);
            }
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
        "host"
            | "content-length"
            | "transfer-encoding"
            | "connection"
            | "proxy-connection"
            | "x-mobile-api-studio-request-id"
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

#[cfg(test)]
mod security_tests {
    use super::*;
    use std::{io::{Read, Write}, net::TcpListener};

    #[test]
    fn replay_stops_before_cross_origin_redirect_with_a_secret_header() {
        let first = TcpListener::bind("127.0.0.1:0").unwrap();
        let second = TcpListener::bind("127.0.0.1:0").unwrap();
        let first_address = first.local_addr().unwrap();
        let second_address = second.local_addr().unwrap();
        second.set_nonblocking(true).unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = first.accept().unwrap();
            stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let mut request = [0_u8; 4096];
            let count = stream.read(&mut request).unwrap();
            assert!(String::from_utf8_lossy(&request[..count]).to_ascii_lowercase().contains("x-api-key: test-secret"));
            write!(stream, "HTTP/1.1 302 Found\r\nLocation: http://{second_address}/other\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
        });
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let execution = runtime.block_on(async {
            ReplayEngine::new().unwrap().execute(ReplayRequest {
                source_flow_id: "test".into(),
                method: "GET".into(),
                url: format!("http://{first_address}/start"),
                headers: vec![HeaderValue { name: "X-API-Key".into(), value: "test-secret".into(), sensitive: true }],
                body: None,
                content_type: None,
                body_is_binary: false,
            }).await.unwrap()
        });
        server.join().unwrap();
        assert_eq!(execution.status_code, 302);
        assert!(second.accept().is_err(), "redirect target must not receive the credential");
    }

    #[test]
    fn replay_follows_a_same_origin_redirect() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            for (path, response) in [
                ("/start", "HTTP/1.1 302 Found\r\nLocation: /final\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"),
                ("/final", "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok"),
            ] {
                let (mut stream, _) = listener.accept().unwrap();
                stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                let mut request = [0_u8; 4096];
                let count = stream.read(&mut request).unwrap();
                assert!(String::from_utf8_lossy(&request[..count]).contains(path));
                stream.write_all(response.as_bytes()).unwrap();
            }
        });
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let execution = runtime.block_on(async {
            ReplayEngine::new().unwrap().execute(ReplayRequest {
                source_flow_id: "test".into(),
                method: "GET".into(),
                url: format!("http://{address}/start"),
                headers: Vec::new(),
                body: None,
                content_type: None,
                body_is_binary: false,
            }).await.unwrap()
        });
        server.join().unwrap();
        assert_eq!(execution.status_code, 200);
        assert_eq!(execution.response_body, b"ok");
    }
}
