package ru.amri.vpn

import org.junit.Assert.assertFalse
import org.junit.Test

class AndroidRoutingModePolicyNonSmartTest {
    @Test
    fun allNonSmartValuesDisableCrossNodeRouting() {
        listOf(-10, -1, 1, 2, 3, 4, 5, 6, 7, 99).forEach { mode ->
            assertFalse(AndroidRoutingModePolicy.smartRoutingEnabled(mode, true))
        }
    }
}
