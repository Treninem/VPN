package ru.amri.vpn

import org.junit.Assert.assertFalse
import org.junit.Test

class AndroidRoutingModePolicyToggleTest {
    @Test
    fun smartModeRespectsExplicitSmartRoutingOffPreference() {
        assertFalse(AndroidRoutingModePolicy.smartRoutingEnabled(0, false))
    }
}
