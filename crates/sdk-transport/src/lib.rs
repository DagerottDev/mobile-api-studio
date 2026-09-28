use sdk_protocol::{SDK_EVENT_PATH, SDK_HEALTH_PATH, SdkEnvelope};
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::Arc,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::broadcast,
    task::JoinSet,
};

const MAX_HEADER_BYTES: usize = 64 * 1024;
const MAX_BODY_BYTES: usize = 1024 * 1024;

#[derive(Clone)]
pub struct SdkIngestionServer {
    address: SocketAddr,
    sender: broadcast::Sender<SdkEnvelope>,
    pairing: Option<Pairing>,
}

#[derive(Clone)]
struct Pairing {
    peer: Ipv4Addr,
    token: String,
}

impl SdkIngestionServer {
    pub fn localhost(port: u16) -> Self {
        let (sender, _) = broadcast::channel(4_096);
        Self {
            address: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port),
            sender,
            pairing: None,
        }
    }

    pub fn paired_lan(address: Ipv4Addr, port: u16, peer: Ipv4Addr, token: String) -> Self {
        let (sender, _) = broadcast::channel(4_096);
        Self {
            address: SocketAddr::new(IpAddr::V4(address), port),
            sender,
            pairing: Some(Pairing { peer, token }),
        }
    }

    pub fn address(&self) -> SocketAddr {
        self.address
    }

    pub fn subscribe(&self) -> broadcast::Receiver<SdkEnvelope> {
        self.sender.subscribe()
    }

    pub async fn bind(&self) -> Result<TcpListener, SdkTransportError> {
        TcpListener::bind(self.address)
            .await
            .map_err(SdkTransportError::bind)
    }

    pub async fn run(self: Arc<Self>) -> Result<(), SdkTransportError> {
        let listener = self.bind().await?;
        self.run_bound(listener).await
    }

    pub async fn run_bound(
        self: Arc<Self>,
        listener: TcpListener,
    ) -> Result<(), SdkTransportError> {
        let mut connections = JoinSet::new();
        let expected_host = listener
            .local_addr()
            .map_err(SdkTransportError::bind)?
            .to_string();
        loop {
            while connections.try_join_next().is_some() {}
            let (stream, peer) = listener.accept().await.map_err(SdkTransportError::accept)?;
            if connections.len() >= 128
                || self
                    .pairing
                    .as_ref()
                    .is_some_and(|pairing| peer.ip() != IpAddr::V4(pairing.peer))
            {
                continue;
            }
            let sender = self.sender.clone();
            let pairing = self.pairing.clone();
            let expected_host = expected_host.clone();
            connections.spawn(async move {
                let _ = handle_connection(stream, sender, pairing, expected_host).await;
            });
        }
    }
}

