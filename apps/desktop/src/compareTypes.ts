import type { CaptureSession } from "./types";

export type Presence = "both" | "baseline_only" | "candidate_only";
export type DifferenceKind = "same" | "changed" | "added" | "removed";

export interface AppContextEvidence {
  appId: string | null;
  appName: string | null;
  platform: string | null;
  screen: string | null;
  feature: string | null;
  sourceFile: string | null;
  sourceFunction: string | null;
  sourceLine: number | null;
}

export interface ScalarDifference {
  kind: DifferenceKind;
  baseline: string | null;
  candidate: string | null;
}

export interface HeaderDifference {
  name: string;
  kind: DifferenceKind;
  baseline: string[];
  candidate: string[];
}

export interface QueryDifference {
  name: string;
  kind: DifferenceKind;
  baseline: string[];
  candidate: string[];
}

export interface JsonTypeChange {
  pointer: string;
  baselineType: string;
  candidateType: string;
}

export interface JsonShapeDifference {
  addedPaths: string[];
  removedPaths: string[];
  typeChanges: JsonTypeChange[];
}

export interface BodyDifference {
  kind: DifferenceKind;
  baselineContentType: string | null;
  candidateContentType: string | null;
  baselineByteSize: number | null;
  candidateByteSize: number | null;
  baselinePreview: string | null;
  candidatePreview: string | null;
  jsonShape: JsonShapeDifference | null;
}

export interface RequestDifference {
  method: ScalarDifference;
  query: QueryDifference[];
  headers: HeaderDifference[];
  body: BodyDifference;
}

export interface ResponseDifference {
  status: ScalarDifference;
  headers: HeaderDifference[];
  body: BodyDifference;
}

export interface TimingDifference {
  baselineTotalMs: number | null;
  candidateTotalMs: number | null;
  deltaMs: number | null;
  deltaPercent: number | null;
  baselineSizeBytes: number | null;
  candidateSizeBytes: number | null;
  sizeDeltaBytes: number | null;
}

export interface CallComparison {
  endpointKey: string;
  occurrence: number;
  presence: Presence;
  baselineFlowId: string | null;
  candidateFlowId: string | null;
  baselineStartedAt: string | null;
  candidateStartedAt: string | null;
  request: RequestDifference | null;
  response: ResponseDifference | null;
  timing: TimingDifference;
  baselineContext: AppContextEvidence | null;
  candidateContext: AppContextEvidence | null;
  changed: boolean;
}

export interface EndpointComparison {
  endpointKey: string;
  method: string;
  host: string;
  pathTemplate: string;
  baselineCount: number;
  candidateCount: number;
  calls: CallComparison[];
}

export interface DuplicateDiagnostic {
  endpointKey: string;
  callCount: number;
  likelyRetryCount: number;
  flowIds: string[];
}

export interface SlowRequestDiagnostic {
  flowId: string;
  endpointKey: string;
  totalMs: number;
  statusCode: number | null;
}

export interface ErrorClusterDiagnostic {
  key: string;
  endpointKey: string;
  statusCode: number | null;
  errorCode: string | null;
  count: number;
  flowIds: string[];
}

export interface WaterfallGroup {
  kind: "overlap" | "sequential";
  startedAtMs: number;
  endedAtMs: number;
  flowIds: string[];
  endpointKeys: string[];
}

export interface SessionDiagnostics {
  duplicates: DuplicateDiagnostic[];
  slowest: SlowRequestDiagnostic[];
  errors: ErrorClusterDiagnostic[];
  waterfallGroups: WaterfallGroup[];
}

export interface ComparisonSummary {
  endpointCount: number;
  matchedCalls: number;
  baselineOnlyCalls: number;
  candidateOnlyCalls: number;
  changedCalls: number;
  statusChanges: number;
  bodyChanges: number;
  jsonShapeDrifts: number;
  timingRegressions: number;
}

export interface SessionComparison {
  schemaVersion: number;
  baselineSession: CaptureSession;
  candidateSession: CaptureSession;
  endpoints: EndpointComparison[];
  summary: ComparisonSummary;
  baselineDiagnostics: SessionDiagnostics;
  candidateDiagnostics: SessionDiagnostics;
}
