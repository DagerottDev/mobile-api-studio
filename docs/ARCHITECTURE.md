# Architecture

## 1. System overview

```text
                 ┌───────────────────────────────┐
                 │      Tauri Desktop App        │
                 │ React UI + Rust commands      │
                 └───────────────┬───────────────┘
                                 │
                    ┌────────────┴─────────────┐
                    │        Rust Core         │
                    │ sessions / storage / UI │
                    └──────┬─────┬─────┬──────┘
                           │     │     │
             ┌─────────────┘     │     └─────────────┐
             ▼                   ▼                   ▼
      Device Manager       Capture Engine       Replay Engine
      iOS / Android        abstraction          HTTP client
             │                   │
       simctl / adb        initial adapter
                                 │
                            mitmdump sidecar
                                 │
                          normalized events
                                 │
                 ┌───────────────┴───────────────┐
                 │    iOS Simulator / Android    │
                 │          Emulator             │
                 └───────────────────────────────┘
```

## 2. Architectural boundaries

### UI layer

Owns rendering and transient interaction state only.

The UI should not:

- execute ADB/simctl directly;
- manage CA files;
- spawn the capture process;
- write SQLite directly;
- hold raw secret values longer than necessary.

### Application/core layer

Owns workflows:

- connect/disconnect;
- session lifecycle;
- flow queries;
- replay;
- mock management;
- export;
- comparison.

### Adapter layer

Wraps external/system dependencies:

- ADB
- `simctl`
- macOS proxy/system configuration
- mitmdump
- OS keychain/credential store

Each adapter returns typed capabilities and errors.

## 3. Rust crate responsibilities

### `core-model`

Pure types and validation. No platform-specific imports.

### `capture-core`

Interfaces, capture event normalization, lifecycle state machine.

### `capture-mitm`

- find/launch managed mitmdump;
- version compatibility checks;
- decode sidecar events;
- heartbeat;
- graceful shutdown;
- translate sidecar failures into core errors.

### `device-ios`

- detect Xcode tools;
- list booted simulators;
- CA installation;
- app/runtime metadata;
- capability checks.

### `device-android`

- find ADB;
- list emulators;
- read runtime metadata;
- proxy strategy;
- clear/rollback;
- certificate diagnostics.

### `storage`

- SQLite migrations;
- session/flow repositories;
- body store;
- retention;
- full-text/search helper later.

### `replay`

- draft validation;
- environment interpolation;
- secret resolution;
- async HTTP execution;
- replay result normalization.

### `mock-engine`

Introduced in v0.3. Rule matching remains deterministic and independent of UI.

### `diff-engine`

Introduced in v0.5. JSON/header/session matching and drift analysis.

## 4. Capture data path

```text
network request
  -> proxy engine
  -> addon flow callback
  -> normalized event
  -> capture-mitm decoder
  -> core flow accumulator
  -> storage writer
  -> summary Tauri event
  -> React timeline
```

Request/response bodies should not be copied repeatedly across every layer.

Preferred approach:

- small bodies: inline up to a low threshold;
- large bodies: temporary file/body store reference;
- UI fetches body on demand.

## 5. Sidecar protocol

Use a versioned protocol from the first commit.

Envelope:

```json
{
  "schema_version": 1,
  "event_id": "uuid",
  "event_type": "flow.response.complete",
  "timestamp": "2026-08-22T10:00:00Z",
  "payload": {}
}
```

Rules:

- one JSON object per frame;
- unknown fields ignored;
- unknown event types logged, not fatal;
- sidecar sends startup capability event;
- protocol version mismatch blocks connection with a clear message.

For v0.1 JSONL is acceptable. If event volume becomes a bottleneck, move to a local domain socket/MessagePack without changing domain models.

## 6. Device strategy abstraction

```text
DeviceProvider
  list_devices()
  get_capabilities(device)
  prepare(device, capture_endpoint)
  verify(device)
  rollback(device)
```

A `prepare` call returns both status and reversible mutations.

### Why capability-based?

Mobile networking differs by:

- OS version;
- emulator image;
- rootability;
- app networking stack;
- CA trust rules;
- proxy awareness;
- corporate/VPN software.

Hard-coded “Android always does X” or “Simulator always does Y” logic will fail quickly.

## 7. Connection transaction

Treat connection as a transaction:

```text
begin
  validate prerequisites
  snapshot mutable settings
  start capture engine
  ensure CA
  configure routing/proxy
  verify traffic
commit connection
```

On any failure:

```text
rollback mutations in reverse order
stop engine
report typed diagnostic
```

Persist a minimal recovery record before mutating system settings. Remove it after successful rollback.

## 8. Storage architecture

SQLite is the source of truth for metadata.

Large body files should be addressed by SHA-256 so identical bodies can share storage later.

Potential file layout:

```text
bodies/ab/cd/<sha256>.body
```

Retention controls:

- max total disk use;
- max age;
- pinned sessions excluded from cleanup;
- explicit “delete bodies but keep metadata” future option.

## 9. Query API to frontend

Commands should be task-oriented:

```text
list_devices
connect_device
disconnect
list_sessions
query_flows
get_flow_detail
get_body_preview
create_replay_draft
execute_replay
copy_curl
```

Avoid exposing raw SQL-like APIs to the frontend.

## 10. Frontend state

Recommended split:

- persisted server-like state queried from Rust: sessions, flows, details;
- transient UI state: selected flow, active tabs, draft filters, panel sizes.

Do not mirror the entire SQLite database into a global JavaScript store.

## 11. Secrets architecture

Three classes of data:

### Normal

Host, path, status, duration.

### Potentially sensitive

Request/response body and headers.

### Explicit secrets

Authorization tokens, cookies, API keys, environment secrets.

Default API to frontend returns redacted secret values. Revealing a value is an explicit operation.

Environment secret variables should be stored using OS secure credential storage.

## 12. Plugin/extensibility direction

Do not create a plugin marketplace early. Instead define internal extension points:

```text
CaptureEngine
DeviceProvider
BodyRenderer
ProtocolDecoder
ExportFormatter
AIProvider
```

Only stabilize a public plugin API after v0.5 usage shows which interfaces are genuinely reusable.

## 13. Protocol roadmap

### v0.1

- HTTP/1
- HTTPS
- HTTP/2 as supported by capture engine

### v0.2+

- WebSocket inspector

### Later

- GraphQL conveniences are UI/schema features on top of HTTP.
- gRPC requires deliberate HTTP/2 + protobuf tooling and should be a separate milestone.
- HTTP/3/QUIC should not block the initial product.

## 14. Platform roadmap

### macOS first

Required because iOS Simulator development requires macOS/Xcode.

### Windows/Linux later

Android-only workflows can become cross-platform once the core product is stable. Tauri and the capture abstraction make this feasible.

Do not force cross-platform packaging into v0.1 if it slows the iOS+Android macOS workflow.
