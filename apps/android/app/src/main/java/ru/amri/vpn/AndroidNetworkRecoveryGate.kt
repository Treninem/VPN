package ru.amri.vpn

internal enum class NetworkRecoveryAction {
    NONE,
    CUT_PROTECTED_PATH,
    RESTART_PROTECTED_PATH,
}

/**
 * Tracks only the opaque Android Network handle. No SSID, carrier, address or other personal
 * network metadata is stored. A changed default network invalidates the current protected
 * generation so transport and public TUN ownership can be re-established on the new path.
 */
internal class AndroidNetworkRecoveryGate {
    private var initialized = false
    private var lastNetworkHandle: Long? = null
    private var waitingForNetwork = false

    @Synchronized
    fun observe(networkHandle: Long?, state: VpnControllerState): NetworkRecoveryAction {
        if (!initialized) {
            initialized = true
            lastNetworkHandle = networkHandle
            waitingForNetwork = networkHandle == null
            return NetworkRecoveryAction.NONE
        }
        if (lastNetworkHandle == networkHandle) return NetworkRecoveryAction.NONE

        lastNetworkHandle = networkHandle

        if (networkHandle == null) {
            // Remember loss even when the transport is still SERVICE_READY/PREPARING. Otherwise a
            // start attempted while offline can fail and never restart when connectivity returns.
            waitingForNetwork = true
            return if (state == VpnControllerState.PROTECTED) {
                NetworkRecoveryAction.CUT_PROTECTED_PATH
            } else {
                NetworkRecoveryAction.NONE
            }
        }

        if (state == VpnControllerState.PROTECTED) {
            waitingForNetwork = false
            return NetworkRecoveryAction.RESTART_PROTECTED_PATH
        }

        if (waitingForNetwork) {
            // Arm recovery even if connectivity returns while the controller is still PREPARING.
            // The service will defer the restart flag: a successful current generation clears it,
            // while a subsequent FAILED transition consumes it immediately.
            waitingForNetwork = false
            return NetworkRecoveryAction.RESTART_PROTECTED_PATH
        }

        return NetworkRecoveryAction.NONE
    }

    @Synchronized
    fun reset() {
        initialized = false
        lastNetworkHandle = null
        waitingForNetwork = false
    }
}
