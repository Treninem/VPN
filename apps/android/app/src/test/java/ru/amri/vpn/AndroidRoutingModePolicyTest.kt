package ru.amri.vpn

import org.junit.Assert.assertFalse
import org.junit.Assert.assertEquals
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
            assertEquals(AndroidRoutingModePolicy.MANUAL, AndroidRoutingModePolicy.normalize(legacyMode))
        }
    }

    @Test
    fun unexpectedStoredModesNormalizeToManual() {
        assertEquals(AndroidRoutingModePolicy.MANUAL, AndroidRoutingModePolicy.normalize(-1))
        assertEquals(AndroidRoutingModePolicy.MANUAL, AndroidRoutingModePolicy.normalize(Int.MAX_VALUE))
    }
}
