# ADR 0001 — Tauri 2 + React + Rust

## Status

Accepted for initial implementation.

## Context

The desktop debugger needs a rich data-heavy UI plus direct access to local processes, ADB, simctl, certificates, ports, files, SQLite, and later secure storage.

## Decision

Use:

- Tauri 2 desktop shell;
- React + TypeScript + Vite frontend;
- Rust for application/core/system integration.

## Consequences

### Positive

- small native desktop footprint compared with a full Electron runtime;
- Rust is suitable for process/network/storage orchestration;
- React ecosystem is strong for virtualized tables/editors;
- architecture can later support Windows/Linux Android workflows.

### Negative

- two-language application boundary;
- Tauri permission/capability configuration must be maintained carefully;
- some platform integrations still require Swift/shell/OS-specific behavior.

## Guardrail

Keep business/domain logic in Rust crates and keep React focused on UI state and rendering.
