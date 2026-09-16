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
            return NetworkRecoveryAction.NONE
        }
        if (lastNetworkHandle == networkHandle) return NetworkRecoveryAction.NONE

        lastNetworkHandle = networkHandle
        if (state == VpnControllerState.PROTECTED) {
            return if (networkHandle == null) {
                waitingForNetwork = true
                NetworkRecoveryAction.CUT_PROTECTED_PATH
            } else {
                waitingForNetwork = false
                NetworkRecoveryAction.RESTART_PROTECTED_PATH
            }
        }

        if (
            waitingForNetwork &&
            networkHandle != null &&
            (state == VpnControllerState.SERVICE_READY || state == VpnControllerState.FAILED)
        ) {
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
