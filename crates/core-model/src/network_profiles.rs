use serde::{Deserialize, Serialize};

pub const NETWORK_PROFILE_SCHEMA_VERSION: u16 = 1;
pub const MAX_NETWORK_PROFILES: usize = 100;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NetworkProfile {
    pub schema_version: u16,
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub priority: i32,
    pub scope: NetworkScope,
    pub latency_ms: u64,
    pub jitter_ms: u64,
    pub upload_bytes_per_second: Option<u64>,
    pub download_bytes_per_second: Option<u64>,
    pub offline: bool,
    pub failure_percent: f64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum NetworkScope {
    Global {},
    App {
        app_id: String,
    },
    Host {
        host: String,
    },
    Endpoint {
        method: String,
        host: String,
        path: String,
    },
}

pub fn validate_network_profile(profile: &NetworkProfile) -> Result<(), String> {
    let text = |value: &str, max: usize| {
        !value.trim().is_empty() && value.len() <= max && !value.chars().any(char::is_control)
    };
    if profile.schema_version != NETWORK_PROFILE_SCHEMA_VERSION {
        return Err("Unsupported network profile version.".into());
    }
    if !text(&profile.id, 120)
        || !text(&profile.name, 120)
        || [&profile.created_at, &profile.updated_at]
            .into_iter()
            .any(|value| {
                value.is_empty()
                    || value.len() > 40
                    || !value.bytes().all(|byte| byte.is_ascii_digit())
            })
    {
        return Err("Network profile ID, name, or numeric timestamp is invalid.".into());
    }
    match &profile.scope {
        NetworkScope::Global {} => {}
        NetworkScope::App { app_id } if text(app_id, 256) => {}
        NetworkScope::Host { host } if valid_host(host) => {}
        NetworkScope::Endpoint { method, host, path }
            if !method.is_empty()
                && method.len() <= 32
                && method.bytes().all(|byte| byte.is_ascii_alphabetic())
                && valid_host(host)
                && text(path, 4096)
                && path.starts_with('/')
                && !path.contains(['?', '#']) => {}
        _ => {
            return Err(
                "Use a bounded exact app, host, or endpoint scope; endpoint paths exclude queries."
                    .into(),
            );
        }
    }
    if profile.latency_ms > 10_000
        || profile.jitter_ms > 10_000
        || [
            profile.upload_bytes_per_second,
            profile.download_bytes_per_second,
        ]
        .into_iter()
        .flatten()
        .any(|rate| !(1_024..=1_073_741_824).contains(&rate))
        || !profile.failure_percent.is_finite()
        || !(0.0..=100.0).contains(&profile.failure_percent)
    {
        return Err("Delay and jitter must be at most 10,000 ms, rates 1 KiB/s–1 GiB/s, and failure percent 0–100.".into());
    }
    Ok(())
}

fn valid_host(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 255
        && !host.chars().any(|c| {
            c.is_control() || c.is_whitespace() || matches!(c, '/' | '?' | '#' | '@' | '*')
        })
}

pub fn validate_network_profiles(profiles: &[NetworkProfile]) -> Result<(), String> {
    if profiles.len() > MAX_NETWORK_PROFILES {
        return Err("The workspace supports at most 100 network profiles.".into());
    }
    let mut ids = std::collections::HashSet::new();
    for profile in profiles {
        validate_network_profile(profile)?;
        if !ids.insert(&profile.id) {
            return Err("Network profile IDs must be unique.".into());
        }
    }
    Ok(())
}

/// Exact endpoint paths exclude the query; a more specific profile always wins.
pub fn select_profile(
    profiles: &[NetworkProfile],
    method: &str,
    host: &str,
    path: &str,
    app_id: Option<&str>,
) -> Option<NetworkProfile> {
    let path = path.split('?').next().unwrap_or(path);
    profiles
        .iter()
        .filter(|profile| profile.enabled)
        .filter_map(|profile| {
            let specificity = match &profile.scope {
                NetworkScope::Global {} => 0,
                NetworkScope::App { app_id: expected } if app_id == Some(expected.as_str()) => 1,
                NetworkScope::Host { host: expected } if expected.eq_ignore_ascii_case(host) => 2,
                NetworkScope::Endpoint {
                    method: expected_method,
                    host: expected_host,
                    path: expected_path,
                } if expected_method.eq_ignore_ascii_case(method)
                    && expected_host.eq_ignore_ascii_case(host)
                    && expected_path == path =>
                {
                    3
                }
                _ => return None,
            };
            Some((specificity, profile))
        })
        .min_by(|(a_scope, a), (b_scope, b)| {
            let a_time = a.created_at.trim_start_matches('0');
            let b_time = b.created_at.trim_start_matches('0');
            b_scope
                .cmp(a_scope)
                .then(a.priority.cmp(&b.priority))
                .then(a_time.len().cmp(&b_time.len()))
                .then(a_time.cmp(b_time))
                .then(a.id.cmp(&b.id))
        })
        .map(|(_, profile)| profile.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn profile(id: &str, scope: NetworkScope) -> NetworkProfile {
        NetworkProfile {
            schema_version: 1,
            id: id.into(),
            name: id.into(),
            enabled: true,
            priority: 0,
            scope,
            latency_ms: 100,
            jitter_ms: 0,
            upload_bytes_per_second: None,
            download_bytes_per_second: None,
            offline: false,
            failure_percent: 0.0,
            created_at: "1".into(),
            updated_at: "1".into(),
        }
    }
    #[test]
    fn network_profile_selection_is_exact_specific_and_stable() {
        let global = profile("global", NetworkScope::Global {});
        let app = profile(
            "app",
            NetworkScope::App {
                app_id: "sample".into(),
            },
        );
        let host = profile(
            "host",
            NetworkScope::Host {
                host: "EXAMPLE.test".into(),
            },
        );
        let endpoint = profile(
            "endpoint",
            NetworkScope::Endpoint {
                method: "GET".into(),
                host: "example.test".into(),
                path: "/items/1".into(),
            },
        );
        let profiles = vec![global, app, host, endpoint];
        assert_eq!(
            select_profile(
                &profiles,
                "get",
                "Example.test",
                "/items/1?q=2",
                Some("sample")
            )
            .unwrap()
            .id,
            "endpoint"
        );
        assert_eq!(
            select_profile(&profiles, "GET", "example.test", "/items/2", Some("sample"))
                .unwrap()
                .id,
            "host"
        );
        assert_eq!(
            select_profile(&profiles, "GET", "other.test", "/", Some("sample"))
                .unwrap()
                .id,
            "app"
        );
        assert_eq!(
            select_profile(&profiles, "GET", "other.test", "/", None)
                .unwrap()
                .id,
            "global"
        );
        let mut late = profile("a", NetworkScope::Global {});
        late.created_at = "10".into();
        let mut early = profile("b", NetworkScope::Global {});
        early.created_at = "2".into();
        assert_eq!(
            select_profile(&[late.clone(), early.clone()], "GET", "x", "/", None)
                .unwrap()
                .id,
            "b"
        );
        late.priority = -1;
        assert_eq!(
            select_profile(&[late.clone(), early.clone()], "GET", "x", "/", None)
                .unwrap()
                .id,
            "a"
        );
        late.enabled = false;
        early.enabled = false;
        assert!(select_profile(&[late, early], "GET", "x", "/", None).is_none());
    }
    #[test]
    fn network_profile_limits_and_unsupported_packet_loss_are_rejected() {
        let base = profile("valid", NetworkScope::Global {});
        assert!(validate_network_profile(&base).is_ok());
        for invalid in [
            NetworkProfile {
                latency_ms: 10_001,
                ..base.clone()
            },
            NetworkProfile {
                jitter_ms: 10_001,
                ..base.clone()
            },
            NetworkProfile {
                failure_percent: f64::NAN,
                ..base.clone()
            },
            NetworkProfile {
                failure_percent: 101.0,
                ..base.clone()
            },
            NetworkProfile {
                upload_bytes_per_second: Some(1_023),
                ..base.clone()
            },
            NetworkProfile {
                download_bytes_per_second: Some(1_073_741_825),
                ..base.clone()
            },
        ] {
            assert!(validate_network_profile(&invalid).is_err());
        }
        assert!(validate_network_profiles(&vec![base.clone(); 101]).is_err());
        assert!(validate_network_profiles(&[base.clone(), base.clone()]).is_err());
        let mut json = serde_json::to_value(base).unwrap();
        json["packetLossPercent"] = 1.into();
        assert!(serde_json::from_value::<NetworkProfile>(json.clone()).is_err());
        json.as_object_mut().unwrap().remove("packetLossPercent");
        json["scope"]["ignored"] = true.into();
        assert!(serde_json::from_value::<NetworkProfile>(json).is_err());
    }
}
