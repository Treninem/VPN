package ru.amri.vpn

import org.junit.Assert.assertFalse
import org.junit.Test

class AndroidRoutingModePolicyDisabledSmartTest {
    @Test
    fun smartModeWithoutSmartToggleDoesNotRankAlternativeNodes() {
        assertFalse(AndroidRoutingModePolicy.smartRoutingEnabled(AndroidRoutingModePolicy.SMART, false))
    }
}
