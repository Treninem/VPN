package ru.amri.vpn

import java.util.Base64
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class AndroidNodeStoreTest {
    @Test
    fun directNodeLinksAreAccepted() {
        val payload = "vless://id@example.com:443?security=tls#NL\ntrojan://pw@example.org:443#DE"
        val nodes = AndroidNodeStore.decodeSubscriptionPayload(payload)

        assertEquals(2, nodes.size)
        assertTrue(nodes[0].startsWith("vless://"))
        assertTrue(nodes[1].startsWith("trojan://"))
    }

    @Test
    fun standardBase64ProviderPayloadIsDecoded() {
        val plain = "vless://id@example.com:443?security=tls#NL\nhysteria2://pw@example.org:443#FI"
        val payload = Base64.getEncoder().encodeToString(plain.toByteArray(Charsets.UTF_8))
        val nodes = AndroidNodeStore.decodeSubscriptionPayload(payload)

        assertEquals(2, nodes.size)
        assertTrue(nodes.any { it.startsWith("vless://") })
        assertTrue(nodes.any { it.startsWith("hysteria2://") })
    }

    @Test
    fun providerUrlIsNeverStoredAsNode() {
        val nodes = AndroidNodeStore.decodeSubscriptionPayload(
            "https://provider.example/subscription?token=secret",
        )

        assertTrue(nodes.isEmpty())
    }
}
