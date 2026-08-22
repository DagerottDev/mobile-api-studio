package dev.mobileapistudio.sample

import android.app.Activity
import android.content.pm.ApplicationInfo
import android.os.Bundle
import android.view.Gravity
import android.widget.Button
import android.widget.LinearLayout
import android.widget.TextView
import dev.mobileapistudio.sdk.MobileAPIStudio
import dev.mobileapistudio.sdk.MobileAPIStudioConfiguration
import dev.mobileapistudio.sdk.MobileAPIStudioInterceptor
import dev.mobileapistudio.sdk.MobileAPIStudioLogLevel
import okhttp3.Call
import okhttp3.Callback
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.Response
import java.io.IOException

class MainActivity : Activity() {
    private lateinit var status: TextView

    private val client by lazy {
        OkHttpClient.Builder()
            .addInterceptor(MobileAPIStudioInterceptor(feature = "Sample request"))
            .build()
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        val debugEnabled = applicationInfo.flags and ApplicationInfo.FLAG_DEBUGGABLE != 0
        MobileAPIStudio.configure(
            this,
            MobileAPIStudioConfiguration(enabled = debugEnabled),
        )
        MobileAPIStudio.setContext(
            screen = "Sample Home",
            feature = "SDK Demo",
            attributes = mapOf("platform" to "android"),
        )
        MobileAPIStudio.log("Android sample launched", MobileAPIStudioLogLevel.INFO)

        status = TextView(this).apply {
            text = "SDK ${if (MobileAPIStudio.isEnabled) "enabled" else "disabled"}. Tap to make a correlated request."
            textSize = 16f
        }
        val button = Button(this).apply {
            text = "Send sample request"
            setOnClickListener { sendRequest() }
        }
        val layout = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            gravity = Gravity.CENTER_HORIZONTAL
            setPadding(48, 96, 48, 48)
            addView(status, LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT)
            addView(button, LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT)
        }
        setContentView(layout)
    }

    private fun sendRequest() {
        MobileAPIStudio.setContext(
            screen = "Sample Home",
            feature = "Load Demo API",
            attributes = mapOf("trigger" to "button"),
        )
        status.text = "Sending…"
        val request = Request.Builder()
            .url("https://httpbin.org/anything/mobile-api-studio")
            .get()
            .build()

        client.newCall(request).enqueue(object : Callback {
            override fun onFailure(call: Call, e: IOException) {
                MobileAPIStudio.log("Sample request failed: ${e.message}", MobileAPIStudioLogLevel.ERROR)
                runOnUiThread { status.text = "Failed: ${e.message}" }
            }

            override fun onResponse(call: Call, response: Response) {
                response.use {
                    MobileAPIStudio.log(
                        "Sample request completed",
                        attributes = mapOf("status" to it.code.toString()),
                    )
                    runOnUiThread { status.text = "Completed with HTTP ${it.code}. Open Traffic → App context." }
                }
            }
        })
    }
}
