package ru.amri.vpn

import org.junit.Assert.assertFalse
import org.junit.Test

class AndroidRoutingModePolicyManualTest {
    @Test
    fun manualModeIgnoresSmartToggle() {
        assertFalse(AndroidRoutingModePolicy.smartRoutingEnabled(AndroidRoutingModePolicy.MANUAL, true))
        assertFalse(AndroidRoutingModePolicy.smartRoutingEnabled(AndroidRoutingModePolicy.MANUAL, false))
    }
}
