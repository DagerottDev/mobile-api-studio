export type MockPathMatch = "exact" | "normalized";
export type MockFailureMode = "none" | "drop" | "timeout";
export type MockBodyEncoding = "text" | "base64";

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
