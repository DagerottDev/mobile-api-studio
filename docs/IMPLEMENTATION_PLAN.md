# Mobile API Studio — Detailed Implementation Plan

## 1. Problem statement

Mobile developers commonly jump between multiple tools to answer one debugging question:

- Xcode / Android Studio to run the app.
- Charles, Proxyman, HTTP Toolkit, or mitmproxy to capture traffic.
- Postman/Bruno/Insomnia to edit and replay requests.
- Ad-hoc mocks or backend flags to simulate failures.
- Separate iOS and Android sessions to compare client behavior.
- Console logs to understand which screen or source path triggered a call.

Mobile API Studio should collapse that loop into one local workflow. The product should understand **devices, sessions, requests, replay, mocks, and app context** rather than treating every API call as an isolated REST request.

## 2. Target users

### Primary

- iOS and Android application developers.
- Mobile QA/SDET engineers reproducing network issues.
- Engineers maintaining equivalent iOS and Android product flows.

### Secondary

- Backend engineers debugging mobile-specific request differences.
- API/platform teams validating backward compatibility.
- Developers working on React Native/Flutter apps when their networking still flows through inspectable native stacks.

## 3. Jobs to be done

1. **Capture:** “Show me every request my simulator/emulator is making.”
2. **Understand:** “Why is this request slow/failing?”
3. **Replay:** “Run the exact request again after I change one field.”
4. **Mock:** “Show me what the app does for 500/401/timeout/out-of-stock without backend help.”
5. **Compare:** “Why does Android work while iOS fails?”
6. **Correlate:** “Which app screen / feature / source path caused this request?”
7. **Share:** “Export a safe, redacted reproduction package for another engineer.”

## 4. Non-goals for the first releases

- Replacing a full API collaboration platform.
- Production-device forensics.
- Defeating certificate pinning in third-party apps.
- Packet-level Wireshark replacement.
- Full TCP/UDP protocol analyzer.
- Cloud account/team synchronization.
- Performance APM backend.
- Automatic code modification or runtime injection into arbitrary apps.

## 5. Delivery strategy

The key architectural decision is to decouple the product from the first capture implementation.

### v0.1 capture strategy

Use `mitmdump` as a sidecar and a small addon that emits normalized flow events. The desktop Rust core owns session state, storage, replay, UI events, device management, and product behavior.

This gives us:

- mature TLS interception behavior;
- HTTP/1 + HTTP/2 + WebSocket foundations;
- existing certificate tooling;
- a fast path to a real product demo;
- a clean seam for replacing the capture engine later.

### Long-term capture strategy

Define a stable Rust `CaptureEngine` boundary. Potential engines:

- `MitmCaptureEngine` — initial implementation;
- `LocalCaptureEngine` — process-scoped host capture where supported;
- future native Rust proxy;
- `SdkCaptureEngine` — app-side event stream from v0.4 SDKs.

No UI component should know which engine produced an event.

---

# Phase 0 — Foundation

## Objective

Create a maintainable monorepo and prove the core data path before integrating real mobile devices.

## Deliverables

### 0.1 Monorepo

Use a Cargo workspace plus pnpm workspace.

```text
apps/desktop
crates/core-model
crates/capture-core
crates/capture-mitm
crates/device-ios
crates/device-android
crates/storage
crates/replay
sidecars/mitm-addon
fixtures/test-server
```

### 0.2 Desktop shell

Create Tauri 2 + React + TypeScript app with four placeholder routes:

- Connect
- Traffic
- Replay
- Settings

Do not build polished UI yet. The goal is to prove Tauri command/event communication.

### 0.3 Core domain models

Define versioned domain types before persistence.

```text
Device
DeviceRuntime
CaptureSession
Flow
Request
Response
Timing
BodyRef
TlsInfo
ReplayDraft
Environment
ConnectionDiagnostic
```

All externally serialized events include `schema_version`.

### 0.4 SQLite storage

Metadata lives in SQLite. Large bodies are stored separately using a content-addressed body store.

Recommended location:

```text
~/Library/Application Support/Mobile API Studio/
  app.db
  bodies/
  certificates/
  logs/
```

Equivalent platform-native application-data directories should be used on Windows/Linux later.

### 0.5 Local deterministic test server

Build a small local fixture server with endpoints:

```text
GET  /json
POST /echo
GET  /delay/:ms
GET  /status/:code
GET  /large/:kb
GET  /binary/image
GET  /redirect/:count
WS   /ws/echo
```