async fn handle_connection(
    mut stream: TcpStream,
    sender: broadcast::Sender<SdkEnvelope>,
    pairing: Option<Pairing>,
    expected_host: String,
) -> Result<(), SdkTransportError> {
    let request = read_request(&mut stream).await?;

    let host_allowed = if pairing.is_some() {
        request.host.as_deref() == Some(expected_host.as_str())
    } else {
        matches!(
            request.host.as_deref(),
            Some("127.0.0.1:8182" | "localhost:8182" | "10.0.2.2:8182")
        )
    };
    if !host_allowed || request.has_origin {
        return write_response(
            &mut stream,
            403,
            "Forbidden",
            br#"{"error":"invalid_origin"}"#,
        )
        .await;
    }

    if let Some(pairing) = &pairing {
        if !request
            .pairing_token
            .as_deref()
            .is_some_and(|token| constant_time_eq(token, &pairing.token))
        {
            return write_response(
                &mut stream,
                401,
                "Unauthorized",
                br#"{"error":"invalid_pairing_token"}"#,
            )
            .await;
        }
    }

    if request.method == "GET" && request.path == SDK_HEALTH_PATH {
        return write_response(&mut stream, 200, "OK", br#"{"status":"ok"}"#).await;
    }

    if request.method != "POST" || request.path != SDK_EVENT_PATH {
        return write_response(&mut stream, 404, "Not Found", br#"{"error":"not_found"}"#).await;
    }

    let envelope: SdkEnvelope = match serde_json::from_slice(&request.body) {
        Ok(value) => value,
        Err(error) => {
            let body = format!(
                "{{\"error\":\"invalid_json\",\"message\":{}}}",
                json_string(&error.to_string())
            );
            return write_response(&mut stream, 400, "Bad Request", body.as_bytes()).await;
        }
    };

    if let Err(error) = envelope.validate() {
        let body = format!(
            "{{\"error\":{},\"message\":{}}}",
            json_string(&error.code),
            json_string(&error.message)
        );
        return write_response(&mut stream, 422, "Unprocessable Entity", body.as_bytes()).await;
    }

    sender.send(envelope).map_err(|_| {
        SdkTransportError::new(
            "sdk_ingestion_unavailable",
            "No SDK event consumer is available",
        )
    })?;

    write_response(&mut stream, 202, "Accepted", br#"{"accepted":true}"#).await
}

struct HttpRequest {
    method: String,
    path: String,
    body: Vec<u8>,
    host: Option<String>,
    pairing_token: Option<String>,
    has_origin: bool,
}

async fn read_request(stream: &mut TcpStream) -> Result<HttpRequest, SdkTransportError> {
    let mut buffer = Vec::with_capacity(8 * 1024);
    let header_end = loop {
        if buffer.len() > MAX_HEADER_BYTES {
            return Err(SdkTransportError::new(
                "sdk_http_headers_too_large",
                "SDK ingestion request headers exceeded the size limit",
            ));
        }
        if let Some(index) = find_header_end(&buffer) {
            break index;
        }
        let mut chunk = [0_u8; 4096];
        let read = stream
            .read(&mut chunk)
            .await
            .map_err(SdkTransportError::read)?;
        if read == 0 {
            return Err(SdkTransportError::new(
                "sdk_http_incomplete",
                "SDK ingestion connection closed before headers were complete",
            ));
        }
        buffer.extend_from_slice(&chunk[..read]);
    };

    let header_bytes = &buffer[..header_end];
    let header_text = std::str::from_utf8(header_bytes)
        .map_err(|error| SdkTransportError::new("sdk_http_headers_invalid", error.to_string()))?;
    let mut lines = header_text.split("\r\n");
    let request_line = lines.next().ok_or_else(|| {
        SdkTransportError::new(
            "sdk_http_request_line_missing",
            "HTTP request line is missing",
        )
    })?;
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    let raw_path = request_parts.next().unwrap_or_default();
    if method.is_empty() || raw_path.is_empty() {
        return Err(SdkTransportError::new(
            "sdk_http_request_line_invalid",
            "HTTP request line is invalid",
        ));
    }
    let path = raw_path.split('?').next().unwrap_or(raw_path).to_string();

    let mut content_length = 0usize;
    let mut host = None;
    let mut duplicate_host = false;
    let mut pairing_token = None;
    let mut duplicate_pairing_token = false;
    let mut has_origin = false;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.trim().eq_ignore_ascii_case("host") {
            if host.is_some() {
                duplicate_host = true;
            } else {
                host = Some(value.trim().to_ascii_lowercase());
            }
        }
        if name.trim().eq_ignore_ascii_case("origin") {
            has_origin = true;
        }
        if name.trim().eq_ignore_ascii_case("x-mas-pairing-token") {
            if pairing_token.is_some() {
                duplicate_pairing_token = true;
            } else {
                pairing_token = Some(value.trim().to_owned());
            }
        }
        if name.trim().eq_ignore_ascii_case("content-length") {
            content_length = value.trim().parse::<usize>().map_err(|error| {
                SdkTransportError::new("sdk_http_content_length_invalid", error.to_string())
            })?;
        }
    }
    if content_length > MAX_BODY_BYTES {
        return Err(SdkTransportError::new(
            "sdk_http_body_too_large",
            format!("SDK event exceeded the {MAX_BODY_BYTES}-byte ingestion limit"),
        ));
    }

    let body_start = header_end + 4;
    while buffer.len().saturating_sub(body_start) < content_length {
        let remaining = content_length - buffer.len().saturating_sub(body_start);
        let mut chunk = vec![0_u8; remaining.min(16 * 1024)];
        let read = stream
            .read(&mut chunk)
            .await
            .map_err(SdkTransportError::read)?;
        if read == 0 {
            return Err(SdkTransportError::new(
                "sdk_http_body_incomplete",
                "SDK ingestion connection closed before the declared body was received",
            ));
        }
        buffer.extend_from_slice(&chunk[..read]);
    }

    Ok(HttpRequest {
        method,
        path,
        body: buffer[body_start..body_start + content_length].to_vec(),
        host: (!duplicate_host).then_some(host).flatten(),
        pairing_token: (!duplicate_pairing_token)
            .then_some(pairing_token)
            .flatten(),
        has_origin,
    })
}

fn constant_time_eq(left: &str, right: &str) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.bytes()
        .zip(right.bytes())
        .fold(0_u8, |difference, (a, b)| difference | (a ^ b))
        == 0
}

fn find_header_end(bytes: &[u8]) -> Option<usize> {
    bytes.windows(4).position(|window| window == b"\r\n\r\n")
}

async fn write_response(
    stream: &mut TcpStream,
    status: u16,
    reason: &str,
    body: &[u8],
) -> Result<(), SdkTransportError> {
    let headers = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n",
        body.len()
    );
    stream
        .write_all(headers.as_bytes())
        .await
        .map_err(SdkTransportError::write)?;
    stream
        .write_all(body)
        .await
        .map_err(SdkTransportError::write)?;
    stream.shutdown().await.map_err(SdkTransportError::write)
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"serialization_failed\"".into())
}

