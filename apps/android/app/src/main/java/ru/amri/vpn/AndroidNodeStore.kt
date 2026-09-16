package ru.amri.vpn

import android.content.Context
import org.json.JSONArray
import ru.amri.vpn.security.AndroidKeystoreSecretStore
import java.io.ByteArrayOutputStream
import java.net.URL
import java.net.URLDecoder
import java.nio.charset.StandardCharsets
import java.security.MessageDigest
import java.util.Base64
import javax.net.ssl.HttpsURLConnection

/**
 * Encrypted Android persistence for credential-bearing node URIs.
 *
 * Raw URIs never enter ordinary SharedPreferences, intents, logs or process arguments. The complete
 * JSON payload is encrypted by AndroidKeystoreSecretStore. Only the selected index is kept in the
 * non-secret UI preference file. Provider subscription URLs are used only for one-shot HTTPS fetches
 * and are never persisted.
 */
internal class AndroidNodeStore(context: Context) {
    private val secureStore = AndroidKeystoreSecretStore(context)

    fun load(): MutableList<String> {
        val plaintext = secureStore.get(POOL_SLOT) ?: return mutableListOf()
        return try {
            val document = JSONArray(String(plaintext, Charsets.UTF_8))
            MutableList(document.length()) { index -> document.getString(index) }
                .filterTo(mutableListOf()) { uri -> isSupportedNodeUri(uri) }
        } finally {
            plaintext.fill(0)
        }
    }

    fun save(nodes: List<String>) {
        val clean = nodes.asSequence()
            .map(String::trim)
            .filter(::isSupportedNodeUri)
            .distinct()
            .take(MAX_NODES)
            .toList()
        if (clean.isEmpty()) {
            secureStore.delete(POOL_SLOT)
            return
        }

        val document = JSONArray()
        clean.forEach(document::put)
        val plaintext = document.toString().toByteArray(Charsets.UTF_8)
        try {
            secureStore.put(POOL_SLOT, plaintext)
        } finally {
            plaintext.fill(0)
        }
    }

    fun importText(text: String, existing: List<String> = load()): MutableList<String> {
        val input = text.trim()
        val payload = if (input.startsWith("https://") && !input.contains('\n') && !input.contains('\r')) {
            fetchProviderPayloadOffMainThread(input) ?: ""
        } else {
            input
        }
        val imported = decodeSubscriptionPayload(payload)
        val merged = existing.toMutableList()
        imported.forEach { uri ->
            if (uri !in merged && merged.size < MAX_NODES) merged += uri
        }
        save(merged)
        return merged
    }

    private fun fetchProviderPayloadOffMainThread(sourceUrl: String): String? {
        var payload: String? = null
        val worker = Thread({ payload = fetchHttpsPayload(sourceUrl) }, "amri-subscription-fetch")
        worker.isDaemon = true
        worker.start()
        worker.join(FETCH_TOTAL_TIMEOUT_MS)
        return if (worker.isAlive) null else payload
    }

    companion object {
        private const val POOL_SLOT = "android-node-pool:v1"
        private const val MAX_NODES = 200
        private const val MAX_PAYLOAD_BYTES = 8 * 1024 * 1024
        private const val FETCH_CONNECT_TIMEOUT_MS = 5_000
        private const val FETCH_READ_TIMEOUT_MS = 10_000
        private const val FETCH_TOTAL_TIMEOUT_MS = 12_000L
        private const val MAX_REDIRECTS = 5
        private val SUPPORTED_SCHEMES = setOf(
            "vless", "vmess", "trojan", "ss", "hysteria2", "hy2", "tuic",
        )

        fun isSupportedNodeUri(raw: String): Boolean {
            val scheme = raw.substringBefore("://", "").lowercase()
            return scheme in SUPPORTED_SCHEMES && raw.length in 8..16_384
        }

        internal fun decodeSubscriptionPayload(payload: String): List<String> {
            val direct = payload.lineSequence()
                .map(String::trim)
                .filter(::isSupportedNodeUri)
                .distinct()
                .take(MAX_NODES)
                .toList()
            if (direct.isNotEmpty()) return direct

            val compact = payload.filterNot(Char::isWhitespace)
            if (compact.isEmpty() || compact.length > MAX_PAYLOAD_BYTES) return emptyList()
            val decodedCandidates = sequenceOf(
                runCatching { Base64.getDecoder().decode(compact) }.getOrNull(),
                runCatching { Base64.getUrlDecoder().decode(compact) }.getOrNull(),
            )
            for (decoded in decodedCandidates) {
                if (decoded == null || decoded.size > MAX_PAYLOAD_BYTES) continue
                val decodedText = runCatching { String(decoded, Charsets.UTF_8) }.getOrNull() ?: continue
                val nodes = decodedText.lineSequence()
                    .map(String::trim)
                    .filter(::isSupportedNodeUri)
                    .distinct()
                    .take(MAX_NODES)
                    .toList()
                if (nodes.isNotEmpty()) return nodes
            }
            return emptyList()
        }

        private fun fetchHttpsPayload(sourceUrl: String): String? {
            var current = runCatching { URL(sourceUrl) }.getOrNull() ?: return null
            repeat(MAX_REDIRECTS + 1) { redirectIndex ->
                if (!current.protocol.equals("https", ignoreCase = true)) return null
                val connection = (current.openConnection() as? HttpsURLConnection) ?: return null
                try {
                    connection.instanceFollowRedirects = false
                    connection.connectTimeout = FETCH_CONNECT_TIMEOUT_MS
                    connection.readTimeout = FETCH_READ_TIMEOUT_MS
                    connection.requestMethod = "GET"
                    connection.setRequestProperty("User-Agent", "AMRI-VPN/0.1")
                    connection.connect()

                    when (connection.responseCode) {
                        in 200..299 -> {
                            val declaredLength = connection.contentLengthLong
                            if (declaredLength > MAX_PAYLOAD_BYTES) return null
                            val output = ByteArrayOutputStream()
                            connection.inputStream.use { input ->
                                val buffer = ByteArray(16 * 1024)
                                var total = 0
                                while (true) {
                                    val read = input.read(buffer)
                                    if (read < 0) break
                                    total += read
                                    if (total > MAX_PAYLOAD_BYTES) return null
                                    output.write(buffer, 0, read)
                                }
                            }
                            return runCatching { output.toString(StandardCharsets.UTF_8.name()) }.getOrNull()
                        }
                        in 300..399 -> {
                            if (redirectIndex >= MAX_REDIRECTS) return null
                            val location = connection.getHeaderField("Location") ?: return null
                            val next = runCatching { URL(current, location) }.getOrNull() ?: return null
                            if (!next.protocol.equals("https", ignoreCase = true)) return null
                            current = next
                        }
                        else -> return null
                    }
                } finally {
                    connection.disconnect()
                }
            }
            return null
        }

        fun safeLabel(raw: String, index: Int): String {
            val scheme = raw.substringBefore("://", "VPN").uppercase()
            val fragment = raw.substringAfterLast('#', "")
                .takeIf(String::isNotBlank)
                ?.let { encoded ->
                    runCatching {
                        URLDecoder.decode(encoded, StandardCharsets.UTF_8.name())
                    }.getOrNull()
                }
                ?.trim()
                ?.take(36)
            return fragment?.let { "$scheme · $it" } ?: "$scheme · ${index + 1}"
        }

        fun safeFingerprint(raw: String): String {
            val digest = MessageDigest.getInstance("SHA-256").digest(raw.toByteArray(Charsets.UTF_8))
            return digest.take(6).joinToString("") { byte -> "%02x".format(byte.toInt() and 0xff) }
        }
    }
}
