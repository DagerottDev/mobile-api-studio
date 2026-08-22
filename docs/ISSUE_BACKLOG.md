# Seed Issue Backlog

This file is designed to be converted into GitHub issues once the repository exists.

## Phase 0 — Foundation

1. **P0: Scaffold Tauri 2 + React desktop app**  
   Labels: `phase-0`, `frontend`, `desktop`

2. **P0: Create Cargo + pnpm workspace structure**  
   Labels: `phase-0`, `architecture`

3. **P0: Define v1 core domain models**  
   Labels: `phase-0`, `rust`, `architecture`

4. **P0: Add SQLite migrations and repositories**  
   Labels: `phase-0`, `storage`, `rust`

5. **P0: Implement content-addressed body store**  
   Labels: `phase-0`, `storage`, `security`

6. **P0: Build deterministic local API fixture server**  
   Labels: `phase-0`, `testing`

7. **P0: Render fake live flows in virtualized traffic table**  
   Labels: `phase-0`, `frontend`

8. **P0: Set up GitHub Actions quality gates**  
   Labels: `phase-0`, `ci`

## Phase 1 — v0.1

9. **P1: Define CaptureEngine trait and event schema**  
   Labels: `v0.1`, `capture`, `rust`

10. **P1: Implement mitmdump addon event bridge**  
    Labels: `v0.1`, `capture`, `python`

11. **P1: Implement mitmdump process manager**  
    Labels: `v0.1`, `capture`, `rust`

12. **P1: Implement iOS Simulator discovery with simctl**  
    Labels: `v0.1`, `ios`

13. **P1: Implement Android Emulator discovery with ADB**  
    Labels: `v0.1`, `android`

14. **P1: Implement CA manager and certificate status**  
    Labels: `v0.1`, `security`, `capture`

15. **P1: Install CA into selected iOS Simulator**  
    Labels: `v0.1`, `ios`, `security`

16. **P1: Implement reversible iOS Simulator connection strategy**  
    Labels: `v0.1`, `ios`, `capture`

17. **P1: Implement reversible Android Emulator proxy strategy**  
    Labels: `v0.1`, `android`, `capture`

18. **P1: Add connection state machine and rollback journal**  
    Labels: `v0.1`, `architecture`, `reliability`

19. **P1: Add typed connection diagnostics**  
    Labels: `v0.1`, `diagnostics`

20. **P1: Build live traffic timeline**  
    Labels: `v0.1`, `frontend`

21. **P1: Build flow inspector tabs**  
    Labels: `v0.1`, `frontend`

22. **P1: Add JSON/text/image body viewers**  
    Labels: `v0.1`, `frontend`

23. **P1: Implement default secret redaction**  
    Labels: `v0.1`, `security`

24. **P1: Implement safe Copy cURL**  
    Labels: `v0.1`, `replay`, `security`

25. **P1: Build replay draft editor**  
    Labels: `v0.1`, `replay`, `frontend`

26. **P1: Implement Rust replay engine**  
    Labels: `v0.1`, `replay`, `rust`

27. **P1: Add 10k-flow performance fixture and benchmark**  
    Labels: `v0.1`, `performance`, `testing`

28. **P1: v0.1 end-to-end fixture app test**  
    Labels: `v0.1`, `testing`, `release`

## Phase 2 — v0.2

29. Persistent named sessions and session browser
30. Advanced filter expression model
31. Fast host/path/body-preview search
32. Endpoint normalization heuristics
33. Saved request collections
34. Environment variable interpolation
35. Secure secret environment variables
36. Session export/import v1
37. Connection Doctor UI
38. Managed/bundled capture sidecar

## Phase 3 — v0.3

39. Mock rule model + persistence
40. Mock matcher
41. Static mock response
42. Response mutation
43. Latency/timeout/drop simulation
44. Failure preset UI
45. Create mock from captured response
46. Request breakpoint
47. Response breakpoint
48. Disable-all-mocks safety action

## Phase 4 — v0.4

49. Define local SDK protocol
50. Swift Package core
51. Swift URLSession integration
52. iOS sample app
53. Kotlin core library
54. OkHttp interceptor
55. Android sample app
56. Debug/release no-op verification
57. SDK pairing/authentication
58. Proxy↔SDK correlation
59. App/screen/feature metadata inspector

## Phase 5 — v0.5

60. Session pairing model
61. Endpoint flow matcher
62. Header/query diff
63. JSON structural diff
64. Timing comparison
65. Missing/extra request detection
66. JSON shape inference
67. Duplicate/retry detector
68. Waterfall sequential/parallel analysis
69. AI provider interface
70. AI redaction/context-preview pipeline
71. Cross-platform diagnostic prompt/result UI

## Suggested labels

```text
phase-0
v0.1
v0.2
v0.3
v0.4
v0.5
frontend
rust
python
ios
android
capture
storage
replay
mocking
comparison
ai
security
performance
diagnostics
testing
ci
release
```
