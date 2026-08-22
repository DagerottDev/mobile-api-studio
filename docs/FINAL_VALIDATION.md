# Final Owner-Led Validation

> This stage begins **after** implementation Phases 0–5. It is intentionally independent of the implementation phase gates.

The repository owner controls how much of this is performed manually, with scripts, or with future automated tooling. This document does **not** add tests or CI by itself; it is a validation checklist and defect-triage guide.

## 1. Validation goals

The final pass should answer four questions:

1. Can the desktop app build and launch on the intended macOS development machine?
2. Do the supported iOS Simulator and Android Emulator workflows behave correctly end to end?
3. Are security/privacy/rollback boundaries preserved during real use and failures?
4. Which defects or unsupported runtime combinations must be fixed/documented before any release decision?

## 2. Record the validation environment

Before testing, record:

```text
macOS version
hardware architecture
Rust version
Node/pnpm version
Tauri/toolchain versions
Xcode version
selected iOS Simulator runtime/device
Android SDK / platform-tools version
selected Android Emulator API/image
mitmproxy/mitmdump version/path
app build/commit SHA
```

When filing a validation issue, include the relevant subset.

---

# 3. Desktop startup and local persistence

Validate:

- desktop app launches successfully;
- Rust health command reports a usable application-data/database path;
- application data directory is created correctly;
- SQLite opens/migrates without destructive data loss;
- body-store directory is writable;
- restarting the app preserves historical sessions/settings expected to persist;
- invalid/corrupted local data produces a useful diagnostic rather than an unexplained crash where practical.

## Suggested defect examples

```text
Validation: app fails to start on clean machine
Validation: database migration fails after upgrading from previous local state
Validation: body store is not readable after restart
```

---

# 4. iOS Simulator connection workflow

Validate on at least one booted Simulator runtime you intend to support:

- Simulator is discovered with correct name/runtime/state;
- Connect starts the capture engine;
- development CA is created/located and installed through `simctl`;
- full-trust guidance is accurate for the selected runtime;
- configured Simulator traffic reaches the proxy;
- plain HTTP and normal HTTPS from a non-pinned debug/test app are observable;
- disconnect stops the capture session cleanly;
- stale rollback/recovery messaging is understandable after an interrupted session;
- pinned/untrusted traffic produces a useful diagnostic rather than promising a bypass.

Document any Xcode/iOS runtime where behavior differs.

---

# 5. Android Emulator connection workflow

Validate on at least one intended Android Emulator image/API level:

- ADB emulator discovery works;
- physical devices are not accidentally treated as supported emulators;
- current global proxy is read correctly;
- Connect applies the expected host alias/port;
- development app CA/network-security guidance is sufficient for HTTPS capture;
- capture works for a non-pinned debug/test app;
- disconnect restores the exact prior proxy state;
- a failed/aborted connection also restores prior proxy state through recovery;
- an app that ignores the system proxy is reported as such rather than misclassified;
- Play Store/non-rootable/user-CA limitations produce understandable diagnostics.

High-priority failure case: verify prior proxy restoration when the previous value was **not** empty.

---

# 6. Traffic capture and Inspector

Validate representative traffic:

- GET/POST/PUT/PATCH/DELETE as available;
- query parameters;
- repeated headers;
- Authorization/Cookie/API-key headers;
- JSON request/response bodies;
- plain text;
- empty bodies;
- binary/image body metadata;
- redirects/errors;
- larger/truncated bodies.

Check that:

- timeline inserts remain usable during capture;
- session/flow association is correct;
- status/duration/size timestamps look credible;
- Inspector loads request/response/timing data correctly;
- binary/truncated bodies are not silently represented as complete text;
- sensitive values are redacted where the UI/export contract says they are.

---

# 7. Search, sessions, collections, and environments

Validate:

