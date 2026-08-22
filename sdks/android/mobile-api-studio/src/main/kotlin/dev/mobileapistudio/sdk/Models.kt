package dev.mobileapistudio.sdk

import android.content.Context
import android.os.Build
import okhttp3.Request

public data class MobileAPIStudioConfiguration(
    val desktopHost: String = "10.0.2.2",
    val desktopPort: Int = 8182,
    val enabled: Boolean = false,
    val appId: String? = null,
    val appName: String? = null,
    val appVersion: String? = null,
    val appBuild: String? = null,
) {
    internal fun resolved(context: Context): ResolvedConfiguration {
        val packageName = context.packageName
        val packageInfo = runCatching { context.packageManager.getPackageInfo(packageName, 0) }.getOrNull()
        val applicationInfo = context.applicationInfo
        val label = runCatching { context.packageManager.getApplicationLabel(applicationInfo).toString() }.getOrNull()

        return ResolvedConfiguration(
            desktopHost = desktopHost,
            desktopPort = desktopPort,
            enabled = enabled,
            appId = appId ?: packageName,
            appName = appName ?: label ?: packageName,
            appVersion = appVersion ?: packageInfo?.versionName,
            appBuild = appBuild ?: packageInfo?.longVersionCode?.toString(),
            deviceName = "${Build.MANUFACTURER} ${Build.MODEL}".trim(),
            osVersion = Build.VERSION.RELEASE,
        )
    }
}

internal data class ResolvedConfiguration(
    val desktopHost: String,
    val desktopPort: Int,
    val enabled: Boolean,
    val appId: String,
    val appName: String,
    val appVersion: String?,
    val appBuild: String?,
    val deviceName: String?,
    val osVersion: String?,
)

public data class MobileAPIStudioSource(
    val file: String? = null,
    val function: String? = null,
    val line: Int? = null,
) {
    internal fun toWire(): Map<String, Any> = buildMap {
        file?.let { put("file", it) }
        function?.let { put("function", it) }
        line?.let { put("line", it) }
    }
}

public data class MobileAPIStudioContext(
    val screen: String? = null,
    val feature: String? = null,
    val attributes: Map<String, String> = emptyMap(),
    val source: MobileAPIStudioSource? = null,
) {
    internal fun toWire(): Map<String, Any> = buildMap {
        screen?.let { put("screen", it) }
        feature?.let { put("feature", it) }
        put("attributes", attributes)
        source?.let { put("source", it.toWire()) }
    }
}

public enum class MobileAPIStudioLogLevel(internal val wireValue: String) {
    DEBUG("debug"),
    INFO("info"),
    WARNING("warning"),
    ERROR("error"),
}

public data class MobileAPIStudioInstrumentedRequest(
    val requestId: String,
    val request: Request,
)
