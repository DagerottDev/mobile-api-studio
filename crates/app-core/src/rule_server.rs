use core_model::proxy_rules::ProxyRule;
use serde::{Deserialize, Serialize};
#[cfg(unix)]
use std::{
    fs,
    io::{ErrorKind, Read},
    os::unix::{
        fs::{DirBuilderExt, PermissionsExt},
        net::UnixStream as StdUnixStream,
    },
    path::{Path, PathBuf},
};
use storage::Database;
#[cfg(unix)]
use tokio::net::UnixListener;
#[cfg(all(unix, test))]
use tokio::net::UnixStream;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    task::{JoinHandle, JoinSet},
    time::{Duration, timeout},
};

const MAX_REQUEST_BYTES: usize = 8 * 1024;
const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RuleRequest {
    method: String,
    host: String,
    path: String,
    #[serde(default)]
    request_id: Option<String>,
    #[serde(default)]
    network_profile_id: Option<String>,
    #[serde(default)]
    token: Option<String>,
}

#[derive(Serialize)]
struct RuleResponse {
    rules: Vec<ProxyRule>,
    #[serde(rename = "networkProfile")]
    network_profile: Option<core_model::network_profiles::NetworkProfile>,
    #[serde(rename = "networkProfileEnabled")]
    network_profile_enabled: bool,
}

#[cfg(unix)]
pub(super) fn socket_path() -> Result<PathBuf, String> {
    let mut random = [0_u8; 8];
    fs::File::open("/dev/urandom")
        .and_then(|mut source| source.read_exact(&mut random))
        .map_err(|error| error.to_string())?;
    let base = if Path::new("/private/tmp").is_dir() {
        "/private/tmp"
    } else {
        "/tmp"
    };
    let suffix = random
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok(Path::new(base)
        .join(format!("mas-rules-{}-{suffix}", std::process::id()))
        .join("s"))
}

#[cfg(unix)]
pub(super) fn start(database: Database, path: &Path) -> Result<JoinHandle<()>, String> {
    let sdk_database =
        sdk_storage::SdkDatabase::open(database.path()).map_err(|error| error.to_string())?;
    let directory = path.parent().ok_or("Rule socket has no parent directory")?;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(directory)
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
    let listener =
        std::os::unix::net::UnixListener::bind(path).map_err(|error| error.to_string())?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .map_err(|error| error.to_string())?;
    listener
        .set_nonblocking(true)
        .map_err(|error| error.to_string())?;
    let listener = UnixListener::from_std(listener).map_err(|error| error.to_string())?;
    Ok(tokio::spawn(async move {
        let mut connections = JoinSet::new();
        loop {
            while connections.try_join_next().is_some() {}
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            if connections.len() >= 64 {
                continue;
            }
            let database = database.clone();
            let sdk_database = sdk_database.clone();
            connections.spawn(async move {
                let _ = timeout(
                    Duration::from_secs(3),
                    serve_one(stream, &database, &sdk_database, None),
                )
                .await;
            });
        }
    }))
}

