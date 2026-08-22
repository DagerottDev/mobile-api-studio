export type MockPathMatch = "exact" | "normalized";
export type MockFailureMode = "none" | "drop" | "timeout";
export type MockBodyEncoding = "text" | "base64";
export type BreakpointStage = "request" | "response";

export interface MockHeaderMutation {
  name: string;
  value: string | null;
  remove: boolean;
}

export interface MockJsonMutation {
  pointer: string;
  value: unknown | null;
  remove: boolean;
}

export interface MockBodyOverride {
  contentType: string | null;
  encoding: MockBodyEncoding;
  data: string;
}

export interface MockRule {
  schemaVersion: number;
  id: string;
  name: string;
  enabled: boolean;
  priority: number;
  method: string | null;
  host: string | null;
  pathPattern: string;
  pathMatch: MockPathMatch;
  statusCode: number | null;
  responseHeaders: MockHeaderMutation[];
  responseBody: MockBodyOverride | null;
  jsonMutations: MockJsonMutation[];
  latencyMs: number | null;
  failureMode: MockFailureMode;
  requestBreakpoint: boolean;
  responseBreakpoint: boolean;
  sourceFlowId: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface MockFixture {
  schemaVersion: number;
  id: string;
  name: string;
  statusCode: number;
  responseHeaders: MockHeaderMutation[];
  responseBody: MockBodyOverride | null;
  sourceFlowId: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface BreakpointHeader {
  name: string;
  value: string;
}

export interface BreakpointBody {
  dataBase64: string;
  contentType: string | null;
  isBinary: boolean;
  isTruncated: boolean;
}

export interface PendingBreakpoint {
  schemaVersion: number;
  id: string;
  flowId: string;
  ruleId: string;
  ruleName: string;
  stage: BreakpointStage;
  createdAt: string;
  deadlineAt: string;
  method: string;
  url: string;
  headers: BreakpointHeader[];
  body: BreakpointBody | null;
  statusCode: number | null;
}
