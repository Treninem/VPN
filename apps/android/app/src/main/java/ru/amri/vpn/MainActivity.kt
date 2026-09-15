package ru.amri.vpn

import android.Manifest
import android.app.Activity
import android.app.AlertDialog
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.content.res.ColorStateList
import android.content.res.Configuration
import android.graphics.Color
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.graphics.drawable.PictureDrawable
import android.net.VpnService
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
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
import java.util.Locale

class MainActivity : Activity() {
    private lateinit var status: TextView
    private lateinit var detail: TextView
    private lateinit var actionButton: ImageButton
    private lateinit var actionLabel: TextView
    private lateinit var settingsContainer: LinearLayout
    private val stateHandler = Handler(Looper.getMainLooper())
    private val stateRefresh = object : Runnable {
        override fun run() {
            refreshState()
            stateHandler.postDelayed(this, AmriTheme.stateRefreshMs)
        }
    }

    override fun attachBaseContext(newBase: Context) {
        val storedTag = newBase
            .getSharedPreferences(UI_PREFS, Context.MODE_PRIVATE)
            .getString(KEY_LANGUAGE, null)
        if (storedTag.isNullOrBlank()) {
            super.attachBaseContext(newBase)
            return
        }

        val locale = Locale.forLanguageTag(storedTag)
        Locale.setDefault(locale)
        val configuration = Configuration(newBase.resources.configuration).apply {
            setLocale(locale)
            setLayoutDirection(locale)
        }
        super.attachBaseContext(newBase.createConfigurationContext(configuration))
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(buildContent())
        refreshState()
    }

    override fun onResume() {
        super.onResume()
        stateHandler.removeCallbacks(stateRefresh)
        stateHandler.post(stateRefresh)
    }

