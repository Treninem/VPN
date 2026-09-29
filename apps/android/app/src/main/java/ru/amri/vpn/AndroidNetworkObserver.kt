package ru.amri.vpn

import android.content.Context
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.net.NetworkRequest
import android.os.PowerManager
import ru.amri.vpn.nativebridge.AccessNetworkKind
import ru.amri.vpn.nativebridge.MobileNetworkSnapshot

/** Observes only privacy-safe properties of Android's current default network. */
internal class AndroidNetworkObserver(
    context: Context,
    private val onSnapshot: (Network?, MobileNetworkSnapshot) -> Unit,
) : AutoCloseable {
    private val connectivity = context.getSystemService(ConnectivityManager::class.java)
    private val power = context.getSystemService(PowerManager::class.java)
    private var registered = false
    private val candidates = linkedMapOf<Network, NetworkCapabilities>()
    private var selectedNetwork: Network? = null

    private val callback = object : ConnectivityManager.NetworkCallback() {
        override fun onAvailable(network: Network) {
            refreshCandidate(network)
            publishCurrent()
        }

        override fun onCapabilitiesChanged(network: Network, caps: NetworkCapabilities) {
            synchronized(this@AndroidNetworkObserver) {
                if (isUnderlyingInternet(caps)) candidates[network] = caps else candidates.remove(network)
                publishCurrent()
            }
        }

        override fun onLost(network: Network) {
            synchronized(this@AndroidNetworkObserver) {
                candidates.remove(network)
                publishCurrent()
            }
        }
    }

    @Synchronized
    fun start() {
        if (registered) return
        connectivity.registerNetworkCallback(
            NetworkRequest.Builder()
                .addCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET)
                .addCapability(NetworkCapabilities.NET_CAPABILITY_NOT_VPN)
                .build(),
            callback,
        )
        registered = true
        connectivity.allNetworks.forEach(::refreshCandidate)
        publishCurrent()
    }

    @Synchronized
    override fun close() {
        if (!registered) return
        connectivity.unregisterNetworkCallback(callback)
        registered = false
        candidates.clear()
        selectedNetwork = null
    }

    @Synchronized
    private fun publishCurrent() {
        val choices = candidates.map { (network, caps) ->
            UnderlyingNetworkChoice(
                value = network,
                validated = caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_VALIDATED),
                wifi = caps.hasTransport(NetworkCapabilities.TRANSPORT_WIFI),
                ethernet = caps.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET),
                cellular = caps.hasTransport(NetworkCapabilities.TRANSPORT_CELLULAR),
            )
        }
        val systemActive = connectivity.activeNetwork
        val preferred = systemActive?.takeIf(candidates::containsKey) ?: selectedNetwork
        val network = AndroidUnderlyingNetworkPolicy.select(preferred, choices)
        selectedNetwork = network
        val caps = network?.let(candidates::get)
        onSnapshot(network, caps?.let(::snapshot) ?: unavailableSnapshot())
    }

    @Synchronized
    private fun refreshCandidate(network: Network) {
        val caps = connectivity.getNetworkCapabilities(network)
        if (caps != null && isUnderlyingInternet(caps)) candidates[network] = caps
        else candidates.remove(network)
    }

    private fun isUnderlyingInternet(caps: NetworkCapabilities): Boolean =
        caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET) &&
            caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_NOT_VPN)

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
