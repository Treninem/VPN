package ru.amri.vpn

import java.net.InetSocketAddress
import java.net.Socket
import java.net.URI
import java.nio.charset.StandardCharsets
import java.util.Base64
import java.util.concurrent.ExecutorCompletionService
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit

internal data class AndroidBootstrapProbeTarget(
    val index: Int,
    val host: String,
    val port: Int,
)

/**
 * Privacy-safe bootstrap selector used before the public Android TUN is activated.
 *
 * It probes only provider endpoints from the encrypted local node pool. It never inspects user
 * destinations, never persists probe targets, and never logs raw URIs or credentials. UDP-only
 * protocols are deliberately not ranked with a fake TCP probe.
 */
internal object AndroidSmartBootstrapSelector {
    private const val MAX_PROBE_CANDIDATES = 4
    private const val MAX_FAILOVER_CANDIDATES = 8
    private const val PER_TARGET_TIMEOUT_MS = 650
    private const val OVERALL_TIMEOUT_MS = 900L

    fun candidateOrder(
        nodes: List<String>,
        preferredIndex: Int,
        smartRouting: Boolean,
        probe: (host: String, port: Int, timeoutMs: Int) -> Long? = ::tcpConnectLatencyMs,
    ): List<Int> {
        if (nodes.isEmpty()) return emptyList()
        val preferred = preferredIndex.coerceIn(nodes.indices)
        if (!smartRouting) return listOf(preferred)

        val targets = buildProbeTargets(nodes, preferred)
        val preferredIsProbeable = targets.any { it.index == preferred }
        val winner = if (preferredIsProbeable && targets.size >= 2) {
            firstReachable(targets, probe)?.index
        } else {
            null
        }

        return buildList {
            add(winner ?: preferred)
            if (preferred !in this) add(preferred)
            for (index in nodes.indices) {
                if (index !in this) add(index)
                if (size >= MAX_FAILOVER_CANDIDATES) break
            }
        }
    }

    internal fun buildProbeTargets(
        nodes: List<String>,
        preferredIndex: Int,
    ): List<AndroidBootstrapProbeTarget> {
        if (nodes.isEmpty()) return emptyList()
        val preferred = preferredIndex.coerceIn(nodes.indices)
        val order = sequenceOf(preferred) + nodes.indices.asSequence().filter { it != preferred }
        return order
            .mapNotNull { index -> tcpTarget(nodes[index], index) }
            .take(MAX_PROBE_CANDIDATES)
            .toList()
    }

    private fun firstReachable(
        targets: List<AndroidBootstrapProbeTarget>,
        probe: (String, Int, Int) -> Long?,
    ): AndroidBootstrapProbeTarget? {
        if (targets.isEmpty()) return null
        val executor = Executors.newFixedThreadPool(targets.size)
        val completion = ExecutorCompletionService<Pair<AndroidBootstrapProbeTarget, Long?>>(executor)
        val deadline = System.nanoTime() + TimeUnit.MILLISECONDS.toNanos(OVERALL_TIMEOUT_MS)
        return try {
            targets.forEach { target ->
                completion.submit<Pair<AndroidBootstrapProbeTarget, Long?>> {
                    target to probe(target.host, target.port, PER_TARGET_TIMEOUT_MS)
                }
            }

            repeat(targets.size) {
                val remaining = deadline - System.nanoTime()
                if (remaining <= 0L) return@repeat
                val future = completion.poll(remaining, TimeUnit.NANOSECONDS) ?: return@repeat
                val result = runCatching { future.get() }.getOrNull() ?: return@repeat
                if (result.second != null) return result.first
            }
            null
        } finally {
            executor.shutdownNow()
        }
    }

    private fun tcpTarget(raw: String, index: Int): AndroidBootstrapProbeTarget? {
        val scheme = raw.substringBefore("://", "").lowercase()
        val endpoint = when (scheme) {
            "vless", "trojan", "ss" -> uriEndpoint(raw)
            "vmess" -> vmessEndpoint(raw)
            // Hysteria2/TUIC are UDP/QUIC transports; TCP reachability would be misleading.
            "hysteria2", "hy2", "tuic" -> null
            else -> null
        } ?: return null
        return AndroidBootstrapProbeTarget(index, endpoint.first, endpoint.second)
    }

    private fun uriEndpoint(raw: String): Pair<String, Int>? {
        val uri = runCatching { URI(raw) }.getOrNull() ?: return null
        val host = uri.host?.takeIf { it.isNotBlank() } ?: return null
        val port = uri.port.takeIf { it in 1..65535 } ?: return null
        return host to port
    }

    private fun vmessEndpoint(raw: String): Pair<String, Int>? {
        val payload = raw.substringAfter("://", "").substringBefore('#').filterNot(Char::isWhitespace)
        if (payload.isBlank() || payload.length > 64 * 1024) return null
        val padded = payload + "=".repeat((4 - payload.length % 4) % 4)
        val decoded = sequenceOf(Base64.getDecoder(), Base64.getUrlDecoder())
            .mapNotNull { decoder -> runCatching { decoder.decode(padded) }.getOrNull() }
            .mapNotNull { bytes -> runCatching { String(bytes, StandardCharsets.UTF_8) }.getOrNull() }
            .firstOrNull() ?: return null
        val host = Regex("\\\"add\\\"\\s*:\\s*\\\"([^\\\"]+)\\\"")
            .find(decoded)?.groupValues?.getOrNull(1)?.takeIf { it.isNotBlank() } ?: return null
        val portMatch = Regex("\\\"port\\\"\\s*:\\s*(?:\\\"(\\d+)\\\"|(\\d+))")
            .find(decoded) ?: return null
        val port = (portMatch.groupValues.getOrNull(1)?.takeIf { it.isNotBlank() }
            ?: portMatch.groupValues.getOrNull(2))?.toIntOrNull()?.takeIf { it in 1..65535 }
            ?: return null
        return host to port
    }

    private fun tcpConnectLatencyMs(host: String, port: Int, timeoutMs: Int): Long? {
        val started = System.nanoTime()
        return runCatching {
            Socket().use { socket ->
                socket.connect(InetSocketAddress(host, port), timeoutMs)
            }
            TimeUnit.NANOSECONDS.toMillis(System.nanoTime() - started).coerceAtLeast(1L)
        }.getOrNull()
    }
}
