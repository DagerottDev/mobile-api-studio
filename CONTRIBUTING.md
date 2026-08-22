# Contributing

The repository is currently private. Production changes should still be made on short-lived branches with focused pull requests.

## Project-stage note

Implementation Phases 0–5 are complete. During those phases, the repository owner explicitly deferred automated tests, CI, benchmark suites, and formal verification until the end of implementation.

The current next stage is **independent owner-led validation**. Do not retroactively claim test evidence for the implementation PRs.

## Branching

Use focused branch names such as:

```text
fix/validation-proxy-rollback
fix/validation-replay-body
feat/post-v05-physical-device
security/import-hardening
```

## Pull requests

A PR should include the information relevant to its stage.

### Validation/fix PRs

Include:

- observed problem and reproduction context;
- implementation/fix summary;
- affected platform/workflow;
- owner validation performed, if any;
- screenshots for UI changes when useful;
- security/privacy implications when touching certificates, secrets, proxy settings, storage, SDK transport, exports, or AI context.

### Future feature PRs

Include:

- goal/scope;
- architecture impact;
- implementation summary;
- compatibility/migration impact;
- documentation changes;
- validation expectations agreed for that future cycle.

## Definition of done for the current validation cycle

A fix/change is ready when:

- the reported implementation defect is addressed;
- no credentials/secrets are intentionally added to logs or fixtures;
- typed errors/diagnostics are preserved where practical;
- rollback behavior remains explicit for system/device mutations;
- security/privacy boundaries are not weakened silently;
- docs are updated when observable behavior or architecture changes;
- the repository owner decides whether that change has been sufficiently validated.

Automated tests are **not automatically required by this document** for the current cycle because final validation remains owner-controlled. If the owner later changes the project testing policy, `AGENTS.md` and this file should be updated together.

## Commit style

Conventional-style commits are recommended:

```text
fix(capture): restore emulator proxy after failed connect
fix(ai): redact token query parameters in preview
docs(architecture): record SDK correlation boundary
feat(device): add post-v0.5 physical Android support
```
