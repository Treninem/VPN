package ru.amri.vpn

import android.Manifest
import android.app.Activity
import android.content.Intent
import android.content.pm.PackageManager
import android.content.res.ColorStateList
import android.graphics.Color
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.net.VpnService
import android.os.Build
import android.os.Bundle
import android.view.Gravity
import android.view.View
import android.widget.Button
import android.widget.HorizontalScrollView
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.Switch
import android.widget.TextView

class MainActivity : Activity() {
    private lateinit var status: TextView
    private lateinit var detail: TextView
    private lateinit var actionButton: Button

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(buildContent())
        refreshState()
    }

    override fun onResume() {
        super.onResume()
        refreshState()
    }

    @Deprecated("Uses the platform VPN permission result for API 26+ compatibility")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode == VPN_PERMISSION_REQUEST && resultCode == RESULT_OK) {
            startController()
        } else if (requestCode == VPN_PERMISSION_REQUEST) {
            AmriVpnService.STATE.permissionRequired()
            refreshState()
        }
    }

    private fun buildContent(): View {
        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(22), dp(22), dp(22), dp(28))
            setBackgroundColor(Color.rgb(13, 16, 22))
        }

        root.addView(text("AMRI", 30f, Color.WHITE, true))
        root.addView(text(getString(R.string.product_subtitle), 14f, Color.rgb(145, 153, 170), false))
        root.addView(space(24))

        val scroll = ScrollView(this)
        val content = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
        }

        val protectionCard = card().apply {
            status = text("", 25f, Color.WHITE, true)
            detail = text("", 14f, Color.rgb(165, 173, 190), false)
            actionButton = primaryButton()
            addView(status)
            addView(space(6))
            addView(detail)
            addView(space(20))
            addView(actionButton)
        }
        content.addView(protectionCard)
        content.addView(space(18))
        content.addView(text(getString(R.string.mode), 19f, Color.WHITE, true))
        content.addView(space(10))
        content.addView(modeSelector())
        content.addView(space(18))
        content.addView(toggleCard(getString(R.string.smart_routing), getString(R.string.smart_routing_description), true))
        content.addView(space(10))
        content.addView(toggleCard(getString(R.string.dns_protection), getString(R.string.dns_protection_description), true))
        content.addView(space(10))
        content.addView(toggleCard(getString(R.string.kill_switch), getString(R.string.kill_switch_description), true))
        content.addView(space(10))
        content.addView(toggleCard(getString(R.string.local_learning), getString(R.string.local_learning_description), true))
        content.addView(space(10))
        content.addView(toggleCard(getString(R.string.federated_learning), getString(R.string.federated_learning_description), false))

        scroll.addView(content)
        root.addView(scroll, LinearLayout.LayoutParams(-1, 0, 1f))
        return root
    }

    private fun modeSelector(): View {
        val row = LinearLayout(this).apply { orientation = LinearLayout.HORIZONTAL }
        resources.getStringArray(R.array.vpn_modes).toList().forEachIndexed { index, label ->
            row.addView(Button(this).apply {
                text = label
                isAllCaps = false
                setTextColor(Color.WHITE)
                backgroundTintList = ColorStateList.valueOf(
                    if (index == 0) Color.rgb(67, 104, 255) else Color.rgb(34, 40, 52),
                )
            })
        }
        return HorizontalScrollView(this).apply {
            isHorizontalScrollBarEnabled = false
            addView(row)
        }
    }

    private fun toggleCard(title: String, subtitle: String, enabled: Boolean): View =
        card(16).apply {
            val row = LinearLayout(this@MainActivity).apply {
                orientation = LinearLayout.HORIZONTAL
                gravity = Gravity.CENTER_VERTICAL
            }
            val labels = LinearLayout(this@MainActivity).apply {
                orientation = LinearLayout.VERTICAL
                addView(text(title, 16f, Color.WHITE, true))
                addView(text(subtitle, 12f, Color.rgb(145, 153, 170), false))
            }
            row.addView(labels, LinearLayout.LayoutParams(0, -2, 1f))
            row.addView(Switch(this@MainActivity).apply { isChecked = enabled })
            addView(row)
        }

    private fun requestVpnStart() {
        requestNotificationPermission()
        val permissionIntent = VpnService.prepare(this)
        if (permissionIntent == null) {
            startController()
        } else {
            AmriVpnService.STATE.permissionRequired()
            @Suppress("DEPRECATION")
            startActivityForResult(permissionIntent, VPN_PERMISSION_REQUEST)
        }
    }

    private fun startController() {
        startForegroundService(
            Intent(this, AmriVpnService::class.java).setAction(AmriVpnService.ACTION_START),
        )
        refreshState()
    }

    private fun stopController() {
        startService(Intent(this, AmriVpnService::class.java).setAction(AmriVpnService.ACTION_STOP))
        refreshState()
    }

    private fun refreshState() {
        if (!::status.isInitialized) return
        when (AmriVpnService.STATE.state) {
            VpnControllerState.SERVICE_READY -> {
                status.text = getString(R.string.status_service_ready)
                detail.text = getString(R.string.detail_service_ready)
                actionButton.text = getString(R.string.stop)
                actionButton.setOnClickListener { stopController() }
            }
            VpnControllerState.PREPARING -> {
                status.text = getString(R.string.status_preparing)
                detail.text = getString(R.string.detail_preparing)
                actionButton.text = getString(R.string.wait)
                actionButton.isEnabled = false
            }
            VpnControllerState.FAILED -> {
                status.text = getString(R.string.status_failed)
                detail.text = getString(R.string.detail_failed)
                actionButton.text = getString(R.string.retry)
                actionButton.isEnabled = true
                actionButton.setOnClickListener { requestVpnStart() }
            }
            else -> {
                status.text = getString(R.string.status_off)
                detail.text = getString(R.string.detail_off)
                actionButton.text = getString(R.string.prepare_vpn)
                actionButton.isEnabled = true
                actionButton.setOnClickListener { requestVpnStart() }
            }
        }
    }

    private fun requestNotificationPermission() {
        if (
            Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
            checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
        ) {
            requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), NOTIFICATION_REQUEST)
        }
    }

    private fun primaryButton(): Button = Button(this).apply {
        isAllCaps = false
        textSize = 17f
        setTextColor(Color.WHITE)
        backgroundTintList = ColorStateList.valueOf(Color.rgb(67, 104, 255))
        minHeight = dp(56)
    }

    private fun card(padding: Int = 22): LinearLayout = LinearLayout(this).apply {
        orientation = LinearLayout.VERTICAL
        setPadding(dp(padding), dp(padding), dp(padding), dp(padding))
        background = GradientDrawable().apply {
            setColor(Color.rgb(24, 30, 41))
            cornerRadius = dp(22).toFloat()
        }
    }

    private fun text(value: String, size: Float, color: Int, bold: Boolean): TextView =
        TextView(this).apply {
            text = value
            textSize = size
            setTextColor(color)
            if (bold) setTypeface(typeface, Typeface.BOLD)
        }

    private fun space(height: Int): View = View(this).apply {
        layoutParams = LinearLayout.LayoutParams(1, dp(height))
    }

    private fun dp(value: Int): Int = (value * resources.displayMetrics.density).toInt()

    companion object {
        private const val VPN_PERMISSION_REQUEST = 7001
        private const val NOTIFICATION_REQUEST = 7002
    }
}
