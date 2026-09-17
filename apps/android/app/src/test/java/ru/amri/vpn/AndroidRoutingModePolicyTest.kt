package ru.amri.vpn

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class AndroidRoutingModePolicyTest {
    @Test
    fun smartModeUsesSmartRoutingWhenEnabled() {
        assertTrue(AndroidRoutingModePolicy.smartRoutingEnabled(AndroidRoutingModePolicy.SMART, true))
    }

    @Test
    fun smartModeCanDisableCrossNodeFailover() {
        assertFalse(AndroidRoutingModePolicy.smartRoutingEnabled(AndroidRoutingModePolicy.SMART, false))
    }

    @Test
    fun manualModeNeverEnablesCrossNodeFailover() {
        assertFalse(AndroidRoutingModePolicy.smartRoutingEnabled(AndroidRoutingModePolicy.MANUAL, true))
    }

    @Test
    fun legacyNonZeroModesFailClosedToManualBehavior() {
        for (legacyMode in 2..6) {
            assertFalse(AndroidRoutingModePolicy.smartRoutingEnabled(legacyMode, true))
        }
    }
}