This server becomes the backbone of repeatable proxy, replay, mock, and UI tests.

### 0.6 CI

Initial checks:

- TypeScript typecheck
- ESLint
- frontend unit tests
- `cargo fmt --check`
- `cargo clippy -- -D warnings`
- `cargo test`
- Python addon lint/test

## Exit gate

A locally generated fake flow can move through:

```text
capture event -> Rust normalization -> SQLite -> Tauri event -> React timeline
```

No simulator/emulator is required yet.

---

# Phase 1 — v0.1 Capture, Inspect, Replay

## Product outcome

A developer can attach to a booted mobile runtime, capture common HTTP(S) traffic, inspect it, and replay captured requests.

## 1.1 Device discovery

### iOS Simulator

Create `device-ios` around `xcrun simctl`.

Capabilities:

- detect whether Xcode command-line tools exist;
- parse `xcrun simctl list --json devices`;
- identify booted simulators;
- capture name, UDID, runtime, state;
- install a root CA with `simctl keychain <UDID> add-root-cert <path>`;
- later list installed apps when useful.

Model:

```text
Device {
  id: "ios:<udid>",
  platform: ios,
  name,
  os_version,
  state,
  capabilities: {
    can_install_ca,
    can_auto_route_proxy,
    can_target_process,
    ...
  }
}
```

Do not hardcode every Xcode/simctl behavior. Run capability probes and present failures as diagnostics.

### Android Emulator

Create `device-android` around ADB.

Capabilities:

- run `adb devices -l`;
- distinguish emulators from physical devices for v0.1;
- read model/API level with `getprop`;
- detect common emulator host routing;
- query current global proxy where possible;
- apply/clear proxy through a strategy abstraction;
- show guided steps if automated configuration is not supported.

Android proxy handling must be capability-based. Some apps ignore system proxy settings, and Android trust rules differ by OS/app network-security configuration.

## 1.2 Connection coordinator

Create a state machine:

```text
Disconnected
  -> CheckingPrerequisites
  -> PreparingCaptureEngine
  -> PreparingCertificate
  -> ConfiguringDevice
  -> VerifyingTraffic
  -> Connected
  -> Disconnecting
  -> Disconnected
```

Every mutation records a rollback action.

Example rollback stack:

```text
restore previous macOS proxy
restore previous Android proxy
stop sidecar
release ports
```

If the app crashes, a “Repair connection” flow should inspect stale state on next launch.

## 1.3 Certificate authority lifecycle

Requirements:

- one locally generated CA per Mobile API Studio installation;
- private key never leaves the machine;
- file permissions restricted to current user;
- clear UI explaining why the CA is required;
- ability to regenerate/rotate CA;
- ability to remove/reinstall trust on known test runtimes;
- no automatic system trust mutation without explicit user action.

For v0.1, it is acceptable to rely on mitmproxy’s generated CA while wrapping it in our lifecycle/diagnostic UI.

## 1.4 Capture engine interface

Rust interface concept:

```rust
#[async_trait]
pub trait CaptureEngine {
    async fn prepare(&self) -> Result<CaptureCapabilities>;
    async fn start(&self, config: CaptureConfig) -> Result<CaptureHandle>;
    async fn stop(&self, handle: CaptureHandle) -> Result<()>;
    fn subscribe(&self) -> broadcast::Receiver<CaptureEvent>;
}
```

`CaptureEvent` should include lifecycle, request headers, request body chunks/body ref, response headers/body, timings, errors, WebSocket events where available, and connection metadata.

## 1.5 mitmdump bridge

Create `sidecars/mitm-addon/addon.py` that normalizes flows into JSON Lines or a local socket protocol.

Suggested event framing:

```json
{
  "schema_version": 1,
  "type": "response_complete",
  "flow_id": "...",
  "started_at": "...",
  "request": { "method": "GET", "url": "..." },
  "response": { "status": 200 },
  "timing": { "duration_ms": 184 }
}
```

Avoid dumping unbounded bodies directly over stdout. For larger bodies, the addon should stream chunks or write to a temporary body file and pass a reference.

### Sidecar process rules

- random free control port;
- explicit working directory;
- capture stderr to structured diagnostic logs;
- heartbeat/health endpoint or heartbeat event;
- force-kill fallback after graceful stop timeout;
- child process must not survive normal app exit.

