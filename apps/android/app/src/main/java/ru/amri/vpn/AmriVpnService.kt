package ru.amri.vpn

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Intent
import android.content.pm.ServiceInfo
import android.net.VpnService
import android.os.Build
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.os.ParcelFileDescriptor
import java.net.DatagramSocket
import java.net.Socket

class AmriVpnService : VpnService() {
    private var controlInterface: ParcelFileDescriptor? = null
    private var networkObserver: AndroidNetworkObserver? = null
    private val networkLease = AndroidNetworkLease()
    private val mainHandler = Handler(Looper.getMainLooper())
    private val runtimeOwner by lazy(LazyThreadSafetyMode.NONE) { AndroidRuntimeOwner.production(this) }
    private val mobilePolicyOwner by lazy(LazyThreadSafetyMode.NONE) { AndroidMobilePolicyOwner.production() }
    private val publicTunnelOwner by lazy(LazyThreadSafetyMode.NONE) { AndroidPublicTunnelOwner(this) }

    private val protectionWatchdog = object : Runnable {
        override fun run() {
            if (STATE.state != VpnControllerState.PROTECTED) return
            val stillProtected = publicTunnelOwner.isRunning() &&
                publicTunnelOwner.readiness()?.protected == true
            if (!stillProtected) {
                downgradeFromPublicTunnel()
                return
            }
            mainHandler.postDelayed(this, PROTECTION_WATCHDOG_MS)
        }
    }

