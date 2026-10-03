use std::{
    fs::{self, File},
    path::{Path, PathBuf},
    process::Command,
};

#[cfg(unix)]
use std::{
    ffi::CString,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::MetadataExt,
    },
};
#[cfg(windows)]
use std::{
    ffi::c_void,
    fs::OpenOptions,
    os::windows::{fs::OpenOptionsExt, io::AsRawHandle},
};

pub fn default_data_dir() -> Result<PathBuf, String> {
    #[cfg(target_os = "macos")]
    let path = home_dir()?.join("Library/Application Support/dev.mobileapistudio.desktop");
    #[cfg(target_os = "windows")]
    let path = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .ok_or("LOCALAPPDATA is required")?
        .join("dev.mobileapistudio.desktop");
    #[cfg(target_os = "linux")]
    let path = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| home_dir().ok().map(|home| home.join(".local/share")))
        .ok_or("XDG_DATA_HOME or HOME is required")?
        .join("dev.mobileapistudio.desktop");
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    let path = return Err("The local service is supported on macOS, Windows, and Linux.".into());
    Ok(path)
}

fn home_dir() -> Result<PathBuf, String> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| "HOME is required".into())
}

#[cfg(unix)]
pub struct DataLock {
    _file: File,
}
#[cfg(windows)]
pub struct DataLock {
    _file: File,
    _overlapped: Overlapped,
}

pub fn lock_data_dir(data_dir: &Path) -> Result<DataLock, String> {
    fs::create_dir_all(data_dir).map_err(|error| error.to_string())?;
    #[cfg(unix)]
    validate_unix_directory(data_dir)?;
    #[cfg(windows)]
    validate_windows_directory(data_dir)?;
    #[cfg(target_os = "macos")]
    check_legacy_macos_process(data_dir)?;

    let file = open_lock_file(data_dir)?;
    #[cfg(unix)]
    lock_file(&file, data_dir)?;
    #[cfg(unix)]
    return Ok(DataLock { _file: file });
    #[cfg(windows)]
    {
        let overlapped = lock_file(&file, data_dir)?;
        return Ok(DataLock {
            _file: file,
            _overlapped: overlapped,
        });
    }
}

