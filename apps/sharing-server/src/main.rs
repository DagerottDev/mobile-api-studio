use axum::{
    Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, Path, Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, patch, post},
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    net::SocketAddr,
    path::{Path as FilePath, PathBuf},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

mod validate;

const MAX_BODY: usize = 6 * 1024 * 1024;
type ApiResult<T> = Result<T, ApiError>;
#[derive(Debug)]
struct ApiError(StatusCode, &'static str);
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({"error":self.1}))).into_response()
    }
}
impl From<rusqlite::Error> for ApiError {
    fn from(_: rusqlite::Error) -> Self {
        Self(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Storage operation failed",
        )
    }
}
fn bad(message: &'static str) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, message)
}
fn denied() -> ApiError {
    ApiError(StatusCode::FORBIDDEN, "Not permitted")
}
fn missing() -> ApiError {
    ApiError(StatusCode::NOT_FOUND, "Not found")
}
fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn random_token() -> ApiResult<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| {
        ApiError(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Random source unavailable",
        )
    })?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

#[derive(Clone)]
struct App {
    db: Arc<Mutex<Connection>>,
    origin: Option<String>,
}
impl App {
    // ponytail: one SQLite mutex, connection pooling if measured sharing throughput needs it.
    fn db(&self) -> ApiResult<std::sync::MutexGuard<'_, Connection>> {
        self.db
            .lock()
            .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "Storage unavailable"))
    }
}
struct Member {
    id: String,
    name: String,
    role: String,
}
impl Member {
    fn value(&self) -> Value {
        json!({"userId":self.id,"userName":self.name,"role":self.role})
    }
    fn write(&self) -> ApiResult<()> {
        if self.role == "viewer" {
            Err(denied())
        } else {
            Ok(())
        }
    }
    fn owner(&self) -> ApiResult<()> {
        if self.role != "owner" {
            Err(denied())
        } else {
            Ok(())
        }
    }
}
fn auth(db: &Connection, headers: &HeaderMap) -> ApiResult<Member> {
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .filter(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or(ApiError(StatusCode::UNAUTHORIZED, "Access token required"))?;
    db.query_row(
        "SELECT id,name,role FROM members WHERE token_hash=?1",
        [hash(token.as_bytes())],
        |r| {
            Ok(Member {
                id: r.get(0)?,
                name: r.get(1)?,
                role: r.get(2)?,
            })
        },
    )
    .optional()?
    .ok_or(ApiError(StatusCode::UNAUTHORIZED, "Invalid access token"))
}
async fn boundaries(State(app): State<App>, req: Request, next: Next) -> Response {
    let origin = req.headers().get("origin");
    let allowed = origin.is_some_and(|o| {
        app.origin
            .as_deref()
            .is_some_and(|s| o.as_bytes() == s.as_bytes())
    });
    let mut response = if origin.is_some() && !allowed {
        denied().into_response()
    } else if req.method() == Method::OPTIONS && allowed {
        StatusCode::NO_CONTENT.into_response()
    } else {
        next.run(req).await
    };
    let headers = response.headers_mut();
    headers.insert(
        "cache-control",
        HeaderValue::from_static("private, no-store"),
    );
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    headers.insert(
        "content-security-policy",
        HeaderValue::from_static("default-src 'none'; frame-ancestors 'none'"),
    );
    if allowed {
        headers.insert(
            "access-control-allow-origin",
            HeaderValue::from_str(app.origin.as_deref().unwrap()).unwrap(),
        );
        headers.insert(
            "access-control-allow-methods",
            HeaderValue::from_static("GET, POST, PUT, PATCH, DELETE, OPTIONS"),
        );
        headers.insert(
            "access-control-allow-headers",
            HeaderValue::from_static("Authorization, Content-Type"),
        );
        headers.insert("vary", HeaderValue::from_static("Origin"));
    }
    response
}
async fn me(State(app): State<App>, h: HeaderMap) -> ApiResult<Json<Value>> {
    let db = app.db()?;
    let m = auth(&db, &h)?;
    Ok(Json(
        json!({"teamId":"default","teamName":"Mobile API Studio team","userId":m.id,"userName":m.name,"role":m.role}),
    ))
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ShareInput {
    artifact: String,
    expires_in_seconds: i64,
    sha256: Option<String>,
}
async fn create_share(
    State(app): State<App>,
    h: HeaderMap,
    Json(input): Json<ShareInput>,
) -> ApiResult<Json<Value>> {
    let db = app.db()?;
    let m = auth(&db, &h)?;
    m.write()?;
    if !(1..=604800).contains(&input.expires_in_seconds) {
        return Err(bad("TTL must be 1 to 604800 seconds"));
    }
    validate::har(&input.artifact)?;
    let digest = hash(input.artifact.as_bytes());
    if input.sha256.as_ref().is_some_and(|v| v != &digest) {
        return Err(bad("Preview digest mismatch"));
    }
    db.execute(
        "DELETE FROM shares WHERE expires_at<=?1 OR revoked=1",
        [now()],
    )?;
    let count: i64 = db.query_row("SELECT count(*) FROM shares", [], |r| r.get(0))?;
    if count >= 1000 {
        return Err(bad("Share limit reached; revoke existing shares"));
    }
    let bytes: i64 = db.query_row(
        "SELECT coalesce(sum(length(CAST(artifact AS BLOB))),0) FROM shares",
        [],
        |r| r.get(0),
    )?;
    if bytes + input.artifact.len() as i64 > 64 * 1024 * 1024 {
        return Err(bad(
            "Shared artifact storage exceeds 64 MiB; revoke existing shares",
        ));
    }
    let token = random_token()?;
    let id = random_token()?;
    let expires = now() + input.expires_in_seconds;
    db.execute("INSERT INTO shares(id,token_hash,creator,artifact,expires_at,sha256) VALUES(?1,?2,?3,?4,?5,?6)",params![id,hash(token.as_bytes()),m.id,input.artifact,expires,digest])?;
    Ok(Json(
        json!({"id":id,"urlPath":format!("/s/{token}"),"expiresAt":expires,"sha256":digest}),
    ))
}
async fn list_shares(State(app): State<App>, h: HeaderMap) -> ApiResult<Json<Value>> {
    let db = app.db()?;
    auth(&db, &h)?;
    let mut stmt=db.prepare("SELECT id,creator,expires_at,sha256,revoked FROM shares ORDER BY expires_at DESC LIMIT 1000")?;
    let rows=stmt.query_map([],|r|Ok(json!({"id":r.get::<_,String>(0)?,"creatorId":r.get::<_,String>(1)?,"expiresAt":r.get::<_,i64>(2)?,"sha256":r.get::<_,String>(3)?,"revoked":r.get::<_,bool>(4)?})))?.collect::<Result<Vec<_>,_>>()?;
    Ok(Json(json!({"shares":rows})))
}
async fn share(State(app): State<App>, Path(token): Path<String>) -> ApiResult<Response> {
    if token.len() != 64 || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(missing());
    }
    let db = app.db()?;
    let artifact: Option<String> = db
        .query_row(
            "SELECT artifact FROM shares WHERE token_hash=?1 AND revoked=0 AND expires_at>?2",
            params![hash(token.as_bytes()), now()],
            |r| r.get(0),
        )
        .optional()?;
    let artifact = artifact.ok_or_else(missing)?;
    Ok(Response::builder()
        .header("content-type", "application/json; charset=utf-8")
        .header(
            "content-disposition",
            "attachment; filename=redacted-capture.har",
        )
        .body(Body::from(artifact))
        .unwrap())
}
async fn revoke(
    State(app): State<App>,
    h: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    let db = app.db()?;
    let m = auth(&db, &h)?;
    let creator: Option<String> = db
        .query_row("SELECT creator FROM shares WHERE id=?1", [&id], |r| {
            r.get(0)
        })
        .optional()?;
    let creator = creator.ok_or_else(missing)?;
    if m.role == "viewer" && creator != m.id {
        return Err(denied());
    }
    db.execute("UPDATE shares SET revoked=1,artifact='' WHERE id=?1", [id])?;
    Ok(StatusCode::NO_CONTENT)
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WorkspaceInput {
    schema_version: u8,
    expected_revision: i64,
    rules: Vec<validate::StrictValue>,
    fixtures: Vec<validate::StrictValue>,
}
fn workspace_value(db: &Connection) -> ApiResult<Value> {
    let (rev, rules, fixtures): (i64, String, String) = db.query_row(
        "SELECT revision,rules,fixtures FROM workspace WHERE id=1",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let rules: Value = serde_json::from_str(&rules).map_err(|_| {
        ApiError(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Stored workspace invalid",
        )
    })?;
    let fixtures: Value = serde_json::from_str(&fixtures).map_err(|_| {
        ApiError(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Stored workspace invalid",
        )
    })?;
    Ok(json!({"schemaVersion":1,"revision":rev,"rules":rules,"fixtures":fixtures}))
}
async fn get_workspace(State(app): State<App>, h: HeaderMap) -> ApiResult<Json<Value>> {
    let db = app.db()?;
    auth(&db, &h)?;
    Ok(Json(workspace_value(&db)?))
}
async fn put_workspace(
    State(app): State<App>,
    h: HeaderMap,
    Json(input): Json<WorkspaceInput>,
) -> ApiResult<Json<Value>> {
    let db = app.db()?;
    let m = auth(&db, &h)?;
    m.write()?;
    if input.schema_version != 1 || input.expected_revision < 0 {
        return Err(bad("Invalid workspace version"));
    }
    let rules: Vec<Value> = input.rules.into_iter().map(|v| v.0).collect();
    let fixtures: Vec<Value> = input.fixtures.into_iter().map(|v| v.0).collect();
    validate::workspace(&rules, &fixtures)?;
    let changed = db.execute(
        "UPDATE workspace SET revision=revision+1,rules=?1,fixtures=?2 WHERE id=1 AND revision=?3",
        params![
            serde_json::to_string(&rules).unwrap(),
            serde_json::to_string(&fixtures).unwrap(),
            input.expected_revision
        ],
    )?;
    if changed != 1 {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "Workspace revision changed; fetch and review before retrying",
        ));
    }
    Ok(Json(workspace_value(&db)?))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewMember {
    name: String,
    role: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RoleInput {
    role: String,
}
fn role(role: &str) -> ApiResult<()> {
    if ["owner", "editor", "viewer"].contains(&role) {
        Ok(())
    } else {
        Err(bad("Invalid role"))
    }
}
async fn members(State(app): State<App>, h: HeaderMap) -> ApiResult<Json<Value>> {
    let db = app.db()?;
    auth(&db, &h)?.owner()?;
    let mut stmt = db.prepare("SELECT id,name,role FROM members ORDER BY name,id LIMIT 100")?;
    let rows = stmt
        .query_map([], |r| {
            Ok(Member {
                id: r.get(0)?,
                name: r.get(1)?,
                role: r.get(2)?,
            }
            .value())
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(json!({"members":rows})))
}
async fn add_member(
    State(app): State<App>,
    h: HeaderMap,
    Json(input): Json<NewMember>,
) -> ApiResult<Json<Value>> {
    let db = app.db()?;
    auth(&db, &h)?.owner()?;
    role(&input.role)?;
    if input.name.trim().is_empty()
        || input.name.len() > 128
        || input.name.chars().any(char::is_control)
    {
        return Err(bad("Name must be 1 to 128 bytes"));
    }
    let count: i64 = db.query_row("SELECT count(*) FROM members", [], |r| r.get(0))?;
    if count >= 100 {
        return Err(bad("Member limit reached"));
    }
    let id = random_token()?;
    let token = random_token()?;
    db.execute(
        "INSERT INTO members(id,name,role,token_hash) VALUES(?1,?2,?3,?4)",
        params![id, input.name, input.role, hash(token.as_bytes())],
    )?;
    Ok(Json(
        json!({"member":Member{id,name:input.name,role:input.role}.value(),"accessToken":token}),
    ))
}
fn protect_last_owner(db: &Connection, id: &str) -> ApiResult<()> {
    let existing: Option<String> = db
        .query_row("SELECT role FROM members WHERE id=?1", [id], |r| r.get(0))
        .optional()?;
    let existing = existing.ok_or_else(missing)?;
    if existing == "owner" {
        let count: i64 =
            db.query_row("SELECT count(*) FROM members WHERE role='owner'", [], |r| {
                r.get(0)
            })?;
        if count <= 1 {
            return Err(bad("Cannot remove or demote the last owner"));
        }
    }
    Ok(())
}
async fn change_role(
    State(app): State<App>,
    h: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<RoleInput>,
) -> ApiResult<StatusCode> {
    let mut db = app.db()?;
    let tx = db.transaction()?;
    auth(&tx, &h)?.owner()?;
    role(&input.role)?;
    if input.role != "owner" {
        protect_last_owner(&tx, &id)?;
    }
    if tx.execute(
        "UPDATE members SET role=?1 WHERE id=?2",
        params![input.role, id],
    )? != 1
    {
        return Err(missing());
    }
    tx.commit()?;
    Ok(StatusCode::NO_CONTENT)
}
async fn remove_member(
    State(app): State<App>,
    h: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    let mut db = app.db()?;
    let tx = db.transaction()?;
    auth(&tx, &h)?.owner()?;
    protect_last_owner(&tx, &id)?;
    tx.execute(
        "UPDATE shares SET revoked=1,artifact='' WHERE creator=?1",
        [&id],
    )?;
    tx.execute("DELETE FROM members WHERE id=?1", [id])?;
    tx.commit()?;
    Ok(StatusCode::NO_CONTENT)
}
async fn rotate_token(
    State(app): State<App>,
    h: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let db = app.db()?;
    auth(&db, &h)?.owner()?;
    let token = random_token()?;
    if db.execute(
        "UPDATE members SET token_hash=?1 WHERE id=?2",
        params![hash(token.as_bytes()), id],
    )? != 1
    {
        return Err(missing());
    }
    Ok(Json(json!({"accessToken":token})))
}
fn router(app: App) -> Router {
    Router::new()
        .route("/v1/me", get(me))
        .route("/v1/shares", get(list_shares).post(create_share))
        .route("/v1/shares/{id}", axum::routing::delete(revoke))
        .route("/s/{token}", get(share))
        .route("/v1/workspace", get(get_workspace).put(put_workspace))
        .route("/v1/members", get(members).post(add_member))
        .route("/v1/members/{id}", patch(change_role).delete(remove_member))
        .route("/v1/members/{id}/token", post(rotate_token))
        .layer(DefaultBodyLimit::max(MAX_BODY))
        .layer(middleware::from_fn_with_state(app.clone(), boundaries))
        .with_state(app)
}
#[cfg(unix)]
fn secure_dir(path: &FilePath) -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt};
    match fs::symlink_metadata(path) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(path)?;
        }
        Err(error) => return Err(error.into()),
    }
    let m = fs::symlink_metadata(path)?;
    if !m.is_dir() || m.file_type().is_symlink() {
        return Err("Data directory must be a real directory".into());
    }
    // SAFETY: geteuid takes no arguments and returns the process effective user ID.
    if m.mode() & 0o777 != 0o700 || m.uid() != unsafe { libc::geteuid() } {
        return Err("Data directory must have mode 0700 and belong to the effective user".into());
    }
    Ok(())
}
#[cfg(not(unix))]
fn secure_dir(_: &FilePath) -> Result<(), Box<dyn std::error::Error>> {
    Err("Sharing storage requires Unix owner-only permissions; Windows owner-only ACL support is not implemented".into())
}
fn secure_database_file(path: &FilePath) -> Result<(), Box<dyn std::error::Error>> {
    match fs::symlink_metadata(path) {
        Ok(m) => {
            if !m.is_file() || m.file_type().is_symlink() {
                return Err("Database files must be regular files, never symlinks".into());
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                // SAFETY: geteuid takes no arguments and returns the process effective user ID.
                if m.uid() != unsafe { libc::geteuid() } || m.mode() & 0o077 != 0 || m.nlink() != 1
                {
                    return Err("Database files must be private, singly linked files owned by the effective user".into());
                }
            }
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}
fn initialize(dir: &FilePath) -> Result<Connection, Box<dyn std::error::Error>> {
    use std::io::Write;
    #[cfg(unix)]
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    secure_dir(dir)?;
    let dbpath = dir.join("sharing.sqlite3");
    for name in [
        "sharing.sqlite3",
        "sharing.sqlite3-journal",
        "sharing.sqlite3-wal",
        "sharing.sqlite3-shm",
    ] {
        secure_database_file(&dir.join(name))?;
    }
    let db = Connection::open(&dbpath)?;
    #[cfg(unix)]
    fs::set_permissions(&dbpath, fs::Permissions::from_mode(0o600))?;
    db.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=DELETE; CREATE TABLE IF NOT EXISTS members(id TEXT PRIMARY KEY,name TEXT NOT NULL,role TEXT NOT NULL CHECK(role IN ('owner','editor','viewer')),token_hash TEXT NOT NULL UNIQUE); CREATE TABLE IF NOT EXISTS shares(id TEXT PRIMARY KEY,token_hash TEXT NOT NULL UNIQUE,creator TEXT NOT NULL,artifact TEXT NOT NULL,expires_at INTEGER NOT NULL,sha256 TEXT NOT NULL,revoked INTEGER NOT NULL DEFAULT 0 CHECK(revoked IN (0,1))); CREATE TABLE IF NOT EXISTS workspace(id INTEGER PRIMARY KEY CHECK(id=1),revision INTEGER NOT NULL,rules TEXT NOT NULL,fixtures TEXT NOT NULL); INSERT OR IGNORE INTO workspace VALUES(1,0,'[]','[]');")?;
    let owners: i64 = db.query_row("SELECT count(*) FROM members WHERE role='owner'", [], |r| {
        r.get(0)
    })?;
    if owners == 0 {
        let provided = std::env::var("MAS_SHARING_OWNER_TOKEN").ok();
        let token=match provided.as_ref() {
            Some(token) if token.len()==64 && token.bytes().all(|b|b.is_ascii_hexdigit())=>token.clone(),
            Some(_)=>return Err("MAS_SHARING_OWNER_TOKEN must be 64 hex characters from a cryptographic random source".into()),
            None=>random_token().map_err(|_|"Random source unavailable")?,
        };
        // Create-new intentionally refuses to read or replace an existing token file.
        if provided.is_none() {
            let mut options = fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            options.mode(0o600);
            let mut file = options.open(dir.join("owner-access-token"))?;
            file.write_all(token.as_bytes())?;
            file.sync_all()?;
        }
        db.execute(
            "INSERT INTO members VALUES('owner','Owner','owner',?1)",
            [hash(token.as_bytes())],
        )?;
    }
    Ok(db)
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut listen: SocketAddr = "127.0.0.1:8190".parse()?;
    let mut dir = None;
    let mut origin = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--listen" => listen = args.next().ok_or("--listen requires an address")?.parse()?,
            "--data-dir" => {
                dir = Some(PathBuf::from(
                    args.next().ok_or("--data-dir requires a path")?,
                ))
            }
            "--desktop-origin" => {
                let value = args
                    .next()
                    .ok_or("--desktop-origin requires an exact origin")?;
                let url = url::Url::parse(&value)?;
                let canonical =
                    url.origin().ascii_serialization() == value || value == "tauri://localhost";
                if !["http", "https", "tauri"].contains(&url.scheme())
                    || url.host_str().is_none()
                    || !canonical
                    || !url.username().is_empty()
                    || url.password().is_some()
                {
                    return Err(
                        "Desktop origin must be one exact http(s) origin or tauri://localhost"
                            .into(),
                    );
                }
                origin = Some(value);
            }
            "--help" => {
                println!(
                    "mobile-api-studio-sharing-server --data-dir PRIVATE_DIRECTORY [--listen 127.0.0.1:8190] [--desktop-origin EXACT_ORIGIN]\nRemote listening is explicit opt-in; terminate TLS at a trusted reverse proxy. Bootstrap token: PRIVATE_DIRECTORY/owner-access-token (never logged)."
                );
                return Ok(());
            }
            _ => return Err("Unknown argument".into()),
        }
    }
    let dir = dir.ok_or("--data-dir is required")?;
    let app = App {
        db: Arc::new(Mutex::new(initialize(&dir)?)),
        origin,
    };
    let listener = tokio::net::TcpListener::bind(listen).await?;
    println!("Sharing service listening on {}", listener.local_addr()?);
    axum::serve(listener, router(app))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
