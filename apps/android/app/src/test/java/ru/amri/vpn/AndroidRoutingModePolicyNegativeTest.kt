package ru.amri.vpn

import org.junit.Assert.assertFalse
import org.junit.Test

class AndroidRoutingModePolicyNegativeTest {
    @Test
    fun corruptNegativeModeFailsClosedToManualBehavior() {
        assertFalse(AndroidRoutingModePolicy.smartRoutingEnabled(-1, true))
    }
}