#[cfg(any(windows, test))]
pub(super) struct LoopbackListener {
    listener: std::net::TcpListener,
    pub(super) port: u16,
    pub(super) token: String,
}
#[cfg(any(windows, test))]
impl std::fmt::Debug for LoopbackListener {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoopbackListener")
            .field("port", &self.port)
            .field("token", &"[redacted]")
            .finish()
    }
}
#[cfg(any(windows, test))]
pub(super) fn loopback_listener() -> Result<LoopbackListener, String> {
    let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .map_err(|_| "Cannot bind private rule loopback listener")?;
    listener
        .set_nonblocking(true)
        .map_err(|_| "Cannot configure private rule loopback listener")?;
    let port = listener
        .local_addr()
        .map_err(|_| "Cannot determine private rule loopback port")?
        .port();
    let mut random = [0u8; 32];
    getrandom::fill(&mut random).map_err(|_| "Cannot generate private rule transport token")?;
    let token = random.iter().map(|b| format!("{b:02x}")).collect();
    Ok(LoopbackListener {
        listener,
        port,
        token,
    })
}
#[cfg(any(windows, test))]
pub(super) fn start_loopback(
    database: Database,
    endpoint: LoopbackListener,
) -> Result<JoinHandle<()>, String> {
    let sdk_database =
        sdk_storage::SdkDatabase::open(database.path()).map_err(|error| error.to_string())?;
    let listener = tokio::net::TcpListener::from_std(endpoint.listener)
        .map_err(|_| "Cannot start private rule loopback listener")?;
    Ok(tokio::spawn(async move {
        let mut connections = JoinSet::new();
        loop {
            while connections.try_join_next().is_some() {}
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            if connections.len() >= 64 {
                continue;
            }
            let database = database.clone();
            let sdk_database = sdk_database.clone();
            let token = endpoint.token.clone();
            connections.spawn(async move {
                let _ = timeout(
                    Duration::from_secs(3),
                    serve_one(stream, &database, &sdk_database, Some(&token)),
                )
                .await;
            });
        }
    }))
}
fn valid_token(expected: &str, actual: &str) -> bool {
    expected.len() == actual.len()
        && expected
            .as_bytes()
            .iter()
            .zip(actual.as_bytes())
            .fold(0u8, |m, (a, b)| m | (a ^ b))
            == 0
}
async fn serve_one<S: AsyncRead + AsyncWrite + Unpin>(
    mut stream: S,
    database: &Database,
    sdk_database: &sdk_storage::SdkDatabase,
    expected_token: Option<&str>,
) -> Result<(), String> {
    let mut bytes = Vec::new();
    loop {
        let mut chunk = [0_u8; 1024];
        let count = stream
            .read(&mut chunk)
            .await
            .map_err(|error| error.to_string())?;
        if count == 0 || bytes.len() + count > MAX_REQUEST_BYTES {
            return Err("Rule request too large or incomplete".into());
        }
        bytes.extend_from_slice(&chunk[..count]);
        if bytes.last() == Some(&b'\n') {
            break;
        }
    }
    let request: RuleRequest = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    if expected_token.is_some_and(|expected| {
        !valid_token(expected, request.token.as_deref().unwrap_or_default())
    }) {
        return Err("Unauthorized rule transport".into());
    }
    if request.method.len() > 32
        || request.host.len() > 255
        || request.path.len() > 4096
        || request.request_id.as_ref().is_some_and(|id| id.len() > 512)
        || request
            .network_profile_id
            .as_ref()
            .is_some_and(|id| id.len() > 120)
    {
        return Err("Rule request fields are too large".into());
    }
    // ponytail: per-flow SQLite scan; cache validated rules when capture throughput makes this costly.
    let mut matches = Vec::new();
    let mut matched_bytes = 0_usize;
    for rule in database
        .list_proxy_rules()
        .map_err(|error| error.to_string())?
    {
        if rule.enabled
            && rule
                .matcher
                .matches(&request.method, &request.host, &request.path)
                .map_err(|error| error.to_string())?
        {
            matched_bytes = matched_bytes.saturating_add(
                serde_json::to_vec(&rule)
                    .map_err(|error| error.to_string())?
                    .len()
                    + 1,
            );
            if matched_bytes + 32 > MAX_RESPONSE_BYTES {
                return Err("Matching rule response exceeds 8 MiB".into());
            }
            matches.push(rule);
        }
    }
    let profiles = database
        .list_network_profiles()
        .map_err(|error| error.to_string())?;
    let app_id = request
        .request_id
        .as_deref()
        .map(|id| sdk_database.app_for_request(id))
        .transpose()
        .map_err(|error| error.to_string())?
        .flatten();
    let network_profile = core_model::network_profiles::select_profile(
        &profiles,
        &request.method,
        &request.host,
        &request.path,
        app_id.as_deref(),
    );
    let network_profile_enabled = request.network_profile_id.as_ref().is_some_and(|id| {
        profiles
            .iter()
            .any(|profile| &profile.id == id && profile.enabled)
    });
    let mut response = serde_json::to_vec(&RuleResponse {
        rules: matches,
        network_profile,
        network_profile_enabled,
    })
    .map_err(|error| error.to_string())?;
    if response.len() > MAX_RESPONSE_BYTES {
        return Err("Rule response too large".into());
    }
    response.push(b'\n');
    stream
        .write_all(&response)
        .await
        .map_err(|error| error.to_string())?;
    stream.shutdown().await.map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_model::proxy_rules::{
        PROXY_RULE_SCHEMA_VERSION, ProxyRuleAction, ProxyRuleMatcher, RulePattern, RulePatternKind,
    };

    #[cfg(unix)]
    #[test]
    fn private_socket_returns_ordered_matching_rules() {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let root = std::env::temp_dir().join(format!(
                    "mas-rule-socket-{}-{}",
                    std::process::id(),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_nanos()
                ));
                fs::create_dir_all(&root).unwrap();
                let database = Database::open(root.join("app.db")).unwrap();
                database
                    .upsert_proxy_rule(&ProxyRule {
                        schema_version: PROXY_RULE_SCHEMA_VERSION,
                        id: "block-api".into(),
                        name: "Block API".into(),
                        enabled: true,
                        priority: 1,
                        matcher: ProxyRuleMatcher {
                            method: Some("GET".into()),
                            host: RulePattern {
                                kind: RulePatternKind::Wildcard,
                                value: "*.example.com".into(),
                            },
                            path: RulePattern {
                                kind: RulePatternKind::Regex,
                                value: "/v[0-9]+/users".into(),
                            },
                        },
                        action: ProxyRuleAction::Block { status_code: 403 },
                        created_at: "1".into(),
                        updated_at: "1".into(),
                    })
                    .unwrap();
                let socket = socket_path().unwrap();
                let task = start(database, &socket).unwrap();
                assert_eq!(
                    fs::metadata(socket.parent().unwrap())
                        .unwrap()
                        .permissions()
                        .mode()
                        & 0o077,
                    0
                );
                assert_eq!(
                    fs::metadata(&socket).unwrap().permissions().mode() & 0o077,
                    0
                );
                let mut stream = UnixStream::connect(&socket).await.unwrap();
                stream
                    .write_all(
                        br#"{"method":"get","host":"API.EXAMPLE.COM","path":"/v2/users"}
"#,
                    )
                    .await
                    .unwrap();
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

    #[test]
    fn authenticated_loopback_selects_rules_profiles_and_rejects_untrusted_input() {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async{
            let root=std::env::temp_dir().join(format!("mas-rule-loopback-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
            std::fs::create_dir_all(&root).unwrap();let database=Database::open(root.join("app.db")).unwrap();
            for (id,priority,host,action) in [("later",2,"api.example.test",ProxyRuleAction::Block{status_code:403}),("first",1,"api.example.test",ProxyRuleAction::ScriptHook{stage:"request".into(),script:"flow.method = 'POST'".into()}),("unselected",0,"other.test",ProxyRuleAction::Allow)] {
                database.upsert_proxy_rule(&ProxyRule{schema_version:PROXY_RULE_SCHEMA_VERSION,id:id.into(),name:id.into(),enabled:true,priority,matcher:ProxyRuleMatcher{method:Some("GET".into()),host:RulePattern{kind:RulePatternKind::Exact,value:host.into()},path:RulePattern{kind:RulePatternKind::Exact,value:"/users".into()}},action,created_at:"1".into(),updated_at:"1".into()}).unwrap();
            }
            let profile:core_model::network_profiles::NetworkProfile=serde_json::from_value(serde_json::json!({"schemaVersion":1,"id":"endpoint-profile","name":"Endpoint","enabled":true,"priority":0,"scope":{"type":"endpoint","method":"GET","host":"api.example.test","path":"/users"},"latencyMs":100,"jitterMs":0,"uploadBytesPerSecond":null,"downloadBytesPerSecond":null,"offline":false,"failurePercent":0,"createdAt":"1","updatedAt":"1"})).unwrap();
            database.upsert_network_profile(&profile).unwrap();
            let endpoint=loopback_listener().unwrap();assert!(endpoint.listener.local_addr().unwrap().ip().is_loopback());assert_eq!(endpoint.token.len(),64);assert!(!format!("{endpoint:?}").contains(&endpoint.token));
            let port=endpoint.port;let token=endpoint.token.clone();let task=start_loopback(database.clone(),endpoint).unwrap();
            async fn query(port:u16,bytes:&[u8])->Vec<u8>{let mut stream=tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST,port)).await.unwrap();stream.write_all(bytes).await.unwrap();let mut response=Vec::new();let result=timeout(Duration::from_secs(4),stream.read_to_end(&mut response)).await.unwrap();assert!(result.is_ok()||response.is_empty());response}
            let request=|token:Option<&str>|{let mut v=serde_json::json!({"method":"get","host":"API.EXAMPLE.TEST","path":"/users","network_profile_id":"endpoint-profile"});if let Some(token)=token{v["token"]=serde_json::json!(token);}let mut bytes=serde_json::to_vec(&v).unwrap();bytes.push(b'\n');bytes};
            assert!(query(port,&request(None)).await.is_empty());assert!(query(port,&request(Some(&"0".repeat(64)))).await.is_empty());
            let response:serde_json::Value=serde_json::from_slice(&query(port,&request(Some(&token))).await).unwrap();assert_eq!(response["rules"][0]["id"],"first");assert_eq!(response["rules"][1]["id"],"later");assert_eq!(response["rules"].as_array().unwrap().len(),2);assert_eq!(response["networkProfile"]["id"],"endpoint-profile");assert_eq!(response["networkProfileEnabled"],true);
            let mut large=serde_json::json!({"method":"GET","host":"x".repeat(MAX_REQUEST_BYTES+1),"path":"/users","token":token}).to_string().into_bytes();large.push(b'\n');assert!(query(port,&large).await.is_empty());
            let mut bad_fields=serde_json::json!({"method":"GET","host":"api.example.test","path":"x".repeat(4097),"token":token}).to_string().into_bytes();bad_fields.push(b'\n');assert!(query(port,&bad_fields).await.is_empty());
            assert!(query(port,b"GET / HTTP/1.1\r\n\r\n").await.is_empty());
            #[cfg(unix)]
            {
                let addon=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../sidecars/mitm-addon/mas_bridge.py");
                let script=r#"
import ast, asyncio, json, os, sys, types
source = ast.parse(open(sys.argv[1], encoding="utf-8").read())
names = {"_rule_service_available", "_rule_document", "_proxy_rules_for"}
functions = [node for node in source.body if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)) and node.name in names]
namespace = {"os": types.SimpleNamespace(name="nt"), "asyncio": asyncio, "json": json, "RULE_SOCKET": None, "RULE_TCP_PORT": os.environ["MAS_RULE_TCP_PORT"], "RULE_TCP_TOKEN": os.environ["MAS_RULE_TCP_TOKEN"], "MAX_RULE_REQUEST_BYTES": 8192, "MAX_RULE_RESPONSE_BYTES": 8 * 1024 * 1024}
exec(compile(ast.Module(body=functions, type_ignores=[]), "addon-rule-transport", "exec"), namespace)
async def check():
    assert namespace["_rule_service_available"]()
    rules = await namespace["_proxy_rules_for"]("get", "API.EXAMPLE.TEST", "/users")
    assert [rule["id"] for rule in rules] == ["first", "later"]
    document = await namespace["_rule_document"]({"method": "GET", "host": "api.example.test", "path": "/users", "network_profile_id": "endpoint-profile"})
    assert document["networkProfile"]["id"] == "endpoint-profile" and document["networkProfileEnabled"]
    try:
        await namespace["_rule_document"]({"method": "GET", "host": "x" * 9000, "path": "/users"})
    except ValueError:
        pass
    else:
        raise AssertionError("Unbounded addon lookup was accepted")
asyncio.run(check())
"#;
                let status=tokio::process::Command::new("python3").arg("-I").arg("-c").arg(script).arg(addon).env_clear().env("MAS_RULE_TCP_PORT",port.to_string()).env("MAS_RULE_TCP_TOKEN",&token).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().await.unwrap();
                assert!(status.success(),"Windows addon fallback check failed");
            }
            assert_eq!(database.list_proxy_rules().unwrap().len(),3);assert_eq!(database.list_network_profiles().unwrap().len(),1);task.abort();std::fs::remove_dir_all(root).unwrap();
        });
    }
}
