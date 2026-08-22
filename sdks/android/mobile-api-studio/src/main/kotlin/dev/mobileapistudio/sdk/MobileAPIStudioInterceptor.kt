package dev.mobileapistudio.sdk

import okhttp3.Interceptor
import okhttp3.Response

/**
 * Opt-in OkHttp interceptor that adds Mobile API Studio correlation metadata.
 * When the SDK is disabled the original request passes through untouched.
 *
 * Source file/function metadata should be supplied by [MobileAPIStudio.setContext] at the
 * feature boundary. The interceptor intentionally does not infer a source location because
 * its call stack is dominated by OkHttp internals rather than the app call site.
 */
public class MobileAPIStudioInterceptor(
    private val feature: String? = null,
    private val attributes: () -> Map<String, String> = { emptyMap() },
) : Interceptor {
    override fun intercept(chain: Interceptor.Chain): Response {
        val original = chain.request()
        val instrumented = MobileAPIStudio.instrument(
            request = original,
            feature = feature,
            attributes = attributes(),
            source = null,
        )

        if (instrumented == null) {
            return chain.proceed(original)
        }

        return try {
            val response = chain.proceed(instrumented.request)
            MobileAPIStudio.complete(
                requestId = instrumented.requestId,
                response = response,
            )
            response
        } catch (throwable: Throwable) {
            MobileAPIStudio.complete(
                requestId = instrumented.requestId,
                error = throwable,
            )
            throw throwable
        }
    }
}
