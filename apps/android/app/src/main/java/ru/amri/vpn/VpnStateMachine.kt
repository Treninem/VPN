package ru.amri.vpn

enum class VpnControllerState {
    IDLE,
    PERMISSION_REQUIRED,
    PREPARING,
    SERVICE_READY,
    PROTECTED,
    STOPPING,
    FAILED,
}

class VpnStateMachine {
    @Volatile
    var state: VpnControllerState = VpnControllerState.IDLE
        private set

    @Synchronized
    fun permissionRequired() {
        if (state == VpnControllerState.IDLE) state = VpnControllerState.PERMISSION_REQUIRED
    }

    @Synchronized
    fun startPreparing(): Boolean {
        if (
            state != VpnControllerState.IDLE &&
            state != VpnControllerState.PERMISSION_REQUIRED &&
            state != VpnControllerState.FAILED
        ) {
            return false
        }
        state = VpnControllerState.PREPARING
        return true
    }

    @Synchronized
    fun serviceReady() {
        check(state == VpnControllerState.PREPARING)
        state = VpnControllerState.SERVICE_READY
    }

    @Synchronized
    fun protectionReady() {
        check(state == VpnControllerState.SERVICE_READY)
        state = VpnControllerState.PROTECTED
    }

    @Synchronized
    fun protectionLost() {
        if (state == VpnControllerState.PROTECTED) state = VpnControllerState.SERVICE_READY
    }

    @Synchronized
    fun fail() {
        state = VpnControllerState.FAILED
    }

    @Synchronized
    fun stop() {
        state = VpnControllerState.STOPPING
        state = VpnControllerState.IDLE
    }
}
