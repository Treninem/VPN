package ru.amri.vpn

import org.junit.Assert.assertEquals
import org.junit.Test

class AndroidRoutingModePolicyContractTest {
    @Test
    fun publicModeIdsRemainStableForPreferences() {
        assertEquals(0, AndroidRoutingModePolicy.SMART)
        assertEquals(1, AndroidRoutingModePolicy.MANUAL)
    }
}
