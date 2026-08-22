# Contributing

The repository is private during the early build, but changes should still follow production-quality workflow.

## Branching

Use short-lived branches:

```text
feat/ios-device-discovery
feat/capture-engine
fix/proxy-rollback
```

## Pull requests

Every PR should include:

- problem/goal;
- implementation summary;
- test evidence;
- platform tested;
- screenshots for UI changes;
- security implications when touching certificates, secrets, proxy settings, storage, or exports.

## Definition of done

- tests added/updated;
- no secrets in logs/fixtures;
- typed errors rather than string matching where practical;
- rollback path for system/device mutations;
- docs updated if behavior/architecture changes.

## Commit style

Conventional-style commits are recommended:

```text
feat(ios): discover booted simulators
fix(capture): terminate sidecar on disconnect
test(replay): cover redacted authorization header
docs(roadmap): refine v0.2 gate
```
