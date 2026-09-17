package ru.amri.vpn

import org.junit.Assert.assertTrue
import org.junit.Test

class AndroidRoutingModePolicySmartTest {
    @Test
    fun smartModeWithSmartToggleEnablesRankingAndFailover() {
        assertTrue(AndroidRoutingModePolicy.smartRoutingEnabled(AndroidRoutingModePolicy.SMART, true))
    }
}
