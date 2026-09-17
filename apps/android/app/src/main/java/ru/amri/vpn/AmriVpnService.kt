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
import ru.amri.vpn.nativebridge.AmriNativeBridge
import ru.amri.vpn.nativebridge.ExternalTransportState
import java.io.File
import java.net.DatagramSocket
import java.net.ServerSocket
import java.net.Socket
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicInteger

class AmriVpnService : VpnService() {
    private var controlInterface: ParcelFileDescriptor? = null
    private var networkObserver: AndroidNetworkObserver? = null
    private val networkLease = AndroidNetworkLease()
    private val networkRecoveryGate = AndroidNetworkRecoveryGate()
    private val mainHandler = Handler(Looper.getMainLooper())
    private val lifecycleGeneration = AtomicInteger(0)
    @Volatile
    private var restartWhenNetworkReady = false
    private val transportExecutor = Executors.newSingleThreadExecutor { runnable ->
        Thread(runnable, "amri-android-transport").apply { isDaemon = true }
    }
    private val runtimeOwner by lazy(LazyThreadSafetyMode.NONE) { AndroidRuntimeOwner.production(this) }
    private val mobilePolicyOwner by lazy(LazyThreadSafetyMode.NONE) { AndroidMobilePolicyOwner.production() }
    private val publicTunnelOwner by lazy(LazyThreadSafetyMode.NONE) { AndroidPublicTunnelOwner(this) }

    private val protectionWatchdog = object : Runnable {
        override fun run() {
            if (STATE.state != VpnControllerState.PROTECTED) return
            val transportReady = runCatching {
                AmriNativeBridge.externalTransportState() == ExternalTransportState.RUNNING
            }.getOrDefault(false)
            val forwardingReady = publicTunnelOwner.isRunning() &&
                publicTunnelOwner.readiness()?.protected == true
            if (!transportReady || !forwardingReady) {
                failProtectedGeneration()
                return
            }
            mainHandler.postDelayed(this, PROTECTION_WATCHDOG_MS)
        }
    }

