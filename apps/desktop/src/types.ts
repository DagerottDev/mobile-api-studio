export type FlowSource = "proxy" | "replay" | "mock" | "sdk" | "fixture";

export interface FlowSummary {
  schemaVersion: number;
  id: string;
  source: FlowSource;
  method: string;
  host: string;
  path: string;
  statusCode: number | null;
  durationMs: number | null;
  responseSizeBytes: number | null;
  startedAt: string;
}
