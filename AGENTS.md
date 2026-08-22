# Mobile API Studio — Project Working Rules

These rules apply to **all implementation phases** unless the repository owner explicitly changes them.

## Implementation-first policy

1. **Do not write automated tests during Phases 0–5.**
2. **Do not add or run CI/testing workflows during Phases 0–5.**
3. **Do not stop feature implementation to create test harnesses, test matrices, benchmark suites, fixture-only tests, or coverage work.**
4. Focus on completing the production implementation phase by phase: architecture, application code, integrations, UI, persistence, capture, replay, mocking, SDKs, comparison, and AI features.
5. Testing and final validation will be performed **independently by the repository owner after implementation is complete**.
6. Test/CI items mentioned in older planning documents are therefore **deferred** and must not block or expand current implementation work.
7. It is still acceptable to write normal production safeguards such as input validation, typed errors, rollback logic, diagnostics, and logging; these are product code, not testing work.
8. Keep code organized so testing can be added later without large architectural rewrites.

## Phase order

Implement in this order:

1. Phase 0 — Foundation
2. Phase 1 — v0.1 Capture / Inspect / Replay
3. Phase 2 — v0.2 Daily Debugger
4. Phase 3 — v0.3 Mocking
5. Phase 4 — v0.4 App-aware SDKs
6. Phase 5 — v0.5 Compare + AI
7. Final owner-led testing/validation after implementation

When a phase contains both implementation work and a testing/verification item, complete the implementation work and defer the testing/verification item to the final validation stage.
