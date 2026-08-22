# Security and Privacy

Mobile API Studio handles authentication headers, cookies, customer/test payloads, and a locally trusted development CA. Security is therefore a core product requirement rather than cleanup work.

## 1. Local-first default

Captured requests, responses, session metadata, certificates, and logs remain local unless the user explicitly exports or sends selected redacted context to an AI provider.

No telemetry should include captured URLs, headers, or bodies.

## 2. CA private key

- Generate uniquely per installation.
- Store only in application-owned data directory.
- Restrict file permissions to current user where supported.
- Never export the private key through normal UI.
- Provide rotation/regeneration.
- Clearly distinguish installing a public root certificate from exposing the private key.

## 3. Proxy/system mutations

Before changing proxy settings:

1. read and persist prior state;
2. apply the minimum required mutation;
3. record rollback action;
4. restore on disconnect;
5. attempt recovery on next launch if prior shutdown was abnormal.

Never assume “no proxy” was the previous state.

## 4. Secret redaction

Default-sensitive header names include case-insensitive variants of:

```text
Authorization
Proxy-Authorization
Cookie
Set-Cookie
X-API-Key
X-Auth-Token
```

Allow user-defined sensitive headers.

Default exports, cURL copy, diagnostic bundles, search previews, and AI context use redacted values.

## 5. Body handling

Bodies may contain credentials or personal/test data.

- avoid logging body content in debugger logs;
- lazy-load large bodies;
- truncate by configurable cap;
- retain `is_truncated` metadata;
- allow per-host “do not store bodies” rule later;
- support clearing bodies from a session.

## 6. AI boundary

AI analysis is opt-in.

Before sending:

```text
selected flows
 -> secret-header redaction
 -> body redaction rules
 -> size limit
 -> user-visible context summary
 -> provider
```

Provider API keys belong in OS secure credential storage.

Core debugging functionality must continue to work with AI disabled.

## 7. Certificate pinning policy

Mobile API Studio must not market or implement silent pinning bypass as a normal connection feature.

When pinning is suspected:

- identify the likely cause;
- explain that a generic MITM CA cannot be trusted by a pinned client;
- recommend debug configuration for apps the developer controls;
- offer the v0.4 SDK path for app-level observability.

## 8. Release-build SDK policy

SDKs introduced in v0.4 must make production inclusion difficult by accident.

Preferred patterns:

- debug-only dependency examples;
- no-op release artifact where appropriate;
- runtime off by default;
- explicit endpoint pairing;
- no remote listener exposed outside local development context.

## 9. Network listener

Capture/control listeners should bind to loopback unless a selected mobile runtime specifically requires host reachability.

If binding to a wider interface is required:

- use random high port;
- authenticate SDK/control channel;
- show an explicit UI indicator;
- do not expose an unauthenticated control API to the LAN.

## 10. Diagnostic bundles

May include:

- app version;
- OS version;
- capture engine version;
- sanitized device metadata;
- error codes;
- redacted configuration;
- application logs.

Must exclude by default:

- CA private key;
- Authorization/Cookie values;
- raw captured bodies;
- environment secrets.

## 11. Threat-model questions before public release

- Can another local user steal the CA key?
- Can a LAN host access the capture/control port?
- Can malicious captured JSON trigger unsafe UI rendering?
- Can a replay request read arbitrary local files through body references?
- Can export/import path traversal overwrite files?
- Can a malicious sidecar event cause unbounded memory use?
- Does crash recovery always restore modified proxy settings?
- Does uninstall leave trusted CA certificates behind without warning?

These questions should become security tests, not only documentation.
