package ru.amri.vpn

import android.Manifest
import android.app.Activity
import android.app.AlertDialog
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
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
import android.text.InputType
import android.view.Gravity
import android.view.View
import android.widget.EditText
import android.widget.FrameLayout
import android.widget.ImageButton
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import com.caverock.androidsvg.SVG
import java.util.Locale

class MainActivity : Activity() {
    private lateinit var status: TextView
    private lateinit var detail: TextView
    private lateinit var actionButton: ImageButton
    private lateinit var actionLabel: TextView
    private lateinit var modePrimary: TextView
    private lateinit var modeSecondary: TextView
    private lateinit var routePrimary: TextView
    private lateinit var routeSecondary: TextView
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
        refreshModeSummary()
        refreshRouteSummary()
        refreshState()
    }

    override fun onResume() {
        super.onResume()
        refreshModeSummary()
        refreshRouteSummary()
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
        val configuration = resources.configuration
        val responsive = ResponsiveLayoutPolicy.resolve(
            widthDp = configuration.screenWidthDp,
            heightDp = configuration.screenHeightDp,
            fontScale = configuration.fontScale,
        )
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
                dp(responsive.horizontalPaddingDp),
                dp(AmriTheme.screenPaddingTop),
                dp(responsive.horizontalPaddingDp),
                dp(AmriTheme.screenPaddingBottom),
            )
        }
        val header = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
        }
        val brandIcon = ImageView(this).apply {
            setImageResource(R.drawable.amri_app_icon)
            scaleType = ImageView.ScaleType.FIT_CENTER
            adjustViewBounds = true
            contentDescription = "AMRI VPN"
        }
        if (responsive.showBrandIcon) {
            header.addView(
                brandIcon,
                LinearLayout.LayoutParams(dp(responsive.headerIconDp), dp(responsive.headerIconDp)).apply {
                    marginEnd = dp(if (configuration.screenWidthDp < 400) 8 else 12)
                },
            )
        }
        val brand = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            addView(text("AMRI VPN", if (configuration.screenWidthDp < 400) 23f else 27f, Color.WHITE, true).apply {
                maxLines = 1
            })
            if (responsive.showSubtitle) {
                addView(text(getString(R.string.product_subtitle), 13f, AmriTheme.mutedTextColor, false).apply {
                    maxLines = 2
                })
            }
        }
        header.addView(brand, LinearLayout.LayoutParams(0, -2, 1f))
        val languageButton = svgIconButton(R.raw.amri_language_button, "Language / Язык").apply {
            setOnClickListener { showLanguageDialog() }
        }
        header.addView(
            languageButton,
            LinearLayout.LayoutParams(dp(responsive.iconButtonDp), dp(responsive.iconButtonDp)),
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
                    dp(responsive.powerButtonDp),
                    dp(responsive.powerButtonDp),
                ).apply { gravity = Gravity.CENTER_HORIZONTAL },
            )
            addView(space(6))
            addView(actionLabel, LinearLayout.LayoutParams(-1, -2))
        }
        content.addView(protectionCard)
        content.addView(space(AmriTheme.sectionGap))
        content.addView(modeSelectionCard())
        content.addView(space(AmriTheme.sectionGap))
        content.addView(routeSelectionCard())

        scroll.addView(content)
        foreground.addView(scroll, LinearLayout.LayoutParams(-1, 0, 1f))
        val foregroundWidth = if (configuration.screenWidthDp > responsive.maxContentWidthDp) {
            dp(responsive.maxContentWidthDp)
        } else {
            -1
        }
        root.addView(
            foreground,
            FrameLayout.LayoutParams(foregroundWidth, -1).apply {
                gravity = Gravity.CENTER_HORIZONTAL
            },
        )
        return root
    }

    private fun modeSelectionCard(): View = card(16).apply {
        isClickable = true
        isFocusable = true
        contentDescription = getString(R.string.choose_mode)
        setOnClickListener { showModeDialog() }
        addView(text(getString(R.string.mode), 13f, AmriTheme.mutedTextColor, true))
        val row = LinearLayout(this@MainActivity).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
        }
        val copy = LinearLayout(this@MainActivity).apply {
            orientation = LinearLayout.VERTICAL
            modePrimary = text("", 17f, Color.WHITE, true).apply { maxLines = 1 }
            modeSecondary = text("", 12f, AmriTheme.detailTextColor, false).apply { maxLines = 3 }
            addView(modePrimary)
            addView(modeSecondary)
        }
        row.addView(copy, LinearLayout.LayoutParams(0, -2, 1f))
        row.addView(text("›", 28f, AmriTheme.actionTextColor, false).apply {
            gravity = Gravity.CENTER
        })
        addView(row)
    }

    private fun currentMode(): Int = AndroidRoutingModePolicy.normalize(
        uiPreferences().getInt(KEY_ROUTING_MODE, AndroidRoutingModePolicy.SMART),
    )

    private fun refreshModeSummary() {
        if (!::modePrimary.isInitialized || !::modeSecondary.isInitialized) return
        val labels = resources.getStringArray(R.array.vpn_modes)
        val selected = currentMode().coerceIn(labels.indices)
        modePrimary.text = labels[selected]
        modeSecondary.text = if (selected == AndroidRoutingModePolicy.SMART) {
            getString(R.string.mode_smart_summary)
        } else {
            getString(R.string.mode_manual_summary)
        }
    }

    private fun showModeDialog() {
        val selected = currentMode()
        val labels = resources.getStringArray(R.array.vpn_modes)
        val descriptions = intArrayOf(R.string.mode_smart_description, R.string.mode_manual_description)
        val tradeoffs = intArrayOf(R.string.mode_smart_tradeoff, R.string.mode_manual_tradeoff)
        val container = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(12), dp(4), dp(12), dp(4))
        }
        lateinit var dialog: AlertDialog
        labels.forEachIndexed { index, label ->
            val option = card(14).apply {
                isClickable = true
                isFocusable = true
                addView(text(if (index == selected) "✓ $label" else label, 17f, Color.WHITE, true))
                addView(space(4))
                addView(text(getString(descriptions[index]), 13f, AmriTheme.detailTextColor, false))
                addView(space(5))
                addView(text(getString(tradeoffs[index]), 12f, AmriTheme.mutedTextColor, false))
                setOnClickListener {
                    val smart = index == AndroidRoutingModePolicy.SMART
                    uiPreferences().edit()
                        .putInt(KEY_ROUTING_MODE, index)
                        .putBoolean(KEY_SMART_ROUTING, smart)
                        .apply()
                    refreshModeSummary()
                    refreshRouteSummary()
                    dialog.dismiss()
                }
            }
            container.addView(option, LinearLayout.LayoutParams(-1, -2).apply {
                bottomMargin = dp(8)
            })
        }
        dialog = AlertDialog.Builder(this)
            .setTitle(getString(R.string.choose_mode))
            .setView(container)
            .setNegativeButton(android.R.string.cancel, null)
            .create()
        dialog.show()
    }

    private fun routeSelectionCard(): View = card(16).apply {
        isClickable = true
        isFocusable = true
        contentDescription = getString(R.string.choose_server)
        setOnClickListener { showServerDialog() }
        val row = LinearLayout(this@MainActivity).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
        }
        val labels = LinearLayout(this@MainActivity).apply {
            orientation = LinearLayout.VERTICAL
            addView(text(getString(R.string.route_selection), 16f, Color.WHITE, true))
            routePrimary = text(getString(R.string.automatic_route), 14f, AmriTheme.actionTextColor, true)
            routeSecondary = text(getString(R.string.no_imported_servers), 12f, AmriTheme.mutedTextColor, false)
            addView(routePrimary)
            addView(routeSecondary)
        }
        row.addView(labels, LinearLayout.LayoutParams(0, -2, 1f))
        row.addView(text("›", 28f, AmriTheme.actionTextColor, false).apply {
            gravity = Gravity.CENTER
        })
        addView(row)
    }

    private fun refreshRouteSummary() {
        if (!::routePrimary.isInitialized || !::routeSecondary.isInitialized) return
        val nodes = runCatching { AndroidNodeStore(this).load() }.getOrDefault(mutableListOf())
        if (nodes.isEmpty()) {
            routePrimary.text = getString(R.string.automatic_route)
            routeSecondary.text = getString(R.string.no_imported_servers)
            return
        }
        val preferences = uiPreferences()
        val selected = preferences.getInt(KEY_SELECTED_NODE, 0).coerceIn(nodes.indices)
        if (selected != preferences.getInt(KEY_SELECTED_NODE, 0)) {
            preferences.edit().putInt(KEY_SELECTED_NODE, selected).apply()
        }
        val smart = AndroidRoutingModePolicy.smartRoutingEnabled(
            preferences.getInt(KEY_ROUTING_MODE, AndroidRoutingModePolicy.SMART),
            preferences.getBoolean(KEY_SMART_ROUTING, true),
        )
        routePrimary.text = if (smart) {
            getString(R.string.automatic_route)
        } else {
            AndroidNodeStore.safeLabel(nodes[selected], selected)
        }
        routeSecondary.text = getString(
            R.string.encrypted_local_pool,
            nodes.size,
            AndroidNodeStore.safeFingerprint(nodes[selected]),
        )
    }

    private fun showServerDialog() {
        val nodes = runCatching { AndroidNodeStore(this).load() }.getOrDefault(mutableListOf())
        val selected = if (nodes.isEmpty()) -1 else uiPreferences()
            .getInt(KEY_SELECTED_NODE, 0)
            .coerceIn(nodes.indices)
        val items = nodes.mapIndexed { index, raw ->
            AndroidNodeStore.safeLabel(raw, index)
        }.toMutableList().apply {
            add("＋ ${getString(R.string.import_vpn_links)}")
            if (nodes.isNotEmpty()) add(getString(R.string.delete_selected_server))
        }
        AlertDialog.Builder(this)
            .setTitle(getString(R.string.choose_server))
            .setSingleChoiceItems(items.toTypedArray(), selected) { dialog, which ->
                when {
                    which < nodes.size -> {
                        uiPreferences().edit().putInt(KEY_SELECTED_NODE, which).apply()
                        dialog.dismiss()
                        refreshRouteSummary()
                    }
                    which == nodes.size -> {
                        dialog.dismiss()
                        showNodeImportDialog()
                    }
                    else -> {
                        dialog.dismiss()
                        deleteSelectedNode()
                    }
                }
            }
            .setNegativeButton(android.R.string.cancel, null)
            .show()
    }

    private fun showNodeImportDialog() {
        val input = EditText(this).apply {
            inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_MULTI_LINE
            minLines = 4
            maxLines = 8
            hint = "vless://…\nvmess://…\ntrojan://…\nss://…"
            setPadding(dp(12), dp(8), dp(12), dp(8))
        }
        val content = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(20), 0, dp(20), 0)
            addView(text(getString(R.string.import_vpn_links_hint), 13f, AmriTheme.detailTextColor, false))
            addView(space(10))
            addView(input, LinearLayout.LayoutParams(-1, -2))
        }
        AlertDialog.Builder(this)
            .setTitle(getString(R.string.import_vpn_links))
            .setView(content)
            .setPositiveButton(getString(R.string.import_action)) { _, _ ->
                runCatching {
                    val store = AndroidNodeStore(this)
                    val nodes = store.importText(input.text.toString())
                    if (nodes.isNotEmpty()) {
                        val selected = uiPreferences().getInt(KEY_SELECTED_NODE, 0).coerceIn(nodes.indices)
                        uiPreferences().edit().putInt(KEY_SELECTED_NODE, selected).apply()
                    }
                }.onFailure {
                    AmriVpnService.STATE.fail()
                }
                refreshRouteSummary()
                refreshState()
            }
            .setNegativeButton(android.R.string.cancel, null)
            .show()
    }

    private fun deleteSelectedNode() {
        val store = AndroidNodeStore(this)
        val nodes = runCatching { store.load() }.getOrDefault(mutableListOf())
        if (nodes.isEmpty()) return
        val selected = uiPreferences().getInt(KEY_SELECTED_NODE, 0).coerceIn(nodes.indices)
        val safeName = AndroidNodeStore.safeLabel(nodes[selected], selected)
        AlertDialog.Builder(this)
            .setTitle(getString(R.string.delete_server_title, safeName))
            .setMessage(getString(R.string.delete_server_message))
            .setPositiveButton(getString(R.string.delete_action)) { _, _ ->
                nodes.removeAt(selected)
                store.save(nodes)
                val replacement = if (nodes.isEmpty()) 0 else selected.coerceAtMost(nodes.lastIndex)
                uiPreferences().edit().putInt(KEY_SELECTED_NODE, replacement).apply()
                refreshRouteSummary()
            }
            .setNegativeButton(android.R.string.cancel, null)
            .show()
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
        val hasNode = runCatching { AndroidNodeStore(this).load().isNotEmpty() }.getOrDefault(false)
        if (!hasNode) {
            showNodeImportDialog()
            return
        }
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
        private const val KEY_SELECTED_NODE = "selected_node"
        private const val KEY_SMART_ROUTING = "smart_routing"
        private val LANGUAGE_TAGS = arrayOf("en", "ru", "es", "pt", "fr", "de", "zh-CN", "hi", "ar")
        private val LANGUAGE_NAMES = arrayOf(
            "English", "Русский", "Español", "Português", "Français", "Deutsch", "简体中文", "हिन्दी", "العربية",
        )
    }
}