## 1.6 Connection strategies

### iOS v0.1

Support at least one reliable route:

- discover booted simulator;
- install CA via `simctl`;
- route simulator traffic through capture using a reversible macOS proxy strategy or a supported local-capture strategy;
- verify by requesting a known URL from a test app/Safari;
- restore prior settings on disconnect.

Because Simulator networking and macOS proxy behavior can vary by environment, the UI should show the actual strategy being used and offer a guided fallback.

### Android v0.1

Use the Android System Proxy path for inspectable app traffic where compatible.

For the standard Android Emulator, host reachability commonly uses the emulator’s host alias rather than `127.0.0.1`. Resolve this in a platform strategy instead of hardcoding it in UI.

Provide diagnostics for:

- system proxy ignored by app;
- CA not trusted by app;
- Play Store/non-rootable image limitations;
- app network-security config rejecting user CAs;
- certificate pinning.

## 1.7 Traffic timeline UI

Minimum columns:

```text
Method | Host | Path | Status | Duration | Size | Started
```

Requirements:

- virtualized list;
- live inserts without jumping user scroll position;
- pause UI updates while capture continues;
- row selection opens request detail;
- clear session;
- basic filters by method/status/host;
- error rows visually distinct;
- binary body indicator.

## 1.8 Request detail

Tabs:

- Overview
- Request headers
- Request body
- Response headers
- Response body
- Timing
- TLS/connection

Body viewers:

- pretty JSON
- raw text
- image preview
- hex/metadata fallback for binary data

For unknown/large content types, default to metadata rather than eagerly loading the entire body into the UI.

## 1.9 Persistence

Tables should roughly separate:

```text
sessions
flows
requests
responses
headers
bodies
connection_events
```

Important indexes:

- session + started_at
- host
- status
- method
- normalized path

Bodies use a hash-based file store. Store compression metadata and preview text separately.

## 1.10 Safe cURL export

Default “Copy cURL” behavior redacts:

- `Authorization`
- `Cookie`
- `Set-Cookie`
- common API key headers

A user can explicitly choose “Copy with secrets” after a warning.

## 1.11 Replay

Clicking Replay creates a draft detached from the original flow.

Editable fields:

- method
- URL
- query params
- headers
- body
- timeout

Replay engine is owned by Rust, not mitmproxy.

Use a modern async HTTP client and preserve reasonable fidelity without trying to reproduce every transport-level characteristic.

Replay results are stored as flows with `source = replay` and linked to their parent flow.

## v0.1 acceptance criteria

A release candidate must demonstrate:

1. Booted iOS Simulator discovery on a supported macOS/Xcode setup.
2. Android Emulator discovery through ADB.
3. Start/stop capture without orphaning the capture process.
4. Successful capture of HTTP and HTTPS from a non-pinned debug/test app on supported runtimes.
5. Request/response headers and common text/JSON bodies visible.
6. At least 10,000 stored flows in a session without the timeline becoming unusable.
7. Copy cURL redacts secrets by default.
8. Captured request can be replayed after editing one field.
9. Disconnect restores known proxy state where the selected connection strategy modified it.
10. Failure diagnostics clearly distinguish “no traffic”, “TLS trust”, “proxy ignored”, and “capture engine failed”.

---

# Phase 2 — v0.2 Sessions, Search, Collections, Environments

## Product outcome

The tool becomes comfortable for daily debugging instead of only being a capture demo.

## 2.1 Persistent sessions

Session metadata:

- name
- platform/device
- app target if known
- started/ended timestamps
- tags
- connection strategy
- notes

Functions:

- rename
- duplicate metadata
- delete
- export
- reopen historical session

## 2.2 Fast search and filters

Search targets:

- host/path
- request/response body preview
- header names/values after redaction rules
- status code
- method
- duration range
- source (captured/replay/mock/SDK)

Support saved filters such as:

```text
host contains api.example.com AND duration > 1000ms
status >= 500
method = POST AND body contains "sku"
```

## 2.3 Endpoint normalization

Create a normalization layer that turns:

```text
/products/123
/products/456
```

into a candidate template:

```text
/products/{id}
```

Do not mutate the original URL. Store a derived `endpoint_key` used for grouping and comparison.

Start with conservative heuristics:

- numeric path components;
- UUIDs;
- long opaque IDs;
- explicitly marked variables.

## 2.4 Collections

Captured requests can be saved as reusable requests.

