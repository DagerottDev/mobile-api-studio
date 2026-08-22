# AI Privacy and Provider Architecture

Phase 5 adds optional AI explanations on top of deterministic Mobile API Studio evidence. AI is never required for capture, replay, mocking, SDK correlation, or session comparison.

## Design rules

1. Deterministic comparison remains the source of truth.
2. The user must explicitly preview AI context before an external request is allowed.
3. The send command recomputes the context and verifies its SHA-256 fingerprint against the preview. If anything changed, the request is rejected and a fresh preview is required.
4. Provider API keys are stored in the OS credential store. They are not stored in SQLite, project exports, diagnostics, or AI-result rows.
5. AI results are stored locally with task, source reference, provider, model, context fingerprint, optional remote response ID, output text, and timestamp.
6. Provider calls are optional and never block deterministic comparison.

## Provider abstraction

`crates/ai-core` exposes an `AiProvider` interface. The initial provider is OpenAI, implemented with the Responses API:

- endpoint: `POST https://api.openai.com/v1/responses`
- authentication: Bearer API key from the OS credential store
- `store: false`
- model is user-configurable; the initial default is `gpt-5.6-luna`
- no web search, file search, tools, background mode, or remote state is enabled by Mobile API Studio

Adding another provider should implement the same interface and must preserve the local preview/redaction/fingerprint gate.

## Context construction

### Session comparison

The AI receives the deterministic `SessionComparison` output rather than both raw sessions. This includes:

- normalized endpoints and aligned call occurrences;
- missing/extra calls;
- request query/header/body differences;
- response status/header/body differences;
- JSON shape/type drift;
- timing and size changes;
- duplicate/retry, slow-request, error-cluster, and waterfall diagnostics;
- SDK app/screen/feature/source evidence where correlated.

### Flow diagnosis

The AI receives one captured `FlowDetail`, text request/response body material where available, and correlated SDK app context.

## Redaction

Redaction happens before preview generation, so the UI displays the same sanitized data that can later be sent.

The sanitizer:

- removes/omits the internal `X-Mobile-API-Studio-Request-Id` correlation header;
- redacts headers already marked sensitive;
- redacts known authentication/cookie/token headers by name;
- recursively parses JSON-looking body strings and redacts configured secret JSON keys;
- redacts named query/header comparison values whose key is configured as sensitive;
- redacts configured secret parameters in raw query strings and URLs;
- caps individual strings;
- caps the complete external context size.

Default secret keys include password, token, access/refresh/id token, secret, client secret, API key, authorization, cookie, session, JWT, bearer, private key, and credential variants. Users can change the list in Settings.

## Context limits

The initial policy caps:

- each string at 12,000 characters;
- complete sanitized context at 120,000 bytes.

If the overall context exceeds the cap, Mobile API Studio sends a JSON wrapper containing a sanitized bounded preview and marks the context as truncated. The exact truncated payload is still shown before send.

## OpenAI data handling choice

Mobile API Studio sets `store: false` for its OpenAI Responses API calls. The app does not use background mode or provider-side conversation state. Users should still apply their organization's own OpenAI API data-retention and compliance requirements.

## BYOK settings

Settings → AI provider exposes:

- provider (OpenAI in v0.5);
- model;
- API key replacement/removal;
- configurable JSON/query secret-key list.

An empty API-key field preserves the currently stored credential. Removing the key deletes it from the OS credential store.

## AI workspace

The AI route supports two explicit workflows:

1. **Session diff** — choose baseline and candidate sessions, preview context, then explain the deterministic comparison.
2. **Flow diagnosis** — choose a captured flow, preview context, then diagnose that flow.

The send button is unavailable until a preview exists and an API key is configured. A changed context invalidates the preview fingerprint and forces re-review.

## Local history

`crates/ai-storage` stores AI results in the same local SQLite database using its own migration/table. The original provider API key and unredacted external context are never persisted in the AI result record.

## Deferred validation

Per the repository implementation-first rule, formal tests, automated verification, benchmarks, and final validation are deferred until all implementation phases are complete and will be performed independently by the repository owner.
