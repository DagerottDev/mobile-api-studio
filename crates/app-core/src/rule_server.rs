use core_model::proxy_rules::ProxyRule;
use serde::{Deserialize, Serialize};
use std::{fs, io::{ErrorKind, Read}, os::unix::{fs::{DirBuilderExt, PermissionsExt}, net::UnixStream as StdUnixStream}, path::{Path, PathBuf}};
use storage::Database;
use tokio::{io::{AsyncReadExt, AsyncWriteExt}, net::{UnixListener, UnixStream}, task::{JoinHandle, JoinSet}, time::{timeout, Duration}};

const MAX_REQUEST_BYTES: usize = 8 * 1024;
const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Deserialize)]
struct RuleRequest {
    method: String,
    host: String,
    path: String,
}

#[derive(Serialize)]
struct RuleResponse {
    rules: Vec<ProxyRule>,
}

pub(super) fn socket_path() -> Result<PathBuf, String> {
    let mut random = [0_u8; 8];
    fs::File::open("/dev/urandom").and_then(|mut source| source.read_exact(&mut random))
        .map_err(|error| error.to_string())?;
    let base = if Path::new("/private/tmp").is_dir() { "/private/tmp" } else { "/tmp" };
    let suffix = random.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
    Ok(Path::new(base).join(format!("mas-rules-{}-{suffix}", std::process::id())).join("s"))
}

pub(super) fn start(database: Database, path: &Path) -> Result<JoinHandle<()>, String> {
    let directory = path.parent().ok_or("Rule socket has no parent directory")?;
    fs::DirBuilder::new().recursive(true).mode(0o700).create(directory)
        .map_err(|error| error.to_string())?;
    fs::set_permissions(directory, fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;
    if path.exists() {
        match StdUnixStream::connect(path) {
            Ok(_) => return Err("Another rule server already owns this socket".into()),
            Err(error) if error.kind() == ErrorKind::ConnectionRefused => {
                fs::remove_file(path).map_err(|error| error.to_string())?;
            }
            Err(error) => return Err(format!("Cannot recover rule socket: {error}")),
        }
    }
    let listener = std::os::unix::net::UnixListener::bind(path).map_err(|error| error.to_string())?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|error| error.to_string())?;
    listener.set_nonblocking(true).map_err(|error| error.to_string())?;
    let listener = UnixListener::from_std(listener).map_err(|error| error.to_string())?;
    Ok(tokio::spawn(async move {
        let mut connections = JoinSet::new();
        loop {
            while connections.try_join_next().is_some() {}
            let Ok((stream, _)) = listener.accept().await else { break };
            if connections.len() >= 64 { continue; }
            let database = database.clone();
            connections.spawn(async move { let _ = timeout(Duration::from_secs(3), serve_one(stream, &database)).await; });
        }
    }))
}

async fn serve_one(mut stream: UnixStream, database: &Database) -> Result<(), String> {
    let mut bytes = Vec::new();
    loop {
        let mut chunk = [0_u8; 1024];
        let count = stream.read(&mut chunk).await.map_err(|error| error.to_string())?;
        if count == 0 || bytes.len() + count > MAX_REQUEST_BYTES { return Err("Rule request too large or incomplete".into()); }
        bytes.extend_from_slice(&chunk[..count]);
        if bytes.last() == Some(&b'\n') { break; }
    }
    let request: RuleRequest = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    if request.method.len() > 32 || request.host.len() > 255 || request.path.len() > 4096 {
        return Err("Rule request fields are too large".into());
    }
    // ponytail: per-flow SQLite scan; cache validated rules when capture throughput makes this costly.
    let mut matches = Vec::new();
    let mut matched_bytes = 0_usize;
    for rule in database.list_proxy_rules().map_err(|error| error.to_string())? {
        if rule.enabled && rule.matcher.matches(&request.method, &request.host, &request.path).map_err(|error| error.to_string())? {
            matched_bytes = matched_bytes.saturating_add(serde_json::to_vec(&rule).map_err(|error| error.to_string())?.len() + 1);
            if matched_bytes + 32 > MAX_RESPONSE_BYTES { return Err("Matching rule response exceeds 8 MiB".into()); }
            matches.push(rule);
        }
    }
    let mut response = serde_json::to_vec(&RuleResponse { rules: matches }).map_err(|error| error.to_string())?;
    if response.len() > MAX_RESPONSE_BYTES { return Err("Rule response too large".into()); }
    response.push(b'\n');
    stream.write_all(&response).await.map_err(|error| error.to_string())?;
    stream.shutdown().await.map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_model::proxy_rules::{ProxyRuleAction, ProxyRuleMatcher, RulePattern, RulePatternKind, PROXY_RULE_SCHEMA_VERSION};

    #[test]
    fn private_socket_returns_ordered_matching_rules() {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
            let root = std::env::temp_dir().join(format!("mas-rule-socket-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
            fs::create_dir_all(&root).unwrap();
            let database = Database::open(root.join("app.db")).unwrap();
            database.upsert_proxy_rule(&ProxyRule {
                schema_version: PROXY_RULE_SCHEMA_VERSION,
                id: "block-api".into(), name: "Block API".into(), enabled: true, priority: 1,
                matcher: ProxyRuleMatcher {
                    method: Some("GET".into()),
                    host: RulePattern { kind: RulePatternKind::Wildcard, value: "*.example.com".into() },
                    path: RulePattern { kind: RulePatternKind::Regex, value: "/v[0-9]+/users".into() },
                },
                action: ProxyRuleAction::Block { status_code: 403 }, created_at: "1".into(), updated_at: "1".into(),
            }).unwrap();
            let socket = socket_path().unwrap();
            let task = start(database, &socket).unwrap();
            assert_eq!(fs::metadata(socket.parent().unwrap()).unwrap().permissions().mode() & 0o077, 0);
            assert_eq!(fs::metadata(&socket).unwrap().permissions().mode() & 0o077, 0);
            let mut stream = UnixStream::connect(&socket).await.unwrap();
            stream.write_all(br#"{"method":"get","host":"API.EXAMPLE.COM","path":"/v2/users"}
"#).await.unwrap();
            let mut response = Vec::new();
            stream.read_to_end(&mut response).await.unwrap();
            let parsed: serde_json::Value = serde_json::from_slice(&response).unwrap();
            assert_eq!(parsed["rules"][0]["id"], "block-api");
            task.abort();
            fs::remove_file(&socket).unwrap();
            fs::remove_dir(socket.parent().unwrap()).unwrap();
            fs::remove_dir_all(root).unwrap();
        });
    }
}
