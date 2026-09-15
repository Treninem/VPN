package ru.amri.vpn.nativebridge

import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test

class AmriNativeBridgeTest {
    @Test
    fun existingAndCreatedStatusesAreDistinctSuccesses() {
        assertEquals(
            RouteProofKeyState.EXISTING,
            AmriNativeBridge.decodeRouteProofKeyStatus(0),
        )
        assertEquals(
            RouteProofKeyState.CREATED,
            AmriNativeBridge.decodeRouteProofKeyStatus(1),
        )
    }

    @Test
    fun invalidStoredKeyFailsClosed() {
        assertThrows(NativeBridgeSecurityException::class.java) {
            AmriNativeBridge.decodeRouteProofKeyStatus(2)
        }
    }

    @Test
    fun entropyFailureFailsClosed() {
        assertThrows(NativeBridgeSecurityException::class.java) {
            AmriNativeBridge.decodeRouteProofKeyStatus(3)
        }
    }

    @Test
    fun unknownNativeStatusFailsClosed() {
        assertThrows(NativeBridgeSecurityException::class.java) {
            AmriNativeBridge.decodeRouteProofKeyStatus(99)
        }
    }

    @Test
    fun mobilePolicyBitFieldIsDecoded() {
        val policy = AmriNativeBridge.decodeMobilePolicy(
            1 or (1 shl 2),
        )

        assertEquals(ProbeIntensity.CONSERVATIVE, policy.probeIntensity)
        assertEquals(true, policy.allowBackgroundWarmup)
        assertEquals(false, policy.allowSecondaryPath)
        assertEquals(false, policy.allowLatencyDuplication)
    }

    @Test
    fun invalidMobilePolicyFailsClosed() {
        assertThrows(NativeBridgeSecurityException::class.java) {
            AmriNativeBridge.decodeMobilePolicy(-1)
        }
        assertThrows(NativeBridgeSecurityException::class.java) {
            AmriNativeBridge.decodeMobilePolicy(1 shl 8)
        }
        assertThrows(NativeBridgeSecurityException::class.java) {
            AmriNativeBridge.decodeMobilePolicy(3)
        }
    }

    @Test
    fun packetForwarderStatesAreStrictlyDecoded() {
        assertEquals(PacketForwarderState.STOPPED, AmriNativeBridge.decodePacketForwarderState(0))
        assertEquals(PacketForwarderState.STARTING, AmriNativeBridge.decodePacketForwarderState(1))
        assertEquals(PacketForwarderState.RUNNING, AmriNativeBridge.decodePacketForwarderState(2))
        assertEquals(PacketForwarderState.FAILED, AmriNativeBridge.decodePacketForwarderState(3))
        assertEquals(PacketForwarderState.STOPPING, AmriNativeBridge.decodePacketForwarderState(4))
        assertThrows(NativeBridgeSecurityException::class.java) {
            AmriNativeBridge.decodePacketForwarderState(5)
        }
    }

    @Test
    fun packetForwarderStartErrorsFailClosed() {
        AmriNativeBridge.decodePacketForwarderStart(0)
        for (status in -1 downTo -5) {
            assertThrows(PacketForwarderStartException::class.java) {
                AmriNativeBridge.decodePacketForwarderStart(status)
            }
        }
        assertThrows(NativeBridgeSecurityException::class.java) {
            AmriNativeBridge.decodePacketForwarderStart(-99)
        }
    }
}
