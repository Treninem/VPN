package ru.amri.vpn

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class VpnStateMachineTest {
    @Test
    fun lifecycle_reaches_ready_and_returns_to_idle() {
        val state = VpnStateMachine()
        assertTrue(state.startPreparing())
        state.serviceReady()
        assertEquals(VpnControllerState.SERVICE_READY, state.state)
        state.stop()
        assertEquals(VpnControllerState.IDLE, state.state)
    }

    @Test
    fun serviceReadyIsNotProtectedUntilExplicitGatePasses() {
        val state = VpnStateMachine()
        assertTrue(state.startPreparing())
        state.serviceReady()
        assertEquals(VpnControllerState.SERVICE_READY, state.state)

        state.protectionReady()
        assertEquals(VpnControllerState.PROTECTED, state.state)

        state.protectionLost()
        assertEquals(VpnControllerState.SERVICE_READY, state.state)
    }

    @Test
    fun duplicate_start_is_rejected_in_preparing_or_protected_state() {
        val state = VpnStateMachine()
        assertTrue(state.startPreparing())
        assertFalse(state.startPreparing())
        state.serviceReady()
        state.protectionReady()
        assertFalse(state.startPreparing())
    }

    @Test
    fun explicit_retry_can_recover_from_failed_bootstrap() {
        val state = VpnStateMachine()
        assertTrue(state.startPreparing())
        state.fail()

        assertTrue(state.startPreparing())
        state.serviceReady()
        assertEquals(VpnControllerState.SERVICE_READY, state.state)
    }
}
