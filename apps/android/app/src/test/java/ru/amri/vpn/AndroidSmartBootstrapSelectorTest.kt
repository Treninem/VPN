package ru.amri.vpn

import java.util.Base64
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class AndroidSmartBootstrapSelectorTest {
    @Test
    fun manualModeUsesOnlyPreferredNode() {
        val nodes = listOf(
            "vless://id@203.0.113.10:443?security=tls#A",
            "trojan://pw@203.0.113.11:443#B",
        )

        assertEquals(
            listOf(1),
            AndroidSmartBootstrapSelector.candidateOrder(nodes, 1, false) { _, _, _ -> 1L },
        )
    }

    @Test
    fun smartModeChoosesFastestReachableTcpCandidate() {
        val nodes = listOf(
            "vless://id@203.0.113.10:443?security=tls#A",
            "trojan://pw@203.0.113.11:443#B",
            "vless://id@203.0.113.12:443?security=tls#C",
        )
        val latency = mapOf(
            "203.0.113.10" to 80L,
            "203.0.113.11" to 15L,
            "203.0.113.12" to 40L,
        )

        val order = AndroidSmartBootstrapSelector.candidateOrder(nodes, 0, true) { host, _, _ ->
            latency[host]
        }

        assertEquals(1, order.first())
        assertEquals(3, order.distinct().size)
    }

    @Test
    fun probePoolIsBoundedAndPreferredFirst() {
        val nodes = (0 until 12).map { index ->
            "vless://id@203.0.113.${index + 1}:443?security=tls#$index"
        }

        val targets = AndroidSmartBootstrapSelector.buildProbeTargets(nodes, 9)

        assertEquals(4, targets.size)
        assertEquals(9, targets.first().index)
    }

    @Test
    fun udpOnlyPreferredNodeIsNotDisplacedByTcpRanking() {
        val nodes = listOf(
            "hysteria2://pw@203.0.113.40:443#udp",
            "vless://id@203.0.113.10:443?security=tls#tcp",
        )

        val order = AndroidSmartBootstrapSelector.candidateOrder(nodes, 0, true) { _, _, _ -> 1L }

        assertEquals(0, order.first())
    }

    @Test
    fun vmessJsonEndpointCanBeProbedWithoutExposingCredentials() {
        val json = "{\"v\":\"2\",\"ps\":\"node\",\"add\":\"198.51.100.7\",\"port\":\"8443\",\"id\":\"secret-id\"}"
        val raw = "vmess://" + Base64.getEncoder().encodeToString(json.toByteArray())

        val targets = AndroidSmartBootstrapSelector.buildProbeTargets(listOf(raw), 0)

        assertEquals(1, targets.size)
        assertEquals("198.51.100.7", targets[0].host)
        assertEquals(8443, targets[0].port)
        assertTrue(targets[0].host.contains("secret-id").not())
    }
}
