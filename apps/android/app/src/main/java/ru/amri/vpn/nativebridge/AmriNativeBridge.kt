package ru.amri.vpn.nativebridge

import ru.amri.vpn.security.AndroidKeystoreSecretStore

enum class RouteProofKeyState {
    EXISTING,
    CREATED,
}

enum class AccessNetworkKind(val nativeCode: Int) {
    WIFI(0),
    CELLULAR(1),
    ETHERNET(2),
    OTHER(3),
}

enum class MobileAccelerationMode(val nativeCode: Int) {
    OFF(0),
    BALANCED(1),
    SPEED(2),
}

data class MobileNetworkSnapshot(
    val kind: AccessNetworkKind,
    val validated: Boolean,
    val metered: Boolean,
    val roaming: Boolean,
    val dataSaver: Boolean,
    val batterySaver: Boolean,
    val estimatedDownstreamKbps: Int?,
    val estimatedUpstreamKbps: Int?,
)

data class MobileAccelerationPreferences(
    val mode: MobileAccelerationMode = MobileAccelerationMode.BALANCED,
    val allowMeteredSecondary: Boolean = false,
    val allowLatencyDuplication: Boolean = false,
)

enum class ProbeIntensity {
    MINIMAL,
    CONSERVATIVE,
    NORMAL,
}

data class MobilePathPolicy(
    val probeIntensity: ProbeIntensity,
    val allowBackgroundWarmup: Boolean,
    val allowSecondaryPath: Boolean,
    val allowLatencyDuplication: Boolean,
)

class NativeBridgeUnavailableException : IllegalStateException(
    "AMRI native runtime is not packaged for this Android build",
)

class NativeBridgeSecurityException(message: String) : IllegalStateException(message)

/**
 * Narrow Kotlin -> Rust entry point.
 *
 * The bridge never serializes the credential database. Rust receives the platform secret-store
 * object and asks it only for the exact logical slot needed by the operation.
 */
object AmriNativeBridge {
    private const val LIBRARY_NAME = "amri_android_ffi"
    private const val STATUS_EXISTING = 0
    private const val STATUS_CREATED = 1
    private const val STATUS_INVALID_STORED_KEY = 2
    private const val STATUS_ENTROPY_FAILURE = 3

    @Volatile
    private var loadAttempted = false

    @Volatile
    private var libraryLoaded = false

    fun isAvailable(): Boolean {
        ensureLoadAttempted()
        return libraryLoaded
    }

    fun ensureRouteProofKey(store: AndroidKeystoreSecretStore): RouteProofKeyState {
        ensureLoadAttempted()
        if (!libraryLoaded) {
            throw NativeBridgeUnavailableException()
        }
        return decodeRouteProofKeyStatus(nativeEnsureRouteProofKey(store))
    }

    fun evaluateMobilePolicy(
        snapshot: MobileNetworkSnapshot,
        preferences: MobileAccelerationPreferences = MobileAccelerationPreferences(),
    ): MobilePathPolicy {
        ensureLoadAttempted()
        if (!libraryLoaded) throw NativeBridgeUnavailableException()
        return decodeMobilePolicy(
            nativeEvaluateMobilePolicy(
                snapshot.kind.nativeCode,
                snapshot.validated,
                snapshot.metered,
                snapshot.roaming,
                snapshot.dataSaver,
                snapshot.batterySaver,
                snapshot.estimatedDownstreamKbps ?: UNKNOWN_BANDWIDTH,
                snapshot.estimatedUpstreamKbps ?: UNKNOWN_BANDWIDTH,
                preferences.mode.nativeCode,
                preferences.allowMeteredSecondary,
                preferences.allowLatencyDuplication,
            ),
        )
    }

    internal fun decodeRouteProofKeyStatus(status: Int): RouteProofKeyState = when (status) {
        STATUS_EXISTING -> RouteProofKeyState.EXISTING
        STATUS_CREATED -> RouteProofKeyState.CREATED
        STATUS_INVALID_STORED_KEY -> throw NativeBridgeSecurityException(
            "stored Route Proof key has an unexpected length",
        )
        STATUS_ENTROPY_FAILURE -> throw NativeBridgeSecurityException(
            "secure random generator failed",
        )
        else -> throw NativeBridgeSecurityException("unknown native security status")
    }

    internal fun decodeMobilePolicy(packed: Int): MobilePathPolicy {
        if (packed < 0 || packed and POLICY_RESERVED_BITS != 0) {
            throw NativeBridgeSecurityException("invalid native mobile policy")
        }
        val probeIntensity = when (packed and POLICY_PROBE_MASK) {
            0 -> ProbeIntensity.MINIMAL
            1 -> ProbeIntensity.CONSERVATIVE
            2 -> ProbeIntensity.NORMAL
            else -> throw NativeBridgeSecurityException("invalid native probe policy")
        }
        return MobilePathPolicy(
            probeIntensity = probeIntensity,
            allowBackgroundWarmup = packed and POLICY_WARMUP != 0,
            allowSecondaryPath = packed and POLICY_SECONDARY != 0,
            allowLatencyDuplication = packed and POLICY_DUPLICATION != 0,
        )
    }

    private fun ensureLoadAttempted() {
        if (loadAttempted) {
            return
        }
        synchronized(this) {
            if (loadAttempted) {
                return
            }
            libraryLoaded = try {
                System.loadLibrary(LIBRARY_NAME)
                true
            } catch (_: UnsatisfiedLinkError) {
                false
            } catch (_: SecurityException) {
                false
            }
            loadAttempted = true
        }
    }

    @JvmStatic
    private external fun nativeEnsureRouteProofKey(store: AndroidKeystoreSecretStore): Int

    @JvmStatic
    private external fun nativeEvaluateMobilePolicy(
        kind: Int,
        validated: Boolean,
        metered: Boolean,
        roaming: Boolean,
        dataSaver: Boolean,
        batterySaver: Boolean,
        downstreamKbps: Int,
        upstreamKbps: Int,
        mode: Int,
        allowMeteredSecondary: Boolean,
        allowLatencyDuplication: Boolean,
    ): Int

    private const val UNKNOWN_BANDWIDTH = -1
    private const val POLICY_PROBE_MASK = 0b11
    private const val POLICY_WARMUP = 1 shl 2
    private const val POLICY_SECONDARY = 1 shl 3
    private const val POLICY_DUPLICATION = 1 shl 4
    private const val POLICY_RESERVED_BITS = (1 shl 5).inv()
}