- historical sessions reopen without capture running;
- session rename/notes/archive/delete behavior;
- cross-session text/method/status/source filtering;
- endpoint normalization for numeric IDs, UUIDs, and long hex-like IDs;
- saving a captured request into a collection;
- collection/request ordering;
- opening a saved request in Replay;
- active environment selection;
- `{{variable}}` substitution in URL/headers/body;
- secret environment values are retrieved from secure storage and are not visible in SQLite/export payloads;
- removing/changing a secret behaves predictably.

---

# 8. Workspace import/export

Use non-production sample data.

Validate:

- export produces a readable versioned bundle;
- sensitive headers are redacted according to policy;
- secret environment values are absent;
- AI API key is absent;
- CA private-key material is absent;
- body data needed for restored capture sessions is preserved as intended;
- merge import does not destroy unrelated local workspace data;
- replace import behaves as documented;
- destructive replace is rejected during an active capture if applicable;
- imported secrets clearly require re-entry;
- malformed/untrusted bundle failures do not overwrite arbitrary paths or crash silently.

---

# 9. Replay

Validate from both captured and saved requests:

- draft correctly copies allowed method/URL/headers/body;
- internal Mobile API Studio correlation header is not sent;
- sensitive captured headers remain protected according to the Replay contract;
- URL/query/header/body edits are applied;
- active environment variables resolve at send time;
- text and representative binary bodies behave correctly;
- truncated captured body cannot be replayed as though complete unless intentionally edited/replaced;
- response/error is shown and persisted as `source = replay`;
- browser CORS does not affect native Replay;
- timeout/DNS/TLS failures produce useful errors.

---

# 10. Mocks and fixtures

Validate:

- create mock from a complete captured response;
- truncated responses are not silently turned into incomplete mocks;
- rule ordering/priority;
- enable/disable behavior;
- status override;
- body override;
- response header add/remove/replace;
- JSON-pointer mutation;
- latency;
- timeout/drop;
- reusable fixture creation/apply/delete;
- invalid base64 body is rejected before sidecar execution;
- mocked traffic is visibly classified as `mock`;
- “Disable all mocks” immediately restores pass-through behavior.

---

# 11. Request and response breakpoints

Validate:

- request breakpoint pauses before upstream dispatch;
- response breakpoint pauses before delivery after mock transforms;
- pending breakpoint appears in desktop UI;
- method/URL/header/body/status edits apply correctly for the supported breakpoint side;
- leaving body untouched preserves the original full body even if the UI preview is truncated;
- explicit body clear/replace is respected;
- continue works;
- cancel/drop behavior works as documented;
- unresolved breakpoint auto-continues after the bounded timeout;
- stale pending/decision files are cleaned without deleting active valid pauses.

---

# 12. App-aware iOS SDK

Using the sample or another debug app, validate:

- disabled SDK does not mutate requests;
- explicit enable sends handshake;
- screen/feature/context events appear;
- structured logs appear;
- manual request instrumentation attaches a correlation ID;
- URLSession instrumentation attaches a correlation ID only when enabled;
- SDK telemetry itself does not recursively appear as captured app traffic;
- proxy captures/removes the internal correlation header before the real backend receives it;
- Traffic Inspector shows app/screen/feature/source/log context;
- capture session gains app attribution from correlated SDK traffic.

---

# 13. App-aware Android SDK

Using the sample or another debug app, validate:

- disabled SDK/OkHttp interceptor is true pass-through;
- explicit enable sends handshake through the emulator host path;
- context/log events appear;
- OkHttp correlation works;
- manual/custom-client instrumentation works;
- SDK transport does not loop through the intercepted OkHttp/proxy path;
- app-supplied source metadata is not replaced with irrelevant OkHttp framework frames;
- proxy removes internal correlation metadata before upstream delivery;
- desktop app/session/flow enrichment works.

---

# 14. Session Compare

Capture two representative completed sessions, ideally one iOS and one Android flow.

Validate:

