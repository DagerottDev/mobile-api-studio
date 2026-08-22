use core_model::{normalize_endpoint, CaptureSession, FlowDetail, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap};

pub const COMPARE_SCHEMA_VERSION: u16 = 1;
const INTERNAL_CORRELATION_HEADER: &str = "x-mobile-api-studio-request-id";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ComparableBody {
    pub content_type: Option<String>,
    pub text: Option<String>,
    pub byte_size: u64,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct AppContextEvidence {
    pub app_id: Option<String>,
    pub app_name: Option<String>,
    pub platform: Option<String>,
    pub screen: Option<String>,
    pub feature: Option<String>,
    pub source_file: Option<String>,
    pub source_function: Option<String>,
    pub source_line: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ComparableFlow {
    pub detail: FlowDetail,
    pub request_body: Option<ComparableBody>,
    pub response_body: Option<ComparableBody>,
    pub app_context: Option<AppContextEvidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionSnapshot {
    pub session: CaptureSession,
    pub flows: Vec<ComparableFlow>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Presence {
    Both,
    BaselineOnly,
    CandidateOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DifferenceKind {
    Same,
    Changed,
    Added,
    Removed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ScalarDifference {
    pub kind: DifferenceKind,
    pub baseline: Option<String>,
    pub candidate: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HeaderDifference {
    pub name: String,
    pub kind: DifferenceKind,
    pub baseline: Vec<String>,
    pub candidate: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct JsonTypeChange {
    pub pointer: String,
    pub baseline_type: String,
    pub candidate_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct JsonShapeDifference {
    pub added_paths: Vec<String>,
    pub removed_paths: Vec<String>,
    pub type_changes: Vec<JsonTypeChange>,
}

impl JsonShapeDifference {
    pub fn is_empty(&self) -> bool {
        self.added_paths.is_empty() && self.removed_paths.is_empty() && self.type_changes.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BodyDifference {
    pub kind: DifferenceKind,
    pub baseline_content_type: Option<String>,
    pub candidate_content_type: Option<String>,
    pub baseline_byte_size: Option<u64>,
    pub candidate_byte_size: Option<u64>,
    pub baseline_preview: Option<String>,
    pub candidate_preview: Option<String>,
    pub json_shape: Option<JsonShapeDifference>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QueryDifference {
    pub name: String,
    pub kind: DifferenceKind,
    pub baseline: Vec<String>,
    pub candidate: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RequestDifference {
    pub method: ScalarDifference,
    pub query: Vec<QueryDifference>,
    pub headers: Vec<HeaderDifference>,
    pub body: BodyDifference,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ResponseDifference {
    pub status: ScalarDifference,
    pub headers: Vec<HeaderDifference>,
    pub body: BodyDifference,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct TimingDifference {
    pub baseline_total_ms: Option<u64>,
    pub candidate_total_ms: Option<u64>,
    pub delta_ms: Option<i64>,
    pub delta_percent: Option<i64>,
    pub baseline_size_bytes: Option<u64>,
    pub candidate_size_bytes: Option<u64>,
    pub size_delta_bytes: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CallComparison {
    pub endpoint_key: String,
    pub occurrence: usize,
    pub presence: Presence,
    pub baseline_flow_id: Option<String>,
    pub candidate_flow_id: Option<String>,
    pub baseline_started_at: Option<String>,
    pub candidate_started_at: Option<String>,
    pub request: Option<RequestDifference>,
    pub response: Option<ResponseDifference>,
    pub timing: TimingDifference,
    pub baseline_context: Option<AppContextEvidence>,
    pub candidate_context: Option<AppContextEvidence>,
    pub changed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EndpointComparison {
    pub endpoint_key: String,
    pub method: String,
    pub host: String,
    pub path_template: String,
    pub baseline_count: usize,
    pub candidate_count: usize,
    pub calls: Vec<CallComparison>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateDiagnostic {
    pub endpoint_key: String,
    pub call_count: usize,
    pub likely_retry_count: usize,
    pub flow_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SlowRequestDiagnostic {
    pub flow_id: String,
    pub endpoint_key: String,
    pub total_ms: u64,
    pub status_code: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ErrorClusterDiagnostic {
    pub key: String,
    pub endpoint_key: String,
    pub status_code: Option<u16>,
    pub error_code: Option<String>,
    pub count: usize,
    pub flow_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WaterfallGroupKind {
    Overlap,
    Sequential,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WaterfallGroup {
    pub kind: WaterfallGroupKind,
    pub started_at_ms: u128,
    pub ended_at_ms: u128,
    pub flow_ids: Vec<String>,
    pub endpoint_keys: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct SessionDiagnostics {
    pub duplicates: Vec<DuplicateDiagnostic>,
    pub slowest: Vec<SlowRequestDiagnostic>,
    pub errors: Vec<ErrorClusterDiagnostic>,
    pub waterfall_groups: Vec<WaterfallGroup>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ComparisonSummary {
    pub endpoint_count: usize,
    pub matched_calls: usize,
    pub baseline_only_calls: usize,
    pub candidate_only_calls: usize,
    pub changed_calls: usize,
    pub status_changes: usize,
    pub body_changes: usize,
    pub json_shape_drifts: usize,
    pub timing_regressions: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionComparison {
    pub schema_version: u16,
    pub baseline_session: CaptureSession,
    pub candidate_session: CaptureSession,
    pub endpoints: Vec<EndpointComparison>,
    pub summary: ComparisonSummary,
    pub baseline_diagnostics: SessionDiagnostics,
    pub candidate_diagnostics: SessionDiagnostics,
}

pub fn compare_sessions(baseline: SessionSnapshot, candidate: SessionSnapshot) -> SessionComparison {
    let baseline_diagnostics = diagnose_session(&baseline);
    let candidate_diagnostics = diagnose_session(&candidate);

    let mut baseline_groups = group_flows(&baseline.flows);
    let mut candidate_groups = group_flows(&candidate.flows);
    let mut keys: BTreeSet<String> = baseline_groups.keys().cloned().collect();
    keys.extend(candidate_groups.keys().cloned());

    let mut endpoints = Vec::new();
    for key in keys {
        let baseline_flows = baseline_groups.remove(&key).unwrap_or_default();
        let candidate_flows = candidate_groups.remove(&key).unwrap_or_default();
        let exemplar = baseline_flows.first().or_else(|| candidate_flows.first());
        let Some(exemplar) = exemplar else { continue };
        let normalized = normalize_endpoint(
            &exemplar.detail.summary.method,
            &exemplar.detail.summary.host,
            &exemplar.detail.summary.path,
        );
        let max_calls = baseline_flows.len().max(candidate_flows.len());
        let mut calls = Vec::with_capacity(max_calls);
        for occurrence in 0..max_calls {
            calls.push(compare_call(
                &key,
                occurrence,
                baseline_flows.get(occurrence),
                candidate_flows.get(occurrence),
            ));
        }
        endpoints.push(EndpointComparison {
            endpoint_key: key,
            method: normalized.method,
            host: normalized.host,
            path_template: normalized.path_template,
            baseline_count: baseline_flows.len(),
            candidate_count: candidate_flows.len(),
            calls,
        });
    }

    let summary = summarize(&endpoints);
    SessionComparison {
        schema_version: COMPARE_SCHEMA_VERSION,
        baseline_session: baseline.session,
        candidate_session: candidate.session,
        endpoints,
        summary,
        baseline_diagnostics,
        candidate_diagnostics,
    }
}

pub fn diagnose_session(snapshot: &SessionSnapshot) -> SessionDiagnostics {
    SessionDiagnostics {
        duplicates: duplicate_diagnostics(&snapshot.flows),
        slowest: slowest_diagnostics(&snapshot.flows, 20),
        errors: error_diagnostics(&snapshot.flows),
        waterfall_groups: waterfall_diagnostics(&snapshot.flows),
    }
}

fn group_flows(flows: &[ComparableFlow]) -> BTreeMap<String, Vec<&ComparableFlow>> {
    let mut groups: BTreeMap<String, Vec<&ComparableFlow>> = BTreeMap::new();
    for flow in flows {
        let endpoint = normalize_endpoint(
            &flow.detail.summary.method,
            &flow.detail.summary.host,
            &flow.detail.summary.path,
        );
        groups.entry(endpoint.key).or_default().push(flow);
    }
    for group in groups.values_mut() {
        group.sort_by_key(|flow| timestamp_ms(&flow.detail.summary.started_at));
    }
    groups
}

fn compare_call(
    endpoint_key: &str,
    occurrence: usize,
    baseline: Option<&&ComparableFlow>,
    candidate: Option<&&ComparableFlow>,
) -> CallComparison {
    match (baseline.map(|flow| *flow), candidate.map(|flow| *flow)) {
        (Some(left), Some(right)) => {
            let request = compare_requests(left, right);
            let response = compare_responses(left, right);
            let timing = compare_timing(left, right);
            let changed = request_changed(&request)
                || response_changed(&response)
                || timing_changed(&timing);
            CallComparison {
                endpoint_key: endpoint_key.into(),
                occurrence,
                presence: Presence::Both,
                baseline_flow_id: Some(left.detail.summary.id.clone()),
                candidate_flow_id: Some(right.detail.summary.id.clone()),
                baseline_started_at: Some(left.detail.summary.started_at.clone()),
                candidate_started_at: Some(right.detail.summary.started_at.clone()),
                request: Some(request),
                response: Some(response),
                timing,
                baseline_context: left.app_context.clone(),
                candidate_context: right.app_context.clone(),
                changed,
            }
        }
        (Some(left), None) => CallComparison {
            endpoint_key: endpoint_key.into(),
            occurrence,
            presence: Presence::BaselineOnly,
            baseline_flow_id: Some(left.detail.summary.id.clone()),
            candidate_flow_id: None,
            baseline_started_at: Some(left.detail.summary.started_at.clone()),
            candidate_started_at: None,
            request: None,
            response: None,
            timing: TimingDifference {
                baseline_total_ms: flow_total_ms(left),
                baseline_size_bytes: left.detail.summary.response_size_bytes,
                ..TimingDifference::default()
            },
            baseline_context: left.app_context.clone(),
            candidate_context: None,
            changed: true,
        },
        (None, Some(right)) => CallComparison {
            endpoint_key: endpoint_key.into(),
            occurrence,
            presence: Presence::CandidateOnly,
            baseline_flow_id: None,
            candidate_flow_id: Some(right.detail.summary.id.clone()),
            baseline_started_at: None,
            candidate_started_at: Some(right.detail.summary.started_at.clone()),
            request: None,
            response: None,
            timing: TimingDifference {
                candidate_total_ms: flow_total_ms(right),
                candidate_size_bytes: right.detail.summary.response_size_bytes,
                ..TimingDifference::default()
            },
            baseline_context: None,
            candidate_context: right.app_context.clone(),
            changed: true,
        },
        (None, None) => unreachable!("comparison occurrence must contain a flow"),
    }
}

fn compare_requests(left: &ComparableFlow, right: &ComparableFlow) -> RequestDifference {
    let left_request = left.detail.request.as_ref();
    let right_request = right.detail.request.as_ref();
    RequestDifference {
        method: scalar_diff(
            left_request.map(|request| request.method.as_str()),
            right_request.map(|request| request.method.as_str()),
        ),
        query: compare_query(
            left_request.and_then(|request| request.query.as_deref()),
            right_request.and_then(|request| request.query.as_deref()),
        ),
        headers: compare_headers(
            left_request.map(|request| request.headers.as_slice()).unwrap_or_default(),
            right_request.map(|request| request.headers.as_slice()).unwrap_or_default(),
        ),
        body: compare_body(left.request_body.as_ref(), right.request_body.as_ref()),
    }
}

fn compare_responses(left: &ComparableFlow, right: &ComparableFlow) -> ResponseDifference {
    let left_response = left.detail.response.as_ref();
    let right_response = right.detail.response.as_ref();
    ResponseDifference {
        status: scalar_diff(
            left_response.map(|response| response.status_code.to_string()).as_deref(),
            right_response.map(|response| response.status_code.to_string()).as_deref(),
        ),
        headers: compare_headers(
            left_response.map(|response| response.headers.as_slice()).unwrap_or_default(),
            right_response.map(|response| response.headers.as_slice()).unwrap_or_default(),
        ),
        body: compare_body(left.response_body.as_ref(), right.response_body.as_ref()),
    }
}

fn compare_timing(left: &ComparableFlow, right: &ComparableFlow) -> TimingDifference {
    let baseline_total_ms = flow_total_ms(left);
    let candidate_total_ms = flow_total_ms(right);
    let delta_ms = signed_delta(candidate_total_ms, baseline_total_ms);
    let delta_percent = match (baseline_total_ms, candidate_total_ms) {
        (Some(0), Some(_)) | (None, _) | (_, None) => None,
        (Some(baseline), Some(candidate)) => {
            let delta = candidate as i128 - baseline as i128;
            Some(((delta * 100) / baseline as i128).clamp(i64::MIN as i128, i64::MAX as i128) as i64)
        }
    };
    let baseline_size_bytes = left.detail.summary.response_size_bytes;
    let candidate_size_bytes = right.detail.summary.response_size_bytes;
    TimingDifference {
        baseline_total_ms,
        candidate_total_ms,
        delta_ms,
        delta_percent,
        baseline_size_bytes,
        candidate_size_bytes,
        size_delta_bytes: signed_delta(candidate_size_bytes, baseline_size_bytes),
    }
}

fn compare_headers(left: &[HeaderValue], right: &[HeaderValue]) -> Vec<HeaderDifference> {
    let left = normalized_headers(left);
    let right = normalized_headers(right);
    let mut names: BTreeSet<String> = left.keys().cloned().collect();
    names.extend(right.keys().cloned());
    names
        .into_iter()
        .filter_map(|name| {
            let baseline = left.get(&name).cloned().unwrap_or_default();
            let candidate = right.get(&name).cloned().unwrap_or_default();
            let kind = difference_kind(!baseline.is_empty(), !candidate.is_empty(), baseline == candidate);
            (kind != DifferenceKind::Same).then_some(HeaderDifference {
                name,
                kind,
                baseline,
                candidate,
            })
        })
        .collect()
}

fn normalized_headers(headers: &[HeaderValue]) -> BTreeMap<String, Vec<String>> {
    let mut map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for header in headers {
        let name = header.name.trim().to_ascii_lowercase();
        if name == INTERNAL_CORRELATION_HEADER || name == "content-length" || name == "transfer-encoding" {
            continue;
        }
        let value = if header.sensitive { "<redacted>".to_string() } else { header.value.clone() };
        map.entry(name).or_default().push(value);
    }
    for values in map.values_mut() {
        values.sort();
    }
    map
}

fn compare_query(left: Option<&str>, right: Option<&str>) -> Vec<QueryDifference> {
    let left = parse_query(left);
    let right = parse_query(right);
    let mut names: BTreeSet<String> = left.keys().cloned().collect();
    names.extend(right.keys().cloned());
    names
        .into_iter()
        .filter_map(|name| {
            let baseline = left.get(&name).cloned().unwrap_or_default();
            let candidate = right.get(&name).cloned().unwrap_or_default();
            let kind = difference_kind(!baseline.is_empty(), !candidate.is_empty(), baseline == candidate);
            (kind != DifferenceKind::Same).then_some(QueryDifference { name, kind, baseline, candidate })
        })
        .collect()
}

fn parse_query(value: Option<&str>) -> BTreeMap<String, Vec<String>> {
    let mut output: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let Some(value) = value else { return output };
    for pair in value.split('&') {
        if pair.is_empty() { continue; }
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        output.entry(percent_decode(key)).or_default().push(percent_decode(value));
    }
    for values in output.values_mut() { values.sort(); }
    output
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => { output.push(b' '); index += 1; }
            b'%' if index + 2 < bytes.len() => {
                if let (Some(high), Some(low)) = (hex(bytes[index + 1]), hex(bytes[index + 2])) {
                    output.push((high << 4) | low);
                    index += 3;
                } else {
                    output.push(bytes[index]);
                    index += 1;
                }
            }
            byte => { output.push(byte); index += 1; }
        }
    }
    String::from_utf8_lossy(&output).into_owned()
}

fn hex(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn compare_body(left: Option<&ComparableBody>, right: Option<&ComparableBody>) -> BodyDifference {
    let left_exists = left.is_some();
    let right_exists = right.is_some();
    let equal = match (left, right) {
        (Some(left), Some(right)) => left.text == right.text && left.content_type == right.content_type && left.byte_size == right.byte_size,
        (None, None) => true,
        _ => false,
    };
    let kind = difference_kind(left_exists, right_exists, equal);
    let json_shape = match (left.and_then(parsed_json), right.and_then(parsed_json)) {
        (Some(left_json), Some(right_json)) => {
            let drift = compare_json_shape(&left_json, &right_json);
            (!drift.is_empty()).then_some(drift)
        }
        _ => None,
    };
    BodyDifference {
        kind,
        baseline_content_type: left.and_then(|body| body.content_type.clone()),
        candidate_content_type: right.and_then(|body| body.content_type.clone()),
        baseline_byte_size: left.map(|body| body.byte_size),
        candidate_byte_size: right.map(|body| body.byte_size),
        baseline_preview: left.and_then(body_preview),
        candidate_preview: right.and_then(body_preview),
        json_shape,
    }
}

fn parsed_json(body: &ComparableBody) -> Option<Value> {
    let text = body.text.as_deref()?;
    serde_json::from_str(text).ok()
}

fn body_preview(body: &ComparableBody) -> Option<String> {
    body.text.as_ref().map(|text| {
        const LIMIT: usize = 4_000;
        if text.chars().count() <= LIMIT { text.clone() } else { format!("{}…", text.chars().take(LIMIT).collect::<String>()) }
    })
}

pub fn compare_json_shape(left: &Value, right: &Value) -> JsonShapeDifference {
    let mut left_paths = BTreeMap::new();
    let mut right_paths = BTreeMap::new();
    flatten_json_shape(left, "", &mut left_paths);
    flatten_json_shape(right, "", &mut right_paths);
    let mut added_paths = Vec::new();
    let mut removed_paths = Vec::new();
    let mut type_changes = Vec::new();
    let mut paths: BTreeSet<String> = left_paths.keys().cloned().collect();
    paths.extend(right_paths.keys().cloned());
    for path in paths {
        match (left_paths.get(&path), right_paths.get(&path)) {
            (None, Some(_)) => added_paths.push(path),
            (Some(_), None) => removed_paths.push(path),
            (Some(left_type), Some(right_type)) if left_type != right_type => type_changes.push(JsonTypeChange {
                pointer: path,
                baseline_type: left_type.clone(),
                candidate_type: right_type.clone(),
            }),
            _ => {}
        }
    }
    JsonShapeDifference { added_paths, removed_paths, type_changes }
}

fn flatten_json_shape(value: &Value, path: &str, output: &mut BTreeMap<String, String>) {
    output.insert(if path.is_empty() { "/".into() } else { path.into() }, json_type(value).into());
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                let escaped = key.replace('~', "~0").replace('/', "~1");
                flatten_json_shape(child, &format!("{path}/{escaped}"), output);
            }
        }
        Value::Array(items) => {
            // Compare the structural union of array members instead of positional indexes.
            for child in items.iter().take(32) {
                flatten_json_shape(child, &format!("{path}/*"), output);
            }
        }
        _ => {}
    }
}

fn json_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn scalar_diff(left: Option<&str>, right: Option<&str>) -> ScalarDifference {
    let kind = difference_kind(left.is_some(), right.is_some(), left == right);
    ScalarDifference {
        kind,
        baseline: left.map(str::to_string),
        candidate: right.map(str::to_string),
    }
}

fn difference_kind(left_exists: bool, right_exists: bool, equal: bool) -> DifferenceKind {
    match (left_exists, right_exists, equal) {
        (_, _, true) => DifferenceKind::Same,
        (true, true, false) => DifferenceKind::Changed,
        (true, false, false) => DifferenceKind::Removed,
        (false, true, false) => DifferenceKind::Added,
        (false, false, false) => DifferenceKind::Same,
    }
}

fn request_changed(diff: &RequestDifference) -> bool {
    diff.method.kind != DifferenceKind::Same
        || !diff.query.is_empty()
        || !diff.headers.is_empty()
        || diff.body.kind != DifferenceKind::Same
        || diff.body.json_shape.is_some()
}

fn response_changed(diff: &ResponseDifference) -> bool {
    diff.status.kind != DifferenceKind::Same
        || !diff.headers.is_empty()
        || diff.body.kind != DifferenceKind::Same
        || diff.body.json_shape.is_some()
}

fn timing_changed(diff: &TimingDifference) -> bool {
    diff.delta_ms.unwrap_or_default() != 0 || diff.size_delta_bytes.unwrap_or_default() != 0
}

fn summarize(endpoints: &[EndpointComparison]) -> ComparisonSummary {
    let mut summary = ComparisonSummary { endpoint_count: endpoints.len(), ..ComparisonSummary::default() };
    for endpoint in endpoints {
        for call in &endpoint.calls {
            match call.presence {
                Presence::Both => summary.matched_calls += 1,
                Presence::BaselineOnly => summary.baseline_only_calls += 1,
                Presence::CandidateOnly => summary.candidate_only_calls += 1,
            }
            if call.changed { summary.changed_calls += 1; }
            if let Some(response) = &call.response {
                if response.status.kind != DifferenceKind::Same { summary.status_changes += 1; }
                if response.body.kind != DifferenceKind::Same { summary.body_changes += 1; }
                if response.body.json_shape.is_some() { summary.json_shape_drifts += 1; }
            }
            if call.timing.delta_percent.is_some_and(|delta| delta >= 25)
                && call.timing.delta_ms.is_some_and(|delta| delta >= 100)
            {
                summary.timing_regressions += 1;
            }
        }
    }
    summary
}

fn duplicate_diagnostics(flows: &[ComparableFlow]) -> Vec<DuplicateDiagnostic> {
    let groups = group_flows(flows);
    let mut output = Vec::new();
    for (endpoint_key, calls) in groups {
        if calls.len() < 2 { continue; }
        let likely_retry_count = calls.windows(2).filter(|pair| {
            let first = pair[0];
            let second = pair[1];
            let gap = timestamp_ms(&second.detail.summary.started_at).saturating_sub(timestamp_ms(&first.detail.summary.started_at));
            gap <= 5_000 && is_error(first)
        }).count();
        output.push(DuplicateDiagnostic {
            endpoint_key,
            call_count: calls.len(),
            likely_retry_count,
            flow_ids: calls.iter().map(|flow| flow.detail.summary.id.clone()).collect(),
        });
    }
    output.sort_by(|left, right| right.call_count.cmp(&left.call_count).then_with(|| left.endpoint_key.cmp(&right.endpoint_key)));
    output
}

fn slowest_diagnostics(flows: &[ComparableFlow], limit: usize) -> Vec<SlowRequestDiagnostic> {
    let mut output: Vec<_> = flows.iter().filter_map(|flow| {
        let total_ms = flow_total_ms(flow)?;
        let endpoint = normalize_endpoint(&flow.detail.summary.method, &flow.detail.summary.host, &flow.detail.summary.path);
        Some(SlowRequestDiagnostic {
            flow_id: flow.detail.summary.id.clone(),
            endpoint_key: endpoint.key,
            total_ms,
            status_code: flow.detail.summary.status_code,
        })
    }).collect();
    output.sort_by(|left, right| right.total_ms.cmp(&left.total_ms).then_with(|| left.flow_id.cmp(&right.flow_id)));
    output.truncate(limit);
    output
}

fn error_diagnostics(flows: &[ComparableFlow]) -> Vec<ErrorClusterDiagnostic> {
    let mut groups: HashMap<String, ErrorClusterDiagnostic> = HashMap::new();
    for flow in flows.iter().filter(|flow| is_error(flow)) {
        let endpoint = normalize_endpoint(&flow.detail.summary.method, &flow.detail.summary.host, &flow.detail.summary.path);
        let status_code = flow.detail.summary.status_code;
        let error_code = flow.detail.error_code.clone();
        let key = format!("{}|{}|{}", endpoint.key, status_code.map(|value| value.to_string()).unwrap_or_else(|| "none".into()), error_code.clone().unwrap_or_else(|| "none".into()));
        let entry = groups.entry(key.clone()).or_insert_with(|| ErrorClusterDiagnostic {
            key,
            endpoint_key: endpoint.key,
            status_code,
            error_code,
            count: 0,
            flow_ids: Vec::new(),
        });
        entry.count += 1;
        entry.flow_ids.push(flow.detail.summary.id.clone());
    }
    let mut output: Vec<_> = groups.into_values().collect();
    output.sort_by(|left, right| right.count.cmp(&left.count).then_with(|| left.key.cmp(&right.key)));
    output
}

fn waterfall_diagnostics(flows: &[ComparableFlow]) -> Vec<WaterfallGroup> {
    let mut intervals: Vec<_> = flows.iter().filter_map(|flow| {
        let start = timestamp_ms(&flow.detail.summary.started_at);
        let duration = flow_total_ms(flow)? as u128;
        let endpoint = normalize_endpoint(&flow.detail.summary.method, &flow.detail.summary.host, &flow.detail.summary.path);
        Some((start, start.saturating_add(duration), flow.detail.summary.id.clone(), endpoint.key))
    }).collect();
    intervals.sort_by_key(|item| item.0);
    let mut output: Vec<WaterfallGroup> = Vec::new();
    for (start, end, flow_id, endpoint_key) in intervals {
        if let Some(group) = output.last_mut() {
            if start <= group.ended_at_ms {
                group.kind = WaterfallGroupKind::Overlap;
                group.ended_at_ms = group.ended_at_ms.max(end);
                group.flow_ids.push(flow_id);
                group.endpoint_keys.push(endpoint_key);
                continue;
            }
        }
        output.push(WaterfallGroup {
            kind: WaterfallGroupKind::Sequential,
            started_at_ms: start,
            ended_at_ms: end,
            flow_ids: vec![flow_id],
            endpoint_keys: vec![endpoint_key],
        });
    }
    output
}

fn is_error(flow: &ComparableFlow) -> bool {
    flow.detail.error_code.is_some()
        || flow.detail.summary.status_code.is_some_and(|status| status >= 400)
}

fn flow_total_ms(flow: &ComparableFlow) -> Option<u64> {
    flow.detail.timing.total_ms.or(flow.detail.summary.duration_ms)
}

fn timestamp_ms(value: &str) -> u128 {
    value.parse::<u128>().unwrap_or_default()
}

fn signed_delta(candidate: Option<u64>, baseline: Option<u64>) -> Option<i64> {
    match (candidate, baseline) {
        (Some(candidate), Some(baseline)) => Some((candidate as i128 - baseline as i128).clamp(i64::MIN as i128, i64::MAX as i128) as i64),
        _ => None,
    }
}
