export type SdkPlatform = "ios" | "android" | "other";
export type SdkLogLevel = "debug" | "info" | "warning" | "error";
export type SdkNetworkPhase = "started" | "completed" | "failed";

export interface SdkSourceLocation {
  file: string | null;
  function: string | null;
  line: number | null;
}

export interface SdkContextSnapshot {
  screen: string | null;
  feature: string | null;
  attributes: Record<string, string>;
  source: SdkSourceLocation | null;
}

export interface SdkHandshake {
  clientId: string;
  appId: string;
  appName: string;
  appVersion: string | null;
  appBuild: string | null;
  platform: SdkPlatform;
  deviceName: string | null;
  osVersion: string | null;
  sdkVersion: string;
}

export interface SdkContextEvent {
  clientId: string;
  context: SdkContextSnapshot;
}

export interface SdkLogEvent {
  clientId: string;
  level: SdkLogLevel;
  message: string;
  context: SdkContextSnapshot;
}

export interface SdkNetworkEvent {
  clientId: string;
  requestId: string;
  phase: SdkNetworkPhase;
  method: string;
  url: string;
  statusCode: number | null;
  durationMs: number | null;
  error: string | null;
  context: SdkContextSnapshot;
}

export type SdkEvent =
  | { type: "handshake"; payload: SdkHandshake }
  | { type: "context"; payload: SdkContextEvent }
  | { type: "log"; payload: SdkLogEvent }
  | { type: "network"; payload: SdkNetworkEvent };

export interface SdkEnvelope {
  schemaVersion: number;
  eventId: string;
  occurredAt: string;
  event: SdkEvent;
}

export interface SdkClientRecord {
  clientId: string;
  appId: string;
  appName: string;
  appVersion: string | null;
  appBuild: string | null;
  platform: SdkPlatform;
  deviceName: string | null;
  osVersion: string | null;
  sdkVersion: string;
  firstSeenAt: string;
  lastSeenAt: string;
}

export interface SdkSetupInfo {
  port: number;
  iosBaseUrl: string;
  androidBaseUrl: string;
  eventPath: string;
  healthPath: string;
  correlationHeader: string;
  ingestionReachable: boolean;
  activeClientCount: number;
  knownClientCount: number;
  latestSeenAt: string | null;
}

export interface FlowSdkEnrichment {
  requestId: string | null;
  client: SdkClientRecord | null;
  requestEvents: SdkEnvelope[];
  nearbyEvents: SdkEnvelope[];
}

export function contextForEnvelope(envelope: SdkEnvelope): SdkContextSnapshot | null {
  if (envelope.event.type === "context") return envelope.event.payload.context;
  if (envelope.event.type === "log") return envelope.event.payload.context;
  if (envelope.event.type === "network") return envelope.event.payload.context;
  return null;
}
