export interface AiSettingsSnapshot {
  provider: string;
  model: string;
  apiKeyConfigured: boolean;
  secureStoreAvailable: boolean;
  secretJsonKeys: string[];
}

export interface AiContextPreview {
  json: string;
  contextFingerprint: string;
  byteCount: number;
  redactionCount: number;
  truncated: boolean;
}

export interface AiResultRecord {
  id: string;
  taskKind: string;
  sourceRef: string;
  provider: string;
  model: string;
  contextFingerprint: string;
  remoteResponseId: string | null;
  outputText: string;
  createdAt: string;
}

export interface AiGenerationResult {
  record: AiResultRecord;
  context: AiContextPreview;
}
