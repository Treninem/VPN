package ru.amri.vpn

import android.content.Context
import org.json.JSONArray
import ru.amri.vpn.security.AndroidKeystoreSecretStore
import java.net.URLDecoder
import java.nio.charset.StandardCharsets
import java.security.MessageDigest

/**
 * Encrypted Android persistence for credential-bearing node URIs.
 *
 * Raw URIs never enter ordinary SharedPreferences, intents, logs or process arguments. The complete
 * JSON payload is encrypted by AndroidKeystoreSecretStore. Only the selected index is kept in the
 * non-secret UI preference file.
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
        val merged = existing.toMutableList()
        text.lineSequence()
            .map(String::trim)
            .filter(::isSupportedNodeUri)
            .forEach { uri -> if (uri !in merged && merged.size < MAX_NODES) merged += uri }
        save(merged)
        return merged
    }

    companion object {
        private const val POOL_SLOT = "android-node-pool:v1"
        private const val MAX_NODES = 200
        private val SUPPORTED_SCHEMES = setOf(
            "vless", "vmess", "trojan", "ss", "hysteria2", "hy2", "tuic",
        )

        fun isSupportedNodeUri(raw: String): Boolean {
            val scheme = raw.substringBefore("://", "").lowercase()
            return scheme in SUPPORTED_SCHEMES && raw.length in 8..16_384
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
