package ru.amri.vpn

import org.junit.Assert.assertEquals
import org.junit.Test

class AndroidRoutingModePolicyBooleanMatrixTest {
    @Test
    fun decisionMatrixIsExplicit() {
        assertEquals(true, AndroidRoutingModePolicy.smartRoutingEnabled(0, true))
        assertEquals(false, AndroidRoutingModePolicy.smartRoutingEnabled(0, false))
        assertEquals(false, AndroidRoutingModePolicy.smartRoutingEnabled(1, true))
        assertEquals(false, AndroidRoutingModePolicy.smartRoutingEnabled(1, false))
    }
}
