package dev.mobileapistudio.sdk

import android.content.Context
import okhttp3.Request
import okhttp3.Response
import java.util.UUID
import java.util.concurrent.ConcurrentHashMap

public object MobileAPIStudio {
    public const val sdkVersion: String = "0.1.0"
    public const val correlationHeader: String = "X-Mobile-API-Studio-Request-Id"

    private data class InFlight(
        val startedAtMillis: Long,
        val method: String,
        val url: String,
        val context: MobileAPIStudioContext,
    )

    private val lock = Any()
    private var configuration: ResolvedConfiguration? = null
    private var transport: MobileAPIStudioTransport? = null
    private var clientId: String? = null
    private var currentContext = MobileAPIStudioContext()
    private val inFlight = ConcurrentHashMap<String, InFlight>()

    public val isEnabled: Boolean
        get() = synchronized(lock) { configuration?.enabled == true }

    public fun configure(
        context: Context,
        configuration: MobileAPIStudioConfiguration = MobileAPIStudioConfiguration(),
    ) {
        val appContext = context.applicationContext
        val resolved = configuration.resolved(appContext)
        val previousTransport = synchronized(lock) {
            val previous = transport
            this.configuration = resolved
            if (!resolved.enabled) {
                transport = null
                clientId = null
                currentContext = MobileAPIStudioContext()
                inFlight.clear()
                previous
            } else {
                val nextClientId = loadClientId(appContext, resolved.appId)
                transport = MobileAPIStudioTransport(resolved.desktopHost, resolved.desktopPort)
                clientId = nextClientId
                previous
            }
        }
        previousTransport?.close()
        if (resolved.enabled) sendHandshake()
    }

    public fun disable() {
        val previous = synchronized(lock) {
            configuration = configuration?.copy(enabled = false)
            val value = transport
            transport = null
            clientId = null
            currentContext = MobileAPIStudioContext()
            inFlight.clear()
            value
        }
        previous?.close()
    }

    public fun setContext(
        screen: String? = null,
        feature: String? = null,
        attributes: Map<String, String> = emptyMap(),
        source: MobileAPIStudioSource? = callerSource(),
    ) {
        val value = MobileAPIStudioContext(screen, feature, attributes, source)
        val payload = synchronized(lock) {
            currentContext = value
            val id = clientId ?: return@synchronized null
            if (configuration?.enabled != true) return@synchronized null
            mapOf("clientId" to id, "context" to value.toWire())
        }
        payload?.let { send("context", it) }
    }

    public fun log(
        message: String,
        level: MobileAPIStudioLogLevel = MobileAPIStudioLogLevel.INFO,
        attributes: Map<String, String> = emptyMap(),
        source: MobileAPIStudioSource? = callerSource(),
    ) {
        val payload = synchronized(lock) {
            val id = clientId ?: return@synchronized null
            if (configuration?.enabled != true) return@synchronized null
            val context = mergedContext(feature = null, attributes = attributes, source = source)
            mapOf(
                "clientId" to id,
                "level" to level.wireValue,
                "message" to message,
                "context" to context.toWire(),
            )
        }
        payload?.let { send("log", it) }
    }

    public fun instrument(
        request: Request,
        feature: String? = null,
        attributes: Map<String, String> = emptyMap(),
        source: MobileAPIStudioSource? = callerSource(),
    ): MobileAPIStudioInstrumentedRequest? {
        val requestId = beginRequest(
            method = request.method,
            url = request.url.toString(),
            feature = feature,
            attributes = attributes,
            source = source,
        ) ?: return null
        val instrumented = request.newBuilder()
            .header(correlationHeader, requestId)
            .build()
        return MobileAPIStudioInstrumentedRequest(requestId, instrumented)
    }

