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
}
