use crate::{atomic_file, AppState, State};
use core_model::{AppError, proxy_rules::{ProxyRule, ProxyRuleAction, PROXY_RULE_SCHEMA_VERSION}};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::{Deserialize, Serialize};
use std::{fs, net::IpAddr, path::{Component, Path}};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use url::Url;

pub(crate) const MAX_RULE_STORAGE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RulePreviewInput {
    pub method: String,
    pub host: String,
    pub path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RulePreview {
    pub matched: bool,
    pub rule_id: String,
}

pub fn preview_proxy_rule(rule: ProxyRule, input: RulePreviewInput) -> Result<RulePreview, AppError> {
    validate_matcher(&rule)?;
    if input.method.len() > 32 || input.host.len() > 255 || input.path.len() > 4096 {
        return Err(AppError::new("proxy_rule_preview_too_large", "Preview request fields exceed the supported size.", true));
    }
    let matched = rule.matcher.matches(&input.method, &input.host, &input.path)
        .map_err(|error| AppError::new("proxy_rule_pattern_invalid", error.to_string(), true))?;
    Ok(RulePreview { matched, rule_id: rule.id })
}

pub fn list_proxy_rules(state: State<'_, AppState>) -> Result<Vec<ProxyRule>, AppError> {
    state.database.list_proxy_rules().map_err(|error| AppError::storage(error.to_string()))
}

pub fn list_proxy_rule_diagnostics(state: State<'_, AppState>) -> Result<Vec<super::ProxyRuleDiagnostic>, AppError> {
    state.proxy_rule_diagnostics.lock().map(|queue| queue.iter().cloned().collect())
        .map_err(|_| AppError::new("proxy_rule_diagnostics_unavailable", "Rule diagnostics are unavailable.", true))
}

#[derive(Serialize)]
pub struct ImportedProxyMap { filename: String }

pub fn import_proxy_map(name: String, data_base64: String, state: State<'_, AppState>) -> Result<ImportedProxyMap, AppError> {
    if data_base64.len() > 3 * 1024 * 1024 {
        return Err(AppError::new("proxy_map_too_large", "Map Local file exceeds 2 MiB.", true));
    }
    let bytes = BASE64.decode(data_base64).map_err(|_| AppError::new("proxy_map_invalid_base64", "Map Local file data is invalid.", true))?;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err(AppError::new("proxy_map_too_large", "Map Local file exceeds 2 MiB.", true));
    }
    let clean_name = name.chars().filter(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_')).take(80).collect::<String>();
    if clean_name.is_empty() || clean_name == "." || clean_name == ".." {
        return Err(AppError::new("proxy_map_name_invalid", "Map Local file needs a valid filename.", true));
    }
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| AppError::new("proxy_map_clock_failed", error.to_string(), true))?.as_nanos();
    let filename = format!("map-{nonce}-{clean_name}");
    let directory = state.capture_engine.conf_dir().join("proxy-maps");
    fs::create_dir_all(&directory).map_err(|error| AppError::new("proxy_map_directory_failed", error.to_string(), true))?;
    if fs::symlink_metadata(&directory).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(AppError::new("proxy_map_directory_invalid", "The proxy-maps folder cannot be a symbolic link.", true));
    }
    #[cfg(unix)]
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).map_err(|error| AppError::new("proxy_map_permissions_failed", error.to_string(), true))?;
    atomic_file::write(&directory.join(&filename), &bytes).map_err(|error| AppError::new("proxy_map_write_failed", format!("{error:?}"), true))?;
    Ok(ImportedProxyMap { filename })
}