- baseline/candidate session selection;
- same-session rejection;
- normalized endpoint grouping;
- repeated calls align in expected occurrence order;
- baseline-only and candidate-only calls are identified correctly;
- method/query/header diffs are accurate;
- sensitive headers remain redacted in comparison output;
- response status/body differences are accurate;
- JSON added/removed/type-change paths are useful and not misleading;
- timing/size deltas use the correct direction (baseline → candidate);
- SDK app/screen/feature/source differences appear when available.

## Deterministic diagnostics

Validate examples that contain:

- duplicate calls;
- probable retries;
- deliberately slow calls;
- 4xx/5xx/errors;
- overlapping requests;
- clearly sequential requests.

Confirm diagnostics are evidence, not claims of causal certainty.

---

# 15. Optional AI workflow

Only use non-sensitive test data for initial validation.

Validate:

- provider settings can be saved;
- API key is stored in Keychain/secure store and not SQLite;
- no AI key appears in workspace export;
- model/provider settings are reflected in preview/send metadata;
- session comparison context can be previewed without sending;
- individual flow context can be previewed without sending;
- Authorization/Cookie/API-key/internal correlation headers are absent/redacted;
- configured JSON secret keys are redacted recursively;
- configured secret-named query parameters/URL values are redacted;
- body/string/context limits are applied;
- displayed SHA-256 fingerprint corresponds to the preview;
- modifying source evidence after preview forces a new preview rather than sending stale authorization;
- external request occurs only after explicit user action;
- OpenAI request uses `store: false`;
- provider failure is recoverable and deterministic Compare remains usable;
- successful result is stored locally with task/target/provider/model/context fingerprint;
- AI history does not store the API key.

---

# 16. Connection Doctor and diagnostics

Validate healthy and intentionally broken states:

- missing mitmdump/custom path;
- capture port unavailable;
- no Simulator/Emulator;
- ADB unavailable;
- CA/trust problem;
- system proxy ignored;
- capture engine start failure;
- no traffic vs HTTPS trust failure distinction;
- SDK ingestion reachable/unreachable states.

Diagnostics should give the developer a concrete next action where possible.

---

# 17. Security-focused manual checks

Before any public release consideration, manually inspect at least:

- local CA/private-key permissions and location;
- Android capture listener exposure on the host;
- Keychain entries and deletion/update behavior;
- import path handling;
- malicious HTML/script-like strings rendered in bodies/logs;
- unusually large/malformed sidecar events;
- proxy rollback after forced termination;
- SDK accidental enablement in a release-like build;
- AI context generated from payloads with nested/encoded secrets.

A dedicated security review can be deeper than this checklist.

---

# 18. Performance observations

The implementation intentionally did not claim benchmark certification. During validation, observe at minimum:

- a small session;
- a medium session;
- a large session approaching the original 10k-flow engineering target.

Record:

```text
capture ingestion responsiveness
Traffic scrolling/selection behavior
Inspector body-open latency
search/filter latency
SQLite growth
body-store growth
Compare computation time
AI preview construction time for larger selected evidence
```

If performance is unacceptable, file concrete measured defects rather than immediately introducing a benchmark/optimization project without a failing scenario.

---

# 19. Defect triage

For each validation defect, record:

```text
Title
Commit/build
Platform/runtime
Prerequisites
Steps to reproduce
Expected behavior
Actual behavior
Relevant diagnostics/log excerpts (redacted)
Screenshots if useful
Severity
Regression? yes/no/unknown
```

Suggested severities:

- **Blocker:** data loss, security boundary failure, unrecoverable proxy/device mutation, app cannot start/use core workflow.
- **High:** major supported capture/replay/mock/compare/SDK path broken.
- **Medium:** important feature incorrect but workaround exists.
- **Low:** polish, messaging, minor layout/ergonomics.

---

# 20. Exit decision

Owner-led validation can be considered complete when the repository owner is satisfied that:

- supported target environments are documented;
- blocker/high defects are fixed or explicitly accepted;
- proxy/device recovery behavior is understood;
- secret/AI boundaries have been inspected with representative data;
- known limitations are documented;
- the next release/private-use decision is intentional.

This checklist does not require the owner to create automated tests or CI unless they choose to do so.
