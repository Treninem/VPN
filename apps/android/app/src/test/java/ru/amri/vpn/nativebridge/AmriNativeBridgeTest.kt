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
}
