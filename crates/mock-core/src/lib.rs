use core_model::{normalize_endpoint, FlowSummary};
use serde::{Deserialize, Serialize};

pub const MOCK_SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MockPathMatch {
    Exact,
    Normalized,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum MockFailureMode {
    #[default]
    None,
    Drop,
    Timeout,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MockHeaderMutation {
    pub name: String,
    pub value: Option<String>,
    pub remove: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MockJsonMutation {
    pub pointer: String,
    pub value: Option<serde_json::Value>,
    pub remove: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MockBodyEncoding {
    Text,
    Base64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MockBodyOverride {
    pub content_type: Option<String>,
    pub encoding: MockBodyEncoding,
    pub data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MockRule {
    pub schema_version: u16,
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub priority: i64,
    pub method: Option<String>,
    pub host: Option<String>,
    pub path_pattern: String,
    pub path_match: MockPathMatch,
    pub status_code: Option<u16>,
    pub response_headers: Vec<MockHeaderMutation>,
    pub response_body: Option<MockBodyOverride>,
    pub json_mutations: Vec<MockJsonMutation>,
    pub latency_ms: Option<u64>,
    pub failure_mode: MockFailureMode,
    pub request_breakpoint: bool,
    pub response_breakpoint: bool,
    pub source_flow_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl MockRule {
    pub fn matches_flow(&self, flow: &FlowSummary) -> bool {
        if !self.enabled {
            return false;
        }
        if let Some(method) = self.method.as_deref() {
            if !method.eq_ignore_ascii_case(&flow.method) {
                return false;
            }
        }
        if let Some(host) = self.host.as_deref() {
            if !host.eq_ignore_ascii_case(&flow.host) {
                return false;
            }
        }

        match self.path_match {
            MockPathMatch::Exact => self.path_pattern == flow.path.split('?').next().unwrap_or(&flow.path),
            MockPathMatch::Normalized => {
                normalize_endpoint(&flow.method, &flow.host, &flow.path).path_template
                    == self.path_pattern
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MockRulesDocument {
    pub schema_version: u16,
    pub enabled: bool,
    pub rules: Vec<MockRule>,
}

impl MockRulesDocument {
    pub fn active(mut rules: Vec<MockRule>, enabled: bool) -> Self {
        rules.retain(|rule| rule.enabled);
        rules.sort_by(|left, right| {
            left.priority
                .cmp(&right.priority)
                .then_with(|| left.created_at.cmp(&right.created_at))
                .then_with(|| left.id.cmp(&right.id))
        });
        Self {
            schema_version: MOCK_SCHEMA_VERSION,
            enabled,
            rules,
        }
    }
}

pub fn normalized_path_for_flow(flow: &FlowSummary) -> String {
    normalize_endpoint(&flow.method, &flow.host, &flow.path).path_template
}
