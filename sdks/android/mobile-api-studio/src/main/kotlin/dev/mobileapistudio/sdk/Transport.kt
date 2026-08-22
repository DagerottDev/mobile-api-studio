package dev.mobileapistudio.sdk

import org.json.JSONArray
import org.json.JSONObject
import java.io.BufferedInputStream
import java.net.InetSocketAddress
import java.net.Socket
import java.nio.charset.StandardCharsets
import java.util.UUID
import java.util.concurrent.Executors

internal class MobileAPIStudioTransport(
    private val host: String,
    private val port: Int,
) {
    private val executor = Executors.newSingleThreadExecutor { runnable ->
        Thread(runnable, "MobileAPIStudio-SDK").apply { isDaemon = true }
    }

    fun send(type: String, payload: Map<String, Any?>) {
        val envelope = JSONObject().apply {
            put("schemaVersion", 1)
            put("eventId", UUID.randomUUID().toString().lowercase())
            put("occurredAt", System.currentTimeMillis().toString())
            put("event", JSONObject().apply {
                put("type", type)
                put("payload", jsonValue(payload))
            })
        }
        val bytes = envelope.toString().toByteArray(StandardCharsets.UTF_8)
        executor.execute {
            runCatching { post(bytes) }
        }
    }

    fun close() {
        executor.shutdownNow()
    }

    private fun post(body: ByteArray) {
        Socket().use { socket ->
            socket.connect(InetSocketAddress(host, port), 1_000)
            socket.soTimeout = 1_500
            val output = socket.getOutputStream()
            val headers = buildString {
                append("POST /v1/events HTTP/1.1\r\n")
                append("Host: $host:$port\r\n")
                append("Content-Type: application/json\r\n")
                append("Content-Length: ${body.size}\r\n")
                append("Connection: close\r\n")
                append("\r\n")
            }.toByteArray(StandardCharsets.US_ASCII)
            output.write(headers)
            output.write(body)
            output.flush()

            // Drain the tiny local response so the desktop can close the connection cleanly.
            val input = BufferedInputStream(socket.getInputStream())
            val buffer = ByteArray(512)
            while (input.read(buffer) >= 0) {
                // Intentionally ignored. SDK telemetry must never affect app behavior.
            }
        }
    }

    private fun jsonValue(value: Any?): Any = when (value) {
        null -> JSONObject.NULL
        is JSONObject, is JSONArray, is String, is Number, is Boolean -> value
        is Map<*, *> -> JSONObject().apply {
            value.forEach { (key, child) ->
                if (key != null) put(key.toString(), jsonValue(child))
            }
        }
        is Iterable<*> -> JSONArray().apply { value.forEach { put(jsonValue(it)) } }
        is Array<*> -> JSONArray().apply { value.forEach { put(jsonValue(it)) } }
        else -> value.toString()
    }
}
