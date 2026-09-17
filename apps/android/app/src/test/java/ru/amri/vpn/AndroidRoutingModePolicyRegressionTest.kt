package ru.amri.vpn

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class AndroidRoutingModePolicyRegressionTest {
    @Test
    fun regressionMatrix() {
        assertTrue(AndroidRoutingModePolicy.smartRoutingEnabled(0, true))
        assertFalse(AndroidRoutingModePolicy.smartRoutingEnabled(0, false))
        assertFalse(AndroidRoutingModePolicy.smartRoutingEnabled(1, true))
        assertFalse(AndroidRoutingModePolicy.smartRoutingEnabled(6, true))
    }
}