Collection hierarchy:

```text
Workspace
  Collection
    Folder
      Request Template
```

Keep this local. Team/cloud collaboration remains out of scope.

## 2.5 Environments

Variables:

```text
{{base_url}}
{{token}}
{{user_id}}
```

Secret variables are stored in the OS credential/keychain facility, not plain SQLite.

Support variable extraction from a captured request.

## 2.6 Connection Doctor

A first-class diagnostic screen should check:

```text
[✓] capture engine found
[✓] local port available
[✓] simulator detected
[✓] CA installed
[✓] route/proxy configured
[✓] plain HTTP observed
[✓] HTTPS observed
[!] pinned.example.com rejected local CA
```

Every failure should include a concrete remediation path.

## 2.7 Packaging improvement

Move from “developer must install mitmproxy separately” toward a bundled/managed sidecar where licensing, platform packaging, and update mechanics are understood.

Keep a developer setting for using a system-installed mitmproxy binary.

## v0.2 acceptance criteria

- Historical session can be reopened instantly without capture running.
- Search/filter remains responsive on large sessions.
- Request templates can use environment variables.
- Connection Doctor identifies the major failure category for supported test fixtures.
- Import/export format is versioned and excludes secrets by default.

---

# Phase 3 — v0.3 Mocking and Failure Simulation

## Product outcome

A developer can simulate backend states directly from a captured endpoint.

## 3.1 Mock rule model

```text
MockRule {
  id,
  enabled,
  priority,
  matcher,
  action,
  scope,
  hit_count
}
```

Matcher fields:

- method
- host
- path glob/regex
- query params
- header conditions
- optional JSON body conditions

Actions:

- static response
- fixture file
- mutate real response
- delay
- timeout/drop
- status override
- header add/remove/replace

## 3.2 Quick mock from capture

From a captured response:

```text
Right click -> Mock this response
```

Pre-populate matcher from method + normalized endpoint and body from captured response.

## 3.3 Failure presets

One-click developer presets:

- 401 unauthorized
- 403 forbidden
- 404 not found
- 429 rate limited
- 500 server error
- 503 unavailable
- timeout
- +2s latency
- empty body
- malformed JSON
- truncated body

## 3.4 Breakpoints

Optional interception breakpoint:

```text
Request arrives -> pause -> edit -> continue
Response arrives -> pause -> edit -> continue
```

Breakpoints must have a visible global indicator so a developer does not forget that traffic is paused.

## 3.5 Rule safety

- rules scoped to a capture session by default;
- persistent rules require explicit save;
- show active-rule count in app chrome;
- provide “Disable all mocks” emergency control;
- mocks must be obvious in captured flow metadata.

## v0.3 acceptance criteria

- A captured endpoint can be mocked in fewer than four user actions.
- Latency/status/body override works against fixture app.
- “Disable all mocks” restores pass-through immediately.
- Mocked flows are clearly distinguishable in history.

---

# Phase 4 — v0.4 iOS and Android App SDKs

## Product outcome

Apps the developer controls can provide context unavailable to a generic proxy and can support observability even when network stacks do not accept the interception CA.

## 4.1 Shared SDK protocol

SDKs connect only to the local desktop tool in debug/test configurations.

Protocol events:

```text
sdk_hello
app_context
network_request_started
network_request_finished
log_event
screen_changed
custom_event
```

Each event contains:

- app bundle/package ID
- build/version
- device/session ID
- trace/correlation ID
- timestamp
- SDK schema version

## 4.2 iOS SDK

Swift Package modules:

```text
MobileAPIStudioCore
MobileAPIStudioURLSession
MobileAPIStudioUI (optional debug menu)
```

Capabilities:

- `URLProtocol`/URLSession-compatible capture where safe;
- explicit instrumentation API for custom clients;
- structured app logs;
- screen/context tagging supplied by app integration;
- request correlation IDs;
- optional stack/call-site capture in debug builds with performance guardrails.

The SDK should not ship behavior in release builds unless the developer explicitly opts in.

## 4.3 Android SDK

Kotlin modules:

```text
mobile-api-studio-core
mobile-api-studio-okhttp
mobile-api-studio-ui
```

Capabilities:

- OkHttp interceptor integration;
- request/response metadata/body capture with limits;
- app context tags;
- structured logs;
- correlation IDs.

Provide a no-op release artifact or compile-time configuration pattern so production builds can exclude debugger code cleanly.

