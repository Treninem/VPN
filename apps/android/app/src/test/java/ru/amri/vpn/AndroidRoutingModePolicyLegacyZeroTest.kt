package ru.amri.vpn

import org.junit.Assert.assertTrue
import org.junit.Test

class AndroidRoutingModePolicyLegacyZeroTest {
    @Test
    fun legacyDefaultZeroRemainsSmartWhenSmartRoutingIsEnabled() {
        assertTrue(AndroidRoutingModePolicy.smartRoutingEnabled(0, true))
    }
}
