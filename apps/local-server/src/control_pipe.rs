use crate::control_protocol::{Dispatch, MAX_CONNECTIONS, handle_connection};
use std::{
    ffi::{OsStr, OsString},
    io,
    os::windows::{ffi::OsStrExt, io::AsRawHandle},
    ptr,
    sync::Arc,
};
use tokio::{
    net::windows::named_pipe::NamedPipeServer,
    sync::{Semaphore, oneshot},
    task::{JoinHandle, JoinSet},
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE, LocalFree},
    Security::{
        Authorization::{
            ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
        },
        GetTokenInformation, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER, TokenUser,
    },
    Storage::FileSystem::{
        FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, PIPE_ACCESS_DUPLEX,
    },
    System::{
        Pipes::{
            CreateNamedPipeW, GetNamedPipeClientProcessId, PIPE_READMODE_BYTE,
            PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
        },
        Threading::{
            GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
        },
    },
};

const PIPE_NAME_PREFIX: &str = r"\\.\pipe\mobile-api-studio-control-";
const SECURITY_DESCRIPTOR_REVISION: u32 = 1;

pub struct ControlPipe {
    stop: Option<oneshot::Sender<()>>,
    task: JoinHandle<()>,
}

impl ControlPipe {
    pub async fn shutdown(mut self) {
        self.stop.take();
        let _ = (&mut self.task).await;
    }
}

pub async fn start(dispatch: Dispatch) -> Result<ControlPipe, String> {
    let sid = current_user_sid()?;
    let name = pipe_name(&sid);
    let server = create_pipe(&name, &sid, true)?;
    let (stop, stopped) = oneshot::channel();
    let task = tokio::spawn(async move { accept_loop(server, name, sid, dispatch, stopped).await });
    Ok(ControlPipe {
        stop: Some(stop),
        task,
    })
}

async fn accept_loop(
    mut server: NamedPipeServer,
    name: OsString,
    sid: String,
    dispatch: Dispatch,
    mut stopped: oneshot::Receiver<()>,
) {
    let slots = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    let mut connections = JoinSet::new();
    loop {
        while connections.try_join_next().is_some() {}
        let connected = tokio::select! {
            _ = &mut stopped => break,
            result = server.connect() => result,
        };
        if connected.is_err() {
            break;
        }
        let next = create_pipe(&name, &sid, false);
        let Ok(permit) = slots.clone().try_acquire_owned() else {
            drop(server);
            if let Ok(next) = next {
                server = next;
                continue;
            }
            break;
        };
        let dispatch = dispatch.clone();
        let sid = sid.clone();
        connections.spawn(async move {
            let _permit = permit;
            if peer_matches_user(&server, &sid) {
                handle_connection(server, dispatch).await;
            }
        });
        match next {
            Ok(next) => server = next,
            Err(_) => break,
        }
    }
    while connections.join_next().await.is_some() {}
}

fn pipe_name(sid: &str) -> OsString {
    OsString::from(format!("{PIPE_NAME_PREFIX}{sid}"))
}

fn create_pipe(name: &OsStr, sid: &str, first: bool) -> Result<NamedPipeServer, String> {
    let descriptor = security_descriptor(sid)?;
    let security = SecurityDescriptor(descriptor);
    let mut attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: security.0.cast(),
        bInheritHandle: 0,
    };
    let name: Vec<u16> = name.encode_wide().chain([0]).collect();
    let open_mode = PIPE_ACCESS_DUPLEX
        | FILE_FLAG_OVERLAPPED
        | if first {
            FILE_FLAG_FIRST_PIPE_INSTANCE
        } else {
            0
        };
    let handle = unsafe {
        CreateNamedPipeW(
            name.as_ptr(),
            open_mode,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            PIPE_UNLIMITED_INSTANCES,
            64 * 1024,
            64 * 1024,
            0,
            &mut attributes,
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(format!(
            "Cannot create the current-user control pipe: {}",
            io::Error::last_os_error()
        ));
    }
    unsafe { NamedPipeServer::from_raw_handle(handle.cast()) }
        .map_err(|error| format!("Cannot initialize the control pipe: {error}"))
}

fn peer_matches_user(pipe: &NamedPipeServer, expected_sid: &str) -> bool {
    let mut process_id = 0;
    if unsafe { GetNamedPipeClientProcessId(pipe.as_raw_handle().cast(), &mut process_id) } == 0 {
        return false;
    }
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
    if process.is_null() {
        return false;
    }
    let process = OwnedHandle(process);
    let mut token = ptr::null_mut();
    if unsafe { OpenProcessToken(process.0, TOKEN_QUERY, &mut token) } == 0 {
        return false;
    }
    let token = OwnedHandle(token);
    token_sid(token.0).is_ok_and(|sid| sid == expected_sid)
}

fn current_user_sid() -> Result<String, String> {
    let mut token = ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(format!(
            "Cannot read the current Windows user: {}",
            io::Error::last_os_error()
        ));
    }
    let token = OwnedHandle(token);
    token_sid(token.0)
}

fn token_sid(token: HANDLE) -> Result<String, String> {
    let mut length = 0;
    unsafe { GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut length) };
    if length == 0 {
        return Err(format!(
            "Cannot size the Windows user token: {}",
            io::Error::last_os_error()
        ));
    }
    let mut buffer = vec![0_usize; (length as usize).div_ceil(std::mem::size_of::<usize>())];
    if unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            length,
            &mut length,
        )
    } == 0
    {
        return Err(format!(
            "Cannot read the Windows user token: {}",
            io::Error::last_os_error()
        ));
    }
    let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
    let mut string_sid = ptr::null_mut();
    if unsafe { ConvertSidToStringSidW(user.User.Sid, &mut string_sid) } == 0 {
        return Err(format!(
            "Cannot read the Windows user SID: {}",
            io::Error::last_os_error()
        ));
    }
    let string_sid = LocalString(string_sid);
    let mut length = 0;
    while unsafe { *string_sid.0.add(length) } != 0 {
        length += 1;
    }
    let wide = unsafe { std::slice::from_raw_parts(string_sid.0, length) };
    String::from_utf16(wide).map_err(|error| format!("Invalid Windows user SID: {error}"))
}

fn security_descriptor(sid: &str) -> Result<*mut std::ffi::c_void, String> {
    let sddl: Vec<u16> = OsStr::new(&format!("D:P(A;;GA;;;{sid})"))
        .encode_wide()
        .chain([0])
        .collect();
    let mut descriptor = ptr::null_mut();
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SECURITY_DESCRIPTOR_REVISION,
            &mut descriptor,
            ptr::null_mut(),
        )
    } == 0
    {
        return Err(format!(
            "Cannot protect the control pipe: {}",
            io::Error::last_os_error()
        ));
    }
    Ok(descriptor)
}

struct OwnedHandle(HANDLE);
impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

struct LocalString(*mut u16);
impl Drop for LocalString {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0.cast());
        }
    }
}

struct SecurityDescriptor(*mut std::ffi::c_void);
impl Drop for SecurityDescriptor {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0);
        }
    }
}
