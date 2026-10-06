package ru.amri.vpn

import android.content.Context
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.os.PowerManager
import ru.amri.vpn.nativebridge.AccessNetworkKind
import ru.amri.vpn.nativebridge.MobileNetworkSnapshot

/**
 * Prevents delayed callbacks for a previous default network from being treated as a fresh handoff.
 *
 * Android may deliver capability callbacks for the old default network while a Wi-Fi/cellular
 * transition is already in progress. Only capabilities belonging to the current default network
 * may be published directly.
 */
internal object DefaultNetworkEventPolicy {
    fun acceptsCapabilities(callbackHandle: Long, activeHandle: Long?): Boolean =
        activeHandle != null && callbackHandle == activeHandle
}

/** Observes only privacy-safe properties of Android's current default network. */
internal class AndroidNetworkObserver(
    context: Context,
    private val onSnapshot: (Network?, MobileNetworkSnapshot) -> Unit,
) : AutoCloseable {
    private val connectivity = context.getSystemService(ConnectivityManager::class.java)
    private val power = context.getSystemService(PowerManager::class.java)
    private var registered = false

    private val callback = object : ConnectivityManager.NetworkCallback() {
        override fun onAvailable(network: Network) = publishCurrent()

        override fun onCapabilitiesChanged(network: Network, caps: NetworkCapabilities) {
            val active = connectivity.activeNetwork
            if (
                DefaultNetworkEventPolicy.acceptsCapabilities(
                    network.networkHandle,
                    active?.networkHandle,
                )
            ) {
                onSnapshot(network, snapshot(caps))
            } else {
                // A delayed callback for the previous Wi-Fi/cellular default must not move the
                // transport lease backwards. Re-read the actual current default instead.
                publishCurrent(active)
            }
        }

        override fun onLost(network: Network) = publishCurrent()
    }

    @Synchronized
    fun start() {
        if (registered) return
        connectivity.registerDefaultNetworkCallback(callback)
        registered = true
        publishCurrent()
    }

    @Synchronized
    override fun close() {
        if (!registered) return
        connectivity.unregisterNetworkCallback(callback)
        registered = false
    }

    private fun publishCurrent(network: Network? = connectivity.activeNetwork) {
        // Capture Network once. Reading activeNetwork twice can pair capabilities from one network
        // with the handle of another during a fast Wi-Fi/cellular handoff.
        val caps = network?.let(connectivity::getNetworkCapabilities)
        onSnapshot(network, caps?.let(::snapshot) ?: unavailableSnapshot())
    }

    private fun snapshot(caps: NetworkCapabilities): MobileNetworkSnapshot {
        val kind = when {
            caps.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) -> AccessNetworkKind.WIFI
            caps.hasTransport(NetworkCapabilities.TRANSPORT_CELLULAR) -> AccessNetworkKind.CELLULAR
            caps.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET) -> AccessNetworkKind.ETHERNET
            else -> AccessNetworkKind.OTHER
        }
        return MobileNetworkSnapshot(
            kind = kind,
            validated = caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_VALIDATED),
            metered = !caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_NOT_METERED),
            roaming = kind == AccessNetworkKind.CELLULAR &&
                !caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_NOT_ROAMING),
            dataSaver = connectivity.restrictBackgroundStatus !=
                ConnectivityManager.RESTRICT_BACKGROUND_STATUS_DISABLED,
            batterySaver = power.isPowerSaveMode,
            estimatedDownstreamKbps = caps.linkDownstreamBandwidthKbps.takeIf { it > 0 },
            estimatedUpstreamKbps = caps.linkUpstreamBandwidthKbps.takeIf { it > 0 },
        )
    }

    private fun unavailableSnapshot() = MobileNetworkSnapshot(
        kind = AccessNetworkKind.OTHER,
        validated = false,
        metered = true,
        roaming = false,
        dataSaver = connectivity.restrictBackgroundStatus !=
            ConnectivityManager.RESTRICT_BACKGROUND_STATUS_DISABLED,
        batterySaver = power.isPowerSaveMode,
        estimatedDownstreamKbps = null,
        estimatedUpstreamKbps = null,
    )
}
