package ru.amri.vpn

import ru.amri.vpn.nativebridge.AmriNativeBridge
import ru.amri.vpn.nativebridge.RouteProofKeyState
import ru.amri.vpn.security.AndroidKeystoreSecretStore

internal enum class AndroidRuntimeState {
    NOT_INITIALIZED,
    READY,
    FAILED,
}

/** Service-owned, fail-closed bootstrap for the native AMRI runtime. */
internal class AndroidRuntimeOwner(
    private val routeProofInitializer: () -> RouteProofKeyState,
) {
    @Volatile
    var state: AndroidRuntimeState = AndroidRuntimeState.NOT_INITIALIZED
        private set

    @Synchronized
    fun initialize(): Boolean {
        if (state == AndroidRuntimeState.READY) return true

        return try {
            routeProofInitializer()
            state = AndroidRuntimeState.READY
            true
        } catch (_: RuntimeException) {
            // Do not log platform/security exception text. An explicit retry may recover from a
            // transient Keystore failure without exposing details to UI or analytics.
            state = AndroidRuntimeState.FAILED
            false
        }
    }

    companion object {
        fun production(service: AmriVpnService): AndroidRuntimeOwner = AndroidRuntimeOwner {
            AmriNativeBridge.ensureRouteProofKey(
                AndroidKeystoreSecretStore(service.applicationContext),
            )
        }
    }
}
