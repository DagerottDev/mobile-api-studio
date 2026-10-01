# Network conditions — milestone 4

The localhost service and browser UI support persisted global, app, host and endpoint profiles. One enabled profile wins: endpoint, host, app, global; then ascending priority, numeric creation time and ID. Host matching ignores case; endpoint paths match exactly without the query. App attribution uses already ingested SDK events and their registered app ID. Missing or ambiguous attribution does not match an app profile; no arbitrary request header is trusted as an app identity.

Profiles add request latency with uniform ±jitter, a nonnegative delay floor, buffered upload/download rate simulation, offline behavior and configurable request failure percentage. Offline/failure stops the request and keeps an inspectable error. These are application request failures, not transport packet loss. DNS, encrypted passthrough and ongoing WebSocket messages are outside these HTTP body profiles.

Mitmproxy buffers HTTP bodies before request/response hooks. Rate simulation waits body-byte-count / bytes-per-second after request rules and response rewrites, then forwards the buffered message. It measures end-to-end transfer time; it does not pace individual packets, share bandwidth across concurrent flows, or emulate a slow streaming connection. The upstream [streaming behavior](https://docs.mitmproxy.org/stable/overview/features/) explains why streamed bodies would bypass these body edits. True transport packet loss is unavailable in the selected capture integration, so no packet-loss setting is accepted.

Each stage permits at most 120 seconds of simulated delay; exceeding that bound stops the flow with a diagnostic instead of silently clipping the rate. Delay/jitter are each limited to 10 seconds, optional rates to 1 KiB/s–1 GiB/s, failure probability to 0–100%, and the workspace to 100 profiles. Disable all is available across routes, prevents profiles on new requests and releases a pending simulated wait after its next policy check (normally within 100 ms, plus local service time). A profile snapshot applies to a flow; disable/delete releases waits, while other edits apply to subsequent requests.

Migration 7 adds profiles with a pre-migration database backup. Portable bundle v6 includes profiles and accepts older versions 2–5. Imports validate profiles before mutation, start them disabled, and include them in the existing atomic replace transaction. Local command authentication and private rule-socket permissions are retained.

## Reproducible checks

Build `cargo build -p mobile-api-studio-server --locked --offline`, then run `python3 scripts/check-network-conditions.py`. The standard-library check launches a disposable service, origin and reverse proxy on loopback; it does not change host-wide proxy settings. It measures 32 KiB bodies at 64 KiB/s in each direction plus 200 ms latency against a no-profile baseline; expected additional elapsed time is 1.2 seconds, accepted at 1.05–1.65 seconds for local scheduling overhead. Jitter samples around 300 ±50 ms accept 180–550 ms. It also verifies disable-all releases a waiting request, new requests lose the delay, and offline/100% failure diagnostics persist.

Focused Rust checks cover scope precedence, invalid/unknown fields (including unsupported packet loss), SDK ambiguity, migration/reopen/disable/cap enforcement, and bundle disabled import/export/replace rollback. Frontend compilation passed. The built-in browser security policy blocked the new localhost preview, so profile editor and cross-route disable interaction checks remain unverified.

Live loopback result: baseline 0.082 s; combined 64 KiB/s upload/download plus 200 ms latency 1.227 s; jitter samples 0.215/0.265/0.204 s after baseline subtraction; disable released the waiting request at 0.340 s. Offline and 100% failure diagnostics persisted. Protocol regression checks passed HTTP/2/TLS/gRPC trailers and WebSocket text/binary/empty capture. OpenJEV selected specificity-first matching without provider fallback.

## Owner-led acceptance still pending

1. Measure profiles in Simulator/Emulator and physical-device HTTP(S) capture; repeat recovery, Replay, SDK and import regressions.
2. Verify app attribution timing with real SDKs and overlapping profiles on concurrent application traffic.
3. Validate large bodies, supported HTTP/2/HTTP/3 capture modes and the explicit streaming/WebSocket/packet-loss limits.
4. Validate packaged entry points, keyboard/accessibility and release compatibility. Current automated loopback checks exercise the localhost service and browser UI.

Milestones 5–8 and earlier device/release acceptance remain separate work.