    override fun onBind(intent: Intent?): IBinder? = super.onBind(intent)

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_STOP -> {
                setDesiredActive(false)
                restartWhenNetworkReady = false
                stopController()
            }
            ACTION_START -> {
                setDesiredActive(true)
                startController()
            }
            null -> {
                if (desiredActive()) startController()
            }
        }
        return START_STICKY
    }

    override fun onRevoke() {
        setDesiredActive(false)
        restartWhenNetworkReady = false
        stopController()
        super.onRevoke()
    }

    override fun onDestroy() {
        lifecycleGeneration.incrementAndGet()
        restartWhenNetworkReady = false
        stopProtectionWatchdog()
        stopPublicForwarding()
        closeInterface()
        stopNetworkObservation()
        runCatching { AmriNativeBridge.stopExternalTransport() }
        transportExecutor.shutdownNow()
        STATE.stop()
        super.onDestroy()
    }

    private fun startController() {
        if (!STATE.startPreparing()) return
        val generation = lifecycleGeneration.incrementAndGet()
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
            if (controlInterface == null) {
                controlInterface = establishControlInterface()
                    ?: error("Android refused to establish the VPN interface")
            }
            STATE.serviceReady()
            transportExecutor.execute { startProtectedGeneration(generation) }
        } catch (_: Exception) {
            failStartup(generation)
        }
    }

    private fun startProtectedGeneration(generation: Int) {
        try {
            val candidates = selectedNodeCandidates()
            check(candidates.isNotEmpty()) { "no selected Android VPN node" }
            val executable = File(applicationInfo.nativeLibraryDir, TRANSPORT_LIBRARY_NAME)
            check(executable.isFile) { "bundled Android VPN transport is unavailable" }

            var lastFailure: Exception? = null
            for ((index, rawUri) in candidates) {
                check(generation == lifecycleGeneration.get()) { "VPN start was cancelled" }
                var transportStarted = false
                try {
                    val localPort = reserveLoopbackPort()
                    AmriNativeBridge.startExternalTransport(rawUri, executable.absolutePath, localPort)
                    transportStarted = true
                    check(generation == lifecycleGeneration.get()) { "VPN start was cancelled" }
                    check(AmriNativeBridge.externalTransportState() == ExternalTransportState.RUNNING) {
                        "VPN transport did not remain ready"
                    }
                    check(activatePublicForwarding(localPort, SAFE_INITIAL_MTU)) {
                        "public forwarding readiness failed"
                    }
                    check(generation == lifecycleGeneration.get()) { "VPN start was cancelled" }
                    rememberSelectedNode(index)
                    return
                } catch (error: Exception) {
                    lastFailure = error
                    stopProtectionWatchdog()
                    if (STATE.state == VpnControllerState.PROTECTED) STATE.protectionLost()
                    stopPublicForwarding()
                    val controlReady = restoreControlInterfaceOrFail()
                    if (transportStarted) runCatching { AmriNativeBridge.stopExternalTransport() }
                    if (generation != lifecycleGeneration.get()) throw error
                    if (!controlReady || STATE.state != VpnControllerState.SERVICE_READY) {
                        throw error("Android protected path could not be restored for failover")
                    }
                }
            }
            throw lastFailure ?: error("no Android VPN candidate could be activated")
        } catch (_: Exception) {
            if (generation == lifecycleGeneration.get()) {
                stopProtectionWatchdog()
                if (STATE.state == VpnControllerState.PROTECTED) STATE.protectionLost()
                stopPublicForwarding()
                restoreControlInterfaceOrFail()
                STATE.fail()
                stopForeground(STOP_FOREGROUND_REMOVE)
            }
            runCatching { AmriNativeBridge.stopExternalTransport() }
        }
    }

    private fun stopController() {
        val generation = lifecycleGeneration.incrementAndGet()
        restartWhenNetworkReady = false
        stopProtectionWatchdog()
        stopPublicForwarding()
        closeInterface()
        stopNetworkObservation()
        transportExecutor.execute {
            runCatching { AmriNativeBridge.stopExternalTransport() }
            mainHandler.post {
                if (generation == lifecycleGeneration.get()) {
                    STATE.stop()
                    stopForeground(STOP_FOREGROUND_REMOVE)
                    stopSelf()
                }
            }
        }
    }

    private fun failStartup(generation: Int) {
        if (generation != lifecycleGeneration.get()) return
        restartWhenNetworkReady = false
        stopProtectionWatchdog()
        stopPublicForwarding()
        closeInterface()
        stopNetworkObservation()
        STATE.fail()
        stopForeground(STOP_FOREGROUND_REMOVE)
    }

    /**
     * Promotes the control-only service to a verified public packet-forwarding generation.
     *
     * The current external transport runs as an AMRI subprocess. The public TUN excludes AMRI's
     * own package, so that subprocess cannot route back into the TUN it feeds. Future in-process
     * adapters must continue to use [prepareTransportSocket] for per-socket protect/bind.
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

    private fun failProtectedGeneration() {
        val generation = lifecycleGeneration.incrementAndGet()
        stopProtectionWatchdog()
        if (STATE.state == VpnControllerState.PROTECTED) STATE.protectionLost()
        stopPublicForwarding()
        restoreControlInterfaceOrFail()
        transportExecutor.execute {
            runCatching { AmriNativeBridge.stopExternalTransport() }
            if (generation == lifecycleGeneration.get()) {
                STATE.fail()
                maybeRestartAfterNetworkChange()
            }
        }
    }

    private fun maybeRestartAfterNetworkChange() {
        if (!restartWhenNetworkReady || !desiredActive()) return
        mainHandler.post {
            if (
                restartWhenNetworkReady &&
                desiredActive() &&
                STATE.state == VpnControllerState.FAILED
            ) {
                restartWhenNetworkReady = false
                startController()
            }
        }
    }

    private fun handleNetworkRecovery(action: NetworkRecoveryAction) {
        when (action) {
            NetworkRecoveryAction.NONE -> Unit
            NetworkRecoveryAction.CUT_PROTECTED_PATH -> {
                restartWhenNetworkReady = false
                if (STATE.state == VpnControllerState.PROTECTED) failProtectedGeneration()
            }
            NetworkRecoveryAction.RESTART_PROTECTED_PATH -> {
                restartWhenNetworkReady = true
                when (STATE.state) {
                    VpnControllerState.PROTECTED -> failProtectedGeneration()
                    VpnControllerState.FAILED -> maybeRestartAfterNetworkChange()
                    else -> Unit
                }
            }
        }
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

    private fun selectedNodeCandidates(): List<Pair<Int, String>> {
        val nodes = AndroidNodeStore(this).load()
        if (nodes.isEmpty()) return emptyList()
        val preferences = getSharedPreferences(UI_PREFS, MODE_PRIVATE)
        val selected = preferences.getInt(KEY_SELECTED_NODE, 0).coerceIn(nodes.indices)
        val smartRouting = preferences.getBoolean(KEY_SMART_ROUTING, true)
        return AndroidSmartBootstrapSelector
            .candidateOrder(nodes, selected, smartRouting)
            .map { index -> index to nodes[index] }
    }

    private fun rememberSelectedNode(index: Int) {
        getSharedPreferences(UI_PREFS, MODE_PRIVATE)
            .edit()
            .putInt(KEY_SELECTED_NODE, index)
            .apply()
    }

    private fun reserveLoopbackPort(): Int = ServerSocket(0, 1).use { socket ->
        socket.localPort.also { require(it in 1..65535) }
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
            handleNetworkRecovery(
                networkRecoveryGate.observe(network?.networkHandle, STATE.state),
            )
        }.also { it.start() }
    }

    private fun stopNetworkObservation() {
        networkObserver?.close()
        networkObserver = null
        networkLease.clear()
        networkRecoveryGate.reset()
    }

    private fun desiredActive(): Boolean =
        getSharedPreferences(SERVICE_PREFS, MODE_PRIVATE).getBoolean(KEY_DESIRED_ACTIVE, false)

    private fun setDesiredActive(value: Boolean) {
        getSharedPreferences(SERVICE_PREFS, MODE_PRIVATE)
            .edit()
            .putBoolean(KEY_DESIRED_ACTIVE, value)
            .apply()
    }

    /** In-process transport adapters must reject and close a socket when this returns false. */
    internal fun prepareTransportSocket(socket: Socket): Boolean = networkLease.prepare(this, socket)

    /** UDP equivalent used by future in-process WireGuard/Hysteria2/TUIC adapters. */
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
        private const val SAFE_INITIAL_MTU = 1420
        private const val TRANSPORT_LIBRARY_NAME = "libsing_box.so"
        private const val UI_PREFS = "amri_ui"
        private const val KEY_SELECTED_NODE = "selected_node"
        private const val KEY_SMART_ROUTING = "smart_routing"
        private const val SERVICE_PREFS = "amri_service"
        private const val KEY_DESIRED_ACTIVE = "desired_active"
    }
}