    /**
     * Starts correlation for a custom networking stack. Add the returned value to
     * [correlationHeader] on the real outgoing request, then call [complete].
     */
    public fun beginRequest(
        method: String,
        url: String,
        feature: String? = null,
        attributes: Map<String, String> = emptyMap(),
        source: MobileAPIStudioSource? = callerSource(),
    ): String? {
        val result = synchronized(lock) {
            val id = clientId ?: return@synchronized null
            if (configuration?.enabled != true) return@synchronized null
            val requestId = UUID.randomUUID().toString().lowercase()
            val context = mergedContext(feature, attributes, source)
            val normalizedMethod = method.ifBlank { "GET" }.uppercase()
            inFlight[requestId] = InFlight(
                startedAtMillis = System.currentTimeMillis(),
                method = normalizedMethod,
                url = url,
                context = context,
            )
            requestId to mapOf<String, Any?>(
                "clientId" to id,
                "requestId" to requestId,
                "phase" to "started",
                "method" to normalizedMethod,
                "url" to url,
                "context" to context.toWire(),
            )
        } ?: return null
        send("network", result.second)
        return result.first
    }

    public fun complete(
        requestId: String,
        response: Response? = null,
        error: Throwable? = null,
    ) {
        complete(
            requestId = requestId,
            statusCode = response?.code,
            error = error,
        )
    }

    public fun complete(
        requestId: String,
        statusCode: Int? = null,
        error: Throwable? = null,
    ) {
        val flight = inFlight.remove(requestId) ?: return
        val payload = synchronized(lock) {
            val id = clientId ?: return@synchronized null
            if (configuration?.enabled != true) return@synchronized null
            buildMap<String, Any?> {
                put("clientId", id)
                put("requestId", requestId)
                put("phase", if (error == null) "completed" else "failed")
                put("method", flight.method)
                put("url", flight.url)
                put("durationMs", (System.currentTimeMillis() - flight.startedAtMillis).coerceAtLeast(0))
                put("context", flight.context.toWire())
                statusCode?.let { put("statusCode", it) }
                error?.let { put("error", it.toString()) }
            }
        }
        payload?.let { send("network", it) }
    }

    public fun sourceFromStack(): MobileAPIStudioSource? = callerSource()

    private fun sendHandshake() {
        val payload = synchronized(lock) {
            val config = configuration ?: return@synchronized null
            val id = clientId ?: return@synchronized null
            if (!config.enabled) return@synchronized null
            buildMap<String, Any?> {
                put("clientId", id)
                put("appId", config.appId)
                put("appName", config.appName)
                put("platform", "android")
                put("sdkVersion", sdkVersion)
                config.appVersion?.let { put("appVersion", it) }
                config.appBuild?.let { put("appBuild", it) }
                config.deviceName?.let { put("deviceName", it) }
                config.osVersion?.let { put("osVersion", it) }
            }
        }
        payload?.let { send("handshake", it) }
    }

    private fun send(type: String, payload: Map<String, Any?>) {
        synchronized(lock) { transport }?.send(type, payload)
    }

    private fun mergedContext(
        feature: String?,
        attributes: Map<String, String>,
        source: MobileAPIStudioSource?,
    ): MobileAPIStudioContext {
        return currentContext.copy(
            feature = feature ?: currentContext.feature,
            attributes = currentContext.attributes + attributes,
            source = source ?: currentContext.source,
        )
    }

    private fun loadClientId(context: Context, appId: String): String {
        val preferences = context.getSharedPreferences("mobile-api-studio", Context.MODE_PRIVATE)
        val key = "client-id.$appId"
        val existing = preferences.getString(key, null)
        if (!existing.isNullOrBlank()) return existing
        val generated = UUID.randomUUID().toString().lowercase()
        preferences.edit().putString(key, generated).apply()
        return generated
    }

    private fun callerSource(): MobileAPIStudioSource? {
        val frame = Throwable().stackTrace.firstOrNull { element ->
            !element.className.startsWith("dev.mobileapistudio.sdk.") &&
                !element.className.startsWith("java.") &&
                !element.className.startsWith("kotlin.")
        } ?: return null
        return MobileAPIStudioSource(
            file = frame.fileName,
            function = frame.methodName,
            line = frame.lineNumber.takeIf { it >= 0 },
        )
    }
}
