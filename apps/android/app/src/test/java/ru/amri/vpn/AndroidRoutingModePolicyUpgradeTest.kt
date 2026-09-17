package ru.amri.vpn

import org.junit.Assert.assertFalse
import org.junit.Test

class AndroidRoutingModePolicyUpgradeTest {
    @Test
    fun everyLegacyExplicitModeUpgradesWithoutCrossNodeFailover() {
        listOf(1, 2, 3, 4, 5, 6).forEach { storedMode ->
            assertFalse(AndroidRoutingModePolicy.smartRoutingEnabled(storedMode, true))
        }
    }
}
