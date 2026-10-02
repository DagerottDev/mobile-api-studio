export type PatternKind = "exact" | "wildcard" | "regex";
export interface RulePattern { kind: PatternKind; value: string }
export interface RuleHeaderMutation { name: string; value: string | null; remove: boolean }
export type ProxyRuleAction =
  | { type: "script_hook"; stage: "request" | "response" | "websocket"; script: string }
  | { type: "allow" }
  | { type: "block"; statusCode: number }
  | { type: "map_local"; path: string }
  | { type: "map_remote"; url: string }
  | { type: "rewrite_request" | "rewrite_response"; headers: RuleHeaderMutation[]; body: string | null }
  | { type: "breakpoint"; stage: "request" | "response" }
  | { type: "no_cache" }
  | { type: "dns_override"; address: string }
  | { type: "inspect_https"; enabled: boolean }
  | { type: "block_cookies" };
export interface ProxyRule {
  schemaVersion: number;
  id: string;
  name: string;
  enabled: boolean;
  priority: number;
  matcher: { method: string | null; host: RulePattern; path: RulePattern };
  action: ProxyRuleAction;
  createdAt: string;
  updatedAt: string;
}
