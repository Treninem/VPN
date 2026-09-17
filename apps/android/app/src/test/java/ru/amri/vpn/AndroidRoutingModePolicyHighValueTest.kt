package ru.amri.vpn

import org.junit.Assert.assertFalse
import org.junit.Test

class AndroidRoutingModePolicyHighValueTest {
    @Test
    fun corruptHighModeFailsClosedToManualBehavior() {
        assertFalse(AndroidRoutingModePolicy.smartRoutingEnabled(Int.MAX_VALUE, true))
    }
}
