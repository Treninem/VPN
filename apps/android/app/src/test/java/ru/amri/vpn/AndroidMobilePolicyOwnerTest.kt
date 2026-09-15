package ru.amri.vpn

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Test
import ru.amri.vpn.nativebridge.AccessNetworkKind
import ru.amri.vpn.nativebridge.MobileNetworkSnapshot
import ru.amri.vpn.nativebridge.MobilePathPolicy
import ru.amri.vpn.nativebridge.ProbeIntensity

class AndroidMobilePolicyOwnerTest {
    private val snapshot = MobileNetworkSnapshot(
        kind = AccessNetworkKind.CELLULAR,
        validated = true,
        metered = true,
        roaming = false,
        dataSaver = false,
        batterySaver = false,
        estimatedDownstreamKbps = 80_000,
        estimatedUpstreamKbps = 20_000,
    )

    @Test
    fun storesPolicyReturnedBySharedCoreBoundary() {
        val expected = MobilePathPolicy(ProbeIntensity.CONSERVATIVE, true, false, false)
        val owner = AndroidMobilePolicyOwner({ _, _ -> expected })

        owner.update(snapshot)

        assertEquals(expected, owner.latestPolicy)
    }

    @Test
    fun nativeFailureDisablesWarmupAndSecondaryPaths() {
        val owner = AndroidMobilePolicyOwner({ _, _ -> error("native unavailable") })

        owner.update(snapshot)

        assertEquals(ProbeIntensity.MINIMAL, owner.latestPolicy.probeIntensity)
        assertFalse(owner.latestPolicy.allowBackgroundWarmup)
        assertFalse(owner.latestPolicy.allowSecondaryPath)
        assertFalse(owner.latestPolicy.allowLatencyDuplication)
    }
}
