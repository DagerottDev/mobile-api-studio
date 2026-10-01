# Protocol inspection — milestone 3

This builds on capture targets and proxy rules. It extends the macOS localhost service and browser UI with focused loopback verification; device, compatibility and release acceptance remain owner-led.

## Capture and inspection

Traffic stores HTTP versions, connection addresses and IDs, TLS/ALPN/cipher information, bounded public certificate metadata, and request/response trailers. The inspector shows gRPC status and message trailers. HTTP/2 stream priority and push promises are not exposed by the capture engine.

WebSocket text, binary and empty messages keep their sequence, direction, opcode, timestamp and bounded body reference. Close metadata is retained. Payloads use the existing content-addressed body store, limited to 2 MiB. Mitmproxy retains only the current WebSocket message after the addon hook; an individual message can exceed the capture limit before truncation. Ping/pong payloads and WebSocket replay are unsupported by mitmproxy.

Body views provide JSON, XML, URL-encoded form, multipart part summaries, raster image previews and GraphQL operation formatting. XML entities/DOCTYPE declarations fall back to raw text; images exclude active SVG content. UI text previews stop at 100,000 characters. Raw views remain available for invalid or unsupported content.

The gRPC/Protobuf view accepts a binary FileDescriptorSet and full message name. Payloads and descriptors are bounded to 2 MiB, gRPC frames to 1,000, and decoded nesting to 64 levels. Compressed gRPC messages remain raw with an explicit diagnostic. Oversized output falls back to the ordinary raw body view.

HTTP/3 uses mitmproxy local capture or a reverse `http3://` listener. Regular, upstream and SOCKS proxy modes do not capture HTTP/3. Only QUIC v1 is supported; client compatibility varies and HTTP/3 replay is not supported. See the upstream [protocol limits](https://docs.mitmproxy.org/stable/concepts/protocols/).

## Persistence, search and interchange

Migration 6 adds WebSocket records and disposable search tables; existing databases are backed up before migration. Portable bundle v5 preserves protocol metadata and WebSocket payloads; versions 2–4 remain importable. Imported message IDs, flow/session references, body sizes and hashes are validated before mutation. Replace uses the existing SQLite workspace transaction.

Search indexes use the existing AI redaction policy, never leave the machine and are excluded from exports. Index text is limited to an 8 KiB UTF-8 prefix per record. JSON/form fields and headers/trailers are searchable; binary bodies and unstructured plaintext are excluded because they have no reliable secret-field boundary. Raw message bodies remain available for local inspection. Settings can rebuild indexes in bounded pages or cancel between pages. Imported raw data remains successful if optional indexing fails; the result reports a warning and Settings offers rebuild.

Exports redact known secret header/trailer values and omit correlation secrets, but include original captured body bytes; review the Settings preview before exporting.

## Verification performed

- Workspace `cargo check --workspace --locked --offline` and frontend `pnpm run build` passed.
- Focused app-core, storage, capture-mitm, core-model and AI redaction checks passed, including descriptor decoding/resource bounds, v5 restart/export/import, malformed message references and payloads, redacted index bounds, and rollback.
- `python3 sidecars/mitm-addon/protocol_live_check.py` passed real loopback HTTP/2, TLS/public certificate metadata, gRPC trailers, and text/binary/empty WebSocket capture.
- `HTTP3_PYTHON=<Python with aioquic> python3 scripts/check-http3.py` passed real loopback QUIC/HTTP/3 reverse request, response, payload and protocol metadata. The fixture needs aioquic in a disposable Python environment, not a new application dependency.
- Codex built-in browser verification used disposable data and loopback port 28181. GraphQL, XML/form/multipart, gRPC framing and WebSocket search were exercised with synthetic fixtures.
- OpenJEV selected a successful-import/index-pending result for a failed optional index rebuild, without provider fallback. A Sol review returned two verified fixes but stopped before completing its remaining review; this is not a complete independent review.

## Remaining owner-led acceptance

1. Verify packaged/native entry points separately; browser UI checks use the localhost service, not the legacy Tauri command surface. Repeat existing Simulator/Emulator capture, disconnect/recovery, Replay, SDK correlation, AI redaction and workspace-import regressions against this branch.
2. Verify selected Mac process and physical iOS/Android HTTP(S) capture, pairing restrictions and listener cleanup; earlier milestone acceptance remains pending.
3. Verify real application WebSocket streams, gRPC descriptors/trailers, image bodies, large captures and malformed content. Check restart/export/import and keyboard/accessibility behavior.
4. Check HTTP/3 in Mac local mode and additional clients; the completed live check covers reverse mode only.
5. Complete milestone 2 reverse/upstream/SOCKS/DNS client acceptance and release/platform compatibility checks.

Milestones 4–8 remain in the full roadmap. This draft does not certify physical devices or a public release.
