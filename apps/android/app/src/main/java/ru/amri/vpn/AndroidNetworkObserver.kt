package ru.amri.vpn

import android.content.Context
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.os.PowerManager
import ru.amri.vpn.nativebridge.AccessNetworkKind
import ru.amri.vpn.nativebridge.MobileNetworkSnapshot

/** Observes only privacy-safe properties of Android's current default network. */
internal class AndroidNetworkObserver(
    context: Context,
    private val onSnapshot: (MobileNetworkSnapshot) -> Unit,
) : AutoCloseable {
    private val connectivity = context.getSystemService(ConnectivityManager::class.java)
    private val power = context.getSystemService(PowerManager::class.java)
    private var registered = false

    private val callback = object : ConnectivityManager.NetworkCallback() {
        override fun onAvailable(network: Network) = publishCurrent()

        override fun onCapabilitiesChanged(network: Network, caps: NetworkCapabilities) {
            onSnapshot(snapshot(caps))
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

    private fun publishCurrent() {
        val caps = connectivity.activeNetwork?.let(connectivity::getNetworkCapabilities)
        onSnapshot(caps?.let(::snapshot) ?: unavailableSnapshot())
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
