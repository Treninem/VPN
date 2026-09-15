package ru.amri.vpn

import android.net.Network
import android.net.VpnService
import java.net.DatagramSocket
import java.net.Socket

internal object TransportSocketGate {
    fun <N : Any, S> prepare(
        network: N?,
        socket: S,
        protect: (S) -> Boolean,
        bind: (N, S) -> Unit,
        isCurrent: (N) -> Boolean,
    ): Boolean {
        network ?: return false
        return try {
            protect(socket) && run {
                bind(network, socket)
                isCurrent(network)
            }
        } catch (_: Exception) {
            false
        }
    }
}

/** Ephemeral default-network lease. No Network handle or socket metadata is persisted or logged. */
internal class AndroidNetworkLease {
    private var current: Network? = null
    private var generation: Long = 0

    @Synchronized
    fun update(network: Network?) {
        if (current != network) {
            current = network
            generation += 1
        }
    }

    @Synchronized
    fun clear() = update(null)

    fun prepare(service: VpnService, socket: Socket): Boolean = prepareSocket(
        socket = socket,
        protect = service::protect,
        bind = Network::bindSocket,
    )

    fun prepare(service: VpnService, socket: DatagramSocket): Boolean = prepareSocket(
        socket = socket,
        protect = service::protect,
        bind = Network::bindSocket,
    )

    private fun <T> prepareSocket(
        socket: T,
        protect: (T) -> Boolean,
        bind: (Network, T) -> Unit,
    ): Boolean {
        val lease = synchronized(this) { current?.let { it to generation } }
        return TransportSocketGate.prepare(
            network = lease?.first,
            socket = socket,
            protect = protect,
            bind = bind,
            isCurrent = { network ->
                synchronized(this) { current == network && generation == lease?.second }
            },
        )
    }
}
