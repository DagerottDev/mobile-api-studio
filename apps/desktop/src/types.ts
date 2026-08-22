export type FlowSource = "proxy" | "replay" | "mock" | "sdk" | "fixture";
export type SessionStatus = "active" | "completed" | "interrupted" | "archived";
export type DevicePlatform = "ios" | "android";

export interface AppError {
  code: string;
  message: string;
  recoverable: boolean;
}

export interface DeviceCapabilities {
  canInstallCa: boolean;
  canAutoRouteProxy: boolean;
  canTargetProcess: boolean;
}

export interface Device {
  schemaVersion: number;
  id: string;
  platform: DevicePlatform;
  name: string;
  osVersion: string | null;
  state: string;
  capabilities: DeviceCapabilities;
}

export interface ConnectionDiagnostic {
  code: string;
  title: string;
  message: string;
  recoverable: boolean;
  suggestedAction: string | null;
}

export interface DeviceDiscoveryPayload {
  devices: Device[];
  diagnostics: ConnectionDiagnostic[];
}

export interface ConnectionSnapshot {
  connected: boolean;
  sessionId: string | null;
  deviceId: string | null;
  strategy: string | null;
  proxyHost: string | null;
  proxyPort: number | null;
}

export interface ConnectDeviceResult {
  connection: ConnectionSnapshot;
  diagnostics: ConnectionDiagnostic[];
}

export interface RollbackJournal {
  schemaVersion: number;
  deviceId: string;
  platform: DevicePlatform;
  sessionId: string;
  previousAndroidProxy: string | null;
  iosCaInstalled: boolean;
}

export interface CaptureSession {
  schemaVersion: number;
  id: string;
  name: string;
  status: SessionStatus;
  startedAt: string;
  endedAt: string | null;
  deviceId: string | null;
  appId: string | null;
  connectionStrategy: string | null;
  captureEngine: string | null;
  notes: string | null;
}

export interface HeaderValue {
  name: string;
  value: string;
  sensitive: boolean;
}

export interface BodyRef {
  sha256: string;
  byteSize: number;
  contentType: string | null;
  encoding: string | null;
  isBinary: boolean;
  isTruncated: boolean;
}

export interface Timing {
  dnsMs: number | null;
  connectMs: number | null;
  tlsMs: number | null;
  requestMs: number | null;
  serverMs: number | null;
  downloadMs: number | null;
  totalMs: number | null;
}

export interface RequestDetail {
  method: string;
  url: string;
  scheme: string;
  host: string;
  port: number | null;
  path: string;
  query: string | null;
  headers: HeaderValue[];
  body: BodyRef | null;
}

export interface ResponseDetail {
  statusCode: number;
  reason: string | null;
  headers: HeaderValue[];
  body: BodyRef | null;
}

export interface FlowSummary {
  schemaVersion: number;
  id: string;
  sessionId: string | null;
  source: FlowSource;
  method: string;
  host: string;
  path: string;
  statusCode: number | null;
  durationMs: number | null;
  responseSizeBytes: number | null;
  startedAt: string;
}

export interface FlowDetail {
  summary: FlowSummary;
  request: RequestDetail | null;
  response: ResponseDetail | null;
  timing: Timing;
  errorCode: string | null;
  errorMessage: string | null;
}

export interface BodyPayload {
  sha256: string;
  text: string | null;
  base64: string | null;
}

export interface ReplayHeaderDraft {
  name: string;
  value: string | null;
  sensitive: boolean;
  useOriginal: boolean;
  enabled: boolean;
  sourceIndex: number | null;
}

export interface ReplayBodyDraft {
  text: string | null;
  base64: string | null;
  isBinary: boolean;
  contentType: string | null;
  useOriginal: boolean;
  sourceTruncated: boolean;
}

export interface ReplayDraft {
  sourceFlowId: string;
  method: string;
  url: string;
  headers: ReplayHeaderDraft[];
  body: ReplayBodyDraft | null;
}

export interface NormalizedEndpoint {
  key: string;
  method: string;
  host: string;
  pathTemplate: string;
}

export interface TrafficSearchQuery {
  text: string | null;
  sessionId: string | null;
  source: FlowSource | null;
  method: string | null;
  statusClass: number | null;
  endpointKey: string | null;
  limit: number | null;
}

export interface TrafficSearchResult {
  flow: FlowSummary;
  endpoint: NormalizedEndpoint;
  sessionName: string | null;
}

export interface SavedCollection {
  schemaVersion: number;
  id: string;
  name: string;
  description: string | null;
  sortOrder: number;
  createdAt: string;
  updatedAt: string;
}

export interface SavedRequestBody {
  text: string | null;
  base64: string | null;
  contentType: string | null;
  isBinary: boolean;
}

export interface SavedRequest {
  schemaVersion: number;
  id: string;
  collectionId: string;
  name: string;
  method: string;
  url: string;
  headers: HeaderValue[];
  body: SavedRequestBody | null;
  sourceFlowId: string | null;
  sortOrder: number;
  createdAt: string;
  updatedAt: string;
}

export interface Environment {
  schemaVersion: number;
  id: string;
  name: string;
  isActive: boolean;
  createdAt: string;
  updatedAt: string;
}

export interface EnvironmentVariable {
  schemaVersion: number;
  id: string;
  environmentId: string;
  key: string;
  value: string | null;
  isSecret: boolean;
  secretRef: string | null;
  enabled: boolean;
  sortOrder: number;
}

export interface EnvironmentSnapshot {
  environment: Environment;
  variables: EnvironmentVariable[];
}

export interface InterpolationResult {
  value: string;
  usedSecret: boolean;
  missingVariables: string[];
}