pub fn upsert_proxy_rule(rule: ProxyRule, state: State<'_, AppState>) -> Result<ProxyRule, AppError> {
    validate_proxy_rule(&rule, &state, false)?;
    let existing = state.database.list_proxy_rules().map_err(|error| AppError::storage(error.to_string()))?;
    if existing.len() >= 1000 && !existing.iter().any(|current| current.id == rule.id) {
        return Err(AppError::new("proxy_rule_limit", "The workspace supports at most 1,000 proxy rules.", true));
    }
    let mut size = serde_json::to_vec(&rule).map_err(|error| AppError::storage(error.to_string()))?.len();
    for current in existing.iter().filter(|current| current.id != rule.id) {
        size = size.saturating_add(serde_json::to_vec(current).map_err(|error| AppError::storage(error.to_string()))?.len());
    }
    if size > MAX_RULE_STORAGE_BYTES {
        return Err(AppError::new("proxy_rule_storage_limit", "Proxy rule definitions exceed the workspace limit of 16 MiB.", true));
    }
    state.database.upsert_proxy_rule(&rule).map_err(|error| AppError::storage(error.to_string()))?;
    Ok(rule)
}

pub(crate) fn validate_proxy_rule(rule: &ProxyRule, state: &State<'_, AppState>, allow_missing_local_map: bool) -> Result<(), AppError> {
    validate_matcher(&rule)?;
    if rule.id.trim().is_empty() || rule.id.len() > 120 || rule.name.trim().is_empty() || rule.name.len() > 120 {
        return Err(AppError::new("proxy_rule_identity_invalid", "Rule ID and name must be 1–120 bytes.", true));
    }
    if !rule.id.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        || [&rule.created_at, &rule.updated_at].into_iter().any(|value| value.is_empty() || value.len() > 40 || !value.bytes().all(|byte| byte.is_ascii_digit())) {
        return Err(AppError::new("proxy_rule_metadata_invalid", "Rule ID or timestamp is invalid.", true));
    }
    match &rule.action {
        ProxyRuleAction::ScriptHook { stage, script } => {
            if !matches!(stage.as_str(), "request" | "response" | "websocket") || script.is_empty() || script.len() > 64 * 1024 {
                return Err(AppError::new("script_hook_invalid", "Choose request, response or websocket and a script of 1–65536 bytes.", true));
            }
        },
        ProxyRuleAction::Allow => {},
        ProxyRuleAction::Block { status_code } if (400..=599).contains(status_code) => {},
        ProxyRuleAction::Block { .. } => return Err(AppError::new("proxy_rule_status_invalid", "Block status must be 400–599.", true)),
        ProxyRuleAction::MapLocal { path } if allow_missing_local_map && !rule.enabled => validate_local_map_name(path)?,
        ProxyRuleAction::MapLocal { path } => validate_local_map(path, state)?,
        ProxyRuleAction::MapRemote { url } => validate_remote_url(url)?,
        ProxyRuleAction::RewriteRequest { headers, body } | ProxyRuleAction::RewriteResponse { headers, body } => validate_rewrite(headers, body)?,
        ProxyRuleAction::Breakpoint { .. } | ProxyRuleAction::NoCache | ProxyRuleAction::BlockCookies => {},
        ProxyRuleAction::InspectHttps { .. } => {
            if rule.matcher.method.as_deref() != Some("TLS")
                || !matches!(rule.matcher.path.kind, core_model::proxy_rules::RulePatternKind::Wildcard) || rule.matcher.path.value != "*" {
                return Err(AppError::new("proxy_rule_tls_invalid", "HTTPS inspection matches the connection host before HTTP: use TLS method and wildcard path.", true));
            }
        }
        ProxyRuleAction::DnsOverride { address } => {
            if address.parse::<IpAddr>().is_err() || rule.matcher.method.as_deref() != Some("DNS")
                || !matches!(rule.matcher.path.kind, core_model::proxy_rules::RulePatternKind::Wildcard) || rule.matcher.path.value != "*" {
                return Err(AppError::new("proxy_rule_dns_invalid", "DNS override needs an IP address, DNS method, and wildcard path.", true));
            }
        }
        _ => return Err(AppError::new("proxy_rule_action_not_ready", "This rule action is not available yet.", true)),
    }
    Ok(())
}

