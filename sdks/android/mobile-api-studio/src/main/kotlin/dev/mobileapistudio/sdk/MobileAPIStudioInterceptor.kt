package dev.mobileapistudio.sdk

import okhttp3.Interceptor
import okhttp3.Response

/**
 * Opt-in OkHttp interceptor that adds Mobile API Studio correlation metadata in debug builds.
 * When the SDK is disabled the original request passes through untouched.
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
            source = MobileAPIStudio.sourceFromStack(),
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
