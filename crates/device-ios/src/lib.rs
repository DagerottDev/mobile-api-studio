use core_model::{Device, DeviceCapabilities, DevicePlatform, SCHEMA_VERSION};
use serde_json::Value;
use std::{path::Path, process::Command};

#[derive(Debug, Clone)]
pub struct IosDeviceProvider;

impl IosDeviceProvider {
    pub fn is_available(&self) -> bool {
        Command::new("xcrun")
            .args(["--find", "simctl"])
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    pub fn list_devices(&self) -> Result<Vec<Device>, DeviceError> {
        let output = Command::new("xcrun")
            .args(["simctl", "list", "--json", "devices", "available"])
            .output()
            .map_err(|error| DeviceError::tool_unavailable("xcrun", error.to_string()))?;

        if !output.status.success() {
            return Err(DeviceError::command_failed(
                "simctl_list_failed",
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ));
        }

        let root: Value = serde_json::from_slice(&output.stdout)
            .map_err(|error| DeviceError::parse_failed("simctl_json_invalid", error.to_string()))?;

        let runtimes = root
            .get("devices")
            .and_then(Value::as_object)
            .ok_or_else(|| DeviceError::parse_failed("simctl_devices_missing", "devices object missing"))?;

        let mut devices = Vec::new();

        for (runtime, runtime_devices) in runtimes {
            if !runtime.contains(".iOS-") {
                continue;
            }

            let Some(runtime_devices) = runtime_devices.as_array() else {
                continue;
            };

            for value in runtime_devices {
                let Some(udid) = value.get("udid").and_then(Value::as_str) else {
                    continue;
                };
                let Some(name) = value.get("name").and_then(Value::as_str) else {
                    continue;
                };

                let state = value
                    .get("state")
                    .and_then(Value::as_str)
                    .unwrap_or("Unknown");

                devices.push(Device {
                    schema_version: SCHEMA_VERSION,
                    id: format!("ios:{udid}"),
                    platform: DevicePlatform::Ios,
                    name: name.to_string(),
                    os_version: parse_ios_runtime_version(runtime),
                    state: state.to_string(),
                    capabilities: DeviceCapabilities {
                        can_install_ca: true,
                        can_auto_route_proxy: false,
                        can_target_process: false,
                    },
                });
            }
        }

        devices.sort_by(|left, right| {
            let left_booted = left.state.eq_ignore_ascii_case("booted");
            let right_booted = right.state.eq_ignore_ascii_case("booted");
            right_booted
                .cmp(&left_booted)
                .then_with(|| left.name.cmp(&right.name))
        });

        Ok(devices)
    }

    pub fn install_root_ca(&self, device_id: &str, certificate: &Path) -> Result<(), DeviceError> {
        let udid = device_id.strip_prefix("ios:").unwrap_or(device_id);
        let certificate = certificate
            .to_str()
            .ok_or_else(|| DeviceError::command_failed("invalid_certificate_path", "certificate path is not valid UTF-8"))?;

        let output = Command::new("xcrun")
            .args(["simctl", "keychain", udid, "add-root-cert", certificate])
            .output()
            .map_err(|error| DeviceError::tool_unavailable("xcrun", error.to_string()))?;

        if output.status.success() {
            Ok(())
        } else {
            Err(DeviceError::command_failed(
                "simctl_ca_install_failed",
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ))
        }
    }
}

fn parse_ios_runtime_version(runtime: &str) -> Option<String> {
    let marker = ".iOS-";
    let start = runtime.find(marker)? + marker.len();
    let version = runtime[start..].replace('-', ".");
    (!version.is_empty()).then_some(version)
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
            code: "ios_tool_unavailable".into(),
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

    fn parse_failed(code: &str, detail: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: detail.into(),
            recoverable: true,
        }
    }
}
