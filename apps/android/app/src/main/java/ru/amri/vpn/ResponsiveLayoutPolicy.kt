package ru.amri.vpn

internal data class ResponsiveLayout(
    val horizontalPaddingDp: Int,
    val headerIconDp: Int,
    val iconButtonDp: Int,
    val powerButtonDp: Int,
    val maxContentWidthDp: Int,
    val showBrandIcon: Boolean,
    val showSubtitle: Boolean,
)

internal object ResponsiveLayoutPolicy {
    fun resolve(widthDp: Int, heightDp: Int, fontScale: Float): ResponsiveLayout {
        val compactWidth = widthDp < 400
        val compactHeight = heightDp < 520
        val largeText = fontScale > 1.30f
        return ResponsiveLayout(
            horizontalPaddingDp = when {
                widthDp < 340 -> 12
                compactWidth -> 16
                widthDp >= 600 -> 28
                else -> AmriTheme.screenPaddingHorizontal
            },
            headerIconDp = if (compactWidth) 44 else 56,
            iconButtonDp = if (compactWidth) 48 else maxOf(48, AmriTheme.iconButtonSize),
            powerButtonDp = when {
                compactHeight -> 96
                compactWidth -> 108
                else -> AmriTheme.powerButtonSize
            },
            maxContentWidthDp = 720,
            showBrandIcon = widthDp >= 340 && !(compactWidth && largeText),
            showSubtitle = widthDp >= 380 && !largeText,
        )
    }
}
