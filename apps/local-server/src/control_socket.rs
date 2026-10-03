#[cfg(test)]
use crate::control_protocol::MAX_REQUEST;
use crate::control_protocol::{Dispatch, MAX_CONNECTIONS, handle_connection, make_dispatch};
use app_core::CoreService;
use std::{
    fs, io,
    os::unix::fs::{FileTypeExt as _, MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::{
    net::{UnixListener, UnixStream},
    sync::{Semaphore, oneshot},
    task::{JoinHandle, JoinSet},
};

pub struct ControlSocket {
    stop: Option<oneshot::Sender<()>>,
    task: JoinHandle<()>,
}

impl ControlSocket {
    pub async fn shutdown(mut self) {
        self.stop.take();
        let _ = (&mut self.task).await;
    }
}

pub async fn start(core: CoreService, path: PathBuf) -> Result<ControlSocket, String> {
    let dispatch = make_dispatch(core);
    start_with(path, dispatch).await
}

async fn start_with(path: PathBuf, dispatch: Dispatch) -> Result<ControlSocket, String> {
    let directory = path.parent().ok_or("Control socket path has no parent")?;
    let uid = unsafe { libc::geteuid() };
    ensure_control_directory(directory, uid)?;
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err("Refusing a symlink at the control socket path".into());
        }
        Ok(metadata) if metadata.file_type().is_socket() && metadata.uid() == uid => {
            if UnixStream::connect(&path).await.is_ok() {
                return Err("A control service is already listening at this path".into());
            }
            fs::remove_file(&path)
                .map_err(|error| format!("Cannot remove stale control socket: {error}"))?;
        }
        Ok(_) => return Err("Refusing to replace a non-owned or non-socket control path".into()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("Cannot inspect control socket path: {error}")),
    }
    let listener = UnixListener::bind(&path)
        .map_err(|error| format!("Cannot bind control socket: {error}"))?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
        .map_err(|error| format!("Cannot protect control socket: {error}"))?;
    let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
    if !metadata.file_type().is_socket()
        || metadata.file_type().is_symlink()
        || metadata.uid() != uid
        || metadata.mode() & 0o777 != 0o600
    {
        let _ = fs::remove_file(&path);
        return Err("Control socket ownership or permissions are invalid".into());
    }
    let identity = (metadata.dev(), metadata.ino());
    let (stop, stopped) = oneshot::channel();
    let cleanup_path = path.clone();
    let task = tokio::spawn(async move {
        accept_loop(listener, uid, dispatch, stopped).await;
        if fs::symlink_metadata(&cleanup_path).is_ok_and(|metadata| {
            metadata.file_type().is_socket()
                && metadata.uid() == uid
                && (metadata.dev(), metadata.ino()) == identity
        }) {
            let _ = fs::remove_file(cleanup_path);
        }
    });
    Ok(ControlSocket {
        stop: Some(stop),
        task,
    })
}

fn ensure_control_directory(directory: &Path, uid: u32) -> Result<(), String> {
    match fs::symlink_metadata(directory) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err("Control directory must be a real directory".into());
        }
        Ok(metadata) if metadata.uid() != uid => {
            return Err("Control directory is not owned by this user".into());
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir(directory)
                .map_err(|error| format!("Cannot create control directory: {error}"))?;
        }
        Err(error) => return Err(format!("Cannot inspect control directory: {error}")),
    }
    fs::set_permissions(directory, fs::Permissions::from_mode(0o700))
        .map_err(|error| format!("Cannot protect control directory: {error}"))?;
    let metadata = fs::symlink_metadata(directory).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink()
        || !metadata.is_dir()
        || metadata.uid() != uid
        || metadata.mode() & 0o777 != 0o700
    {
        return Err("Control directory ownership or permissions are invalid".into());
    }
    Ok(())
}

async fn accept_loop(
    listener: UnixListener,
    expected_uid: u32,
    dispatch: Dispatch,
    mut stopped: oneshot::Receiver<()>,
) {
    let slots = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    let mut connections = JoinSet::new();
    loop {
        while connections.try_join_next().is_some() {}
        let accepted = tokio::select! {
            _ = &mut stopped => break,
            accepted = listener.accept() => accepted,
        };
        let Ok((stream, _)) = accepted else { break };
        let Ok(permit) = slots.clone().try_acquire_owned() else {
            continue;
        };
        let dispatch = dispatch.clone();
        connections.spawn(async move {
            let _permit = permit;
            if let Ok(uid) = peer_uid(&stream) {
                if uid == expected_uid {
                    handle_connection(stream, dispatch).await;
                    return;
                }
            }
            // Close without reflecting details to an unauthorized peer.
        });
    }
    while connections.join_next().await.is_some() {}
}

