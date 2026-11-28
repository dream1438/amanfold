package com.amanfold.client

import java.io.IOException
import java.net.HttpURLConnection
import java.net.URI
import java.net.URL
import java.util.Base64
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.intOrNull
import kotlinx.serialization.json.jsonPrimitive

// Client for the Amanfold sync server (docs/SERVER.md). JSON over HTTPS,
// byte strings as unpadded base64url. The server only ever sees ciphertext.

/** Unpadded base64url, as the server API uses. */
object B64 {
    private val enc = Base64.getUrlEncoder().withoutPadding()
    private val dec = Base64.getUrlDecoder()
    fun encode(b: ByteArray): String = enc.encodeToString(b)
    fun decode(s: String): ByteArray = dec.decode(s)
}

/** The server refused a call or could not be reached (`status` 0). */
class ApiException(
    val status: Int,
    val kind: String,
    message: String,
    val seq: Long? = null,
) : IOException("$kind: $message")

/** The server address is not acceptable. */
class BadServerException(message: String) : IllegalArgumentException(message)

/**
 * Checks a server address and returns it without a trailing slash. HTTPS is
 * required, except for this device or the emulator's host when [allowLocalHttp]
 * is set (debug builds, self-hosting tests).
 */
fun checkServerUrl(raw: String, allowLocalHttp: Boolean): String {
    val u = try {
        URI(raw.trim())
    } catch (_: Exception) {
        throw BadServerException("server address")
    }
    val host = u.host ?: throw BadServerException("server address")
    val local = host in setOf("localhost", "127.0.0.1", "[::1]", "::1", "10.0.2.2")
    val ok = u.scheme == "https" || (u.scheme == "http" && local && allowLocalHttp)
    if (!ok) throw BadServerException("server must use https")
    if (u.rawUserInfo != null || u.rawQuery != null || u.rawFragment != null) {
        throw BadServerException("server address")
    }
    val port = if (u.port == -1) "" else ":${u.port}"
    val path = (u.rawPath ?: "").trimEnd('/')
    return "${u.scheme}://$host$port$path"
}

internal val json = Json {
    ignoreUnknownKeys = true
    explicitNulls = false
}

/** Minimal blocking HTTP client. Call it from a background dispatcher. */
class Api(val base: String, var token: String? = null) {

    fun call(method: String, path: String, body: JsonElement? = null): JsonObject {
        val conn = try {
            (URL(base + path).openConnection() as HttpURLConnection)
        } catch (e: IOException) {
            throw ApiException(0, "offline", "could not reach the server")
        }
        try {
            conn.requestMethod = method
            conn.instanceFollowRedirects = false
            conn.useCaches = false
            conn.connectTimeout = 15_000
            conn.readTimeout = 30_000
            conn.setRequestProperty("Accept", "application/json")
            token?.let { conn.setRequestProperty("Authorization", "Bearer $it") }
            if (body != null) {
                conn.doOutput = true
                conn.setRequestProperty("Content-Type", "application/json")
                conn.outputStream.use { it.write(body.toString().toByteArray()) }
            }
            val status = try {
                conn.responseCode
            } catch (e: IOException) {
                throw ApiException(0, "offline", "could not reach the server")
            }
            val stream = if (status in 200..299) conn.inputStream else conn.errorStream
            val text = stream?.use { it.readBytes().decodeToString() } ?: ""
            val obj = if (text.isBlank()) JsonObject(emptyMap()) else try {
                json.parseToJsonElement(text) as JsonObject
            } catch (_: Exception) {
                throw ApiException(status, "bad_response", "the server sent something unexpected")
            }
            if (status !in 200..299) {
                val kind = obj.str("error") ?: "http"
                val msg = obj.str("message") ?: "HTTP $status"
                val seq = (obj["seq"] as? kotlinx.serialization.json.JsonPrimitive)?.contentOrNull?.toLongOrNull()
                throw ApiException(status, kind, msg, seq)
            }
            return obj
        } catch (e: ApiException) {
            throw e
        } catch (e: IOException) {
            throw ApiException(0, "offline", "could not reach the server")
        } finally {
            conn.disconnect()
        }
    }

    fun get(path: String) = call("GET", path)
    fun post(path: String, body: JsonElement = JsonObject(emptyMap())) = call("POST", path, body)
    fun delete(path: String) = call("DELETE", path)
}

internal fun JsonObject.str(key: String): String? {
    val v = this[key] ?: return null
    if (v is JsonNull) return null
    return v.jsonPrimitive.contentOrNull
}

internal fun JsonObject.long(key: String): Long? =
    this[key]?.takeIf { it !is JsonNull }?.jsonPrimitive?.contentOrNull?.toLongOrNull()

internal fun JsonObject.int(key: String): Int? =
    this[key]?.takeIf { it !is JsonNull }?.jsonPrimitive?.intOrNull

internal fun JsonObject.need(key: String): String =
    str(key) ?: throw ApiException(200, "bad_response", "missing $key")
