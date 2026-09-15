package ru.amri.vpn

import android.graphics.Color

/** AMRI Android theme tokens. Values are dp/sp inputs unless named as a color. */
internal object AmriTheme {
    val backgroundColor: Int = Color.rgb(3, 8, 18)
    val mutedTextColor: Int = Color.rgb(145, 153, 170)
    val detailTextColor: Int = Color.rgb(165, 173, 190)
    val actionTextColor: Int = Color.rgb(205, 225, 245)
    val cardColor: Int = Color.argb(238, 24, 30, 41)
    val accentColor: Int = Color.rgb(67, 104, 255)
    val inactiveControlColor: Int = Color.rgb(34, 40, 52)

    const val screenPaddingHorizontal = 22
    const val screenPaddingTop = 22
    const val screenPaddingBottom = 28
    const val iconButtonSize = 46
    const val powerButtonSize = 124
    const val cardPadding = 22
    const val cardRadius = 22
}
