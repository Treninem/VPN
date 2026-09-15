package ru.amri.vpn

import android.net.VpnService
import android.os.ParcelFileDescriptor
import ru.amri.vpn.nativebridge.AmriNativeBridge
import ru.amri.vpn.nativebridge.NativeProtectionState
import ru.amri.vpn.nativebridge.PacketForwarderState
import java.net.InetSocketAddress
import java.net.Socket
import java.util.concurrent.FutureTask
import java.util.concurrent.TimeUnit

internal data class AndroidPublicTunnelConfig(
    val localSocksPort: Int,
    val safeInitialMtu: Int,
) {
    init {
        require(localSocksPort in 1..65535) { "local SOCKS port is out of range" }
        require(safeInitialMtu in MIN_MTU..MAX_MTU) { "tunnel MTU is out of range" }
    }

    companion object {
        const val MIN_MTU = 1280
        const val MAX_MTU = 1500
    }
}

internal data class AndroidPublicTunnelReadiness(
    val effectiveMtu: Int,
    val publicEgressVerified: Boolean,
    val protectionState: NativeProtectionState,
) {
    val protected: Boolean get() = protectionState == NativeProtectionState.PROTECTED
}

internal class AndroidPublicTunnelOwner(
    private val service: VpnService,
    private val nativeBridge: NativeTunnelBridge = ProductionNativeTunnelBridge,
    private val egressVerifier: PublicEgressVerifier = NumericTcpEgressVerifier,
) : AutoCloseable {
    private val lock = Any()
    private var tunnel: ParcelFileDescriptor? = null
    private var activeConfig: AndroidPublicTunnelConfig? = null
    private var activeMtu: Int? = null
    private var lastReadiness: AndroidPublicTunnelReadiness? = null

    fun start(config: AndroidPublicTunnelConfig): AndroidPublicTunnelReadiness? = synchronized(lock) {
        if (isRunningLocked() && activeConfig == config && lastReadiness?.protected == true) {
            return lastReadiness
        }
        stopLocked()

        if (!loopbackSocksReady(config.localSocksPort)) return null
        val mtu = try {
            nativeBridge.resetAdaptiveMtu(config.safeInitialMtu)
        } catch (_: Exception) {
            return null
        }

        val established = establishPublicTun(mtu) ?: return null
        val detachedFd = try {
            ParcelFileDescriptor.dup(established.fileDescriptor).detachFd()
        } catch (_: Exception) {
            established.closeQuietly()
            return null
        }

        var nativeOwnsFd = false
        try {
            nativeBridge.start(detachedFd, config.localSocksPort, mtu)
            nativeOwnsFd = true
            if (!waitUntilRunning()) {
                nativeBridge.stop()
                established.closeQuietly()
                return null
            }
        } catch (_: Exception) {
            if (!nativeOwnsFd) closeDetachedFd(detachedFd)
            established.closeQuietly()
            return null
        }

        // These sockets are deliberately NOT VpnService.protect()'ed: they must traverse the new
        // public TUN, then tun2proxy and the confirmed SOCKS transport. Numeric endpoints avoid DNS.
        val egressVerified = egressVerifier.verify()
        val protectionState = try {
            nativeBridge.evaluateProtection(
                requested = true,
                transportReady = true,
                packetForwardingActive = nativeBridge.state() == PacketForwarderState.RUNNING,
                dnsProtectionReady = true,
                leakProtectionReady = true,
                publicEgressVerified = egressVerified,
            )
        } catch (_: Exception) {
            NativeProtectionState.PREPARING
        }

        val readiness = AndroidPublicTunnelReadiness(
            effectiveMtu = mtu,
            publicEgressVerified = egressVerified,
            protectionState = protectionState,
        )
        if (!readiness.protected) {
            nativeBridge.stop()
            established.closeQuietly()
            return null
        }

        tunnel = established
        activeConfig = config
        activeMtu = mtu
        lastReadiness = readiness
        readiness
    }

    fun isRunning(): Boolean = synchronized(lock) { isRunningLocked() }

    fun readiness(): AndroidPublicTunnelReadiness? = synchronized(lock) {
        if (isRunningLocked()) lastReadiness else null
    }

    /**
     * Records classified PMTU evidence. The recommendation is kept in shared Rust state and is
     * applied on the next safe tunnel establishment. We intentionally do not tear down/rebuild the
     * live default-route TUN here because that could create a direct-route leak window.
     */
    fun reportSuspectedPmtuFailure(): Int? = synchronized(lock) {
        if (!isRunningLocked()) return null
        try {
            nativeBridge.recordSuspectedPmtuFailure()
        } catch (_: Exception) {
            null
        }
    }

    fun reportPathSuccess(): Int? = synchronized(lock) {
        if (!isRunningLocked()) return null
        try {
            nativeBridge.recordMtuSuccess()
        } catch (_: Exception) {
            null
        }
    }

    fun stop() = synchronized(lock) { stopLocked() }

    override fun close() = stop()

    private fun establishPublicTun(mtu: Int): ParcelFileDescriptor? = try {
        service.Builder()
            .setSession("AMRI")
            .setMtu(mtu)
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
    }

    private fun isRunningLocked(): Boolean =
        tunnel != null && nativeBridge.state() == PacketForwarderState.RUNNING

    private fun stopLocked() {
        try {
            nativeBridge.stop()
        } catch (_: Exception) {
            // Closing the service-owned TUN below still cuts this forwarding generation.
        }
        tunnel?.closeQuietly()
        tunnel = null
        activeConfig = null
        activeMtu = null
        lastReadiness = null
    }

    private fun waitUntilRunning(): Boolean {
        repeat(STARTUP_POLLS) {
            when (nativeBridge.state()) {
                PacketForwarderState.RUNNING -> return true
                PacketForwarderState.FAILED, PacketForwarderState.STOPPED -> return false
                PacketForwarderState.STARTING, PacketForwarderState.STOPPING ->
                    Thread.sleep(STARTUP_POLL_MS)
            }
        }
        return false
    }

    private fun loopbackSocksReady(port: Int): Boolean = boundedTcpProbe(
        targets = arrayOf(InetSocketAddress("127.0.0.1", port)),
        connectTimeoutMs = LOOPBACK_CONNECT_TIMEOUT_MS,
        totalTimeoutMs = LOOPBACK_VERIFY_TIMEOUT_MS,
        threadName = "amri-loopback-ready",
    )

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

    internal interface NativeTunnelBridge {
        fun start(tunFd: Int, localSocksPort: Int, mtu: Int)
        fun stop()
        fun state(): PacketForwarderState
        fun resetAdaptiveMtu(safeInitialMtu: Int): Int
        fun recordSuspectedPmtuFailure(): Int
        fun recordMtuSuccess(): Int
        fun evaluateProtection(
            requested: Boolean,
            transportReady: Boolean,
            packetForwardingActive: Boolean,
            dnsProtectionReady: Boolean,
            leakProtectionReady: Boolean,
            publicEgressVerified: Boolean,
        ): NativeProtectionState
    }

    internal fun interface PublicEgressVerifier {
        fun verify(): Boolean
    }

    private object ProductionNativeTunnelBridge : NativeTunnelBridge {
        override fun start(tunFd: Int, localSocksPort: Int, mtu: Int) =
            AmriNativeBridge.startPacketForwarder(tunFd, localSocksPort, mtu)
        override fun stop() = AmriNativeBridge.stopPacketForwarder()
        override fun state(): PacketForwarderState = AmriNativeBridge.packetForwarderState()
        override fun resetAdaptiveMtu(safeInitialMtu: Int): Int =
            AmriNativeBridge.resetAdaptiveMtu(safeInitialMtu)
        override fun recordSuspectedPmtuFailure(): Int = AmriNativeBridge.recordSuspectedPmtuFailure()
        override fun recordMtuSuccess(): Int = AmriNativeBridge.recordAdaptiveMtuSuccess()
        override fun evaluateProtection(
            requested: Boolean,
            transportReady: Boolean,
            packetForwardingActive: Boolean,
            dnsProtectionReady: Boolean,
            leakProtectionReady: Boolean,
            publicEgressVerified: Boolean,
        ): NativeProtectionState = AmriNativeBridge.evaluateProtection(
            requested,
            transportReady,
            packetForwardingActive,
            dnsProtectionReady,
            leakProtectionReady,
            publicEgressVerified,
        )
    }

    private object NumericTcpEgressVerifier : PublicEgressVerifier {
        override fun verify(): Boolean = boundedTcpProbe(
            targets = EGRESS_TARGETS.map { InetSocketAddress(it, 443) }.toTypedArray(),
            connectTimeoutMs = EGRESS_CONNECT_TIMEOUT_MS,
            totalTimeoutMs = EGRESS_VERIFY_TIMEOUT_MS,
            threadName = "amri-egress-verify",
        )
    }

    companion object {
        private const val IPV4_TUN_ADDRESS = "10.253.0.2"
        private const val IPV6_TUN_ADDRESS = "fd00:616d:7269::2"
        private const val IPV4_DNS = "1.1.1.1"
        private const val IPV6_DNS = "2606:4700:4700::1111"
        private val EGRESS_TARGETS = arrayOf("1.1.1.1", "8.8.8.8")
        private const val LOOPBACK_CONNECT_TIMEOUT_MS = 250
        private const val LOOPBACK_VERIFY_TIMEOUT_MS = 600L
        private const val EGRESS_CONNECT_TIMEOUT_MS = 800
        private const val EGRESS_VERIFY_TIMEOUT_MS = 1800L
        private const val STARTUP_POLLS = 20
        private const val STARTUP_POLL_MS = 25L
    }
}

private fun boundedTcpProbe(
    targets: Array<InetSocketAddress>,
    connectTimeoutMs: Int,
    totalTimeoutMs: Long,
    threadName: String,
): Boolean {
    val task = FutureTask {
        targets.any { target ->
            try {
                Socket().use { socket -> socket.connect(target, connectTimeoutMs) }
                true
            } catch (_: Exception) {
                false
            }
        }
    }
    Thread(task, threadName).apply { isDaemon = true }.start()
    return try {
        task.get(totalTimeoutMs, TimeUnit.MILLISECONDS)
    } catch (_: Exception) {
        task.cancel(true)
        false
    }
}