## 4.4 Correlation model

SDK flow can be matched to proxy flow using:

1. injected correlation header for developer-owned backends when enabled;
2. SDK-generated stable request ID when network stack allows;
3. timestamp + URL + method + body fingerprint heuristic fallback.

Never inject headers into production builds by accident.

## 4.5 App-aware UI

A flow can show:

```text
App: com.example.shop
Screen: Product Detail
Feature: Add To Bag
Source: CartService.swift:87 (when supplied)
Trace: PDP -> CartRepository -> CartService
```

This metadata comes from SDK instrumentation, not speculative source-code inference.

## v0.4 acceptance criteria

- Swift Package and Android library integrate into sample apps with documented steps.
- App metadata appears beside captured traffic.
- SDK events can be correlated with network flows.
- Release/no-op configuration is verified by CI sample builds.
- No automatic certificate-pinning bypass is included.

---

# Phase 5 — v0.5 Cross-Platform Diff + AI Debugging

## Product outcome

Mobile API Studio becomes especially valuable to teams shipping parallel iOS and Android clients.

## 5.1 Session comparison

Compare two sessions or subsets:

```text
iOS session <-> Android session
before change <-> after change
working user <-> failing user
```

Match flows using:

- endpoint key
- method
- relative ordering
- request fingerprints
- optional SDK trace IDs

## 5.2 Flow diff

Show structured differences:

- URL/query
- headers
- JSON bodies
- status
- response schema
- duration
- missing/extra calls
- call ordering

Secrets remain redacted in diffs unless explicitly revealed.

## 5.3 Schema drift detection

Infer lightweight JSON shapes from observed responses.

Example:

```text
/users/{id}
  iOS observed:  name:string, tier:string, flags:[string]
  Android observed: name:string, flags:[string]

Potential mismatch: `tier` missing from Android response/session
```

Treat this as observed-session analysis, not authoritative API schema generation.

## 5.4 AI debugging — opt-in only

Useful prompts:

- “Why is this flow slower on Android?”
- “Explain why iOS got 401 and Android got 200.”
- “Which request most likely blocks this screen?”
- “Generate a mock for the failure state.”
- “Summarize network differences between these two sessions.”

Architecture:

```text
Selected local flows
 -> redact secrets
 -> build bounded diagnostic context
 -> user preview/consent
 -> provider adapter
 -> structured diagnostic result
```

Provider support should be adapter-based. BYOK keys live in OS secure storage.

AI is never required for core capture/replay/mock functionality.

## 5.5 Deterministic analysis before AI

Implement normal algorithms first:

- longest request
- critical path approximation
- sequential vs overlapping call groups
- duplicate requests
- retry storms
- 4xx/5xx clusters
- schema diff
- header/body diff

AI receives these computed facts rather than raw traffic alone.

## v0.5 acceptance criteria

- Two sessions can be paired and endpoint matches reviewed.
- Structured iOS/Android request/response diff works without AI.
- AI analysis is opt-in, redacted, bounded, and provider-independent.
- The user can inspect exactly which data is being sent externally.

---

# 6. Data model

A starting model follows. Exact columns belong in migrations, not this document.

## CaptureSession

```text
id
name
started_at
ended_at
device_id
app_id?
connection_strategy
capture_engine
notes?
created_at
```

## Flow

```text
id
session_id
source: proxy | replay | mock | sdk
parent_flow_id?
method
url
scheme
host
port
path
endpoint_key?
status_code?
started_at
completed_at?
duration_ms?
request_body_ref?
response_body_ref?
error_code?
mock_rule_id?
trace_id?
```

## Header

```text
flow_id
side: request | response
name
value_encrypted_or_plain
is_sensitive
ordinal
```

Sensitive values should be handled deliberately; UI queries should return redacted values by default.

## Body

```text
id
sha256
content_type
encoding
byte_size
stored_path
preview_text?
is_binary
is_truncated
```

## Device

Persist only useful historical identity, not every volatile device field.

## ReplayDraft

```text
id
parent_flow_id?
method
url
headers
body_ref?
environment_id?
created_at
updated_at
```

## MockRule

Added in v0.3.

---

# 7. Event model

Tauri event traffic should be compact. Do not send complete multi-megabyte bodies through UI events.

Recommended live events:

```text
session.updated
flow.started
flow.updated
flow.completed
flow.failed
connection.status
connection.diagnostic
mock.hit
sdk.context
```

