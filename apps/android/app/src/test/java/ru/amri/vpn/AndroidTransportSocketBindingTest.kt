package ru.amri.vpn

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class AndroidTransportSocketBindingTest {
    @Test
    fun missing_network_fails_without_touching_socket() {
        var calls = 0
        val prepared = TransportSocketGate.prepare<String, String>(
            network = null,
            socket = "socket",
            protect = { calls += 1; true },
            bind = { _, _ -> calls += 1 },
            isCurrent = { true },
        )
        assertFalse(prepared)
        assertEquals(0, calls)
    }

    @Test
    fun protection_failure_prevents_underlying_network_bind() {
        var bound = false
        val prepared = TransportSocketGate.prepare(
            network = "wifi",
            socket = "socket",
            protect = { false },
            bind = { _, _ -> bound = true },
            isCurrent = { true },
        )
        assertFalse(prepared)
        assertFalse(bound)
    }

    @Test
    fun stale_network_lease_is_rejected_after_bind() {
        val prepared = TransportSocketGate.prepare(
            network = "wifi",
            socket = "socket",
            protect = { true },
            bind = { _, _ -> },
            isCurrent = { false },
        )
        assertFalse(prepared)
    }

    @Test
    fun current_protected_bound_socket_is_admitted() {
        val prepared = TransportSocketGate.prepare(
            network = "wifi",
            socket = "socket",
            protect = { true },
            bind = { _, _ -> },
            isCurrent = { true },
        )
        assertTrue(prepared)
    }
}
