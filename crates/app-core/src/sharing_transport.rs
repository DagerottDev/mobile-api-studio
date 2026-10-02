use core_model::AppError;
use serde_json::Value;
use std::time::Duration;
use url::Url;

fn invalid(message: &str) -> AppError {
    AppError::new("sharing_request_invalid", message, true)
}

fn validate(origin: &str, token: &str, path: &str, method: &str) -> Result<Url, AppError> {
    let url = Url::parse(origin).map_err(|_| invalid("Use a sharing service origin."))?;
    let local = matches!(url.host_str(), Some("127.0.0.1" | "[::1]"));
    if origin.len() > 2048
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
        || !(url.scheme() == "https" || url.scheme() == "http" && local)
    {
        return Err(invalid(
            "Use an HTTPS origin, or HTTP on literal loopback for development.",
        ));
    }
    if token.len() != 64 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(invalid("Use the 64-character issued access token."));
    }
    let id = |prefix: &str, suffix: &str| {
        path.strip_prefix(prefix)
            .and_then(|value| value.strip_suffix(suffix))
            .is_some_and(|value| {
                !value.is_empty()
                    && value.len() <= 120
                    && value
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            })
    };
    let allowed = match method {
        "GET" => matches!(
            path,
            "/v1/me" | "/v1/shares" | "/v1/workspace" | "/v1/members"
        ),
        "POST" => matches!(path, "/v1/shares" | "/v1/members") || id("/v1/members/", "/token"),
        "PUT" => path == "/v1/workspace",
        "PATCH" => id("/v1/members/", ""),
        "DELETE" => id("/v1/members/", "") || id("/v1/shares/", ""),
        _ => false,
    };
    if !allowed {
        return Err(invalid("This sharing operation is not available."));
    }
    Ok(url)
}

pub async fn sharing_request(
    origin: String,
    access_token: String,
    path: String,
    method: String,
    body: Value,
) -> Result<Value, AppError> {
    let origin = validate(&origin, &access_token, &path, &method)?;
    let bytes = serde_json::to_vec(&body).map_err(|_| invalid("Invalid sharing payload."))?;
    if bytes.len() > 6 * 1024 * 1024 {
        return Err(invalid("Sharing payload exceeds 6 MiB."));
    }
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|_| {
            AppError::new(
                "sharing_unavailable",
                "Unable to initialize the sharing client.",
                true,
            )
        })?;
    let mut request = client
        .request(
            method
                .parse::<reqwest::Method>()
                .map_err(|_| invalid("Invalid method."))?,
            origin.join(&path).map_err(|_| invalid("Invalid path."))?,
        )
        .bearer_auth(access_token)
        .header("Cache-Control", "no-store");
    if !body.is_null() {
        request = request
            .header("Content-Type", "application/json")
            .body(bytes);
    }
    let mut response = request.send().await.map_err(|_| {
        AppError::new(
            "sharing_unavailable",
            "Sharing service request failed. Check its address and TLS configuration.",
            true,
        )
    })?;
    if !response.status().is_success() {
        return Err(AppError::new(
            "sharing_service_error",
            match response.status().as_u16() {
                409 => "The team workspace changed. Reload before publishing.",
                401 | 403 => "Sharing access denied. Check the token and role.",
                _ => "The sharing service rejected the operation.",
            },
            true,
        ));
    }
    if response.status() == reqwest::StatusCode::NO_CONTENT {
        return Ok(Value::Null);
    }
    if response
        .content_length()
        .is_some_and(|size| size > 5 * 1024 * 1024)
    {
        return Err(invalid("Sharing response exceeds 5 MiB."));
    }
    let mut output = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| AppError::new("sharing_unavailable", "Sharing response interrupted.", true))?
    {
        if output.len().saturating_add(chunk.len()) > 5 * 1024 * 1024 {
            return Err(invalid("Sharing response exceeds 5 MiB."));
        }
        output.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&output)
        .map_err(|_| invalid("The sharing service returned invalid JSON."))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn destination_and_operations_are_bounded() {
        let token = "a".repeat(64);
        assert!(validate("http://127.0.0.1:8190", &token, "/v1/me", "GET").is_ok());
        assert!(
            validate(
                "https://sharing.example.test",
                &token,
                "/v1/members/member-1/token",
                "POST"
            )
            .is_ok()
        );
        for origin in [
            "http://192.168.1.2",
            "http://localhost",
            "https://user:secret@example.test",
            "https://example.test/path",
            "https://example.test?token=secret",
        ] {
            assert!(validate(origin, &token, "/v1/me", "GET").is_err());
        }
        for path in [
            "//example.test/v1/me",
            "/v1/members/../token",
            "/api/session",
            "/v1/me?other=true",
        ] {
            assert!(validate("https://example.test", &token, path, "POST").is_err());
        }
        assert!(validate("https://example.test", "bad", "/v1/me", "GET").is_err());
    }
}