fn peer_uid(stream: &UnixStream) -> io::Result<u32> {
    stream.peer_cred().map(|credentials| credentials.uid())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::UnixListener,
        runtime::Builder,
    };

    fn runtime() -> tokio::runtime::Runtime {
        Builder::new_current_thread().enable_all().build().unwrap()
    }

    #[test]
    fn private_socket_dispatch_and_rejections() {
        runtime().block_on(async {
            let root = std::env::temp_dir().join(format!("mas-control-{}", std::process::id()));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root).unwrap();
            let socket_path = root.join("socket");
            let called = Arc::new(AtomicBool::new(false));
            let listener = UnixListener::bind(&socket_path).unwrap();
            fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600)).unwrap();
            for (message, should_call, expected) in [
                (
                    br#"{"command":"list_sessions","args":{}}"#.to_vec(),
                    true,
                    "\"ok\":true",
                ),
                (
                    br#"{"command":"send_replay","args":{}}"#.to_vec(),
                    false,
                    "control_command_forbidden",
                ),
                (b"not-json".to_vec(), false, "control_request_invalid"),
                (
                    vec![b'x'; MAX_REQUEST + 1],
                    false,
                    "control_request_too_large",
                ),
            ] {
                let mut client = UnixStream::connect(&socket_path).await.unwrap();
                let server = listener.accept().await.unwrap().0;
                let called = called.clone();
                let dispatch_called = called.clone();
                let dispatch: Dispatch = Arc::new(move |_, _| {
                    dispatch_called.store(true, Ordering::SeqCst);
                    Box::pin(async { Ok(json!({"ok":true})) })
                });
                let task = tokio::spawn(handle_connection(server, dispatch));
                client.writable().await.unwrap();
                client.write_all(&message).await.unwrap();
                client.shutdown().await.unwrap();
                let mut response = Vec::new();
                client.read_to_end(&mut response).await.unwrap();
                task.await.unwrap();
                let body = String::from_utf8(response).unwrap();
                assert!(body.contains(expected), "{body}");
                assert_eq!(called.load(Ordering::SeqCst), should_call);
                called.store(false, Ordering::SeqCst);
            }
            drop(listener);
            fs::remove_dir_all(root).unwrap();
        });
    }

    #[test]
    fn socket_path_is_private_owned_and_rejects_symlinks() {
        runtime().block_on(async {
            let root =
                std::env::temp_dir().join(format!("mas-control-perms-{}", std::process::id()));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir(&root).unwrap();
            let path = root.join("control/socket");
            let dispatch: Dispatch = Arc::new(|_, _| Box::pin(async { Ok(Value::Null) }));
            let socket = start_with(path.clone(), dispatch.clone()).await.unwrap();
            assert_eq!(
                fs::symlink_metadata(path.parent().unwrap()).unwrap().mode() & 0o777,
                0o700
            );
            assert_eq!(fs::symlink_metadata(&path).unwrap().mode() & 0o777, 0o600);
            assert_eq!(
                fs::symlink_metadata(&path).unwrap().uid(),
                fs::symlink_metadata(path.parent().unwrap()).unwrap().uid()
            );
            socket.shutdown().await;
            assert!(!path.exists());
            let stale_listener = UnixListener::bind(&path).unwrap();
            drop(stale_listener);
            let socket = start_with(path.clone(), dispatch.clone()).await.unwrap();
            socket.shutdown().await;
            fs::remove_dir(path.parent().unwrap()).unwrap();
            std::os::unix::fs::symlink(root.join("elsewhere"), path.parent().unwrap()).unwrap();
            assert!(start_with(path.clone(), dispatch).await.is_err());
            fs::remove_file(path.parent().unwrap()).unwrap();
            fs::remove_dir_all(root).unwrap();
        });
    }

    #[test]
    fn shutdown_waits_for_active_connection_tasks() {
        runtime().block_on(async {
            let root =
                std::env::temp_dir().join(format!("mas-control-shutdown-{}", std::process::id()));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir(&root).unwrap();
            let path = root.join("control/socket");
            let entered = Arc::new(tokio::sync::Notify::new());
            let release = Arc::new(tokio::sync::Notify::new());
            let dispatch: Dispatch = {
                let entered = entered.clone();
                let release = release.clone();
                Arc::new(move |_, _| {
                    let entered = entered.clone();
                    let release = release.clone();
                    Box::pin(async move {
                        entered.notify_one();
                        release.notified().await;
                        Ok(json!({"ok":true}))
                    })
                })
            };
            let socket = start_with(path.clone(), dispatch).await.unwrap();
            let mut client = UnixStream::connect(&path).await.unwrap();
            let entered_wait = entered.notified();
            client
                .write_all(br#"{"command":"health","args":{}}"#)
                .await
                .unwrap();
            client.shutdown().await.unwrap();
            entered_wait.await;

            let shutdown = tokio::spawn(socket.shutdown());
            tokio::task::yield_now().await;
            assert!(!shutdown.is_finished());
            release.notify_one();
            shutdown.await.unwrap();
            let mut response = Vec::new();
            client.read_to_end(&mut response).await.unwrap();
            assert_eq!(
                serde_json::from_slice::<Value>(&response).unwrap(),
                json!({"ok":true})
            );
            assert!(!path.exists());
            fs::remove_dir_all(root).unwrap();
        });
    }

    #[test]
    fn live_socket_is_never_replaced_and_peer_uid_is_kernel_reported() {
        runtime().block_on(async {
            let root =
                std::env::temp_dir().join(format!("mas-control-live-{}", std::process::id()));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir(&root).unwrap();
            let path = root.join("socket");
            let listener = UnixListener::bind(&path).unwrap();
            let identity = fs::symlink_metadata(&path).unwrap().ino();
            let dispatch: Dispatch = Arc::new(|_, _| Box::pin(async { Ok(Value::Null) }));
            assert!(start_with(path.clone(), dispatch).await.is_err());
            assert_eq!(fs::symlink_metadata(&path).unwrap().ino(), identity);
            let (probe, _) = listener.accept().await.unwrap();
            drop(probe);

            let client = UnixStream::connect(&path).await.unwrap();
            let (server, _) = listener.accept().await.unwrap();
            assert_eq!(peer_uid(&server).unwrap(), unsafe { libc::geteuid() });
            drop((client, server, listener));
            fs::remove_dir_all(root).unwrap();
        });
    }
}