#[cfg(unix)]
fn validate_unix_directory(data_dir: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(data_dir).map_err(|error| {
        format!(
            "Cannot inspect data directory {}: {error}",
            data_dir.display()
        )
    })?;
    if metadata.file_type().is_symlink()
        || !metadata.is_dir()
        || metadata.uid() != unsafe { libc::getuid() }
        || metadata.mode() & 0o022 != 0
    {
        return Err(format!(
            "Data directory {} must be a non-symlink directory owned by this user and not writable by group or others.",
            data_dir.display()
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn open_lock_file(data_dir: &Path) -> Result<File, String> {
    use std::os::unix::ffi::OsStrExt;

    let dir = CString::new(data_dir.as_os_str().as_bytes())
        .map_err(|_| "Data directory contains an invalid NUL byte")?;
    let directory_fd = unsafe {
        libc::open(
            dir.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    if directory_fd < 0 {
        return Err(format!(
            "Cannot securely open data directory {}: {}",
            data_dir.display(),
            std::io::Error::last_os_error()
        ));
    }
    let directory = unsafe { File::from_raw_fd(directory_fd) };
    let lock_name = c"local-server.lock";
    let lock_fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            lock_name.as_ptr(),
            libc::O_CREAT | libc::O_RDWR | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            0o600,
        )
    };
    if lock_fd < 0 {
        return Err(format!(
            "Cannot securely open service lock in {}: {}",
            data_dir.display(),
            std::io::Error::last_os_error()
        ));
    }
    let file = unsafe { File::from_raw_fd(lock_fd) };
    let metadata = file
        .metadata()
        .map_err(|error| format!("Cannot inspect service lock: {error}"))?;
    if !metadata.is_file() || metadata.uid() != unsafe { libc::getuid() } || metadata.nlink() != 1 {
        return Err(format!(
            "Service lock in {} must be a regular file owned by this user with one link.",
            data_dir.display()
        ));
    }
    if unsafe { libc::fchmod(file.as_raw_fd(), 0o600) } != 0 {
        return Err(format!(
            "Cannot restrict service lock permissions: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(file)
}

#[cfg(windows)]
fn validate_windows_directory(data_dir: &Path) -> Result<(), String> {
    let local_app_data = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .ok_or("LOCALAPPDATA is required to validate Windows data-directory permissions")?;
    let trusted_root = fs::canonicalize(&local_app_data)
        .map_err(|error| format!("Cannot validate the per-user LOCALAPPDATA directory: {error}"))?;
    let metadata = fs::symlink_metadata(data_dir).map_err(|error| {
        format!(
            "Cannot inspect data directory {}: {error}",
            data_dir.display()
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(format!(
            "Data directory {} must be a real directory, not a reparse point.",
            data_dir.display()
        ));
    }
    let canonical = fs::canonicalize(data_dir).map_err(|error| {
        format!(
            "Cannot resolve data directory {}: {error}",
            data_dir.display()
        )
    })?;
    if !canonical.starts_with(&trusted_root) {
        return Err(format!(
            "Windows data directory {} must remain inside LOCALAPPDATA so it inherits per-user access controls.",
            data_dir.display()
        ));
    }
    Ok(())
}

#[cfg(windows)]
fn open_lock_file(data_dir: &Path) -> Result<File, String> {
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    let path = data_dir.join("local-server.lock");
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(&path)
        .map_err(|error| {
            format!(
                "Cannot securely open service lock in {}: {error}",
                data_dir.display()
            )
        })?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("Cannot inspect service lock: {error}"))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(format!(
            "Service lock in {} must be a regular file, not a reparse point.",
            data_dir.display()
        ));
    }
    Ok(file)
}

#[cfg(unix)]
fn lock_file(file: &File, data_dir: &Path) -> Result<(), String> {
    // flock releases the lock when the process exits, including after a crash.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
        Ok(())
    } else {
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::WouldBlock {
            Err(format!(
                "Another local service is using {}.",
                data_dir.display()
            ))
        } else {
            Err(format!("Cannot lock {}: {error}", data_dir.display()))
        }
    }
}

#[cfg(windows)]
#[repr(C)]
struct Overlapped {
    internal: usize,
    internal_high: usize,
    offset: u32,
    offset_high: u32,
    event: *mut c_void,
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn LockFileEx(
        file: *mut c_void,
        flags: u32,
        reserved: u32,
        low: u32,
        high: u32,
        overlapped: *mut Overlapped,
    ) -> i32;
}

#[cfg(windows)]
fn lock_file(file: &File, data_dir: &Path) -> Result<Overlapped, String> {
    let mut overlapped = Overlapped {
        internal: 0,
        internal_high: 0,
        offset: 0,
        offset_high: 0,
        event: std::ptr::null_mut(),
    };
    let ok = unsafe { LockFileEx(file.as_raw_handle().cast(), 3, 0, 1, 0, &mut overlapped) };
    if ok != 0 {
        Ok(overlapped)
    } else {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(33) {
            Err(format!(
                "Another local service is using {}.",
                data_dir.display()
            ))
        } else {
            Err(format!("Cannot lock {}: {error}", data_dir.display()))
        }
    }
}

#[cfg(target_os = "macos")]
fn check_legacy_macos_process(data_dir: &Path) -> Result<(), String> {
    for process_name in ["mobile-api-studio", "Mobile API Studio"] {
        match Command::new("pgrep").arg("-x").arg(process_name).output() {
            Ok(result) if result.status.success() => {
                return Err(
                    "Close the historical Mobile API Studio app before starting the local service."
                        .into(),
                );
            }
            Ok(result) if result.status.code() == Some(1) => {}
            Ok(result) => {
                return Err(format!(
                    "Cannot check whether the old app is running (pgrep status {}).",
                    result.status
                ));
            }
            Err(error) => {
                return Err(format!(
                    "Cannot check whether the old app is running: {error}"
                ));
            }
        }
    }
    let database = data_dir.join("app.db");
    if database.exists() {
        let output = Command::new("lsof")
            .arg("-t")
            .arg(&database)
            .output()
            .map_err(|error| {
                format!(
                    "Cannot check whether {} is in use: {error}",
                    database.display()
                )
            })?;
        if output.status.success() && !output.stdout.is_empty() {
            return Err(format!(
                "{} is in use. Close the other process before starting the local service.",
                database.display()
            ));
        }
        if !output.status.success() && output.status.code() != Some(1) {
            return Err(format!(
                "Cannot check whether {} is in use (lsof status {}).",
                database.display(),
                output.status
            ));
        }
    }
    Ok(())
}

pub fn open_browser(url: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let result = Command::new("open").arg(url).spawn();
    #[cfg(target_os = "linux")]
    let result = Command::new("xdg-open").arg(url).spawn();
    #[cfg(target_os = "windows")]
    let result = Command::new("rundll32.exe")
        .arg("url.dll,FileProtocolHandler")
        .arg(url)
        .spawn();
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    let result: Result<std::process::Child, std::io::Error> = Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "unsupported platform",
    ));
    result
        .map(|_| ())
        .map_err(|error| format!("Cannot open the local browser: {error}"))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{os::unix::fs::symlink, time::SystemTime};

    fn temporary_dir() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("mas-platform-{}-{nonce}", std::process::id()));
        fs::create_dir(&path).unwrap();
        path
    }

    #[test]
    fn process_lock_is_exclusive_and_released_on_drop() {
        let directory = temporary_dir();
        let lock = lock_data_dir(&directory).unwrap();
        assert!(lock_data_dir(&directory).is_err());
        drop(lock);
        assert!(lock_data_dir(&directory).is_ok());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn process_lock_rejects_symlink_paths() {
        let directory = temporary_dir();
        let target = directory.join("target");
        let linked = directory.join("linked");
        fs::create_dir(&target).unwrap();
        symlink(&target, &linked).unwrap();
        assert!(lock_data_dir(&linked).is_err());

        let lock_directory = directory.join("lock");
        fs::create_dir(&lock_directory).unwrap();
        let other = directory.join("other");
        fs::write(&other, b"").unwrap();
        symlink(&other, lock_directory.join("local-server.lock")).unwrap();
        assert!(lock_data_dir(&lock_directory).is_err());
        fs::remove_dir_all(directory).unwrap();
    }
}
