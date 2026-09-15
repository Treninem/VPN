package ru.amri.vpn

internal enum class MainScreenAction {
    START,
    STOP,
    RETRY,
    NONE,
}

internal data class MainScreenPresentation(
    val statusRes: Int,
    val detailRes: Int,
    val actionLabelRes: Int,
    val powerOn: Boolean,
    val actionEnabled: Boolean,
    val action: MainScreenAction,
)

internal fun presentControllerState(state: VpnControllerState): MainScreenPresentation =
    when (state) {
        VpnControllerState.PROTECTED -> MainScreenPresentation(
            R.string.status_protected,
            R.string.detail_protected,
            R.string.stop,
            powerOn = true,
            actionEnabled = true,
            action = MainScreenAction.STOP,
        )
        VpnControllerState.SERVICE_READY -> MainScreenPresentation(
            R.string.status_service_ready,
            R.string.detail_service_ready,
            R.string.stop,
            powerOn = false,
            actionEnabled = true,
            action = MainScreenAction.STOP,
        )
        VpnControllerState.PREPARING -> MainScreenPresentation(
            R.string.status_preparing,
            R.string.detail_preparing,
            R.string.wait,
            powerOn = false,
            actionEnabled = false,
            action = MainScreenAction.NONE,
        )
        VpnControllerState.FAILED -> MainScreenPresentation(
            R.string.status_failed,
            R.string.detail_failed,
            R.string.retry,
            powerOn = false,
            actionEnabled = true,
            action = MainScreenAction.RETRY,
        )
        else -> MainScreenPresentation(
            R.string.status_off,
            R.string.detail_off,
            R.string.prepare_vpn,
            powerOn = false,
            actionEnabled = true,
            action = MainScreenAction.START,
        )
    }
