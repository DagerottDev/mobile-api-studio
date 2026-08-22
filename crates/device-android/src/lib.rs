use core_model::{Device, DeviceCapabilities, DevicePlatform, SCHEMA_VERSION};
use std::process::Command;

#[derive(Debug, Clone)]
pub struct AndroidDeviceProvider;

impl AndroidDeviceProvider {
    pub fn is_available(&self) -> bool {
        Command::new("adb")
            .arg("version")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    pub fn list_devices(&self) -> Result<Vec<Device>, DeviceError> {
        let output = Command::new("adb")
            .args(["devices", "-l"])
            .output()
            .map_err(|error| DeviceError::tool_unavailable("adb", error.to_string()))?;

        if !output.status.success() {
            return Err(DeviceError::command_failed(
                "adb_devices_failed",
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut devices = Vec::new();

        for line in stdout.lines().skip(1) {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            let mut parts = line.split_whitespace();
            let Some(serial) = parts.next() else {
                continue;
            };
            let state = parts.next().unwrap_or("unknown");

            if !serial.starts_with("emulator-") {
                continue;
            }

            let attributes: Vec<&str> = parts.collect();
            let model = attribute(&attributes, "model")
                .map(|value| value.replace('_', " "))
                .unwrap_or_else(|| serial.to_string());

            let os_version = if state == "device" {
                let release = self.get_prop(serial, "ro.build.version.release").ok();
                let api_level = self.get_prop(serial, "ro.build.version.sdk").ok();
                format_android_version(release, api_level)
            } else {
                None
            };

            devices.push(Device {
                schema_version: SCHEMA_VERSION,
                id: format!("android:{serial}"),
                platform: DevicePlatform::Android,
                name: model,
                os_version,
                state: state.to_string(),
                capabilities: DeviceCapabilities {
                    can_install_ca: false,
                    can_auto_route_proxy: state == "device",
                    can_target_process: false,
                },
            });
        }

        devices.sort_by(|left, right| {
            let left_online = left.state == "device";
            let right_online = right.state == "device";
            right_online
                .cmp(&left_online)
                .then_with(|| left.name.cmp(&right.name))
        });

        Ok(devices)
    }

    pub fn get_http_proxy(&self, device_id: &str) -> Result<Option<String>, DeviceError> {
        let serial = normalize_serial(device_id);
        let value = self.shell(serial, &["settings", "get", "global", "http_proxy"])?;
        let value = value.trim();

        if value.is_empty() || value == "null" || value == ":0" {
            Ok(None)
        } else {
            Ok(Some(value.to_string()))
        }
    }

    pub fn set_http_proxy(&self, device_id: &str, host: &str, port: u16) -> Result<(), DeviceError> {
        let serial = normalize_serial(device_id);
        let proxy = format!("{host}:{port}");
        self.shell(serial, &["settings", "put", "global", "http_proxy", &proxy])?;
        Ok(())
    }

    pub fn clear_http_proxy(&self, device_id: &str) -> Result<(), DeviceError> {
        let serial = normalize_serial(device_id);
        self.shell(serial, &["settings", "delete", "global", "http_proxy"])?;
        Ok(())
    }

    fn get_prop(&self, serial: &str, key: &str) -> Result<String, DeviceError> {
        self.shell(serial, &["getprop", key])
    }

    fn shell(&self, serial: &str, args: &[&str]) -> Result<String, DeviceError> {
        let mut command = Command::new("adb");
        command.arg("-s").arg(serial).arg("shell");
        command.args(args);

        let output = command
            .output()
            .map_err(|error| DeviceError::tool_unavailable("adb", error.to_string()))?;

        if !output.status.success() {
            return Err(DeviceError::command_failed(
                "adb_shell_failed",
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ));
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
}

fn normalize_serial(device_id: &str) -> &str {
    device_id.strip_prefix("android:").unwrap_or(device_id)
}

fn attribute<'a>(attributes: &'a [&str], key: &str) -> Option<&'a str> {
    let prefix = format!("{key}:");
    attributes
        .iter()
        .find_map(|value| value.strip_prefix(&prefix))
}

fn format_android_version(release: Option<String>, api_level: Option<String>) -> Option<String> {
    match (release.filter(|value| !value.is_empty()), api_level.filter(|value| !value.is_empty())) {
        (Some(release), Some(api)) => Some(format!("{release} (API {api})")),
        (Some(release), None) => Some(release),
        (None, Some(api)) => Some(format!("API {api}")),
        (None, None) => None,
    }
}

#[derive(Debug, Clone)]
pub struct DeviceError {
    pub code: String,
    pub message: String,
    pub recoverable: bool,
}

impl DeviceError {
    fn tool_unavailable(tool: &str, detail: String) -> Self {
        Self {
            code: "android_tool_unavailable".into(),
            message: format!("{tool} is unavailable: {detail}"),
            recoverable: true,
        }
    }

    fn command_failed(code: &str, detail: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: detail.into(),
            recoverable: true,
        }
    }
}
