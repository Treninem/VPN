package ru.amri.vpn

import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test

class AndroidPublicTunnelConfigTest {
    @Test
    fun acceptsLoopbackTransportPortAndIpv6SafeMtu() {
        val config = AndroidPublicTunnelConfig(localSocksPort = 20800, safeInitialMtu = 1420)
        assertEquals(20800, config.localSocksPort)
        assertEquals(1420, config.safeInitialMtu)
    }

    @Test
    fun rejectsInvalidPort() {
        assertThrows(IllegalArgumentException::class.java) {
            AndroidPublicTunnelConfig(localSocksPort = 0, safeInitialMtu = 1420)
        }
        assertThrows(IllegalArgumentException::class.java) {
            AndroidPublicTunnelConfig(localSocksPort = 65_536, safeInitialMtu = 1420)
        }
    }

    @Test
    fun rejectsMtuOutsideSharedControllerBounds() {
        assertThrows(IllegalArgumentException::class.java) {
            AndroidPublicTunnelConfig(localSocksPort = 20800, safeInitialMtu = 1279)
        }
        assertThrows(IllegalArgumentException::class.java) {
            AndroidPublicTunnelConfig(localSocksPort = 20800, safeInitialMtu = 1501)
        }
    }
}
