package ru.amri.vpn.nativebridge

import ru.amri.vpn.security.AndroidKeystoreSecretStore

enum class RouteProofKeyState {
    EXISTING,
    CREATED,
}

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
}
