package ru.amri.vpn

import android.net.VpnService
import android.os.ParcelFileDescriptor
import ru.amri.vpn.nativebridge.AmriNativeBridge
import ru.amri.vpn.nativebridge.PacketForwarderState
import java.net.InetSocketAddress
import java.net.Socket

internal data class AndroidPublicTunnelConfig(
    val localSocksPort: Int,
    val mtu: Int,
) {
    init {
        require(localSocksPort in 1..65535) { "local SOCKS port is out of range" }
        require(mtu in MIN_MTU..MAX_MTU) { "tunnel MTU is out of range" }
    }

    companion object {
        const val MIN_MTU = 1280
        const val MAX_MTU = 1500
    }
}

/**
 * Owns Android's public TUN and the native TUN -> local SOCKS packet bridge.
 *
 * This class deliberately has no public/UI start API. A transport owner may activate it only after
 * the local SOCKS endpoint is confirmed ready and the transport's real network sockets are protected
 * from VpnService recursion. Until then AmriVpnService keeps its narrow control-only TUN.
 */
internal class AndroidPublicTunnelOwner(
    private val service: VpnService,
    private val nativeBridge: PacketForwarderBridge = ProductionPacketForwarderBridge,
) : AutoCloseable {
    private val lock = Any()
    private var tunnel: ParcelFileDescriptor? = null
    private var activeConfig: AndroidPublicTunnelConfig? = null

    fun start(config: AndroidPublicTunnelConfig): Boolean = synchronized(lock) {
        if (isRunningLocked() && activeConfig == config) return true
        stopLocked()

        if (!loopbackSocksReady(config.localSocksPort)) return false

        val established = try {
            service.Builder()
                .setSession("AMRI")
                .setMtu(config.mtu)
                .addAddress(IPV4_TUN_ADDRESS, 32)
                .addRoute("0.0.0.0", 0)
                .addAddress(IPV6_TUN_ADDRESS, 128)
                .addRoute("::", 0)
                .addDnsServer(IPV4_DNS)
                .addDnsServer(IPV6_DNS)
                .setBlocking(false)
                .establish()
        } catch (_: Exception) {
            null
        } ?: return false

        val detachedFd = try {
            ParcelFileDescriptor.dup(established.fileDescriptor).detachFd()
        } catch (_: Exception) {
            established.closeQuietly()
            return false
        }

        var nativeOwnsFd = false
        try {
            nativeBridge.start(detachedFd, config.localSocksPort, config.mtu)
            nativeOwnsFd = true
            if (!waitUntilRunning()) {
                nativeBridge.stop()
                established.closeQuietly()
                return false
            }
        } catch (_: Exception) {
            if (!nativeOwnsFd) closeDetachedFd(detachedFd)
            established.closeQuietly()
            return false
        }

        tunnel = established
        activeConfig = config
        true
    }

    fun isRunning(): Boolean = synchronized(lock) { isRunningLocked() }

    fun stop() = synchronized(lock) {
        stopLocked()
    }

    override fun close() = stop()

    private fun isRunningLocked(): Boolean =
        tunnel != null && nativeBridge.state() == PacketForwarderState.RUNNING

    private fun stopLocked() {
        try {
            nativeBridge.stop()
        } catch (_: Exception) {
            // Stop remains best-effort during service teardown; closing TUN still cuts public traffic.
        }
        tunnel?.closeQuietly()
        tunnel = null
        activeConfig = null
    }

    private fun waitUntilRunning(): Boolean {
        repeat(STARTUP_POLLS) {
            when (nativeBridge.state()) {
                PacketForwarderState.RUNNING -> return true
                PacketForwarderState.FAILED,
                PacketForwarderState.STOPPED,
                -> return false
                PacketForwarderState.STARTING,
                PacketForwarderState.STOPPING,
                -> Thread.sleep(STARTUP_POLL_MS)
            }
        }
        return false
    }

    private fun loopbackSocksReady(port: Int): Boolean = try {
        Socket().use { socket ->
            socket.connect(InetSocketAddress("127.0.0.1", port), LOOPBACK_TIMEOUT_MS)
        }
        true
    } catch (_: Exception) {
        false
    }

    private fun closeDetachedFd(fd: Int) {
        try {
            ParcelFileDescriptor.adoptFd(fd).close()
        } catch (_: Exception) {
            // Nothing else can safely be done with an already-detached invalid fd.
        }
    }

    private fun ParcelFileDescriptor.closeQuietly() {
        try {
            close()
        } catch (_: Exception) {
            // Service teardown must continue.
        }
    }

    internal interface PacketForwarderBridge {
        fun start(tunFd: Int, localSocksPort: Int, mtu: Int)
        fun stop()
        fun state(): PacketForwarderState
    }

    private object ProductionPacketForwarderBridge : PacketForwarderBridge {
        override fun start(tunFd: Int, localSocksPort: Int, mtu: Int) =
            AmriNativeBridge.startPacketForwarder(tunFd, localSocksPort, mtu)

        override fun stop() = AmriNativeBridge.stopPacketForwarder()

        override fun state(): PacketForwarderState = AmriNativeBridge.packetForwarderState()
    }

    companion object {
        private const val IPV4_TUN_ADDRESS = "10.253.0.2"
        private const val IPV6_TUN_ADDRESS = "fd00:616d:7269::2"
        private const val IPV4_DNS = "1.1.1.1"
        private const val IPV6_DNS = "2606:4700:4700::1111"
        private const val LOOPBACK_TIMEOUT_MS = 250
        private const val STARTUP_POLLS = 20
        private const val STARTUP_POLL_MS = 25L
    }
}
