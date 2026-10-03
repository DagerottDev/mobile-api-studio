use core_model::AppError;
#[cfg(any(windows, target_os = "linux", test))]
use serde_json::Value;
use std::{net::Ipv4Addr, process::Command};

const MAX_OUTPUT: usize = 2 * 1024 * 1024;
const MAX_ITEMS: usize = 1_024;
const MAX_NAME: usize = 256;

fn failed(code: &str, message: impl Into<String>) -> AppError {
    AppError::new(code, message, true)
}

fn run(program: &str, args: &[&str], code: &str) -> Result<Vec<u8>, AppError> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|error| failed(code, format!("Discovery command failed: {error}")))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        return Err(failed(
            code,
            format!(
                "Discovery command failed: {}",
                detail.chars().take(512).collect::<String>()
            ),
        ));
    }
    if output.stdout.len() > MAX_OUTPUT {
        return Err(failed(
            code,
            "Discovery output exceeded the supported size.",
        ));
    }
    Ok(output.stdout)
}

fn command_name(value: &str) -> Option<String> {
    let name = std::path::Path::new(value.trim())
        .file_name()?
        .to_str()?
        .trim();
    (!name.is_empty() && name.len() <= MAX_NAME && !name.chars().any(char::is_control))
        .then(|| name.to_owned())
}

#[cfg(any(target_os = "macos", target_os = "linux", test))]
fn parse_processes(text: &str) -> Vec<super::MacProcess> {
    text.lines()
        .take(MAX_ITEMS)
        .filter_map(|line| {
            let mut parts = line.trim().splitn(2, char::is_whitespace);
            let pid = parts.next()?.parse::<u32>().ok()?;
            let name = command_name(parts.next()?)?;
            (pid > 0).then_some(super::MacProcess { pid, name })
        })
        .collect()
}

#[cfg(any(windows, test))]
fn parse_windows_processes(value: &Value) -> Vec<super::MacProcess> {
    values(value)
        .into_iter()
        .take(MAX_ITEMS)
        .filter_map(|item| {
            let pid = item.get("Id")?.as_u64()?.try_into().ok()?;
            let name = command_name(item.get("ProcessName")?.as_str()?)?;
            (pid > 0).then_some(super::MacProcess { pid, name })
        })
        .collect()
}

fn private_address(value: &str) -> Option<String> {
    let ip = value.parse::<Ipv4Addr>().ok()?;
    ip.is_private().then(|| ip.to_string())
}

#[cfg(any(target_os = "linux", test))]
fn parse_linux_interfaces(value: &Value) -> Vec<super::LanInterface> {
    let mut result = Vec::new();
    for interface in values(value).into_iter().take(MAX_ITEMS) {
        let Some(name) = interface
            .get("ifname")
            .and_then(Value::as_str)
            .filter(|name| {
                !name.is_empty() && name.len() <= MAX_NAME && !name.chars().any(char::is_control)
            })
        else {
            continue;
        };
        let Some(addresses) = interface.get("addr_info").and_then(Value::as_array) else {
            continue;
        };
        for info in addresses.iter().take(16) {
            if info.get("family").and_then(Value::as_str) != Some("inet") {
                continue;
            }
            let Some(address) = info
                .get("local")
                .and_then(Value::as_str)
                .and_then(private_address)
            else {
                continue;
            };
            result.push(super::LanInterface {
                name: name.to_owned(),
                address,
            });
            if result.len() >= MAX_ITEMS {
                return result;
            }
        }
    }
    result
}

#[cfg(any(windows, target_os = "linux", test))]
fn values(value: &Value) -> Vec<&Value> {
    match value {
        Value::Array(items) => items.iter().collect(),
        Value::Object(_) => vec![value],
        _ => Vec::new(),
    }
}

#[cfg(windows)]
fn powershell(script: &str, code: &str) -> Result<Value, AppError> {
    let script = format!("[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new(); {script}");
    let bytes = run(
        "powershell.exe",
        &["-NoProfile", "-NonInteractive", "-Command", &script],
        code,
    )?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| failed(code, "Discovery returned invalid UTF-8."))?;
    serde_json::from_str(text).map_err(|_| failed(code, "Discovery returned invalid JSON."))
}

pub(super) fn processes() -> Result<Vec<super::MacProcess>, AppError> {
    #[cfg(windows)]
    {
        let value = powershell(
            "Get-Process | Select-Object -Property Id,ProcessName | ConvertTo-Json -Compress",
            "process_discovery_failed",
        )?;
        return Ok(parse_windows_processes(&value));
    }
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        let bytes = run(
            "ps",
            &["-x", "-o", "pid=", "-o", "comm="],
            "process_discovery_failed",
        )?;
        let text = std::str::from_utf8(&bytes).map_err(|_| {
            failed(
                "process_discovery_failed",
                "Process list was not valid UTF-8.",
            )
        })?;
        return Ok(parse_processes(text));
    }
    #[allow(unreachable_code)]
    Ok(Vec::new())
}

