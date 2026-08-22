use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FlowSource {
    Proxy,
    Replay,
    Mock,
    Sdk,
    Fixture,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FlowSummary {
    pub schema_version: u16,
    pub id: String,
    pub source: FlowSource,
    pub method: String,
    pub host: String,
    pub path: String,
    pub status_code: Option<u16>,
    pub duration_ms: Option<u64>,
    pub response_size_bytes: Option<u64>,
    pub started_at: String,
}

impl FlowSummary {
    #[allow(clippy::too_many_arguments)]
    pub fn fixture(
        id: impl Into<String>,
        method: impl Into<String>,
        host: impl Into<String>,
        path: impl Into<String>,
        status_code: u16,
        duration_ms: u64,
        response_size_bytes: u64,
        started_at: impl Into<String>,
    ) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            id: id.into(),
            source: FlowSource::Fixture,
            method: method.into(),
            host: host.into(),
            path: path.into(),
            status_code: Some(status_code),
            duration_ms: Some(duration_ms),
            response_size_bytes: Some(response_size_bytes),
            started_at: started_at.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DevicePlatform {
    Ios,
    Android,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub schema_version: u16,
    pub id: String,
    pub platform: DevicePlatform,
    pub name: String,
    pub os_version: Option<String>,
    pub state: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_flow_uses_current_schema() {
        let flow = FlowSummary::fixture(
            "flow-1",
            "GET",
            "api.example.dev",
            "/json",
            200,
            20,
            42,
            "2026-08-22T00:00:00Z",
        );

        assert_eq!(flow.schema_version, SCHEMA_VERSION);
        assert_eq!(flow.source, FlowSource::Fixture);
    }
}
