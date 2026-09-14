package ru.amri.vpn

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Intent
import android.content.pm.ServiceInfo
import android.net.VpnService
import android.os.Build
import android.os.IBinder
import android.os.ParcelFileDescriptor

class AmriVpnService : VpnService() {
    private var controlInterface: ParcelFileDescriptor? = null

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
        closeInterface()
        STATE.stop()
        super.onDestroy()
    }

    private fun startController() {
        if (!STATE.startPreparing()) {
            return
        }
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
            // This narrow control-only interface proves VpnService ownership without
            // capturing public traffic before a real transport adapter is ready.
            controlInterface = Builder()
                .setSession(getString(R.string.app_name))
                .setMtu(1500)
                .addAddress(CONTROL_ADDRESS, 32)
                .addRoute(CONTROL_ADDRESS, 32)
                .setBlocking(false)
                .establish() ?: error("Android refused to establish the VPN interface")
            STATE.serviceReady()
        } catch (_: Exception) {
            STATE.fail()
            closeInterface()
            stopForeground(STOP_FOREGROUND_REMOVE)
            stopSelf()
        }
    }

    private fun stopController() {
        closeInterface()
        STATE.stop()
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    private fun closeInterface() {
        controlInterface?.close()
        controlInterface = null
    }

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
            .setSmallIcon(android.R.drawable.stat_sys_warning)
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
    }
}
