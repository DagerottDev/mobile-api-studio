# App-Aware SDK Integration

Mobile API Studio can enrich proxy-captured requests with app metadata such as the current screen, feature, source file/function, custom attributes, and nearby logs.

The SDK is optional. Proxy capture continues to work normally when no SDK is installed.

## How correlation works

For an instrumented app request the SDK:

1. creates a unique request ID;
2. adds `X-Mobile-API-Studio-Request-Id` to the outgoing request;
3. sends a local SDK network event to the desktop ingestion service;
4. the capture proxy records the request ID locally and removes the header before forwarding the request to the real backend;
5. the desktop joins the proxy flow and SDK events by request ID.

The correlation header is therefore development metadata only. Safe cURL export and the desktop Replay engine also omit it.

SDK telemetry uses the local desktop service on port `8182`:

- iOS Simulator: `http://127.0.0.1:8182`
- Android Emulator: `http://10.0.2.2:8182`
- event endpoint: `POST /v1/events`
- health endpoint: `GET /health`

The desktop server binds only to host loopback. The Android Emulator reaches that host through its standard `10.0.2.2` alias.

## iOS

The package is located at:

```text
sdks/ios/MobileAPIStudio
```

Add it as a local Swift Package during development, then configure it early in app startup:

```swift
import MobileAPIStudio

MobileAPIStudio.configure()
```

The default configuration is enabled in `DEBUG` builds and disabled otherwise. You can explicitly control it:

```swift
MobileAPIStudio.configure(
    .init(enabled: isInternalDebugBuild)
)
```

Calling `MobileAPIStudio.disable()` stops telemetry and request instrumentation.

### Add app context

Set context at screen or feature boundaries:

```swift
MobileAPIStudio.setContext(
    screen: "Product Detail",
    feature: "Delivery Promise",
    attributes: ["productType": "fashion"]
)
```

`setContext` automatically records its Swift file, function, and line as the source location.

Structured logs inherit the current context:

```swift
MobileAPIStudio.log(
    "Refreshing delivery modes",
    attributes: ["reason": "pincode_changed"]
)
```

### URLSession integration

For URLSession-based clients, instrument the configuration before creating the session:

```swift
let config = MobileAPIStudio.instrument(URLSessionConfiguration.default)
let session = URLSession(configuration: config)
```

This installs the SDK's opt-in `URLProtocol`. When the SDK is disabled the configuration is returned unchanged.

### Manual/custom networking integration

For a custom client, instrument a `URLRequest` directly:

```swift
var request = URLRequest(url: url)
let requestID = MobileAPIStudio.instrument(
    &request,
    feature: "Load PDP"
)

customClient.send(request) { response, error in
    if let requestID {
        MobileAPIStudio.complete(
            requestID: requestID,
            response: response,
            error: error
        )
    }
}
```

The SDK telemetry transport disables URLProtocol handling and proxy use so SDK events do not recursively appear as captured app traffic.

### iOS sample

A minimal SwiftUI integration example is available at:

```text
samples/ios-sdk-demo
```

It uses an XcodeGen `project.yml`. Generate/open the sample project with your normal XcodeGen workflow, boot an iOS Simulator, start Mobile API Studio, then tap **Send sample request**.

## Android

The Android library lives at:

```text
sdks/android/mobile-api-studio
```

The repository Android SDK workspace includes the `:mobile-api-studio` library and `:sample` application modules.

Configure the SDK from your app using a debug/internal-build condition:

```kotlin
MobileAPIStudio.configure(
    context = applicationContext,
    configuration = MobileAPIStudioConfiguration(
        enabled = BuildConfig.DEBUG,
    ),
)
```

The Android configuration defaults to `enabled = false`, so the library is a pass-through unless the app explicitly opts in. `MobileAPIStudio.disable()` can turn it off at runtime.

### Add app context

```kotlin
MobileAPIStudio.setContext(
    screen = "Product Detail",
    feature = "Delivery Promise",
    attributes = mapOf("productType" to "fashion"),
)
```

`setContext` captures the app call site's file/function/line from the stack. Logs inherit the context:

```kotlin
MobileAPIStudio.log(
    message = "Refreshing delivery modes",
    attributes = mapOf("reason" to "pincode_changed"),
)
```

### OkHttp integration

Add the interceptor to the application's development/debug OkHttp client:

```kotlin
val client = OkHttpClient.Builder()
    .addInterceptor(MobileAPIStudioInterceptor())
    .build()
```

When the SDK is disabled, the interceptor forwards the exact original request without adding a correlation header.

The interceptor preserves source metadata already set by `MobileAPIStudio.setContext`; it intentionally does not treat OkHttp framework stack frames as the source call site.

### Manual/custom networking integration

For clients other than OkHttp:

```kotlin
val requestId = MobileAPIStudio.beginRequest(
    method = "POST",
    url = url,
    feature = "Add to Bag",
)

// Add requestId to MobileAPIStudio.correlationHeader on the actual request.

try {
    val status = customClient.execute()
    requestId?.let { MobileAPIStudio.complete(it, statusCode = status) }
} catch (error: Throwable) {
    requestId?.let { MobileAPIStudio.complete(it, error = error) }
    throw error
}
```

Android SDK telemetry is sent through a raw local TCP socket rather than OkHttp. This keeps telemetry outside both the app's OkHttp interceptor chain and the emulator HTTP proxy.

### Android sample

The runnable sample module is:

```text
sdks/android/sample
```

It enables the SDK only when the application is debuggable, trusts user-installed CAs for the development capture scenario, and sends a sample OkHttp request through the SDK interceptor.

## Desktop workflow

The **SDK** screen shows:

- ingestion service reachability and active/known client counts;
- app ID, app version/build, device, OS, and SDK version;
- local SDK endpoints;
- recent context/log/network events;
- capture sessions attributed to SDK app IDs.

The **Traffic** inspector shows an **App context** section for correlated flows with:

- app/device metadata;
- screen and feature;
- source file/function/line;
- custom attributes;
- nearby logs and context changes.

Flows without the SDK remain explicitly labeled as proxy-only.

Traffic also has a separate app-context search field. It can filter captured flows by text present in SDK event metadata, including screen, feature, source, attributes, and logs attached to request events.

Historical sessions display their attributed app ID once a correlated SDK request is observed.

## Safety and scope

- This feature is intended for applications you develop or are authorized to debug.
- It does not bypass certificate pinning or modify third-party applications.
- The SDK does not replace proxy capture; it enriches it.
- SDK telemetry is local-only and best-effort. A telemetry failure must never fail the app's real network request.
- Do not put secrets into context attributes or log messages. They are stored locally in the Mobile API Studio workspace database.
- Keep production/release instrumentation disabled unless your own internal build policy explicitly requires otherwise.

## Troubleshooting

If the app does not appear under **SDK**:

1. confirm Mobile API Studio desktop is running;
2. confirm the SDK screen reports **ingestion online**;
3. confirm the SDK is enabled in the app build;
4. on iOS Simulator, confirm the SDK uses `127.0.0.1:8182`;
5. on Android Emulator, confirm it uses `10.0.2.2:8182`;
6. check the SDK screen for the expected handshake.

If the SDK client appears but a proxy flow says **proxy-only**, confirm the request is going through the URLSession integration, OkHttp interceptor, or manual correlation API, and that the request itself is captured by the proxy.
