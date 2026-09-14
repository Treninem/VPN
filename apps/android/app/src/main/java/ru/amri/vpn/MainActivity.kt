package ru.amri.vpn

import android.Manifest
import android.app.Activity
import android.app.AlertDialog
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
import android.widget.ImageButton
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.Switch
import android.widget.TextView

class MainActivity : Activity() {
    private lateinit var status: TextView
    private lateinit var detail: TextView
    private lateinit var actionButton: ImageButton
    private lateinit var routeGalaxy: RouteGalaxyView

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
            setBackgroundResource(R.drawable.amri_background_mobile)
        }

        root.addView(text("AMRI", 30f, Color.WHITE, true))
        root.addView(text("Adaptive multi-route VPN", 14f, COLOR_MUTED, false))
        root.addView(space(24))

        val scroll = ScrollView(this)
        val content = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }

        val protectionCard = card().apply {
            val row = LinearLayout(this@MainActivity).apply {
                orientation = LinearLayout.HORIZONTAL
                gravity = Gravity.CENTER_VERTICAL
            }
            val labels = LinearLayout(this@MainActivity).apply {
                orientation = LinearLayout.VERTICAL
                status = text("", 25f, Color.WHITE, true)
                detail = text("", 14f, COLOR_DETAIL, false)
                addView(status)
                addView(space(6))
                addView(detail)
            }
            actionButton = ImageButton(this@MainActivity).apply {
                setBackgroundColor(Color.TRANSPARENT)
                setPadding(dp(4), dp(4), dp(4), dp(4))
                scaleType = android.widget.ImageView.ScaleType.FIT_CENTER
                minimumWidth = dp(116)
                minimumHeight = dp(116)
                contentDescription = getString(R.string.action_prepare_vpn)
            }
            row.addView(labels, LinearLayout.LayoutParams(0, -2, 1f))
            row.addView(actionButton, LinearLayout.LayoutParams(dp(124), dp(124)))
            addView(row)
        }
        content.addView(protectionCard)
        content.addView(space(18))
        routeGalaxy = RouteGalaxyView(this).apply {
            contentDescription = getString(R.string.action_route_galaxy)
            setOnClickListener {
                AlertDialog.Builder(this@MainActivity)
                    .setTitle("AMRI Route Galaxy")
                    .setMessage(
                        "Живая карта покажет каждое локальное назначение и его отдельный VPN-выход или DIRECT. " +
                            "Цвет и толщина луча будут отражать RouteScore и confidence.",
                    )
                    .setPositiveButton(android.R.string.ok, null)
                    .show()
            }
        }
        content.addView(routeGalaxy)
        content.addView(space(18))
        content.addView(shadowRaceCard())
        content.addView(space(18))
        content.addView(text("Режим", 19f, Color.WHITE, true))
        content.addView(space(10))
        content.addView(modeSelector())
        content.addView(space(18))
        content.addView(toggleCard("Smart Routing", "Отдельный лучший маршрут для каждого назначения", true))
        content.addView(space(10))
        content.addView(toggleCard("DNS protection", "DNS будет направляться через выбранный защищённый маршрут", true))
        content.addView(space(10))
        content.addView(toggleCard("Kill Switch", "Блокировка защищаемого трафика при потере маршрута", true))
        content.addView(space(10))
        content.addView(toggleCard("Локальное обучение", "История решений остаётся только на устройстве", true))
        content.addView(space(10))
        content.addView(toggleCard("Обмен обезличенным опытом", "Выключен по умолчанию", false))

        scroll.addView(content)
        root.addView(scroll, LinearLayout.LayoutParams(-1, 0, 1f))
        return root
    }

    private fun shadowRaceCard(): View = card(18).apply {
        val row = LinearLayout(this@MainActivity).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
        }
        val labels = LinearLayout(this@MainActivity).apply {
            orientation = LinearLayout.VERTICAL
            addView(text("Теневая гонка AMRI", 18f, Color.WHITE, true))
            addView(
                text(
                    "Три параллельных подтверждения дают мгновенное переключение без разрыва текущего маршрута.",
                    12f,
                    COLOR_DETAIL,
                    false,
                ),
            )
            addView(space(8))
            addView(text("ОЖИДАНИЕ РЕАЛЬНЫХ МАРШРУТОВ", 11f, COLOR_ACCENT, true))
        }
        val info = ImageButton(this@MainActivity).apply {
            setImageResource(R.drawable.amri_info_button)
            setBackgroundColor(Color.TRANSPARENT)
            setPadding(dp(4), dp(4), dp(4), dp(4))
            contentDescription = getString(R.string.action_shadow_race_info)
            setOnClickListener {
                AlertDialog.Builder(this@MainActivity)
                    .setTitle("Теневая гонка AMRI")
                    .setMessage(
                        "Текущий маршрут продолжает работать, пока AMRI сравнивает альтернативу. " +
                            "Новый маршрут должен стабильно улучшать RouteScore и иметь достаточный confidence.",
                    )
                    .setPositiveButton(android.R.string.ok, null)
                    .show()
            }
        }
        row.addView(labels, LinearLayout.LayoutParams(0, -2, 1f))
        row.addView(info, LinearLayout.LayoutParams(dp(52), dp(52)))
        addView(row)
    }

    private fun modeSelector(): View {
        val row = LinearLayout(this).apply { orientation = LinearLayout.HORIZONTAL }
        listOf("Smart", "Speed", "Ping", "Privacy", "Streaming", "Gaming", "Manual").forEachIndexed { index, label ->
            row.addView(Button(this).apply {
                text = label
                isAllCaps = false
                setTextColor(Color.WHITE)
                backgroundTintList = ColorStateList.valueOf(
                    if (index == 0) Color.rgb(18, 132, 223) else Color.rgb(16, 42, 76),
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
                addView(text(subtitle, 12f, COLOR_MUTED, false))
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
        actionButton.setImageResource(R.drawable.amri_vpn_power_off)
        if (::routeGalaxy.isInitialized) routeGalaxy.updateControllerState(AmriVpnService.STATE.state)
        actionButton.isEnabled = true
        when (AmriVpnService.STATE.state) {
            VpnControllerState.SERVICE_READY -> {
                status.text = "VPN-служба готова"
                detail.text = "Публичный трафик ещё не перехватывается: защищённый transport не подключён"
                actionButton.contentDescription = getString(R.string.action_stop_vpn_service)
                actionButton.setOnClickListener { stopController() }
            }
            VpnControllerState.PREPARING -> {
                status.text = "Подготовка…"
                detail.text = "Android создаёт безопасный контрольный интерфейс"
                actionButton.contentDescription = getString(R.string.action_vpn_preparing)
                actionButton.isEnabled = false
            }
            VpnControllerState.FAILED -> {
                status.text = "Не удалось подготовить VPN"
                detail.text = "Системный интерфейс не создан; обычная сеть не затронута"
                actionButton.contentDescription = getString(R.string.action_retry_vpn)
                actionButton.setOnClickListener { requestVpnStart() }
            }
            else -> {
                status.text = "Защита выключена"
                detail.text = "Подготовить Android VpnService для будущего защищённого transport"
                actionButton.contentDescription = getString(R.string.action_prepare_vpn)
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

    private fun card(padding: Int = 22): LinearLayout = LinearLayout(this).apply {
        orientation = LinearLayout.VERTICAL
        setPadding(dp(padding), dp(padding), dp(padding), dp(padding))
        background = GradientDrawable().apply {
            setColor(Color.argb(224, 8, 27, 59))
            setStroke(dp(1), Color.argb(105, 39, 163, 239))
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
        private val COLOR_MUTED = Color.rgb(165, 181, 204)
        private val COLOR_DETAIL = Color.rgb(184, 199, 220)
        private val COLOR_ACCENT = Color.rgb(65, 221, 239)
    }
}