#[derive(Debug, Clone)]
pub struct SdkTransportError {
    pub code: String,
    pub message: String,
}

impl SdkTransportError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    fn bind(error: std::io::Error) -> Self {
        Self::new("sdk_ingestion_bind_failed", error.to_string())
    }

    fn accept(error: std::io::Error) -> Self {
        Self::new("sdk_ingestion_accept_failed", error.to_string())
    }

    fn read(error: std::io::Error) -> Self {
        Self::new("sdk_ingestion_read_failed", error.to_string())
    }

    fn write(error: std::io::Error) -> Self {
        Self::new("sdk_ingestion_write_failed", error.to_string())
    }
}

#[cfg(test)]
mod paired_lan_tests {
    use super::*;

    #[test]
    fn pairing_token_gates_lan_ingestion() {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
            let server = Arc::new(SdkIngestionServer::paired_lan(Ipv4Addr::LOCALHOST, 0, Ipv4Addr::LOCALHOST, "secret".into()));
            let mut events = server.subscribe();
            let listener = server.bind().await.unwrap();
            let address = listener.local_addr().unwrap();
            let task = tokio::spawn(server.run_bound(listener));
            let body = br#"{"schemaVersion":1,"eventId":"one","occurredAt":"1","event":{"type":"context","payload":{"clientId":"client","context":{"attributes":{}}}}}"#;
            async fn send(address: SocketAddr, token: &str, body: &[u8]) -> String {
                let mut stream = TcpStream::connect(address).await.unwrap();
                let request = format!("POST /v1/events HTTP/1.1\r\nHost: {address}\r\nX-MAS-Pairing-Token: {token}\r\nContent-Length: {}\r\n\r\n", body.len());
                stream.write_all(request.as_bytes()).await.unwrap();
                stream.write_all(body).await.unwrap();
                let mut response = Vec::new();
                stream.read_to_end(&mut response).await.unwrap();
                String::from_utf8(response).unwrap()
            }
            assert!(send(address, "wrong", body).await.starts_with("HTTP/1.1 401"));
            assert!(send(address, "secret", body).await.starts_with("HTTP/1.1 202"));
            assert_eq!(tokio::time::timeout(std::time::Duration::from_secs(2), events.recv()).await.unwrap().unwrap().event_id, "one");
            task.abort();
        });
    }
}

impl std::fmt::Display for SdkTransportError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for SdkTransportError {}
