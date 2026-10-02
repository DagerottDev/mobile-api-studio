# Selected sharing and team workspace

Sharing is optional. The Sharing page stores an issued access token in memory only; sign-in reads membership without starting capture, uploading local data, or syncing. The self-hosted server has one team per deployment with owner/editor/viewer roles. Owners manage members and token rotation; owners/editors publish artifacts and definitions; viewers read. The last owner cannot be removed or demoted.

## Review and upload

Load local requests, select explicit flows, and prepare a HAR preview. Query values and bodies are omitted by default. Including either requires a separate checkbox and a new preview. Known credential headers/query names are redacted; SDK correlation headers, cookies, CA/secure-store context and custom metadata are excluded. Known private key material is rejected, including decoded binary bodies. Other secrets in ordinary paths, headers or application bodies still require human review.

Upload sends the exact immutable preview string and its SHA-256 digest; the server validates rather than rewriting it, and returns its stored digest. Links last one hour, one day or seven days in the UI and can be revoked. Public link retrieval returns an attachment with exactly those bytes. Expired/revoked links return 404. Link tokens are never returned by list endpoints, so copy the link when created. Removing a member also revokes their shares. Server access tokens and link tokens are stored only as hashes; the initial owner token is issued once in a private bootstrap file, never logged or read by the service.

Native `sharing_request` uses only allowlisted sharing paths/methods, a 64-hex transient access token, HTTPS origins or HTTP literal loopback, no environment proxy, no redirects, a fifteen-second timeout, and bounded request/response bodies. The browser keeps its existing same-origin CSP and authenticated local command channel. No arbitrary fetch endpoint or LAN browser API was added.

## Manual team workspace sync

Load the team document and inspect it before importing. Import validates all definitions, then saves fresh local IDs in a single SQLite transaction; proxy rules are disabled and original local rules remain intact. Scripts, Map Local files and source flow references cannot be shared. Selected rule/fixture export omits sensitive headers and bodies by default. Body inclusion requires explicit review.

Publishing replaces the team's reviewed selection using revision compare-and-swap. A concurrent update produces a conflict, requiring reload/review. There is no background sync, automatic merge, sign-in-triggered capture, or telemetry upload.

## Run and checks

See `apps/sharing-server/README.md` for deployment, token provisioning, limits and API contracts. This server currently runs only on Unix hosts with private owned storage; non-Unix startup fails closed. A Windows desktop client may connect to a verified HTTPS deployment. External hosting, TLS, backups and retention are owner configuration and have not been deployed or certified here.

Verified on macOS with disposable loopback data: all 21 app-core checks; frontend production build; server HTTP check (roles, token rotation, last-owner protection, CAS, exact bytes/digest, expiry/revocation, redaction/malformed input rejection, origin isolation, bootstrap permissions and dangling database symlink refusal); combined native command path `scripts/check-sharing.py` (selected HAR preview/import, explicit upload, public exact bytes, identity-only sign-in, team publish/pull and disabled fresh-ID import preserving originals). No real traffic upload or external deployment occurred. Browser interaction remains blocked by tool security policy, and physical-device/release acceptance is separate.

```sh
cargo build -p mobile-api-studio-server -p mobile-api-studio-sharing-server --offline --locked
python3 apps/sharing-server/checks/http_check.py target/debug/mobile-api-studio-sharing-server
python3 scripts/check-sharing.py
```
