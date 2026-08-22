# Mobile API Studio — Project Working Rules

These rules apply unless the repository owner explicitly changes them.

## Current project stage

Implementation Phases 0–5 are **complete and merged to `main`**.

The project is now in the separate **owner-led final validation** stage. The repository owner decides when/how to run manual tests, automated tests, CI, benchmarks, compatibility checks, and release validation.

## Implementation-first policy record

The completed implementation cycle followed these rules:

1. Automated tests were not written as part of Phases 0–5.
2. CI/testing workflows were not used as implementation phase gates.
3. Feature work was not paused to build test harnesses, benchmark suites, fixture-only tests, or coverage work.
4. Production safeguards such as typed errors, validation, rollback logic, diagnostics, redaction, and bounded inputs were still part of implementation.
5. Testing and final validation were intentionally deferred until after implementation completion.

Do not rewrite project history to imply those phases were formally tested when they were not.

## Completed phase order

1. ✅ Phase 0 — Foundation
2. ✅ Phase 1 — v0.1 Capture / Inspect / Replay
3. ✅ Phase 2 — v0.2 Daily Debugger
4. ✅ Phase 3 — v0.3 Mocking
5. ✅ Phase 4 — v0.4 App-aware SDKs
6. ✅ Phase 5 — v0.5 Compare + AI
7. ➡️ Final owner-led testing/validation

## Rules for the current validation cycle

- Treat Phases 0–5 as implementation-complete unless an observed defect requires a fix.
- New fixes should be tied to concrete validation findings rather than speculative refactors.
- Do not add CI/test infrastructure automatically unless the repository owner requests it.
- Preserve security boundaries around CA material, secrets, proxy rollback, SDK correlation, import/export, and AI context.
- Keep documentation honest about what is implemented vs what is independently verified.
- Keep future changes modular so a testing strategy can be added later without architectural rewrites.

See `docs/FINAL_VALIDATION.md` for the current validation checklist.
