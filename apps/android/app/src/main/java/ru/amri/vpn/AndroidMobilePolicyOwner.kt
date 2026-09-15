package ru.amri.vpn

import ru.amri.vpn.nativebridge.AmriNativeBridge
import ru.amri.vpn.nativebridge.MobileAccelerationPreferences
import ru.amri.vpn.nativebridge.MobileNetworkSnapshot
import ru.amri.vpn.nativebridge.MobilePathPolicy
import ru.amri.vpn.nativebridge.ProbeIntensity

/** Keeps live Android observations behind the shared Rust policy boundary. */
internal class AndroidMobilePolicyOwner(
    private val evaluator: (MobileNetworkSnapshot, MobileAccelerationPreferences) -> MobilePathPolicy,
    private val preferences: MobileAccelerationPreferences = MobileAccelerationPreferences(),
) {
    @Volatile
    var latestPolicy: MobilePathPolicy = CONSERVATIVE_FALLBACK
        private set

    fun update(snapshot: MobileNetworkSnapshot) {
        latestPolicy = try {
            evaluator(snapshot, preferences)
        } catch (_: RuntimeException) {
            CONSERVATIVE_FALLBACK
        }
    }

    companion object {
        val CONSERVATIVE_FALLBACK = MobilePathPolicy(
            probeIntensity = ProbeIntensity.MINIMAL,
            allowBackgroundWarmup = false,
            allowSecondaryPath = false,
            allowLatencyDuplication = false,
        )

        fun production(): AndroidMobilePolicyOwner = AndroidMobilePolicyOwner(
            evaluator = AmriNativeBridge::evaluateMobilePolicy,
        )
    }
}
