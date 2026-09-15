package ru.amri.vpn

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import ru.amri.vpn.nativebridge.RouteProofKeyState

class AndroidRuntimeOwnerTest {
    @Test
    fun successfulInitializationIsOwnedAndPerformedOnce() {
        var calls = 0
        val owner = AndroidRuntimeOwner {
            calls += 1
            RouteProofKeyState.CREATED
        }

        assertTrue(owner.initialize())
        assertTrue(owner.initialize())
        assertEquals(AndroidRuntimeState.READY, owner.state)
        assertEquals(1, calls)
    }

    @Test
    fun initializationFailsClosedAndCanRetry() {
        var calls = 0
        val owner = AndroidRuntimeOwner {
            calls += 1
            if (calls == 1) throw IllegalStateException("sensitive platform detail")
            RouteProofKeyState.EXISTING
        }

        assertFalse(owner.initialize())
        assertEquals(AndroidRuntimeState.FAILED, owner.state)
        assertTrue(owner.initialize())
        assertEquals(AndroidRuntimeState.READY, owner.state)
        assertEquals(2, calls)
    }
}
