package ru.amri.vpn

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class MainScreenPresentationTest {
    @Test
    fun only_verified_protected_state_uses_on_artwork() {
        for (state in VpnControllerState.entries) {
            assertEquals(state == VpnControllerState.PROTECTED, presentControllerState(state).powerOn)
        }
    }

    @Test
    fun preparing_is_disabled_and_failure_retries() {
        val preparing = presentControllerState(VpnControllerState.PREPARING)
        assertFalse(preparing.actionEnabled)
        assertEquals(MainScreenAction.NONE, preparing.action)

        val failed = presentControllerState(VpnControllerState.FAILED)
        assertTrue(failed.actionEnabled)
        assertEquals(MainScreenAction.RETRY, failed.action)
    }

    @Test
    fun control_only_service_can_stop_but_never_looks_protected() {
        val ready = presentControllerState(VpnControllerState.SERVICE_READY)
        assertEquals(MainScreenAction.STOP, ready.action)
        assertFalse(ready.powerOn)
    }
}
