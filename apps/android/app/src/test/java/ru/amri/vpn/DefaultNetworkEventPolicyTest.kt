package ru.amri.vpn

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class DefaultNetworkEventPolicyTest {
    @Test
    fun acceptsCapabilitiesOnlyFromCurrentDefaultNetwork() {
        assertTrue(DefaultNetworkEventPolicy.acceptsCapabilities(42L, 42L))
        assertFalse(DefaultNetworkEventPolicy.acceptsCapabilities(41L, 42L))
    }

    @Test
    fun rejectsCallbackWhenThereIsNoActiveDefaultNetwork() {
        assertFalse(DefaultNetworkEventPolicy.acceptsCapabilities(42L, null))
    }

    @Test
    fun rapidWifiCellularHandoffCannotMoveLeaseBackToStaleNetwork() {
        val wifi = 100L
        val cellular = 200L

        assertTrue(DefaultNetworkEventPolicy.acceptsCapabilities(wifi, wifi))
        assertFalse(DefaultNetworkEventPolicy.acceptsCapabilities(wifi, cellular))
        assertTrue(DefaultNetworkEventPolicy.acceptsCapabilities(cellular, cellular))
        assertFalse(DefaultNetworkEventPolicy.acceptsCapabilities(cellular, wifi))
    }
}
