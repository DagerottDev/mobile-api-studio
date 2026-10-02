use crate::{AppState, State, now_epoch_millis};
use base64::{Engine, engine::general_purpose::STANDARD};
use core_model::{
    AppError,
    proxy_rules::{ProxyRule, ProxyRuleAction},
};
use mock_fixtures::{MockFixture, MockFixtureDatabase};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_BYTES: usize = 4 * 1024 * 1024;
fn invalid(message: impl Into<String>) -> AppError {
    AppError::new("sharing_invalid", message, true)
}
fn storage(error: impl std::fmt::Display) -> AppError {
    AppError::storage(error.to_string())
}
fn sensitive(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    replay::is_sensitive_header(&name)
        || ["x-csrf-token", "x-xsrf-token", "x-amz-security-token"].contains(&name.as_str())
        || name.contains("secret")
        || name.ends_with("-token")
        || name.ends_with("-api-key")
}
fn sdk(name: &str) -> bool {
    name.to_ascii_lowercase()
        .starts_with("x-mobile-api-studio-")
}
fn secret_query(name: &str) -> bool {
    sensitive(name)
        || [
            "password",
            "passwd",
            "token",
            "access_token",
            "refresh_token",
            "api_key",
            "apikey",
        ]
        .contains(&name.to_ascii_lowercase().as_str())
}
fn material(bytes: &[u8]) -> Result<(), AppError> {
    let text = String::from_utf8_lossy(bytes).to_ascii_lowercase();
    if [
        "-----begin ",
        "private_key",
        "privatekey",
        "client_secret",
        "aws_secret_access_key",
    ]
    .iter()
    .any(|needle| text.contains(needle))
    {
        return Err(invalid(
            "Remove private or credential material before sharing.",
        ));
    }
    Ok(())
}
fn safe_url(value: &str, include_query: bool) -> Result<String, AppError> {
    let mut url = url::Url::parse(value).map_err(|_| invalid("Invalid shared URL."))?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(invalid("Shared URLs cannot contain credentials."));
    }
    url.set_fragment(None);
    if !include_query {
        url.set_query(None);
    } else if url.query().is_some() {
        let pairs = url
            .query_pairs()
            .map(|(name, value)| {
                (
                    name.to_string(),
                    if secret_query(&name) {
                        "<redacted>".into()
                    } else {
                        value.to_string()
                    },
                )
            })
            .collect::<Vec<_>>();
        url.query_pairs_mut().clear().extend_pairs(pairs);
    }
    Ok(url.into())
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SharePreview {
    pub artifact: String,
    pub sha256: String,
    pub entry_count: usize,
}
pub fn preview_share_har(
    flow_ids: Vec<String>,
    include_query: bool,
    include_bodies: bool,
    state: State<'_, AppState>,
) -> Result<SharePreview, AppError> {
    if flow_ids.is_empty() || flow_ids.len() > 500 {
        return Err(invalid("Select 1 to 500 flows."));
    }
    let artifact = crate::interchange_commands::export_interchange(
        "har".into(),
        flow_ids,
        vec![],
        State(&state),
    )?;
    let mut har: Value = serde_json::from_str(&artifact).map_err(storage)?;
    let entries = har["log"]["entries"]
        .as_array_mut()
        .ok_or_else(|| invalid("HAR entries unavailable."))?;
    for entry in entries.iter_mut() {
        let url = safe_url(
            entry["request"]["url"].as_str().unwrap_or_default(),
            include_query,
        )?;
        let pairs = url::Url::parse(&url)
            .map_err(storage)?
            .query_pairs()
            .map(|(name, value)| json!({"name":name,"value":value}))
            .collect::<Vec<_>>();
        entry["request"]["url"] = json!(url);
        entry["request"]["queryString"] = json!(pairs);
        for side in ["request", "response"] {
            let headers = entry[side]["headers"]
                .as_array_mut()
                .ok_or_else(|| invalid("HAR headers unavailable."))?;
            headers.retain(|h| !sdk(h["name"].as_str().unwrap_or_default()));
            for header in headers {
                if sensitive(header["name"].as_str().unwrap_or_default()) {
                    header["value"] = json!("<redacted>");
                }
            }
            entry[side]["cookies"] = json!([]);
        }
        if !include_bodies {
            entry["request"].as_object_mut().unwrap().remove("postData");
            entry["request"]["bodySize"] = json!(0);
            entry["response"]["content"]
                .as_object_mut()
                .unwrap()
                .remove("text");
            entry["response"]["content"]
                .as_object_mut()
                .unwrap()
                .remove("encoding");
            entry["response"]["content"]["size"] = json!(0);
            entry["response"]["bodySize"] = json!(0);
        } else {
            for body in [&entry["request"]["postData"], &entry["response"]["content"]] {
                if body["encoding"] == "base64" {
                    material(
                        &STANDARD
                            .decode(body["text"].as_str().unwrap_or_default())
                            .map_err(storage)?,
                    )?;
                }
            }
        }
        let redirect = entry["response"]["redirectURL"]
            .as_str()
            .unwrap_or_default();
        if !redirect.is_empty() {
            entry["response"]["redirectURL"] = json!(safe_url(redirect, include_query)?);
        }
    }
    let entry_count = entries.len();
    let artifact = serde_json::to_string_pretty(&har).map_err(storage)?;
    if artifact.len() > MAX_BYTES {
        return Err(invalid("Shared HAR exceeds 4 MiB; select fewer flows."));
    }
    material(artifact.as_bytes())?;
    Ok(SharePreview {
        sha256: format!("{:x}", Sha256::digest(artifact.as_bytes())),
        artifact,
        entry_count,
    })
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SharedWorkspace {
    pub schema_version: u16,
    pub revision: i64,
    pub rules: Vec<ProxyRule>,
    pub fixtures: Vec<MockFixture>,
}
fn portable_rule(rule: &ProxyRule) -> Result<(), AppError> {
    if matches!(
        rule.action,
        ProxyRuleAction::ScriptHook { .. } | ProxyRuleAction::MapLocal { .. }
    ) {
        return Err(invalid(
            "Script hooks and local-file rules cannot be shared.",
        ));
    }
    Ok(())
}
fn validate_document(
    document: &SharedWorkspace,
    state: &State<'_, AppState>,
) -> Result<(), AppError> {
    if document.schema_version != 1
        || document.revision < 0
        || document.rules.len() > 500
        || document.fixtures.len() > 500
    {
        return Err(invalid("Unsupported or oversized team workspace."));
    }
    let bytes = serde_json::to_vec(document).map_err(storage)?;
    if bytes.len() > MAX_BYTES {
        return Err(invalid("Team workspace exceeds 4 MiB."));
    }
    material(&bytes)?;
    let mut ids = HashSet::new();
    for rule in &document.rules {
        if !ids.insert(&rule.id) {
            return Err(invalid("Duplicate proxy rule ID."));
        }
        portable_rule(rule)?;
        crate::proxy_rule_commands::validate_proxy_rule(rule, state, false)?;
        if let ProxyRuleAction::RewriteRequest { headers, .. }
        | ProxyRuleAction::RewriteResponse { headers, .. } = &rule.action
        {
            if headers.iter().any(|h| sensitive(&h.name) || sdk(&h.name)) {
                return Err(invalid(
                    "Sensitive or SDK headers cannot be shared in definitions.",
                ));
            }
        }
        if let ProxyRuleAction::MapRemote { url }
        | ProxyRuleAction::ReverseProxy { url }
        | ProxyRuleAction::UpstreamProxy { url } = &rule.action
        {
            let parsed = url::Url::parse(url).map_err(storage)?;
            if parsed.query_pairs().any(|(name, _)| secret_query(&name)) {
                return Err(invalid(
                    "Remove secret query parameters from shared rule URLs.",
                ));
            }
        }
    }
    ids.clear();
    for fixture in &document.fixtures {
        if !ids.insert(&fixture.id)
            || fixture.id.is_empty()
            || fixture.id.len() > 128
            || fixture.name.len() > 128
            || fixture.schema_version != 1
            || fixture.source_flow_id.is_some()
            || fixture.response_headers.len() > 64
        {
            return Err(invalid("Invalid portable fixture metadata."));
        }
        crate::fixture_commands::validate_fixture(fixture)?;
        for h in &fixture.response_headers {
            if sensitive(&h.name)
                || sdk(&h.name)
                || h.name.is_empty()
                || h.name.len() > 256
                || !h
                    .name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
                || h.value
                    .as_ref()
                    .is_some_and(|s| s.len() > 8192 || s.contains(['\r', '\n']))
                || (h.remove && h.value.is_some())
            {
                return Err(invalid("Invalid or sensitive shared fixture header."));
            }
        }
        if let Some(body) = &fixture.response_body {
            if body.data.len() > MAX_BYTES {
                return Err(invalid("Fixture body too large."));
            }
            if body.encoding == mock_core::MockBodyEncoding::Base64 {
                material(&STANDARD.decode(&body.data).map_err(storage)?)?;
            }
        }
    }
    Ok(())
}
pub fn export_team_workspace(
    rule_ids: Vec<String>,
    fixture_ids: Vec<String>,
    include_bodies: bool,
    state: State<'_, AppState>,
) -> Result<SharedWorkspace, AppError> {
    if rule_ids.len() > 500
        || fixture_ids.len() > 500
        || rule_ids.is_empty() && fixture_ids.is_empty()
    {
        return Err(invalid("Select up to 500 explicit rules and fixtures."));
    }
    let all_rules = state.database.list_proxy_rules().map_err(storage)?;
    let all_fixtures = crate::fixture_commands::list_mock_fixtures(State(&state))?;
    let mut rules = Vec::new();
    let mut fixtures = Vec::new();
    let mut seen = HashSet::new();
    for id in rule_ids {
        if !seen.insert(id.clone()) {
            continue;
        }
        let mut rule = all_rules
            .iter()
            .find(|r| r.id == id)
            .cloned()
            .ok_or_else(|| invalid("Selected rule missing."))?;
        portable_rule(&rule)?;
        if let ProxyRuleAction::RewriteRequest { headers, body }
        | ProxyRuleAction::RewriteResponse { headers, body } = &mut rule.action
        {
            headers.retain(|h| !sensitive(&h.name) && !sdk(&h.name));
            if !include_bodies {
                *body = None;
            }
        }
        rules.push(rule);
    }
    seen.clear();
    for id in fixture_ids {
        if !seen.insert(id.clone()) {
            continue;
        }
        let mut fixture = all_fixtures
            .iter()
            .find(|f| f.id == id)
            .cloned()
            .ok_or_else(|| invalid("Selected fixture missing."))?;
        fixture.source_flow_id = None;
        fixture
            .response_headers
            .retain(|h| !sensitive(&h.name) && !sdk(&h.name));
        if !include_bodies {
            fixture.response_body = None;
        }
        fixtures.push(fixture);
    }
    let document = SharedWorkspace {
        schema_version: 1,
        revision: 0,
        rules,
        fixtures,
    };
    validate_document(&document, &state)?;
    Ok(document)
}
pub fn preview_team_workspace(
    artifact: String,
    state: State<'_, AppState>,
) -> Result<SharedWorkspace, AppError> {
    if artifact.len() > MAX_BYTES {
        return Err(invalid("Team workspace exceeds 4 MiB."));
    }
    let document: SharedWorkspace =
        serde_json::from_str(&artifact).map_err(|_| invalid("Invalid team workspace JSON."))?;
    validate_document(&document, &state)?;
    Ok(document)
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedImport {
    pub rules_imported: usize,
    pub fixtures_imported: usize,
    pub rules_disabled: bool,
}
pub fn import_team_workspace(
    artifact: String,
    state: State<'_, AppState>,
) -> Result<SharedImport, AppError> {
    let mut document = preview_team_workspace(artifact, State(&state))?;
    let timestamp = now_epoch_millis()?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(storage)?
        .as_nanos();
    for (index, rule) in document.rules.iter_mut().enumerate() {
        rule.id = format!("team-{nonce}-rule-{index}");
        rule.enabled = false;
        rule.created_at = timestamp.clone();
        rule.updated_at = timestamp.clone();
    }
    for (index, fixture) in document.fixtures.iter_mut().enumerate() {
        fixture.id = format!("team-{nonce}-fixture-{index}");
        fixture.created_at = timestamp.clone();
        fixture.updated_at = timestamp.clone();
    }
    // Initialize the existing fixture schema only after every definition has been validated.
    MockFixtureDatabase::open(state.database.path()).map_err(storage)?;
    let mut db = Connection::open(state.database.path()).map_err(storage)?;
    db.pragma_update(None, "foreign_keys", "ON")
        .map_err(storage)?;
    let tx = db.transaction().map_err(storage)?;
    let count: i64 = tx
        .query_row("SELECT count(*) FROM proxy_rules", [], |r| r.get(0))
        .map_err(storage)?;
    let size: i64 = tx
        .query_row(
            "SELECT coalesce(sum(length(CAST(rule_json AS BLOB))),0) FROM proxy_rules",
            [],
            |r| r.get(0),
        )
        .map_err(storage)?;
    if count + document.rules.len() as i64 > 1000
        || size + serde_json::to_vec(&document.rules).map_err(storage)?.len() as i64
            > crate::proxy_rule_commands::MAX_RULE_STORAGE_BYTES as i64
    {
        return Err(invalid("Import exceeds local proxy rule limits."));
    }
    let fixture_count: i64 = tx
        .query_row("SELECT count(*) FROM mock_fixtures", [], |r| r.get(0))
        .map_err(storage)?;
    let fixture_size: i64 = tx
        .query_row(
            "SELECT coalesce(sum(length(CAST(fixture_json AS BLOB))),0) FROM mock_fixtures",
            [],
            |r| r.get(0),
        )
        .map_err(storage)?;
    if fixture_count + document.fixtures.len() as i64 > 1000
        || fixture_size
            + serde_json::to_vec(&document.fixtures)
                .map_err(storage)?
                .len() as i64
            > 16 * 1024 * 1024
    {
        return Err(invalid("Import exceeds local fixture limits."));
    }
    for rule in &document.rules {
        tx.execute("INSERT INTO proxy_rules(id,enabled,priority,created_at,rule_json) VALUES(?1,0,?2,?3,?4)",params![rule.id,rule.priority,rule.created_at,serde_json::to_string(rule).map_err(storage)?]).map_err(storage)?;
    }
    for f in &document.fixtures {
        tx.execute("INSERT INTO mock_fixtures(id,schema_version,name,fixture_json,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6)",params![f.id,f.schema_version,f.name,serde_json::to_string(f).map_err(storage)?,f.created_at,f.updated_at]).map_err(storage)?;
    }
    tx.commit().map_err(storage)?;
    Ok(SharedImport {
        rules_imported: document.rules.len(),
        fixtures_imported: document.fixtures.len(),
        rules_disabled: true,
    })
}

#[cfg(test)]
mod checks {
    use super::*;
    #[test]
    fn selected_team_roundtrip_is_redacted_disabled_and_atomic() {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async{
            let root=std::env::temp_dir().join(format!("mas-team-check-{}-{}",std::process::id(),SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
            let state=crate::initialize_state(root.clone(),crate::resolve_addon_path().unwrap()).unwrap();
            let rule:ProxyRule=serde_json::from_value(json!({"schemaVersion":1,"id":"selected-rule","name":"Selected","enabled":true,"priority":1,"matcher":{"method":"GET","host":{"kind":"exact","value":"example.test"},"path":{"kind":"wildcard","value":"*"}},"action":{"type":"rewrite_request","headers":[{"name":"Authorization","value":"synthetic-secret","remove":false}],"body":"synthetic-private-body"},"createdAt":"1","updatedAt":"1"})).unwrap();
            crate::proxy_rule_commands::upsert_proxy_rule(rule.clone(),State(&state)).unwrap();
            let fixture:MockFixture=serde_json::from_value(json!({"schemaVersion":1,"id":"selected-fixture","name":"Selected fixture","statusCode":200,"responseHeaders":[{"name":"Set-Cookie","value":"synthetic-cookie","remove":false}],"responseBody":{"contentType":"text/plain","encoding":"text","data":"synthetic-private-body"},"sourceFlowId":"sdk-correlation","createdAt":"1","updatedAt":"1"})).unwrap();
            crate::fixture_commands::upsert_mock_fixture(fixture,State(&state)).unwrap();
            let document=export_team_workspace(vec![rule.id.clone()],vec!["selected-fixture".into()],false,State(&state)).unwrap();
            let artifact=serde_json::to_string(&document).unwrap();
            assert!(!artifact.contains("synthetic-secret")&&!artifact.contains("synthetic-cookie")&&!artifact.contains("synthetic-private-body")&&!artifact.contains("sdk-correlation"));
            let before=state.database.list_proxy_rules().unwrap();
            preview_team_workspace(artifact.clone(),State(&state)).unwrap();
            assert_eq!(state.database.list_proxy_rules().unwrap(),before);
            let imported=import_team_workspace(artifact,State(&state)).unwrap();assert_eq!(imported.rules_imported,1);assert_eq!(imported.fixtures_imported,1);
            let rules=state.database.list_proxy_rules().unwrap();assert_eq!(rules.len(),2);assert!(rules.iter().find(|r|r.id==rule.id).unwrap().enabled);assert!(!rules.iter().find(|r|r.id!=rule.id).unwrap().enabled);
            let mut invalid=document.clone();invalid.rules[0].action=ProxyRuleAction::ScriptHook{stage:"request".into(),script:"print('bad')".into()};
            assert!(import_team_workspace(serde_json::to_string(&invalid).unwrap(),State(&state)).is_err());assert_eq!(state.database.list_proxy_rules().unwrap(),rules);
            let source=json!({"log":{"version":"1.2","entries":[{"startedDateTime":"2026-10-02T00:00:00Z","time":1,"request":{"method":"GET","url":"https://example.test/path?token=synthetic-query","httpVersion":"HTTP/1.1","headers":[{"name":"Authorization","value":"synthetic-secret"},{"name":"X-Mobile-API-Studio-Request-ID","value":"sdk-correlation"}],"postData":{"mimeType":"text/plain","text":"synthetic-request-body"}},"response":{"status":200,"httpVersion":"HTTP/1.1","statusText":"OK","headers":[],"content":{"mimeType":"text/plain","text":"synthetic-response-body"}},"timings":{"send":0,"wait":1,"receive":0}}]}});
            let bundle=crate::interchange_commands::preview_interchange("har".into(),source.to_string(),State(&state)).unwrap().bundle.unwrap();
            state.database.create_session(&bundle.sessions[0].session).unwrap();
            let flow=&bundle.sessions[0].flows[0];
            for encoded in [&flow.request_body_base64,&flow.response_body_base64].into_iter().flatten(){state.body_store.put(&STANDARD.decode(encoded).unwrap()).unwrap();}
            state.database.upsert_flow_detail(flow.detail.as_ref().unwrap()).unwrap();
            let preview=preview_share_har(vec![flow.summary.id.clone()],false,false,State(&state)).unwrap();
            for secret in ["synthetic-query","synthetic-secret","sdk-correlation","synthetic-request-body","synthetic-response-body"]{assert!(!preview.artifact.contains(secret));}
            assert_eq!(preview.entry_count,1);assert_eq!(preview.sha256,format!("{:x}",Sha256::digest(preview.artifact.as_bytes())));
            let preview=preview_share_har(vec![flow.summary.id.clone()],true,true,State(&state)).unwrap();assert!(!preview.artifact.contains("synthetic-query"));assert!(preview.artifact.contains("synthetic-response-body"));
            drop(state);std::fs::remove_dir_all(root).unwrap();
        });
    }
}
