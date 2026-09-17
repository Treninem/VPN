package ru.amri.vpn

import org.junit.Assert.assertEquals
import org.junit.Test

class AndroidRoutingModePolicyIdempotenceTest {
    @Test
    fun routingDecisionIsDeterministicForSamePreferenceState() {
        val first = AndroidRoutingModePolicy.smartRoutingEnabled(AndroidRoutingModePolicy.SMART, true)
        val second = AndroidRoutingModePolicy.smartRoutingEnabled(AndroidRoutingModePolicy.SMART, true)
        assertEquals(first, second)
    }
}
