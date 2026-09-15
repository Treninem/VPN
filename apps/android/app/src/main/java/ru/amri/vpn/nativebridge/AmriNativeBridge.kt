package ru.amri.vpn.nativebridge

import ru.amri.vpn.security.AndroidKeystoreSecretStore

enum class RouteProofKeyState { EXISTING, CREATED }

enum class AccessNetworkKind(val nativeCode: Int) {
    WIFI(0), CELLULAR(1), ETHERNET(2), OTHER(3),
}

enum class MobileAccelerationMode(val nativeCode: Int) {
    OFF(0), BALANCED(1), SPEED(2),
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

enum class ProbeIntensity { MINIMAL, CONSERVATIVE, NORMAL }

data class MobilePathPolicy(
    val probeIntensity: ProbeIntensity,
    val allowBackgroundWarmup: Boolean,
    val allowSecondaryPath: Boolean,
    val allowLatencyDuplication: Boolean,
)

enum class PacketForwarderState { STOPPED, STARTING, RUNNING, FAILED, STOPPING }

enum class NativeProtectionState { OFF, PREPARING, PROTECTED }

class NativeBridgeUnavailableException : IllegalStateException(
    "AMRI native runtime is not packaged for this Android build",
)
class NativeBridgeSecurityException(message: String) : IllegalStateException(message)
class PacketForwarderStartException(message: String) : IllegalStateException(message)

object AmriNativeBridge {
    private const val LIBRARY_NAME = "amri_android_ffi"
    private const val STATUS_EXISTING = 0
    private const val STATUS_CREATED = 1
    private const val STATUS_INVALID_STORED_KEY = 2
    private const val STATUS_ENTROPY_FAILURE = 3

    @Volatile private var loadAttempted = false
    @Volatile private var libraryLoaded = false

    fun isAvailable(): Boolean {
        ensureLoadAttempted()
        return libraryLoaded
    }

    fun ensureRouteProofKey(store: AndroidKeystoreSecretStore): RouteProofKeyState {
        requireLibrary()
        return decodeRouteProofKeyStatus(nativeEnsureRouteProofKey(store))
    }

