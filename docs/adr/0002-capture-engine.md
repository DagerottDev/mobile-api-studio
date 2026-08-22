# ADR 0002 — Abstract Capture Engine; start with mitmdump

## Status

Accepted for v0.1, subject to spike validation.

## Context

Building a production-grade TLS interception proxy, certificate lifecycle, HTTP/2 support, and edge-case handling from scratch would dominate the first release and delay validation of the actual product UX.

## Decision

Define a Rust `CaptureEngine` interface and implement v0.1 with a mitmdump sidecar plus a small event-bridge addon.

## Why

- mature interception behavior;
- fast path to HTTP(S) capture;
- existing support for important web protocols/modes;
- MIT-licensed upstream project;
- clear process boundary.

## Consequences

### Positive

- product work can focus on device connection, session UX, replay, mocking, comparison;
- sidecar can be replaced later without rewriting UI/storage models.

### Negative

- Python/sidecar packaging complexity;
- startup/process health must be managed;
- product behavior can be affected by upstream version changes.

## Mitigation

- versioned sidecar protocol;
- compatibility test against supported mitmproxy versions;
- managed process lifecycle;
- keep all domain models outside the Python addon;
- plan a future native engine only if profiling/product needs justify it.
