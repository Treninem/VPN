package ru.amri.vpn

import android.Manifest
import android.app.Activity
import android.content.Intent
import android.content.pm.PackageManager
import android.content.res.ColorStateList
import android.graphics.Color
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.graphics.drawable.PictureDrawable
import android.net.VpnService
import android.os.Build
import android.os.Bundle
import android.view.Gravity
import android.view.View
import android.widget.Button
import android.widget.FrameLayout
import android.widget.HorizontalScrollView
import android.widget.ImageButton
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.Switch
import android.widget.TextView
import com.caverock.androidsvg.SVG

class MainActivity : Activity() {
    private lateinit var status: TextView
    private lateinit var detail: TextView
    private lateinit var actionButton: ImageButton
    private lateinit var actionLabel: TextView

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
        val root = FrameLayout(this).apply {
            setBackgroundColor(Color.rgb(3, 8, 18))
        }

        val background = svgImageView(R.raw.amri_background_mobile).apply {
            scaleType = ImageView.ScaleType.CENTER_CROP
            contentDescription = null
            importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO
        }
        root.addView(background, FrameLayout.LayoutParams(-1, -1))

        val foreground = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(22), dp(22), dp(22), dp(28))
        }

        foreground.addView(text("AMRI", 30f, Color.WHITE, true))
        foreground.addView(
            text(getString(R.string.product_subtitle), 14f, Color.rgb(145, 153, 170), false),
        )
        foreground.addView(space(24))

        val scroll = ScrollView(this).apply {
            isFillViewport = true
            isVerticalScrollBarEnabled = false
        }
        val content = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
        }

        val protectionCard = card().apply {
            status = text("", 25f, Color.WHITE, true)
            detail = text("", 14f, Color.rgb(165, 173, 190), false)
            actionButton = svgPowerButton()
            actionLabel = text("", 14f, Color.rgb(205, 225, 245), true).apply {
                gravity = Gravity.CENTER
            }
            addView(status)
            addView(space(6))
            addView(detail)
            addView(space(16))
            addView(
                actionButton,
                LinearLayout.LayoutParams(dp(124), dp(124)).apply {
                    gravity = Gravity.CENTER_HORIZONTAL
                },
            )
            addView(space(6))
            addView(actionLabel, LinearLayout.LayoutParams(-1, -2))
        }
        content.addView(protectionCard)
        content.addView(space(18))
        content.addView(text(getString(R.string.mode), 19f, Color.WHITE, true))
        content.addView(space(10))
        content.addView(modeSelector())
        content.addView(space(18))
        content.addView(
            toggleCard(
                getString(R.string.smart_routing),
                getString(R.string.smart_routing_description),
                true,
            ),
        )
        content.addView(space(10))
        content.addView(
            toggleCard(
                getString(R.string.dns_protection),
                getString(R.string.dns_protection_description),
                true,
            ),
        )
        content.addView(space(10))
        content.addView(
            toggleCard(
                getString(R.string.kill_switch),
                getString(R.string.kill_switch_description),
                true,
            ),
        )
        content.addView(space(10))
        content.addView(
            toggleCard(
                getString(R.string.local_learning),
                getString(R.string.local_learning_description),
                true,
            ),
        )
        content.addView(space(10))
        content.addView(
            toggleCard(
                getString(R.string.federated_learning),
                getString(R.string.federated_learning_description),
                false,
            ),
        )

        scroll.addView(content)
        foreground.addView(scroll, LinearLayout.LayoutParams(-1, 0, 1f))
        root.addView(foreground, FrameLayout.LayoutParams(-1, -1))
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
                setPowerState(
                    imageRes = R.raw.amri_vpn_power_on,
                    label = getString(R.string.stop),
                    enabled = true,
                )
                actionButton.setOnClickListener { stopController() }
            }
            VpnControllerState.PREPARING -> {
                status.text = getString(R.string.status_preparing)
                detail.text = getString(R.string.detail_preparing)
                setPowerState(
                    imageRes = R.raw.amri_vpn_power_off,
                    label = getString(R.string.wait),
                    enabled = false,
                )
                actionButton.setOnClickListener(null)
            }
            VpnControllerState.FAILED -> {
                status.text = getString(R.string.status_failed)
                detail.text = getString(R.string.detail_failed)
                setPowerState(
                    imageRes = R.raw.amri_vpn_power_off,
                    label = getString(R.string.retry),
                    enabled = true,
                )
                actionButton.setOnClickListener { requestVpnStart() }
            }
            else -> {
                status.text = getString(R.string.status_off)
                detail.text = getString(R.string.detail_off)
                setPowerState(
                    imageRes = R.raw.amri_vpn_power_off,
                    label = getString(R.string.prepare_vpn),
                    enabled = true,
                )
                actionButton.setOnClickListener { requestVpnStart() }
            }
        }
    }

    private fun setPowerState(imageRes: Int, label: String, enabled: Boolean) {
        setSvg(actionButton, imageRes)
        actionLabel.text = label
        actionButton.contentDescription = label
        actionButton.isEnabled = enabled
        actionButton.alpha = if (enabled) 1f else 0.58f
    }

    private fun requestNotificationPermission() {
        if (
            Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
            checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
        ) {
            requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), NOTIFICATION_REQUEST)
        }
    }

    private fun svgPowerButton(): ImageButton = ImageButton(this).apply {
        background = null
        setPadding(0, 0, 0, 0)
        scaleType = ImageView.ScaleType.FIT_CENTER
        setLayerType(View.LAYER_TYPE_SOFTWARE, null)
        setSvg(this, R.raw.amri_vpn_power_off)
    }

    private fun svgImageView(resourceId: Int): ImageView = ImageView(this).apply {
        setLayerType(View.LAYER_TYPE_SOFTWARE, null)
        setSvg(this, resourceId)
    }

    private fun setSvg(target: ImageView, resourceId: Int) {
        val svg = SVG.getFromResource(this, resourceId)
        target.setImageDrawable(PictureDrawable(svg.renderToPicture()))
    }

    private fun card(padding: Int = 22): LinearLayout = LinearLayout(this).apply {
        orientation = LinearLayout.VERTICAL
        setPadding(dp(padding), dp(padding), dp(padding), dp(padding))
        background = GradientDrawable().apply {
            setColor(Color.argb(238, 24, 30, 41))
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