#[cfg(target_os = "macos")]
fn parse_macos_interfaces(names: &str) -> Result<Vec<super::LanInterface>, AppError> {
    let mut result = Vec::new();
    for name in names
        .split_whitespace()
        .filter(|name| *name != "lo0")
        .take(128)
    {
        if name.len() > MAX_NAME || name.chars().any(char::is_control) {
            continue;
        }
        let output = Command::new("ipconfig")
            .args(["getifaddr", name])
            .output()
            .map_err(|error| {
                failed(
                    "interface_discovery_failed",
                    format!("Interface discovery failed: {error}"),
                )
            })?;
        // Interfaces without an IPv4 address are normal (for example bridges and tunnels).
        if !output.status.success() {
            continue;
        }
        if output.stdout.len() > MAX_OUTPUT {
            return Err(failed(
                "interface_discovery_failed",
                "Interface output exceeded its limit.",
            ));
        }
        let bytes = output.stdout;
        let text = std::str::from_utf8(&bytes).map_err(|_| {
            failed(
                "interface_discovery_failed",
                "Interface address was not valid UTF-8.",
            )
        })?;
        if let Some(address) = text.trim().lines().next().and_then(private_address) {
            result.push(super::LanInterface {
                name: name.to_owned(),
                address,
            });
        }
    }
    Ok(result)
}

pub(super) fn interfaces() -> Result<Vec<super::LanInterface>, AppError> {
    #[cfg(target_os = "macos")]
    {
        let bytes = run("ifconfig", &["-l"], "interface_discovery_failed")?;
        let names = std::str::from_utf8(&bytes).map_err(|_| {
            failed(
                "interface_discovery_failed",
                "Interface list was not valid UTF-8.",
            )
        })?;
        return parse_macos_interfaces(names);
    }
    #[cfg(target_os = "linux")]
    {
        let bytes = run(
            "ip",
            &["-j", "-4", "address", "show", "up"],
            "interface_discovery_failed",
        )?;
        let value: Value = serde_json::from_slice(&bytes).map_err(|_| {
            failed(
                "interface_discovery_failed",
                "Interface list was not valid JSON.",
            )
        })?;
        return Ok(parse_linux_interfaces(&value));
    }
    #[cfg(windows)]
    {
        let value = powershell(
            "Get-NetIPAddress -AddressFamily IPv4 | Select-Object -Property InterfaceAlias,IPAddress | ConvertTo-Json -Compress",
            "interface_discovery_failed",
        )?;
        return Ok(parse_windows_interfaces(&value));
    }
    #[allow(unreachable_code)]
    Ok(Vec::new())
}

#[cfg(any(windows, test))]
fn parse_windows_interfaces(value: &Value) -> Vec<super::LanInterface> {
    values(value)
        .into_iter()
        .take(MAX_ITEMS)
        .filter_map(|item| {
            let name = item.get("InterfaceAlias")?.as_str()?;
            if name.is_empty() || name.len() > MAX_NAME || name.chars().any(char::is_control) {
                return None;
            }
            Some(super::LanInterface {
                name: name.to_owned(),
                address: private_address(item.get("IPAddress")?.as_str()?)?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_and_filters_synthetic_platform_data() {
        let processes: Value = serde_json::from_str(r#"[{"Id":41,"ProcessName":"Safari"},{"Id":0,"ProcessName":"bad"},{"Id":42,"ProcessName":"bad\nname"}]"#).unwrap();
        assert_eq!(
            parse_windows_processes(&processes)
                .iter()
                .map(|p| p.pid)
                .collect::<Vec<_>>(),
            vec![41]
        );

        let interfaces: Value = serde_json::from_str(r#"[{"ifname":"en0","addr_info":[{"family":"inet","local":"192.168.1.7"},{"family":"inet","local":"8.8.8.8"},{"family":"inet6","local":"fd00::1"}]},{"ifname":"lo","addr_info":[{"family":"inet","local":"127.0.0.1"}]}]"#).unwrap();
        let found = parse_linux_interfaces(&interfaces);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "en0");
        assert_eq!(found[0].address, "192.168.1.7");

        let windows: Value = serde_json::from_str(r#"[{"InterfaceAlias":"Wi-Fi","IPAddress":"10.1.2.3"},{"InterfaceAlias":"Public","IPAddress":"203.0.113.2"}]"#).unwrap();
        let found = parse_windows_interfaces(&windows);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "Wi-Fi");

        let process_text = (1..=MAX_ITEMS + 1)
            .map(|pid| format!("{pid} /usr/bin/task-{pid}\n"))
            .collect::<String>();
        let parsed = parse_processes(&process_text);
        assert_eq!(parsed.len(), MAX_ITEMS);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn actual_process_discovery_is_read_only() {
        let found = processes().expect("macOS ps process discovery");
        assert!(
            found
                .iter()
                .any(|process| process.pid > 0 && !process.name.is_empty())
        );
    }
}
