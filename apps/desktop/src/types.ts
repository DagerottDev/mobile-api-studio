export type FlowSource = "proxy" | "replay" | "mock" | "sdk" | "fixture";
export type SessionStatus = "active" | "completed" | "interrupted";
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
