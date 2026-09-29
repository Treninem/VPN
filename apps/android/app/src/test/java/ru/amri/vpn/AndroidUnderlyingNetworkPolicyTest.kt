package ru.amri.vpn

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class AndroidUnderlyingNetworkPolicyTest {
    @Test
    fun keepsCurrentPhysicalNetworkWhenItIsStillAvailable() {
        val wifi = choice("wifi", validated = true, wifi = true)
        val cell = choice("cell", validated = true, cellular = true)

        assertEquals("cell", AndroidUnderlyingNetworkPolicy.select("cell", listOf(wifi, cell)))
    }

    @Test
    fun ignoresVpnActiveNetworkAndPrefersValidatedPhysicalNetwork() {
        val wifi = choice("wifi", validated = true, wifi = true)
        val cell = choice("cell", validated = false, cellular = true)

        assertEquals("wifi", AndroidUnderlyingNetworkPolicy.select("amri-vpn", listOf(cell, wifi)))
    }

    @Test
    fun validatedCellularBeatsUnvalidatedWifi() {
        val wifi = choice("wifi", validated = false, wifi = true)
        val cell = choice("cell", validated = true, cellular = true)

        assertEquals("cell", AndroidUnderlyingNetworkPolicy.select(null, listOf(wifi, cell)))
    }

    @Test
    fun noPhysicalNetworkReturnsNull() {
        assertNull(AndroidUnderlyingNetworkPolicy.select<String>("amri-vpn", emptyList()))
    }

    private fun choice(
        value: String,
        validated: Boolean,
        wifi: Boolean = false,
        cellular: Boolean = false,
    ) = UnderlyingNetworkChoice(
        value = value,
        validated = validated,
        wifi = wifi,
        ethernet = false,
        cellular = cellular,
    )
}
