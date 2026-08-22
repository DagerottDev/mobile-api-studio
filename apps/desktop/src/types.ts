export type FlowSource = "proxy" | "replay" | "mock" | "sdk" | "fixture";
export type SessionStatus = "active" | "completed" | "interrupted";

export interface AppError {
  code: string;
  message: string;
  recoverable: boolean;
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