    fun evaluateMobilePolicy(
        snapshot: MobileNetworkSnapshot,
        preferences: MobileAccelerationPreferences = MobileAccelerationPreferences(),
    ): MobilePathPolicy {
        requireLibrary()
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

    /** Native takes ownership of [tunFd] only when this method returns normally. */
    fun startPacketForwarder(tunFd: Int, localSocksPort: Int, mtu: Int) {
        requireLibrary()
        decodePacketForwarderStart(nativeStartPacketForwarder(tunFd, localSocksPort, mtu))
    }

    fun stopPacketForwarder() {
        ensureLoadAttempted()
        if (libraryLoaded) nativeStopPacketForwarder()
    }

    fun packetForwarderState(): PacketForwarderState {
        ensureLoadAttempted()
        if (!libraryLoaded) return PacketForwarderState.STOPPED
        return decodePacketForwarderState(nativePacketForwarderState())
    }

    fun resetAdaptiveMtu(safeInitialMtu: Int): Int {
        requireLibrary()
        return decodeMtu(nativeResetAdaptiveMtu(safeInitialMtu))
    }

    fun currentAdaptiveMtu(): Int {
        requireLibrary()
        return decodeMtu(nativeCurrentAdaptiveMtu())
    }

    /** Call only for evidence classified as likely PMTU/fragmentation, never generic packet loss. */
    fun recordSuspectedPmtuFailure(): Int {
        requireLibrary()
        return decodeMtu(nativeRecordSuspectedPmtuFailure())
    }

    fun recordAdaptiveMtuSuccess(): Int {
        requireLibrary()
        return decodeMtu(nativeRecordAdaptiveMtuSuccess())
    }

    fun evaluateProtection(
        requested: Boolean,
        transportReady: Boolean,
        packetForwardingActive: Boolean,
        dnsProtectionReady: Boolean,
        leakProtectionReady: Boolean,
        publicEgressVerified: Boolean,
    ): NativeProtectionState {
        requireLibrary()
        return decodeProtectionState(
            nativeEvaluateProtection(
                requested,
                transportReady,
                packetForwardingActive,
                dnsProtectionReady,
                leakProtectionReady,
                publicEgressVerified,
            ),
        )
    }

    internal fun decodeRouteProofKeyStatus(status: Int): RouteProofKeyState = when (status) {
        STATUS_EXISTING -> RouteProofKeyState.EXISTING
        STATUS_CREATED -> RouteProofKeyState.CREATED
        STATUS_INVALID_STORED_KEY -> throw NativeBridgeSecurityException(
            "stored Route Proof key has an unexpected length",
        )
        STATUS_ENTROPY_FAILURE -> throw NativeBridgeSecurityException("secure random generator failed")
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

    internal fun decodePacketForwarderStart(status: Int) {
        when (status) {
            0 -> return
            -1 -> throw PacketForwarderStartException("invalid TUN descriptor")
            -2 -> throw PacketForwarderStartException("invalid local transport port")
            -3 -> throw PacketForwarderStartException("invalid tunnel MTU")
            -4 -> throw PacketForwarderStartException("packet forwarder is already active")
            -5 -> throw PacketForwarderStartException("packet forwarder could not start")
            else -> throw NativeBridgeSecurityException("unknown native packet-forwarder status")
        }
    }

    internal fun decodePacketForwarderState(status: Int): PacketForwarderState = when (status) {
        0 -> PacketForwarderState.STOPPED
        1 -> PacketForwarderState.STARTING
        2 -> PacketForwarderState.RUNNING
        3 -> PacketForwarderState.FAILED
        4 -> PacketForwarderState.STOPPING
        else -> throw NativeBridgeSecurityException("unknown native packet-forwarder state")
    }

    internal fun decodeMtu(value: Int): Int {
        if (value !in MIN_MTU..MAX_MTU) {
            throw NativeBridgeSecurityException("invalid native adaptive MTU state")
        }
        return value
    }

    internal fun decodeProtectionState(status: Int): NativeProtectionState = when (status) {
        0 -> NativeProtectionState.OFF
        1 -> NativeProtectionState.PREPARING
        2 -> NativeProtectionState.PROTECTED
        else -> throw NativeBridgeSecurityException("invalid native protection state")
    }

    private fun requireLibrary() {
        ensureLoadAttempted()
        if (!libraryLoaded) throw NativeBridgeUnavailableException()
    }

    private fun ensureLoadAttempted() {
        if (loadAttempted) return
        synchronized(this) {
            if (loadAttempted) return
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

    @JvmStatic private external fun nativeEnsureRouteProofKey(store: AndroidKeystoreSecretStore): Int
    @JvmStatic private external fun nativeEvaluateMobilePolicy(
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
    @JvmStatic private external fun nativeStartPacketForwarder(
        tunFd: Int,
        localSocksPort: Int,
        mtu: Int,
    ): Int
    @JvmStatic private external fun nativeStopPacketForwarder()
    @JvmStatic private external fun nativePacketForwarderState(): Int
    @JvmStatic private external fun nativeResetAdaptiveMtu(safeInitialMtu: Int): Int
    @JvmStatic private external fun nativeCurrentAdaptiveMtu(): Int
    @JvmStatic private external fun nativeRecordSuspectedPmtuFailure(): Int
    @JvmStatic private external fun nativeRecordAdaptiveMtuSuccess(): Int
    @JvmStatic private external fun nativeEvaluateProtection(
        requested: Boolean,
        transportReady: Boolean,
        packetForwardingActive: Boolean,
        dnsProtectionReady: Boolean,
        leakProtectionReady: Boolean,
        publicEgressVerified: Boolean,
    ): Int

    private const val UNKNOWN_BANDWIDTH = -1
    private const val POLICY_PROBE_MASK = 0b11
    private const val POLICY_WARMUP = 1 shl 2
    private const val POLICY_SECONDARY = 1 shl 3
    private const val POLICY_DUPLICATION = 1 shl 4
    private const val POLICY_RESERVED_BITS = ((1 shl 5) - 1).inv()
    private const val MIN_MTU = 1280
    private const val MAX_MTU = 1500
}