React fetches full details on demand through Tauri commands.

---

# 8. UI information architecture

## Connect screen

```text
Devices
--------------------------------
iPhone 17 Pro Simulator   Booted
Pixel 9 API 36            Online

Connection status
--------------------------------
Capture engine    Ready
Certificate       Installed
Routing           Not configured

[ Connect ]
```

## Main traffic screen

```text
┌ Sessions/Filters ┬ Traffic Timeline ┬ Inspector ┐
│                  │                  │           │
│ devices          │ GET /products    │ Overview  │
│ sessions         │ POST /cart       │ Request   │
│ hosts            │ GET /offers      │ Response  │
│ saved filters    │ ...              │ Timing    │
└──────────────────┴──────────────────┴───────────┘
```

Responsive priorities:

1. traffic list must remain fast;
2. JSON viewer must handle large payloads safely;
3. live capture must not force-scroll when inspecting an old request;
4. keyboard navigation should be first class.

Useful shortcuts later:

```text
Cmd/Ctrl+K  search
Cmd/Ctrl+R  replay selected
Cmd/Ctrl+F  filter
Space       pause/resume UI stream
```

---

# 9. Performance targets

Treat these as engineering targets, not promises.

- 10,000 flows/session usable on a typical development laptop.
- Timeline virtualized; no DOM row per entire session.
- Persist flow metadata asynchronously in batches.
- UI should receive summaries, not whole bodies.
- Default text body preview cap: 1 MB.
- Default stored body cap: configurable; start around 10 MB per body with truncation metadata.
- Binary preview generated lazily.
- Search indexes created after measurement, not guessed excessively.

---

# 10. Error taxonomy

Every connection failure should map to a typed category:

```text
PrerequisiteMissing
DeviceUnavailable
PortUnavailable
CaptureEngineStartFailed
ProxyConfigurationFailed
CertificateInstallFailed
CertificateNotTrusted
NoTrafficObserved
ProxyIgnoredByClient
PinnedCertificateSuspected
NetworkOffline
ReplayDnsError
ReplayTlsError
ReplayTimeout
StorageFailure
```

UI should show human text, but logs retain structured error code and causal chain.

---

# 11. Observability for the debugger itself

Local logs:

- structured JSON log files;
- rolling retention;
- diagnostic bundle export;
- sidecar stdout/stderr captured separately;
- no request bodies in app logs by default.

A “Create diagnostic bundle” action should include versions, device discovery output, redacted settings, connection diagnostics, and application logs — not captured secrets.

---

# 12. Testing strategy

## Unit tests

- URL/endpoint normalization
- secret redaction
- cURL generation
- flow merge/state transitions
- filter parser
- mock matcher
- JSON diff
- session matcher

## Integration tests

- fixture server + capture engine
- HTTPS trust using test CA
- replay against fixture server
- persistence/reopen
- sidecar crash/restart

## Device adapter tests

Use recorded command-output fixtures for most CI tests.

Actual simulator/emulator tests run in a smaller platform matrix:

- macOS: iOS Simulator smoke tests
- macOS/Linux: Android Emulator where CI capacity allows

## UI E2E

Seed a deterministic 1k/10k flow database and test:

- scroll
- filter
- inspect
- replay creation
- mock creation later

---

# 13. Release policy

Before public open source:

- choose license;
- document bundled third-party software and licenses;
- threat-model CA/key handling;
- add SECURITY.md reporting process;
- audit update mechanism;
- ensure exports redact secrets by default;
- verify clean uninstall/connection rollback behavior.

Pre-alpha builds should clearly warn that the tool can install trusted development CAs and alter local proxy settings.

---

# 14. Immediate build order

After repository setup, implement in this exact order:

1. Tauri/React shell.
2. Shared Rust models.
3. SQLite session/flow storage.
4. Fixture API server.
5. Fake capture event stream displayed in UI.
6. `CaptureEngine` trait.
7. mitmdump addon + Rust sidecar bridge.
8. Desktop browser/cURL capture smoke test.
9. iOS Simulator discovery.
10. Android Emulator discovery.
11. CA manager.
12. iOS connection strategy.
13. Android connection strategy.
14. Connection Doctor.
15. request inspector.
16. safe cURL exporter.
17. replay editor/engine.
18. v0.1 hardening and release.

This order avoids spending the first weeks fighting simulator certificates before the capture/storage/UI pipeline is proven.
