use serde::{Deserialize, Serialize};
use regex::RegexBuilder;

pub const PROXY_RULE_SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProxyRule {
    pub schema_version: u16,
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub priority: i64,
    pub matcher: ProxyRuleMatcher,
    pub action: ProxyRuleAction,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProxyRuleMatcher {
    pub method: Option<String>,
    pub host: RulePattern,
    pub path: RulePattern,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RulePattern {
    pub kind: RulePatternKind,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RulePatternKind {
    Exact,
    Wildcard,
    Regex,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuleHeaderMutation {
    pub name: String,
    pub value: Option<String>,
    pub remove: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BreakpointStage {
    Request,
    Response,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum ProxyRuleAction {
    Allow,
    Block { status_code: u16 },
    MapLocal { path: String },
    MapRemote { url: String },
    RewriteRequest { headers: Vec<RuleHeaderMutation>, body: Option<String> },
    RewriteResponse { headers: Vec<RuleHeaderMutation>, body: Option<String> },
    ScriptHook { stage: String, script: String },
    Breakpoint { stage: BreakpointStage },
    InspectHttps { enabled: bool },
    NoCache,
    BlockCookies,
    DnsOverride { address: String },
    ReverseProxy { url: String },
    UpstreamProxy { url: String },
    SocksProxy { address: String },
}

impl ProxyRuleMatcher {
    pub fn matches(&self, method: &str, host: &str, path: &str) -> Result<bool, regex::Error> {
        if self.method.as_deref().is_some_and(|expected| !expected.eq_ignore_ascii_case(method)) {
            return Ok(false);
        }
        Ok(self.host.matches_with_case(host, true)? && self.path.matches(path)?)
    }
}

impl RulePattern {
    pub fn matches(&self, candidate: &str) -> Result<bool, regex::Error> {
        self.matches_with_case(candidate, false)
    }

    fn matches_with_case(&self, candidate: &str, case_insensitive: bool) -> Result<bool, regex::Error> {
        let expression = match self.kind {
            RulePatternKind::Exact => return Ok(if case_insensitive { self.value.eq_ignore_ascii_case(candidate) } else { self.value == candidate }),
            RulePatternKind::Wildcard => {
                let escaped = regex::escape(&self.value);
                format!("^{}$", escaped.replace(r"\*", ".*").replace(r"\?", "."))
            }
            RulePatternKind::Regex => format!("^(?:{})$", self.value),
        };
        Ok(RegexBuilder::new(&expression).case_insensitive(case_insensitive).build()?.is_match(candidate))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matcher_keeps_exact_wildcard_and_regex_distinct() {
        let wildcard = RulePattern { kind: RulePatternKind::Wildcard, value: "/v*/users/?".into() };
        assert!(wildcard.matches("/v2/users/7").unwrap());
        assert!(!wildcard.matches("/v2/users/77").unwrap());
        let regex = RulePattern { kind: RulePatternKind::Regex, value: r"/v[0-9]+/users/[0-9]+".into() };
        assert!(regex.matches("/v12/users/77").unwrap());
        assert!(!regex.matches("/v12/users/x").unwrap());
        let exact = RulePattern { kind: RulePatternKind::Exact, value: "/v2/users/7".into() };
        assert!(!exact.matches("/v2/users/77").unwrap());
        let matcher = ProxyRuleMatcher {
            method: Some("GET".into()),
            host: RulePattern { kind: RulePatternKind::Wildcard, value: "*.Example.COM".into() },
            path: exact,
        };
        assert!(matcher.matches("get", "api.example.com", "/v2/users/7").unwrap());
    }
}
