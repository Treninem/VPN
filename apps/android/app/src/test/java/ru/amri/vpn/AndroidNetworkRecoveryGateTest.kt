package ru.amri.vpn

import org.junit.Assert.assertEquals
import org.junit.Test

class AndroidNetworkRecoveryGateTest {
    @Test
    fun firstObservationDoesNotRestartProtectedGeneration() {
        val gate = AndroidNetworkRecoveryGate()

        assertEquals(
            NetworkRecoveryAction.NONE,
            gate.observe(10L, VpnControllerState.PROTECTED),
        )
    }

    @Test
    fun sameNetworkDoesNotRestartProtectedGeneration() {
        val gate = AndroidNetworkRecoveryGate()
        gate.observe(10L, VpnControllerState.SERVICE_READY)

        assertEquals(
            NetworkRecoveryAction.NONE,
            gate.observe(10L, VpnControllerState.PROTECTED),
        )
    }

    @Test
    fun protectedNetworkSwitchRequiresFullRestart() {
        val gate = AndroidNetworkRecoveryGate()
        gate.observe(10L, VpnControllerState.SERVICE_READY)

        assertEquals(
            NetworkRecoveryAction.RESTART_PROTECTED_PATH,
            gate.observe(20L, VpnControllerState.PROTECTED),
        )
    }

    @Test
    fun networkLossCutsPathAndAvailabilityRestartsIt() {
        val gate = AndroidNetworkRecoveryGate()
        gate.observe(10L, VpnControllerState.SERVICE_READY)

        assertEquals(
            NetworkRecoveryAction.CUT_PROTECTED_PATH,
            gate.observe(null, VpnControllerState.PROTECTED),
        )
        assertEquals(
            NetworkRecoveryAction.RESTART_PROTECTED_PATH,
            gate.observe(20L, VpnControllerState.SERVICE_READY),
        )
    }

    @Test
    fun networkReturnAfterTeardownCompletedStillRestarts() {
        val gate = AndroidNetworkRecoveryGate()
        gate.observe(10L, VpnControllerState.SERVICE_READY)
        gate.observe(null, VpnControllerState.PROTECTED)

        assertEquals(
            NetworkRecoveryAction.RESTART_PROTECTED_PATH,
            gate.observe(20L, VpnControllerState.FAILED),
        )
    }

    @Test
    fun resetDropsPreviousNetworkIdentity() {
        val gate = AndroidNetworkRecoveryGate()
        gate.observe(10L, VpnControllerState.SERVICE_READY)
        gate.reset()

        assertEquals(
            NetworkRecoveryAction.NONE,
            gate.observe(20L, VpnControllerState.PROTECTED),
        )
    }
}