    override fun onPause() {
        stateHandler.removeCallbacks(stateRefresh)
        super.onPause()
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
        val root = FrameLayout(this).apply { setBackgroundColor(AmriTheme.backgroundColor) }
        val background = svgImageView(R.raw.amri_background_mobile).apply {
            scaleType = ImageView.ScaleType.CENTER_CROP
            contentDescription = null
            importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO
        }
        root.addView(background, FrameLayout.LayoutParams(-1, -1))

        val foreground = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(
                dp(AmriTheme.screenPaddingHorizontal),
                dp(AmriTheme.screenPaddingTop),
                dp(AmriTheme.screenPaddingHorizontal),
                dp(AmriTheme.screenPaddingBottom),
            )
        }
        val header = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
        }
        val brand = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            addView(text("AMRI", 30f, Color.WHITE, true))
            addView(text(getString(R.string.product_subtitle), 14f, AmriTheme.mutedTextColor, false))
        }
        header.addView(brand, LinearLayout.LayoutParams(0, -2, 1f))
        val languageButton = svgIconButton(R.raw.amri_language_button, "Language / Язык").apply {
            setOnClickListener { showLanguageDialog() }
        }
        header.addView(
            languageButton,
            LinearLayout.LayoutParams(dp(AmriTheme.iconButtonSize), dp(AmriTheme.iconButtonSize)),
        )
        val settingsButton = svgIconButton(R.raw.amri_settings_button, "Settings / Настройки").apply {
            setOnClickListener { toggleSettingsPanel() }
        }
        header.addView(
            settingsButton,
            LinearLayout.LayoutParams(dp(AmriTheme.iconButtonSize), dp(AmriTheme.iconButtonSize)).apply {
                marginStart = dp(8)
            },
        )
        foreground.addView(header)
        foreground.addView(space(24))

        val scroll = ScrollView(this).apply {
            isFillViewport = true
            isVerticalScrollBarEnabled = false
        }
        val content = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        val protectionCard = card().apply {
            status = text("", 25f, Color.WHITE, true)
            detail = text("", 14f, AmriTheme.detailTextColor, false)
            actionButton = svgPowerButton()
            actionLabel = text("", 14f, AmriTheme.actionTextColor, true).apply {
                gravity = Gravity.CENTER
            }
            addView(status)
            addView(space(6))
            addView(detail)
            addView(space(16))
            addView(
                actionButton,
                LinearLayout.LayoutParams(
                    dp(AmriTheme.powerButtonSize),
                    dp(AmriTheme.powerButtonSize),
                ).apply { gravity = Gravity.CENTER_HORIZONTAL },
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
        content.addView(routeSelectionCard())
        content.addView(space(18))

        settingsContainer = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            visibility = View.GONE
            addView(toggleCard(getString(R.string.smart_routing), getString(R.string.smart_routing_description), KEY_SMART_ROUTING, true))
            addView(space(10))
            addView(toggleCard(getString(R.string.dns_protection), getString(R.string.dns_protection_description), KEY_DNS_PROTECTION, true))
            addView(space(10))
            addView(toggleCard(getString(R.string.kill_switch), getString(R.string.kill_switch_description), KEY_KILL_SWITCH, true))
            addView(space(10))
            addView(toggleCard(getString(R.string.local_learning), getString(R.string.local_learning_description), KEY_LOCAL_LEARNING, true))
            addView(space(10))
            addView(toggleCard(getString(R.string.federated_learning), getString(R.string.federated_learning_description), KEY_FEDERATED_LEARNING, false))
        }
        content.addView(settingsContainer)
        scroll.addView(content)
        foreground.addView(scroll, LinearLayout.LayoutParams(-1, 0, 1f))
        root.addView(foreground, FrameLayout.LayoutParams(-1, -1))
        return root
    }

    private fun modeSelector(): View {
        val row = LinearLayout(this).apply { orientation = LinearLayout.HORIZONTAL }
        val labels = resources.getStringArray(R.array.vpn_modes)
        val selected = uiPreferences().getInt(KEY_ROUTING_MODE, 0).coerceIn(labels.indices)
        val buttons = mutableListOf<Button>()
        fun refreshButtons(active: Int) {
            buttons.forEachIndexed { index, button ->
                button.backgroundTintList = ColorStateList.valueOf(
                    if (index == active) AmriTheme.accentColor else AmriTheme.inactiveControlColor,
                )
            }
        }
        labels.forEachIndexed { index, label ->
            val button = Button(this).apply {
                text = label
                isAllCaps = false
                setTextColor(Color.WHITE)
                setOnClickListener {
                    uiPreferences().edit().putInt(KEY_ROUTING_MODE, index).apply()
                    refreshButtons(index)
                }
            }
            buttons += button
            row.addView(button)
        }
        refreshButtons(selected)
        return HorizontalScrollView(this).apply {
            isHorizontalScrollBarEnabled = false
            addView(row)
        }
    }

    private fun routeSelectionCard(): View = card(16).apply {
        val row = LinearLayout(this@MainActivity).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
        }
        val labels = LinearLayout(this@MainActivity).apply {
            orientation = LinearLayout.VERTICAL
            addView(text(getString(R.string.route_selection), 16f, Color.WHITE, true))
            addView(text(getString(R.string.automatic_route), 14f, AmriTheme.actionTextColor, true))
            addView(text(getString(R.string.no_imported_servers), 12f, AmriTheme.mutedTextColor, false))
        }
        row.addView(labels, LinearLayout.LayoutParams(0, -2, 1f))
        row.addView(
            svgIconButton(R.raw.amri_more_button, getString(R.string.choose_server)).apply {
                setOnClickListener { showServerDialog() }
            },
            LinearLayout.LayoutParams(
                dp(AmriTheme.iconButtonSize),
                dp(AmriTheme.iconButtonSize),
            ),
        )
        addView(row)
    }

    private fun showServerDialog() {
        AlertDialog.Builder(this)
            .setTitle(getString(R.string.choose_server))
            .setSingleChoiceItems(arrayOf(getString(R.string.automatic_route)), 0) { dialog, _ ->
                dialog.dismiss()
            }
            .setNegativeButton(android.R.string.cancel, null)
            .show()
    }

    private fun toggleCard(
        title: String,
        subtitle: String,
        preferenceKey: String,
        defaultValue: Boolean,
    ): View = card(16).apply {
        val row = LinearLayout(this@MainActivity).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
        }
        val labels = LinearLayout(this@MainActivity).apply {
            orientation = LinearLayout.VERTICAL
            addView(text(title, 16f, Color.WHITE, true))
            addView(text(subtitle, 12f, AmriTheme.mutedTextColor, false))
        }
        row.addView(labels, LinearLayout.LayoutParams(0, -2, 1f))
        row.addView(Switch(this@MainActivity).apply {
            isChecked = uiPreferences().getBoolean(preferenceKey, defaultValue)
            setOnCheckedChangeListener { _, checked ->
                uiPreferences().edit().putBoolean(preferenceKey, checked).apply()
            }
        })
        addView(row)
    }

    private fun toggleSettingsPanel() {
        if (!::settingsContainer.isInitialized) return
        settingsContainer.visibility = if (settingsContainer.visibility == View.VISIBLE) View.GONE else View.VISIBLE
    }

    private fun showLanguageDialog() {
        val preferences = getSharedPreferences(UI_PREFS, Context.MODE_PRIVATE)
        val currentTag = preferences.getString(KEY_LANGUAGE, null)
        val checkedIndex = LANGUAGE_TAGS.indexOf(currentTag)
        AlertDialog.Builder(this)
            .setTitle("Language / Язык")
            .setSingleChoiceItems(LANGUAGE_NAMES, checkedIndex) { dialog, which ->
                preferences.edit().putString(KEY_LANGUAGE, LANGUAGE_TAGS[which]).apply()
                dialog.dismiss()
                recreate()
            }
            .setNegativeButton(android.R.string.cancel, null)
            .show()
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
        startForegroundService(Intent(this, AmriVpnService::class.java).setAction(AmriVpnService.ACTION_START))
        refreshState()
    }

    private fun stopController() {
        startService(Intent(this, AmriVpnService::class.java).setAction(AmriVpnService.ACTION_STOP))
        refreshState()
    }

    private fun refreshState() {
        if (!::status.isInitialized) return
        val presentation = presentControllerState(AmriVpnService.STATE.state)
        status.setText(presentation.statusRes)
        detail.setText(presentation.detailRes)
        setPowerState(
            if (presentation.powerOn) R.raw.amri_vpn_power_on else R.raw.amri_vpn_power_off,
            getString(presentation.actionLabelRes),
            presentation.actionEnabled,
        )
        actionButton.setOnClickListener(when (presentation.action) {
            MainScreenAction.START, MainScreenAction.RETRY -> View.OnClickListener { requestVpnStart() }
            MainScreenAction.STOP -> View.OnClickListener { stopController() }
            MainScreenAction.NONE -> null
        })
    }

    private fun uiPreferences() = getSharedPreferences(UI_PREFS, Context.MODE_PRIVATE)

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

    private fun svgPowerButton(): ImageButton = svgIconButton(
        R.raw.amri_vpn_power_off,
        getString(R.string.prepare_vpn),
    )

    private fun svgIconButton(resourceId: Int, description: String): ImageButton = ImageButton(this).apply {
        background = null
        setPadding(0, 0, 0, 0)
        scaleType = ImageView.ScaleType.FIT_CENTER
        contentDescription = description
        setLayerType(View.LAYER_TYPE_SOFTWARE, null)
        setSvg(this, resourceId)
    }

    private fun svgImageView(resourceId: Int): ImageView = ImageView(this).apply {
        setLayerType(View.LAYER_TYPE_SOFTWARE, null)
        setSvg(this, resourceId)
    }

    private fun setSvg(target: ImageView, resourceId: Int) {
        val svg = SVG.getFromResource(this, resourceId)
        target.setImageDrawable(PictureDrawable(svg.renderToPicture()))
    }

    private fun card(padding: Int = AmriTheme.cardPadding): LinearLayout = LinearLayout(this).apply {
        orientation = LinearLayout.VERTICAL
        setPadding(dp(padding), dp(padding), dp(padding), dp(padding))
        background = GradientDrawable().apply {
            setColor(AmriTheme.cardColor)
            cornerRadius = dp(AmriTheme.cardRadius).toFloat()
        }
    }

    private fun text(value: String, size: Float, color: Int, bold: Boolean): TextView = TextView(this).apply {
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
        private const val UI_PREFS = "amri_ui"
        private const val KEY_LANGUAGE = "language_tag"
        private const val KEY_ROUTING_MODE = "routing_mode"
        private const val KEY_SMART_ROUTING = "smart_routing"
        private const val KEY_DNS_PROTECTION = "dns_protection"
        private const val KEY_KILL_SWITCH = "kill_switch"
        private const val KEY_LOCAL_LEARNING = "local_learning"
        private const val KEY_FEDERATED_LEARNING = "federated_learning"
        private val LANGUAGE_TAGS = arrayOf("en", "ru", "es", "pt", "fr", "de", "zh-CN", "hi", "ar")
        private val LANGUAGE_NAMES = arrayOf(
            "English", "Русский", "Español", "Português", "Français", "Deutsch", "简体中文", "हिन्दी", "العربية",
        )
    }
}