fn validate_local_map(path: &str, state: &State<'_, AppState>) -> Result<(), AppError> {
    validate_local_map_name(path)?;
    let relative = Path::new(path);
    let root = state.capture_engine.conf_dir().join("proxy-maps");
    let file = root.join(relative);
    let metadata = fs::symlink_metadata(&file).map_err(|error| AppError::new("proxy_rule_map_file_missing", error.to_string(), true))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 2 * 1024 * 1024 {
        return Err(AppError::new("proxy_rule_map_file_invalid", "Map Local requires a regular file of at most 2 MiB.", true));
    }
    Ok(())
}

fn validate_local_map_name(path: &str) -> Result<(), AppError> {
    let relative = Path::new(path);
    if path.is_empty() || path.len() > 255 || !matches!(relative.components().collect::<Vec<_>>().as_slice(), [Component::Normal(_)]) {
        return Err(AppError::new("proxy_rule_map_path_invalid", "Map Local requires one filename in the proxy-maps folder.", true));
    }
    Ok(())
}

fn validate_remote_url(value: &str) -> Result<(), AppError> {
    let url = Url::parse(value).map_err(|error| AppError::new("proxy_rule_map_url_invalid", error.to_string(), true))?;
    if value.len() > 2048 || value.chars().any(char::is_control) || !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() || !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
        return Err(AppError::new("proxy_rule_map_url_invalid", "Map Remote requires a credential-free HTTP(S) URL without a fragment.", true));
    }
    Ok(())
}

fn validate_rewrite(headers: &[core_model::proxy_rules::RuleHeaderMutation], body: &Option<String>) -> Result<(), AppError> {
    if headers.len() > 64 || body.as_ref().is_some_and(|value| value.len() > 2 * 1024 * 1024) {
        return Err(AppError::new("proxy_rule_rewrite_too_large", "Rewrite exceeds 64 headers or 2 MiB of body text.", true));
    }
    for header in headers {
        if header.name.is_empty() || header.name.len() > 256 || !header.name.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte)) || header.value.as_ref().is_some_and(|value| value.len() > 8192 || value.contains(['\r', '\n'])) || (header.remove && header.value.is_some()) {
            return Err(AppError::new("proxy_rule_rewrite_header_invalid", "Rewrite has an invalid header mutation.", true));
        }
    }
    Ok(())
}

pub fn delete_proxy_rule(id: String, state: State<'_, AppState>) -> Result<(), AppError> {
    state.database.delete_proxy_rule(&id).map_err(|error| AppError::storage(error.to_string()))
}

pub fn disable_all_proxy_rules(state: State<'_, AppState>) -> Result<usize, AppError> {
    state.database.disable_all_proxy_rules().map_err(|error| AppError::storage(error.to_string()))
}

fn validate_matcher(rule: &ProxyRule) -> Result<(), AppError> {
    if rule.schema_version != PROXY_RULE_SCHEMA_VERSION {
        return Err(AppError::new("proxy_rule_version_unsupported", "Unsupported proxy rule version.", true));
    }
    if [rule.matcher.host.value.len(), rule.matcher.path.value.len()].into_iter().any(|length| length == 0 || length > 256) {
        return Err(AppError::new("proxy_rule_pattern_size", "Rule patterns must be 1–256 bytes.", true));
    }
    for pattern in [&rule.matcher.host, &rule.matcher.path] {
        pattern.matches("").map_err(|error| AppError::new("proxy_rule_pattern_invalid", error.to_string(), true))?;
    }
    if rule.matcher.method.as_deref().is_some_and(|method| method.is_empty() || method.len() > 32 || !method.bytes().all(|byte| byte.is_ascii_alphabetic())) {
        return Err(AppError::new("proxy_rule_method_invalid", "Rule method must contain 1–32 letters.", true));
    }
    Ok(())
}