    override fun onBind(intent: Intent?): IBinder? = super.onBind(intent)

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_STOP -> stopController()
            ACTION_START -> startController()
        }
        return START_STICKY
    }

    override fun onRevoke() {
        stopController()
        super.onRevoke()
    }

    override fun onDestroy() {
        stopProtectionWatchdog()
        stopNetworkObservation()
        stopPublicForwarding()
        closeInterface()
        STATE.stop()
        super.onDestroy()
    }

    private fun startController() {
        if (!STATE.startPreparing()) return
        createNotificationChannel()
        val notification = buildNotification()
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            startForeground(
                NOTIFICATION_ID,
                notification,
                ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE,
            )
        } else {
            startForeground(NOTIFICATION_ID, notification)
        }

        try {
            check(runtimeOwner.initialize()) { "AMRI native runtime initialization failed" }
            startNetworkObservation()
            controlInterface = establishControlInterface()
                ?: error("Android refused to establish the VPN interface")
            STATE.serviceReady()
        } catch (_: Exception) {
            STATE.fail()
            stopNetworkObservation()
            stopPublicForwarding()
            closeInterface()
            stopForeground(STOP_FOREGROUND_REMOVE)
            stopSelf()
        }
    }

    private fun stopController() {
        stopProtectionWatchdog()
        stopNetworkObservation()
        stopPublicForwarding()
        closeInterface()
        STATE.stop()
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    /**
     * Promotes the control-only service to a verified public packet-forwarding generation.
     * The caller must already own a transport-ready loopback SOCKS endpoint and protect/bind the
     * transport's real network sockets through [prepareTransportSocket].
     */
    internal fun activatePublicForwarding(localSocksPort: Int, safeInitialMtu: Int): Boolean {
        if (STATE.state != VpnControllerState.SERVICE_READY) return false

        closeInterface()
        val readiness = try {
            publicTunnelOwner.start(AndroidPublicTunnelConfig(localSocksPort, safeInitialMtu))
        } catch (_: Exception) {
            null
        }
        if (readiness?.protected == true) {
            STATE.protectionReady()
            startProtectionWatchdog()
            return true
        }

        restoreControlInterfaceOrFail()
        return false
    }

    internal fun deactivatePublicForwarding(): Boolean {
        stopProtectionWatchdog()
        if (STATE.state == VpnControllerState.PROTECTED) STATE.protectionLost()
        stopPublicForwarding()
        if (STATE.state != VpnControllerState.SERVICE_READY) return false
        return restoreControlInterfaceOrFail()
    }

    internal fun isPublicForwardingActive(): Boolean =
        STATE.state == VpnControllerState.PROTECTED && publicTunnelOwner.isRunning()

    /** Generic packet loss must never call this method. */
    internal fun reportSuspectedPmtuFailure(): Int? = publicTunnelOwner.reportSuspectedPmtuFailure()

    internal fun reportForwardingPathSuccess(): Int? = publicTunnelOwner.reportPathSuccess()

    internal fun isAlwaysOnLockdownActive(): Boolean =
        Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q && isAlwaysOn && isLockdownEnabled

    private fun startProtectionWatchdog() {
        mainHandler.removeCallbacks(protectionWatchdog)
        mainHandler.postDelayed(protectionWatchdog, PROTECTION_WATCHDOG_MS)
    }

    private fun stopProtectionWatchdog() {
        mainHandler.removeCallbacks(protectionWatchdog)
    }

    private fun downgradeFromPublicTunnel() {
        stopProtectionWatchdog()
        if (STATE.state == VpnControllerState.PROTECTED) STATE.protectionLost()
        stopPublicForwarding()
        restoreControlInterfaceOrFail()
    }

    private fun restoreControlInterfaceOrFail(): Boolean {
        if (controlInterface != null) return true
        controlInterface = establishControlInterface()
        if (controlInterface == null) {
            STATE.fail()
            return false
        }
        return true
    }

    private fun establishControlInterface(): ParcelFileDescriptor? = try {
        Builder()
            .setSession(getString(R.string.app_name))
            .setMtu(1500)
            .addAddress(CONTROL_ADDRESS, 32)
            .addRoute(CONTROL_ADDRESS, 32)
            .setBlocking(false)
            .establish()
    } catch (_: Exception) {
        null
    }

    private fun stopPublicForwarding() {
        publicTunnelOwner.stop()
    }

    private fun closeInterface() {
        controlInterface?.close()
        controlInterface = null
    }

    private fun startNetworkObservation() {
        if (networkObserver != null) return
        networkObserver = AndroidNetworkObserver(applicationContext) { network, snapshot ->
            networkLease.update(network)
            mobilePolicyOwner.update(snapshot)
        }.also { it.start() }
    }

    private fun stopNetworkObservation() {
        networkObserver?.close()
        networkObserver = null
        networkLease.clear()
    }

    /** Transport adapters must reject and close a socket when this returns false. */
    internal fun prepareTransportSocket(socket: Socket): Boolean = networkLease.prepare(this, socket)

    /** UDP equivalent used by WireGuard/Hysteria2/TUIC adapters. */
    internal fun prepareTransportSocket(socket: DatagramSocket): Boolean =
        networkLease.prepare(this, socket)

    private fun createNotificationChannel() {
        val manager = getSystemService(NotificationManager::class.java)
        manager.createNotificationChannel(
            NotificationChannel(
                CHANNEL_ID,
                getString(R.string.notification_channel),
                NotificationManager.IMPORTANCE_LOW,
            ),
        )
    }

    private fun buildNotification(): Notification {
        val openApp = PendingIntent.getActivity(
            this,
            0,
            Intent(this, MainActivity::class.java),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        return Notification.Builder(this, CHANNEL_ID)
            .setSmallIcon(R.drawable.amri_app_icon)
            .setContentTitle(getString(R.string.app_name))
            .setContentText(getString(R.string.notification_ready))
            .setContentIntent(openApp)
            .setOngoing(true)
            .build()
    }

    companion object {
        const val ACTION_START = "ru.amri.vpn.action.START"
        const val ACTION_STOP = "ru.amri.vpn.action.STOP"
        val STATE = VpnStateMachine()

        private const val CHANNEL_ID = "amri_vpn_connection"
        private const val NOTIFICATION_ID = 1001
        private const val CONTROL_ADDRESS = "10.253.0.1"
        private const val PROTECTION_WATCHDOG_MS = 1000L
    }
}
